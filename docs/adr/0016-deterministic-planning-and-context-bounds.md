# ADR 0016: Deterministic Planning and Context Bounds

- Status: Accepted
- Date: 2026-08-09
- Scope: amends and supersedes ADR 0013 §§4–5b where deterministic planning,
  bounded context construction, identities, or C3 projection were open.
  Reviewer/execution events remain out of scope.

## Decision

### 1. Fixed typed policy DTO and golden bytes

The implementation owns checked-in typed `PlannerPolicyV1` and
`ContextPolicyV1` DTOs. No semantic setting is a free implementation constant;
each `rules` value is a closed typed enum serialized by its listed tag, never
caller-supplied prose.
Their combined, lexicographically-key-sorted canonical JSON is exactly:

```json
{"context":{"anchors_per_file":1024,"callees_depth":3,"callers_depth":2,"canonical_envelope_bytes":786432,"contains_edges":1000000,"discovery_paths":20,"edge_kind_direction_order":["calls:forward","calls:reverse","contains:forward","contains:reverse","covers:forward","covers:reverse"],"exclusion_reason_precedence":["path_cap","test_cap","not_reached","included_file_cap","artifact_bytes_cap","total_resolved_bytes_cap","giant_line","excerpt_bytes_cap","total_excerpt_bytes_cap"],"excerpt_lines":400,"included_files":64,"loss_descriptions":[["artifact_bytes_cap","context_loss:artifact_bytes_cap"],["excerpt_bytes_cap","context_loss:excerpt_bytes_cap"],["excerpt_window_truncated","context_loss:excerpt_window_truncated"],["giant_line","context_loss:giant_line"],["included_file_cap","context_loss:included_file_cap"],["not_reached","context_loss:not_reached"],["path_cap","context_loss:path_cap"],["test_cap","context_loss:test_cap"],["total_excerpt_bytes_cap","context_loss:total_excerpt_bytes_cap"],["total_resolved_bytes_cap","context_loss:total_resolved_bytes_cap"]],"max_assumptions":64,"max_candidates":4096,"max_discovered_structural_ids":4096,"max_excerpt_bytes":262144,"max_losses":64,"max_resolved_artifact_bytes":1048576,"max_resolved_bytes":8388608,"max_string_bytes":16384,"max_total_excerpt_bytes":1048576,"max_unknowns":64,"obligations_per_envelope":1,"related_tests":10,"relation_scan":1000000,"rules":["all_candidates_metadata_closure","anchors_out_of_range_domain_failure","anchors_reached_contains_locations","baseline_assumptions_empty","baseline_exactly_one_obligation","bfs_visited_edge_kind_direction_depth","bounded_canonical_serialization","calls_caller_to_callee","canonical_bfs_predecessor_paths","contains_file_to_member","context_identity_bounded_writer","covers_test_to_subject","event_admissions_context_projection","excerpts_lf_raw","full_range_some_normalizes_none","giant_line_exclude_loss","live_projection_private_admission","loss_adr0013_grouped_v1","max_string_all_canonical_text","offline_replay_metadata_only","ranked_greedy_two_phase_resolution","seed_kind_exact_expansion","selection_path_test_caps_exclude_loss","safety_caps_incomplete","source_artifact_excerpt_integrity"],"unknown_descriptions":["context_unknown:unresolved_invariant_scope","context_unknown:unresolved_relation_endpoint","context_unknown:unresolved_review_context_member","context_unknown:unresolved_seed_reference"],"version":"context.baseline@1"},"planner":{"canonical_input_bytes":786432,"canonical_plan_bytes":786432,"deferral_reasons":["budget_exhausted","prerequisite_deferred"],"empty_universe":"allowed","initial_lifecycle":"generated","max_edges":65536,"max_obligations":2048,"max_obligations_per_wave":2048,"max_string_bytes":16384,"max_waves":1024,"ordering":["impact_desc","prerequisite_depth_desc","stable_id_asc"],"rules":["all_obligations_generated","bounded_canonical_serialization","dependency_cycle_domain_failure","dependency_dangling_domain_failure","kahn_boundary_ready_only","likelihood_medium","max_string_all_canonical_text","plan_contains_planner_input_hash","planner_input_bounded_writer","reasons_closed_tags_only","risk_rationale_absent","schedule_wave_reason_absent","wave_id_plan_index_ids"],"thresholds":[["0","0.25","info"],["0.25","0.5","low"],["0.5","1","medium"],["1","2","high"],["2","infinity","critical"]],"version":"scheduler.baseline@1"}}
```

