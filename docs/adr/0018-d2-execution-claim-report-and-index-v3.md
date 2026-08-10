# ADR 0018: D2 Execution, Claim, Report, and Index Schema V3

- Status: Accepted
- Date: 2026-08-10
- Scope: closes the implementation contract for the execution portion of
  ADR 0013 that ADR 0016 deliberately left out of D1. It amends ADR 0013's
  execution DTO, outcome spelling, trace, bounds, atomicity, and claim shape;
  amends ADR 0017 with derived-index schema version 3; and defines
  `reviewgraphen.review.report.v2`. It does not add a real provider, a process
  reviewer, tool execution, verification authority, evidence reconciliation,
  a gate, or a CLI.

## Context

ADR 0013 correctly chose one atomic `ReviewExecutionRecorded { execution,
claims }` event and reserved it in `reviewgraphen.review_event.v2`, but its
DTO is not yet complete enough to implement safely:

- the prose requires system-prompt version and an always-present tool-call
  record, but its example `ExecutionRecord` has neither field;
- `Completed` is easy to confuse with lifecycle `Completed`; the closed
  execution result needs an output-specific spelling;
- the current M1 `ReviewClaim` has no `property_id`, `target_refs`,
  `assumptions`, or `requested_evidence`, although ADR 0013's reviewer output
  requires all four;
- identities, canonical ordering, collision behavior, byte/count bounds, CAS
  publication order, and crash boundaries remain underspecified;
- ADR 0017 intentionally excludes execution from index schema version 2; and
- report v1 has unresolved/sentinel execution metadata and cannot preserve the
  D2 trace or full claim body.

These are contract gaps, not permission to infer missing metadata. Program
facts, review claims, evidence, verification, and decisions remain separate.
Neither a fake reviewer nor a high confidence value grants authority.

## Decision

### 1. D2 boundary and version decisions

D2 implements only the deterministic, fixture-backed fake reviewer. Its
descriptor is:

```text
reviewer_kind          = fake
reviewer_id            = reviewgraphen.fake_reviewer@1
provider               = null
model                  = null
model_revision         = null
system_prompt_version  = reviewgraphen.system.no_tools@1
prompt_template_version= fixture@1
inference_settings     = {}
tool_policy_version    = reviewgraphen.tool_policy.none@1
tool_calls             = []
```

The repository contains no D2 provider/network/process adapter and no tool
dispatcher. D2 event construction/apply accepts only the exact descriptor
above; provider-bearing execution requires a later ADR/version decision. A nonempty
`tool_calls`, a different tool policy, or an attempted tool capability is a
typed policy failure before an execution event is appended.

D2 continues to use `reviewgraphen.review_event.v2`. ADR 0013 already made
`ReviewExecutionRecorded` a normative v2 payload and made v1 read-only. This
ADR closes an unimplemented v2 reservation; it does not widen v1 and therefore
does not invent event v3. Old binaries fail forward on the new payload as
expected. A stream remains homogeneous: it is wholly v1 or wholly v2.

`ClaimProposed` is legacy-v1 replay only. A v2 command constructor cannot mint
it. Its historical JSON, ID, event payload hash, event hash, aggregate result,
and report-v1 projection remain byte-exact. D2 claims exist only inside the
atomic v2 payload below.

### 2. Closed execution record and outcome

The canonical D2 record has exactly these fields:

```rust
ExecutionRecord {
  id, plan_id, wave_id, obligation_ids, envelope_id, snapshot_id,
  reviewer_kind, reviewer_id, provider, model, model_revision,
  system_prompt_version, prompt_template_version, inference_settings,
  tool_policy_version, tool_calls, attempt,
  raw_artifact_registration_id, raw_artifact_hash,
  parsed_claim_ids, outcome
}
```

`provider`, `model`, and `model_revision` are present JSON fields and use
explicit `null`; any string value is outside D2. `tool_calls` is an
always-present array and is exactly `[]`. Empty/missing/null are not
interchangeable for any other trace field. `snapshot_id` equals both the plan
and envelope snapshot.

The closed wire outcomes are:

```json
{"kind":"structured"}
{"detail":"...","kind":"abstained","reason":"insufficient_context"}
{"diagnostic":"...","kind":"malformed","reason":"schema_violation"}
{"diagnostic":"...","kind":"provider_failure","retryable":true}
```

