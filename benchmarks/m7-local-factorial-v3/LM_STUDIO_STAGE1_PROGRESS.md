# LM Studio stage1 execution record

## Runner plumbing interruption

The first LM Studio batch used the preregistered model condition, but the
repository copy of `run_trial.sh` still passed a second output-path argument to
the one-argument ADR 0037 extractor. The model response completed; only the
post-response deterministic extraction command failed. The batch then began
snapshot-06 full before the plumbing defect was identified. That second trial
was interrupted after 40,497 ms with process status 130 and a zero-byte partial
response. It produced no final response and consumes no semantic attempt.

The extractor invocation was corrected before any remaining semantic call.
The completed B1 response was not regenerated. Its retained raw final was
processed once by the already-frozen extractor and unchanged candidate schema
in a separate recovery directory.

## Snapshot-06 B1 recovery

The raw final contained 3,398 bytes; deterministic extraction produced a
3,397-byte candidate. Candidate validation and collection both succeeded with
candidate hash
`sha256:09cace30767b0d9fe42a95fb71935d5ed7f718be8810e3e2b50f5d0c3b374b17`.
This is one semantic-valid result toward the unchanged 4/4 gate.

Raw SSE recorded 16,496 provider input tokens, 57,646 provider output tokens,
56,865 provider-reported reasoning tokens, 56,295 reasoning delta events
(212,003 UTF-8 bytes), and 778 output-text delta events (3,398 UTF-8 bytes).
Final content was nonempty. Therefore this trial is neither
`reasoning_runaway` nor `candidate_schema_invalid`.

This case directly verifies the sole v3 packaging hypothesis: although the
model emitted a wrapped response rather than a bare candidate, ADR 0037
extraction reached and passed the unchanged schema. It does not establish that
all prose or fenced responses will pass.

## Snapshot-06 full

The provider completed with HTTP 200 after 2,658.716 seconds. Raw SSE recorded
39,647 input tokens, 25,351 output tokens, 24,422 provider-reported reasoning
tokens, 24,090 reasoning delta events (91,230 UTF-8 bytes), and 928 output-text
delta events (3,086 UTF-8 bytes). Final content was nonempty and ADR 0037
extraction succeeded.

The extracted object failed the unchanged candidate schema because its schema
identifier was misspelled as `reviewgraphen.bandidate_output.v1`. This is
`candidate_schema_invalid`, specifically an extracted-JSON contract failure;
it is not a prose/fence packaging failure and not `reasoning_runaway`.

## Output-budget observations

| Trial | Provider output | Reported reasoning | Reasoning / 65,536 | Remaining output allowance | Final |
| --- | ---: | ---: | ---: | ---: | --- |
| snapshot-06 B1 | 57,646 | 56,865 | 86.8% | 7,890 | nonempty, schema-valid after extraction |
| snapshot-06 full | 25,351 | 24,422 | 37.3% | 40,185 | nonempty, extracted JSON schema-invalid |

Only two semantic trials completed, so this is not the requested four-trial
distribution and does not establish production feasibility. The B1 result had
little output headroom; the full result did not show the hypothesized larger
reasoning burden in this pair.

## Snapshot-34 B1 client idle timeout

At 2026-08-17T08:07:19+09:00 the Codex client reported `idle timeout waiting
for SSE` after 300,305 ms. The request had reached the shaper: its second raw
artifact existed as a zero-byte partial, but no upstream response header or
body arrived and no completion JSONL row could be written. The admitted input
was 867,861 bytes; the closest frozen v2 observation for this unit was 212,520
provider input tokens, but no LM Studio token count is available because the
response did not complete.

This is classified as `client_idle_timeout`, not
`upstream_server_unresponsive`, infrastructure, or a semantic failure. Codex
0.147 stopped waiting for an SSE event at approximately 300 seconds; the
operator did not stop it. It consumes no semantic attempt and is excluded from
the 4/4 denominator. The batch stopped immediately and snapshot-34 full was
not issued. The pre-timeout-amendment stage1 result is therefore one valid
semantic result, one schema-invalid semantic result, one excluded client
timeout, and one unexecuted trial. The 4/4 gate was not met; over the two
semantic completions the observed validity was 1/2.