The `rules` arrays are closed, exhaustive semantic tags for this ADR's planner
and context rules; changing, adding, or reordering one is a DTO change. The
exact UTF-8 byte length and SHA-256 are fixed in §8. Individual policy hashes
are SHA-256 over the respective canonical sub-DTO. Both hashes, all limits,
all semantic tags/rules in the DTO, and their exact versions are
identity-bearing.

### 2. Planner input, plan identity, and exact mechanics

`PlannerInput` is a private typed value serialized as:

```text
{
  "budget": PlanBudget,
  "obligations": [
    {"depends_on": [StableId...], "id": StableId,
     "weight_ieee754_bits": "16-lowercase-hex"}
  ],
  "planner_policy_hash": ContentHash,
  "snapshot_id": StableId,
  "universe_id": StableId
}
```

Keys are canonical-sorted, obligations and dependencies StableId-sorted, and
each validated finite positive `f64` weight is represented by its exact
`to_bits()` IEEE-754 hexadecimal string. `planner_input_hash` is SHA-256 of
these exact bytes. Canonical `PlannerInput` bytes are capped at 786,432 and
are emitted through a bounded writer that refuses before any capacity
preallocation beyond the remaining limit. Planner policy
`max_string_bytes=16,384` applies to every canonical planner `String` and
`StableId` text in `PlannerInput` and `ReviewPlan`, including policy tags,
versions, IDs, and deferral tags; excess is `Incomplete`. Empty input is legal
and yields an empty plan. Every input obligation must have lifecycle `Generated`; any other lifecycle, more than
2,048 obligations, more than 65,536 dependency edges, a dangling dependency,
or a cycle is a typed domain failure. The canonical serialized `ReviewPlan`
may not exceed 786,432 bytes.

Risk impact maps `(0,0.25] -> info`, `(0.25,0.5] -> low`, `(0.5,1] -> medium`,
`(1,2] -> high`, and `>2 -> critical`; nonpositive/nonfinite values are
rejected. Likelihood is `medium`. This baseline removes `RiskDescriptor`'s
`rationale` field: the descriptor contains only impact and likelihood, so no
unprojected or nonidentity rationale can exist. Priority is impact descending,
longest prerequisite-chain depth descending, then StableId ascending.

Kahn mechanics are fixed: only the ready set at a wave boundary is eligible;
choose its first `max_obligations_per_wave` items by priority; commit that
wave; then calculate the next ready set. A prerequisite and successor never
share a wave. `max_waves` and `max_obligations_per_wave` are nonzero and are
bounded by the DTO. When wave budget ends, unplaced ready IDs have closed
reason `budget_exhausted`; all remaining descendants blocked by a deferred
prerequisite have `prerequisite_deferred`. Reasons are enum tags only: no
optional detail exists.

`ReviewPlan` identity body includes `planner_input_hash`, universe/snapshot,
planner policy version/hash, budget, ordered wave contents, and deferred ID
set. `ScheduleWave.id = derived("schedule-wave", {plan_id,wave_index,ids})`.
ADR 0013's M3 `ScheduleWave.reason: String` is removed: a wave is exactly
`{wave_index, obligation_ids}` and carries no reason. Deferral reasons exist
only in the plan's closed `deferred` map. The plan body contains those wave
contents only, never wave IDs, so this is acyclic. The store independently
rejects a plan projection exceeding its configured row/image limits as typed
`Incomplete`; it never changes scheduling content.

### 3. Context seeds, directed discovery, and complete partition

The candidate denominator is all accepted `file` artifacts for the exact
snapshot in StableId order; more than 4,096 is `Incomplete`, never a prefix.
The baseline has exactly one input obligation and therefore exactly one
`obligation.property_id`; zero or more than one obligation is a typed domain
failure. Baseline assumptions remain exactly `[]`.

