# ADR 0015: FD-Relative Serialized SQLite Derived Index

- Status: Accepted
- Date: 2026-08-09
- Scope: the SQLite implementation required by ADR 0014 §7, including its
  dependency features, in-memory build, serialized-file publication, read-only
  query reconstruction, locks, bounds, schema guards, and restart behavior.
  This ADR does not change the CAS or JSONL canonical-state contracts and does
  not permit any filesystem pathname to reach SQLite.

> D1 schema version 2, its exact plan/context columns and hash preimages, and
> the version-1 rebuild boundary are closed by
> [ADR 0017](0017-d1-derived-index-schema-v2.md). This ADR remains normative
> for the descriptor-relative SQLite, locking, bounds, and publication
> boundary.

## Context

ADR 0007 makes the append-only event stream canonical and SQLite a disposable,
rebuildable query projection. ADR 0014 requires a V2 index to be rebuilt from
hash-verified genesis bytes, a confirmed JSONL prefix, and core's
`ValidatedEventView` plus `OfflineProjectionState`. An offline store cannot
mint `EventAdmissions`, and SQLite must never become an alternative authority
source.

`StoreRoot` deliberately permits only descriptor-relative, no-follow access
after its one trusted workspace anchor. `rusqlite` can open a database by
filesystem pathname, but a path-based connection would reopen the ambient
namespace and contradict ADR 0014 §4. `/proc/self/fd/<dirfd>/<name>` still
passes a filesystem pathname to SQLite and permits its VFS to derive sidecar
and temporary paths. It therefore does not close the boundary sufficiently.
A custom VFS would add a broad unsafe FFI surface to a workspace that forbids
unsafe code.

Rusqlite 0.40.1's `serialize` feature exposes SQLite's serialize/deserialize
API. That permits all SQL work to occur in an in-memory connection while the
store alone performs bounded, FD-relative file I/O. This ADR selects that
boundary and rejects every SQLite filesystem-open alternative.

## Decision

### 1. Exact dependency and trust boundary

`reviewgraphen-store` uses exactly:

```toml
rusqlite = { version = "=0.40.1", default-features = false, features = ["bundled", "serialize", "limits"] }
```

`bundled` removes host SQLite-library drift, the exact version pin removes
patch drift, `serialize` is the only persistence bridge, and `limits` exposes
the per-connection SQLite limits this ADR requires. Disabling default features
prevents an unreviewed feature from silently widening the SQLite surface. The
crate remains Linux-only, depends on `reviewgraphen-core` only, and never
constructs `EventAdmissions` or another `*Admission` capability.

No call to `Connection::open`, `open_with_flags`,
`open_with_flags_and_vfs`, ATTACH, VACUUM INTO, URI filenames, `/proc/self/fd`,
or a custom VFS is permitted. SQLite receives no filesystem pathname at all.
The store's safe `rustix` code is the sole filesystem boundary for index
bytes. SQLite extensions remain disabled and no SQL statement may enable or
load one.

### 2. Layout and descriptor admission

The store creates or opens `indexes/` from the admitted store-root FD using
one-component `mkdirat`/`openat`, `O_DIRECTORY | O_NOFOLLOW`, owner equality,
and exact `0700` mode. All entries have store-owned names:

```text
indexes/
  index.lock                         fixed, regular, 0600
  reviewgraphen.sqlite               fixed active image, regular, 0600
  candidate-<full-random-hex>.sqlite disposable publication candidate, 0600
```

Run IDs, snapshot IDs, source paths, profile text, and caller input are never
filesystem components. Candidate names use a full OS-CSPRNG nonce. A candidate
is created anonymously with `O_TMPFILE`, checked as owner-only regular `0600`,
written and synced, then linked under its create-only random name with
`linkat(AT_EMPTY_PATH)`. There is no check-then-create path.

Existing active, lock, and candidate entries are inspected with
`statat(..., SYMLINK_NOFOLLOW)`, opened with `O_NOFOLLOW | O_NONBLOCK`, and
matched to `fstat` by type, owner, mode, device, inode, and nonnegative size.
Symlinks, FIFOs, devices, directories, loose modes, or identity changes are
typed corruption/race failures.

### 3. Public API and lock ownership

The public surface accepts an `EventJournal`, not a caller-created
`JournalReader` or `JournalWriter`:

```rust
pub struct DerivedIndex<'a> { /* admitted StoreRoot/indexes descriptors */ }

pub struct IndexRebuildReceipt {
    pub run_id: StableId,
    pub genesis_hash: ContentHash,
    pub confirmed_offset: u64,
    pub tail_hash: ContentHash,
    pub event_count: u64,
    pub serialized_bytes: u64,
    pub image_hash: ContentHash,
}

pub enum IndexError {
    Missing,
    CorruptIndex,
    IndexPathRace,
    ProjectionContractViolation,
    DuplicateIndexKey,
    PublicationDurabilityUncertain {
        run_id: StableId,
        tail_hash: ContentHash,
        image_hash: ContentHash,
    },
    CommittedIndexStale {
        indexed_offset: u64,
        indexed_tail: ContentHash,
        committed_offset: u64,
        committed_tail: ContentHash,
    },
    Incomplete { limit: u64, observed: u64 },
    IntegerOutOfRange,
    UnsupportedPlatform,
    // typed Store, Journal, Core, and sanitized SQLite wrappers
}

impl<'a> DerivedIndex<'a> {
    pub fn open(root: &'a StoreRoot) -> Result<Self, IndexError>;
    pub fn rebuild(
        &self,
        journal: &EventJournal<'_>,
        cas: &CasStore<'_>,
    ) -> Result<IndexRebuildReceipt, IndexError>;
    pub fn snapshot_current(
        &self,
        journal: &EventJournal<'_>,
    ) -> Result<IndexSnapshot, IndexError>;
}
```

`DerivedIndex::open` admits and retains directory descriptors only. It does
not acquire or retain an operation flock, open a journal, read an active DB,
or imply that an index is current. Each operation acquires and releases its
own locks.

Callers must not hold a journal reader, writer, or recovery operation across
`rebuild` or `snapshot_current`. The API intentionally does not accept such a
guard: otherwise a caller could invert the mandatory order and deadlock. The
Rust type system cannot prove that an independently acquired guard is absent,
and `flock` is blocking, so this is an explicit caller precondition rather
than a best-effort runtime diagnostic. Callers must `drop` a writer/reader
before calling either index operation.
operation lock order is always:

```text
index.lock (shared query / exclusive rebuild) -> journal shared lock -> work
```

Locks are released in reverse order. Append/recovery uses only the journal's
existing exclusive lock and never takes `index.lock`.

### 4. Lock-file identity and threat model

`index.lock` is a fixed, owner-only regular file. Before flock, the store
records its no-follow directory-entry stat, opens it descriptor-relatively,
and verifies the opened FD has the same device/inode. Immediately after flock
and again immediately before returning, it repeats `statat` and requires the
fixed directory entry to still name that same inode. Replacement at either
seam is `IndexPathRace`, and the operation returns no successful result.

These checks plus a trusted owner-only `0700` directory protect cooperating
ReviewGraphen actors and detect tested replacement attempts. They do not claim
that `flock` prevents a malicious same-UID process from renaming the locked
inode and arranging a perfectly timed split lock between checks. Same-UID
adversarial namespace control is outside the StoreRoot threat model. Nor do
inode/owner/mode checks detect a malicious same-UID process writing forged but
structurally valid SQLite bytes in place through an already-open FD, including
bytes with a forged current marker. The index has no signature/MAC and must not
be treated as protection from that actor; canonical events remain the recovery
authority. Three-actor and injected split-lock tests remain mandatory so
accidental lock-entry replacement cannot silently become supported behavior.

### 5. Fixed resource limits and arithmetic

`StoreLimits` supplies these nonzero index bounds:

```text
max_index_rows                 aggregate rows across every table
max_index_serialized_bytes     main DB image and active-file byte limit
max_index_working_bytes        store-owned simultaneous image/read/result bytes
max_index_query_bytes          total returned IndexSnapshot payload bytes
max_index_statement_bytes      UTF-8 bytes in any one SQL statement
```

`max_index_rows` is one aggregate counter, not a per-table allowance. It
counts every successful row insertion, including `index_meta`, `events`,
event-derived rows, and baseline tables. This schema uses no junction tables:
ordered ID collections are stored in named canonical-encoded columns (§12).
The counter reserves one row before each insert. No table can independently
consume another full limit.

Every addition, multiplication, offset advance, page calculation, allocation
size, and row counter uses checked arithmetic. Every `u64`/`usize` written to
SQLite INTEGER is converted with `i64::try_from`; every INTEGER read back is
rejected if negative or not representable in its typed destination. Overflow
or lossy conversion is `IntegerOutOfRange`/typed `Incomplete`, never wrapping,
saturation, or an SQL cast.

The main database uses a fixed page size of 4096 bytes. Before any DDL, rebuild
sets and reads back:

```text
PRAGMA page_size = 4096
PRAGMA temp_store = MEMORY
PRAGMA foreign_keys = ON
PRAGMA journal_mode = MEMORY
PRAGMA locking_mode = EXCLUSIVE
PRAGMA trusted_schema = OFF
PRAGMA user_version = 1
```

`user_version` is set to exactly 1 and read back before DDL. It is read back
again from the candidate reread and from every query deserialize, and must
equal `index_meta.index_schema_version`. Any other value is an incompatible
derived schema requiring rebuild, never an in-place migration.

`max_page_count` is `floor(max_index_serialized_bytes / 4096)`, must be at
least one, is checked into SQLite's accepted integer range, and is set/read
back through `PRAGMA max_page_count`. Page count multiplied by page size may
never exceed `max_index_serialized_bytes`. The connection's SQL-length,
value-length, column-count, compound-select, expression-depth, and variable-
number SQLite limits are set to fixed implementation constants no larger than
the corresponding store bounds. Each static DDL/DML/query statement is
checked against `max_index_statement_bytes` before prepare; dynamically
constructed SQL is forbidden.