`structured` replaces ADR 0013's execution-outcome wire spelling
`completed`; lifecycle `completed` keeps its existing spelling. The other
outcome tags are `abstained`, `malformed`, and `provider_failure`. The eight
abstention reasons and malformed-output reasons remain the closed enums in
ADR 0013; unknown strings are rejected, never downgraded to prose. Outcome
detail/diagnostic is descriptive and grants no authority.

The lifecycle is exact:

```text
Generated -> Planned -> InProgress
InProgress + structured execution -> Completed (a later transition event)
InProgress + abstained/malformed/provider_failure -> InProgress
```

The execution event never changes lifecycle implicitly. Before an attempt the
obligation is already `InProgress`. Only after an atomic nonempty structured
event is durable may a separate `ObligationTransition` move every addressed
obligation to `Completed`. Aggregate replay rejects a completion transition
without such an earlier execution. An abstention or failure is visited but not
completed, verified, accepted, or a no-issue conclusion.

### 3. Full D2 claim and legacy separation

The canonical D2 claim has exactly:

```rust
ExecutionClaimV2 {
  id, execution_id, obligation_ids, property_id, target_refs, polarity,
  disposition, summary, source_ids, assumptions, requested_evidence,
  candidate_confidence, author_kind, review_status
}
```

Every newly recorded D2 claim begins with `disposition = proposed`,
`author_kind = ai`, and `review_status = unreviewed`. Confidence is `null` or
a finite JSON number in `[0,1]`; it is descriptive and is never an ID input,
gate, verification, or acceptance signal. The fake is a test double for the
reviewer boundary, not a deterministic fact extractor.

Core uses version-specific strict DTO decoding. The old body is named
`LegacyClaimV1` at the v1 `ClaimProposed` boundary. It is never deserialized
through `ExecutionClaimV2`, populated with guessed fields, or reserialized in
the new shape. Aggregate query APIs may expose an explicitly versioned common
view, but canonical serialization always returns to the original DTO. There
is no automatic v1-claim-to-D2-claim migration.

For D2's one-obligation envelope baseline:

- `obligation_ids` is the exact one-element execution set;
- `property_id` equals that obligation's property ID;
- `target_refs` is nonempty and each member occurs in that obligation's
  normalized target set;
- `source_ids` is nonempty and is a subset of the envelope's
  `normalized_included_source_ids` (artifact IDs, not uncaptured symbols);
- `assumptions` and `requested_evidence` are explicit arrays, including `[]`;
  neither is evidence or verification; and
- `polarity` is the existing closed claim polarity, including
  `issue_absent` for a structured no-issue result.

### 4. Atomic event and collision behavior

The only D2 write is:

```rust
ReviewExecutionRecorded {
  execution: ExecutionRecord,
  claims: Vec<ExecutionClaimV2>,
}
```

Before append, core validates the complete payload against a cloned aggregate.
For `structured`, claims contains 1 through 16 records and
`parsed_claim_ids` equals their exact ID set. For every other outcome both are
empty. Every claim points to the execution and satisfies §3. Claims are sorted
strictly by claim ID. Any error rejects the whole event; no execution, claim,
lifecycle change, `ReviewExecutionRecorded` journal line, or execution/claim
index row becomes visible. This does not erase an earlier step-5
`ArtifactRegistered`: if raw registration already committed, it remains the
auditable unreferenced registration described in §7 and is never rolled back
or misreported as part of the rejected atomic execution event.

Canonical event IDs are unique commands, not idempotency keys. A second
canonical event for an already-recorded execution or claim ID returns typed
`IdCollision`, even if the repeated bytes are identical. Resume and whole-run
replay apply the same rule. This supersedes ADR 0013 §6a item 7's identical
reapply wording for v2 execution events; historical v1 replay is unchanged.

### 5. Exact IDs, hashes, and ordering

Canonical JSON uses UTF-8, lexicographically ordered object keys, no
whitespace, shortest legal JSON number spelling, and the existing StableId
ordering. Set-like arrays are strictly sorted and unique. Map keys are
lexicographic. Claims are ordered by claim ID. No implementation hashes a
`serde_json::Value` tree or SQLite JSON output.

Every byte limit in this ADR is measured on actual UTF-8 bytes, never Unicode
scalar count or a conservative character multiplier. In addition to the
field-specific limits below, every string value and object key owned by a v2
report has an inclusive 16,384-byte ceiling. Thus 4,096 repetitions of `界`
are valid as 4,096 JSON Schema characters but are rejected for a 4,096-byte
outcome field because their UTF-8 representation is 12,288 bytes.

