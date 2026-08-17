# qwen_skill stopped: output cap was our own constraint, not a server limit

Status: recorded immediately after stopping. Not a time-based or
outcome-based stop: the execution condition itself (max_output_tokens)
was found to be an unexamined self-imposed limit, not a server or model
constraint, and needs to be corrected before generation continues.

## What was stopped

`head-local-02` (qwen_skill), in progress since 2026-08-17T21:50 JST
(9,568,256 bytes streamed, no completed response), was operator-terminated
at approximately 2026-08-17T22:0x JST. No `generation-metrics.json` or
`candidate.json` was written for it — it consumes **zero semantic
attempts**, the same `operator_terminated` classification already used
for the earlier `head-local-00` `qwen_full` stop
(`STOPPED_CONSTRUCT_VALIDITY.md`). The batch process, request shaper, and
all child processes were confirmed fully terminated before this record was
written.

## The two results already obtained under the old (65,536-token) output cap

These are preserved as observations under the condition that produced
them and are **not pooled** with whatever condition replaces it:

| Unit | Outcome | Elapsed | reasoning events / tokens | output events / tokens | Notes |
| --- | --- | --- | --- | --- | --- |
| `head-local-00` | `valid` | 3,051.361 s | 60,128 / 62,619 | 1,802 / (non-reasoning 1,802) | 8 obligations enumerated (cap respected), 4 `issue_present` with findings, 2 `issue_absent`, 2 `inconclusive`; `provider_output_tokens` 64,421/65,536 |
| `head-local-01` | `empty_final_after_process_completion` | 3,070.295 s | 63,120 / 65,535 | 0 / 0 | `provider_output_tokens` 65,535/65,536, entirely reasoning, zero final content |

## Why this is a condition defect, not a result

`max_output_tokens: 65536` in `preregistration.json`
`generation_execution_condition` was inherited unchanged from
`m7-local-factorial-v3`, where it was set for a different task (bare
defect-finding, no self-enumeration). It was never re-derived for the
`qwen_skill` task's actual budget headroom. The context window is 262,144
tokens; the largest admitted packet is at most roughly 120,000 bytes of
source (~30,000 tokens) plus the skill body and schema (bringing total
input to roughly 40,000 tokens) — leaving on the order of 200,000+ tokens
of context unused by input, none of which the 65,536-token output cap
could ever reach. `head-local-01`'s failure at exactly 65,535/65,536
output tokens is consistent with hitting this self-imposed wall, not with
any evidence that the model's reasoning process itself is unbounded — that
distinction is exactly what the low-reasoning probe and the raised-cap
observation below are designed to establish.

## Next: minimal low-reasoning probe, then an output-cap amendment

Per operator instruction, before raising the output cap: a single minimal
synthetic-input probe checks whether `reasoning_effort=low` (untested;
`none` is already confirmed not to suppress reasoning on this LM Studio
backend, per `m7-local-factorial-v3`/`LM_STUDIO_TRANSITION.md`) has any
effect on this backend. Recorded separately in this same directory after
it runs. The output-cap amendment (target 131,072, rationale, and the
known increased-exposure-to-server-side-failure risk) is written and
committed only after that probe's result is known and reported.
