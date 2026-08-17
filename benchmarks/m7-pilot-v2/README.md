# M7 detection pilot v2

This is a fresh synthetic corpus for protocol v2. Reviewer sessions receive only one generated `agent_input/` directory and its sibling manifest. The checked-in private directory, pair mapping, commitments, verification tooling, other units, and other arms are excluded from reviewer inputs.

The corpus contains twelve opaque Rust units: six seeded positives and six matched empty-root controls. Pair membership and labels are private scoring data. All candidate mechanism tags are drawn from `reviewgraphen.benchmark.mechanism_ontology.v1`.

The corpus measures a narrow set of retry, scope, state, concurrency, and cross-file effect patterns. It is synthetic, has one injected root per positive, and does not establish general code-review performance or full-G3 performance. G3 remains the non-authority proxy defined by ADR 0025.

## Addendum (2026-08-17, appended, not a rewrite)

A later investigation
(`docs/measurement-validity-obligation-synthesis-capability-gap.md`)
directly checked this experiment's `g3_proxy` obligations
(`results/replicate-1/agent-inputs/*-g3/obligations.json`, `p7m1-g3`,
`b9q6-g3`, `c4z9-g3`, `d2v8-g3`) and found all 5 obligations in every
checked case are `reviewgraphen.capability_gap`, not substantive review
obligations — the same degeneration found in every other
`full_reviewgraphen`/`g3_proxy` packet checked across this benchmark
program. The g3 arm's higher score relative to B1 in this pilot was
therefore not driven by ReviewGraphen obligation content; whatever drove
it came from something else in the packet (program-space facts, context
visibility, or chance). This does not change the recorded scores; it
corrects the inference that g3's obligation layer contributed to them.
