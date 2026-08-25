# C2 resolved-target planner handoff

```rust
pub fn plan_resolved_target_obligations(
    program: &ProgramSpace,
    bundle: &ObligationBundle,
    budget: PlanBudget,
) -> Result<ReviewPlan>
```

The function accepts a validated D two-layer bundle and schedules only
`bundle.universe().resolved_target_obligation_ids()`. It never mutates the
bundle; C7 must retain the bundle universe and emit
`candidate_space_gap_obligation_ids()` with its existing enumeration trace in
coverage reporting. Gap obligations are therefore not executable work, but are
not removed from coverage evidence.