Input decoding rejects the non-standard JSON constants `NaN`, `Infinity`, and
`-Infinity`; canonical encoding enables no extension that can emit them. Every
floating-point value admitted anywhere in a D2 DTO is finite and in `[0,1]`.
At present the only such D2 wire field is `candidate_confidence`; future float
fields must either retain this domain or introduce an explicit versioned
contract.

The execution identity preimage is the canonical object with exactly:

```text
attempt, envelope_id, inference_settings, model, model_revision,
obligation_ids, plan_id, prompt_template_version, provider, reviewer_id,
reviewer_kind, snapshot_id, system_prompt_version, tool_policy_version,
wave_id
```

The explicit null model fields participate. Output, raw registration/hash,
tool calls (fixed empty), parsed claims, and outcome do not. Define:

```text
execution_identity_body_hash = SHA-256(identity bytes)
execution.id = StableId::derived("execution", &identity_body)
```

`attempt` is positive. Retrying the same immutable input increments attempt
and therefore changes the ID. Reusing an attempt cannot replace its output.

The D2 claim identity preimage has exactly:

```text
assumptions, execution_id, obligation_ids, polarity, property_id,
requested_evidence, source_ids, summary, target_refs
```

`candidate_confidence`, disposition, author kind, review status, and ID are
excluded. Define `claim_identity_body_hash` and `claim.id` analogously with
`StableId::derived("claim", &identity_body)`. Full execution/claim `body_hash` values are
SHA-256 of their complete canonical records; the ID field participates in the
full body and not the identity body. Event payload/event hashes retain their
existing distinct ADR 0017 preimages. Equality of any two hashes is never
assumed.

### 6. Bounds and typed failures

The following inclusive D2 limits are part of the contract:

| Item | Limit |
|---|---:|
| claims per structured execution | 16 |
| target refs / claim | 64 |
| source IDs / claim | 128 |
| assumptions / claim | 32 |
| requested-evidence entries / claim | 32 |
| inference entries | 32 |
| reviewer/trace/inference key bytes | 256 |
| inference value bytes | 1,024 |
| summary bytes | 8,192 |
| assumption/requested-evidence item bytes | 2,048 |
| outcome detail/diagnostic bytes | 4,096 |
| canonical claim bytes | 32,768 |
| canonical execution bytes | 131,072 |
| raw reviewer bytes | 1,048,576 |
| resolved request source bytes | 8,388,608 |
| simultaneous reviewer-stage working bytes | 16,777,216 |
| canonical event JSON plus its one LF | 1,048,576 |

The CAS/store `max_object_bytes` and `max_event_line_bytes` also apply; the
effective limit is the smaller bound. Source bytes reuse ADR 0016's exact
8 MiB envelope resolution maximum. The working charge includes every retained
full source buffer, excerpt buffer, request/response vector capacity, raw
bytes, parser DTO/string capacity, canonical writer scratch, and pending
execution/claim/event bytes. It is checked at each ownership-stage peak.

All counts and capacities are reserved and charged before allocation with
checked `u64` addition/multiplication. Overflow reports observed `u64::MAX`.
Limit exhaustion is a closed typed `Incomplete { operation, limit, observed }`
(or exactly equivalent typed domain error), never truncation, partial claims,
panic, unbounded `serde_json::to_value`, or a generic error string. Fixtures
exercise each exact limit, limit+1, arithmetic overflow, and aggregate
working-set peak.

### 7. Raw artifact, source closure, and append order

Every attempt, including fake abstention/malformed/provider failure, produces
exact raw bytes. The trusted orchestrator performs this order:

1. derive the execution ID from immutable input and attempt;
2. resolve the envelope's registrations from CAS and construct the bounded,
   byte-validated `ReviewerRequest`;
3. invoke only the deterministic fake reviewer;
4. enforce raw/working/store bounds and write the exact raw bytes to CAS;
5. append `ArtifactRegistered` with sensitivity `Sensitive`, the exact hash,
   size/media type, and source `ReviewerExecution { run_id, execution_id,
   reviewer_id }`;
6. construct and validate the execution/claims against that registration;
7. append the one atomic `ReviewExecutionRecorded`; and
8. for a structured result only, append the separate lifecycle transition.

