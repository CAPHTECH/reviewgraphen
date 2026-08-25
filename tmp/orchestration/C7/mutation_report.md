# C7 mutation report

- Date: 2026-08-24
- Mutation: changed the v2 coverage constructor from `call_graph_complete: false` to `true`.
- Command: `cargo test -p reviewgraphen-runtime --test generic_v2 deterministic_v2_run_is_schema_valid_canonical_and_non_authority`
- Result: failed as expected at the run-v2 schema assertion; the schema requires `call_graph_complete = false`.
- Restoration: restored the fixed `false` value before subsequent verification.
