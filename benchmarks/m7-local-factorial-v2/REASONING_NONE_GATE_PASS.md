# v2 reasoning-control and B1 gate amendment

This additive amendment records the post-recovery replacement-3 result. It
does not rewrite the frozen v1/v2 pilot records.

## Verified transport condition

For Qwen `qwen3.8:27b-mlx` through the named Codex profile, with
`model_reasoning_effort=none`:

- 5 provider Responses requests completed;
- every request reported `reasoning.effort=none`;
- reasoning SSE delta events: 0;
- provider reasoning tokens: 0 for every request;
- output-text delta events: 465 in the final request and nonempty final text;
- final candidate schema validation and collection both passed;
- aggregate trial status: `valid`.

The first four requests are Codex internal plan turns; all five raw streams are
retained and bound in `transport-record.jsonl`. They are not automatic retries.

Therefore reasoning suppression is **verified**, and a reasoning-token cap is
not required for this route. The earlier `todo_list` protocol failure was an
adapter allow-list defect, not a model or reasoning failure; it is fixed by
admitting only that non-tool plan diagnostic while continuing to reject command,
file, MCP, web, and unknown execution items.

## Execution-condition amendment

All local-factorial-v2 local arms must use `model_reasoning_effort=none` from
this point forward. Existing frontier rows used their historical reasoning
configuration (effectively `high`) and are not rerun. This is a known
asymmetry: frontier and local rows are descriptive comparisons, not a perfectly
matched reasoning-setting factorial contrast. It must appear in every final
comparison report.

This gate pass permits progression to the preregistered local B1/full pilot
only after the normal schema-adherence threshold is applied. It does not alter
the frozen frontier data or make an inference about detection performance.