`max_index_working_bytes` is enforced on explicit ownership stages, before
allocation and again as buffers become materialized. During serialize, the
build connection's main image/cache, SQLite's serialized view, and the owned
publication `Vec` are charged as `3 * image + configured build cache`. The
connection is dropped before publication takes ownership of that `Vec`.
During deserialize, the owned read `Vec`, reconstructed main image, and query
cache are charged together; `deserialize_read_exact` consumes the `Vec`, which
is dropped before marker/table queries materialize an `IndexSnapshot`.
Candidate validation and current-query admission then charge the main image,
configured cache, and preflight result without retaining a second input buffer.
SQLite allocator/header overhead is not represented as persisted bytes; page
count, cache configuration, SQLite limits, statement bounds, and returned-data
bounds independently constrain its inputs. `PRAGMA cache_size` is set to a
checked negative-KiB value derived from the remaining working budget: rebuild
reserves three maximum serialized images before assigning cache; query/candidate
deserialize reserves two maximum images plus the complete query budget. Query
admission checks the actual peak `main DB + configured cache + preflight
result`. `temp_store =
MEMORY` is mandatory, so a sort or temporary table cannot create an ambient
file. Exceeding any bound is a typed incomplete operation, never a truncated
successful index or query.

For schema-version-2 current queries, the lock-held canonical journal view is
store-owned working state, not an input exempt from this budget. ADR 0017 §6
defines its raw, decoded, validation-scratch, and comparison charges and the
stage peaks in which they coexist with the deserialized main image, configured
cache, and complete-result reservation. The query cache reservation is reduced
by that retained journal charge; `max_replay_bytes` does not enlarge or replace
`max_index_working_bytes`.

### 6. In-memory rebuild

After acquiring exclusive index lock then journal shared lock, rebuild:

1. obtains exactly the complete confirmed event slice, byte offset, and tail
   hash from `JournalReader::with_locked_snapshot`;
2. constructs one complete `ValidatedEventView` over that same confirmed
   slice: V1 uses the identity's V1 genesis hash; V2 first reads and rehashes
   the manifest's genesis object through `CasStore`, strictly decodes the
   exact `RunGenesisSnapshot` bytes, then supplies those bytes to core;
3. opens only an in-memory SQLite connection with
   `SQLITE_OPEN_READ_WRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_NO_MUTEX`,
   applies the fixed pragmas/limits, creates the fixed schema, and starts one
   explicit transaction;
4. follows exactly one of the version branches below;
5. writes the singleton marker including the exact event count; and
6. runs all validation before commit and serialization.

For V1, `ValidatedEventView` validates only the homogeneous V1 chain and the
closed payload shape. Rebuild writes the validated envelope fields to `events`
and writes no baseline/domain/shadow/finding row. It does **not** construct or
call `OfflineProjectionState`; V1 has neither verified V2 genesis bytes nor the
execution/source closure required for an honest domain projection. A full
domain index requires import into a fresh V2 run.

For V2, rebuild seeds the typed ProgramSpace/universe/obligation baseline from
the already verified genesis snapshot. It constructs exactly one
`OfflineProjectionState` from that validated V2 view, applies every view event
exactly once and in sequence under §7, then requires
`OfflineProjectionState::is_complete()` to be true and its `tail_hash()` to
equal the reader's locked tail hash. A false completion check, tail mismatch,
skipped event, repeated apply, or apply past the view is a hard projection
failure and publishes nothing.

Reports and raw JSONL payload inspection are never rebuild inputs.

### 7. Projection branching contract

This section applies only to the V2 branch. For each V2 `ValidatedEvent`,
rebuild first records only its envelope metadata in `events`, then calls
`OfflineProjectionState::apply` exactly once and branches on both its boolean
and the closed `DecodedPayload` variant:

- `true` is valid only for authority-free domain variants other than
  `FindingRecorded`: genesis, artifact/source records, plan/context/execution
  records, obligation transitions, and claims. Their domain rows are inserted
  only after successful apply.
- `false` with `EvidenceRecorded`, `EvidenceBound`,
  `VerificationRecorded`, or `DecisionRecorded` writes shadow metadata only
  with `authority_reconciled = 0`.
- `false` with `FindingRecorded` writes only the separate projected-finding
  metadata row with `projection_status = 'shadow_only'`.
- every other `(bool, payload kind)` pair is
  `ProjectionContractViolation` and aborts the transaction/rebuild.

This match is exhaustive. A newly added payload cannot fall through to a
generic row, and a future change to core's boolean classification cannot
silently promote or discard it.

### 8. DDL guards and separated authority

The schema uses strict tables, explicit NOT NULL columns, primary/unique keys,
closed-enum CHECK constraints, and foreign keys. At minimum it contains:

```text
index_meta
events
program_objects
program_relations
universe
obligations
obligation_lifecycle
claims
artifact_registrations
snapshot_source_index
context_envelopes
review_plans
executions
unreconciled_authority_records
projected_findings
```

Normative guards include:

- `index_meta` has exactly one row (`singleton = 1 CHECK (singleton = 1)`),
  index/projection schema versions, event-contract version, projection mode,
  run ID, genesis hash, confirmed offset, tail hash, and `event_count`.
- `events.sequence` is positive and primary, `event_id` is unique, and
  `(sequence, event_id)` is additionally unique for composite references.
  `schema` records the exact event-contract tag and is CHECK-limited to v1/v2.
  `payload_kind` has a CHECK enumerating every v1/v2 kind accepted by the
  current core contract.
- every event-derived table stores `(event_sequence, event_id)` and has a
  composite foreign key to `events(sequence, event_id)`; record IDs and their
  natural/composite keys are unique.
- lifecycle, claim polarity, target kind, sensitivity, artifact-source kind,
  execution outcome, abstention kind, authority kind, and projection status
  use CHECK constraints containing exactly core's closed serialized values.
- `unreconciled_authority_records.kind` is CHECK-limited to
  `evidence_recorded`, `evidence_bound`, `verification_recorded`, and
  `decision_recorded`; its `authority_reconciled` is
  `INTEGER NOT NULL DEFAULT 0 CHECK (authority_reconciled = 0)`.
- `projected_findings` is a different table and has
  `projection_status TEXT NOT NULL CHECK (projection_status = 'shadow_only')`.
- Program facts, claims, authority/evidence shadows, and findings never share
  a polymorphic object table. CAS registrations and source records retain
  source/registration/hash IDs and do not copy source bytes into SQLite.
- `obligations.lifecycle` is the current materialized lifecycle, initialized
  from the genesis obligation. After a V2 transition has been accepted by
  `OfflineProjectionState`, projection inserts exactly one immutable
  `obligation_lifecycle` history row and executes one guarded update equivalent
  to `UPDATE obligations SET lifecycle = ?next WHERE obligation_id = ?id AND
  lifecycle = ?previous`. `changes()` must equal exactly one; zero or more than
  one is `ProjectionContractViolation`. No other event updates this column.
- This schema has no ID junction tables. Ordered multi-ID fields use dedicated
  `*_ids_canonical_json` TEXT columns containing core canonical JSON arrays,
  strictly sorted and unique by StableId. Rebuild creates them through core's
  canonical serializer; query must decode, validate shape/order/uniqueness,
  reserialize, and require byte equality before returning them.

The security-critical portion is literal DDL, not an implementation hint
(noncritical columns in the other typed tables follow the same strict/FK
pattern):

```sql
CREATE TABLE index_meta (
  singleton                   INTEGER PRIMARY KEY CHECK (singleton = 1),
  index_schema_version        INTEGER NOT NULL CHECK (index_schema_version = 1),
  projection_contract_version TEXT NOT NULL
                              CHECK (projection_contract_version =
                                     'reviewgraphen.index_projection.v1'),
  event_contract_version      TEXT NOT NULL CHECK (event_contract_version IN (
    'reviewgraphen.review_event.v1',
    'reviewgraphen.review_event.v2'
  )),
  projection_mode             TEXT NOT NULL CHECK (projection_mode IN (
    'v1_event_metadata_only',
    'v2_domain'
  )),
  run_id                      TEXT NOT NULL,
  genesis_hash                TEXT NOT NULL,
  confirmed_offset            INTEGER NOT NULL CHECK (confirmed_offset >= 0),
  tail_hash                   TEXT NOT NULL,
  event_count                 INTEGER NOT NULL CHECK (event_count >= 0),
  CHECK (
    (event_contract_version = 'reviewgraphen.review_event.v1'
      AND projection_mode = 'v1_event_metadata_only')
    OR
    (event_contract_version = 'reviewgraphen.review_event.v2'
      AND projection_mode = 'v2_domain')
  )
) STRICT;

CREATE TABLE events (
  sequence       INTEGER PRIMARY KEY CHECK (sequence > 0),
  event_id       TEXT NOT NULL UNIQUE,
  schema         TEXT NOT NULL CHECK (schema IN (
    'reviewgraphen.review_event.v1',
    'reviewgraphen.review_event.v2'
  )),
  event_hash     TEXT NOT NULL,
  payload_hash   TEXT NOT NULL,
  payload_kind   TEXT NOT NULL CHECK (payload_kind IN (
    'obligation_transition',
    'claim_proposed',
    'evidence_recorded',
    'evidence_bound',
    'verification_recorded',
    'decision_recorded',
    'finding_recorded',
    'review_plan_recorded',
    'context_envelope_projected',
    'review_execution_recorded',
    'run_genesis_manifest',
    'artifact_registered',
    'snapshot_sources_recorded'
  )),
  actor          TEXT NOT NULL,
  logical_time   INTEGER NOT NULL CHECK (logical_time >= 0),
  UNIQUE (sequence, event_id)
) STRICT;

CREATE TABLE unreconciled_authority_records (
  event_sequence       INTEGER NOT NULL CHECK (event_sequence > 0),
  event_id             TEXT NOT NULL,
  record_id            TEXT PRIMARY KEY,
  kind                 TEXT NOT NULL CHECK (kind IN (
    'evidence_recorded',
    'evidence_bound',
    'verification_recorded',
    'decision_recorded'
  )),
  body_hash            TEXT NOT NULL,
  authority_reconciled INTEGER NOT NULL DEFAULT 0
                       CHECK (authority_reconciled = 0),
  FOREIGN KEY (event_sequence, event_id)
    REFERENCES events (sequence, event_id)
) STRICT;

CREATE TABLE projected_findings (
  event_sequence   INTEGER NOT NULL CHECK (event_sequence > 0),
  event_id         TEXT NOT NULL,
  finding_id       TEXT PRIMARY KEY,
  body_hash        TEXT NOT NULL,
  projection_status TEXT NOT NULL DEFAULT 'shadow_only'
                    CHECK (projection_status = 'shadow_only'),
  FOREIGN KEY (event_sequence, event_id)
    REFERENCES events (sequence, event_id)
) STRICT;
```