The execution event must resolve its registration, whose hash equals
`raw_artifact_hash`, sensitivity is `Sensitive`, source execution/reviewer/run
tuple is exact, and CAS bytes hash and size match. CAS bytes alone are not a
canonical observation. A crash after CAS write but before registration leaves
an unreferenced GC candidate. A crash after registration but before execution
leaves an auditable registration and an incomplete run. Neither is rolled back
or converted into a fabricated execution. A crash after execution but before
lifecycle completion is resumed by aggregate state: it may append the missing
valid transition, but must not invoke the reviewer again with the same attempt.

The registration identity preimage has exactly `run_id`, `cas_hash`,
`media_type`, `sensitivity`, and the complete `source` object; its ID is
`StableId::derived("registration", &identity_body)`. The outer `run_id`, the
reviewer-execution source `run_id`, and the enclosing report metadata run ID
must be identical. Stable registration, execution, claim, event, and CAS
source IDs are checked for uniqueness before construction of any map or set.

### 8. Restart, offline replay, and authority

Execution records and D2 claims are authority-free observations/proposals.
They contain no live admission, capability, verifier, acceptance, or signing
token. Restart reconstructs live context admission only by resolving the exact
registered CAS bytes and re-running ADR 0016's validation. Metadata-only replay
may build an offline projection, but cannot resume a reviewer from unchecked
workspace bytes.

Evidence, bindings, verification, decisions, and findings remain
`unreconciled_authority_records`/shadow projections under ADR 0015. D2 does
not infer authority from a matching execution, structured outcome, confidence,
fake-reviewer determinism, or report shape.

### 9. Derived index schema version 3

D2 uses exactly:

```text
PRAGMA user_version                    = 3
index_meta.index_schema_version        = 3
index_meta.projection_contract_version = reviewgraphen.index_projection.v3
```

Schema v3 is ADR 0017's complete schema-v2 DDL with those three markers, the
following `events.payload_kind` check, a new `executions` table, and the exact
replacement `claims` table below. No other schema-v2 table or constraint
changes. The old narrow claims DDL is not also created.

```sql
CHECK (payload_kind IN (
  'obligation_transition', 'claim_proposed', 'evidence_recorded',
  'evidence_bound', 'verification_recorded', 'decision_recorded',
  'finding_recorded', 'run_genesis_manifest', 'artifact_registered',
  'snapshot_sources_recorded', 'review_plan_recorded',
  'context_envelope_projected', 'review_execution_recorded'
))

CREATE TABLE executions (
  event_sequence                    INTEGER NOT NULL CHECK (event_sequence > 0),
  event_id                          TEXT NOT NULL,
  execution_id                      TEXT NOT NULL UNIQUE,
  plan_id                           TEXT NOT NULL,
  wave_id                           TEXT NOT NULL,
  snapshot_id                       TEXT NOT NULL,
  envelope_id                       TEXT NOT NULL,
  obligation_ids_canonical_json     TEXT NOT NULL,
  reviewer_kind                     TEXT NOT NULL CHECK (reviewer_kind = 'fake'),
  reviewer_id                       TEXT NOT NULL
    CHECK (reviewer_id = 'reviewgraphen.fake_reviewer@1'),
  provider                          TEXT,
  model                             TEXT,
  model_revision                    TEXT,
  system_prompt_version             TEXT NOT NULL
    CHECK (system_prompt_version = 'reviewgraphen.system.no_tools@1'),
  prompt_template_version           TEXT NOT NULL
    CHECK (prompt_template_version = 'fixture@1'),
  inference_settings_canonical_json TEXT NOT NULL
    CHECK (inference_settings_canonical_json = '{}'),
  tool_policy_version               TEXT NOT NULL
    CHECK (tool_policy_version = 'reviewgraphen.tool_policy.none@1'),
  tool_calls_canonical_json          TEXT NOT NULL
    CHECK (tool_calls_canonical_json = '[]'),
  attempt                            INTEGER NOT NULL CHECK (attempt > 0),
  raw_registration_id               TEXT NOT NULL,
  raw_hash                           TEXT NOT NULL,
  parsed_claim_ids_canonical_json    TEXT NOT NULL,
  outcome_kind                      TEXT NOT NULL CHECK (outcome_kind IN (
    'structured', 'abstained', 'malformed', 'provider_failure'
  )),
  outcome_canonical_json             TEXT NOT NULL,
  identity_body_hash                 TEXT NOT NULL,
  body_hash                          TEXT NOT NULL,
  PRIMARY KEY (event_sequence, execution_id),
  FOREIGN KEY (event_sequence, event_id)
    REFERENCES events (sequence, event_id),
  FOREIGN KEY (plan_id) REFERENCES review_plans (plan_id),
  FOREIGN KEY (envelope_id) REFERENCES context_envelopes (envelope_id),
  FOREIGN KEY (raw_registration_id)
    REFERENCES artifact_registrations (registration_id),
  CHECK (provider IS NULL AND model IS NULL AND model_revision IS NULL),
  CHECK (attempt <= 4294967295)
) STRICT;

CREATE TABLE claims (
  event_sequence                      INTEGER NOT NULL CHECK (event_sequence > 0),
  event_id                            TEXT NOT NULL,
  claim_id                            TEXT NOT NULL UNIQUE,
  execution_id                        TEXT NOT NULL,
  obligation_ids_canonical_json       TEXT NOT NULL,
  property_id                         TEXT NOT NULL,
  target_refs_canonical_json          TEXT NOT NULL,
  polarity                            TEXT NOT NULL CHECK (polarity IN (
    'issue_present', 'issue_absent', 'inconclusive', 'not_applicable',
    'conflict'
  )),
  disposition                         TEXT NOT NULL CHECK (disposition = 'proposed'),
  summary                             TEXT NOT NULL,
  source_ids_canonical_json           TEXT NOT NULL,
  assumptions_canonical_json          TEXT NOT NULL,
  requested_evidence_canonical_json   TEXT NOT NULL,
  candidate_confidence_canonical_json TEXT NOT NULL,
  author_kind                         TEXT NOT NULL CHECK (author_kind = 'ai'),
  review_status                       TEXT NOT NULL CHECK (review_status = 'unreviewed'),
  identity_body_hash                  TEXT NOT NULL,
  body_hash                           TEXT NOT NULL,
  PRIMARY KEY (event_sequence, claim_id),
  FOREIGN KEY (event_sequence, event_id)
    REFERENCES events (sequence, event_id),
  FOREIGN KEY (execution_id) REFERENCES executions (execution_id)
) STRICT;
```

