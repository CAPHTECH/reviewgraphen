# ADR 0017: D1 Derived-Index Schema V2

- Status: Accepted
- Date: 2026-08-10
- Amended: 2026-08-10 — narrows `max_index_working_bytes` to exact
  store-visible requested capacities and records allocator-opaque core replay
  memory as an explicit MVP limitation.
- Scope: closes the D1 SQLite projection contract left open between ADR 0015
  and ADR 0016. It amends ADR 0015 only where that ADR fixes index schema
  version 1, table projections, payload-kind checks, snapshot columns, or the
  scope of `max_index_working_bytes`. It does not change ADR 0015's
  descriptor-relative SQLite boundary, locking, publication protocol, or
  authority rules.

## Context

ADR 0015 defines the disposable SQLite implementation and its current schema
version 1. ADR 0016 later fixes deterministic `ReviewPlan` and
`ReviewContextEnvelope` bodies and requires a schema-version-2 follow-up.
Several details still permit incompatible implementations:

- the version transition and treatment of existing version-1 images;
- the exact closed `events.payload_kind` set for D1;
- the preimage of the existing `review_plans.budget_hash` placeholder;
- `context_envelopes.loss_ids_canonical_json`, although `EnvelopeLoss` has no
  stable ID;
- the distinction among event payload, full domain body, identity body, and
  component hashes;
- the relationship between the fixed D1 1 MiB event-record admission and a
  caller-configured `StoreLimits.max_event_line_bytes`; and
- the enforceable distinction between exact store-visible buffer capacities
  and allocator-opaque core semantic replay memory.

SQLite remains a derived query projection. This ADR cannot turn a projected
hash or JSON column into accepted program state, review evidence, verification,
or sign-off.

## Decision

### 1. Version and replacement policy

The D1 index uses all three exact literals:

```text
PRAGMA user_version                         = 2
index_meta.index_schema_version             = 2
index_meta.projection_contract_version      = reviewgraphen.index_projection.v2
```

Version establishment and validation use this exact order:

1. on a fresh in-memory connection, set `PRAGMA user_version = 2` and read it
   back before any DDL;
2. create the complete static version-2 DDL;
3. insert exactly one `index_meta` row, then query its count and version fields
   and require `user_version = index_schema_version = 2` and the exact
   projection literal before inserting any event or domain row;
4. insert the baseline, event, and domain rows, then run all pre-serialization
   validation;
5. after deserializing the candidate into a new connection, read
   `user_version` first, require 2, read exactly one `index_meta` row, and
   compare all marker fields before accepting any projected row; and
6. perform that same pragma-first, singleton-row, equality check on every
   query deserialize before exposing any `IndexSnapshot` row.

Reading `user_version` first also lets an old version-1 image be classified
without preparing a query against a version-2 table shape. A missing,
duplicated, malformed, or disagreeing `index_meta` row is corruption; a clean
`user_version = 1` image is rebuild-required as described below.

An active image with version 1 is recognized as an incompatible derived image
requiring rebuild. It is not corrupt canonical state, is never migrated with
`ALTER TABLE`, copied row-by-row, or opened writable for upgrade, and is never
returned as a current version-2 snapshot. Rebuild ignores its rows, replays the
confirmed canonical JSONL/CAS inputs into a fresh in-memory version-2 database,
and replaces the active image through ADR 0015's existing atomic publication
protocol. A typed `RebuildRequired { found: 1, required: 2 }` classification (or
an exactly equivalent closed typed error) distinguishes this case from malformed
SQLite bytes.

Index schema version and event-contract version are independent. A canonical
version-1 event journal may be rebuilt into a schema-version-2 image in
`v1_event_metadata_only` mode. Its D1 domain tables are empty. “Version 1 is
rebuild-required” refers to the SQLite image version, not rejection of a valid
historical version-1 event journal.

There is no in-place schema migration fixture or supported migration API. The
only migration policy is deterministic rebuild from canonical inputs.

### 2. Closed D1 event vocabulary

