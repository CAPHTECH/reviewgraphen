# M11 result — agentic review completion diagnostic

## Outcome

Neither observed review produced a valid `review.json` within the preregistered
5,400-second wall-clock cap.

| Observation | Server identity | ReviewGraphen first | Outcome | Elapsed | Provider-reported output | Thinking share | Report |
| --- | --- | ---: | --- | ---: | ---: | ---: | --- |
| `reviewgraphen-1` | original pinned baseline metadata | yes, mechanically compliant | timeout | 5,400 s | 107,447 | 99.86% | absent |
| `control-1` | restarted current-server diagnostic; detailed decode metadata unavailable | no | timeout | 5,400 s | 58,935 | 99.07% | absent |

Both retained source manifests are unchanged and neither retained stream has a
suspected truncated tail. The ReviewGraphen observation made 9 model calls as
recognized by the original-server stream profiler. The current-server control
stream used a changed usage/event projection, so that profiler's `api_calls=1`
is not comparable; direct event counting records 31 tool uses and 31 tool
results. Its provider reported zero input tokens, so no input-token comparison
is made.

## What this answers

The proposed explanation — that the no-ReviewGraphen review would now complete
under the server state available on 2026-08-21 — was **not supported by this
single diagnostic**. The old exact failure shape did change: instead of one
65,535/65,536-token reasoning response with zero final content, the current
agent loop repeatedly reached tool calls and accumulated a nonempty stream.
Nevertheless it did not converge to the required final review artifact within
90 minutes.

This does not establish that no-ReviewGraphen review can never complete. It
establishes only failure under this unit, prompt, current server state, agent
harness, and wall budget. The current server stopped exposing mode, drafter,
reasoning-effort, context-window, and server-output-cap fields before the
control ran, and it required a new canonical model ID. Therefore the two rows
are not a balanced or causal ReviewGraphen comparison. Amendments 002–004
preserve that result-informed transition explicitly.

## Judge

Codex judging was not run. The frozen judge requires a completed candidate
report; neither observation produced one. Calling a judge on an absent report
would conflate generation completion with finding quality. The judge state is
`no_valid_candidate`, not a negative quality disposition.

## Authority

The timeouts, hashes, process statuses, source manifests, and retained streams
are runtime observations. The interpretations above are review-required
inferences. No model output was promoted to accepted fact, evidence,
verification, or human acceptance.