The schema-v2 narrow claims projection is replaced, not supplemented. A v1
journal is metadata-only and therefore has no claim rows to migrate; every v2
D2 claim has the full body above. Every execution and all its claims use the
same event sequence/event ID and are inserted in one SQLite transaction only
after full prevalidation. SQL uniqueness/FKs are backstops, not the domain
definition.

Every `*_canonical_json` value follows ADR 0017's byte-equality rule.
`candidate_confidence_canonical_json` is exactly ASCII `null` or the canonical
finite JSON number; `outcome_canonical_json` is exactly one of §2's complete
objects. Query decodes, bounded-reserializes, and requires identical bytes.

Schema-version-1 and schema-version-2 SQLite images both return
`RebuildRequired { found, required: 3 }`. There is no `ALTER TABLE`, row copy,
or in-place upcast. Rebuild reads only confirmed canonical JSONL/CAS inputs
into a fresh v3 image. Historical event-v1 journals rebuild metadata-only;
event-v2 D1 journals rebuild with empty execution tables; event-v2 D2 journals
project all complete rows.

The v3 `IndexSnapshot` adds complete `executions` ordered by
`(event_sequence, execution_id)` and full `claims` ordered by
`(event_sequence, claim_id)`. It reconstructs and revalidates each full body,
identity, hash, array/map canonical bytes, outcome/claim pairing, registration
closure, same-event atomicity, and plan/envelope/obligation/property/source
relationships. `max_index_rows` counts both tables; every returned byte and
element counts toward `max_index_query_bytes`; ADR 0017's journal view,
deserialize/query peak formulas and `max_index_working_bytes` apply unchanged.
Exact-limit and limit+1 fixtures include nonempty D2 tables. No partial
snapshot is returned.

### 10. Review report v2

The new additive schema is `reviewgraphen.review.report.v2`. Report v1 and its
fixture remain frozen; the generic v1 path is not silently repointed. A v2
report is a projection of a confirmed event/CAS/index view, never canonical
state.

Validation has two explicitly different layers. The current bundle's local
gate validates JSON
Schema, canonical ordering, exact report-local ID/body preimages, status,
authority, and internal references. It receives only a report and therefore
must not claim that a tail, denominator, plan, CAS object, lifecycle, or
attempt set is confirmed. A future runtime source-bound cross-validator must
receive canonical typed event/CAS/index/universe/plan/envelope inputs from the
actual core/store implementation. Only that implementation may report source
proof, after recomputing the event chain/tail, raw byte hashes and sizes,
index projection, exact attempt set, denominator, lifecycle coverage, and
every closure.

