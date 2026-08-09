# ADR 0014: Snapshot Source CAS and Durable Replay Admission

- Status: Accepted
- Date: 2026-08-09

Minimal, implementable M3 contract for durable source bytes and a rebuildable derived index. See
ADR 0013 for `SourceArtifactRef`, `ExecutionRecord`, and the `reviewgraphen.review_event.v2` event
contract this ADR extends; both ADRs share one schema generation and are not readable independently.

## 1. Scope and crate DAG

Adds to `reviewgraphen-core`: `SnapshotSourceBundle`/`SnapshotSourceEntry`, three v2-only
`PersistedPayload` kinds (§3), `ValidatedEventView` (§3). Adds a new crate `reviewgraphen-store`: CAS
(§5), locked JSONL writer/reader (§6), SQLite derived index (§7). Crate DAG: `ingest -> core <-
store`, always a DAG — never `store -> ingest` or `ingest -> store`. `reviewgraphen-ingest::ingest()`
is not removed or changed in place; §2 is additive only.

## 2. `ingest_with_sources`

```rust
pub struct IngestWithSourcesResult {
    pub program_space: ProgramSpace,
    pub extraction_report: ExtractionReport,
    pub source_bundle: SnapshotSourceBundle,
}

pub fn ingest_with_sources(
    request: &IngestRequest,
    max_total_source_bytes: u64,
) -> Result<IngestWithSourcesResult, IngestError>;
```

`ingest()` keeps its exact current signature, behavior, and output; `ingest_with_sources()` shares
`ingest()`'s internal pipeline (`git::load_snapshot` then `lift`) rather than reimplementing it, so
the two never diverge for the same request. Bundle entries are built from `GitSnapshot.files[*]
.content` — the same bytes `git::load_snapshot`'s one `git show` per tracked file already read and
`lift()` already fed to `syn`/`file_artifact_draft` — never a second Git read of the same blob. A
tracked file with zero bytes (a real empty file in the tree) is a legal entry: bytes length `0` is
accepted, not rejected as missing content.

`SnapshotSourceBundle` (`reviewgraphen-core`, unchanged shape from the original design):

```rust
pub struct SnapshotSourceBundle {
    snapshot_id: StableId,
    entries: Vec<SnapshotSourceEntry>,   // ordered by path, no duplicate artifact_id
    total_bytes: u64,                    // sum of entries[].bytes.len(), maintained incrementally
}

pub struct SnapshotSourceEntry {
    artifact_id: StableId,      // the file:* artifact this entry's bytes belong to
    path: String,               // repository-relative, matches the artifact's SourceRef
    content_hash: ContentHash,  // current M2 contract: sha256(bytes); never a CAS path key
    cas_hash: ContentHash,      // sha256 of `bytes`; re-validated by CasHash (§5) before any CAS use
    bytes: Vec<u8>,
}
```

Construction validates against the `ProgramSpace` the same call produced, rejecting the whole bundle
on first failure (`IngestError::InvalidSourceBundle`, carrying the offending `artifact_id`):
`cas_hash` equals `sha256(bytes)`; `artifact_id` is unique; `path` matches the entry's `SourceRef`;
and the entry set is exactly the set of `file:*` artifacts `program_space` accepted for this
snapshot — every accepted file artifact has exactly one entry, and no entry names an artifact the
`ProgramSpace` does not contain. Both public functions share one private pipeline;
`ingest_with_sources()` threads `max_total_source_bytes` into `git::load_snapshot`'s existing
per-file read loop, which counts the actual bytes received and returns `SourceBundleTooLarge`
before issuing the next `git show` once the bound is exceeded. `ingest()` supplies no aggregate
budget, preserving its current behavior. No partial bundle is ever returned. This is independent
of `IngestLimits::
max_file_bytes`/`max_files` (§8), which bound ingestion itself, not the bundle's aggregate size.

## 3. Event v2 payload family, authority, and `ValidatedEventView`

Three v2-only, authority-free payload kinds are always tagged
`reviewgraphen.review_event.v2`, never `...v1`. A v1 stream predates CAS-backed sources and may
only be read/imported, never extended with v2 events. The single-tag-per-stream rule (ADR 0013
Invariant 14) rejects a stream mixing the two tags.