Schema version 2 changes `events.payload_kind` to exactly:

```sql
CHECK (payload_kind IN (
  'obligation_transition',
  'claim_proposed',
  'evidence_recorded',
  'evidence_bound',
  'verification_recorded',
  'decision_recorded',
  'finding_recorded',
  'run_genesis_manifest',
  'artifact_registered',
  'snapshot_sources_recorded',
  'review_plan_recorded',
  'context_envelope_projected'
))
```

This SQL check is only a lexical guard. ADR 0015's exhaustive typed projection
match and the event-contract-version rules still reject a kind that is illegal
for the particular stream or projection mode.

The canonical version-2 stream additionally contains at most one
`review_plan_recorded` event for a given `plan_id` and at most one
`context_envelope_projected` event for a given `envelope_id`. Event-log append,
resume, and whole-stream replay check this invariant before aggregate mutation
and reject the entire command, suffix, or batch with typed
`IdCollision { id: plan_id }` or `IdCollision { id: envelope_id }`, even when
the second payload is byte-identical.
An aggregate's idempotent internal apply branch may remain as defensive
protection, but it is not event-log idempotence and does not legalize a second
canonical record. The SQL `UNIQUE` constraints are a projection backstop for
the same stream invariant, never the first definition of it.

`review_execution_recorded` is deliberately absent. ADR 0016 is the later
Accepted decision for D1 and explicitly leaves reviewer/execution events out of
scope. Reserving the tag now would allow an event for which this schema has no
complete Accepted projection. Adding execution, reviewer, or report payloads
therefore requires a later index schema version and an exact table/preimage
amendment. Schema version 2 has no `executions` table or execution rows in its
typed `IndexSnapshot`; an empty future-facing placeholder is not a contract.

### 3. Exact `PlanBudget` bytes and hash

`review_plans` stores both `budget_canonical_json` and `budget_hash`.
`budget_canonical_json` is exactly the bounded canonical `PlanBudget` object:

```json
{"max_obligations_per_wave":2048,"max_waves":1024}
```

The numbers above show the maximum baseline values, not constants substituted
for every plan. For a particular plan they are its validated positive `u32`
values. Key order is exactly `max_obligations_per_wave`, then `max_waves`; there
is no whitespace, alternate integer spelling, additional key, or surrounding
payload object.

The baseline bounds make the longest legal object exactly 50 UTF-8 bytes. The
core writer checks the two numeric policy bounds and the 50-byte component cap
before append/allocation. A future policy that permits a longer representation
requires a policy and index-schema review; the store does not silently widen
this preimage.

The hash is exactly:

```text
budget_hash = SHA-256(UTF8(budget_canonical_json))
```

It is not the planner-input hash, planner-policy hash, plan identity-body hash,
full-plan body hash, event payload hash, or hash of a JSON string containing the
object. Rebuild computes the bytes and hash through core's bounded canonical
writer. Query decodes the object, validates the same bounds, reserializes it,
requires byte equality, and recomputes `budget_hash`.

### 4. Exact version-2 D1 tables

All ADR 0015 baseline tables and authority separation remain, except that its
future-facing plan/context/execution placeholder shapes are superseded here.
Schema version 2 adds the following exact plan and context projections and does
not add an execution projection:

```sql
CREATE TABLE review_plans (
  event_sequence               INTEGER NOT NULL CHECK (event_sequence > 0),
  event_id                     TEXT NOT NULL,
  plan_id                      TEXT NOT NULL UNIQUE,
  universe_id                  TEXT NOT NULL,
  snapshot_id                  TEXT NOT NULL,
  planner_input_hash           TEXT NOT NULL,
  planner_policy_version       TEXT NOT NULL
                               CHECK (planner_policy_version =
                                      'scheduler.baseline@1'),
  planner_policy_hash          TEXT NOT NULL,
  budget_canonical_json        TEXT NOT NULL,
  budget_hash                  TEXT NOT NULL,
  risk_breakdown_canonical_json TEXT NOT NULL,
  waves_canonical_json         TEXT NOT NULL,
  deferred_canonical_json      TEXT NOT NULL,
  identity_body_hash           TEXT NOT NULL,
  body_hash                    TEXT NOT NULL,
  PRIMARY KEY (event_sequence, plan_id),
  FOREIGN KEY (event_sequence, event_id)
    REFERENCES events (sequence, event_id)
) STRICT;

CREATE TABLE context_envelopes (
  event_sequence                        INTEGER NOT NULL CHECK (event_sequence > 0),
  event_id                              TEXT NOT NULL,
  envelope_id                           TEXT NOT NULL UNIQUE,
  snapshot_id                           TEXT NOT NULL,
  context_policy_version                TEXT NOT NULL
                                        CHECK (context_policy_version =
                                               'context.baseline@1'),
  context_policy_hash                   TEXT NOT NULL,
  candidate_ids_canonical_json          TEXT NOT NULL,
  obligation_ids_canonical_json         TEXT NOT NULL,
  context_policy_canonical_json         TEXT NOT NULL,
  included_sources_canonical_json       TEXT NOT NULL,
  excluded_sources_canonical_json       TEXT NOT NULL,
  unknowns_canonical_json               TEXT NOT NULL,
  assumptions_canonical_json            TEXT NOT NULL,
  losses_canonical_json                 TEXT NOT NULL,
  projection_hash                       TEXT NOT NULL,
  body_hash                             TEXT NOT NULL,
  PRIMARY KEY (event_sequence, envelope_id),
  FOREIGN KEY (event_sequence, event_id)
    REFERENCES events (sequence, event_id)
) STRICT;
```

The composite event foreign key relies on ADR 0015's existing unique
`events(sequence,event_id)` pair. The ordered query keys remain
`(event_sequence,plan_id)` and `(event_sequence,envelope_id)` ascending.

`review_plans.run_id`, the old narrow `policy_hash`,
`context_envelopes.obligation_id`, `source_ids_canonical_json`, and
`loss_ids_canonical_json` are not version-2 columns. They do not losslessly
represent the Accepted domain records.

### 5. Losses are bodies, not invented IDs

`EnvelopeLoss` has no canonical stable ID. Schema version 2 therefore chooses
`losses_canonical_json`, the exact canonical array from the envelope, rather
than inventing loss IDs or a new loss-hash namespace. This is the smallest
projection that preserves existing facts and permits full-body reconstruction.

Each loss element retains exactly:

```text
affected_properties, description, severity, source_ids
```

The array uses the deterministic builder order. In the D1 baseline, each loss
has exactly one `affected_properties` member, equal to the envelope obligation's
`property_id`; zero or multiple affected properties are rejected. Its
`source_ids` remain canonical sorted and unique and may contain one or more
source IDs. Query re-decodes and re-canonicalizes the full array. No row count,
array position, description, or confidence value is promoted into an identity.
If a later design needs addressable losses, it must define that identity in
core first and bump the projection contract; the store may not manufacture it.

The same preservation rule applies to `unknowns_canonical_json`,
`included_sources_canonical_json`, and `excluded_sources_canonical_json`: these
are full canonical bodies with source traces, not counts or ID-only summaries.

### 6. Hash and preimage separation

The following preimages are normative and non-interchangeable:

| Field | Exact SHA-256 preimage |
|---|---|
| `events.event_hash` | Core's canonical JSON object with exactly `actor`, `event_id`, `genesis_hash`, `logical_time`, `payload_hash`, `previous_event_hash`, `run`, `schema`, and `sequence`. For the first event, `previous_event_hash` is the core-derived chain-genesis sentinel; thereafter it is the immediately preceding event's `event_hash`. |
| `events.payload_hash` | Core's canonical complete persisted-payload wrapper, including its closed `type` discriminator and `data`. |
| `review_plans.planner_input_hash` | ADR 0016's bounded canonical `PlannerInput` body. |
| `review_plans.planner_policy_hash` | The exact canonical `scheduler.baseline@1` policy DTO bytes. |
| `review_plans.budget_hash` | The exact canonical `PlanBudget` object from §3. |
| `review_plans.identity_body_hash` | ADR 0016's complete canonical plan identity body: budget, sorted deferred IDs, planner input hash, planner policy version/hash, snapshot ID, universe ID, and ordered identity wave contents. |
| `review_plans.body_hash` | The exact bounded canonical full `ReviewPlan` object embedded in the event payload. |
| `context_envelopes.context_policy_hash` | The exact canonical `context.baseline@1` policy DTO bytes stored in `context_policy_canonical_json`. |
| `context_envelopes.projection_hash` | ADR 0016's complete canonical context identity body. |
| `context_envelopes.body_hash` | The exact bounded canonical full `ReviewContextEnvelope` object embedded in the event payload. |

The plan ID must equal `plan:<identity_body_hash>` under core's accepted ID
syntax. The context envelope ID must equal the corresponding core-derived
context-envelope ID for `projection_hash`. Neither `body_hash` includes the
event payload `type`/`data` wrapper; `events.payload_hash` does. Rebuild and
query must recompute all applicable hashes independently and must never assert
equality merely because two fields happen to contain the same algorithm.

The full plan is reconstructed in core canonical field order from the budget,
deferred, ID, planner input/policy, risk breakdown, snapshot, universe, and
wave columns. The full envelope is reconstructed in core canonical field order
from assumptions, candidate IDs, policy body/hash, exclusions, ID, inclusions,
losses, normalized included IDs derived and checked from inclusions,
obligation IDs, projection hash/version, snapshot ID, and unknowns. A missing
full-body field is a projection-contract violation, not permission to hash a
narrower row.

Wave IDs are not stored or hashed as part of `waves_canonical_json`; they are
re-derived from the plan ID, wave index, and obligation IDs when a query shape
needs them. `risk_breakdown_canonical_json` contains the deterministic impact
and likelihood only. `deferred_canonical_json` contains the closed reason tag,
while the identity body uses only its sorted IDs, exactly as ADR 0016 defines.

The `events` projection intentionally does not duplicate canonical payload
bytes or `previous_event_hash`. Consequently, query validation must not claim
to reconstruct `events.payload_hash` or `events.event_hash` from SQLite columns
alone. While holding ADR 0015's shared index lock followed by the journal shared
lock, `snapshot_current` reads and validates the exact confirmed journal slice
through `index_meta.confirmed_offset`. The read is bounded by
`max_event_line_bytes`, `max_events`, and `max_replay_bytes`, and core validates
canonical event bytes, payload decoding, the run/genesis binding, sequence,
event IDs, and the complete chain including the preceding hash.

Before returning a snapshot, the query then cross-compares that validated
journal pass one-for-one with `events`, in sequence order, over `sequence`,
`event_id`, `schema`, `event_hash`, `payload_hash`, `payload_kind`, `actor`, and
`logical_time`, as well as marker event count and tail hash. Payload hashes are
recomputed from the journal's exact canonical persisted-payload wrapper;
event hashes are recomputed from the exact preimage above. Any missing, extra,
reordered, or unequal SQLite event row is a typed corrupt-index/projection
failure and returns no partial snapshot. Rebuild performs the same comparison
through a bounded pass before publication.

#### Query working-set accounting

`max_index_working_bytes` covers only capacities whose ownership is visible
and enforceable at the store boundary. It is not a claim about allocator heap,
RSS, `std::collections::BTreeMap` node layout, or transient allocations made by
serde/core typed decoding. The exact store-visible journal charge is:

- `J_raw`: capacity of the one bounded physical-line JSONL input buffer;
- `J_meta`: capacity of retained compact event metadata needed for the ordered
  SQLite comparison; it excludes raw payload bodies and decoded domain values;
- `J_store_scratch`: capacity of additional store-owned canonical-event, hash,
  torn-suffix-metadata, and cursor buffers; it expressly excludes the physical-
  line buffer already charged as `J_raw`; and