The checked-in [claim-free report](../../schemas/reviewgraphen.report.v2.example.json)
is deliberately a local schema/self-consistency example only. There is no
checked-in source-bound fixture and the bundle validator makes no canonical
source, confirmed-tail, or CAS-presence claim. A source fixture becomes valid
only when generated from actual core canonical output and rebuilt by the
actual v3 index implementation. Any claim in that future fixture must belong
to a real nonempty structured `ReviewExecutionRecorded` atomic event and must
appear consistently in the journal, index, and report; detached claim test
vectors are forbidden.

Every v2 report also projects the complete raw `ArtifactRegistered` body
(registration/run/CAS hash/media type/size/sensitivity/source tuple), so its
execution reference can be checked without a sentinel. Every execution
projection contains the complete §2 trace plus
`execution_identity_body_hash` and `body_hash`. Its `outcome.kind` is exactly
`structured|abstained|malformed|provider_failure`; report-level status is
separate and has the four reserved wire values
`completed|partial|unsupported_input|failed`. D2 generation uses this exclusive
decision table:

| Status | Exact D2 generation condition |
|---|---|
| `completed` | The selected set is nonempty; at least one attempt exists; every selected obligation has at least one `structured` execution whose nonempty claim set is exact, and a later lifecycle `Completed` transition. Earlier abstained/malformed/provider-failure retries may remain in the complete attempt set. |
| `partial` | At least one selected obligation has an attempt, but the lifecycle-completed set is a strict subset of the selected set. An `abstained`, `malformed`, or `provider_failure` attempt never adds its obligation to completed; a later structured attempt may do so. |
| `unsupported_input` | The selected set is nonempty, no reviewer attempt occurred, and executions, claims, referenced raw registrations, visited IDs, and completed IDs are all empty. One or more `pre_review_unsupported_input` obstructions have nonempty source IDs and their combined `blocks` set equals the selected set. |
| `failed` | Reserved for report-schema evolution only. D2 has no legitimate generation condition; its generator and cross-record validator always reject it. A reviewer `provider_failure` is an execution outcome and yields `partial`, never report `failed`. A later ADR must define any report-generation failure record before enabling this status. |

Mechanically: reject `failed`; otherwise choose `unsupported_input` only for
the exact zero-attempt obstruction case; with attempts choose `completed` only
when completed IDs equal selected IDs, otherwise choose `partial`. No input can
satisfy two rows. Every view payload status must equal the chosen result
status.

`tool_policy_version` and `tool_calls` are required. Provider/model/revision
are explicit null fields in D2. No report uses unknown/unresolved sentinels for
a D2 execution. Every D2 claim projection contains every §3 field and is
unconditionally `proposed`/`unreviewed`; D2 report v2 never projects a later
claim disposition or review status. `authority_reconciled` is exactly false,
the evidence/verification/decision/finding ID arrays are exactly empty, and
verified/accepted coverage is exactly zero. Authority-bearing event shadows,
if present, remain outside this authority-free report contract.

No D2 report contains a finding or severity. In particular it must not derive
severity from confidence, polarity, risk, graph centrality, or reviewer kind.
A later finding/report contract must project an explicit canonical finding and
its authority trace instead of synthesizing one here.

Every projection view contains nonempty `source_ids` and an always-present
`information_loss` array. Each loss has `kind`, `reason`, nonempty
`source_ids`, `affected_properties`, `meaningful`, `recoverable`, and a
`recovery_ref` exactly when recoverable. Every current view payload is a
strict summary of the full report, so `information_loss` is nonempty and every
entry has `meaningful = true`. An empty or omitted loss array is invalid even
when the payload happens to contain no claims. Source IDs resolve to
records in the report or to the report's declared external ProgramSpace,
universe, obligation, artifact-registration, or CAS source set.

The report binds itself to the confirmed tail with `genesis_hash`,
`confirmed_offset`, `confirmed_tail_hash`, and `confirmed_event_count`. For the
scenario plan and selected obligations it contains the exact set of every D2
attempt at or before that tail, including failed/retried attempts. It is not a
latest-attempt view. `claims` is exactly the union of every projected
execution's `parsed_claim_ids` and contains no other claim. The projected raw
registrations and `scenario.artifact_registration_ids` are both exactly the
set referenced by those executions. An earlier unreferenced registration from
§7 remains in canonical history but is not smuggled into the referenced set.
Consequently a source-bound validator distinguishes all canonical artifact
registrations from the report's exact execution-raw subset; equality between
those two sets is neither required nor generally correct.

