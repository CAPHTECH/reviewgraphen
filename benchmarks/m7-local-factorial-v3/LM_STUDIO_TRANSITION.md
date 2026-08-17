# LM Studio backend transition and capacity-claim audit

Status: recorded before the first LM Studio model request.

The endpoint and model identifier remain
`http://192.168.68.71:11999` and `qwen3.8:27b-mlx`, but the server behind the
cch-normalizing proxy changed from Ollama 0.32.13 to LM Studio. Sampling
parameters remain provider defaults; ReviewGraphen does not set temperature or
top-p. The 262,144-token context and 65,536-token output limit remain explicit.

Reasoning suppression verified against Ollama is not assumed to transfer to
LM Studio. Before restarting v3 stage1, one minimal Codex request must verify
from retained raw SSE whether `reasoning_effort=none` yields zero reasoning
delta events and zero provider-reported reasoning tokens. No large benchmark
input is admitted to this probe. If reasoning is present, the same response is
used to determine whether a nonempty final body is produced within 65,536
output tokens; no automatic retry is permitted.

The server administrator withdrew the claims that 200k input tokens are a
danger zone and that reading a large file causes the server failure. The r2
failure occurred with approximately 16,283 provider input tokens, so context
size is not identified as its cause and the client could not have prevented
it by a capacity guard.

The repository audit found no experimental conclusion that relies on the
withdrawn causal claim. The v2 G3-proxy exclusion remains justified
independently: its frozen 8,131,409-character input cannot be admitted to the
262,144-token model context without changing or truncating that historical
arm. This is a structural fit constraint, not a server-stability claim.
Existing v2 capacity-audit rows are explicitly risk labels derived from an
empirical byte/token interval, not a claim that large inputs caused the server
failures. Frozen v2 observations are not changed.

If the LM Studio path exhibits the same server symptom, the record must include
the time noticed, approximate token count of the immediately preceding
request, and that Codex was the client. No retry or next benchmark request may
be issued.

## Minimal reasoning-control probe result

The single LM Studio provider request completed with HTTP 200 in 29.460
seconds. Codex CLI 0.147.0 sent 6,996 input tokens and the provider reported 36
output tokens. Despite `reasoning_effort=none`, raw SSE contained 34
`response.reasoning_text.delta` events and the provider reported 34 reasoning
tokens. Reasoning suppression is therefore **not reproduced** on LM Studio.

The same response contained two `response.output_text.delta` events and a
nonempty final body, `OK`. Thus this minimal request reached final content
within the unchanged 65,536-token output allowance. No conclusion is made
about larger benchmark prompts from this one request. The result changes the
recorded backend condition but does not alter temperature, top-p, schema,
projection, oracle, or scoring.