- `T_compare`: the one current SQLite event-row tuple and other compact cursor
  state not already charged to the result;
- `G`: the single shared backing capacity of canonical V2 genesis bytes. The
  journal owns it for the complete locked operation, so every stage charges it;
  readers/index locals may borrow or clone a compact certificate but may not
  clone `G`; and
- `C`: one fixed streaming CAS comparison chunk of at most 65,536 bytes, used
  to compare the manifest artifact with `G` and released before SQLite work.
  The store must not read the complete CAS object into a second `Vec`.

“Capacity” here means the byte size requested from the allocator for an
application-visible buffer: checked `Layout::array::<T>(capacity).size()` for
typed arrays plus exact requested UTF-8/byte-buffer capacity. Shared backing is
charged once, not once per `Arc`/borrow. Allocator headers, size-class rounding,
and RSS are outside this metric as stated below.

Capacity growth for those store-owned values is reserved and charged before
allocation. Fixed-width sizes and every multiplication/addition use checked
`u64` arithmetic. The bounded index path validates one physical event line at
a time, drops its payload/decode scratch before advancing, and must not call an
unbounded whole-prefix `ValidatedEventView` or binary-search replay helper. A
second bounded pass is permitted while the same journal lock is held. No pass
may read a workspace/repository tree; the only inputs are the confirmed JSONL
prefix and the exact referenced genesis/CAS object. The index path does not
retain a per-event `starts` vector or copy a torn suffix: it keeps only the
current checked offset plus torn length/hash metadata, while the bytes remain
in `J_raw`. `J_raw` and per-pass scratch are dropped before a later stage unless
that later peak explicitly charges them.

The implementation evaluates these exact **store-visible** stage peaks:

```text
semantic_store_peak    = G + C + J_raw + J_meta_so_far + J_store_scratch
journal_scan_peak      = G + J_raw + J_meta_so_far + J_store_scratch
rebuild_projection_peak = G + J_raw + J_meta + J_store_scratch
                        + reconstructed_main + configured_build_cache
                        + current_insert_tuple
deserialize_peak       = G + J_meta + active_read_image
                       + reconstructed_main + configured_query_cache
query_compare_peak     = G + J_raw + J_meta + J_store_scratch
                       + reconstructed_main + configured_query_cache
                       + snapshot_reservation + T_compare
query_return_peak      = G + J_meta + reconstructed_main
                       + configured_query_cache + actual_snapshot
serialize_peak         = G + J_meta_if_retained + reconstructed_main
                       + configured_build_cache + serialized_view + owned_copy
candidate_write_peak   = G + J_meta_if_retained + owned_copy
candidate_reread_peak  = G + J_meta + candidate_read_image
                       + reconstructed_main + configured_query_cache
                       + snapshot_reservation + T_compare
```

`snapshot_reservation` is ADR 0015's complete preflight query charge. During
row materialization it is replaced by the actual charged result ownership; if
an implementation retains both, it charges their sum. Likewise, any changed
ordering that keeps `J_raw`, canonical scratch, a result reservation, or a
second image alive into a later stage adds that ownership to the applicable
peak rather than relying on the formulas' baseline lifetimes.
`rebuild_projection_peak` applies while the bounded journal pass and SQLite
insert cursor overlap; `query_compare_peak` applies while journal and SQLite
cursors are compared. An implementation may omit one of these peaks only by
using an ordering that proves those owners never overlap.

Publication writes and fsyncs the candidate from `owned_copy`, verifies the
write result, then drops `owned_copy` before opening or allocating
`candidate_read_image`. Candidate reread/deserialization therefore uses
`candidate_reread_peak`; retaining both images is forbidden. The reread image
is consumed/dropped under the same deserialize seam used by a current query.

