# C8 mutation report

- Mutation: changed the generated proposed-claim `status` from `"proposed"`
  to `"reviewed"` in `generic_non_authority.rs`.
- Command: `cargo test -p reviewgraphen-report --test generic_non_authority`.
- Result: failed as expected (exit 101): 3 failed, 3 passed.  The generated
  manifest no longer satisfied the closed human-report schema's `status`
  constant, detected by the projection-state, tamper, and determinism tests.
- Restoration: the mutation was reverted before subsequent validation.
