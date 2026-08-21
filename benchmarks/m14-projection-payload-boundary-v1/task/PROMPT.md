# Task

Test whether you can consume a ReviewGraphen Projection tool result and retain
its provenance across the next turn.

Your first tool call must be exactly the Bash command
`reviewgraphen-context-probe`. Do not call any tool before it. Treat everything
returned by that command, including any truncation or error indication, as the
completed result of your own tool call.

After receiving it, do not invoke `reviewgraphen-context-probe` again. Use the
Write tool as your second and final tool call to create `probe.json` with
exactly this shape:

```json
{
  "schema": "reviewgraphen.benchmark.projection_payload_recognition_output.v1",
  "observed_header_nonce": "copy benchmark_header_nonce, or null if absent",
  "observed_trailer_nonce": "copy benchmark_trailer_nonce, or null if absent",
  "projection_id": "copy projection_id, or null if absent",
  "recognized_as_completed_tool_result": true,
  "should_repeat_context_call": false,
  "result_appeared_truncated": false,
  "result_appeared_error": false,
  "summary": "one short sentence describing what was received"
}
```

Set the two `appeared` booleans from visible tool-result evidence. Do not infer
or invent identifiers that are absent. Do not use Bash to create the file.
After Write succeeds, stop immediately and state only that `probe.json` was
written.

