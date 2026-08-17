# LM Studio stream idle-timeout amendment

Status: frozen after the pre-amendment stage1 results and before any
post-amendment provider request.

The pre-amendment stage1 observed Codex 0.147 terminate snapshot-34 B1 after
300,305 ms with `idle timeout waiting for SSE`. This is a measurement-system
limit, not the candidate schema or ReviewGraphen scaffold. The condition is
changed transparently rather than retrofitted into the frozen result.

For the replacement stage1, the profile sets
`stream_idle_timeout_ms = 3600000` (60 minutes). The shared slowest prefill
measurement is 37 minutes at approximately 205k tokens. Sixty minutes exceeds
that observation by 23 minutes (about 62%) while remaining finite and
auditable. The timeout measures absence of SSE activity, so it does not cap a
response that is actively streaming.

The provider profile also explicitly retains `request_max_retries = 0` and
`stream_max_retries = 0`. Those two keys were already zero in both the v2 and
initial v3 profiles; they were not implicit defaults. Nevertheless the frozen
r2 Ollama-era failure recorded two upstream request sequences after an
eight-hour 502. Therefore the keys alone did not prevent every higher-level
Codex follow-up in that run. The v3 shaper now independently refuses to forward
any POST after the first upstream 5xx, and the batch has no external retry.

This is a post-result execution-condition amendment. The replacement stage1
runs all four assigned trials from the beginning under one uniform condition:
LM Studio behind cch, Codex CLI 0.147.0, `reasoning_effort=none`, provider
default sampling, 262,144 context tokens, 65,536 output tokens, 3,600,000 ms
SSE idle timeout, and zero configured retries. Its results must not be pooled
with the pre-amendment stage1.

The 4/4 semantic-validity threshold is unchanged. `client_idle_timeout` and
`upstream_server_unresponsive` remain excluded from the semantic denominator,
consume no semantic attempt, and stop execution without retry.

## Post-amendment observation

The first replacement trial crossed the old 300-second idle boundary and then
streamed continuously, directly verifying that the 3,600,000 ms setting was
applied. It ran for 9,536.248 seconds before the provider stream ended with
`Model unloaded.` Codex exited 1, the shaper retained HTTP status 200 plus an
incomplete SSE stream, and the batch stopped before issuing another request.
This is classified as `upstream_server_stream_incomplete`, a server-side
execution failure distinct from a 502-before-stream response and from the old
client idle timeout. It consumes no semantic attempt.

The failing request was noticed at 2026-08-17T10:52:54+09:00. The client was
Codex CLI 0.147.0. Completed usage was absent, so an exact input-token count is
unavailable; the same cell previously measured 16,496 provider input tokens
under the pre-amendment LM Studio condition. No automatic or operator retry
was issued.