Before retrieving the first owned SQLite row, query performs fixed-SQL
`COUNT(*)` and `SUM(length(CAST(column AS BLOB)))` preflight for every returned
text/blob column, computes every result-vector `Layout`, checks the complete
query/working peaks, and reserves the result vectors. Row iteration first reads
text/blob values as borrowed `ValueRef`; it verifies the preflight length and
reserves the exact destination string/byte capacity before copying or parsing.
`row.get::<String>`, owned ID/hash parsing, or a `push` before these admissions
is forbidden. The same rule applies to the one current comparison tuple.

Before opening/deserializing the image, the query cache is derived from the
remaining working budget after reserving `G`, `J_meta`, two maximum serialized
images, and the complete configured query budget. It must still satisfy ADR
0015's nonzero checked cache rule. Before each journal/result allocation and
before returning the snapshot, the applicable checked peak must be no greater
than `max_index_working_bytes`. Overflow uses observed `u64::MAX`; exceeding
the limit returns typed `Incomplete { limit, observed }` and no partial
`IndexSnapshot`. Thus a journal may satisfy the 1 GiB default replay limit yet
be refused under the 256 MiB default store-visible working limit.

`J_meta` remains available through the ordered `events` cursor, or is produced
again by a bounded second pass, so each validated journal event is compared
with exactly one SQLite row before either cursor advances. Both cursors must
reach EOF together; this resource rule does not weaken the one-for-one
hash-bound comparison or the lock lifetime.

#### Core decode/replay boundary and accepted limitation

Core typed payload decode, `RunGenesisSnapshot`, `ReviewAggregate`, and
`OfflineProjectionState` are governed independently by
`max_event_line_bytes`, `max_events`, `max_replay_bytes`, the fixed canonical
D1 body caps, and these fixed structural caps:

```text
max_event_json_depth          = 128
max_event_json_values         = 65_536
max_genesis_json_depth        = 128
max_genesis_json_values       = 1_048_576
```

“Values” counts every array element and object member across the complete
document, including nested evidence attributes; depth counts every entered
array/object. A no-owned-allocation lexical pass checks both before serde/core
typed decode. Duplicate object-key rejection and all stricter domain caps
still apply. Exact limit is accepted. The first value/depth at `limit + 1`
returns core `DomainError::Incomplete` with one of these stable operation
strings, the applicable limit, and `observed = limit + 1`:

```text
index event JSON structure
index genesis JSON structure
```

Journal maps that result to `JournalError::Incomplete` without changing the
tuple; index maps it to `IndexError::Incomplete`. Structural-cap exhaustion is
a resource refusal, never `CorruptIndex`, schema invalidity, migration, or a
partial projection. These limits are checked before a physical line or
confirmed prefix can exceed them. Semantic
replay is streaming: it retains only its current aggregate/projection state
and one event's decode/canonical scratch, and drops that state before SQLite
deserialize/query/publication. Full genesis bytes have one owner; journal
readers and index operations retain only a compact hash/manifest-binding
certificate or a borrow, never another full clone. The journal's single `G`
backing remains live and charged until the locked operation returns.

This does **not** prove an exact allocator-heap or RSS peak. Standard-library
map node layout, allocator metadata, and serde transients are deliberately
outside `max_index_working_bytes`. The named MVP limitation is
`allocator_heap_not_exactly_accounted`: a structurally bounded but adversarial
high-fan-out/deep payload may consume more process memory than its canonical
byte length suggests. Deployments requiring hostile-input memory isolation
must run review/index work under an external process memory limit. A follow-up
ADR must choose either project-owned fixed-capacity arena/collections with
allocation-aware streaming visitors, or an isolated worker with a specified
OS memory-limit contract. Until then, documentation and reports must not call
`max_index_working_bytes` an exact heap/RSS bound.

The structural caps are operation limits of the bounded derived-index path,
not new fields or validity rules in event/genesis payload schemas. Therefore
this amendment does not bump the event contract, genesis schema, planner/
context policy, or SQLite schema version. Canonical V1/V2 events and genesis
objects written before this amendment remain canonical history even when they
exceed a new index-operation cap. A schema-v2 rebuild/query encountering such
history returns typed `Incomplete`; it must not relabel the journal corrupt or
migrate/rewrite it. The operator may use a future explicitly versioned resource
profile with higher caps, or a future arena/worker contract. The current fixed
MVP caps cannot be widened by an unversioned local setting.

