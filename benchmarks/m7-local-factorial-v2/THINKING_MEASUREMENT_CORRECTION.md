# Thinking measurement correction

Status: additive correction; frozen observations are not rewritten.

The first v2 collector treated Responses usage as content semantics. It copied
`output_tokens_details.reasoning_tokens` to `thinking_tokens` and calculated
`final_content_tokens = output_tokens - reasoning_tokens`. That calculation is
invalid for this proxy: the provider reported reasoning usage as zero while
streaming `response.reasoning_summary_text.delta` events. Provider usage alone
does not say which output-item type carried the tokens.

The authorized snapshot-34 B1 diagnostic rerun was intentionally stopped after
the cause was visible. The retained partial gzip is incomplete by design and
is not a successful or protocol-valid trial. Its recoverable prefix contains
12,902 complete data lines: one `response.created`, one
`response.in_progress`, one reasoning `response.output_item.added`, and 12,898
`response.reasoning_summary_text.delta` events. The maximum sequence number is
12,900. The deltas contain 62,351 UTF-8 bytes. There is no
`response.output_text.delta`, completed response, or final assistant message.

This directly establishes reasoning-only generation for the diagnostic rerun.
It also supplies a concrete mechanism consistent with the earlier snapshot-34
B1 empty-final observations. It does not make the nondeterministic rerun byte
identical to the frozen prior attempt, whose 9.3 MB raw stream was not retained.
Therefore the prior 49,624 output tokens are reclassified as
`reasoning_final_split_unknown_under_measurement_defect`, rather than asserted
as exactly 49,624 reasoning tokens or final-content tokens.

The snapshot-06 full raw provider stream was not retained. Its historical
`thinking_tokens=0` and `final_content_tokens=6,606` are also under the same
measurement defect. Its nonempty 1,030-byte materialized final response and
valid candidate schema remain measured facts; its exact reasoning/final token
split cannot be reconstructed.

New transport records classify event semantics before token attribution. If a
completed response has only reasoning deltas, all provider output tokens are
attributed to thinking and final-content tokens are zero. If it has only
output-text deltas, all provider output tokens are attributed to final content.
Mixed or absent content event classes remain unknown rather than being split
by subtraction. Exact event counts and UTF-8 bytes are always recorded.

The v2 profile did not disable thinking. The adapter explicitly passed
`model_reasoning_effort=high`; the captured provider request contains
`reasoning.effort=high` and `reasoning.summary=auto`. Earlier interpretations
based on provider-reported zero reasoning usage are withdrawn.
