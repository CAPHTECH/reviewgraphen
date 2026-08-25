# ADR 0038: Changed Public Callee Relation Slice

- Status: Accepted
- Date: 2026-08-23

## Context

The ordinary Git path can ingest accepted, conservative local `calls`
relations, propagate base-to-head changed containment to function artifacts,
and record exact unqualified `pub` syntax. It still cannot produce a generally
useful, applicable Relation obligation from those facts.

The Rust adapter accepts a direct call only when a path expression has exactly
one local syntactic target. It records `resolution = "syntactic_unique"` on the
accepted relation. Shadowed, ambiguous, imported, cross-crate, UFCS, method,
dynamic-dispatch, and macro-expanded targets are not guessed. The adapter
therefore declares `direct_calls` permanently `partial`, with a source-backed
limitation. This is correct and must remain true.

The current synthesizer, equally correctly, treats only a `complete`
capability as satisfying `required_capabilities`. A concrete relation target
whose rule requires `direct_calls` is consequently `unknown`, and a separate
rule-level capability-gap obligation is emitted. That behavior conflates two
questions for this slice:

1. whether an already accepted relation target is sufficiently supported for
   the property to be reviewed; and
2. whether every possible relation target was enumerated.

The questions must be separated without claiming that the partial call graph is
complete. This decision extends [ADR 0003](./0003-obligations-define-the-coverage-universe.md),
[ADR 0004](./0004-project-minimal-context-with-declared-loss.md),
[ADR 0005](./0005-llm-output-is-a-reviewable-claim.md),
[ADR 0011](./0011-program-space-v2-capability-trace.md),
[ADR 0016](./0016-deterministic-planning-and-context-bounds.md),
[ADR 0021](./0021-m4-evidence-bound-verification.md), and
[ADR 0030](./0030-generic-review-orchestration.md). It does not weaken their
fact/claim/evidence, denominator, projection-loss, process, or authority
boundaries.

A read-only development-history probe examined 143 commits containing
production Rust changes across three already-present repositories. Candidate A,
changed explicit unsafe syntax, occurred in **0/143**, including under a
permissive changed-file upper bound. Changed function/call prerequisites for
this decision occurred far more often. This is development evidence, not a
holdout estimate, but it makes Candidate A unsuitable as the first practical
wedge.

## Decision

The first practical Relation slice is the following fixed contract:

| Contract element | Exact value |
| --- | --- |
| Rule ID | `relation.changed_public_callee@1` |
| Property ID | `rust.callee_contract_review@1` |
| Target kind | `relation` |
| Target-support field | `target_support_capabilities` |
| Enumeration field | `enumeration_capabilities` |
| Review profile ID | `rust.production.v1` |
| Review profile schema | `reviewgraphen.review_profile.v1` |
| Active context policy | `context.subject_windows@3` |
| Frozen context compatibility policy | `context.subject_windows@2` |
| Deterministic observer | `deterministic.abstain@1` |
| Deferred verifier descriptor | `workspace.cargo_test@1` |
| Active request schema | `reviewgraphen.generic_review_request.v3` |
| Active run schema | `reviewgraphen.generic_review_run.v3` |
| Active human-report schema | `reviewgraphen.generic_review_human_report.v2` |
| Frozen compatibility schemas | request/run v2 and human-report v1 |
| Evaluation directory | `benchmarks/m20-changed-public-callee-utility-v1/` |

The rule and property are review questions. They mean “review whether callers'
assumptions about this changed callee's preconditions, postconditions, error
paths, and return-value meaning remain valid.” They do not mean that a contract
changed or broke. Implementations and reports must not name this fact
`breaking_call`, `contract_violation`, `unsafe_bug`, or any equivalent
assertion.

### 1. Exact rule trigger

For each accepted ProgramSpace relation `r`, the synthesizer must evaluate the
following predicate. Every clause is mandatory:

1. `r.kind == "calls"`.
2. `r.attributes["resolution"]` exists and is exactly the string
   `"syntactic_unique"`.
3. `r.source_id` resolves to one accepted artifact, the caller endpoint.
4. `r.target_ids` contains exactly one accepted artifact ID, the callee
   endpoint. Zero or multiple targets are not candidates and must produce a
   typed synthesis obstruction if such a malformed accepted `calls` relation
   reaches this rule.
5. The callee artifact has `kind == "function"`.
6. The callee artifact has `attributes["public"] == true`. For the Rust
   adapter, this fact must continue to mean exactly
   `matches!(visibility, Visibility::Public(_))`. `pub(crate)`, `pub(super)`,
   `pub(in path)`, private items, re-export reachability, and inferred external
   visibility do not satisfy this clause.
7. The callee is changed only through change-family containment: there must be
   an accepted `contains` relation whose target set contains the callee ID and
   whose accepted source artifact has `attributes["changed"] == true`. The new
   rule must not read `changed` from the callee's own attributes, must not copy
   a base-relative `changed` flag onto the callee, and must not use a changed
   caller as a substitute.
8. The candidate is not excluded by the exact `rust.production.v1` review
   profile defined in section 4.

The current helper that also accepts `artifact.attributes.changed` is not the
trigger contract for this rule. A new containment-only predicate must be used.
The accepted relation remains a Program fact. Clauses 5–7 are deterministic
selection facts; none asserts that the callee's semantic contract changed.

Exactly one substantive obligation must be emitted for each accepted
caller-to-callee relation ID satisfying the predicate. Two distinct accepted
edges with the same endpoints produce two obligations. A duplicate observation
of the same accepted relation ID must not produce a second obligation.

The obligation must have:

- `rule = "relation.changed_public_callee@1"`;
- `property_id = "rust.callee_contract_review@1"`;
- `target_kind = "relation"`;
- `target_refs = [r.id]`;
- a semantic key equal to the property ID followed by the one normalized
  relation target according to the existing obligation identity algorithm;
- `generator_ids` containing the call relation, caller, callee, every matching
  change artifact, and every matching change-family `contains` relation;
- source provenance containing those generator IDs plus the repository and
  snapshot sources already required by the obligation constructor;
- context IDs equal to the sorted unique union of contexts containing the call
  relation, caller, or callee;
- evidence modes `source_inspection` and `test`;
- weight `4.0`; and
- no dependency on a Node obligation.

All ID collections must be normalized, duplicate-free, and deterministically
ordered. Obligation identity must retain the existing snapshot, profile, rule,
extractor, property, and relation-target inputs. Change artifacts and
containment witnesses are provenance, not an alternative target identity.

The existing `relation.changed_call_contract@1` and its
`payment.idempotency_contract` semantics must not be reused, widened, renamed,
or migrated to this rule. Stored records using that ID retain their original
meaning.

### 2. Capability-contract split

The v2 public contract must contain two distinct, required fields on every
contract obligation:

```text
target_support_capabilities
enumeration_capabilities
```

Both fields are sorted, unique arrays of nonempty capability names. They must
be present even when one is empty. They must be disjoint. Unknown fields,
duplicate values, omitted fields, `null`, an old `required_capabilities` field,
or a value appearing in both arrays must be rejected by the closed v2 schema
and semantic validator.

`target_support_capabilities` means the capabilities that must be `complete`
before the property is applicable to the exact accepted target. For
`relation.changed_public_callee@1`, the set is exactly:

```text
ast
containment
changed_structure
```

Each capability can legitimately become complete. `ast` and `containment` are
complete when all admitted Rust source parses; a parse failure makes them
partial. `changed_structure` is complete when the Git diff contains no changed
entry excluded for unsupported mode/type; such an excluded changed entry makes
it partial. No other capability may be inserted into this set for rule `@1`.

`enumeration_capabilities` means capabilities that determine whether the
candidate space was fully enumerated. For this rule, the set is exactly:

```text
direct_calls
```

`direct_calls` must remain `partial` for the current Rust extractor. Its state
must not make an otherwise supported accepted relation target `unknown`.
Instead, it must retain its source-backed extraction limitation and cause one
rule-level capability-gap obligation for
`relation.changed_public_callee@1`. A future extractor may report it complete
only under a new extractor contract that actually proves complete enumeration;
this ADR does not provide such a contract.

For a substantive D obligation, `applicable` means only:

> the property may be reviewed for this exact accepted relation target under
> complete target-support capabilities.

It must never mean that all callers, all callees, all Rust calls, or all
contract changes were found. If any target-support capability is non-complete,
the substantive target obligation must be `unknown`, with the existing exact
state-specific reason and source-backed qualification IDs. Enumeration state
must not overwrite that target applicability.

#### 2.1 Integration with current `materialize`

The existing `materialize(program, ObligationSpec)` path and its meaning of
`required_capabilities` must remain unchanged for the five existing substantive
rules. Their trigger behavior, applicability, obligation bodies, StableIds,
contract bytes, universe bytes, and checked-in canonical v1 fixtures must remain
byte-identical.

The implementation must add a separate split-capability construction path. Its
behavior is normative:

1. A split spec contains both exact sets and refuses overlap or an omitted set.
2. It invokes the existing applicability calculation with
   `target_support_capabilities` only. A private shared helper may be extracted,
   but the legacy wrapper must produce identical values and bytes.
3. The v2 contract projection serializes the two sets under their fixed names;
   it must not serialize the ambiguous v1 name `required_capabilities`.
4. The v2 rule-level gap calculation examines the union of target-support and
   enumeration capabilities. Every non-complete target-support capability is
   classified in the gap's target-support field; every non-complete enumeration
   capability is classified in its enumeration field. At least one must be
   nonempty for a gap.
5. The D substantive obligation may therefore be `applicable` while the D
   rule-level gap is `unknown` because `direct_calls` is partial.

The v2 projection of a legacy rule must map its existing
`required_capabilities` to `target_support_capabilities` and use an empty
`enumeration_capabilities` array. That compatibility projection must not change
legacy evaluation. A later reclassification of a legacy rule requires a new
rule version.

The generic v2 compatibility path and active v3 path must use the same
split-capability rule-pack version distinct from the v1
`m1.fixture@1` pack. The implementation must not mutate the v1 pack in place or
rewrite stored v1 genesis/universe/obligation records. There is no lossless
v1-to-v2 obligation migration because v1 did not state the capability split;
v1 is read and replayed with v1 semantics. New D requests are v3, while their
unchanged split-capability obligation contract remains the v2 contract.

Schema mutation tests must independently delete, exchange, duplicate, overlap,
rename, null, and add unknown siblings to the two fields. Semantic mutation
tests must prove that moving `direct_calls` into target support changes a D
target to `unknown` and is rejected as a rule-contract mismatch, while moving
`ast`, `containment`, or `changed_structure` into enumeration is likewise
rejected.

### 3. Two-layer coverage denominator

The D coverage contract has two layers that must be serialized separately.

#### 3.1 Resolved-target denominator

The resolved-target denominator is the exact sorted set of substantive
obligation IDs emitted for accepted D relation targets after explicit profile
exclusions. It includes both applicable and target-support-unknown substantive
obligations. Its stage numerators—planned, deferred, executed, structured,
abstained, malformed, provider-failed, and verifier-observed—must be explicit ID
sets and subsets of that denominator.

This denominator may answer only questions phrased as “of the accepted
syntactic-unique D targets in this universe.” It is not a call-graph or all-
caller denominator.

#### 3.2 Candidate-space incompleteness

Candidate-space incompleteness consists of:

- the exact `direct_calls` capability state;
- the source-backed limitation IDs qualifying that state;
- the D rule-level capability-gap obligation ID; and
- the exact extraction-obstruction IDs for call enumeration, including the
  observed unresolved call occurrences.

An unresolved direct, imported, UFCS, method, trait, dynamic-dispatch,
cross-crate, or macro-expanded call has no accepted relation ID and therefore
must not be manufactured as an individual D obligation. Its concrete
occurrence remains an extraction obstruction with source/path provenance. The
legacy combined unique-local-syntax limitation remains unchanged in the legacy
`UniverseDescriptor`'s `limitation_ids`; the v2-only global record does not
enter that collection. The D v2 rule-level gap remains in its separate v2
denominator. This is an explicit unknown candidate space, not zero applicable
work.

The v2 Rust extraction contract must expose a closed
`reviewgraphen.ingestion_obstruction.v2` record. Every observed unresolved call
occurrence must have `related_capabilities` exactly `["direct_calls"]` and must
contain these typed fields:

- `call_kind`, one of `direct`, `method`, or `macro_invocation`;
- `reason`, one of `direct_non_path`, `direct_empty_path`,
  `direct_shadowed_binding`, `direct_unresolved_scope`,
  `direct_target_count_zero`, `direct_target_count_multiple`,
  `method_dispatch_unresolved`, or `macro_expansion_unresolved`;
- the exact normalized source path and inclusive start/end line and column;
- sorted nonempty accepted source IDs, snapshot ID, and extractor ID; and
- kind, severity, description, and sorted related capabilities.

Occurrence severity is exactly `medium`. Kind is exactly
`relation_unresolved` for `direct`, `dynamic_dispatch_unresolved` for `method`,
and `macro_expansion_unresolved` for `macro_invocation`. The global record in
section 3.3 has kind `relation_unresolved` and severity `info`.

The obstruction ID is `StableId::derived("ingestion-obstruction", bindings)`.
Its preimage must bind every semantic field except its own ID and description,
including the exact record `schema`; description is a
deterministic rendering of the typed fields and must validate byte-for-byte. A
missing span or source ID is a typed v2 report-construction failure and cannot
be serialized or silently counted as a located occurrence. V1 obstruction
serialization remains unchanged; v2 must not infer these fields from v1 prose.

The v2 sidecar defined in section 3.3 must also emit a distinct source-backed
global obstruction and limitation with
`related_capabilities == ["direct_calls"]` explaining that
unique-local syntactic resolution is incomplete. This record is required even
when no individual unresolved occurrence was observed. The existing v1
combined five-capability limitation remains historical and must not be
reinterpreted as this v2 record. A macro occurrence records only the observed
invocation; because expansion can contain an unknown number of call sites, its
global limitation must state `latent_occurrence_count = "unknown"`. Neither
the invocation count nor zero observed invocations may be presented as the
number of unresolved expanded calls.

`reviewgraphen.generic_review_run.v2` and
`reviewgraphen.generic_review_run.v3` must each include a closed per-rule coverage
object containing at least the exact rule ID, resolved-target obligation IDs,
candidate-space-gap obligation IDs, enumeration capability states,
enumeration limitation IDs, mandatory `enumeration_obstruction_ids`, and all
resolved-target stage numerator IDs. `enumeration_obstruction_ids` is exactly
the sorted union of the v2 sidecar's located-occurrence IDs and its one global
limitation ID; the located-occurrence IDs are separately serialized as
`enumeration_occurrence_obstruction_ids`, and
`enumeration_limitation_ids` is exactly the singleton global-limitation ID.
The semantic validator must rebuild all three sets from the exact
`reviewgraphen.ingestion_report.v2`, require the occurrence set to be a proper
typed subset of the obstruction set, and reject omission, extras, or a record
tied to another capability. When `direct_calls` is partial, the global v2
record makes `enumeration_obstruction_ids` and `enumeration_limitation_ids`
nonempty; the candidate-space gap set must also be nonempty. The object must also contain
these exact fields:

```text
call_graph_complete: false
global_call_coverage_claim: "prohibited"
```

for every run in which `direct_calls` is not complete. The schema must not
provide a global `call_coverage_percentage` field. The semantic validator must
reject `call_graph_complete = true`, an empty limitation/gap trace under a
partial state, an empty enumeration-obstruction set under a partial state, a
numerator outside the resolved denominator, or a gap ID misclassified as a
resolved target.

The Markdown renderer must always qualify a numeric result as coverage of
“resolved accepted syntactic-unique targets” and must render the enumeration
state and limitation/gap trace beside it. It must not emit “call coverage 100%”,
“all callers reviewed”, or equivalent text, even when every resolved-target
obligation completed.

#### 3.3 V2 ingestion sidecar and structural v1 isolation

This ADR adopts a separate top-level sidecar artifact with schema
`reviewgraphen.ingestion_report.v2`. It does not add a field to the legacy
`IngestResult`, change `reviewgraphen.extraction_report.v1`, or change the Rust
adapter's v1 identity. The sidecar projection extractor ID is exactly
`reviewgraphen.ingest.rust-call-enumeration@2`.

The closed top-level report contains exactly:

