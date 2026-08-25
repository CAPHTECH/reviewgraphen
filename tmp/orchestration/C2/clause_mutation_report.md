# C2 clause mutation report

All mutations below were applied with `apply_patch`, exercised against the
named test, and restored before the next mutation. Every listed command exited
nonzero because its assertion detected the mutation.

| Clause | Temporary mutation | Detecting test | Result |
| --- | --- | --- | --- |
| 1 `kind == calls` | Removed the relation-kind guard | `only_calls_relations_may_trigger_the_d_rule` | detected |
| 2 exact resolution | Removed the `syntactic_unique` guard | `trigger_negatives_do_not_create_a_substantive_obligation` | detected (`missing resolution`) |
| 3 accepted caller | Replaced the dangling caller error with fallback to the first artifact | `invalid_zero_target_and_duplicate_relation_inputs_cannot_reach_synthesis` | detected |
| 4 one accepted callee | Changed arity guard from `!= 1` to `> 1` | `invalid_zero_target_and_duplicate_relation_inputs_cannot_reach_synthesis` | detected (zero-target panic) |
| 5 function callee | Removed the `callee.kind == function` guard | `trigger_negatives_do_not_create_a_substantive_obligation` | detected (`method`) |
| 6 exact public | Removed the `public == true` guard | `trigger_negatives_do_not_create_a_substantive_obligation` | detected (`private`) |
| 7 containment-only changed | Allowed `caller.attributes.changed` to substitute for containment | `changed_caller_or_callee_attribute_does_not_substitute_for_changed_containment` | detected |
| 8 profile exclusion | Replaced endpoint paths passed to the profile with production literals | `production_profile_exclusion_is_visible_outside_the_denominator` | detected |

The clause 3/4 tests use the public endpoint-validator seam with detached
relations because ProgramSpace admission correctly rejects dangling or empty
relations before normal synthesis. Both errors are asserted as typed
`DomainError` variants rather than matching strings.