- **`RunGenesisManifest`** — always the first event of a v2 run.
  ```rust
  RunGenesisManifest {
      run_id: StableId,
      event_contract_version: String,             // "reviewgraphen.review_event.v2"
      genesis_artifact: ArtifactRegistered,        // applied atomically with this manifest
      repository_identity: String,
      snapshot_id: StableId,
      profile_id: String,
      profile_version: String,
  }
  ```
  `genesis_artifact.cas_hash` must equal the enclosing envelope's own `genesis_hash` field, checked
  as a cross-field rule in `EventEnvelope::validate()` (the same place `actor` is already checked
  against `payload.actor()`), never as two independently-writable copies. `reviewgraphen-store`
  writes `canonical_json(RunGenesisSnapshot)` to CAS, fsyncs it (§5), and only then mints this event.
  The nested registration is inserted into `registered_artifacts` atomically while applying event
  #1, eliminating a forward reference.

  `RunGenesisSnapshot` is a versioned typed DTO, not `ReviewAggregate`'s private serialization:
  ```rust
  RunGenesisSnapshot {
      schema: String,                 // "reviewgraphen.run_genesis.v1"
      program_space: ProgramSpace,
      universe: UniverseDescriptor,
      obligations: Vec<Obligation>,   // canonical StableId order
  }
  ```
  Core alone decodes it with unknown fields denied, reconstructs it through
  `ReviewAggregate::new`, requires pristine state, and checks snapshot/profile values against the
  manifest. It also checks `repository_identity` against the ProgramSpace snapshot provenance/source
  locator through a core accessor; a caller cannot relabel the same genesis bytes as another
  repository. Consumers never deserialize `ReviewAggregate` directly.

  For v2, `genesis_hash` is explicitly `sha256(canonical_json(RunGenesisSnapshot))`, which is also
  the nested registration's `cas_hash`. `EventLog::new_v2` reconstructs the supplied initial
  aggregate from that snapshot, checks canonical equivalence, then uses this hash for its chain and
  admissions. V1 replay alone retains the old
  `sha256(canonical_json(initial ReviewAggregate))` formula, so historical hashes remain valid.
- **`ArtifactRegistered`** — one per **registration**, not one per CAS object: several registrations
  may name the same `cas_hash` under different context.
  ```rust
  ArtifactRegistered {
      run_id: StableId,            // must equal the enclosing EventEnvelope.run_id
      registration_id: StableId,   // kind "registration"; derived from (run_id, cas_hash,
                                    // media_type, sensitivity, source) — never from cas_hash alone
      cas_hash: ContentHash,       // re-validated by CasHash (§5) before any CAS path is touched
      media_type: String,
      size: u64,                   // computed by the store from the actual write, never caller-supplied
      sensitivity: ArtifactSensitivity,  // required; see §8, no silent default
      source: ArtifactSource,      // producing adapter/run
  }
  ```
  `registration_id` is derived from the exact canonical tuple `(run_id, cas_hash, media_type,
  sensitivity, source)`. `ArtifactSource` is a closed, canonically serialized enum so registration
  IDs are stable: `RunGenesis { run_id }`, `SnapshotIngest { run_id, snapshot_id, adapter_id }`, or
  `ReviewerExecution { run_id, execution_id, reviewer_id }`. Every source variant's explicit
  `run_id`, the registration's `run_id`, and the enclosing envelope's `run_id` must be identical.
  This redundancy prevents a valid registration for run B from being replayed or indexed under run
  A; neither `snapshot_id` nor `execution_id` is a substitute for that ownership binding. The nested
  genesis registration has
  `CanonicalState` sensitivity; snapshot bytes use `WorkspaceSource`; raw reviewer bytes use
  `Sensitive`. None has a default.
  This durably replaces a CAS-adjacent metadata sidecar: the CAS blob (§5) carries no metadata
  beyond its own bytes; everything contextual lives here, in the event log.
- **`SnapshotSourcesRecorded`** — one per `SnapshotSourceBundle` commit; entries reference a
  registration, never bytes directly:
  ```rust
  SnapshotSourcesRecorded {
      snapshot_id: StableId,
      entries: Vec<SnapshotSourceRecordEntry>,  // ordered by path
  }
  pub struct SnapshotSourceRecordEntry {
      artifact_id: StableId,
      path: String,
      content_hash: ContentHash,
      registration_id: StableId,   // resolves to a prior ArtifactRegistered (§3a)
      cas_hash: ContentHash,        // equals that registration's CasHash
      line_count: u64,              // count('\n') + 1, including a zero-byte file as one empty line
  }
  ```

### §3a Reference integrity at apply time

`ReviewAggregate` gains `genesis_manifest: Option<RunGenesisManifest>`,
`registered_artifacts: BTreeMap<StableId, ArtifactRegistered>`, and
`snapshot_sources: BTreeMap<StableId, SnapshotSourcesRecorded>`. Applying a second source record for
one snapshot is accepted only when canonically identical; otherwise it is an ID collision. `apply()`
enforces, using the same `DanglingReference` shape already used for evidence `target_ids`:

- The trusted store commit path checks persisted source entries against the in-memory bundle before
  append. During replay, every source entry's registration must already exist; its persisted
  `cas_hash` must equal the registration hash; artifact/path/content hash must equal the genesis
  ProgramSpace file fact; and `line_count` must equal the value computed from bundle bytes before
  append. This persisted bound lets core validate excerpts without reading CAS bytes.
