# C2 read-only D aggregate handoff

```rust
impl ReviewAggregate {
    pub fn read_only_from_d_two_layer_bundle(
        program: ProgramSpace,
        bundle: &ObligationBundle,
    ) -> Result<ReviewAggregate>;
}
```

The constructor validates the bundle's existing two-layer universe against the
program and full obligation map, then retains that exact universe and every
obligation. It is for read-only consumers, including
`prepare_subject_windows_v2`; C7 must retain the aggregate's universe coverage
trace and must not delete candidate-space-gap obligations.