```text
schema = "reviewgraphen.ingestion_report.v2"
report_id
snapshot_id
target_revision
legacy_program_space_sha256
legacy_extraction_report_sha256
projection_extractor_id = "reviewgraphen.ingest.rust-call-enumeration@2"
located_call_occurrences
global_direct_calls_limitation
```

`legacy_program_space_sha256` and `legacy_extraction_report_sha256` are hashes
of the exact canonical v1 values returned by the unchanged legacy ingest for
the same run. `report_id` is `StableId::derived("ingestion-report", bindings)`,
where bindings contain every other top-level field, replacing record bodies by
their sorted IDs. The canonical report hash binds the complete bodies. Every
object is closed with `additionalProperties: false`; arrays are sorted,
duplicate-free, and bounded by the same admitted snapshot/file/byte limits as
the ingest that produced them.

`located_call_occurrences` contains only closed
`reviewgraphen.ingestion_obstruction.v2` values from section 3.2.
`global_direct_calls_limitation` is one mandatory closed value with schema
`reviewgraphen.ingestion_global_limitation.v2` and record kind
`global_direct_calls_limitation`. It contains its ID, snapshot ID, projection
extractor ID, kind, severity, deterministic description, sorted source IDs,
`related_capabilities = ["direct_calls"]`, and
`latent_occurrence_count = "unknown"`. Its source IDs are exactly the source
IDs in the legacy `direct_calls` capability declaration and are nonempty
whenever that declaration has an admitted Rust source. Its schema rejects
`span`, `path`, `call_kind`, `reason`, or any occurrence field. Conversely, an
occurrence schema rejects `latent_occurrence_count` and the global record kind.
The two record types therefore share one report but cannot be deserialized or
converted into one another.

The global-limitation ID is
`StableId::derived("ingestion-limitation", bindings)` and binds every semantic
field except its own ID and description, including schema and record kind. The
located-occurrence ID obeys the same rule established in section 3.2. The only
description renderings are:

```text
direct_calls unresolved occurrence: call_kind=<call_kind>; reason=<reason>; path=<path>; span=<start_line>:<start_column>-<end_line>:<end_column>
direct_calls enumeration is incomplete; macro latent occurrence count is unknown
```

The first template is used only for a located occurrence and the second only
for the global limitation. UTF-8 bytes are compared exactly; there is no
localization, whitespace normalization, alternate prose, or fallback to stored
description. A macro occurrence records the observed invocation span in the
first collection. It never records a number of expanded calls. The singular
global record retains unknown latent cardinality even when the occurrence
collection is empty.

The public constructor/decoder must use private record fields plus validating
factories, not publicly writable structs. It must expose
`decode_and_validate_ingestion_report_v2(bytes, legacy_program_space,
legacy_extraction_report)`. That operation must validate field closure,
schema/kind discrimination, canonical ordering, all StableId namespaces and
preimages, both legacy hashes, snapshot/target/extractor equality, exact
description reconstruction, exact `direct_calls` source closure, source-ID
resolution, normalized paths, inclusive spans within the accepted source, and
the singular global record. An unchecked deserializer must not return the
public validated type. Live admission must additionally rebuild the report
from the immutable snapshot and compare canonical bytes; replay may substitute
only an already sealed report hash bound to the same request and legacy hashes.

The existing public functions `ingest` and `ingest_with_sources`, their return
types, and their canonical serializers remain unchanged and cannot expose this
report. New public functions are the only v2 route:

```text
ingest_v2(request) -> IngestResultV2 { legacy: IngestResult, ingestion_report_v2 }
ingest_with_sources_v2(request, source_budget) -> IngestWithSourcesResultV2 { legacy: IngestWithSourcesResult, ingestion_report_v2 }
```

The v2 wrappers are in-memory typed products and have no permissive combined
serializer. They expose the unchanged legacy canonical output and the separate
canonical sidecar bytes. Both `reviewgraphen.generic_review_run.v2` and
`reviewgraphen.generic_review_run.v3` serialize them under distinct required
fields `legacy_ingestion` and `ingestion_report_v2`, bind the sidecar ID/hash
into their universe and candidate-space projections, and validate the pair
before synthesis. The split-capability synthesis API accepts the
validated pair explicitly. Sidecar IDs may qualify only v2 enumeration/gap/
coverage fields; they must not be inserted into a legacy obligation source set
whose closure is ProgramSpace-only.

The implementation must partition legacy issue drafts and v2 call-enumeration
drafts before constructing any public collection. Legacy drafts alone may
populate legacy `limitation_by_id`, legacy `obstruction_by_id`, core
`Extraction.limitations`, capability qualification IDs,
`reviewgraphen.extraction_report.v1`, v1 universe inputs, and existing-rule
obligations. V2 drafts alone may populate the two sidecar record domains. V2
record types must have no `From`/`Into`/shared insertion API for a core
`Limitation` or legacy `IngestionObstruction`; adding `serde(skip)` fields to a
legacy record is not isolation. Thus computing a v2 report cannot change any
legacy record count, ID, body, adapter/extractor hash, or canonical byte.

Adding a v2-only field to legacy `IngestResult` was rejected because a skipped
field does not prevent shared builders from inserting new records into v1
collections. Cutting the entire Rust extractor to a new major was rejected for
this slice because it would deliberately change the legacy adapter-set and
ProgramSpace identities, defeating the required same-input compatibility
oracle. A future extractor-major migration remains possible under a successor
ADR; it is not implicit in this sidecar projection.

#### 3.4 Empirical scale amendment: exact source summaries, not occurrence rows

The public-retention clauses in sections 3.2 and 3.3 were written before a
conforming repository-scale run existed. On 2026-08-24, the section 11
quickstart tuple (270 Rust files, three changed files, and 126 changed lines)
produced 321,098 observed unresolved call occurrences. Only 36,704 (11%) came
from the 82,401-line `event.rs`; parsing and visiting that file took about 0.83
and 0.51 seconds respectively. The failure is therefore not one exceptional
file. It is the general product of retaining and repeatedly validating one
large JSON record per observed call syntax. The old run shape also cannot be a
conforming section 11 artifact because its schema admits at most 20,000 such
rows.

This subsection supersedes only the earlier requirements to serialize every
`reviewgraphen.ingestion_obstruction.v2` body in
`located_call_occurrences`, to copy every such ID into per-rule coverage, and
to validate that public array as the occurrence closure. It does **not**
supersede the requirement to observe every unresolved direct, method, and macro
invocation; the closed call-kind/reason taxonomy; source-backed extraction;
the prohibition on fabricated D obligations or global call coverage; or the
distinct global unknown-latent limitation. The extractor must still construct
the exact closed occurrence values internally, and their sorted IDs remain the
input to the digests below. No occurrence may be sampled, truncated, or omitted
from a count or digest.

The adopted public projection is one closed
`reviewgraphen.ingestion_obstruction_summary.v1` per accepted Rust **file**
that has at least one observed unresolved occurrence. A summary contains
exactly:

```text
schema = "reviewgraphen.ingestion_obstruction_summary.v1"
id
kind = "call_enumeration_summary"
severity = "medium"
snapshot_id
projection_extractor_id = "reviewgraphen.ingest.rust-call-enumeration@2"
file_source_id
path
related_capabilities = ["direct_calls"]
observed_occurrence_count
occurrence_id_set_sha256
buckets
detail_retention = "spans_and_owner_sources_omitted_rebuildable"
```

`file_source_id` is the accepted file artifact at `path`. `buckets` is the
strictly sorted, duplicate-free set of nonempty `(call_kind, reason,
observed_occurrence_count, occurrence_id_set_sha256)` rows, using only the
legal kind/reason pairs from section 3.2 and with at most eight rows. A bucket
digest is SHA-256 over the canonical sorted array of the underlying occurrence
IDs in that bucket. The file digest is computed the same way over the exact
union of its buckets. Underlying occurrence IDs must be unique; counts use
checked `u64`, and the file count must equal both the ID-set cardinality and
the checked sum of bucket counts. The summary ID is
`StableId::derived("ingestion-obstruction-summary", bindings)` and binds every
field except its own ID. Summaries are sorted by ID. The report-level count and
digest repeat the checked sum and canonical sorted-ID digest across every file,
so changing one occurrence's path, span, kind, reason, accepted sources,
snapshot, or extractor changes an underlying ID and therefore the bucket,
file, and report commitments.

The public `reviewgraphen.ingestion_report.v2` shape is corrected before its
first conforming release: `located_call_occurrences` is replaced by
`source_occurrence_summaries`, `observed_occurrence_count`, and
`occurrence_id_set_sha256`; the singular
`global_direct_calls_limitation` remains unchanged. There is at most one
summary per admitted file, so the existing ingest `max_files` bound is also a
mechanical summary-row bound; `maxItems: 20000` is no longer a dataset-derived
occurrence cap. The existing `ingestion-report` ID namespace is retained, but
its corrected preimage replaces located-occurrence IDs with the exact sorted
summary IDs and additionally binds the report count and digest; it still binds
the global-limitation ID and every unchanged top-level identity/hash field.
Live admission must re-extract from immutable source bytes and match every
count, digest, summary ID, report ID, and canonical report byte. Replay may use
only the already sealed report hash and the same request, snapshot, extractor,
and legacy hashes. The pre-amendment draft shape is rejected rather than
silently upcast. Legacy v1 bytes and adapter identity remain unchanged.

This is an explicitly lossy projection of location detail, not deletion of an
observed unknown. The run retains the exact accepted file source, path,
kind/reason partition, multiplicity, and a commitment to every underlying
occurrence. Exact spans and owner-level source IDs are omitted from the public
run, declared by `detail_retention`, and deterministically rebuildable in a
clean clone from the immutable snapshot plus the pinned extractor. A renderer
must call the number an **observed unresolved syntax count**, never a candidate
denominator, total caller count, or call-graph coverage denominator.

A macro bucket counts only observed macro-invocation syntax. It has no
`latent_occurrence_count`. The unchanged singular global limitation still has
`latent_occurrence_count = "unknown"`, and that value is not included in any
summary or report count. Thus 10 observed macro invocations means exactly 10
observed invocation syntaxes and an unknown number of latent expanded calls;
it can never be rendered or validated as 10 unresolved expanded calls.

Per-rule coverage now closes over summaries as follows:

```text
enumeration_obstruction_summary_ids
  = exact sorted source_occurrence_summaries[*].id
enumeration_limitation_ids
  = [global_direct_calls_limitation.id]
enumeration_obstruction_ids
  = exact sorted union(enumeration_obstruction_summary_ids,
                       enumeration_limitation_ids)
observed_unresolved_call_occurrence_count
  = ingestion_report_v2.observed_occurrence_count
occurrence_id_set_sha256
  = ingestion_report_v2.occurrence_id_set_sha256
```

The semantic validator rebuilds all five values from the report and rejects
omission, extras, count/digest mismatch, another capability, or an empty
limitation/gap trace while `direct_calls` is partial. The length of either ID
set is a summary/limitation record count and must not be presented as an
occurrence count. This preserves B-R2-6/D1-B6: every observed unknown affects
an exact count and digest in the per-rule projection, remains source-backed,
and cannot disappear without changing the closure. It also preserves the
AGENTS.md denominator rule: the resolved-target denominator remains the exact
obligation-ID set from section 3.1, while observed counts and the unbounded
candidate-space limitation are separately named and never combined into a
percentage.

Raising the occurrence array limit to 321,098 was rejected because that number
is one observation, not a bound; another admitted snapshot can exceed it.
Raising it to a large guessed value merely moves the failure and retains the
pathological artifact. Truncation, sampling, top-N spans, and “first 20,000”
were rejected because they silently delete observed unknowns and manufacture
an implicit denominator. A global count-only record was rejected because it
loses source trace and kind/reason partitioning. Moving all individual rows to
a separate retained sidecar was rejected as the default Stage 0 contract: it
shrinks the run document but still requires retaining the same record
explosion. A diagnostic implementation may reconstruct or temporarily export
the committed occurrence set, but that export is not a required Stage 0
artifact or an authority-bearing input.

Stage 0 cannot be called cheap merely because it invokes no model. The measured
optimized core time is 183.2 seconds per cluster, or about 15.3 hours for 300
clusters in serial before full evaluator/artifact overhead. Retaining the old
shape would add 96,329,400 individual JSON records if every cluster resembled
the measured quickstart, making full-run storage, hashing, validation, and
clone-to-clone comparison operationally unreasonable. The adopted summaries
bound retained rows by admitted files, but they do not erase the 15.3-hour
compute measurement. Stage 0 must record wall
time, peak bytes, report bytes, summary rows, and observed counts per cluster;
parallel execution may reduce elapsed time but cannot change canonical output
or any gate. The model stages remain blocked until the full 300-cluster run
finishes and all seven gates pass.

This amendment changes the still-unreleased v2 schema shape, its examples and
hashes, and the meaning of the enumeration-honesty gate's exact obstruction
IDs. Therefore the m20 evaluator specification, executable checks, reference
vectors, mutation tests, preregistration wording, and freeze manifest require
an explicit coordinated re-freeze before Stage 0. Section 5.4 additionally
changes the treatment-arm context projection and therefore joins this same
re-freeze. Provided no Stage 0 or model result has been observed and no
intermediate occurrence-only freeze is treated as final, both amendments must
be implemented, validated, and frozen atomically in **one** coordinated
re-freeze; two sequential freezes are neither required nor desirable. If an
occurrence-only freeze has already been sealed or used, section 5.4 requires a
new freeze and the old study cannot absorb its outputs. The gate remains one of
the same seven gates and keeps its threshold and failure semantics; the
re-freeze may only replace occurrence-row closure with the exact
summary/count/digest closure above and replace v2 context materialization with
section 5.4's exact v3 commitment/materialization/summary closure. The fixed
rule/property/profile identifiers, profile DTO, and context-v2 DTO remain
unchanged.

### 4. Fixed review profile, fan-out, and planning

The only profile admitted for the D `@1` rule is `rust.production.v1`, with
schema `reviewgraphen.review_profile.v1`. The schema is a closed Draft 2020-12
object with `additionalProperties: false` at every object. Its canonical bytes
are the UTF-8 bytes of this single-line JSON, with no BOM or trailing newline:

```json
{"category_precedence":["vendor","generated","test","example","docs"],"exclusion_matchers":[{"category":"vendor","id":"path.vendor_component@1","operator":"component_equals_any","values":["third_party","vendor","vendored"]},{"category":"generated","id":"path.generated_component@1","operator":"component_equals_any","values":["generated","target"]},{"category":"generated","id":"path.generated_suffix@1","operator":"basename_suffix_any","values":[".generated.rs"]},{"category":"test","id":"path.test_component@1","operator":"component_equals_any","values":["benches","tests"]},{"category":"test","id":"path.test_basename@1","operator":"basename_equals_any","values":["test.rs","tests.rs"]},{"category":"test","id":"path.test_suffix@1","operator":"basename_suffix_any","values":["_test.rs","_tests.rs"]},{"category":"example","id":"path.example_component@1","operator":"component_equals_any","values":["example","examples"]},{"category":"docs","id":"path.docs_component@1","operator":"component_equals_any","values":["doc","docs"]}],"id":"rust.production.v1","path_normalization":{"absolute":"reject","backslash":"reject","case_fold":false,"dot":"reject","dot_dot":"reject","empty_component":"reject","encoding":"utf-8","nul":"reject","separator":"/","unicode_normalization":"none"},"reason_ids":{"docs":"profile.exclude.docs@1","example":"profile.exclude.example@1","generated":"profile.exclude.generated@1","test":"profile.exclude.test@1","vendor":"profile.exclude.vendor@1"},"rust_source":{"basename_suffix":".rs","case_sensitive":true},"schema":"reviewgraphen.review_profile.v1"}
```

The profile hash is exactly
`sha256:4b6cca93794ab03b1576e17d2e395ec43f731d316685247a89363ae2e840dd96`.
Schema validation must re-canonicalize the DTO by RFC 8785/JCS, require exact
byte equality with the bytes above, recompute the hash, and reject any other
ID, hash, field, matcher, order, or value. The profile ID, schema, canonical
bytes, and hash are universe and baseline-packet inputs; a freeze manifest may
refer to them but may not define or replace them.

#### 4.1 Path and category semantics