- Every ADR 0013 `SourceArtifactRef` (in a `ContextEnvelopeProjected` event) must match the same
  snapshot's source map on registration ID, artifact ID, content hash, and CAS hash; the source
  registration must be `WorkspaceSource`/`SnapshotIngest`; and its excerpt is within `line_count`.
- Every ADR 0013 `ExecutionRecord.raw_artifact_registration_id` must resolve to an already-applied
  registration whose hash equals `raw_artifact_hash`, sensitivity is `Sensitive`, and source binds
  the same run/execution ID.

A reference that does not resolve is `DomainError::DanglingReference`; the event is rejected in
full, exactly like every other cross-record check in this codebase — never a typed obstruction
recorded as accepted state. (A *missing or corrupt CAS blob on disk*, discovered at store read time
rather than at aggregate-apply time, is a different, store-level failure: it becomes a typed
`CorruptedArtifact`/obstruction at the point of read, §5, never a silent gap.)

### `ValidatedEventView`: the only way `reviewgraphen-store` reads events

```rust
pub struct ValidatedEventView<'a> { /* private fields */ }

impl EventEnvelope {
    pub fn validated_view<'a>(
        run_id: &StableId,
        genesis_hash: &ContentHash,
        events: &'a [EventEnvelope],
    ) -> Result<ValidatedEventView<'a>>;
}

pub struct ValidatedEvent<'a> { /* envelope accessors + one variant below */ }

pub enum DecodedPayload {
    ObligationTransition { obligation_id: StableId, next: ObligationLifecycle },
    ClaimProposed(ReviewClaim),
    EvidenceRecorded(Evidence),
    EvidenceBound(EvidenceBinding),
    VerificationRecorded(Verification),
    DecisionRecorded(Decision),
    FindingRecorded(Finding),
    ReviewPlanRecorded(ReviewPlan),                 // ADR 0013
    ContextEnvelopeProjected(ReviewContextEnvelope), // ADR 0013
    ReviewExecutionRecorded { execution: ExecutionRecord, claims: Vec<ReviewClaim> }, // ADR 0013
    RunGenesisManifest(RunGenesisManifest),
    ArtifactRegistered(ArtifactRegistered),
    SnapshotSourcesRecorded(SnapshotSourcesRecorded),
}
```

`EventEnvelope::validated_view` is the sole constructor and is not `pub` on any store-defined type:
it re-runs the same hash-chain check `validate_sequence` performs over exactly the slice given, then
type-decodes every payload (reusing the private `decode_canonical_payload`) into `DecodedPayload` —
never exposing the private `Value` field. `reviewgraphen-store`'s JSONL reader (§6) determines which
prefix is *confirmed on disk* and passes only that slice in; `core` does not touch the filesystem and
re-validates only the chain, not disk state. For v2 it also requires exactly one
`RunGenesisManifest` at sequence 1 and nowhere else, with manifest run/schema equal to the envelope;
after the store supplies the hash-verified genesis bytes, core checks the decoded snapshot/profile
tuple and canonical hash against the manifest and envelope genesis hash. Nothing in
`reviewgraphen-store` reads raw JSONL lines
for indexing or replay — every consumer goes through this view, which by construction cannot expose
an event past a broken or unconfirmed chain position.

`ValidatedEventView` proves chain and typed shape, not aggregate acceptance. Offline indexing must
additionally pass the exact bound view events in order through core's `OfflineProjectionState`,
seeded from the validated genesis snapshot. Its `apply()` returns `true` only when an authority-free
payload is applied to the accepted aggregate. It returns `false` for an authority-bearing payload
after validating it against the private unreconciled authority shadow, and for the authority-free
`FindingRecorded` after validating its references against that shadow and retaining separate
authority-free projected-finding metadata. Findings are never classified as authority-bearing and
never receive `authority_reconciled`. It enforces all reference, plan/wave, source-registration,
lifecycle, and atomic execution/claim invariants available without authority tokens before exposing
a row; a rehashed or semantically dangling event never becomes an index row.

**Authority-bearing** kinds (need `event.rs`'s admission mechanism to reconstruct *accepted*
state): `EvidenceRecorded` (gated by `EvidenceAdmission`, not `EvidenceBindingAdmission` — that
capability instead gates `EvidenceBound`), `EvidenceBound`, `VerificationRecorded`,
`DecisionRecorded`. **Authority-free** kinds: `ObligationTransition`, `ClaimProposed`,
`FindingRecorded`, `ContextEnvelopeProjected`, `ReviewExecutionRecorded`, `ReviewPlanRecorded`,
`RunGenesisManifest`, `ArtifactRegistered`, `SnapshotSourcesRecorded`. `EventAdmissions` remains a
runtime-only, non-serialized capability type with no constructor outside the trusted-host boundary:
**no v0.1 caller of `store rebuild-index` can ever supply one**, so every rebuild is offline by
construction and every row derived from an authority-bearing event is written with
`authority_reconciled: false`, unconditionally (§7).

