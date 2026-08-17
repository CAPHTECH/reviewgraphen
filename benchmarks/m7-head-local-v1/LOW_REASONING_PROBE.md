# reasoning_effort=low probe result: no effect, confirmed byte-identical

Status: minimal synthetic probe, does not touch any of the 9 real units.
Result: negative — `low` does not reduce reasoning on this backend.
Proceeding to the output-cap amendment per the operator's pre-agreed
branch ("効かないなら…出力上限の引き上げのみで進めること").

## What was run

Same minimal pattern as the already-frozen `reasoning_effort=none` probe
(`LM_STUDIO_TRANSITION.md`): a single Codex CLI request through the same
LM Studio/cch profile and request shaper, prompt `"Reply with exactly: OK.
Do not use tools."`, only `model_reasoning_effort` changed to `'low'`.

## Result

| | `none` (frozen, `LM_STUDIO_TRANSITION.md`) | `low` (this probe) |
| --- | --- | --- |
| elapsed | 29.460 s | 211.71 s |
| provider_input_tokens | 6,996 | 6,989 |
| provider_output_tokens | 36 | 36 |
| reasoning tokens | 34 | 34 |
| reasoning_delta_events | 34 | 34 |
| reasoning text SHA-256 | `c2d74893d92c39ad053ff0d67fd47099519870c77257370793d1673d8b7ed36e` | `c2d74893d92c39ad053ff0d67fd47099519870c77257370793d1673d8b7ed36e` |
| final text SHA-256 | `db8b8e836881534b3e62cf633db64f28af421e09feaae85bd3f3249912053c65` | `db8b8e836881534b3e62cf633db64f28af421e09feaae85bd3f3249912053c65` |

The reasoning content and the final answer are **byte-for-byte identical**
between `none` and `low` — not merely similar in magnitude. This is
direct, mechanical evidence (SHA-256 match on the actual generated text,
not an inference from token counts) that `reasoning_effort` has no effect
on this backend's output at all: it is not read, or is read and ignored,
by whatever sits behind the LM Studio/cch proxy for this model. The only
observed difference, elapsed wall-clock time (29.5s vs 211.7s), is
consistent with ordinary server-side latency variance (queueing, cache
state) unrelated to the parameter, not with any generation difference —
the generation itself did not change.

## Decision

`reasoning_effort` is not a viable lever for this backend at any value
tried so far (`none`, `low`). Per the operator's pre-agreed branch, this
is recorded and the output-cap increase proceeds as the sole remaining
change, without adopting `low` into the execution condition.