Context construction is a two-phase API and `reviewgraphen-core` performs no
CAS I/O. First, the caller supplies metadata closure for **every candidate**:
the snapshot/artifact/source/CAS tuple, `WorkspaceSource` kind, declared byte
length, and content hash. The builder validates that closure before ranking.
Missing registration, tuple/source mismatch, invalid length/hash metadata, or
non-`WorkspaceSource` registration is a typed domain failure, not an
exclusion. It ranks and preselects using this registered metadata and size.
Second, an external resolver supplies full bytes for preselected files strictly
in rank order; the builder validates them against its registered tuple,
declared length, and content hash before excerpting. If a resolved candidate is
excluded (including `giant_line` or an excerpt budget reason), it records that
closed exclusion/loss and backfills by advancing to the next ranked candidate.
It continues until 64 files are included or candidates or a byte budget are
exhausted. These staged resolutions share `max_resolved_bytes=8,388,608`; an
over-limit next file is excluded using the relevant byte-budget reason without
resolving it. A supplied byte mismatch is a typed domain failure.

Context policy `max_string_bytes=16,384` applies to every canonical context
`String` and `StableId` text in `ContextIdentity`, `ContextEnvelope`, source
refs, descriptions, and `property_id`; any excess is `Incomplete`. It is
checked by bounded canonical writers before allocation. It does not define the
outer event contract: `EventEnvelope` strings/record bytes are independently
governed by `EventContract`/`StoreLimits` and the D1 outer-record bound below.
The canonical context identity body is likewise written through a 786,432-byte
bounded writer, not constructed unbounded then measured.

Seed expansion is exact and deterministic. For every input seed, expansion is
by its accepted kind only: an `Artifact` ID adds itself only; a `Relation` ID
adds its source and all target IDs; a `ReviewContext` ID adds its `member_ids`;
and an `Invariant` ID adds its `scope_ids`. No other implicit member, endpoint,
or containment expansion occurs at this step. The resulting seed set is
StableId-sorted and duplicate-free.

Only accepted directed `calls`, `covers`, and `contains` relations participate.
`calls` is caller -> callee: reverse caller traversal has depth 2 and forward
callee traversal depth 3. `covers` is test -> covered subject, so tests follow
reverse `covers` edges with cap 10. `contains` is file -> member and reverse
`contains` closure maps structural IDs to containing files. Relation scanning
is globally capped at 1,000,000 entries and reverse-contains traversal at
1,000,000 edges. No missing direction, relation kind, or endpoint is guessed.

Discovery first completes a single canonical BFS/priority traversal of all
reachable nodes within the declared depths and safety caps; path selection
happens only afterward. Queue key is `(path_length, edge_kind_direction_sequence,
path_node_stable_ids)`, with the total edge-kind/direction order exactly
`calls:forward < calls:reverse < contains:forward < contains:reverse <
covers:forward < covers:reverse`. The visited/predecessor key is exactly
`(edge_kind, direction, node_id, depth_remaining)`; for each such key retain
only the first/minimal canonical predecessor path and never enqueue a second
path for that key. Seeds have no discovered path. Thus path enumeration cannot
explode: each reached keyed node has one predecessor and one canonical path.
Seeds count toward, and all traversal queues together share,
`max_discovered_structural_ids=4096`.

Sort the non-seed canonical discovered paths by that same key and select the
first 20. Files reached only through unselected paths are successful
`path_cap` exclusions with losses; files reached by a selected path or directly
as seeds are not `path_cap`. Similarly, sort reached tests by their canonical
path key then StableId, select the first 10, and classify files reachable only
through later tests as successful `test_cap` exclusions with losses. In
contrast, exceeding the shared 4,096 discovered-ID safety cap, the 1,000,000
relation-scan safety cap, or the 1,000,000 contains-edge safety cap is
`Incomplete`; it produces no partial successful envelope.

Reached files rank by `(direct_seed_file_first, minimum_discovery_distance,
minimum_path_queue_rank, StableId)`. Greedily include files in that rank only
when all selected-byte and excerpt budgets remain feasible. The exact closed
exclusion enum is `not_reached`, `path_cap`, `test_cap`, `included_file_cap`,
`artifact_bytes_cap`, `total_resolved_bytes_cap`, `excerpt_bytes_cap`,
`total_excerpt_bytes_cap`, or `giant_line`; there is no optional detail and no
arbitrary first-file fallback. Included and excluded IDs are duplicate-free,
StableId-ordered, disjoint, and union to the complete candidate denominator.
When more than one reason can apply, the policy's exact precedence is
`path_cap > test_cap > not_reached > included_file_cap > artifact_bytes_cap >
total_resolved_bytes_cap > giant_line > excerpt_bytes_cap >
total_excerpt_bytes_cap`; the first matching tag is recorded. The same order
decides stage-crossing or otherwise impossible-in-one-stage combinations, so a
later resolver/excerpt observation never makes reason choice implementation
dependent.

