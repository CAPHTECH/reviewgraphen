# LM Studio parallel-2 execution amendment

Status: frozen before any parallel-2 provider request and after the sequential
recovery attempt was interrupted by the experimenter.

The server administrator changed the server capacity to two concurrent model
requests. A new stage1 attempt will therefore issue at most two trials at a
time. Parallelism is an execution condition, not the measured semantic
property. The candidate schema, ADR 0037 extraction contract, model settings,
context and output limits, zero-retry policy, semantic classifications, and
the v3 4/4 gate are unchanged.

Parallel-2 results are not pooled with any sequential attempt. The known risks
are reduced prefix-cache reuse and server resource contention; their effects
on latency, completion, reasoning volume, and validity will be reported from
observations. No third trial may be started while two trials are active.

One shared thread-capable request shaper remains on port 12080. Starting one
fixed-port shaper per trial would collide. Instead, each concurrently running
trial receives a unique `X-ReviewGraphen-Trial-Key` in its isolated temporary
Codex profile. The shaper validates and records that key, removes it before
forwarding upstream, serializes JSONL writes, and uses its existing locked
capture sequence for unique raw artifacts. Each runner selects only transport
rows bearing its key. Thus the measurement-only routing header does not change
the provider request and evidence remains unambiguous.

Result directories include snapshot and arm and are disjoint. Each runner
already creates a distinct `mktemp` Codex home. Raw captures share a directory
but have lock-assigned sequence names and are copied into the keyed trial's
result directory with hash verification.

Trials launch in waves of at most two. If either active trial observes an
upstream 5xx or incomplete stream, no next wave starts. The already-active peer
is allowed to finish; it is not killed. The shaper's existing global failure
barrier rejects a request that has not yet reached upstream after a prior 5xx.
There is no external retry, and Codex request and stream retry counts remain
zero.

The sequential recovery attempt `v3-lm-studio-stage1-recovery-1` was stopped
after the parallelism decision and is not pooled. It did issue a provider
request: a 1,245,184-byte partial SSE with 6,136 reasoning-text delta events
and no output-text delta was retained. Codex exited 130 after 305,555 ms; the
shaper was then stopped before it could write a completed transport row. This
is an experimenter interruption, not `protocol_invalid`, and consumes no
semantic attempt.

## Observed result

The first parallel wave started at approximately
2026-08-17T11:12:52+09:00 and both trials ended at
2026-08-17T12:10:22+09:00. Routing keys correctly associated each transport
row and raw artifact with its trial. No third request was issued.

Both upstream responses used HTTP 200 and ended with a
`response.failed`/`internal_error` event reporting that the model had crashed.
Codex surfaced `stream disconnected before completion: The model has crashed
without additional information. (Exit code: null)` for both trials. These are
recorded as the `upstream_model_crash` subtype of the existing
`upstream_server_stream_incomplete` exclusion, not semantic failures.

- snapshot-06 B1 ran 3,449.481 seconds and retained 7,085,799 raw SSE bytes,
  32,876 reasoning delta events (132,892 UTF-8 payload bytes), and zero output
  text events.
- snapshot-06 full ran 3,449.289 seconds and retained 72,632 raw SSE bytes,
  zero reasoning delta events, and zero output text events.

Thus the parallel-2 stage1 attempt has zero semantic completions, two excluded
server failures, and two unissued cells. The v3 4/4 gate is not evaluated from
this attempt. The simultaneous failures are compatible with the preregistered
resource-contention risk, but do not establish that parallelism caused the
crash: the earlier sequential LM Studio run also ended in a model unload.

This result materially limits operational feasibility. Two small stage1 cells
occupied the server for about 57.5 minutes and neither completed. The issue is
completion reliability, not an elapsed-time stopping policy. No retry was
issued after the failures.