The remaining closed enums are likewise literal `CHECK (... IN (...))`
clauses generated as checked-in static SQL, not values interpolated at runtime.
Adding a core enum value or payload kind therefore requires an index schema
version change and fixture update; an old index never accepts the new string
under a generic TEXT fallback.

`PRAGMA foreign_keys = ON` is read back before any insert. Before commit,
`PRAGMA foreign_key_check` must return zero rows and
`PRAGMA integrity_check` must return exactly the single row `ok`. There is no
`INSERT OR IGNORE`, `INSERT OR REPLACE`, upsert, or duplicate coalescing.
Any duplicate primary, unique, or composite key is `DuplicateIndexKey`, even
when its row bytes would be identical.

Before commit and after read-only deserialize, the store also requires
`SELECT COUNT(*) FROM index_meta` to equal one and `index_meta.event_count` to
equal both the checked length of `events` and the locked journal event count.
Every `events.schema` must equal `index_meta.event_contract_version`; the V1
mode must have every non-event table empty, while the V2 mode must have the
verified baseline and complete projection required by §§6-7.
The count and marker checks are application validation in addition to the DDL;
SQLite `CHECK` expressions cannot contain the required cross-table subquery.

Authority-bearing presence is never converted to `supported`, `verified`,
`accepted`, `human_accepted`, or sign-off. `authority_reconciled = 0` is an
unconditional DDL and projection invariant for every v0.1 rebuild.

### 9. Serialize and bounded FD-relative candidate write

After validation, rebuild commits the in-memory transaction, verifies
autocommit state, and calls `Connection::serialize(DatabaseName::Main)`. It
checks the returned image length before copying or writing it, accounts the
serialize-stage main/cache/view/owned-copy peak against
`max_index_working_bytes`, drops the connection, and only then moves the owned
image into publication. It requires length to be
nonzero, at most `max_index_serialized_bytes`, page-aligned, and consistent
with the checked SQLite page count.

The store writes those bytes with `write_all` to the anonymous candidate FD,
verifies the actual file length and SHA-256 by bounded descriptor reads,
deserializes that exact reread image into a fresh read-only in-memory
connection, and reruns marker, `foreign_key_check`, `integrity_check`, and
typed `IndexSnapshot` queries before publication. This reread first requires
`PRAGMA user_version` to equal 1 and to equal the singleton
`index_meta.index_schema_version`. SQLite never observes the candidate FD or
its name.

### 10. Publication state machine and restart classification

Publication has three explicit states:

```text
Prepared     candidate bytes synced and verified; active entry untouched
Renamed      candidate atomically renamed over active; directory not yet synced
Durable      active inode verified and indexes directory fsynced
```

In `Prepared`, the candidate is linked create-only, the directory is fsynced
so the candidate itself is durable, and its entry is checked against the
candidate FD. Any failure before atomic rename leaves the old active entry
intact. The store may unlink only the exact audited candidate inode and fsync
the directory; cleanup failure is reported but never turns the candidate into
an active index.

Atomic descriptor-relative rename moves the state to `Renamed`. The store then
opens `reviewgraphen.sqlite` no-follow and requires it to be the exact
candidate device/inode, size, and image hash. It finally fsyncs `indexes/` and
moves to `Durable`.

After rename succeeds, failure of active-inode verification or parent fsync is
not reported as success and is not rolled back. It returns
`PublicationDurabilityUncertain` with run/tail/image identity because the
rename may or may not survive a crash; a compensating rename would introduce
another uncertain transition.

On restart, after exclusive index locking, the store classifies the fixed
active entry only from durable bytes:

- absent: `Missing`, requiring rebuild;
- valid image and marker equal to the locked journal run/genesis/offset/tail/
  event-count tuple: current and usable;
- valid image with a different tuple: `CommittedIndexStale`, requiring
  rebuild for current queries;
- invalid, truncated, schema-incompatible, or semantically malformed image:
  `CorruptIndex`, requiring rebuild.

An orphan `candidate-*.sqlite` is never queried or promoted. Candidate audit
reuses ADR 0014's existing `max_tmp_gc_entries` and
`max_tmp_gc_scan_bytes`; no additional implicit scan budget exists.
`DerivedIndex::open` performs no scan. Each query, after taking shared index
lock, audits names/types/count/apparent bytes but does not delete; each rebuild,
after taking exclusive index lock, performs the same audit and may delete only
an exact owner/mode/device/inode-verified candidate before continuing. Exactly
the configured limit is accepted. Observing the first entry or apparent byte
beyond either limit returns `Incomplete` and aborts cleanup, query, or rebuild
with no rows/receipt and no candidate promotion. A malformed candidate name or
non-regular/loose-mode candidate is `CorruptIndex`, not skipped. Thus a crash
at any state is classified without treating a receipt or report as canonical
state.

### 11. FD-relative read and read-only deserialize query

`snapshot_current` acquires shared index lock then journal shared lock. It
opens the active entry descriptor-relatively with no-follow checks, rejects a
negative or over-limit `st_size`, and performs one bounded exact read: at most
`max_index_serialized_bytes + 1` bytes are observed so an oversized image is
typed `Incomplete`, never truncated. It confirms EOF, unchanged FD identity,
and the working-memory budget.

It then opens only an in-memory connection with
`SQLITE_OPEN_READ_WRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_NO_MUTEX`—these
flags create the empty ephemeral destination, not a writable persisted DB—and
calls:

```text
deserialize_read_exact(DatabaseName::Main, reader, exact_size, read_only = true)
```

SQLite's `sqlite3_db_readonly(main)` reports `false` for this in-memory
deserialize destination even when `read_only = true`; it is therefore not a
usable enforcement signal for this design. The derived-query read-only
guarantee is instead: the `Connection` is never public, `read_only = true` is
always supplied to deserialize, `PRAGMA query_only = ON` is set and read back,
and a mutation attempt is required to fail without sidecar files. The query
path also sets/read backs `temp_store = MEMORY` and the SQLite
statement/value/result limits. Before reading the marker or another table, it also reads
`PRAGMA user_version`, requires exactly 1, and requires equality with
`index_meta.index_schema_version`; an unknown/future value is
`CorruptIndex`/rebuild-required, never migrated or queried. A mutation attempt
is a required negative test. No borrowed
`'static` image and no `deserialize(..., read_only = false)` query path is
allowed.

While both locks remain held, the singleton marker is compared to the exact
locked journal event-contract/run/genesis/confirmed-offset/tail/event-count
tuple. A mismatch returns `CommittedIndexStale` before exposing rows. Locks are
retained through all explicit ordered queries, connection close, active-entry
post-stat, and lock-entry post-stat. This closes the compare/query/append
TOCTOU.

### 12. Exact `IndexSnapshot` contract

`IndexSnapshot` contains every persisted projection table and the complete
marker, not an implementation-selected subset:

```text
marker: SQLite user_version, index/projection versions, event contract,
        projection mode, run ID, genesis hash, confirmed offset, tail hash,
        event_count
events: sequence, event ID, schema, event hash, payload hash, payload kind,
        actor, logical time
program_objects: object ID, object kind, canonical body hash
program_relations: relation ID/kind, source ID, target ID, canonical body hash
universe: snapshot/profile/rule/extractor descriptors and canonical body hash
obligations: obligation ID, target kind/ID, property ID, lifecycle, body hash
obligation_lifecycle: event sequence/ID, obligation ID, next lifecycle
claims: event sequence/ID, claim ID, execution ID, polarity, body hash
artifact_registrations: event sequence/ID, registration/run/CAS IDs, media
                        type, size, sensitivity, source kind/ID, body hash
snapshot_source_index: event sequence/ID, snapshot/artifact/registration IDs,
                       path, content/CAS hashes, line count
context_envelopes: event sequence/ID, envelope/obligation IDs, projection
                   hash, source_ids_canonical_json,
                   loss_ids_canonical_json, body hash
review_plans: event sequence/ID, plan/run IDs, budget/policy hashes, body hash
executions: event sequence/ID, execution/reviewer/envelope IDs, outcome,
            raw registration/hash, obligation_ids_canonical_json,
            claim_ids_canonical_json, body hash
unreconciled_authority_records: event sequence/ID, record ID, closed kind,
                                body hash, authority_reconciled=false
projected_findings: event sequence/ID, finding ID, body hash,
                    projection_status=shadow_only
```