The typed builder emits exactly one `SourceArtifactRef` per included file after
its staged byte and registration verification.

ADR 0013's `SourceArtifactRef` is amended with identity-bearing
`excerpt_byte_length: u64` and `excerpt_hash: ContentHash`. They are the length
and SHA-256 of the exact raw excerpt bytes after range normalization; for a
whole-file `None`, they are still the whole file's byte length and hash. Both
fields join `registration_id`, artifact/content/CAS hashes, and normalized
excerpt range in the context identity body and in canonical projection JSON.

Semantic EventLog replay/resume admits `ContextEnvelopeProjected` only through
`EventAdmissions::ContextProjectionAdmission`, a private builder-produced
admission containing the byte-reverified metadata closure, normalized ranges,
and excerpt hashes. No public caller can construct that admission. CAS-free
metadata-only replay validates recorded metadata, hashes, ranges, canonical
identity, and event integrity but does not re-prove byte semantics; it is
`OfflineProjectionState`-only and cannot enter the live aggregate map, resume
state, future execution lookup, or `EventAdmissions`. Such replay never
promotes a program fact, review claim, evidence, verification, or acceptance
state.

### 4. Exact raw-byte excerpts, unknowns, and losses

Files split only at LF byte `0x0a`; CR remains data and no text decoding occurs.
There are `count(LF)+1` one-based lines, including an empty file and the empty
final line after a trailing LF. For `Some(start_line,end_line)`, the raw range
starts at the selected first-line byte and ends immediately after the selected
last line's LF when present; therefore every selected LF is included. `None`
means the whole file and is legal **only** when the entire file fits all line
and byte bounds. A `Some(start_line,end_line)` that covers every line is
normalized to `None` before validation, serialization, hashing, or storage;
there is no stored full-file `Some` form.

For an included file, anchors are exactly accepted, range-bearing `Location`
records with `owner_id` in the reached structural-ID set and `path` exactly
equal to that containing file's path through accepted reverse-`contains`
closure. A `Location` with no range is not an anchor; no other location is an
anchor. Deduplicate and sort this set by `(start_line,end_line,StableId)`; more
than 1,024 anchors for one file is `Incomplete`. A missing/invalid owner or
path, an inverted range, or an anchor outside the file line range is a typed
domain failure. Use min start to max end if it fits. Otherwise begin at the lowest start and
take the maximum number of complete lines fitting both 400 lines and the
excerpt byte bound. With no anchor, use whole-file `None` if it fits; otherwise
use the analogous line-1 maximum complete-line prefix. If the first required
line cannot fit, the file is always excluded as `giant_line` and produces a
loss. Aggregate budgets are checked before every next file; a next file that
would exceed one is skipped with its closed exclusion reason, never partially
truncated.

Unknowns and losses are builder-derived only. `EnvelopeUnknown` is grouped
canonically by its existing `(description, sorted_source_ids)` structure. Its
description is exactly one of `context_unknown:unresolved_seed_reference`,
`context_unknown:unresolved_relation_endpoint`,
`context_unknown:unresolved_review_context_member`, or
`context_unknown:unresolved_invariant_scope`; no prose variant is legal.

`EnvelopeLoss` uses ADR 0013's existing structure and groups by
`(reason, affected_property)`, not by file. `affected_property` is the sole
input obligation's `property_id`, `severity=low`, and `source_ids` is the
StableId-sorted exact set of every affected artifact for that group. The full
literal mapping is `not_reached -> context_loss:not_reached`,
`path_cap -> context_loss:path_cap`, `test_cap -> context_loss:test_cap`,
`included_file_cap -> context_loss:included_file_cap`,
`artifact_bytes_cap -> context_loss:artifact_bytes_cap`,
`total_resolved_bytes_cap -> context_loss:total_resolved_bytes_cap`,
`excerpt_bytes_cap -> context_loss:excerpt_bytes_cap`,
`total_excerpt_bytes_cap -> context_loss:total_excerpt_bytes_cap`,
`giant_line -> context_loss:giant_line`, and truncated included windows ->
`context_loss:excerpt_window_truncated`. These mapped strings are the loss
descriptions. Groups are sorted/deduplicated; each exclusion is represented by
one group containing its artifact ID, and every truncated window contributes
its included artifact ID to the window group. `max_losses=64` bounds these
groups; a future policy that admits more than 64 distinct
`(reason, affected_property)` groups is rejected as a typed domain failure.
Source IDs may be empty only where ADR 0013 permits it; none of these bounded
file/window groups is empty. Excluded files require one or more such losses.

