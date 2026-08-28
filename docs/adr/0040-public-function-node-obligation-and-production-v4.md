# ADR 0040: Public-function Node obligation and production-v4 boundary

- Status: Accepted
- Date: 2026-08-28

## Context

`rust.production.v1` currently materializes only the D relation obligation.
That compatibility path is persisted, replayed, and consumed by benchmark and
generic-runtime v1-v3 surfaces.  A public free function can instead be
enumerated from accepted `ast` and `containment` facts, independently of diff
or call resolution.  Adding that obligation to the legacy family would change
its universe, canonical bytes, and coverage meaning.

## Decision

### Rule and predicate

The production-v4 rule set adds this exact descriptor:

| field | value |
| --- | --- |
| rule ID | `node.public_function_contract@1` |
| property ID | `rust.public_function_contract_review@1` |
| target kind | `node` |
| weight | `3.0` (policy choice, not risk calibration) |
| target-support capabilities | `ast`, `containment` |
| enumeration capabilities | none |
| evidence modes | `source_inspection`, `test` |
| context policy | `context.subject_windows@4` |

The predicate ranges over accepted artifacts only: `kind == "function"`,
exact `public == true`, and at least one accepted `contains` relation from an
accepted module to that function.  Restricted visibility, methods, inferred
reachability, and non-module witnesses do not match.  Endpoint/cardinality
violations in accepted containment facts are typed synthesis obstructions.
One included function produces one obligation; sorted unique module and
containment-witness IDs are provenance, not multiplicity.  Profile matching is
on the function path after the predicate and before materialization.  An
excluded function produces one exclusion record with weight `3.0` and complete
function/module/witness source trace.

The new public mixed entry and types are versioned separately:
`ObligationBundleV3`, `UniverseDescriptorV3`, `ObligationContractV3`, and
closed `RuleCoverageV3`.  The legacy `MvpRulePack::synthesize()` production
branch continues to call only `synthesize_changed_public_callee`; it must not
call the mixed entry.  Existing `ObligationBundle`, `UniverseDescriptor`,
`ObligationContract`, persisted aggregates, and legacy coverage serialization
are unchanged.

### Coverage and exclusions

The mixed universe is the union of D substantive obligations, D candidate-gap
obligations, and Node substantive obligations.  Coverage is a closed,
rule-ID-ordered union:

- `ResolvedTargetWithCandidateGap(DTwoLayerCoverage)` for D;
- `ResolvedTargetOnly(SingleLayerCoverage)` for Node.

Node coverage has only its eligible-target denominator and the
planned/deferred/executed/structured/abstained/malformed/provider-failed/
verifier-observed partitions.  It has no candidate-space, call-graph, or gap
fields.  Partial `ast` or `containment` yields declared unknown/deferred and an
incomplete authority state; it never yields an invented zero denominator.

Profile-exclusion inputs become a closed rule-neutral enum, but the D wire
body, candidate key (`{rule}|{target_id}`), preimage fields, weight `4.0`, and
`RUST_PRODUCTION_PROFILE_HASH` remain byte-identical.  Node keys therefore use
`node.public_function_contract@1|function:…`, with no synthetic `node:` prefix.

### Context policy v4

Policy selection is by the exact closed `(rule, property, target)` registry:

| rule | property | target | policy |
| --- | --- | --- | --- |
| `relation.changed_public_callee@1` | `rust.callee_contract_review@1` | `relation` | `context.subject_windows@3` |
| `node.public_function_contract@1` | `rust.public_function_contract_review@1` | `node` | `context.subject_windows@4` |

`ContextWindowRoleV2` is unchanged.  v4 has a separate role family with only
`subject` and `support`: exactly one subject reserves one window, up to seven
support windows fill the eight-window envelope, and discovery traverses only
reverse `contains` at depth one.  It does not seed or traverse calls, tests,
or changed structure.  Selection priority is subject then support; wire order
remains canonical source/range/window order.

The canonical UTF-8, no-BOM, no-newline v4 literal is exactly 2040 bytes:

```json
{"accepted_file_denominator_bound":"request.ingest.max_files","anchors_per_file":1024,"assumptions":"empty","candidate_order":["subject_priority","containment_distance","path_rank","artifact_id"],"canonical_envelope_bytes":786432,"containment_depth":1,"contains_edges":1000000,"edge_kind_direction_order":["contains:reverse"],"excerpt_lines":400,"final_window_order":["source_artifact_id","start_line","end_line","window_id"],"included_files":64,"latent_cardinality":"known_zero_under_complete_ast_containment","loss_reason_precedence":["missing_location","missing_source","giant_line","per_window_lines","per_window_bytes","per_file_window_cap","total_window_cap","total_excerpt_bytes","overlap_unmergeable","not_reached","included_file_cap","artifact_bytes_cap","total_resolved_bytes_cap"],"materialized_source_denominator":"subject_file_ids_union_reached_file_ids","max_assumptions":64,"max_discovered_structural_ids":4096,"max_excerpt_bytes":262144,"max_materialized_source_candidates":4096,"max_resolved_artifact_bytes":1048576,"max_resolved_bytes":8388608,"max_string_bytes":16384,"max_subject_losses":1,"max_support_loss_summaries":13,"max_total_excerpt_bytes":1048576,"max_unknowns":64,"obligations_per_envelope":1,"policy_id":"context.subject_windows@4","relation_scan":1000000,"seed_fields":["source_ids","target_refs","context_ids"],"source_candidate_denominator":"all_accepted_file_ids_known_count_and_sorted_id_set_sha256","subject_endpoints":1,"subject_order":["subject"],"subject_window_slots":1,"support_anchor_denominator":"reached_range_bearing_exact_path_anchor_ids_known_count_and_sorted_id_set_sha256","support_loss_summary":"reason_known_count_and_sorted_anchor_id_set_sha256","support_window_slots":7,"unknown_reason_ids":["unresolved_review_context_member","unresolved_seed_reference"],"window_candidate_order":["priority","role","source_artifact_id","start_line","end_line","owner_id"],"window_merge":"same_source_overlap_or_adjacent_if_union_within_per_window_bounds","windows_per_envelope":8,"windows_per_file":4}
```

Its SHA-256 is
`sha256:9f15006986a73853ff3be7b9a158e8c79f69b9a8099c9d0a1991d8c7da170aed`.
The ADR literal, Rust `canonical_json`, and schema constant are three separate
test inputs and must agree.  Any disagreement stops implementation; v2/v3
cross-decode is rejected.

### Versioned wire contracts and migration

| request | run | human report | policies |
| --- | --- | --- | --- |
| v2 | v2 | v1 | D / @2 |
| v3 | v3 | v2 | D / @3 |
| v4 | v4 | v3 | D / @3 and Node / @4 |

Run-v4 contexts are a closed D/Node `oneOf`.  D retains caller/callee, two
subject outcomes, and @3 constants.  Node requires one subject, one target
equal to it, one subject outcome, and @4 constants; relation fields are
forbidden.  Its ordered two-item coverage likewise has exact D and Node arms.
Semantic validation reconstructs each rule/property/target/capability tuple
from the registry; wire validation alone does not establish basis closure.

Request v4 includes its exact schema major in provider-free task-ID preimages.
All cross-major decoding, fallback, upcast, and downcast are rejected.
There is no automatic migration: v1-v3 are read/replay-only and a v4 review of
the same snapshot is a new resynthesis with no inherited authority or coverage.
The current obligation alias advances to v3 only after its exact former v2
schema and example bytes are copied to new versioned v2 paths.

Human-report-v3 accepts only a basis-validated run-v4 capability.  It projects
per-rule coverage without flattening D's call-graph limitation into Node.  It
remains non-authoritative: `trusted_pass` is false when the D candidate-space
gap remains and no claim, evidence, verification, or human-acceptance state is
promoted.

## Consequences

New production-v4 CLI artifacts are `audit.run.v4.json` and
`human-report.manifest.v3.json`; v3 names and bytes remain unchanged.  M20's
sealed v3 binary/hash/evaluator and the m22 frozen v3 harness are not changed.
The benchmark production implementation remains a D-only consumer, protected
by a new test-only compatibility test.  Ingest, store durability, reviewer and
verifier seams, M4/M5, legacy generic runtime entries, and existing quickstarts
remain outside this change.
