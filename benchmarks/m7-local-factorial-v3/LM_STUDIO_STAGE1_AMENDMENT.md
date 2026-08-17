# LM Studio stage1 gate amendment

Status: frozen after the minimal backend probe and before the first LM Studio
stage1 result.

The original v3 4/4 semantic-validity threshold is unchanged. An
`upstream_server_unresponsive` outcome is excluded from the semantic
denominator and consumes no semantic attempt because it measures provider
availability rather than candidate behavior. It stops the batch immediately;
no retry or next trial is issued pending the server administrator's decision.

Every completed stage1 trial records from retained raw SSE:

- `reasoning_delta_events` and `reasoning_delta_utf8_bytes`;
- `output_text_delta_events` and `output_text_delta_utf8_bytes`;
- provider-reported reasoning tokens; and
- whether final content was empty.

Outcomes are classified without merging causes:

- `reasoning_runaway`: reasoning is emitted but final content is empty, or the
  output/context allowance is reached before final content;
- `candidate_schema_invalid`: final content is nonempty, deterministic ADR
  0037 extraction runs, but the extracted unchanged candidate schema fails;
- `upstream_server_unresponsive`: upstream HTTP 502 or a chat request remains
  unresponsive while control endpoints respond;
- `client_idle_timeout`: Codex stops waiting for an SSE event before the
  provider produces a completed response;
- `infrastructure`: another execution-path failure; and
- `valid`: extraction and the unchanged candidate schema both succeed.

For every `candidate_schema_invalid`, the record distinguishes failure to find
a JSON object, extracted JSON that remains schema-invalid, and any other
deterministic extraction failure. This directly tests whether v2's prose/fence
packaging failures are admitted by v3 without repair or normalization.

The minimal LM Studio probe does not establish that reasoning terminates or
that final content fits within 65,536 tokens on review workloads. It establishes
only that `reasoning_effort=none` did not suppress reasoning and that the
trivial `Reply with exactly: OK` request produced nonempty final content.