A profile path must already be a repository-relative UTF-8 Git path. It is
valid only if it is nonempty, contains no NUL or backslash, does not begin with
`/`, and its `/`-separated components are all nonempty and neither `.` nor
`..`. No case folding, Unicode normalization, percent decoding, symlink
resolution, filesystem canonicalization, or platform separator conversion is
performed. Invalid paths produce a typed profile obstruction and are not
eligible.

The matcher grammar is closed to the three operators present in the canonical
DTO:

- `component_equals_any` matches when any complete path component equals one
  listed value byte-for-byte; and
- `basename_equals_any` and `basename_suffix_any` compare only the final path
  component byte-for-byte.

No glob, regular expression, substring, prefix, case-insensitive, attribute,
model, Cargo-metadata, or filesystem matcher exists. Matchers are evaluated in
`category_precedence`, then their array order. The first match wins and fixes
both matcher ID and the category's typed reason ID. A path not matched by the
canonical DTO is not excluded by profile convention.

For this ADR, a **Rust source** is exactly a valid normalized path whose
basename ends in lowercase `.rs`. A **non-test Rust source** is a Rust source
that matches none of the three `test` matchers; it can still be example,
generated, vendored, or docs source. A **production Rust source** is a Rust
source that matches no matcher in any category. These are syntactic profile
classes, not claims about deployment, cfg reachability, ownership, or code
quality.

For a Git change entry, the classification path is its target path except for
a deletion, which uses its base path. A **production diff entry** is an
accepted added, modified, deleted, or renamed regular-blob entry whose
classification path is production Rust source. Unsupported modes/types and an
invalid or absent classification path are typed obstructions, not production
entries. A rename is classified only by its target path; its base path remains
source trace. A **production diff** is the exact sorted set of production diff
entry IDs, including the empty set. No count or prose label can substitute for
that ID set.

Any evaluation baseline packet labeled `rust.production.v1` must serialize the
fixed profile ID/hash and the exact sorted production-diff entry ID set. Its
diff/source inventory may contain only bytes and hunks source-bound to those
entries; every admitted source ID and byte hash must be listed. An empty set
produces an explicit empty production-diff packet, not an implementation-chosen
fallback. Adding an excluded or unbound source, or omitting a production entry,
invalidates the packet.

For a D candidate, the classified paths are exactly the accepted callee
location path followed by the accepted caller location path. A missing or
invalid endpoint path is a typed profile obstruction and makes the target
`unknown`; it is not an exclusion. Evaluate every canonical matcher against
both valid paths. If any match, exclude the candidate using the earliest
category, then matcher array position, then endpoint order `callee`, `caller`.
Thus a production callee called only from `tests/` is visibly excluded, as is a
production caller targeting a vendored or generated callee. Change-artifact
paths, support-context paths, model labels, and Cargo target membership do not
participate in candidate classification.

#### 4.2 Exclusion identity

A D relation candidate matched by this profile must produce one stable
`ExclusionRecord` with excluded weight `4.0`. Its source set is the sorted
unique union of the call relation, caller, callee, every matching change
artifact, and every matching containment witness. Its `candidate_key` is
exactly `relation.changed_public_callee@1|<relation-id>`.

The D exclusion ID is `StableId::derived("exclusion", bindings)`, where the
canonical binding map contains exactly:

```text
candidate_key
excluded_weight = "4.0"
matcher_id
profile_hash
profile_id = "rust.production.v1"
reason_id
rule = "relation.changed_public_callee@1"
snapshot_id
source_ids
```

`source_ids` is the canonical sorted ID array; `excluded_weight` is the exact
decimal string shown, not a host float rendering. The record body must repeat
and validate all bindings. This D-specific constructor must not use the legacy
exclusion preimage of only candidate and snapshot. A different profile,
matcher, reason, weight, rule, or source set must therefore have a different
ID. Exclusions remain outside eligible coverage but visible in the universe.

#### 4.3 Planning and exact Stage 0 gates

Planning bounds are not profile exclusions. Every nonexcluded candidate must
first receive its obligation ID and weight and enter the resolved-target
denominator. If the plan cannot schedule it, the plan must retain that exact
obligation ID as `deferred`, preserve weight `4.0`, and use the typed reason
`budget_exhausted` or `prerequisite_deferred`. It must never delete the
obligation or replace it with a count.

Let `C` be the exact sorted set of all 300 fixed Stage 0 commit-cluster IDs;
clusters with no applicable D obligations remain in `C`. For each `c`:

- `A_c` is the exact set of resolved-target obligation IDs in cluster `c`
  whose applicability is `applicable` and rule is
  `relation.changed_public_callee@1`;
- `D_c` is the exact subset of `A_c` whose plan status is `deferred`; and
- `n_c = |A_c|`.

The fan-out vector is the 300 pairs `(c, n_c)`, sorted by `(n_c, c)`. The
nearest-rank p95 is the `n_c` at one-based rank `ceil(0.95 * |C|)` in the
nondecreasing sequence of counts; for 300 clusters this is rank 285. Zeroes are
retained. The gate passes exactly when this value is at most 50.

Let `A = union(A_c)` and `D = union(D_c)`. IDs must be globally unique or the
Stage 0 input is invalid. The deferred fraction is `0` when `A` is empty and
otherwise `|D| / |A|`, computed as the exact integer comparison
`20 * |D| <= |A|`; floating-point rounding is forbidden. The gate passes
exactly when that comparison holds. Stage 0 must serialize `C`, every `A_c`
and `D_c`, `A`, `D`, the sorted count vector, rank, and both decisions.

Both gates are independent mandatory advance conditions. If either fails, no
model stage may start and the slice fails. Exact-boundary and +1 fixtures must
cover p95 50/51 and deferred 5%/the smallest ratio above 5%, including
zero-obligation clusters. The only remedy is a new, narrower, versioned trigger
or profile with an explicit new denominator. An implicit cap, top-50
truncation, dropped caller, changed weight, or post-result filter is forbidden.

### 5. Context policy `context.subject_windows@2`

The v2 generic path must construct one envelope per obligation using the fixed
policy `context.subject_windows@2`. The relation target's caller and callee
artifacts are both **subjects**. The changed callee is ordered before the
caller, but both are processed before every support source. A support source
must never evict an otherwise representable subject.

The v2 policy is a new closed DTO and algorithm. It must not alter
`ContextPolicyV1`, `context.baseline@1`, or the v1 `ReviewContextEnvelope`.
Those types and bytes must remain readable and replayable. A v1 envelope must
not be reinterpreted as v2.

#### 5.1 Fixed bounds

The policy has these inclusive bounds:

| Bound | Value |
| --- | ---: |
| obligations per envelope | 1 |
| subject endpoints | 2 |
| windows per envelope | 8 |
| windows per file | 4 |
| lines per window | 400 |
| bytes per window | 262,144 |
| total excerpt bytes | 1,048,576 |
| included files | 64 |
| source anchors | 1,024 |
| candidate files | 4,096 |
| candidate structural IDs | 4,096 |
| relation scan | 1,000,000 |
| reverse-containment edges | 1,000,000 |
| discovered paths | 20 |
| related tests | 10 |
| resolved bytes | 8,388,608 |
| one resolved artifact | 1,048,576 bytes |
| one canonical string | 16,384 bytes |
| assumption records | 64 |
| unknown records | 64 |
| loss records | 64 |
| canonical envelope bytes | 786,432 |

The complete policy DTO is the following single-line UTF-8 JSON with no BOM or
trailing newline:

```json
{"anchors_per_file":1024,"assumptions":"empty","callees_depth":3,"callers_depth":2,"candidate_order":["subject_priority","distance","path_rank","artifact_id"],"canonical_envelope_bytes":786432,"contains_edges":1000000,"discovery_paths":20,"edge_kind_direction_order":["calls:forward","calls:reverse","contains:forward","contains:reverse","covers:forward","covers:reverse"],"excerpt_lines":400,"final_window_order":["source_artifact_id","start_line","end_line","window_id"],"included_files":64,"loss_reason_precedence":["missing_location","missing_source","giant_line","per_window_lines","per_window_bytes","per_file_window_cap","total_window_cap","total_excerpt_bytes","overlap_unmergeable","path_cap","test_cap","not_reached","included_file_cap","artifact_bytes_cap","total_resolved_bytes_cap"],"max_assumptions":64,"max_candidates":4096,"max_discovered_structural_ids":4096,"max_excerpt_bytes":262144,"max_losses":64,"max_resolved_artifact_bytes":1048576,"max_resolved_bytes":8388608,"max_string_bytes":16384,"max_total_excerpt_bytes":1048576,"max_unknowns":64,"obligations_per_envelope":1,"policy_id":"context.subject_windows@2","related_tests":10,"relation_scan":1000000,"seed_fields":["source_ids","target_refs","context_ids"],"source_candidate_denominator":"all_accepted_file_artifacts_with_exact_snapshot_source_registration_closure","subject_endpoints":2,"subject_order":["callee","caller"],"support_anchor_denominator":"reached_range_bearing_accepted_artifacts_with_exact_path_reverse_contains_file","unknown_reason_ids":["unresolved_invariant_scope","unresolved_relation_endpoint","unresolved_review_context_member","unresolved_seed_reference"],"window_candidate_order":["priority","role","source_artifact_id","start_line","end_line","owner_id"],"window_merge":"same_source_overlap_or_adjacent_if_union_within_per_window_bounds","windows_per_envelope":8,"windows_per_file":4}
```

Its hash is exactly
`sha256:7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26`.
The v2 type must accept only this DTO after RFC 8785/JCS canonicalization and
hash recomputation. Every field is exact, not an upper bound an implementation
may lower. Changing any field, ordering token, discovery rule, loss vocabulary,
or cap requires a new policy ID. `ContextPolicyV1::baseline()` and its own
canonical bytes/hash remain unchanged and separately dispatched.

Every counter and addition must be checked before allocation or resolution.
Like v1, v2 projection code receives accepted metadata and bytes only through
the ordered resolver session; it must not open CAS or workspace paths itself.

#### 5.2 Exact support discovery

The seed ID multiset is exactly the obligation's `source_ids`, `target_refs`,
and `context_ids`; it is sorted and deduplicated. Each accepted artifact seed
adds itself. Each accepted relation seed adds its source and target endpoints.
Each accepted review-context seed adds its members, and each accepted invariant
seed adds its scope IDs. A reference outside those four accepted domains emits
the corresponding typed unknown. The caller and callee resolved from the D
target relation are marked subjects independently of their presence in the
expanded seed set.

The only unknown reason IDs are `unresolved_invariant_scope`,
`unresolved_relation_endpoint`, `unresolved_review_context_member`, and
`unresolved_seed_reference`, chosen by the failed expansion case above.
Assumptions are exactly empty for policy `@2`; adding an assumption is a policy
mismatch, despite the reserved cap.

Discovery scans at most 1,000,000 accepted directed relations and admits only
these adjacency tokens in this exact order:

| Token | Accepted edge and direction | Consecutive-token depth |
| ---: | --- | ---: |
| 0 | `calls`, source to target | 3 |
| 1 | `calls`, target to source | 2 |
| 2 | `contains`, source to target | unbounded within other caps |
| 3 | `contains`, target to source | unbounded within other caps |
| 4 | `covers`, source to target | 1 |
| 5 | `covers`, target to source | 1 |

Adjacency lists are sorted and deduplicated by `(token, next_id, depth)`. The
BFS state is `(distance, token_sequence, node_sequence, incoming_token,
node_id, remaining_same_token_depth)` and the priority queue orders that tuple
lexicographically. Initial remaining depth is `maximum - 1`; contains uses an
unbounded sentinel. Continuing the same non-contains token decrements remaining
and is rejected at zero; continuing contains does not decrement. Changing token
resets remaining to the new token's `maximum - 1`. A state cannot revisit a
node in its node sequence. The visited key is `(incoming_token, node_id,
remaining_same_token_depth)`. Seeds are never re-enqueued. Discovery must fail
typed-incomplete before adding structural ID 4,097.

The candidate-file denominator is exactly all accepted `kind == "file"`
artifacts and must have an exact one-to-one snapshot-source registration
closure. For every discovered structural ID, reverse accepted `contains`
edges are followed transitively until accepted file artifacts are reached,
with at most 1,000,000 examined containment edges. A range-bearing structural
artifact is a support anchor only for each reached containing file whose
normalized path equals the artifact location path exactly. Missing ownership
adds no anchor; mismatched ownership is a validation failure. More than 1,024
anchors in a file is typed-incomplete.

Canonical path entries are `(node_id, (distance, token_sequence,
node_sequence))`, sorted by the second tuple and then node ID. The first 20
path entries select their containing files. Remaining path files receive
`path_cap` unless also selected or a subject file. Among path entries whose
node artifact has `kind == "test"`, the first 10 under the same ordering select
test files; later test files receive `test_cap` unless selected or a subject
file. Candidate rank is subject first, then distance, first path rank, and file
ID. Files not reached receive `not_reached`. These rules define the complete
support anchor denominator; there is no plugin, heuristic, model-selected
source, or implementation-defined traversal.

#### 5.3 Window construction

Window construction must be deterministic:

1. Resolve the caller and callee accepted artifact locations. A subject with no
   valid source/location becomes a typed subject loss; it is not silently
   omitted.
2. Create subject window candidates from each endpoint's exact inclusive source
   span. Create support candidates only from deterministically discovered,
   source-backed anchors.
3. Sort candidates by `(priority, role, source_artifact_id, start_line,
   end_line, owner_id)`, where subject priority precedes support and role order
   is `callee`, `caller`, `support`.
4. Process candidates in that order. Windows from different files never merge.
   In one file, overlapping or immediately adjacent candidates merge only when
   their union satisfies every per-window bound. The merged record contains the
   sorted union of owner IDs and roles.
5. If an overlap cannot merge within bounds, retain the already admitted
   higher-priority/canonically earlier window and emit a typed loss for the
   rejected candidate. Accepted windows in one file must therefore be disjoint
   and separated by at least one unselected line.
6. Apply per-file and total-window caps only after subject priority. An excluded
   subject produces a high-severity typed loss. An excluded support candidate
   produces a low-severity typed loss.
7. Sort final windows by `(source_artifact_id, start_line, end_line,
   window_id)`. Sort unknowns and losses by their complete canonical keys.

A window is a first-class projection source. Its ID is derived with kind
`context-window` from the policy ID, snapshot ID, obligation ID, source artifact
ID, registration ID, content/CAS hashes, inclusive range, sorted owner IDs, and
sorted roles. It also records exact byte length and excerpt hash. Multiple
nonoverlapping windows may therefore reference one file without pretending the
whole file was included. Reviewer claim grounding in v2 must use admitted
window IDs; each window retains its underlying accepted file/source ID.

A subject loss must contain a stable loss ID, reason enum, affected endpoint
ID, source artifact ID when known, requested range when known, property ID,
severity `high`, and a recovery reference to the exact source/range. Supported
reasons must include missing location/source, giant line, per-window lines,
per-window bytes, per-file window cap, total window cap, total excerpt bytes,
and overlap-unmergeable. A subject loss prevents an unconditional
`issue_absent`, pass, or complete-property conclusion for that obligation.

For frozen v2, the projection hash preimage must be the canonical bytes of exactly:

- policy ID, complete policy DTO, and policy hash;
- snapshot and obligation IDs;
- target relation, caller, and callee IDs;
- the complete candidate source/window denominator;
- every admitted window record, including its source/registration/content/CAS
  identity, range, owners, roles, byte length, and excerpt hash;
- every excluded candidate, unknown, assumption, and loss record; and
- the fixed canonical ordering rules above.

The envelope ID must continue to be derived from the projection hash. Raw
source bytes are not retained in the envelope; their exact admitted excerpts
are bound by length and hash. Any range, role, owner, order, loss, policy, or
source identity mutation must change the projection hash or fail validation.

#### 5.4 Repository-scale amendment: `context.subject_windows@3`

Sections 5.1--5.3 remain the complete decode, validation, and replay contract
for `context.subject_windows@2`. Its canonical bytes and hash remain exactly
unchanged:

```text
sha256:7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26
```

No v2 constant, DTO field, ordering rule, envelope identity, fixture, or
accepted historical byte may be raised, filtered, reinterpreted, or rewritten.
All new generic D executions, the section 11 quickstart, and m20 Stage 0 instead
use the separately versioned policy `context.subject_windows@3`.

