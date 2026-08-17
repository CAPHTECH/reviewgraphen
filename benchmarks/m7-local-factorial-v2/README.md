# M7 local-model × ReviewGraphen factorial v2

Interpretation correction: the snapshot-34 B1 diagnostic rerun retained a raw
stream containing 12,898 reasoning-summary deltas and no output-text event
through sequence 12,900. The earlier usage-derived `thinking_tokens=0` and
`final_content_tokens=49,624` labels did not measure event semantics. See
`THINKING_MEASUREMENT_CORRECTION.md`; the original observations remain frozen.

Status: preregistered before the first v2 semantic model call.

This additive experiment keeps `m7-local-factorial-v1` frozen and compares
`b1_free_form` with `full_reviewgraphen` on the twenty frozen positive units
from `m7-real-v1`.  It uses Qwen through Codex CLI with explicit transport
metadata: a 262,144-token context window and a 65,536-token output ceiling.

G3-proxy is excluded.  Its observed 8,131,409-character input cannot fit the
model context without changing the historical projection, which would no
longer be the frontier G3 condition.  The v1 inability to measure local G3 is
retained as an experimental result, not repaired in place.

The primary endpoint is mechanical known-target detection against the frozen
private oracle.  Protocol-invalid trials count as not detected.  Elapsed time
is reproduction metadata and never a stopping or sample-size variable.

The local row is descriptive: n=20, the FSL presence-eligible population is
28, and the conservative target is 83.  No significance, equivalence, or
factorial-interaction claim is permitted.

Each new trial also records admitted bytes, provider-reported input/output and
reasoning usage when available, event-attributed thinking/final tokens when
unambiguous, reasoning/output-text delta counts and bytes, exact final
bytes, empty-final state, failure class, and elapsed time.  These metrics test
the exploratory v1 observation that snapshot-34 B1 emitted no final message
while the smaller full input completed. One matched control
identifies a boundary to measure; it is not evidence that establishes the
hypothesis.

## Addendum (2026-08-17, appended, not a rewrite)

A later investigation
(`docs/measurement-validity-obligation-synthesis-capability-gap.md`)
directly checked this experiment's `full_review_graphen` obligations
(built agent_input packets, snapshot-06 and snapshot-40) and found all 5
obligations in each are `reviewgraphen.capability_gap`, not substantive
review obligations — the same degeneration found across every
`full_reviewgraphen` packet checked in this benchmark program. Any
comparison in this experiment's records between the `full_reviewgraphen`
and `b1_free_form` arms measured a scaffold whose obligation content never
carried a real review target; it does not establish anything about
ReviewGraphen's obligation-driven review capability. This does not change
any recorded trial outcome; it corrects how the `full_reviewgraphen` arm's
obligation content should be understood.