`reviewgraphen-store` must reject an empty v2 durable log: a durable v2 run has the mandatory
sequence-one genesis manifest and cannot be represented as an empty stream. This is a Unit C store
admission/recovery requirement, not permission for core to synthesize a missing manifest.

## 4. `StoreRoot` admission and security

`StoreRoot::open` follows one fixed sequence and fails closed at any step rather than degrading:

1. Canonicalize the workspace root path.
2. Path-based open the canonicalized *workspace* directory once, as the trusted anchor; check its
   owner UID/GID against the current process. Mismatch is a hard refusal.
3. `mkdirat(workspace_fd, ".reviewgraphen", 0o700)` — creates the store directory if absent; `EEXIST`
   on an already-present directory is not an error and execution continues to step 4.
4. `openat(workspace_fd, ".reviewgraphen", O_DIRECTORY | O_NOFOLLOW)` to obtain the store root's own
   FD — rejects a symlink planted at that exact name.
5. Check that FD's owner UID/GID and mode (`0700`, no group/other bits) — on both a freshly created
   and a pre-existing directory. Mismatch is a hard refusal, never an automatic `chmod`/`chown`.
6. Every subsequent file/dir operation is FD-relative (`*at`-family, no-follow), rooted at this FD —
   never a fresh path-based open from ambient CWD.

On a platform lacking FD-relative no-follow opens, `StoreRoot::open` returns `UnsupportedPlatform`
and refuses to run degraded; there is no path-based fallback. Every on-disk filename the store
creates is the object's own hash or a fixed literal name — never a caller-supplied string — so no
`SnapshotSourceEntry.path` can participate in a store-relative filesystem path.

The initial store-root implementation is Linux-only and uses the safe `rustix = "=1.1.4"` `fs` and
`process` APIs directly for descriptor-relative operations and current UID/GID checks; the crate
depends on `reviewgraphen-core` only. CAS and journal layers must receive this admitted root rather
than reopen the workspace by path.

The supplied workspace itself must not be a symlink. After canonicalization, the single anchor open
uses Linux `openat2` with `RESOLVE_NO_SYMLINKS`, so a symlink in the canonical absolute traversal is
refused rather than followed. This establishes the anchor before any store-relative operation; the
implementation makes no claim to protect callers that hand it a path after another principal has
already replaced that caller-visible name, so callers must retain their own workspace admission.
An older Linux kernel that reports `ENOSYS` for `openat2` is a typed `UnsupportedPlatform` refusal,
not a fallback to a weaker open path.

## 5. CAS protocol

`CasHash` (`reviewgraphen-store`) is a dedicated, narrower grammar than `ContentHash::parse`'s
existing `sha256|blake3|git`, ≥8-hex acceptance: exactly `sha256:` followed by exactly 64 lowercase
hex characters — no other algorithm, no short form, no uppercase. `CasHash::parse` is the only way
an arbitrary string becomes usable as a CAS path component; every `ContentHash`-typed CAS-key field
(ADR 0013's `SourceArtifactRef.cas_hash`/`ExecutionRecord.raw_artifact_hash`, this ADR's
`SnapshotSourceEntry.cas_hash`/`ArtifactRegistered.cas_hash`) is re-validated through it before
`reviewgraphen-store` touches a path; a syntactically valid but non-CAS `ContentHash` is rejected
before any filesystem operation.

Layout: `artifacts/sha256/<2-hex>/<64-hex>` for objects, `artifacts/tmp/<random>` for write staging.
Directories `0700`, objects `0600`, owner-only.

**Write**: reject up front if a declared length exceeds `max_object_bytes` (§8). Create the temp file
with `O_CREAT|O_EXCL` (collision is a hard error) and immediately acquire a non-blocking exclusive
lease lock on it, held until rename or abort. Stream bytes to disk while incrementally hashing in the
same pass; the bound is enforced against the **actual** running byte count as written, not the
declared length — the write aborts mid-stream the instant actual bytes exceed `max_object_bytes`,
even if the declared length under-reported it. `fsync` the temp file. Publish is atomic, create-only,
and never a fallback race: use a true atomic create-only rename primitive (`RENAME_NOREPLACE`-class
`rename`, or a `link()`-then-unlink-temp sequence, whichever the host offers) into `sha256/<hash>`;
if the host offers **neither**, the write fails closed (`UnsupportedPlatform`) — a "check the
destination doesn't exist, then rename" sequence is never constructed, at any bound, because it
already contradicts the no-replace invariant it would exist to guarantee. After a successful
create-only rename, `fsync` the parent directory. If the destination already exists, the store does
not skip the write as a no-op: it rehashes the existing object (bounding the read the same way as
below) to confirm it truly matches before discarding the new temp; a mismatch is `CorruptedArtifact`
on the *existing* object, never silently overwritten. Release the lease lock on every exit path.