Vector order is normative. Every query enumerates columns explicitly, never
uses `SELECT *`, and uses exactly these keys:

```text
index_meta:                       singleton
events:                           sequence, event_id
program_objects:                  object_id
program_relations:                relation_id
universe:                         singleton
obligations:                      obligation_id
obligation_lifecycle:             event_sequence, obligation_id
claims:                           event_sequence, claim_id
artifact_registrations:           event_sequence, registration_id
snapshot_source_index:            snapshot_id, path, artifact_id
context_envelopes:                event_sequence, envelope_id
review_plans:                     event_sequence, plan_id
executions:                       event_sequence, execution_id
unreconciled_authority_records:   event_sequence, record_id
projected_findings:               event_sequence, finding_id
```

Each is an `ORDER BY` over the complete listed key in the listed direction
(ascending). Every `*_ids_canonical_json` array is independently sorted by
StableId ascending. Duplicate rows, primary/unique keys, composite keys, or
nested IDs are corruption, not deduplicated. Two rebuilds are deterministic
when these complete typed snapshots compare equal; raw SQLite byte equality
is not required.

The builder and query accumulator charge every returned string byte, hash/ID
byte, vector element, and fixed-width scalar against `max_index_query_bytes`
with checked arithmetic before pushing it. Limit exhaustion returns no partial
`IndexSnapshot`.

## Consequences

### Positive

- SQLite never receives a filesystem pathname and cannot create a journal,
  WAL, SHM, or ambient temp file.
- All durable index bytes pass through the existing FD-relative StoreRoot
  boundary and a bounded publication state machine.
- Query images are deserialized read-only and remain tail-bound for the whole
  typed query.
- Program facts, Review claims, authority/evidence shadows, and findings stay
  structurally separate.

### Negative

- Rebuild needs enough bounded memory for an in-memory SQLite image and its
  serialized representation.
- Query reads and deserializes the complete active image instead of relying on
  demand paging from a filesystem DB.
- Shared query/exclusive rebuild locking serializes publication against long
  queries.
- A post-rename fsync failure has an honest uncertain outcome that requires
  restart classification rather than automatic rollback.

## Alternatives considered

### A. A normal path or `/proc/self/fd` pathname passed to SQLite

Rejected. Both are filesystem path opens by SQLite and allow VFS-derived
sidecar/temp behavior outside the store's direct descriptor checks.

### B. A custom descriptor-based SQLite VFS

Rejected for MVP. It creates a larger unsafe FFI and filesystem policy surface
than the project currently admits.

### C. SQLite as canonical state

Rejected by ADR 0007. Mutable DB rows cannot replace immutable events and
explicit authority transitions.

### D. Expose a connection or arbitrary SQL callback

Rejected. It loses fixed statement/result bounds, exact snapshot ordering,
read-only enforcement, and tail-lock lifetime control.

### E. Compare rebuilt SQLite bytes

Rejected. SQLite page/free-list layout is not the semantic determinism
contract. Complete ordered `IndexSnapshot` equality is.

## Invariants

1. SQLite receives no filesystem pathname, FD, URI, VFS, or caller-controlled
   storage name; persistence is serialize bytes -> FD-relative store write.
2. Rebuild uses a bounded in-memory connection; query uses bounded FD-relative
   read -> in-memory `deserialize_read_exact(..., read_only=true)`.
3. `index.lock` is shared for queries and exclusive for rebuilds; every
   operation takes index lock before journal shared lock and verifies the lock
   inode before/after flock and before return.
4. Every numeric conversion and size/count/page operation is checked; row,
   image, working-memory, statement, and result limits never truncate success.
5. `max_index_rows` is aggregate across all tables and counts the singleton
   marker and every physical row; the schema has no junction tables.
6. V1 passes homogeneous chain/shape validation and writes event metadata only,
   without `OfflineProjectionState`. Every V2 event goes through exactly one
   exhaustive `(apply bool, payload kind)` branch, after which projection must
   be complete at the locked tail; an unexpected pair aborts rebuild.
7. Every authority-bearing row has a DDL-enforced
   `authority_reconciled = 0`; findings are separate `shadow_only` rows.
8. Foreign keys, closed enum CHECKs, `foreign_key_check`, and
   `integrity_check` must all pass before serialization and after candidate
   reread. `PRAGMA user_version`, the supported index schema version, and
   `index_meta.index_schema_version` are exactly 1 and equal on every read.
9. Failure before rename leaves the old active image intact. Failure after
   rename but before verified parent fsync is
   `PublicationDurabilityUncertain`, never success or automatic rollback.
10. Current queries bind marker run/genesis/offset/tail/event_count to one
    lock-held journal snapshot through query completion.
11. Program facts, claims, authority/evidence shadows, and findings never
    share a polymorphic canonical-status table.
12. Rebuild determinism is complete ordered `IndexSnapshot` equality, not raw
    SQLite image equality.
