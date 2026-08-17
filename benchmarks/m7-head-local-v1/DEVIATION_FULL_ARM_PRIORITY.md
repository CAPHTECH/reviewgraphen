# Deviation: full-arm-first execution order, b1 reduced to 1 unit

Status: recorded and committed before any further generation trial is
issued. This is an explicit, disclosed deviation from
`preregistration.json` `execution.trial_order`
("units in unit_index order 0..8; within each unit, qwen_b1 before
qwen_full"), decided by the operator after observing generation results —
that timing is stated plainly below, not hidden.

## What changed

`preregistration.json`'s original trial order interleaves both arms per
unit across all 9 units (18 trials total: u0-b1, u0-full, u1-b1, u1-full,
...). As of this amendment, `qwen_b1` stops after its one completed
observation (unit `head-local-00`); the remaining 8 units run `qwen_full`
only, unit by unit in order (`head-local-01` through `head-local-08`).
`qwen_b1` is not run again unless a separate, later decision restarts it.

## Why

The operator's original research request was specifically about
`qwen3.8:27b-mlx` running with the ReviewGraphen scaffold
(`qwen_full`) on live, previously-unexamined `fsl` HEAD code.
`qwen_b1` was added to the design as the necessary comparator so "is
ReviewGraphen useful" would have an answer (see `README.md`), not because
it was the primary object of interest.

By the time this decision was made, `qwen_b1` had been observed to fail
completely, twice, in the same way:

| Observation | Elapsed | provider_output_tokens | Final content |
| --- | --- | --- | --- |
| `m7-local-factorial-v3` stage1, snapshot-06 B1 (a different unit, same execution condition and model, from before this experiment existed) | 5,146.477 s (85.8 min) | 65,535 / 65,536 | 0 bytes |
| `m7-head-local-v1` head-local-00, `qwen_b1` | 6,548.797 s (109.1 min) | 65,535 / 65,536 | 0 bytes |

Both observations show reasoning consuming essentially the entire output
budget with zero final content — the same failure mode
(`empty_final_after_process_completion`) each time. Continuing to run
`qwen_b1` on the remaining 8 units was projected to cost roughly 13 more
hours of server time (extrapolating from these two observations) with a
low prior expectation of a different outcome, while `qwen_full` — the
operator's actual object of interest — was still unexecuted for 8 of 9
units. The operator judged the marginal value of 7 more near-certain
`qwen_b1` failures too low relative to that cost, and redirected the
remaining execution budget to `qwen_full`.

## When this decision was made (stated plainly)

This decision was made **after** observing both `qwen_b1` failures — the
`m7-local-factorial-v3` one (from a closed, prior experiment) and the
`head-local-00` one (from this experiment, generated before this
amendment). It is a real, disclosed deviation from the original frozen
trial order, made in response to an observed result, not a
before-the-fact design choice. It is recorded here specifically because
the discipline this whole benchmark program follows requires exactly that
disclosure whenever a condition changes after a result is seen.

## Measures against selective reporting

Both `qwen_b1` observations — not just one, and not a cherry-picked
favorable one — are reported above in full, including their exact
`generation-metrics.json` figures. Both show the identical failure mode.
This is stated explicitly so a reader can judge for themselves whether the
decision to stop `qwen_b1` was reasonable, rather than taking the
operator's or this assistant's word for it. No `qwen_b1` observation is
omitted, redacted, or described only in aggregate.

## Consequences for comparison

`qwen_b1` now has n=1 unit; `qwen_full` will have n=9. This is not a
matched, balanced comparison and must never be presented as one. No
statistical comparison, significance claim, or superiority claim between
arms is made anywhere in this experiment's reporting. `qwen_b1`'s single
observation is reported only as a narrow, descriptive fact: on the one
unit attempted, raw unscaffolded `qwen3.8:27b-mlx` did not complete —
consistent with, but not proof beyond, the earlier `m7-local-factorial-v3`
observation of the same failure mode on a different unit. See
`preregistration.json` `usefulness_determination` (updated by this same
commit) for the corresponding change to how results are reported.

## What is unchanged

The generation execution condition (backend, model, reasoning effort,
sampling, context/output limits, idle timeout, retry policy, concurrency)
is exactly as frozen in `preregistration.json`
`generation_execution_condition` and is not touched by this amendment. Unit
selection (`units.json`), the candidate schema, ADR 0037 extraction, and
the judge protocol (`JUDGE_PROTOCOL.md`) are all unchanged. Blinding
remains fully intact: the judge still never learns arm identity, and
`truth.json` will correctly record `qwen_b1` as contributing to only
`head-local-00`'s finding pool and to no other unit's.