### 5. One envelope identity body

One private `ContextEnvelopeIdentityBody` is the sole preimage for both
`projection_hash` and `StableId::derived("context-envelope", ...)`: exact
snapshot, ordered obligation IDs, full context policy DTO/hash, candidate IDs,
ordered included registration/source/excerpt refs, ordered exclusions/reason
tags, canonical unknown groups, fixed assumptions, and canonical loss groups.
Each output excludes only itself. The canonical envelope bytes are bounded to
786,432; no alternate narrow ID body exists. This final envelope cap applies
after candidate IDs, the complete partition, and loss groups are assembled;
even individually legal components that exceed it in combination are typed
`Incomplete`, never a truncated envelope.

Every D1 `review_plan_recorded` or `context_envelope_projected` outer canonical
event JSONL record is admission-checked with checked addition as
`canonical_json(EventEnvelope).len() + 1 <= 1_048_576`; the added byte is the
required trailing LF. Arithmetic overflow or a false predicate is typed
`Incomplete`. A configured store limit below that also applies. The
786,432-byte inner plan/envelope cap is therefore necessary but does not waive
outer-event admission.

Canonical serialization of `PlannerInput`, `ReviewPlan`, `ContextIdentity`, the
full `ContextEnvelope`, the full `EventEnvelope`, and every full-record
`body_hash` preimage uses a bounded canonical writer or an exact checked-size
proof before allocation. `serde_json::to_value` (or any unbounded intermediate
tree/serialization) is forbidden for all of those paths. The applicable inner
786,432-byte cap or outer EventContract/StoreLimits bound is checked during
that serialization, not after materializing bytes.

### 6. Exact C3 schema-v2 follow-up

[ADR 0017](0017-d1-derived-index-schema-v2.md) closes this follow-up's exact
schema, component preimages, version-1 rebuild policy, and fixtures. ADR 0017
governs storage-detail conflicts; this ADR remains authoritative for planner
and context canonical bodies and identities.

Adding plan/context events changes the derived-index projection contract literal
to `reviewgraphen.index_projection.v2`, `PRAGMA user_version = 2`, and the
literal `index_meta.index_schema_version = 2`. Existing v1 images are
rebuild-required, never migrated in place. V2 adds exactly
`review_plan_recorded` and `context_envelope_projected` to `events.payload_kind`
CHECK. `review_plans` has `event_sequence INTEGER NOT NULL CHECK
(event_sequence > 0)`, `event_id TEXT NOT NULL`, `plan_id TEXT NOT NULL`,
`universe_id TEXT NOT NULL`, `snapshot_id TEXT NOT NULL`,
`planner_input_hash TEXT NOT NULL`, `planner_policy_version TEXT NOT NULL`,
`planner_policy_hash TEXT NOT NULL`, `budget_canonical_json TEXT NOT NULL`,
`risk_breakdown_canonical_json TEXT NOT NULL`,
`waves_canonical_json TEXT NOT NULL`, `deferred_canonical_json TEXT NOT NULL`,
`identity_body_hash TEXT NOT NULL`, and `body_hash TEXT NOT NULL`; its constraints are primary key
`(event_sequence,plan_id)`, unique `plan_id`, and foreign key
`(event_sequence,event_id)` to
`events(sequence,event_id)`. `context_envelopes` has `event_sequence INTEGER
NOT NULL CHECK (event_sequence > 0)`, `event_id TEXT NOT NULL`, `envelope_id
TEXT NOT NULL`, `snapshot_id TEXT NOT NULL`, `context_policy_version TEXT NOT
NULL`, `context_policy_hash TEXT NOT NULL`, `candidate_ids_canonical_json TEXT
NOT NULL`, `obligation_ids_canonical_json TEXT NOT NULL`,
`context_policy_canonical_json TEXT NOT NULL`,
`included_sources_canonical_json TEXT NOT NULL`,
`excluded_sources_canonical_json TEXT NOT NULL`, `unknowns_canonical_json TEXT
NOT NULL`, `assumptions_canonical_json TEXT NOT NULL`,
`losses_canonical_json TEXT NOT NULL`, `projection_hash TEXT NOT NULL`, and
`body_hash TEXT NOT NULL`;
its constraints are primary key `(event_sequence,envelope_id)`, unique
`envelope_id`, and the same composite event FK. All tables remain `STRICT`;
exact ordered query keys are `(event_sequence,plan_id)` and
`(event_sequence,envelope_id)`.