Top-level record arrays and the projection-view array use these exact ascending
keys:

```text
artifact_registrations: (event_sequence, registration_id)
executions:              (event_sequence, execution_id)
claims:                  (event_sequence, claim_id)
obstructions:            canonical JSON bytes of the complete obstruction
projection.views:        human, ci, machine enum order
```

`information_loss` exists only inside an individual projection view. Within
each view independently it is ascending and duplicate-free by `(kind, reason,
source_ids, affected_properties)`. Deduplication never crosses view
boundaries: the same declared loss may legitimately appear once in each of
two different views. All other arrays are duplicate-free. Every nested ID set
is StableId ascending and unique; every string/property set is UTF-8 lexical
ascending and unique. An omission, duplicate, or reorder is a report-contract
failure, not a request to sort or deduplicate untrusted input.

The report cross-record validator, beyond JSON Schema, requires:

1. confirmed-tail marker equality and the exact attempt set described above;
2. unique/canonical ordering for every top-level and nested array;
3. the complete claim union and exact referenced-registration sets;
4. plan/wave/envelope/snapshot/obligation closure and one-obligation D2 scope;
5. exact structured/non-structured claim cardinality;
6. claim execution/property/target/source closure from §§3-4;
7. raw registration/hash/source/sensitivity closure to confirmed CAS metadata;
8. body and identity hash recomputation from the exact preimages;
9. denominator equality with the recorded universe; `selected` equals the
   selected-ID count, `visited` the selected IDs with at least one attempt, and
   `completed` the selected IDs with both a structured attempt and lifecycle
   completion; the three ID sets and counts must agree exactly; lifecycle is
   derived solely by typed replay of every journal `ObligationTransition` in
   sequence—never from a redundant sidecar list;
10. result status agrees with those lifecycle facts and every view payload's
    status equals `result.status`;
11. projection source resolution and nonempty information-loss consistency;
    and
12. claims fixed proposed/unreviewed, authority arrays empty,
    `authority_reconciled=false`, and verified/accepted coverage zero, with no
    authority or severity inferred from execution or claim fields.

The inclusive report construction limits are:

| Item | Limit |
|---|---:|
| executions | 8,192 |
| referenced raw registrations | 8,192 |
| claims | 131,072 |
| obstructions | 8,192 |
| projection views | 3 |
| sum of per-view information-loss records | 4,096 |
| report rows | 200,000 |
| canonical report bytes | 67,108,864 (64 MiB) |
| simultaneous report working bytes | 268,435,456 (256 MiB) |

`report_rows` is exactly the checked sum of registrations, executions, claims,
obstructions, views, and loss records; nested IDs and strings count toward the
byte/working limits rather than the row count. The canonical-byte preimage is
the complete top-level report object, including projection payloads and tail
markers, recursively key-sorted and serialized with no whitespace or trailing
LF. There is no alternate narrow sizing body.

Let `J` be the sum of every canonical confirmed-tail event byte sequence plus
its one LF, `I` the canonical byte length of the complete v3 index snapshot,
`Rr` the preflight reservation for every report-owned vector/string/scalar
capacity, `R` the realized report ownership, `S` the largest canonical byte
length of any complete registration/execution/claim/obstruction/view record,
and `O` the canonical byte length of the complete output report. `Rr` and `R`
use the same deterministic ownership charge: actual UTF-8 bytes for strings
and keys, 8 bytes per list slot, 16 bytes per object entry, 8 bytes per
integer/float, 1 byte per Boolean, and 0 for null, recursively summed with
checked `u64`. The implementation checks these simultaneous peaks:

```text
projection_peak   = J + I + Rr
serialization_peak= J + I + R + S + O
```

These are normative runtime accounting formulas, not `canonical_length * 4`
or another scalar estimate. Changed lifetimes add, never subtract, simultaneously
owned buffers. Counts, capacities, UTF-8 bytes, and both formulas use checked
`u64` arithmetic before reserve/allocation; overflow reports observed
`u64::MAX`. Runtime exact/+1 fixtures must construct actual `J`, `I`, `Rr`,
`R`, `S`, and `O` through the bounded Rust builders and prove refusal occurs
before reserve, serialization, or output allocation. The Python bundle script
is only a checked-add/multiply and serialized-example reference oracle; Python
container capacity is not a normative model of Rust allocation and the script
does not claim these runtime proofs. Exceeding rows,
canonical bytes, or either working peak returns typed `Incomplete { operation,
limit, observed }` and no report bytes or partial projection. Unbounded
`serde_json::to_value` is forbidden.

