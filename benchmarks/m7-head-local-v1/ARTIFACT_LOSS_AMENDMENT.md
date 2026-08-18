# /tmp data loss amendment — scope reduced to 4 units, before any final judge call

Status: frozen before the final judge pass runs. A PC restart cleared
`/tmp` and destroyed every unrecovered generation artifact for this
experiment. Operator-directed Plan B: recover what is recoverable, mark
the rest `artifact_lost`, do not regenerate. Full recovery procedure and
verification: `diagnostics/final-judge-pool/RECOVERY.md`. This amendment
records the effect on scope and adds `artifact_lost` to the taxonomy;
it does not change any generation or judge execution condition.

## New classification: `artifact_lost`

Distinct from every classification already in `execution.failure_taxonomy`.
Every existing class describes something that happened **during
generation or judging** (a model behavior, an upstream failure, a
schema mismatch). `artifact_lost` describes neither — it is applied to
a unit whose generation trial already completed and was already recorded
as `valid`, where the **artifact preserving that valid result** was
subsequently destroyed by an operational gap unrelated to generation,
judging, or the model at all: benchmark outputs were left in `/tmp`
pending selective, report-driven copying into the repository instead of
being copied immediately and unconditionally after every trial. This is
recorded plainly as an operator-side procedural failure, not a
generation, judging, or ReviewGraphen defect, and not folded into any
existing upstream-failure or model-failure category, because doing so
would misattribute the cause.

`artifact_lost` does not affect the trial's original outcome — the
trial itself is still `valid` in every record that mentions it — it
affects only whether that outcome's evidence can be pooled for judging.
A unit marked `artifact_lost` is excluded from the judge pool exactly
like `upstream_blocked` or `empty_pool`, but for a different, explicitly
distinct reason.

## Affected units

`head-local-03`, `head-local-05`, `head-local-06` — all three
`qwen_skill`-only (no `claude_skill` coverage), all three `valid`
generation trials whose sole surviving evidence (their `candidate.json`)
was destroyed. 5 findings total (1 + 1 + 3) are unrecoverable. Full
detail: `diagnostics/final-judge-pool/RECOVERY.md`.

`head-local-00`, `head-local-01`, `head-local-02` (`qwen_skill` only —
`claude_skill`'s copies were already repo-committed and survived) were
also destroyed in the same event but **are** recoverable: their findings
survive byte-for-byte inside `diagnostics/interim-judge-head-local-00-01-02/`,
the exploratory interim judge pass's own committed input, verified two
ways (hash match against that pass's own recorded input hash;
`finding_id` self-consistency recomputed fresh) before being reused here.
`head-local-04`'s `qwen_skill` candidate and all three `claude_skill`
candidates were already repo-committed before the loss and are
unaffected.

## Systematic bias, stated explicitly (not just "smaller n")

The three lost units (1, 1, 3 findings) are the three lowest non-zero
`qwen_skill` finding counts among the original 8 measured units (only
`head-local-07`'s 0 is lower, and it was already excluded as
`empty_pool` before this loss, for an unrelated reason). **The
retained `qwen_skill` sample (`00:4, 01:5, 02:4, 04:2` = 15 of the
original 20 findings, 75%) is therefore skewed toward units where
`qwen_skill` found more, not a random subsample.** Any rate or yield
computed from the 4 retained units describes `qwen_skill`'s denser
units specifically; it is not representative of its full original
8-unit spread, and this report does not present it as if it were.

## Effect on scope

`preregistration.json` `execution.judge_calls` said "at most 9 (one per
unit)." **The actual final judge pass covers 4 distinct units**
(`head-local-00`, `01`, `02`, `04`) — not 9, and not the 7 units that
had a nonempty pool immediately before this loss. Per-unit composition:

| unit | qwen_skill | claude_skill | pool | reason if excluded |
| --- | --- | --- | --- | --- |
| head-local-00 | 4 (recovered) | 9 | 13 | — |
| head-local-01 | 5 (recovered) | 5 | 10 | — |
| head-local-02 | 4 (recovered) | 6 | 10 | — |
| head-local-03 | — | — | — | `artifact_lost` |
| head-local-04 | 2 | — | 2 | — |
| head-local-05 | — | — | — | `artifact_lost` |
| head-local-06 | — | — | — | `artifact_lost` |
| head-local-07 | — | — | — | `empty_pool` (unaffected by the loss) |
| head-local-08 | — | — | — | `upstream_blocked` (unaffected by the loss) |

**4 units judged, 35 pooled findings (15 `qwen_skill` + 20
`claude_skill`), out of the original 9-unit design.** This is a
limitation on the conclusions this experiment can draw, stated here
before any judge output exists, not discovered afterward: `qwen_skill`'s
per-arm metrics now rest on a 4-unit, upward-biased-toward-denser-units
sample rather than the intended 8 measured units; `claude_skill`'s
metrics are unaffected (its 3 units were never touched by the loss).
Cross-arm comparison (both arms present) is unaffected in the 3 units
where it was always going to exist (`00`, `01`, `02`) — the loss removes
`qwen_skill`-only units, not any cross-arm observation.

## Operational change, recorded here and in RECOVERY.md

Every future generation trial's artifacts are copied into the
repository's `diagnostics/` tree immediately upon completion, not left
in `/tmp` pending selective later use. Applied starting with this pass's
own judge outputs (`scripts/run_final_judge_pool.sh` now copies each
unit's result into the repo as soon as that unit's call returns).