### 7. Canonical JSON and rebuild validation

Every `*_canonical_json` column contains UTF-8 canonical JSON text, never a
quoted JSON string, SQLite-generated JSON, or lossy projection. Rebuild obtains
these bytes from validated core types and bounded canonical writers. SQL string
concatenation and SQLite JSON functions are not canonical serializers.

Before insertion, candidate-image publication, and query return, validation
requires:

1. exact version and projection literals;
2. exact canonical byte equality for every JSON column;
3. closed enum/reason values and all nested ordering/uniqueness rules;
4. complete event foreign keys plus stream-level duplicate-plan/context
   refusal before aggregate apply;
5. component, identity, full-body, payload, and event hashes against their own
   preimages;
6. reconstruction of the complete typed plan or envelope through core's
   strict metadata-only decode/validation boundary;
7. ADR 0015 row, statement, image, store-visible working-capacity, and
   query-result bounds.

Validation failure aborts the complete rebuild or query. No partially decoded
row or partial `IndexSnapshot` is returned.

Two rebuilds from the same confirmed journal/genesis/CAS tuple must yield equal
typed `IndexSnapshot` values in the normative query order. Raw SQLite image
bytes need not be equal. The projection contains no timestamps, random IDs,
source-path-derived keys, or migration history.

### 8. Outer event line and store limits

For `review_plan_recorded` and `context_envelope_projected`, core first enforces
ADR 0016's fixed condition with checked arithmetic:

```text
canonical_event_envelope_bytes + 1 trailing LF <= 1_048_576
```

The journal independently measures that same physical canonical JSONL line,
including its one LF, and requires:

```text
line_bytes <= StoreLimits.max_event_line_bytes
```

The effective admission limit is therefore the smaller of 1,048,576 and the
configured store limit. Raising `max_event_line_bytes` above 1 MiB never widens
the D1 core contract. Lowering it is allowed and causes the store to refuse an
otherwise core-valid event atomically before append. The 786,432-byte inner
plan/envelope bound does not reserve or guarantee the remaining event-wrapper
space and does not waive either outer check.

Index `max_index_*` limits are separate resource bounds. They do not authorize
larger event lines and do not truncate canonical JSON columns. A complete row
or typed query result that exceeds an index bound is a typed `Incomplete`
failure.

### 9. Fixture and rollout policy

The implementation change must include checked-in or exact golden fixtures for:

1. literal schema-version-2 DDL and all three version markers, including the
   pragma-before-DDL, singleton-before-domain-DML, and pragma-first candidate/
   query read-back order from §1;
2. a version-1 SQLite image classified rebuild-required, followed by a fresh
   deterministic version-2 rebuild with no migrated rows;
3. a valid historical version-1 event stream rebuilt as version-2
   metadata-only;
4. a version-2 stream containing both D1 payload kinds, with complete plan and
   context rows and equal query/rebuild snapshots;
5. exact `PlanBudget` bytes/hash at minimum and maximum legal values and
   canonical-byte mutations;
6. a positive envelope with multiple loss groups, each naming exactly the one
   affected obligation property, with one or more source IDs and at least one
   multi-source group, proving no loss ID is synthesized and grouping survives
   rebuild/query; plus negative zero-property and multi-property loss fixtures;
7. recomputed identity/full-body/payload/event hash mutations that demonstrate
   the four preimage boundaries are checked independently; SQLite-only
   mutations of each projected event field are rejected by exact locked
   journal cross-comparison, including a chain whose recomputation changes its
   `previous_event_hash` input;
8. canonical ordering, unknown enum/reason, missing FK, and malformed JSON
   refusals; duplicate plan/context append, resume suffix, and whole-stream
   replay fixtures prove atomic typed refusal with no second event or row;