## Replacement stage1 under the 60-minute idle timeout

The replacement batch began all four cells anew under the amended profile.
Its first cell, snapshot-06 B1, crossed the former 300-second idle threshold,
began streaming, and continued for 9,536.248 seconds. The response then ended
without a completed usage record; Codex reported `Model unloaded.` and exited
1. The shaper recorded HTTP 200, 5,479,124 uncompressed response bytes,
25,530 reasoning delta events (95,192 UTF-8 payload bytes), zero output-text
delta events, and a 238,866-byte gzip artifact with SHA-256
`504f5ecf9523011ef05124358fbc9bfb62c32c8a653fbc83407884b4922f5736`.

This is `upstream_server_stream_incomplete`. It is not a candidate-schema
failure, `reasoning_runaway`, or `client_idle_timeout`: the provider began an
HTTP 200 stream and the configured client timeout did not fire, but the model
was unloaded before any final text or completed usage arrived. It consumes no
semantic attempt. The fixed batch stopped immediately, so no second provider
request was issued.

LM Studio used `response.reasoning_text.delta`, whereas the earlier Ollama
transport used `response.reasoning_summary_text.delta`. Both v2 and v3 shapers
classify any event whose type starts with `response.reasoning_` as reasoning;
only exact `response.output_text.delta` events are counted as final text.
Therefore this event-name difference does not reproduce the earlier token
attribution bug.

The replacement run also demonstrates large stochastic variation for the
same snapshot-06 B1 input and nominal model condition. The earlier successful
run emitted 56,295 reasoning events carrying 212,003 UTF-8 payload bytes and
then a valid final. This run emitted 25,530 reasoning events carrying 95,192
payload bytes but a 5.48 MB SSE envelope before server-side interruption and
never emitted final text. Raw-file size must not be equated with reasoning
payload size because SSE framing and repeated event metadata dominate it.
The divergent trajectory is descriptive support for treating validity as a
stochastic rate; it does not identify what the interrupted run would have
produced had the model remained loaded.

## Server-recovery restart decision

After the server administrator verified a separate minimal chat completion
(57 prompt tokens, 34 completion tokens, `finish_reason=stop`, 13.97 seconds),
provider requests were re-authorized. The model alias
`qwen3.8:27b-mlx` resolved to `qwen3.8-27b-mlx`; this name difference is not
treated as a cause of the preceding interruption.

The interrupted attempt remains frozen and is not pooled. Because
`upstream_server_stream_incomplete` is excluded from the semantic denominator,
a new attempt starts all four cells from the beginning under exactly the same
LM Studio/cch, Codex 0.147.0, provider-default sampling, context, output,
reasoning-effort, timeout, and zero-retry condition. This is recovery from a
server failure, not a changed semantic condition.

The earlier Ollama run ended after an eight-hour wait with 502 responses; the
LM Studio run instead began an HTTP 200 reasoning stream and later ended with
`Model unloaded.` These are distinct transport symptoms. Both nevertheless
show that long-running generation did not complete under two provider-server
implementations, which is evidence consistent with a failure below the
provider-specific surface. It does not identify the shared lower-layer cause.

Operational feasibility is assessed separately from the schema gate. A
single snapshot-06 B1 run can exceed 9,536 seconds and can still fail to
complete. The planned production run is 20 units by 2 arms, or 40 trials.
Elapsed time remains descriptive and is not a stopping criterion, but a low
completion rate would be an execution-feasibility limit rather than merely a
runtime cost.

## Parallel-2 amendment

The sequential recovery attempt was interrupted after the administrator made
two-way server parallelism available. Despite the runner's eventual
`request_count_mismatch` surface, raw evidence proves that one provider request
had been issued: 1,245,184 partial SSE bytes containing 6,136
`response.reasoning_text.delta` events and no output-text delta were retained.
Codex status 130 after 305,555 ms records an experimenter interruption. No
completed shaper row or semantic output exists, so this attempt is not pooled
and consumes no semantic attempt.

The prospective parallel execution contract is frozen separately in
`PARALLEL2_AMENDMENT.md`. A fresh four-cell attempt will use waves of at most
two, with unique trial routing keys and unchanged semantic conditions.