This amendment follows the same rule as section 3.4: retain exact commitments
to every observed accepted item while bounding the number of materialized
records. It does not turn a count of retained rows into a coverage denominator.
The motivating positive pair
`a8b6b24d5ed704f53f721b25db42d5d631f946c7` to
`8569a2261e8a62145228872a2fde9f4c48093d00` generated two substantive D
obligations. Its target tree contained 4,816 accepted file artifacts, while one
planned obligation reached one file, 260 structural IDs, 259 exact support
anchors, and two subjects. Bypassing only the v2 4,096-file bound then failed
at the 65th loss under the v2 64-loss bound. Thus 4,816 was the complete file
denominator, not an anchor count, and subject-first ordering was operating.
Raising either v2 bound would conceal, not solve, record materialization scale.

The complete v3 policy DTO is the following single-line UTF-8 JSON with no BOM
or trailing newline:

```json
{"accepted_file_denominator_bound":"request.ingest.max_files","anchors_per_file":1024,"assumptions":"empty","callees_depth":3,"callers_depth":2,"candidate_order":["subject_priority","distance","path_rank","artifact_id"],"canonical_envelope_bytes":786432,"contains_edges":1000000,"discovery_paths":20,"edge_kind_direction_order":["calls:forward","calls:reverse","contains:forward","contains:reverse","covers:forward","covers:reverse"],"excerpt_lines":400,"final_window_order":["source_artifact_id","start_line","end_line","window_id"],"included_files":64,"latent_cardinality":"known_zero_or_unknown_with_qualification_ids","loss_reason_precedence":["missing_location","missing_source","giant_line","per_window_lines","per_window_bytes","per_file_window_cap","total_window_cap","total_excerpt_bytes","overlap_unmergeable","path_cap","test_cap","not_reached","included_file_cap","artifact_bytes_cap","total_resolved_bytes_cap"],"materialized_source_denominator":"subject_file_ids_union_reached_file_ids","max_assumptions":64,"max_discovered_structural_ids":4096,"max_excerpt_bytes":262144,"max_materialized_source_candidates":4096,"max_resolved_artifact_bytes":1048576,"max_resolved_bytes":8388608,"max_string_bytes":16384,"max_subject_losses":2,"max_support_loss_summaries":15,"max_total_excerpt_bytes":1048576,"max_unknowns":64,"obligations_per_envelope":1,"policy_id":"context.subject_windows@3","related_tests":10,"relation_scan":1000000,"seed_fields":["source_ids","target_refs","context_ids"],"source_candidate_denominator":"all_accepted_file_ids_known_count_and_sorted_id_set_sha256","subject_endpoints":2,"subject_order":["callee","caller"],"support_anchor_denominator":"reached_range_bearing_exact_path_anchor_ids_known_count_and_sorted_id_set_sha256","support_loss_summary":"reason_known_count_and_sorted_anchor_id_set_sha256","unknown_reason_ids":["unresolved_invariant_scope","unresolved_relation_endpoint","unresolved_review_context_member","unresolved_seed_reference"],"window_candidate_order":["priority","role","source_artifact_id","start_line","end_line","owner_id"],"window_merge":"same_source_overlap_or_adjacent_if_union_within_per_window_bounds","windows_per_envelope":8,"windows_per_file":4}
```

Its golden hash is exactly
`sha256:932bfa18c5d286c63196366d6d2dc1aaf402f50baa1f1ab5f075b1007be55dd8`.
The v3 type accepts only those canonical bytes after RFC 8785/JCS
canonicalization and hash recomputation.
Changing a field, token, order, cap, denominator rule, or aggregation rule
requires another policy ID. The fixed `rust.production.v1` DTO and its hash
remain unchanged at
`sha256:4b6cca93794ab03b1576e17d2e395ec43f731d316685247a89363ae2e840dd96`.

##### 5.4.1 Exact denominator commitments

V3 derives four separately named domains; none may be inferred from the number
of serialized detail rows:

1. `accepted_file_denominator` is exactly every accepted `kind == "file"` ID
   covered one-to-one by the snapshot source-registration closure. Its bound is
   the request's already validated `ingest.max_files`, not 4,096.
2. `reached_file_denominator` is exactly the accepted file IDs reached by the
   unchanged section 5.2 discovery and reverse-containment algorithm.
3. `materialized_source_denominator` is exactly the sorted union of both valid
   subject source-file IDs and `reached_file_denominator`. Its 4,096 bound is
   checked before materialization. Non-reached files are never cloned into
   candidate records or emitted one-by-one merely to prove that they were not
   reached.
4. `support_anchor_denominator` is exactly every `(source file, range-bearing
   accepted owner, inclusive range)` satisfying section 5.2's reached and
   exact-path rules.

Every m20 Stage 0 product request sets `ingest.max_files` to exactly **20,000**,
the closed v3 request-schema maximum and the value already fixed by the section
11 quickstart. This admits the measured complete 4,816-file denominator without
introducing a corpus-fitted 5,000 cutoff. It is an ingest admission/resource
bound, not a Stage 0 sampling or coverage denominator: `C` remains all 300
clusters, and `A_c`, `S_c`, and `D_c` are derived only from each cluster's
complete accepted facts. Exceeding 20,000 is a typed Stage 0 failure; it may not
truncate the accepted-file set, remove the cluster from `C`, or reinterpret it
as model-ineligible. The separate materialized-source bound remains 4,096. This
literal is included in the sixth atomic m20 seal.

Each domain is serialized as a closed known-cardinality commitment containing
`cardinality = "known"`, checked `observed_count`, and
`sorted_id_set_sha256`. The digest is SHA-256 of the canonical sorted JSON
array of the exact IDs, using the same algorithm as section 3.4; the count must
equal the rebuilt set cardinality. Accepted file IDs are the existing file
StableIds. A support-anchor ID is
`StableId::derived("context-support-anchor", {"anchor_contract":
"context.support_anchor@1", "end_line": end, "owner_artifact_id": owner,
"snapshot_id": snapshot, "source_artifact_id": file, "start_line": start})`.
All four commitments are projection-hash inputs. Live validation rebuilds them
from accepted state and exact source registrations; replay requires their
sealed hashes and exact snapshot/request binding.

The envelope also carries a separate `latent_cardinality` closed union. It is
`{"state":"known_zero"}` only when the capabilities governing that domain are
complete. Otherwise it is `{"state":"unknown","capability_states":...,
"qualification_ids":...}` with nonempty exact source-backed qualification IDs.
An unknown latent count has no numeric value, lower/upper bound, empty-set
interpretation, or contribution to an observed count. Thus v3 records the
known accepted denominator without presenting unknown unobserved structure as
known, and never uses summary-row cardinality as file or anchor cardinality.
Every actually observed unresolved context member remains an individual typed
unknown with its source/qualification IDs. Exceeding the fixed unknown-record
bound fails context construction with a typed overflow containing the observed
count and set digest; omission, sampling, or folding an observed unknown into
a known support-loss summary is forbidden.

##### 5.4.2 Subject-first materialization and exact loss aggregation

V3 retains exactly two ordered subject outcome records, callee then caller.
Each names the endpoint and is a closed union of either `admitted`, with its
source file, requested range, and admitted window ID, or `lost`, with the
existing high-severity typed subject-loss body and recovery reference. Before
support processing, the builder reserves both subject source/file/window slots
and processes every representable subject. A support file, anchor, window, or
loss-summary row cannot consume those reservations. Missing, invalid, or
over-bound subject source/range remains a named subject loss and prohibits an
unconditional issue-absent or complete conclusion; it is never replaced by a
digest-only summary.

Only subject and reached files enter materialized candidate metadata. Source
bytes are resolved subject-first, then by the unchanged deterministic reached
rank, under the existing 64-file and byte bounds. Every admitted v3 window
retains the sorted support-anchor IDs it covers in addition to its existing
source/range/owner/role identities. A known support anchor not covered by an
admitted window belongs to exactly one nonempty `support_loss_summaries` row,
selected by the existing loss-reason precedence. There is at most one row per
reason and therefore at most 15 rows. Each row contains exactly `reason`,
`cardinality = "known"`, checked `observed_count`, and
`sorted_anchor_id_set_sha256`. Rows are sorted by reason precedence.

The rebuilt admitted-anchor set and every rebuilt loss-summary set must be
pairwise disjoint and their union must equal `support_anchor_denominator`.
Counts and digests must match those exact sets. Dropping an anchor, sampling,
truncating at 64 losses, moving a known rejected anchor into latent unknown,
or reporting an unknown latent count as a known zero/count is schema-invalid.
Unknowns from incomplete accepted structure remain the separate typed unknown
and latent-cardinality records; they are never fabricated as support anchors.
This is the context analogue of section 3.4's occurrence summaries: detail-row
retention is bounded, while observed identity, source trace, reason partition,
known cardinality, and unknown cardinality remain independently auditable.

The v3 projection hash preimage contains the complete v3 policy bytes/hash,
snapshot/request/obligation/relation/endpoint identities, all four denominator
commitments and latent-cardinality records, both subject outcomes, every
materialized source and admitted window, every support-loss summary, and all
remaining unknown/loss/source identities. The basis-bound semantic validator
in section 5.4.3 rebuilds every set and partition; trusting caller-supplied
counts or digests is forbidden.

##### 5.4.3 Wire validation and semantic validation basis

Wave-5 option C is the final decision. It incorporates the earlier option B
boundary—a trusted basis is required and bytes-only validation remains
structural—but adds the independent reached/support-anchor oracle specified
below because production re-execution alone cannot detect common-mode builder
omissions. The compact v3 wire intentionally does not serialize the
accepted-file ID set, reached-file ID set, or per-reason lost-anchor ID sets.
Consequently canonical context bytes alone cannot prove that their declared
counts/digests are complete relative to the accepted snapshot. A bytes-only
validator may check closed schema/canonical encoding, policy and projection
hashes, ID preimages visible on the wire, ordering and caps, the materialized
set reconstructed from `materialized_sources`, admitted-anchor closure from
windows, and internal count/digest syntax. It must be named and documented as
**wire validation**, not semantic validation. A coherently changed commitment
plus recomputed projection/context IDs can remain wire-valid.
Accordingly `validate_subject_windows_v3_read_only` must not be the semantic
entry point or return a semantic validated type. The implementation must expose
distinct wire and basis-bound entry points (for example
`validate_subject_windows_v3_wire_read_only` and
`validate_subject_windows_v3_against_basis`) with non-interchangeable result
types; naming alone must not imply a reconstruction that did not occur.

Full semantic validation requires a trusted `ContextValidationBasisV3` supplied
separately from the wire. The basis contains the exact accepted ProgramSpace,
snapshot source-registration closure and source index, relation-adjacency
index, request `ingest.max_files`, obligation/relation/caller/callee bindings,
and fixed policy bytes/hash. It is admitted only by deterministic live ingest
of the immutable Git snapshot or by a sealed replay basis whose ProgramSpace,
source-registration, snapshot, request, and extractor hashes match the run;
caller-supplied denominator sets, counts, or digests are not a basis.

Core is the sole semantic implementation. Given `(canonical_context,
ContextValidationBasisV3)`, it uses two deliberately distinct checks. A simple
reference oracle walks accepted ProgramSpace artifacts, relation adjacency, and
containment facts directly, without calling `prepare_subject_windows_v3`, its
reached-set or anchor-enumeration helpers, or its subject-first/materialization
indexes. It independently rebuilds the exact accepted-file, reached-file, and
support-anchor ID sets and their counts/digests. The production builder is then
re-executed from the basis
to rebuild accepted and materialized commitments, latent known/unknown state,
both subject outcomes, windows, admitted anchors, per-reason lost-anchor sets,
and every partition. The oracle sets must first equal the corresponding
builder sets, after which all rebuilt counts, digests, losses, projection hash,
context ID, and canonical bytes must match the supplied context. Either
mismatch is typed semantic failure.

Here **independent** means implementation-independent for the
two graph-discovery denominators whose silent omission would otherwise be a
common-mode builder failure; it does not claim a second implementation of the
complete window/loss policy. Option A is rejected because duplicating every
ranking, cap, merge, loss-precedence, and serialization rule would create a
second versioned policy and add an exhaustive pass for every policy detail.
Option B alone is superseded because wire independence plus production
re-execution accepts the demonstrated support-anchor omission when builder and
validator share the defect; its trusted-basis/wire-only boundary remains part
of option C. The
reference oracle may be slow and simple, but its traversal code and tests must
not share production discovery helpers. Its operation counts and elapsed time
on the 4,816-file positive pair and on the frozen Stage 0 corpus must be
measured before Stage 0; it is a validator, not a new materialization path, and
must request no source bytes.

The admitted basis is retained only until that obligation has been validated;
it is not owned by the returned semantic token or run. Runtime constructs one
immutable run-scoped accepted-snapshot basis (aggregate, source index/bytes,
and validated bindings), may share it by `Arc`, and borrows a lightweight
obligation view for construction and immediate validation. That view is
dropped after validation. The validated result retains only canonical run data
and compact basis-identity/validated-context receipts, never a deep clone of
ProgramSpace or source bytes per obligation. `Arc` is an ownership optimization,
not an authority source, and cannot outlive or replace admission checks.

Run-v3 decoding is therefore two-level. Bytes-only decoding returns an
unvalidated wire value and cannot claim denominator completeness or construct
a semantically validated human report. A validated run type is produced only
by live construction that retained its admitted basis through immediate
validation or by decoding followed by basis-bound validation for every context.
The basis may then be discarded as specified above. The CLI/report path must consume
that validated type (or the bytes plus matching basis). Bytes-only report
checking may verify schema, canonical audit/report hashes, and projection
equality, but must be labeled projection validation and cannot recover the
missing source semantics. Section 11 `verify.py` remains an exact artifact-
byte verifier, not a semantic denominator validator. A third party can repeat
semantic validation by deterministically re-ingesting the immutable Git pair;
the context bytes alone are insufficient, and this limitation must be stated.
Any basis-free function named `decode_and_validate_generic_review_run_v3` must
therefore be narrowed/renamed as wire validation or changed to require the
basis; it cannot retain a semantic-valid return contract.

The earlier wire-shape options A and D remain rejected. Including ID arrays
would still not prove
completeness without ProgramSpace, because an attacker can change the arrays
and reseal their counts/digests. It also defeats bounded aggregation: one set
of 4,816 80-byte StableId strings costs roughly 400 KiB before object/field
overhead; accepted plus lost-anchor sets alone can exceed the 786,432-byte
envelope, before reached IDs, windows, subjects, unknowns, or losses. The
earlier wire-shape option C is rejected because generation-time type guarantees
alone would abandon independent semantic rebuilding. No ID-set arrays are added, so the canonical
v3 policy DTO/hash, run-v3 schema, and 786,432-byte envelope limit are
unchanged. Basis working sets remain internally bounded by request
`ingest.max_files`, 4,096 materialized files, 1,024 anchors per file, and the
existing discovery/relation/source-byte resource caps; they are not wire rows.

This correction changes no context-v2, context-v3, profile, request/run, packet,
or report canonical bytes or hashes. It does change m20's semantic acceptance
algorithm because the evaluator's frozen internal basis must now pass the
independent reached/support-anchor oracle, and the demonstrated omitted-anchor
mutant becomes a required rejection. Therefore m20 requires one coordinated
re-freeze, including the oracle implementation, mutant, reference vectors,
operation/time measurements, and freeze manifest, before Stage 0. No Stage 0 or
model result has been observed, so this is permitted. The previous manifest
hash
`sha256:95b75a62353b56111fe8913700c72bf9881f93efdc99629e72adb3e2c6a9fa73`
identifies the superseded pre-oracle freeze and must not be presented as the
active Wave-5 freeze. Re-freezing must not change any canonical context/run
bytes, primary endpoint, scorer, gate threshold, or reviewer-visible packet.

The completed atomic m20 re-freeze rebuilt its treatment-arm contexts, source
inventories, packets, hidden bindings, and their hashes under
`context.subject_windows@3`, together with section 3.4's occurrence-summary
amendment. Its evaluator, vectors, mutations, preregistration, and freeze
manifest test the v3 commitments, subject outcomes, and support-loss closure;
the baseline algorithm and arm-neutral task-ID preimage remained unchanged.
That original wire/algorithm change required the re-freeze before Stage 0. The
subsequent Wave-5 oracle correction above reopens only semantic validation and
requires the additional coordinated re-freeze just specified.