**Read**: recompute `sha256` over the bytes actually read, bounding the read at `max_object_bytes +
1` so an oversized or corrupt object is rejected without buffering it whole; a hash mismatch or a
missing object is `CorruptedArtifact`/`MissingArtifact` — a typed obstruction, never returned as
valid, on every read, not only an offline scan. Every hash string is `CasHash`-validated before it
touches a path-join, and every path component opened is confirmed regular via no-follow open (§4) —
a symlink where an object is expected is `CorruptedArtifact`, never followed.

**Temp GC** (`store gc-tmp`): for each entry under `artifacts/tmp/`, open with `O_NOFOLLOW` and skip
anything that is not a regular file, then attempt a non-blocking exclusive lease-lock on it — a
failed acquisition means a live writer still holds it, so the entry is skipped regardless of age.
Only a lease-locked entry is deleted; an age filter may apply as an additional courtesy, but
correctness rests on the lock, never on age alone.

## 6. Durable JSONL protocol

Readers take a shared advisory lock (`LOCK_SH`); each append/recovery transaction takes an exclusive
lock (`LOCK_EX`) until its durable commit or rollback completes. Any number of readers may hold the shared lock together; the writer blocks
until every reader releases, and readers block while the writer holds its exclusive lock — this is
standard flock coordination, not the independent/uncoordinated model an earlier draft of this ADR
used. On open for writing, the writer validates the **entire** chain from genesis under its lock:
every line parses as JSON and `previous_event_hash`/`event_hash` links it to the one before it, up
to `max_replay_bytes`/`max_events` (§8; exceeding either is a typed `Incomplete` failure, not a
truncated success). Three distinct outcomes at the tail:

1. **Unterminated final fragment** — the last line is physically incomplete (no trailing `\n`, or
   otherwise syntactically truncated). This is the only case `store recover` auto-repairs: it is a
   `CorruptNeedsRecovery { good_offset }` refusal to open for writing.
2. **Complete but invalid last line** — newline-terminated, parses as JSON, but fails the hash chain
   or shape validation. This is *not* a torn tail and is never auto-truncated: same
   `CorruptNeedsRecovery`, but manual only, like case 3.
3. **Invalid interior line** — any non-last line fails to parse or breaks the chain.
   `CorruptNeedsRecovery`; `store recover` reports the offending offset and never truncates.

**Recovery** (`store recover`, exclusive lock, only for case 1) uses one `recovery_id` and atomic,
create-only files under `recovery/intents/` and `recovery/completions/`, not another append log that
could itself acquire a torn tail:

1. Compute `good_offset` and `discarded_hash` (sha256 over exactly the bytes from `good_offset` to
   EOF).
2. Write an **intent** record — recovery ID, `good_offset`, `discarded_hash`, pre-tail hash, actor,
   tool version, timestamp — through temp+fsync+atomic-create-only-publish+parent-fsync.
3. Truncate the main log to `good_offset`, then `fsync`/`sync_data` the truncated file.
4. Publish a completion record (same recovery ID plus post-tail hash) with the same atomic protocol.

A crash between steps resumes deterministically on next open: an intent record with no matching
completion, and the main log already at `good_offset`, means only step 4 is missing; otherwise step
3 is redone before step 4. Invalid/torn intent or completion files are corruption, never ignored;
case 2/3 never gains an auto-repair path, including via `store recover`.

**Append** (writer, lock held): build the canonical-JSON line plus `\n` in memory, bounded by
`max_event_line_bytes` (§8); `write_all`/`flush`/`sync_data` at the current confirmed EOF. Only
after `sync_data` succeeds does the writer advance its confirmed-tail state; on failure it truncates
back to the pre-write offset and `sync_data`s the rollback, or transitions to
`CorruptNeedsRecovery` if either operation fails.

## 7. SQLite derived index

```toml
rusqlite = { version = "=0.40.1", features = ["bundled"] }
```

The exact pin is deliberate: `bundled` removes host-SQLite drift, and `=0.40.1` (not `^0.40.1`)
removes patch-version drift too.