9. exact 1 MiB outer line and one-byte-over refusal, plus a configured store
   limit below and above 1 MiB;
10. delete-and-rebuild determinism at an identical confirmed journal tail.
11. a combined store-visible working-set fixture charges nonempty `J_raw`,
    `J_meta`, `J_store_scratch`, `G`, the CAS chunk, rebuild-insert overlap,
    serialize view/copy, both deserialize images/cache, and the query
    journal/SQLite comparison overlap plus snapshot reservation. It derives
    serialize admission from pre-call `page_count * page_size`, and derives
    query admission from SQL count/UTF-8-byte preflight before any owned row.
    Setting
    `max_index_working_bytes` to the larger exact store-visible stage peak
    succeeds; lowering it by one byte returns typed `Incomplete` before a
    partial snapshot or store-owned allocation, including checked-arithmetic
    overflow coverage;
12. journal fixtures cover maximum event count, exact/over replay bytes,
    exact/over physical line bytes, checked cursor overflow, and torn suffix
    length/hash handling without a suffix copy. They prove the bounded index
    path retains no starts vector and never invokes an unbounded whole-prefix
    replay or binary-search validator; and
13. high-fan-out collection and maximum permitted JSON-depth fixtures exercise
    each closed payload family and genesis. Exact depth/value caps succeed;
    cap-plus-one returns the specified stable-operation `Incomplete` tuple
    through core, journal, and index without partial output. A pre-amendment
    canonical over-cap journal remains canonical and is refused only for this
    bounded index operation, never classified corrupt/migration-required.
    These are structural-bound tests, not evidence of exact allocator heap/RSS
    accounting; and
14. publication lifetime instrumentation proves the owned serialized image is
    dropped after candidate write/fsync and before candidate reread allocation;
    exact and one-byte-low `candidate_reread_peak` fixtures include `G`, compact
    journal metadata, reread image, reconstructed main/cache, snapshot
    reservation, and comparison tuple.

There is no fixture that treats an old SQLite row as migration input. Version
1 images are disposable outputs; canonical JSONL/CAS data is the only rebuild
input.

## Consequences

### Positive

- D1 plan/context projection is reconstructible and independently hash-checked.
- No loss identity or execution support is fabricated in the store.
- Historical event journals remain usable while disposable SQLite images have
  one clear rebuild boundary.
- Inner, outer, journal, and index resource limits have non-overlapping
  authority.

### Negative

- Existing schema-version-1 images must be rebuilt even if they contain no D1
  rows.
- Full canonical JSON bodies use more derived-index space than ID-only
  summaries.
- Reviewer execution requires another schema version rather than reusing a
  reserved tag/table.
- `max_index_working_bytes` is an exact store-visible capacity bound, not an
  exact process-memory bound. Core decode/replay retains the explicit
  `allocator_heap_not_exactly_accounted` limitation until the arena or isolated
  worker follow-up is accepted and implemented.

## Superseded text

For D1 schema version 2, this ADR supersedes:

- ADR 0015's `PRAGMA user_version = 1`, projection-contract-v1, plan/context/
  execution placeholder columns, and version-1 `IndexSnapshot` list;
- ADR 0015's payload-kind example where it pre-reserves plan/context/execution
  strings beyond the implemented projection;
- ADR 0016 §6 only where this ADR adds the exact `budget_hash` preimage,
  eliminates nonexistent loss IDs, and fixes the rollout/fixture policy.

ADR 0015's filesystem, lock, SQLite serialization, publication,
authority-separation, and tail-freshness decisions remain unchanged. Its
working-memory wording is narrowed by this ADR to exact store-visible
capacities; it is not an allocator heap/RSS guarantee. ADR 0016's planner/
context canonical bodies and identities remain authoritative.

## Accepted D2 follow-up

[ADR 0018](0018-d2-execution-claim-report-and-index-v3.md) defines the first
complete execution projection as index schema version 3. It preserves this
ADR's D1 schema-v2 contract and classifies existing schema-v2 images as
rebuild-required rather than migrating them in place.