Schema-valid but cross-record-invalid input yields no report. Report v1 cannot
be losslessly upcast because it lacks the complete trace and claim body. A v2
report is regenerated from confirmed canonical event/CAS data; it is never
made by filling missing fields with sentinels or model prose.

### 11. Implementation Definition of Done

The D2 implementation unit must include only deterministic fake fixtures:

1. structured issue-present and issue-absent responses;
2. all eight abstention reasons, every malformed reason, and provider failure
   with both retryability values;
3. prompt-injection bytes treated solely as source data;
4. no-tools enforcement and exact empty `tool_calls` in event/index/report;
5. all bounds at exact and +1, checked overflow, and combined working peak;
6. atomic failures for every claim/event cross-reference and collision;
7. CAS/register/event/lifecycle crash points and deterministic restart;
8. retry attempts producing distinct IDs while attempt reuse is refused;
9. v1 byte-exact replay, v2 D1 empty execution projection, and v1/v2 SQLite
   rebuild-required-to-v3 cases;
10. two rebuilds yielding equal ordered v3 snapshots;
11. a local-only report-v2 abstention example containing no substantive review
    claim, plus a core-generated canonical source fixture and source-bound
    negative fixtures from typed runtime replay; and
12. report omission, duplicate, reorder, empty-loss, exact attempt/claim/raw-
    registration set, authority nonzero, status/coverage mismatch, and exact/
    +1 row/byte/working-bound fixtures.

The checked-in claim-free report example's raw artifact is exactly this one
canonical UTF-8 JSON byte sequence (no trailing LF):

```json
{"abstention":{"detail":"The deterministic fixture declares insufficient projected context.","reason":"insufficient_context"},"claims":[],"execution_id":"execution:sha256:cdf45e445e0931d64f219ab75fc0241df6bd597303cef4949437ab6d298b94d8","schema":"reviewgraphen.reviewer_output.v1"}
```

Its length is 281 bytes and its hash is
`sha256:27d5941642c432f2d0b588a4aa7761005af04da7d8062ac2be23a27d95331a30`.
This is a local reference vector, not proof that those bytes exist in CAS or
were emitted by core. The fixture makes an abstention observation only; it
asserts no program fact, claim, evidence, verification, or acceptance.

There is no fixture that calls a real model, network, arbitrary shell command,
or tool. A requested-evidence string is data, never permission to execute it.

## Consequences

### Positive

- Execution, raw output, claim, plan, context, and source registration form one
  closed audit chain without conflating claim and evidence.
- Retry, crash, replay, index rebuild, and report projection are deterministic
  and bounded.
- Report v2 can express honest abstention/failure without fake claims or M1
  execution sentinels.

### Negative

- Schema-v2 SQLite images require a full rebuild.
- The full D2 claim projection makes schema-v2 SQLite images unusable without
  deterministic rebuild.
- The fake-only boundary postpones provider usage/cost/token contracts and
  real tool traces to later ADRs.

## Superseded text

This ADR supersedes ADR 0013 only where it uses execution outcome `Completed`,
omits system-prompt/tool-call fields, allows identical v2 execution-event
reapply, or describes the narrow M1 `ReviewClaim` as the D2 claim body. It
supersedes ADR 0017 only by adding schema version 3 and the D2 projection.
ADR 0013's event-v2 choice, atomic event principle, closed taxonomies,
prompt-injection boundary, and authority rules remain. ADRs 0014-0017 retain
their CAS, admission, canonical planning/context, and store-security decisions.

## Decision acceptance

This ADR is Accepted because its boundary, versioning, exact preimages,
state/authority separation, ordering, bounds, failure behavior, report shape,
and index-v3 migration decision are complete and internally consistent. The
report-v2 schema and local-only example validate, and the document bundle is
self-consistent. Acceptance records the design decision only: it does not
claim that D2 is implemented, that a core-generated source fixture exists, or
that source-bound replay and runtime allocation proofs have passed. Those
artifacts and proofs remain mandatory under §11 before implementation is
complete.
