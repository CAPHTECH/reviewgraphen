# M7 detection pilot v2

This is a fresh synthetic corpus for protocol v2. Reviewer sessions receive only one generated `agent_input/` directory and its sibling manifest. The checked-in private directory, pair mapping, commitments, verification tooling, other units, and other arms are excluded from reviewer inputs.

The corpus contains twelve opaque Rust units: six seeded positives and six matched empty-root controls. Pair membership and labels are private scoring data. All candidate mechanism tags are drawn from `reviewgraphen.benchmark.mechanism_ontology.v1`.

The corpus measures a narrow set of retry, scope, state, concurrency, and cross-file effect patterns. It is synthetic, has one injected root per positive, and does not establish general code-review performance or full-G3 performance. G3 remains the non-authority proxy defined by ADR 0025.