Rebuild acquires a dedicated rebuild lock, distinct from the JSONL locks in §6 (it reads the log
under a shared lock as an ordinary reader, and separately holds this lock so two concurrent
`store rebuild-index` invocations serialize instead of racing the same temp-file-and-swap sequence).
It always targets a fresh temporary file, never the live `index.sqlite` in place, with
`PRAGMA journal_mode = DELETE` (never WAL/TRUNCATE/MEMORY — DELETE leaves no `-wal`/`-shm` sidecar
that would complicate an atomic single-file swap): create the full schema, set `PRAGMA user_version`,
write a policy/config row, a genesis marker row, and a tail marker row. For v2, rebuild reads event
#1's genesis registration from CAS under the log's shared lock, re-verifies its hash, asks core to
decode/validate `RunGenesisSnapshot`, and seeds objects, relations, obligations, universe, and other
baseline tables from that typed view. It then projects event tables from
`ValidatedEventView` plus `OfflineProjectionState` (docs/15 §7's table list, plus `context_envelopes`, `review_plans`,
`artifact_registrations`, and `snapshot_source_index`), bounded by `max_index_rows`/
`max_index_temp_bytes` (§8, counting database plus journal and every rebuild temp file; exceeding
either is a typed `Incomplete` failure). This is a direct
**typed metadata projection**, not authority-bearing aggregate replay: for each `ValidatedEvent`, rebuild pattern-
matches its already-decoded `DecodedPayload` and writes exactly the columns each table needs (IDs,
references, sequence, hashes) — it never calls `ReviewAggregate::apply`/`validate`, which require a
live `EventAdmissions` capability `reviewgraphen-store` structurally cannot mint (§3). Rows derived
from an authority-bearing kind therefore always carry `authority_reconciled: false`. Rows are not
inserted until `OfflineProjectionState` has reconciled every non-authority semantic invariant; the
two flags are independent. A v1 stream has no genesis artifact, so v0.1 supports event-metadata-only
indexing for it; a full domain index requires importing it into a fresh v2 run.

Run `PRAGMA integrity_check`; only on `ok` does the store `COMMIT`, close the connection, and
`fsync` the resulting file. `index.sqlite`, unlike a CAS object, is disposable and **may** be
replaced: the swap uses the platform's atomic rename-into-place primitive, then `fsync`s the parent
directory; no atomic rename at all fails closed (`UnsupportedPlatform`), no copy-then-delete
fallback. On every open, the store verifies the file's mode (`0600`), `PRAGMA user_version`/expected
schema, and a well-formed tail-marker row; any mismatch is typed failure requiring rebuild. Every
query lists its columns explicitly with an explicit `ORDER BY` — no `SELECT *`.

`CommittedIndexStale` is typed: any query requiring guaranteed-current state compares the index's
tail marker against the log's actual current tail while holding the log shared lock through query
completion; a mismatch returns `CommittedIndexStale` rather than serving stale rows. Two independent rebuilds from the same log are compared by canonical
**`IndexSnapshot` query-state equality** — a fixed set of explicit, ordered queries whose typed
results must match — never by raw `.sqlite` bytes, since SQLite gives no such byte-identity
guarantee across rebuilds (free-list layout and page ordering are unspecified).

## 8. Bounds, sensitivity, and permissions

`IngestLimits::max_file_bytes`/`max_files` bound ingestion; `max_total_source_bytes` (§2)
independently bounds `SnapshotSourceBundle`'s aggregate size; `max_object_bytes` (§5) independently
bounds any single CAS write. `reviewgraphen-store` additionally enforces `max_event_line_bytes`
(one JSONL line), `max_events` (replay/rebuild event count), `max_replay_bytes` (bytes read while
validating/replaying a chain), `max_index_rows` (rows per rebuilt table), and `max_index_temp_bytes`
(rebuild temp-file size) — §6/§7. Exceeding any of these is always a typed `Incomplete`-shaped
failure: `store rebuild-index`/`store verify`/log open never complete as a truncated silent success
(docs/16 §11: "limit到達をpass扱いしません").

`ArtifactSensitivity` (`CanonicalState | WorkspaceSource | Sensitive`) is a **required** field on
`ArtifactRegistered` with no default: genesis snapshots are `CanonicalState`, source bundle bytes
are `WorkspaceSource`, and reviewer/model responses are `Sensitive`. No redaction pipeline is implemented — the field is reserved for future policy, matching
`docs/16` §7's deferred posture, but it is never silently defaulted or omitted. All CAS/JSONL/SQLite
and recovery paths are `0700`(dirs)/`0600`(files), owner-only, via `StoreRoot` (§4); every existing
child is checked on no-follow open and a looser mode is a hard failure.

## 9. Reports excluded

`store rebuild-index` never reads `reports/`; reports remain projections, never a replay input.
Unchanged from ADR 0007/`docs/15` §17.

## Consequences

**Positive**: source bytes are retrievable by hash without re-opening the workspace; every CAS read
self-verifies; `CasHash` closes off non-CAS `ContentHash` values before any path is built; the
registration-vs-blob split represents multiple contextual registrations of one blob without
duplicating bytes; the recovery WAL survives a crash mid-recovery; "typed metadata projection, not
aggregate replay" gives an honest, checkable answer to what offline rebuild can reconstruct.

**Negative**: a new crate and dependency; every CAS read pays a full rehash; readers now coordinate
with the writer's lock, adding contention; recovery, temp GC, and SQLite rebuild each add their own
lock; a complete-but-invalid or interior-line tail always requires manual operator recovery.

## Alternatives considered

- Re-read the workspace instead of a bundle+CAS — rejected; reintroduces the staleness/availability
  failure ADR 0004 warns against.
