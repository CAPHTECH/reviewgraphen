# Output-cap amendment: max_output_tokens 65,536 -> 131,072

Status: frozen before any further generation call. Amends
`preregistration.json` `generation_execution_condition`. Decided after
`OUTPUT_CAP_STOP_AND_PROBE.md` (the stop) and `LOW_REASONING_PROBE.md`
(negative result), before any new generation result exists under the
raised cap — this ordering matters and is stated for the record: no
qwen_skill result under 131,072 informed this number.

## What changes

`max_output_tokens: 65536` becomes `max_output_tokens: 131072`. No other
execution-condition value changes: backend, model, `reasoning_effort=none`,
sampling, 262,144 context window, 3,600,000 ms idle timeout, zero retries,
concurrency 1 — all unchanged. This requires updating the shaper's
enforced limit (`request_shaper.py`'s `MAX_OUTPUT_TOKENS` and the injected
`X-ReviewGraphen-Max-Output-Tokens` header) and the profile's declared
`max_output_tokens`, both changed together so the enforced cap and the
declared cap never disagree.

## Why 65,536 was wrong, not just conservative

`max_output_tokens: 65536` was carried over unchanged from
`m7-local-factorial-v3`'s execution condition, itself set for a different
task (single-pass defect finding against a fixed, ReviewGraphen-supplied
obligation set) with no self-enumeration step. It was never re-derived for
`qwen_skill`'s actual input/output balance. The context window is 262,144
tokens; the largest admitted `qwen_skill` packet is bounded by
`units.json`'s 120,000-byte source cap (roughly 30,000 tokens) plus the
frozen skill body and schema (bringing total input to roughly 40,000
tokens for the largest unit) — leaving on the order of 200,000+ tokens of
context that a 65,536-token output cap could never reach. `head-local-01`
failed at exactly 65,535 of 65,536 output tokens
(`OUTPUT_CAP_STOP_AND_PROBE.md`) — a value pinned to the cap itself, which
is the signature of hitting a wall, not evidence about how much the model
actually needed.

## Why 131,072, not a much larger number — recorded as a deliberate diagnostic choice

Two independent reasons, both decided before any result under the new cap
exists:

1. **Diagnostic value.** Doubling the cap and observing the outcome
   distinguishes two different explanations for `head-local-01`'s
   failure. If most trials now complete, 65,536 was simply too narrow a
   wall. If roughly half still fail even at double the budget, that is
   evidence the model's reasoning volume for this task is effectively
   unbounded for at least some inputs, and raising the cap further would
   not fix it — only relabel where the wall sits. Setting the cap far
   higher (close to the ~200,000-token headroom) would destroy this
   distinction: a pass at a huge cap would not tell us whether 131,072
   would also have passed, and a fail at a huge cap would leave no cheaper
   next step to try. 131,072 is chosen specifically so the result remains
   informative either way.
2. **Server-side failure exposure, a known and accepted tradeoff.** This
   server has been observed to fail on long-running streams independent of
   any client-side token cap: a 2h39m stream ended in `Model unloaded`
   (`m7-local-factorial-v3`), and a 57-minute two-way parallel session
   crashed both trials (`PARALLEL2_AMENDMENT.md`). A higher output cap
   directly increases how long a single trial can legitimately keep
   streaming, which increases exposure to exactly these server-side
   failure modes — raising the cap risks trading a client-side budget
   failure for a server-side stream failure, which is not actually
   progress. `head-local-00`'s successful trial under the old cap ran
   3,051 s (50.9 min) using 64,421 of 65,536 tokens; roughly doubling the
   token budget is expected to roughly double plausible per-trial
   duration for the trials that do use most of their budget, putting a
   single `qwen_skill` trial in the range of 1.5-2 hours — below the
   2h39m point where a stream-ending failure has already been observed,
   but not by a large margin. This tradeoff is accepted, not hidden: it is
   why the cap is not raised further than needed to get diagnostic value
   out of doubling it.

## Known risk, explicit

Raising the output cap increases the wall-clock duration of any trial
that uses most of its budget, which increases (not decreases) exposure to
the server-side stream failures already observed in this benchmark family
(`upstream_server_stream_incomplete`, `upstream_model_crash`,
`Model unloaded`). A trial that would have failed at 65,536 tokens on a
reasoning-budget wall could instead fail later, at a similar wall-clock
duration, to a server-side stream failure instead — this would be recorded
under its own existing exclusion category (consuming no semantic attempt)
rather than as `empty_final_after_process_completion`, and the two failure
modes must not be conflated when reporting completion rates under the new
condition.

## Non-pooling

Results obtained under the old 65,536-token cap
(`head-local-00`: valid; `head-local-01`:
`empty_final_after_process_completion`; both recorded in
`OUTPUT_CAP_STOP_AND_PROBE.md`) are **not pooled** with results obtained
under this amendment. The `qwen_skill` arm restarts all 9 units from the
beginning under the new cap.

## Preregistration update

`preregistration.json` `generation_execution_condition.max_output_tokens`
is updated to `131072` by this same commit, with a pointer to this
document.