### 6. Verifier seam: `workspace.cargo_test@1` is deferred

This slice must not execute `workspace.cargo_test@1`. A valid v2 or v3 request may
name that fixed registry ID only to exercise the verifier seam; the result is
the canonical typed outcome `unsupported` with reason
`workspace_cargo_test_deferred`. No process may be spawned, no executable may
be resolved, and no verifier-observation/evidence-material record may be
created. The resolved-target `verifier_observed` numerator is therefore empty
for every conforming `@1` run. A request containing any command, executable,
argv, path, cwd, environment, mount, cache, credential, toolchain, or resource
limit field is schema-invalid rather than unsupported.

When the ID is selected, the run contains exactly one closed unsupported record
with descriptor ID, request/snapshot/universe IDs, `outcome = "unsupported"`, the
fixed reason, `process_started = false`, and `executable_resolved = false`.
Its StableId preimage binds exactly those fields. When verification is disabled
the record is absent. Any extra field, different reason, process/executable
identity, output, resource observation, or obligation binding is invalid.

This deferral is a security decision, not a claim that `cargo test` is inert.
The repository, every workspace member and dependency, `build.rs`, proc macro,
test binary, doctest, Cargo plugin/configuration, and compiler input must be
treated as untrusted executable input. `--locked` and `--offline` constrain
resolution/network behavior; they do not prevent arbitrary code execution.
Repository, workspace, parent-directory, and `CARGO_HOME` Cargo configuration
can select wrappers, runners, linkers, environment, credentials, aliases, and
absolute executable paths. Read-only source and fixed top-level argv alone do
not contain those execution paths.

A successor ADR may activate this registry ID only after it fixes and tests at
least all of the following as one versioned descriptor contract:

- non-root UID/GID, all Linux capabilities dropped, `no_new_privileges`, and a
  syscall policy that denies namespace creation, mount/pivot/chroot, ptrace,
  keyring, BPF, perf, raw I/O, and privilege-changing calls;
- separate mount, PID, IPC, UTS, user, and network namespaces, with no network
  interface including loopback, a minimal read-only `/proc`, and no host
  `/sys`, device tree, sockets, or device nodes beyond fixed null/zero/random;
- a read-only, nodev/nosuid repository mount and read-only toolchain/cache;
  fresh bounded `/tmp` as nodev/nosuid/noexec; and a fresh bounded target mount
  as nodev/nosuid but executable, because Cargo must execute build scripts,
  proc-macro helpers, and test binaries from build output. The executable
  target mount is why syscall, namespace, identity, and mount isolation cannot
  be omitted;
- an exact Cargo-config algorithm that rejects every `.cargo/config` or
  `.cargo/config.toml` in the repository, workspace, or any searched parent,
  supplies a fresh `CARGO_HOME` containing no config or credentials, and admits
  dependency cache content only after proving it contains neither config nor
  credentials. Normalizing selected keys while retaining unknown Cargo config
  is forbidden;
- exact Cargo/rustc/linker/toolchain/sandbox identities, fixed argv and closed
  environment, canonical snapshot-bound cwd, no shell, no model-controlled
  value, and time-of-check/time-of-use identity closure;
- inclusive wall, CPU, RSS/address-space, process/thread, output, writable-byte,
  inode, open-file-descriptor, and per-file-size limits, with descendant
  termination and cleanup; and
- adversarial tests for malicious build scripts, proc macros, tests, doctests,
  repository and parent Cargo config, wrapper/runner/linker and absolute-path
  execution, fork/file/inode/output/memory exhaustion, network/loopback,
  mount-external reads/writes, device/proc access, namespace/symlink escape,
  cache credentials/config, and every exact/+1 resource boundary.

Until that successor exists, there is no execution/dedup/reuse contract and no
obligation-level binding of a workspace test observation, so the prior
workspace-wide cost ambiguity does not arise. Model stages and the practical-
utility gate must record `verifier_available = false`; primary endpoint
`usable_grounded_disposition_completed` cannot require or receive credit for
verifier execution. A model's prose, source text, tool calls, or
`requested_evidence` can at most produce an
untrusted evidence request plus the fixed typed unsupported result. They can
never create argv/path/env or accepted Evidence, Verification, Decision,
Finding, `trusted_pass`, or human acceptance. The two process-free M4
descriptors and ADR 0021 remain unchanged.

### 7. Deterministic observer `deterministic.abstain@1`

`deterministic.abstain@1` is a provider-free, repository-independent observer.
For every valid single-obligation v2 or v3 reviewer request it must return exactly one
canonical typed abstention with:

```text
reason = required_evidence_unavailable
detail = "deterministic.abstain@1 does not evaluate semantic properties"
```

The response must use the current structured reviewer-output schema, caller-
fixed execution ID, and no claims after the provider-free public response in
section 8.2.1 is deterministically lowered into the generic run. The public
response bytes themselves use the closed provider-free abstention schema and
are retained, with their hash, in a non-authority deterministic observation
record containing the observer ID/version and input manifest hash. Neither
form may contain provider, model, credential, process, tool, or fixture
metadata.

The observer may validate request closure and bounds. It must not branch on
repository identity, source bytes, property name, relation name, path, fixture
key, benchmark name, environment, network, or filesystem state. It performs no
I/O and never returns a claim, evidence request, verification, or accepted
state.

This observer is not `FakeReviewer`. `FakeReviewer` performs a named lookup by
immutable obligation/snapshot fixture key and may return fixture-specific
responses. The deterministic observer must own no fixture table, fixture key,
expected repository, or response lookup and must work for any valid obligation.
The generic v2 and v3 observation records must each be a closed union of process, replay,
and deterministic records; a deterministic result must not be forged as a
synthetic process record.

### 8. Public schemas and compatibility

The implementation must add these closed Draft 2020-12 schemas with
`additionalProperties: false` on every object:

- `reviewgraphen.generic_review_request.v2`;
- `reviewgraphen.generic_review_run.v2`;
- `reviewgraphen.generic_review_human_report.v1`;
- `reviewgraphen.generic_review_request.v3`;
- `reviewgraphen.generic_review_run.v3`;
- `reviewgraphen.generic_review_human_report.v2`;
- `reviewgraphen.generic_review_diagnostics.v1`;
- `provider-free.source-grounded-packet@1`;
- `provider-free.source-grounded-abstention@1`; and
- `provider-free.source-inventory@1`.

#### 8.1 Request v2

Request v2 must retain immutable repository/base/target and bounded ingest/plan
inputs, and must additionally select the v2 rule/profile contract, fixed context
policy, closed observer kind, and optional deferred verifier descriptor. The
observer union must include Codex CLI, Claude CLI, app-server-as-unsupported, replay, and
`deterministic.abstain@1`. The request must not inline rule definitions,
profile exclusion matchers, capability states, context-policy parameters,
verifier argv/path/env/limits, report authority, or `trusted_pass`. The only
non-null verifier ID is `workspace.cargo_test@1`, and it deterministically
selects the unsupported seam in section 6 rather than execution.

The retained v2 compatibility path may execute D only through its frozen v2
request/profile/rule-pack/context combination. Every new D execution uses the
v3 request boundary in section 8.5.
Repository identity, hidden flags, environment variables, executable aliases,
or fixture names must not select another algorithm, preserving
[ADR 0029](./0029-generic-only-review-command.md).

#### 8.2 Reviewer-visible arm-neutral disposition contract

This subsection is exclusively the m20 model-evaluation contract. It is not a
generic-runtime or quickstart packet contract.

Model-evaluation reviewer packets must use the closed disposition contract
`arm-neutral.source-grounded-disposition@1`. The caller supplies exactly one
opaque task ID of StableId kind `review-task`, serialized as
`review-task:sha256:<64 lowercase hexadecimal digits>`. For m20 pair unit `u`,
the ID is derived once and shared byte-for-byte by both arms:

```text
StableId::derived("review-task", {
  "comparison_contract": "m20-changed-public-callee-utility-v1",
  "task_contract": "arm-neutral.source-grounded-disposition@1",
  "unit_id": u
})
```

No arm, rule, property, obligation, relation, caller, callee, endpoint, subject,
window role, or source-inventory identity enters this preimage. `unit_id` is the
sealed commit-cluster ID common to both arms. The reviewer-visible packet
contains the task ID, closed arm-specific source inventory, arm-neutral
instruction, and response schema. Reviewer output contains only `schema`,
`task_id`, the arm-specific source-inventory ID, and a closed disposition
union; `schema` is exactly `arm-neutral.source-grounded-disposition@1`. The
visible disposition vocabulary is the arm-neutral set `issue_present`,
`issue_absent`, `inconclusive`, and
`abstention`; it does not name what property is being scored. The common
instruction may say only that the reviewer must inspect admitted Rust source
and return a source-grounded disposition under that vocabulary. It must not
describe callee contracts, D, or the hidden property.

After schema validation, the caller—not the model—derives the parsed record ID
as `StableId::derived("review-disposition", {"disposition_hash": <SHA-256 of
the canonical disposition union>, "source_inventory_id": <exact visible
inventory ID>, "task_id": <exact visible task ID>})`. That ID and raw/parsed
hashes are audit fields and are not requested from the reviewer. Changing an
arm's sources or disposition changes this record ID without changing the common
task ID.

In particular, neither arm's reviewer-visible instruction, schema, structured
metadata, inventory, or generated wrapper may contain any of:

- `relation.changed_public_callee@1` or
  `rust.callee_contract_review@1`;
- a `rule_id`, `property_id`, `obligation_id`, `relation_id`, `endpoint_id`,
  `caller_id`, `callee_id`, or `subject_id` field;
- a typed role/value `caller`, `callee`, `endpoint`, or `subject`; or
- an explanation that the selected sources were chosen by the D trigger or
  subject-first projection.

The prohibition applies to evaluator-owned metadata and instructions; exact
repository source bytes and paths remain unmodified even if user code happens
to contain one of those strings. B may expose opaque admitted window/source IDs,
ranges, hashes, and bytes, but not their caller/callee/subject roles. A and B
must receive byte-identical task ID, disposition schema, and instruction. Only
their admitted source inventory and corresponding inventory/hash IDs differ.

Before adding arm-specific sources, the evaluator constructs one
**shared change-evidence core** from the authenticated selected-D binding and
the pinned base/head trees. It contains (a) the complete profile-defined
base/head production-diff inventory produced by the frozen baseline planner,
using its existing three-line expansion and merge rules, and (b) the selected
changed public callee's exact complete head implementation source range. These
bytes, normalized paths, ranges, snapshot sides, IDs, and hashes are identical
in A and B; visible roles
remain only the generic `changed`, `context`, or `support` vocabulary and never
reveal the binding. A is exactly this core. B is the union of this core and the
admitted `context.subject_windows@3` inventory. Exact duplicate source
keys/payloads are serialized once; a conflicting duplicate is invalid. The complete union in
each arm independently remains subject to the frozen 65,536-byte ceiling, with
no padding or truncation. Failure to derive or close a mandatory core source is
the same typed task-blocking source/reference loss in both arms and must never
be silently omitted.

Thus m20 measures the incremental utility of subject-window context beyond a
shared task/change basis; it does not compare a diff-aware baseline with a
change-blind projection. Updated tests are included in both arms when the
already frozen profile/baseline rules admit their changed hunks; other context
remains included only when the subject-window rules admit it. There is no
special test-file exception. This is an evaluator packet-contract
change, not a change to `context.subject_windows@3`, its DTO, projection hash,
or the C/A/S/D denominators. The packet constructor, schemas, hidden bindings,
reference vectors, attacks, and hashes require one new atomic seal. Stage 0's
semantic sets and seven gate definitions are unchanged, but packet byte counts,
model eligibility, and the Stage 1 selection manifest can change, so Stage 0
must be rerun under that seal and an earlier Stage 0 output is not reusable.

The property relation remains auditable through a **hidden task binding** in
every canonical run v3 participating in m20. For each task, the binding
contains task ID, comparison/unit IDs, exact sorted obligation IDs,
rule/property IDs, target relation IDs,
caller/callee endpoint IDs, subject/window role records, this run's hidden arm
ID, and this run's packet and inventory hashes. Its binding ID is derived from
that complete canonical body. Each arm records its own binding; the evaluation
audit joins the two only by their common task ID after both outputs are sealed.
The semantic validator reconstructs the D side from the v3 universe, plan,
context, and evaluator's frozen internal validation basis, and validates the
baseline side against the sealed comparison-unit manifest. The scorer consumes
bindings server-side. A binding must never be
copied into, joined onto, or made queryable from a reviewer packet before both
arms finish.

The canonical human-report JSON manifest must retain its run's binding ID,
complete binding body, and reviewer-packet hash. The joined evaluation audit
retains both binding bodies and packet hashes. Deterministic Markdown may render
the join only after both arm records are sealed and must be labeled evaluation
audit, never reviewer input. Thus hidden means hidden from the model reviewer,
not erased from canonical audit state.

Required negative fixtures must inspect the complete canonical A/B packet
bytes, instruction bytes, schema, and inventory metadata and reject every
forbidden field/value above, D-specific natural-language cue, foreign role, or
attempt to derive task ID from property/arm/source identity. Separate fixtures
must prove that source bytes containing the same literal strings are preserved
without being mistaken for evaluator metadata. Mutating a hidden binding,
packet hash, or task-to-property/obligation closure must fail run-v3 and
human-report-v2 semantic validation.

#### 8.2.1 Provider-free generic packet contract

The provider-free generic path adopts a separate contract, option (b). Every
generic v2 or v3 execution whose selected observer is
`deterministic.abstain@1` uses
the closed reviewer-visible packet schema
`provider-free.source-grounded-packet@1`. Selection follows only from that
already validated observer union member; no fixture name, hidden flag,
environment value, evaluator wrapper, or extra request field selects this
path. The generic runtime is the sole packet builder. A request, CLI caller,
replay record, or observer cannot supply a packet, task ID, or binding.

For canonical execution ID `e`, the runtime derives exactly:

```text
StableId::derived("provider-free-review-task", {
  "execution_id": e,
  "packet_contract": "provider-free.source-grounded-packet@1",
  "request_contract": exact_request_schema
})
```

Here `exact_request_schema` is exactly
`reviewgraphen.generic_review_request.v2` or
`reviewgraphen.generic_review_request.v3`, matching the already dispatched
request major. It is not caller-selectable. This preserves every v2 task ID
byte-for-byte while giving v3 a disjoint task-ID domain.

The resulting visible ID is serialized as
`provider-free-review-task:sha256:<64 lowercase hexadecimal digits>`. This
preimage is valid only for the provider-free contract. In particular it is
forbidden as an m20 task-ID derivation and does not alter the m20
`{comparison_contract, task_contract, unit_id}` rule or its prohibition on
source/obligation identity.

The packet object contains exactly `schema`, `task_id`, `instruction`,
`response_schema`, `source_inventory`, and `payloads`. `instruction` is exactly
`Return the fixed provider-free abstention using the supplied closed schema.`
The embedded response schema admits only this object shape:

```json
{
  "schema": "provider-free.source-grounded-abstention@1",
  "task_id": "<exact packet task ID>",
  "source_inventory_id": "<exact packet inventory ID>",
  "disposition": {
    "kind": "abstention",
    "reason": "required_evidence_unavailable",
    "detail": "deterministic.abstain@1 does not evaluate semantic properties"
  }
}
```

Every object is closed and the response has no claim variant. The inventory
contains exactly `schema`, `source_inventory_id`, sorted `admitted_sources`,
and `canonical_sha256`; its schema is
`provider-free.source-inventory@1`. Each admitted source contains exactly
`source_id`, normalized repository-relative `path`, inclusive
`range {start_line,end_line}`, `payload_id`, positive UTF-8 `bytes`, and
payload-byte `sha256`. It contains no semantic role. A payload contains exactly
`payload_id`, `encoding:"utf-8"`,
`media_type:"text/x-rust; charset=utf-8"`, positive `byte_length`, `sha256`,
and exact strict-UTF-8 `text`. IDs are derived exactly as:

```text
StableId::derived("source-payload", {
  "byte_length": byte_length,
  "sha256": sha256
})
StableId::derived("source", {
  "bytes": bytes,
  "path": path,
  "payload_id": payload_id,
  "range": range,
  "sha256": sha256
})
```

Sources and payloads sort by ID; equal payload `(sha256,byte_length)` pairs
deduplicate; every source references exactly one payload and every payload is
referenced.