- A CAS-adjacent metadata sidecar file — rejected (§3); makes blob identity ambiguous.
- Let a caller supply `EventAdmissions` to `store rebuild-index` — rejected for v0.1; no persistence
  format for it exists.
- Auto-repair any torn/invalid JSONL tail on open — rejected (§6); only a physically-unterminated
  fragment is repaired automatically.
- Age-threshold-only temp GC — rejected (§5); age cannot distinguish an abandoned write from a slow
  live one, which a lease lock resolves directly.
- Check-then-rename as an accepted narrow race for CAS publish — rejected (§5); contradicts the
  no-replace invariant it would exist to guarantee, so an unsupported platform fails closed instead.
- Compare rebuilt SQLite files byte-for-byte — rejected (§7); no such guarantee exists.

## Invariants

1. A CAS object at `sha256/<hash>` always has bytes whose SHA-256 equals `<hash>`, checked on every
   read and re-checked against any pre-existing object before a write is treated as a duplicate.
2. No CAS write is visible at its final path until complete; no CAS write ever replaces an existing
   object; no platform lacking an atomic create-only primitive performs a check-then-rename instead.
3. No CAS blob carries metadata beyond its own bytes; `sha256`+`size` are its only intrinsic
   identity. Contextual metadata lives only in one or more `ArtifactRegistered` registrations.
4. A JSONL append that fails before `sync_data` succeeds leaves the file at its pre-write offset or
   transitions the store to `CorruptNeedsRecovery`.
5. Only an unterminated final fragment is auto-repaired; a complete-but-invalid last line or an
   invalid interior line is always manual, via a receipted `store recover`.
6. A `store recover` run is resumable: a crash between its intent and completion receipts is
   detected on next open and finished deterministically, never re-applied from scratch.
7. A v2 `index.sqlite` can be deleted and rebuilt from the validated genesis snapshot plus
   `ValidatedEventView`; rebuilds compare `IndexSnapshot` query state, never file bytes. A v1 stream
   supports event-metadata-only rebuild until imported into v2.
8. `authority_reconciled` is `false` for every row derived from `EvidenceRecorded`, `EvidenceBound`,
   `VerificationRecorded`, or `DecisionRecorded` in every v0.1 rebuild, unconditionally.
9. `reviewgraphen-store` never mints `EventAdmissions` or any `*Admission` type, and never depends on
   `reviewgraphen-ingest`, nor vice versa.
10. `SnapshotSourceEntry.cas_hash`/`ArtifactRegistered.cas_hash` are always `CasHash`-valid and equal
    `sha256(bytes)`; `content_hash` never substitutes for `cas_hash` as a CAS key.
11. Every `SnapshotSourceRecordEntry.registration_id`, `SourceArtifactRef.registration_id`, and
    `ExecutionRecord.raw_artifact_registration_id` resolves within the aggregate's own `registered_artifacts`/
    `SnapshotSourcesRecorded` history at apply time, or the event is rejected in full (§3a).
12. `RunGenesisManifest.genesis_artifact.cas_hash` equals its envelope's v2 `genesis_hash`, and its
    decoded repository/snapshot/profile/run/schema fields equal the manifest/envelope.
13. `ArtifactRegistered.sensitivity` is always present: `CanonicalState` for genesis,
    `WorkspaceSource` for source bytes, or `Sensitive` for raw reviewer bytes; never defaulted.
14. Exceeding any §8 bound is always a typed `Incomplete` failure, never a truncated success.
15. A reader holds a shared lock and each append/recovery transaction an exclusive lock on the same
    JSONL file; the two never proceed concurrently.
16. A CAS temp file is deleted by `store gc-tmp` only after that exact file's lease lock is acquired,
    and only if it is a regular, non-symlinked file.

## Versioning and migration

`ingest()` is unchanged; `ingest_with_sources()` is new and additive. `RunGenesisManifest`,
`ArtifactRegistered`, and `SnapshotSourcesRecorded` are `reviewgraphen.review_event.v2`-only payload
kinds (ADR 0013 §2) — there is no v1 form of any of them, and no automatic upcast mints them onto an
imported v1 stream; a v1 run needing durable CAS-backed sources starts a fresh v2 run, per ADR 0013's
own migration rule. `index.sqlite` carries no versioning contract beyond `PRAGMA user_version`: an
old index is deleted and rebuilt, never migrated in place. A future incompatible CAS-layout or JSONL-
format change needs its own migration ADR (`docs/15` §13/§15); none is introduced here. A future live
trusted-host reconciliation pass that lets `authority_reconciled` become `true` outside a fresh
rebuild is out of scope and must be its own ADR.

## Negative tests

1. A CAS read over corrupted on-disk bytes is rejected as `CorruptedArtifact`, never valid.
2. A CAS write whose declared `cas_hash` mismatches the actual SHA-256 of its bytes is rejected.
3. A write to an existing `cas_hash` whose on-disk object has since corrupted surfaces
   `CorruptedArtifact` and never silently replaces it.