13. Orphan candidate audits reuse `max_tmp_gc_entries` and
    `max_tmp_gc_scan_bytes`; exceeding either aborts cleanup/query/rebuild with
    `Incomplete` and no partial successful result.

## Acceptance tests

### Staged payload-coverage boundary

The exhaustive projection match covers every `DecodedPayload` variant known to
the current core. A payload that the current V2 event log cannot legally admit
cannot be manufactured solely to populate an index fixture: its projection arm
remains fail-closed and is exercised directly. When a later Accepted event
contract makes that payload legal in a V2 stream, the same change must add it
to the all-legal-payload stream fixture. This is an Accepted staged acceptance
rule, not an unresolved schema decision and not permission to omit an
exhaustive typed arm.

1. A V2 fixture containing every payload kind legally admissible by the
   current Accepted V2 event contract builds, serializes, publishes, reads,
   deserializes, and yields an equal complete `IndexSnapshot` after
   delete/rebuild. Every other known typed projection arm is exercised by a
   direct fail-closed test until its event admission becomes legal.
2. V1 passes `ValidatedEventView` homogeneous chain/shape validation, produces
   event metadata only, never constructs `OfflineProjectionState`, and cannot
   synthesize a V2 domain baseline.
3. Every authority event remains `authority_reconciled = 0`; a finding remains
   `shadow_only`; no verified/accepted/human-accepted state is fabricated.
4. Every legal V2 payload is applied exactly once and exercises its expected
   boolean branch; injected skip/repeat/unexpected boolean-kind pairs,
   `is_complete = false`, or a projection-tail mismatch are hard failures.
5. A rehashed semantically dangling V2 event is rejected by
   `OfflineProjectionState` and cannot replace a good active image.
6. A single SQL statement one byte over `max_index_statement_bytes` is refused
   before prepare; exact-bound statements, image bytes, rows, and query returns
   are accepted, while `limit + 1` is typed incomplete. Schema-version-2 query
   working-set exact-bound and `limit + 1` coverage includes ADR 0017's retained
   lock-held journal view.
7. Fixed page size, calculated `max_page_count`, `temp_store=MEMORY`, cache
   bound, SQLite limits, checked `i64` conversions, and `PRAGMA user_version =
   index_meta.index_schema_version = 1` are read-back tested before DDL, after
   candidate reread, and on query deserialize; another version requires
   rebuild.
8. A monitored rebuild and query create no ambient temp, journal, WAL, SHM, or
   other SQLite filesystem file; no SQLite path-open API is called.
9. A deserialized query connection reports read-only/query-only behavior and a
   mutation statement fails without changing returned state.
10. Crash injection covers: anonymous write, file sync, candidate link,
    candidate-dir fsync, pre-rename, post-rename/pre-inode-check, and
    post-inode-check/pre-dir-fsync. Pre-rename failures preserve the old
    active image; post-rename failures report uncertainty; restart classifies
    old/new/missing/corrupt/stale images correctly.
11. Symlink/FIFO/device/directory/loose-mode/owner/inode replacement for active,
    candidate, or lock entries is a typed refusal. Candidate audit reuses
    `max_tmp_gc_entries`/`max_tmp_gc_scan_bytes`; exact limits pass, limit+1
    aborts cleanup, query, and rebuild with `Incomplete`, and no orphan is
    promoted.
12. Three cooperating actors prove two readers may query, a rebuild excludes
    them, and a journal writer is blocked by their journal shared lock without
    deadlock under the fixed order.
13. A split-lock injection renames/replaces `index.lock` between each audited
    seam; the affected operation returns `IndexPathRace` and no success.
14. Appending after rebuild makes `snapshot_current` return
    `CommittedIndexStale`; pausing after marker comparison proves the writer
    cannot complete until the current query releases its journal lock.
15. Duplicate IDs/composite keys, unknown enum values, missing event FKs,
    `authority_reconciled = 1`, and finding status other than `shadow_only`
    are rejected by DDL or typed duplicate handling. Each accepted obligation
    transition inserts one history row and changes exactly one current
    `obligations.lifecycle` row; zero/double updates fail.
16. Corrupt/truncated/oversized active images, wrong `user_version`, malformed
    marker/event_count, nonempty `foreign_key_check`, and failed
    `integrity_check` return no `IndexSnapshot`.
17. Missing or hash-mismatched genesis CAS bytes, or a manifest inconsistent
    with run/snapshot/profile/genesis, rejects V2 rebuild before publication.
18. Cargo metadata and lockfile tests prove exactly rusqlite 0.40.1 with
    `default-features = false` and exactly the `bundled`, `serialize`, and
    `limits` features requested by `reviewgraphen-store`.

## Documentation relationship

ADR 0014 §7 and `docs/15_storage_and_repository_layout.md` use this
serialize/deserialize contract instead of filesystem-open SQLite,
DELETE-journal, or sidecar behavior. ADR 0017 supersedes only the D1 schema-v2
details enumerated there; this ADR remains normative for descriptor-relative
access, locking, bounds, publication, and authority separation.