Let `inventory_body` be exactly
`{schema:"provider-free.source-inventory@1",admitted_sources}`.
`source_inventory_id = StableId::derived("source-inventory", inventory_body)`
and `canonical_sha256` is the SHA-256 of its canonical bytes; neither enters
its own preimage. The runtime validates all path/range/hash/length/payload
closure before constructing the packet, then deterministically lowers the
fixed response to the generic structured abstention bound to `e`. Neither
representation is Evidence or authority.

The complete reviewer-lens prohibition in section 8.2 applies independently
to this packet's metadata, instruction, embedded response schema, inventory
wrapper, and output: none may expose or name the D rule/property, obligation,
relation, endpoint, caller, callee, subject, or window role. Exact admitted
repository bytes and paths retain the same literal-source exception. Only the
opaque task ID is visible; its execution-ID preimage and the run's semantic
closure are not included or queryable in the packet.

Each generic run internally constructs a closed provider-free packet binding
containing the task ID, execution ID, packet and inventory hashes, input
manifest hash, observer ID/version, and observation-record ID. Its binding ID
is derived from the complete canonical body. The matching run-v2/human-report-v1
validators retain their existing contract. Run-v3 rebuilds the binding and
execution/context/source closure only through section 5.4.3's admitted basis;
human-report-v2 then validates its projection from that validated run. A
bytes-only report check does not rebuild source semantics. The
binding is audit state and is never reviewer input. This preserves section 9's
authority ceiling.

The two packet families are non-substitutable: their schema names, task-ID
kinds, response schemas, builders, and audit bindings differ, and every
cross-decode or cross-binding must fail. The superseded sealed m20 pipeline was
the only constructor of `arm-neutral.source-grounded-packet@2`; the section 8.2
shared-change-evidence successor is
`arm-neutral.source-grounded-packet@3`. Its pipeline remains the only
constructor and continues to reject caller-supplied packets. The packet-family
separation itself changes no m20 algorithm; the explicit section 8.2 amendment
does and therefore requires the stated successor seal. Section 5.4 independently
changes m20 context construction and was included in the completed coordinated
re-freeze specified in sections 3.4 and 5.4.3.
Allowing m20 to accept or
construct the provider-free contract would instead require a new evaluator
version and complete re-freeze.

#### 8.3 Run v2

Run v2 must contain the exact ProgramSpace and extraction-report-v1 identities
under `legacy_ingestion` and the separately validated
`reviewgraphen.ingestion_report.v2` under `ingestion_report_v2`, followed by the
v2 obligation contract with both capability fields, two-layer denominator,
complete plan and
deferrals, `context.subject_windows@2` envelopes, the closed observation-record
union, structured claims/abstentions/failures, the closed verifier-unsupported
record, the provider-free packet binding when that observer is selected,
coverage ID sets, limitations/losses, and the fixed authority ceiling.
It must contain no verifier process or evidence-material record and must
validate that every `verifier_observed` ID set is empty. Its
semantic validator must rebuild all set partitions and source/target/hash
closure rather than trusting serialized counts.

Run v2 must not deserialize a v1 obligation contract and infer an empty
enumeration set. It must reject v1 policy/envelope/record bodies under a v2
schema tag. Replay records must match the same request/run major version and
exact packet/input hashes.

#### 8.4 Human report v1

Human report v1 is the frozen projection of run v2 only. Its schema, canonical
bytes, fixtures, decoder, and semantic validation remain byte- and meaning-
stable; it must reject a run-v3 audit schema/hash or any v3 context field rather
than add a run-version union.

The human-report-v1 schema describes a closed canonical JSON projection manifest,
not authority state. It must include the canonical audit schema/run/snapshot/
universe IDs, SHA-256 of the complete canonical run-v2 audit JSON, fixed
non-authority/trusted-pass fields, proposed claims and abstentions, verifier
unsupported status, resolved-target denominator and stage IDs, candidate-space
capability/limitation/gap trace, exclusions/deferrals, source/window IDs,
information loss, and SHA-256 of the rendered Markdown bytes. For a
provider-free run it also retains the provider-free binding ID and complete
binding body plus the reviewer-packet hash; these remain non-authority audit
fields and are never rendered as reviewer input.

Markdown is a deterministic bounded rendering of that validated manifest. It
must cite the audit hash and source/window IDs, display
`trusted_pass = false`, distinguish proposed claims, verifier-unsupported
status, and unknowns, render that no verifier observation was run, and use the qualified
coverage wording in section 3. It must not be
accepted as input by any Core, Store, runtime, verifier, decision, or report
authority API. There is no Markdown-to-state parser.

#### 8.4.1 Human report v2

Run v3 requires the new closed public schema
`reviewgraphen.generic_review_human_report.v2`; changing or widening v1 is
forbidden. Human report v2 is constructed only from a fully schema- and
semantics-valid `reviewgraphen.generic_review_run.v3` value produced under
section 5.4.3's basis-bound validator, never from bytes-only wire validation.
It contains the exact
run-v3 schema/run/snapshot/universe IDs and SHA-256 of the complete canonical
run-v3 audit JSON. In addition to the unchanged v1 projection domains, it must
project the v3 context-policy ID/hash, all four denominator count/digest
commitments, latent-cardinality records, both subject outcomes, admitted
anchor-bearing windows, support-loss summaries, and meaningful-information-
loss declarations. A retained row count must never substitute for a committed
denominator count.

The v2 manifest and its deterministic Markdown projection retain exactly the
section 9 authority ceiling: `classification = "non_authority"`,
`trusted_pass = false`, and `result_status = "incomplete"`. Proposed,
reviewed, evidence-supported, verified, and human-accepted states remain
distinct; projection cannot promote any of them. Markdown remains a one-way,
loss-declaring display artifact with no parser or state-admission route.

The report pairing is exact and closed:

| Run schema | Human-report schema | Result |
| --- | --- | --- |
| `reviewgraphen.generic_review_run.v2` | `reviewgraphen.generic_review_human_report.v1` | allowed |
| `reviewgraphen.generic_review_run.v3` | `reviewgraphen.generic_review_human_report.v2` | allowed |
| run v2 | human-report v2 | wrong-version, reject |
| run v3 | human-report v1 | wrong-version, reject |

There is no permissive report union, negotiation, fallback, automatic
upcast/downcast, or schema-less crate-local manifest. A renderer may share
private implementation only after exact dispatch; it may not erase the two
public types or their independent validators.

This report-version decision does **not** require another m20 re-freeze. The
completed atomic freeze remains identified by the exact freeze-manifest byte
hash
`sha256:95b75a62353b56111fe8913700c72bf9881f93efdc99629e72adb3e2c6a9fa73`.
M20's frozen inputs, evaluator bundle, reference vectors, mutations, packet and
source-inventory contracts, task/hidden bindings, Stage 0 gates, scorer,
primary endpoint, and operational rectangle do not consume or emit the generic
human-report manifest. Human report v2 is a deterministic downstream
projection of an already sealed generic run v3 and changes none of those
frozen bytes or decisions. If m20 later makes a generic human-report schema or
its bytes part of an evaluator input, scored output, frozen artifact contract,
or hash preimage, that future change requires a new versioned freeze; merely
rendering the non-authority projection does not.

#### 8.5 Request/run v3 boundary

`reviewgraphen.generic_review_request.v2` remains permanently bound to
`context.subject_windows@2`; because that closed request has no policy selector,
it cannot select, negotiate, or imply v3. A v2 request always produces only a
`reviewgraphen.generic_review_run.v2`, and run v2 retains only v2 envelopes and
the exact section 8.3 semantics.

Every new D execution uses the closed
`reviewgraphen.generic_review_request.v3`. It retains the immutable v2
repository/base/target, ingest, plan, observer, and verifier selections and adds
exactly the required field
`context_policy_id = "context.subject_windows@3"`. The caller supplies neither
policy parameters nor a policy hash; the registry fixes both to section 5.4.
Any other ID, omitted field, inline parameter/hash, or v2 schema tag is invalid.
The fixed D rule/property/profile IDs and profile hash are unchanged.

Request v3 produces only `reviewgraphen.generic_review_run.v3`. Run v3 retains
the unchanged legacy-ingestion and separately validated ingestion-report-v2
boundary, split-capability obligation contract, two-layer coverage, observer,
verifier-unsupported, provider-free binding, and authority ceiling from run
v2. It replaces only the context domain with v3 envelopes, the four exact
denominator commitments, latent-cardinality records, two subject outcomes,
anchor-bearing windows, and support-loss summaries from section 5.4. Its wire
validator checks only the wire-valid closure defined in section 5.4.3. Its
basis-bound semantic validator rebuilds every set, digest, count, partition,
subject binding, packet inventory, and projection hash against admitted state.
The corresponding closed
`reviewgraphen.generic_review_human_report.v2` follows section 8.4.1 exactly.

Dispatch is by exact top-level schema before decoding. Request/run v2 rejects
every v3 field, policy, envelope, denominator commitment, summary, and replay
record. Request/run v3 rejects v2's absent selector and every v2 policy or
envelope. A request and its run, replay records, human report, provider-free
binding, context IDs, packet/inventory hashes, and artifact manifest must use
exactly one version tuple: request-v2/run-v2/human-report-v1/context-v2 or
request-v3/run-v3/human-report-v2/context-v3. Cross-decode, automatic
upcast/downcast, mixed-family replay, or relabeling v2 canonical bytes as v3 is
forbidden. Re-executing the same Git
pair under v3 creates new context/run/report identities and no migrated
authority; historical v2 bytes remain readable and byte-identical.

#### 8.6 v1/v2/v3 read and replay compatibility

The existing request/run schemas, v1 decoders, `context.baseline@1`, process
records, canonical fixtures, and replay behavior must remain present and byte-
stable. The CLI/runtime must dispatch by exact top-level schema ID into separate
v1, v2, and v3 types; it must not deserialize into a common permissive struct.
Legacy ingest/read/replay accepts only the unchanged ProgramSpace and
`reviewgraphen.extraction_report.v1`; it has no sidecar field, union arm, map,
or decoder for either v2 record type. Encountering
`reviewgraphen.ingestion_report.v2` at a v1 entry point is a wrong-major error,
not an ignored extension.

There is no automatic semantic upcast from request/run v1 to v2 or v3. A user may
construct a new compatibility-v2 or active-v3 request over the same immutable
Git revisions, producing a new run/universe under that exact
rule/profile/context contract. That is
resynthesis, not migration of prior authority or coverage. Stored v1 output
remains historical v1 and may not acquire the D rule, split capabilities, v2
or v3 context, ingestion sidecar, verifier-unsupported status, or human report by
patching fields.

Changing any existing rule trigger, reclassifying a legacy capability,
changing v1 canonical bytes, or reusing a stored rule/property meaning requires
a new major contract or rule version and an explicit migration ADR. The
implementation of this decision must not do so.

### 9. Authority ceiling

Every generic v2 or v3 run and human report has:

```text
classification = "non_authority"
trusted_pass = false
result_status = "incomplete"
```

The incomplete reasons must include at least model/observer non-authority,
human decision not recorded, and candidate-space enumeration incomplete while
`direct_calls` is partial. Relevant abstention, malformed output, provider
failure, subject loss, deferred work, verifier unsupported/inconclusive, and
stale-input reasons must be added without replacing those base reasons.

Accepted exact call resolution, schema validity, deterministic observer output,
a hypothetical future Cargo exit zero, a judge-positive disposition,
Stage 0/1/2 evaluation success,
or completion of every resolved-target obligation must not change
`trusted_pass`, claim disposition, review status, verification status, Finding
status, or human acceptance.

The generic v2 and v3 paths may create only proposal/abstention/process/replay/
deterministic/verifier-unsupported records defined by their schemas. They must not
mint or append authority-bearing `Evidence`, `Verification`, `Decision`,
`Finding`, accepted claim, gate, Store admission, or trusted sign-off. The
Markdown report is a lossy projection of canonical audit JSON and cannot mint,
import, or recover any of those records.

### 10. Evaluation commitment

Stage 0 is model-free and deterministic over the exact 300-cluster set `C` in
section 4.3. Every treatment context is built through request/run v3 and
`context.subject_windows@3`; v2 context rows are ineligible. It has exactly
seven gate IDs, and its advance decision is the logical conjunction of all
seven. There is no substitute, aggregate score, or later gate that can rescue
one failure:

1. `prevalence`: at least 45 of 300 `A_c` sets are nonempty and `|A| >= 60`.
2. `subject_retention`: let `S` be the exact subset of `A` whose reviewer
   packet admits at least one window for each of its exact caller and callee.
   It passes iff `20 * |S| >= 19 * |A|`. Every ID in `A - S` must have a typed
   subject loss or unknown naming the affected source and recovery reference;
   a profile exclusion is not in `A` and cannot satisfy this remainder.
3. `bounded_context`: over the same applicable packet ID set `A`, the median
   admitted-source bytes must be at most half the median whole changed
   production-file bytes, and the one-based nearest-rank p90 admitted-source
   bytes must be at most 65,536. An even-count median is the exact midpoint
   average of the two central values; empty `A` fails through `prevalence` and
   must not fabricate a median.
4. `determinism`: two clean rebuilds must match obligation, universe, source,
   applicability, all four v3 denominator commitments, subject outcomes,
   windows, inventories, support-loss summaries, latent-cardinality records,
   and canonical bytes for every one of the 300 cluster IDs.
5. `enumeration_honesty`: every repository snapshot must retain
   `direct_calls = partial`, source-backed limitations, the exact section 3.4
   summary/count/digest closure (including `enumeration_obstruction_ids`), and
   the D rule-level gap, with `call_graph_complete = false` and no
   complete/global call-coverage claim. Summary-ID cardinality is never used as
   occurrence cardinality.
6. `fan_out`: the exact p95 condition and ID sets in section 4.3 pass.
7. `deferred_fraction`: the exact `20 * |D| <= |A|` condition and ID sets in
   section 4.3 pass.

The seven IDs, thresholds, exact sets, nearest-rank rules, zero-obligation
treatment, exact/+1 fixtures, and failure semantics in this section and
section 4.3 are one authority. A mismatch is schema-invalid; an operator may
not choose between the sections. Any failed gate stops all model execution and
is slice failure.

Stage 0 orchestration is part of the frozen evaluator contract, not an
unfrozen protocol script. Repository/commit enumeration, proof that all 300
clusters were processed twice, seven-gate reduction, model-eligibility, and the
hash-ranked Stage 1/2A membership can change the evaluated sample and therefore
cannot be delegated to `scripts/` or operator logic. The evaluator must expose
exactly one model-free production entry point
`stage0 NEW_OUTPUT_ROOT`. It resolves the frozen corpus, runs the frozen
ingest/synthesis/universe/planning/context pipeline twice from independent
empty caches, evaluates all seven gates, and seals the corpus, per-cluster,
determinism, gate, and selection records beneath the initially nonexistent
root. It invokes neither reviewer nor judge transport.

For canonical repository ID `r` and exact Git object IDs `b` and `h`, the
cluster ID is
`D("commit-cluster", {"cluster_contract":"m20.commit_cluster@1",
"experiment_id":"m20-changed-public-callee-utility-v1",
"repository_id":r,"base_commit_oid":b,"head_commit_oid":h})`, using the
evaluator's frozen restricted canonical JSON and StableId derivation. Local
paths, ordinal, timestamps, and observed results are forbidden from the
preimage. The output root is caller-selected only as a fresh filesystem
destination; its path is not an identity input. Its closed layout is
`corpus-manifest.v1.json`, `clusters/<cluster digest>/build-1/`,
`clusters/<cluster digest>/build-2/`, `stage0-result.v1.json`,
`stage0-selection.v1.json`, and `artifact-manifest.v1.json`; writes outside the
root or unlisted files are invariant failure.

Stage 0 records may affect Stage 1 only through the sealed exact eligible-ID
set and its frozen hash order, conditional on all seven gates passing. They
contain no arm result, primary cell, judge result, or completion value. The
paired-run launch must authenticate the selection-manifest hash and membership;
the primary scorer accepts no Stage 0 metric or gate field. Ineligible clusters
are selection exclusions, not primary zeros, and no Stage 0 count contributes
to `n`, `b`, `c`, or `n00`.

