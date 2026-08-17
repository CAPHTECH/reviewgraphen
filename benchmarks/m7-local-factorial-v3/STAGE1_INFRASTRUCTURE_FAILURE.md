# Stage 1 non-semantic attempts and measurement correction

Two non-semantic attempts occurred before any registered v3 result:

1. `v3-stage1`: wrapper omitted `M7_V3_REASONING_EFFORT=none`. It did issue a
   model request; raw SSE is isolated under
   `/tmp/m7-local-factorial-v3-runs/v3-stage1/` and showed reasoning events.
   It consumed no semantic attempt.
2. `v3-stage1-r1`: the corrected wrapper set `reasoning_effort=none`. The
   experimenter interrupted it with SIGINT after 282,318 ms; the recorded
   process status is 130. It produced no candidate and is not counted toward
   the 4/4 gate. This was an experimenter interruption, not an observed
   infrastructure failure.

The request-shaper JSONL is written only after `forward()` returns with a
complete or failed upstream response. Therefore its zero-byte JSONL cannot
establish that no request arrived; it can also mean that an upstream response
was still in progress. The earlier claim that no request reached the shaper was
unsupported and is withdrawn.

An earlier status report also described the attempt as taking 7 ms and exiting
with status 4. Those numbers were the byte sizes of the
`elapsed-milliseconds` and `process-status` files reported by `find -printf
'%s'`, not their contents. The measured values are 282,318 ms and status 130.

Neither attempt is a v3 stage1 result or consumes a semantic attempt. Their run
roots are separate from the replacement run, so their generated data cannot be
collected into the v3 gate. The preregistered 4/4 valid threshold remains
unchanged.

## Upstream server unresponsive observation

At 2026-08-16T22:30:48+09:00, `v3-stage1-r2` had waited more than 79
minutes for the same snapshot-06 B1 input without receiving an upstream
response header. No raw partial response had been created. Before this point,
local socket inspection had established the complete connection path from
`reviewer-backend` through the request shaper to `192.168.68.71:11999`.

The server administrator independently observed at this time that `/api/ps`
and `/api/version` responded immediately. `/api/ps` reported
`qwen3.8:27b-mlx` resident with 18.3 GB VRAM and context length 262,144;
`/api/version` reported 0.32.13. The chat request remained without a response
header. This is classified as `upstream_server_unresponsive`, distinct from
the v2 HTTP 500 response and distinct from an experimenter interruption.

No new provider request or retry was issued. The r2 process was left running
for the server administrator to decide whether to stop. r1 remains classified
as an experimenter interruption: r2 makes it plausible that r1 was observing
the same server symptom, but does not establish what r1 would ultimately have
returned.

For context, the corresponding v2 request reported 16,730 provider input
tokens. The shared server performance observation for 13k-token prefill was
54.9 seconds, so the r2 wait exceeded that reference by roughly two orders of
magnitude. This timing difference is recorded as an observation, not used as
the failure criterion. A possible prefix-cache miss remains unverified.

## Final r2 outcome

The 79-minute statement above was an interim observation. The process
ultimately returned after 28,951,818 ms (8 hours 2 minutes) with process status
1. Request sequence 1 recorded 28,951.693 seconds, upstream HTTP 502, zero
output-text deltas, zero reasoning deltas, and a 79-byte compressed raw
response. Request sequence 2 returned HTTP 502 after 0.013 seconds, with a
99-byte compressed raw response. The second sequence was generated inside the
same already-running Codex invocation; the experimenter did not start a new
trial or external retry.

This final result confirms the `upstream_server_unresponsive` classification:
the chat request waited for hours and then failed, while control endpoints had
remained responsive. It is not a schema result and consumes no v3 semantic
attempt. It is distinct from v2's immediate `temporary high demand` HTTP 500.
