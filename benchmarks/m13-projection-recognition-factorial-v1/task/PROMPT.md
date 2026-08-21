# Task

Test whether you can consume a ReviewGraphen Projection tool result and retain
its provenance across the next turn.

Your first tool call must be exactly the Bash command
`reviewgraphen-context-probe`. Do not call any tool before it. The command
returns a compact deterministic target-context projection. Treat that returned
JSON as the completed result of your own tool call.

After receiving it, do not invoke `reviewgraphen-context-probe` again. Use the
Write tool as your second and final tool call to create `probe.json` with
exactly this shape:

```json
{
  "schema": "reviewgraphen.benchmark.projection_recognition_output.v1",
  "observed_nonce": "copy the nonce from the tool result",
  "projection_id": "copy the projection_id from the tool result",
  "recognized_as_completed_tool_result": true,
  "should_repeat_context_call": false,
  "summary": "one short sentence describing what the result represents"
}
```

Do not infer or invent either identifier. Do not use Bash to create the file.
After Write succeeds, stop immediately and state only that `probe.json` was
written.