The current evaluator freeze has no such production entry point and is
incomplete despite containing the per-cluster `stage0_contract.py` closure
helpers. Because Stage 0 and model results remain unobserved, one further
atomic pre-observation re-seal is permitted and required. It must add the
entry point, closed launch/result/selection/layout schemas, corpus and
missing/duplicate/reordered-cluster attacks, and authenticated Stage 1 handoff.
The primary endpoint, its denominator and forced-zero rules, seven gates,
sample sizes/order, rectangles, profile/context DTOs, and reviewer-visible
packet remain byte-for-byte and semantically unchanged.

Packet construction and primary scoring must be executable evaluator code, not
an interpretation of prose in this ADR. The m20 preregistration and freeze
manifest must refer to that code and its executable checks through these exact
fields:

```text
evaluator_code.packet_builder_path
evaluator_code.packet_builder_sha256
evaluator_code.scorer_path
evaluator_code.scorer_sha256
evaluator_code.reference_vectors_path
evaluator_code.reference_vectors_sha256
evaluator_code.mutation_tests_path
evaluator_code.mutation_tests_sha256
```

Each path is normalized, repository-relative, symlink-free, and confined to
the m20 benchmark directory. A code/test hash is SHA-256 of the exact checked-in
file bytes; a reference-vector hash is SHA-256 of its canonical JSON bytes.
The evaluator owner must be distinct from the slice implementation owner. The
owner must implement packet builder and scorer, pass the reference vectors and
mutation suite, and freeze all eight literal path/hash values before slice
implementation starts. The runner must recompute and match every hash before
corpus resolution, packet construction, scoring, or model execution. A missing
value or mismatch stops the study. Any subsequent evaluator code, vector, or
mutation-test byte change changes a hash and requires a new versioned study;
the old study cannot absorb the change. Operational evaluator algorithms are
therefore referenced, not duplicated here; the seven gate IDs/thresholds and
time authorizations above remain the stable ADR boundary.

Stage 1 uses 10 paired commit clusters. Its cumulative model-execution ceiling
is exactly `cumulative_model_ceiling = 18,900 seconds` (5.25 hours), while its
inclusive serial planning/validation/execution reserve is exactly
`wall_clock_envelope = 21,600 seconds` (6 hours). Its frozen primary endpoint
is exactly `usable_grounded_disposition_completed`; the large-effect gate is
ReviewGraphen-only completion `b >= 8` and baseline-only completion `c <= 1`.

Stage 2A runs only after Stage 1 passes and extends to 40 total pairs. The
cumulative ceilings including Stage 1 are exactly
`cumulative_model_ceiling = 75,600 seconds` (21 hours) and
`wall_clock_envelope = 86,400 seconds` (24 hours). Its gate is `b >= 18`,
`c <= 7`. Reserve time is not model authorization: no stage may convert unused
wall-clock reserve into model execution, and all reviewer/judge calls together
must remain within the applicable cumulative model ceiling.

The benchmark directory is
`benchmarks/m20-changed-public-callee-utility-v1/`. Claims/findings within one
commit are one correlated cluster and must not increase `n`. Judge-positive
yield and clean-control false positives are secondary non-authority metrics and
cannot rescue a failed primary gate.

There is no genuine holdout access boundary on the current machine. The study
must be labeled **open development evaluation**. It must not claim
organizational blindness, holdout validity, population precision/recall, or
generalization. Changing the primary endpoint, timeout classification, unit,
thresholds, or missing-data rule after observing an arm result creates a new
versioned experiment; it cannot replace a failed frozen result.

The excluded, nonregistered corrected pilot observed only one of six completed
primary outcomes at the frozen 12,000-token output request and five of twelve
responses reaching 32,000 tokens under the exploratory condition. This does
not authorize a new m20 output cap: no preregistered selection or stopping rule
chooses 32,000, and that value demonstrably does not eliminate truncation.
Accordingly m20 retains the common 12,000-token request and 900-second hard
timeout. A budget terminal is a primary zero under the frozen bounded-utility
estimand, not missing data, and arm-specific completion/truncation must be
published. A higher-budget question is a new versioned experiment and cannot
replace m20; no seal or time rectangle changes solely for this ruling.

M21 must likewise keep task/change evidence common when it compares context
construction treatments, but this m20 ruling neither freezes nor otherwise
changes that separate benchmark.

### 11. Clone-reproducible provider-free quickstart

Implementation of this ADR is incomplete until a normal clone contains this
exact directory and file contract:

```text
examples/changed-public-callee-quickstart/
  README.md
  request.v3.json
  expected-hashes.json
  verify.py
```

Any previously sealed v2 quickstart request/run remains a readable v2
compatibility artifact with unchanged bytes, but it does not satisfy this
active quickstart contract and must not be relabeled or rewritten as v3.

- `README.md` contains the exact command sequence and exit codes below, states
  that it must run from the repository root, and explains the non-authority and
  verifier-deferred result.
- `request.v3.json` is an ordinary closed
  `reviewgraphen.generic_review_request.v3`, not a fixture request. It uses
  repository/workspace root `.`, two full immutable Git object IDs reachable
  from the clone: base
  `a8b6b24d5ed704f53f721b25db42d5d631f946c7` and target
  `8569a2261e8a62145228872a2fde9f4c48093d00`. It uses
  `rust.production.v1`, the D `@1` rule/property contract,
  `context.subject_windows@3`, `deterministic.abstain@1`, and verification
  disabled, with `ingest.max_files` exactly 20,000. It contains no provider
  executable, credential, replay record, fixture key, machine-specific absolute
  path, branch name, or mutable `HEAD`.
- `expected-hashes.json` is a closed canonical JSON object that pins the request
  SHA-256, the exact first/second exit codes, and a sorted map from every
  expected artifact path to its literal lowercase `sha256:<64 hex>` value and
  byte length. In particular, it must pin the canonical hash of
  `audit.run.v3.json`; placeholders, values computed only at test time, and
  acceptance of an updated observed hash are forbidden.
- `verify.py` uses only the Python standard library, performs no writes or
  network access, validates canonical JSON and every byte length/hash against
  `expected-hashes.json`, rejects missing/extra/symlink/non-regular artifacts,
  and exits 0 only on exact equality.

For this v3 request, each literal `.` root is resolved once against the
canonical invocation cwd, which must be the top-level clone containing the
request. Resolution rejects symlinks and escape and is used only for local
admission; physical absolute paths, inode numbers, and clone directory names
must not enter canonical IDs or output. Repository identity is the request's
fixed logical identity plus the exact Git object/tree hashes. This is what lets
two clones at different filesystem paths produce identical bytes; no
environment or template substitution is permitted.

The first successful run creates exactly this fresh output layout:

```text
.reviewgraphen-quickstart-output/
  artifact-manifest.v1.json
  audit.run.v3.json
  human-report.manifest.v2.json
  human-report.md
  records/
    <execution-id-sha256>.provider-free-reviewer-packet.v1.json
    <execution-id-sha256>.deterministic-observer-output.v1.json
```

Accordingly the active quickstart changes the manifest filename from
`human-report.manifest.v1.json` to `human-report.manifest.v2.json`.
`human-report.md` keeps its unversioned filename because it is a one-way display
artifact, not a schema-dispatched input; its exact bytes remain bound by the v2
manifest and `artifact-manifest.v1.json`. The artifact-manifest schema itself
remains v1 because its closed role/path/hash container contract is unchanged.

There is one sorted pair of record files for every execution ID declared by
the canonical audit; `<execution-id-sha256>` is the lowercase 64-hex SHA-256 of
the execution ID's exact UTF-8 bytes. The packet and observer output use the
provider-free contracts in section 8.2.1; their task ID and binding are
runtime-derived, never request fields. `artifact-manifest.v1.json` lists every
other relative artifact path, role, byte length, and SHA-256 and binds the
request/run/snapshot/universe IDs; `expected-hashes.json` also pins the
manifest itself.

The run, human-report manifest, packet, observer output, and artifact manifest
must be canonical JSON. The Markdown bytes are deterministic and its hash must
match both manifests. CLI stdout is byte-identical to `audit.run.v3.json`.
There are no unlisted files under the canonical artifact root. Because the task ID is derived from the
canonical execution ID, and inventory IDs use only normalized logical paths,
ranges, and exact payload bytes, neither provider state nor a physical clone
path enters these bytes; two clean clones therefore construct identical
packet, output, binding, run, and manifest bytes.

#### 11.1 Optional operational diagnostics

The canonical artifact namespace is exactly the recursive tree rooted at the
`--artifacts` argument. An operational diagnostic is **not** a member excluded
from that namespace; it is a separate caller-selected file outside the
namespace. The only admitted opt-in form is:

```text
reviewgraphen review --request <request.json> --artifacts <fresh-dir> --diagnostics <fresh-file>
```

`--diagnostics` may occur at most once and is absent by default. Its path is
resolved once against the canonical invocation cwd. The resolved file must not
equal, descend from, or be an ancestor of the artifact root, must not already
exist, and must not be a symlink; every existing parent component must be a
non-symlink directory, and creation must use no-follow/create-new semantics. A
path violating this closure is exit-code-2 CLI usage failure before either
output path is created, not permission to extend the artifact root.
The single file is the closed
`reviewgraphen.generic_review_diagnostics.v1` observation from required
verification item 20.

Because the diagnostic is outside the artifact namespace,
`artifact-manifest.v1.json` neither lists nor excludes it. The manifest still
lists every other file under its root, and an in-root diagnostic remains an
unlisted-file failure. The section 11 commands omit the opt-in, so
`expected-hashes.json` contains no diagnostic path/hash and `verify.py` scans
only the canonical artifact root. The two-clone reproducibility gate compares
only those canonical roots. A separate opt-in test must schema-validate each
diagnostic and compare the two canonical roots byte-for-byte, but must not
compare elapsed values or accept a newly observed diagnostic hash. Enabling
the flag must not change stdout, exit classification, any canonical artifact
byte/hash, or the set of files below the artifact root.

There is no fixed product post-ingest watchdog in this decision. The prior
ten-second public-CLI proposal is withdrawn: on the 4,816-file positive pair,
the product completed ingest at about 155 seconds and legitimately required
about 40 more seconds for synthesize, context, observer, report, and artifact
write, while the proposed watchdog terminated the same valid v3 request with
exit 21. A fixture deadline cannot be promoted to a workload deadline.
`docs/13_cli_contract.md` must not assign exit 21 to this proposal, and the
product CLI must not emit that exit or watchdog stderr under ADR 0038.

Fixed per-stage seconds are likewise unsupported without a measured envelope
for each admitted workload, and an input-size multiplier cannot account for
reachable graph shape, source bytes, obligation count, hardware, or I/O. The
known six-hour regression is instead closed by item 17's pre-registration
candidate preflight and exact operation-count/zero-side-effect assertions, and
by item 18's no-full-scan complexity contract. These deterministic work bounds
are the product guard for this failure class. Process supervisors may impose an
external operator deadline, but it is not ReviewGraphen result semantics. A
future native cancellation/deadline option requires a separately versioned,
opt-in operational contract backed by measurements; it may not become a
default by reusing the test watchdog.

This diagnostic-placement and watchdog-withdrawal contract changes no context/
profile DTO, generic run/report canonical byte, or frozen m20 input/evaluator/
output/hash preimage.
It therefore requires no m20 re-freeze, and freeze-manifest hash
`sha256:95b75a62353b56111fe8913700c72bf9881f93efdc99629e72adb3e2c6a9fa73`
remains unchanged.

From a clean clone with no `.reviewgraphen-quickstart-output` entry, the exact
provider-free commands and expected exit codes are:

```text
cargo build --locked -p reviewgraphen-cli
# exit 0
target/debug/reviewgraphen review --request examples/changed-public-callee-quickstart/request.v3.json --artifacts .reviewgraphen-quickstart-output
# exit 0
python3 examples/changed-public-callee-quickstart/verify.py --expected examples/changed-public-callee-quickstart/expected-hashes.json --output-root .reviewgraphen-quickstart-output
# exit 0
target/debug/reviewgraphen review --request examples/changed-public-callee-quickstart/request.v3.json --artifacts .reviewgraphen-quickstart-output
# exit 20
python3 examples/changed-public-callee-quickstart/verify.py --expected examples/changed-public-callee-quickstart/expected-hashes.json --output-root .reviewgraphen-quickstart-output
# exit 0
```

The output root must not exist at first invocation; an empty existing
directory, file, or symlink is not fresh. The runtime must create it without
following symlinks. The second invocation must refuse before ingest, observer,
or artifact writes with stderr exactly
`generic review artifact root already exists`; it must leave every artifact
byte and modification timestamp unchanged. This refusal is not the verifier
unsupported record and does not alter the successful audit.

CI must exercise this sequence in a newly created throwaway local clone or
detached worktree at the exact checked-out revision, with the output root
initially absent. The test must verify both exit codes, stdout equality, all
pinned hashes, immutable source revisions, absence of provider/network/process
observer use, no tracked-file modification, second-run byte/modification-time
immutability, and success after a second independent clean clone. A quickstart
whose hashes depend on a local cache, absolute path, clock, hostname, user,
locale, or existing artifact is nonconforming.

## Alternatives considered

### A. Candidate A: changed explicit unsafe boundary

Rejected as the first wedge. The read-only development probe found 0/143
production-Rust commits even under a permissive upper bound. Its syntax fact is
clean, but a predictably empty census cannot demonstrate practical utility.

### B. Candidate C: public-API compatibility invariant

Deferred. It can provide strong mechanical value, but it requires a pinned
rustdoc/toolchain/cfg/feature/re-export comparator and a larger schema/extractor
surface. It remains the fallback if D fails its model-free prevalence or
fan-out gate.

### C. Declare `direct_calls` complete

Rejected. The current extractor does not resolve method, trait, dynamic,
cross-crate, imported, ambiguous, or macro-generated calls. Declaring complete
would launder an incomplete candidate space and permit false 100% coverage.

### D. Keep all D targets unknown under the legacy capability field

Rejected for the v2 slice. It is honest but makes an accepted, individually
supported relation unreviewable solely because other calls were not enumerated.
The split preserves that unknown space without discarding target utility.

### E. Reuse `relation.changed_call_contract@1`

Rejected. That ID denotes payment-idempotency fixture semantics. Reusing it
would silently change stored meaning and defeat rule-version staleness and
replay.

### F. Include changed callers or every caller of any changed callee

The latter remains bounded by the exact-public-callee trigger; the former is
deferred. A changed caller does not itself establish a changed callee contract
and substantially increases noise. Adding it requires a new versioned rule.

### G. Let the model choose tests or commands

Rejected. Prose and `requested_evidence` are untrusted claim material. They
cannot become executable authority, path, argv, cwd, environment, or mount
configuration.

### H. Put an evaluator-supplied opaque task binding in request v2

Rejected for the provider-free path. A quickstart has no owning evaluator, so
the caller would become the unaudited binding authority or would require an
otherwise unnecessary provider. It would also make the ordinary request carry
m20-only comparison state. The runtime-owned provider-free binding preserves
the existing request contract and clone reproducibility.

### I. Reuse the m20 packet or improvise its task preimage

Rejected. Deriving an m20 task ID from execution, obligation, context, source,
repository, or Git identity violates section 8.2. A CLI wrapper cannot author
the frozen packet either. The separate provider-free schema permits its
explicit execution-bound derivation without weakening or overloading the m20
contract.

## Explicit non-goals and deferred work

This slice does not implement or claim:

- changed-caller-side obligations;
- method, trait, dynamic-dispatch, imported, UFCS, cross-crate, or macro-expanded
  call resolution;
- a complete call graph or one missing obligation per unresolved call;
- callsite-change, written-signature-change, error-type-change, ABI-change, or
  return-type-change facts;
- semantic proof that a callee contract changed or that a caller is correct;
- external public reachability through module visibility, re-exports, cfg, or
  feature selection beyond exact `Visibility::Public(_)` syntax;
- Candidate A unsafe analysis or Candidate C public-API compatibility;
- arbitrary reviewer browsing, shell, test filters, features, packages, or
  commands;
- execution of `workspace.cargo_test@1` or any repository-controlled code;
- networked dependency acquisition or a generic verifier command facility;
- self-contained proof of v3 denominator completeness from context/run bytes
  without the matching admitted validation basis;
- automatic Evidence/Verification admission, human acceptance, Store authority,
  gluing, global safety, merge gate, or automatic repair;