In both tables, existing `body_hash` means the hash of the full serialized
record, never an identity-body hash. `context_envelopes.projection_hash` is the
hash of its complete context identity body. `review_plans.identity_body_hash`
is the hash of its complete plan identity body, kept distinct from `body_hash`.
`review_plans` reconstructs the complete serialized `ReviewPlan` from exactly
`plan_id`, `universe_id`, `snapshot_id`, planner policy version/hash,
`planner_input_hash`, `budget_canonical_json`,
`risk_breakdown_canonical_json`, `waves_canonical_json`, and
`deferred_canonical_json`; `risk_breakdown_canonical_json` contains only each
obligation's deterministic impact and likelihood (no rationale). Wave IDs are
re-derived from `plan_id`, `wave_index`, and `obligation_ids`, never projected
as separate fields. Thus all full-record fields required for `body_hash` are
enumerated and reconstructible.
`waves_canonical_json` is exactly the ordered JSON array of identity wave
contents `{"wave_index": u32, "obligation_ids": [StableId...]}`; it never
stores a wave ID or reason. Rebuild preflight verifies both version-2 literals,
canonical JSON byte equality, that `review_plans.identity_body_hash` equals its
reconstructed canonical identity body (including those exact wave contents),
that `context_envelopes.projection_hash` equals its reconstructed canonical
identity body, and that each `body_hash` equals the full serialized record hash;
it then checks closed reason tags before insertion. Query
re-decodes/re-canonicalizes every JSON column and rechecks all applicable
hashes. Fixtures cover v1-index rebuild refusal, v2 plan/context projection,
unknown/loss source grouping, outer-event admission, canonical-path/backfill
behavior, and canonical-byte mutation.
Until this exact projection ships, C3 fails closed on these payloads.

### 7. Deferred work and alternatives

Execution/reviewer/report payloads and v1 replay changes remain a later
amendment. Rejected alternatives are numeric weighted scores, full-repository
default context, CAS I/O in core, lossy text excerpts, optional reason details,
and turning cycles/dangling dependencies into ordinary deferrals.

## Golden policy record

The exact §1 canonical JSON is 3,392 UTF-8 bytes and has SHA-256:

```text
sha256:5243db119b42b57d8f3e0d418e9eb202acdd43f8861f7834b9e74ef559a498fa
```

## Acceptance tests

1. The typed DTO bytes and golden hash match exactly.
2. Planner input IEEE-bit encoding, planner/context string bounds, bounded
   serialization, plan/wave identity acyclicity, absent wave reasons, empty
   plan, lifecycle precondition, limits, cycles, dangling IDs, and Kahn wave
   rules are deterministic and typed.
3. Exact seed-kind expansion, directed relation closure, single-predecessor
   canonical BFS/PQ paths keyed by edge kind/direction/depth and their total
   edge order, successful path/test selection caps, incomplete safety caps,
   metadata-first rank-order resolve/backfill, exact exclusion precedence, and
   every exclusion tag match.
4. LF/CR/non-UTF8/trailing-LF/range-bearing owner/path anchor selection and
   anchor cap, anchor range/window/full-None normalization/giant-line, and
   aggregate byte behavior preserve exact raw bytes and losses.
5. Candidate metadata/byte registration failures, rank-order backfill,
   excerpt length/hash integrity, private live admission, metadata-only offline
   replay, partition/order/duplicate failures, exact grouped unknown/loss
   literals, bounded writers, final envelope byte cap, and outer JSONL
   admission yield no partial successful record.
6. C3 v1 rebuild refusal and v2 literal DDL/risk-breakdown/full-record versus
   identity hash/preflight/query fixtures pass.
