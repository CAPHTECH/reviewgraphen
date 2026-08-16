# Local v2 schema pilot: `reasoning_effort=none`

This is an additive record. Frozen v1/v2 artifacts and the prior reasoning
control records are unchanged.

Stage 1 ran four control trials with the recovered server and the fixed local
condition `qwen3.8:27b-mlx`, Codex profile `ollama-priv-v2`, and
`model_reasoning_effort=none`.

Observed outcomes:

- snapshot-06 B1: `candidate_schema_invalid`; final content was nonempty,
  thinking was 0, and the response began with prose before an embedded JSON
  abstention object.
- snapshot-06 full: `candidate_schema_invalid`; final content was nonempty,
  thinking was 0, and the response contained prose plus a fenced JSON object.
- snapshot-34 B1: `valid`; final content was nonempty, thinking was 0, and
  candidate validation and collection passed.
- snapshot-34 full: `upstream_server_failure`; the shaper recorded HTTP 500
  after a completed error event stating temporary high demand. No candidate
  score was assigned.

The stage result is therefore 1 valid, 2 model-output schema/framing failures,
and 1 environment/provider failure. It did not meet the preregistered 4/4
stage-1 threshold, so production 40-trial execution is stopped.

The two schema failures are measured model-output contract failures, not
reasoning or adapter execution failures: both had nonempty final text and zero
reasoning tokens. The HTTP 500 is separately classified as an environment
failure and is not evidence that the model cannot follow the schema.

The raw provider streams for all 19 requests are retained as base64 gzip
artifacts. The provider output is not normalized or repaired.

No reasoning-high final response with retained raw bytes exists for a valid
same-input comparison. The historical snapshot-06 `final_content_tokens=6606`
value remains under the earlier measurement defect and its raw stream is not
available; it is not used to claim a high-vs-none schema difference.

The adapter accepts arbitrary reasoning strings at its typed boundary, so
`low` could be tested in a separately preregistered study. It was not run here:
adding it after observing these results would be a new semantic attempt and
would violate the frozen stage-1 design.