4. A simulated crash between temp-write and rename leaves no object at the final path on restart.
5. A `CasHash` string with `..`, `/`, a non-hex character, wrong length, or a non-`sha256` prefix is
   rejected before any path is constructed; a symlink at an object's expected path is rejected, never
   followed.
6. `SnapshotSourceBundle` construction exceeding `max_total_source_bytes` fails with
   `SourceBundleTooLarge` and returns no partial bundle; a bundle omitting or adding an artifact ID
   relative to the same call's `ProgramSpace` is rejected.
7. A zero-byte tracked file produces a valid, accepted `SnapshotSourceEntry`.
8. An unterminated final JSONL fragment is `CorruptNeedsRecovery`; `store recover` truncates to
   `good_offset` and writes intent/completion receipts with `discarded_hash`/actor/tool version,
   resuming correctly from a simulated crash between the two receipts; the main log is untouched
   until that call.
9. A complete, chain-breaking last line and an invalid interior line are both `CorruptNeedsRecovery`
   and are never truncated by any code path, including `store recover`.
10. Two concurrent readers may hold the shared lock together; a writer blocks until both release, and
    a reader blocks while the writer holds its exclusive lock; no interleaved line ever appears.
11. `ArtifactRegistered` events sharing one `cas_hash` under two different `registration_id`s are both
    indexed; the underlying CAS object is written once.
12. A `SnapshotSourceRecordEntry` whose `registration_id` does not resolve to a prior
    `ArtifactRegistered`, or whose `cas_hash` mismatches, is rejected as `DanglingReference`; so is a
    `ReviewExecutionRecorded` whose raw registration ID/hash/sensitivity/source tuple mismatches.
13. `store rebuild-index` over a log with only authority-free events reconstructs `index.sqlite`
    with an `IndexSnapshot` matching a pre-deletion snapshot, including `review_plans`; over a log
    containing authority-bearing events it indexes existence/sequence/referenced-ID metadata but
    marks every dependent row `authority_reconciled: false` and passes non-authority semantics
    through `OfflineProjectionState`.
14. A query against a stale index tail returns `CommittedIndexStale`; the shared log lock held from
    tail comparison through query completion closes the append TOCTOU.
15. `store gc-tmp` skips a temp file whose lease lock cannot be acquired and any non-regular or
    symlinked entry; only lease-locked regular temp files are deleted.
16. Exceeding any bound in §8 (`max_object_bytes` through `max_index_temp_bytes`) fails the
    operation with a typed `Incomplete` error, never a partial success.
17. `StoreRoot::open` refuses a workspace or store directory with a mismatched owner or mode, and
    never falls back to a path-based open when FD-relative no-follow opens are unsupported.
18. Missing/duplicate/non-first genesis manifests, or a manifest whose run/schema/repository/
    snapshot/profile/canonical hash disagrees with its envelope and decoded genesis snapshot, are rejected.
19. A rehashed but semantically dangling event is rejected by `OfflineProjectionState` and produces
    no accepted domain index row.

## Crate ownership and exit criteria

| Crate | New responsibility |
| --- | --- |
| `reviewgraphen-core` | `SnapshotSourceBundle`/`SnapshotSourceEntry`, `RunGenesisSnapshot`, the three v2 payload kinds, `ValidatedEventView`/`DecodedPayload`/`OfflineProjectionState`, genesis/registration/snapshot-source aggregate maps and §3a checks. No filesystem or CAS access. |
| `reviewgraphen-ingest` | `ingest()` unchanged; `ingest_with_sources()` populates the bundle from the same Git read already performed and cross-checks it against `ProgramSpace`. No store I/O. |
| `reviewgraphen-store` (new) | `CasHash`, CAS read/write/verify/gc-tmp, locked JSONL append/validate/recover with receipted WAL recovery, `StoreRoot` admission, SQLite rebuild/query via `rusqlite = "=0.40.1"` bundled. Depends on `reviewgraphen-core` only. |

Exit criteria (M2/M3 readiness):

1. Every invariant and negative test above passes.
2. `ingest_with_sources()`'s bundle entries verify against the bytes and `ProgramSpace` the same call
   already produced — no second Git read, no bundle/`ProgramSpace` drift.
3. `reviewgraphen-store` persists a `SnapshotSourceBundle` and an `ExecutionRecord.raw_artifact_hash`
   through the same CAS write and `ArtifactRegistered` registration path, with no type-specific case.
4. `store rebuild-index` round-trips (delete, rebuild, `IndexSnapshot`-compare) against a fixture log
   containing every payload kind ADR 0013 and this ADR define, including `review_plans`.
5. `store recover` is exercised against an unterminated-fragment fixture (auto-recovered, resumable
   across a simulated mid-recovery crash) and a complete-but-invalid/interior-line fixture (refused).
6. Neither `reviewgraphen-store` nor `reviewgraphen-ingest` depends on the other.
