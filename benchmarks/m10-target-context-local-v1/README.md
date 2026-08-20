# m10 target-context implementation experiment

Status: preregistered before the first generation request on 2026-08-20.

This experiment tests the implementation role proposed in
`docs/24_context_projection_feasibility_for_implementation.md`: the agent uses
a deterministic ReviewGraphen projection to grasp the target region, but is
not made to begin by enumerating obligations and is not asked for a
claim/evidence ledger.

The `projection` and `control` arms receive the same task and ordinary final
response contract. The projection arm alone receives the frozen
`reviewgraphen-context` command and a short instruction to use it before its
first edit. See `preregistration.json` for the frozen comparison, authority
ceiling, hashes, stopping rules, and interpretation thresholds.