- migration of v1 coverage/claims into v2 semantics;
- a blinded holdout, population precision/recall, or general Rust utility
  estimate; or
- promotion from confidence, test existence/success, judge agreement, or
  evaluation success.

These items are deferred because their contracts and evidence are absent, not
because they are unnecessary.

## Required verification

Implementation is incomplete unless all of the following pass:

1. Rule tests cover every trigger clause independently; changed caller only,
   own-symbol `changed`, `pub(crate)`, method/UFCS/cross-crate, ambiguous target,
   missing resolution, non-function callee, zero/multiple targets, profile
   exclusion, duplicate relation, and two edges with identical endpoints.
2. Same input under reordered ProgramSpace vectors produces identical D
   obligation, exclusion, universe, gap, source, and contract IDs/bytes.
3. `ast`, `containment`, or `changed_structure` partial/missing/unknown makes the
   substantive target unknown with the exact limitation trace; only
   `direct_calls` partial leaves it applicable and creates the D rule-level gap.
4. Both capability fields pass the omission/exchange/overlap/duplicate/null/
   unknown-field mutation matrix. Existing five rules and every v1 canonical
   fixture remain byte-identical.
5. Resolved-target and candidate-space sets close exactly. Every observed
   unresolved direct/method/macro occurrence internally has the closed call
   kind, reason, span, sources, and exact `direct_calls` relation. The run
   rebuilds the section 3.4 per-file summaries, bucket/report counts and
   occurrence-ID digests exactly, retains the mandatory global limitation and
   unknown macro-expansion cardinality, and rejects count/digest mismatch,
   hidden/truncated occurrences, cross-capability mixing, fabricated
   obligations, and every global-call-coverage claim.
6. The profile canonical-byte/hash golden test passes. Invalid path encodings
   and every matcher/preference overlap have fixtures. Exclusion ID mutations
   of profile/rule/candidate/reason/matcher/weight/source all change the ID or
   fail. Planning preserves every overflow ID and weight. The 300-cluster p95
   and deferred-set exact/+1 and zero-obligation fixtures use section 4.3.
7. Context tests independently verify the exact v2 and v3 policy bytes/hashes,
   byte-identical v2 replay, every BFS token/direction/same-token depth reset and
   tie-break, and v3's four rebuilt count/digest commitments, known-zero versus
   unknown latent cardinality, caller/callee reservation, admitted-anchor/loss
   partition, every reason summary, and subject/reached-only materialization.
   Wire-only and basis-bound APIs must have distinct result types. Three
   coherent mutations—accepted count/digest, reached count/digest, and one
   support-loss count/digest—are each resealed with matching projection/context
   IDs and all visible inequalities preserved: wire validation must accept
   their intentionally maintained internal closure, but semantic
   validation against the original basis must reject all three. Materialized-
   source mutations remain wire-reconstructible and must fail wire validation.
   The basis-bound validator must additionally use a code-path-distinct,
   exhaustive relation/containment oracle to derive reached-file and support-
   anchor ID sets without any production discovery helper. A mutant that omits
   `rust/fslc/src/literate_access.rs` from the production anchor scan must leave
   clause, subject, and source-access probe checks otherwise passing but be
   rejected by the oracle/builder set comparison. Equivalent omission mutants
   cover reached files and another support anchor. The four real-pair fixtures
   must compare literal oracle sets/digests derived from checked-in Git/source
   witnesses, never values captured from the current builder.
   A valid context must pass against independently rebuilt live and sealed-
   replay bases; wrong snapshot, source-registration, request, extractor,
   obligation, endpoint, or policy bindings must fail before any semantic
   validated type is returned.
   Path/test selection, same/cross-file endpoints, overlap/adjacency/disjoint
   merge, every exact/+1 cap, giant lines, missing source/location, typed loss,
   and source/hash/range/role/order mutations must also pass.
   Runtime tests must prove one run-scoped immutable snapshot basis is shared,
   each obligation view is dropped immediately after validation, and the
   returned validated run retains no ProgramSpace/source-byte basis or per-
   obligation deep clone. Oracle operation counts and wall time for the
   4,816-file positive pair and frozen Stage 0 corpus are recorded before
   re-freeze; those diagnostics remain outside canonical artifacts.
8. Verifier-seam tests prove that absent or selected
   `workspace.cargo_test@1` never resolves or spawns a process and that selection
   yields only the fixed unsupported record with empty `verifier_observed`.
   Every executable/argv/path/env/mount/cache/identity/limit field is rejected.
   Fixtures containing malicious `build.rs`, proc macros, tests, doctests,
   wrapper/runner/linker Cargo config, absolute executables, fork bombs,
   network attempts, mount-external reads/writes, escape attempts, and file/
   inode/output/memory exhaustion must produce the same no-I/O result.
9. Deterministic-observer tests use unrelated valid repositories/obligations and
   obtain the same closed abstention class with caller-fixed IDs; any fixture
   table, source-content branch, I/O, claim, or forged process record fails.
10. Canonical A/B reviewer-packet fixtures share the exact task ID, instruction,
    and disposition schema and expose no D rule/property/relation/endpoint/
    subject metadata or semantic cue. Hidden-binding and forbidden-field/value
    mutations fail, while identical literal text inside admitted source bytes
    remains unchanged and valid.
11. The section 11 quickstart files are checked in with literal expected hashes.
    Its exact commands pass in two independent clean throwaway clones; first run
    exits 0 with exact artifacts/stdout/hashes, second run exits 20 without byte
    or modification-time mutation, and both read-only `verify.py` invocations
    exit 0. Its packet/output use only the provider-free schemas and task-ID
    kind; m20 schemas, task IDs, comparison/unit IDs, and hidden bindings are
    rejected. Provider-free packet/binding mutation and every reviewer-lens
    forbidden-field/value mutation fail semantic validation.
12. Request/run/human-report schemas reject every missing/extra/wrong-major
    field. Run-v2/report-v1 and run-v3/report-v2 are the only accepted report
    pairs; both cross-pairs fail, v1 canonical bytes remain unchanged, and v3
    denominator/latent/subject/window/support-loss projection mutations fail
    basis-bound validation. A bytes-only run-v3 decoder must return only the
    unvalidated wire type; it cannot be passed to the semantic report generator
    without a matching basis. Bytes-only human-report checking is tested and
    labeled as projection validation, never denominator/source validation.
    Replay requires exact hashes; Markdown hash/audit hash/source/loss/coverage/
    authority tampering fails.
13. No path can set `trusted_pass=true` or create accepted claims, Evidence,
    Verification, Decision, Finding, or importable state from generic v2/v3 or
    Markdown.
14. Formatter, lint, unit/integration tests, schema examples/mutations,
    deterministic rebuild, Markdown link validation, and
    `python3 scripts/validate_bundle.py` pass together.
15. A live compatibility test ingests the same ordinary repository/snapshot
    containing resolved calls plus unresolved direct, method, and macro syntax
    through `ingest`, `ingest_with_sources`, `ingest_v2`, and
    `ingest_with_sources_v2`. Both v2 wrappers' legacy canonical bytes must be
    byte-identical to the corresponding legacy API bytes and to pre-sidecar
    frozen live-oracle hashes for that exact snapshot/profile/rule/extractor
    tuple. Live synthesis of all five existing rules from each legacy member
    must produce identical
   obligation, qualification, universe, contract, and exclusion IDs/bodies/
   canonical bytes and match their pre-sidecar frozen hashes. Every v2 sidecar
   summary and limitation ID must be absent from ProgramSpace core
    limitations, v1 report obstructions, v1 universe limitations, and existing
   obligation sources. The same test must validate the singular unknown-latent
   global record and exact source-summary/count/digest closure, then reject
   schema/record-kind/ID-preimage/description/extra-field/cross-collection and
   hidden-occurrence mutations. Checked-in fixture bytes alone do not satisfy
   this live oracle.
16. A **source-access effect test**, owned by `reviewgraphen-core` with a
    `reviewgraphen-runtime` real-path integration, must observe context
    construction through an injected read-only build probe. The probe records
    ordered graph-index lookups, source-metadata lookups, candidate-
    materialization IDs, source-byte requests, submitted-source IDs, subject
    outcomes, and typed limit failures; it never receives source bytes,
    selects behavior, or enters a canonical record. The
    test must not infer reads from the returned context. For immutable pair
    `a8b6b24d5ed704f53f721b25db42d5d631f946c7` to
    `8569a2261e8a62145228872a2fde9f4c48093d00`, an independent exhaustive
    traversal of the accepted relation/containment facts and a checked-in Git-
    tree oracle produced by a small implementation-independent enumerator must
    establish 4,816 accepted file IDs and one reached file; hand-reviewed
    declaration/call witnesses must place caller and callee in
    `crates/reviewgraphen-core/src/context.rs`. The general source-access set is
    exactly `subject_file_ids union reached_file_ids`; for this pair both
    subjects are in the sole reached file, so that union is the one reached
    file. The probe must then record
    exactly that one file ID for candidate materialization, source metadata,
    source-byte request, and submission, with no other source-byte access. The
    literal manifest binds repository ID, both
    commit/tree OIDs, normalized paths/ranges, and source-byte hashes; it may
    not be generated, updated, or accepted from the system-under-test's current
    context or run output.
17. A **v2 fail-fast effect test**, shared by `reviewgraphen-core` and
    `reviewgraphen-runtime`, must run the post-ingest 4,816-file fixture in an
    isolated child process. It must return exactly typed
    `DomainError::Incomplete { operation: "context candidate files",
    limit: 4096, observed: 4097 }` at the first impossible candidate. Runtime
    must perform this allocation-free candidate-count preflight immediately
    after the post-ingest handoff and before any per-source aggregate
    registration/admission. The probe must show exactly 4,097 candidate
    metadata visits and zero snapshot registration admissions, source-byte
    requests/submissions, observer calls,
    report calls, Git subprocesses, and artifact writes for the entire
    post-ingest invocation. Constants and the deliberately sized fixture are the
    oracle, not a captured error. Deterministic operation counts are the
    primary complexity assertion. A coarse ten-second watchdog, applied only
    by the test-harness parent to this deliberately prebuilt child, is
    additionally mandatory to prevent a broken test from occupying CI for
    hours. Expiry kills and reaps that child and fails the test as `runaway`;
    it is not a product CLI invocation, exit code, stderr contract, latency
    benchmark, or substitute for the operation-count assertions. The child has
    no legitimate post-preflight work, unlike a normal product run.
18. A **context complexity contract test**, owned by `reviewgraphen-core`, must
    build the accepted-file/source-registration and relation-adjacency indexes
    once outside the measured context phase, then run the same subject/reached
    graph while adding 1x, 8x, and 64x unrelated accepted files and unrelated
    relations. The accepted-file count/digest must change correctly, but every
    context-phase probe trace must have zero full-artifact/full-relation scans,
    exactly one prebuilt-denominator-commitment lookup, and identical counts
    and IDs for adjacency queries, subject/reached metadata lookups, candidate
    materializations, and source-byte requests.
    The expected trace is fixed by a small independently authored reachable-
    graph manifest. Thus per-context work is
    `O(subjects + reached vertices/edges + materialized bytes)`; the necessary
    `O(repository metadata)` index build belongs to one-time ingest/admission,
    not each obligation. CI must gate on these exact access counts, not elapsed-
    time ratios. Wall time may be published diagnostically, but cannot make the
    test pass or fail, avoiding scheduler-dependent flakiness.
19. A **real-corpus subject guarantee test**, owned by
    `reviewgraphen-runtime` and exercising `reviewgraphen-ingest` plus Core,
    must cover at least five immutable positive parent/commit pairs: the
    ReviewGraphen pair in item 16 and at least two each from `fsl` and
    `casegraphen`. The repository objects or exact content-addressed snapshot
    fixtures must be checked in so the test uses no network or developer-local
    checkout. A hand-reviewed oracle manifest, authored from Git source
    declaration/call witnesses rather than a ReviewGraphen run, binds each
    repository/OID/tree/path/range/content hash and expected caller/callee.
    For every emitted D obligation, the effect probe must record exactly one
    terminal outcome for each ordered role: a submitted source whose bytes
    cover the witnessed range, or a high-severity typed subject-loss event with
    endpoint, source/range when known, reason, and recovery reference. Missing,
    duplicate, digest-only, or support-summary-only subject outcomes fail. The
    existing missing-source/location, giant-line, and cap mutants must traverse
    the same probe and cover the loss arm; assertions on returned
    `subject_outcomes` alone are insufficient.
20. **Stage timing is a separate operational diagnostic, never a run field.**
    `reviewgraphen-runtime` owns an injected monotonic stage observer for
    `ingest`, `synthesize`, `context`, and `observer`; the one-time source and
    adjacency index build is part of `ingest`. `reviewgraphen-cli` owns the
    enclosing coordinator and the `report` and `artifact_write` boundaries;
    `reviewgraphen-report` remains timed by its caller and does not read a
    clock. Only the explicit section 11.1
    `--diagnostics <fresh-file>` opt-in writes the
    JCS-serialized closed JSON observation
    `reviewgraphen.generic_review_diagnostics.v1`, outside the canonical
    artifact root. It contains the request/run bindings when available, a
    terminal success/failure code, and exactly six ordered stage rows with
    `completed | failed | skipped` plus monotonic elapsed microseconds. Those
    elapsed values are intentionally non-deterministic, which is why the
    diagnostic has no StableId, authority, evidence status, coverage role, or
    hash edge into a request, run, report, packet, artifact manifest, or m20
    record; absolute clock values are forbidden. It is emitted after the
    measured artifact-write stage and is available even when a prior stage
    fails. With an injected fake clock, unit tests must prove exact begin/end
    ordering, one terminal row per stage, and failure/skipped propagation.
    CLI integration tests use the item-16 success and item-17 failure fixtures,
    observe the separate-file write, and prove that enabling diagnostics leaves
    every canonical byte, StableId, hash, exit classification, and the section
    11 output layout unchanged. Expected stage names/order/durations come from
    the literal fake-clock schedule, never from a captured live run.

Items 16--20 must leave the fixed context-v2, context-v3, and profile DTO bytes
and hashes unchanged. They require no m20 re-freeze: the access probe, indexes,
test-only watchdog, and operational diagnostic are outside the frozen
evaluator, packet, source-inventory, Stage 0, scoring, primary-endpoint, and hash-preimage
surfaces, so freeze-manifest hash
`sha256:95b75a62353b56111fe8913700c72bf9881f93efdc99629e72adb3e2c6a9fa73`
remains authoritative. An implementation that changes canonical v3 context/run
bytes or any frozen m20 file is nonconforming rather than grounds to silently
re-freeze.

## Consequences

### Positive

- Ordinary Rust changes can produce a useful Relation obligation without a new
  parser or dishonest call-graph completeness claim.
- Accepted-target applicability and unknown candidate-space completeness remain
  separately inspectable.
- Caller and callee source survive bounded projection or produce explicit typed
  loss.
- The provider-free observer exercises the real generic path without a named
  fixture.
- The verifier seam remains typed and testable without executing untrusted
  repository-controlled code.
- Reports remain short projections of complete, non-authority audit JSON.

### Negative

- Method, cross-crate, imported, trait, and macro calls remain outside the
  resolved-target denominator.
- A body-only change to an exact-public callee may still fan out to many local
  callers even when its written signature is unchanged.
- The v2 and v3 contracts require separate schemas/types and cannot be a
  permissive additive patch to an earlier major.
- Multi-window projection and exact profile/obstruction closure add substantial
  validation and resource-accounting work.
- The first release collects no workspace test observation; practical utility
  is evaluated without allow-listed workspace verification, and a successor
  sandbox contract is required before that evidence seam can become available.

## Revisit triggers

Revisit this decision only through a new versioned ADR if:

- Stage 0 produces applicable D obligations in fewer than 15% of its 300 commit
  clusters;
- p95 fan-out exceeds 50 or the deferred fraction exceeds 5%;
- subject-window loss exceeds 5% of applicable D envelopes;
- a sound extractor provides complete method/cross-crate call enumeration;
- a versioned signature/error/API-diff fact supports a narrower trigger; or
- the 86,400-second wall-clock staged envelope fails its frozen practical-
  utility gate.

Failure triggers do not authorize silent narrowing, identifier reuse, lowered
thresholds, hidden exclusions, authority promotion, or rewriting this accepted
decision. They require a successor contract that preserves this history.
