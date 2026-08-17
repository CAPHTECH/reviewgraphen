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
