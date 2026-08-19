# Final blind judge pass — results

Status: the primary result for m7-head-local-v1, per `preregistration.json`
`judge.interim_analysis_guardrail`. Covers 4 units
(`head-local-00`, `01`, `02`, `04`), not the originally designed 9 — see
`ARTIFACT_LOSS_AMENDMENT.md` for why, and read every number below with
that scope reduction in mind. Conditions were not changed after seeing
these results.

## Mechanical validation (before any interpretation)

All 4 judge calls: exit 0, `judgments[].finding_id` set exactly equals
the pool's `finding_id` set (no fewer, no extra, no duplicates), every
`duplicate_of` entry is a valid same-pool `finding_id` distinct from its
own judgment. No `judge_call_failed`.

## Disposition tally, raw, by arm

| Arm | issue_should_be_created | should_not_be_created | unable_to_determine | raw total |
| --- | --- | --- | --- | --- |
| qwen_skill | 11 | 4 | 0 | 15 |
| claude_skill | 19 | 1 | 0 | 20 |

Per-unit breakdown:

| unit | qwen_skill | claude_skill |
| --- | --- | --- |
| head-local-00 | issue:3, should_not:1 | issue:9 |
| head-local-01 | issue:5 | issue:4, should_not:1 |
| head-local-02 | issue:2, should_not:2 | issue:6 |
| head-local-04 | issue:1, should_not:1 | — (no claude_skill coverage) |

## Quality distribution, by arm

| Field | qwen_skill (n=15) | claude_skill (n=20) |
| --- | --- | --- |
| specificity: file_and_line_identified_and_relevant | 15/15 | 20/20 |
| unresolvable_location | 0/15 | 0/20 |
| reproduction_conditions_stated | 13/15 | 19/20 |
| false_positive_suspected | 3/15 | 1/20 |
| design_intent_confusion_suspected | 3/15 | 1/20 |

**Zero `unresolvable_location` across all 35 findings, both arms** — no
hallucinated location in this final pass either.

## Duplicate clustering — 35 raw findings → 30 distinct clusters

Union-find over the judge's own `duplicate_of` assertions (bidirectional
edges, per `JUDGE_PROTOCOL.md` section 7). **30 clusters from 35
findings: 5 pairs collapsed, all 5 cross-arm, zero within-arm
duplicates in either arm.**

### The 5 cross-arm duplicate pairs — independent corroboration

Every single collapse is `qwen_skill` and `claude_skill` independently
reporting the same underlying defect, worded differently, both rated
`issue_should_be_created`:

1. **head-local-00** (`db.rs`): both flag that the sanitizer `safe()`
   is not injective, so distinct `(table, column)` pairs can collide
   into the same generated catalog identity.
2. **head-local-00** (`compose.rs`): both flag that
   `rewrite_compose_statements`/`lower_compose` can panic
   (`.expect(...)`) on a reachable error path instead of returning the
   `CoreError` the API promises.
3. **head-local-01** (`model.rs`/`origin.rs`): both flag that
   `source_origin`'s hardcoded id prefix is reused for the
   annotation-validation error path, independent of the actual error
   kind.
4. **head-local-02** (`explicit.rs`): both flag that
   `init_write_key` (duplicate-write guard) and
   `assignment_coverage`/`Coverage` use different granularities for the
   same statement.
5. **head-local-02** (`server.rs`): both flag that the LSP's local
   `KEYWORDS` completion list and the rename path's reserved-word check
   (`crate::index::is_keyword`) are two unsynchronized definitions,
   citing the omission of `'use'` specifically.

**This is exactly the kind of observation the operator asked this
analysis to surface: two independently-generated, cross-family arms
converging on the same specific defect is mutual corroboration, not
noise.** All 5 are in the 3 units where both arms had coverage
(`head-local-00/01/02`) — `head-local-04` has no cross-arm overlap since
`claude_skill` never covered it.

**High-confidence clusters (per `JUDGE_PROTOCOL.md` section 7's bar —
`issue_should_be_created` AND every quality gate clear): 22 of 30.**

## Per-arm metrics (JUDGE_PROTOCOL.md section 7)

| Metric | qwen_skill | claude_skill |
| --- | --- | --- |
| raw_findings_count | 15 | 20 |
| distinct_count (clusters touched) | 15 | 20 |
| redundancy_ratio | 1.0 | 1.0 |
| distinct_issue_worthy_count | **9** | **17** |
| unresolvable_location_count / rate | 0/15 (0%) | 0/20 (0%) |

**redundancy_ratio = 1.0 for both arms**: neither arm repeated any of
its own claims across this reduced 4-unit sample — every raw finding
from each arm landed in its own distinct cluster (the only collapses
were cross-arm, not within an arm). This differs from `m7-real-v1`'s
much higher B1 redundancy (115 raw vs 58 distinct) — not comparable
directly, since this is a different arm design (`qwen_skill`/`claude_skill`
executing the same methodology, not raw-B1-vs-scaffolded), but notable
that neither arm here shows the repetition pattern that motivated
distinct-counting in the first place.

## `usefulness_determination`'s three status fields, against real numbers

**`primary_report` (qwen_skill's absolute results, standing alone) —**
`distinct_issue_worthy_count`: 9 of 15 distinct clusters. Raw
`issue_should_be_created`: 11/15 (73.3%). Distinct-cluster
`issue_should_be_created`-containing clusters: 9/15 (60%,
i.e. `distinct_issue_worthy_count`/`distinct_count`, the high-confidence
subset). Generation completion, **read with `ARTIFACT_LOSS_AMENDMENT.md`'s
scope reduction**: of the 8 originally measured units (excluding
`head-local-08`'s `upstream_blocked`), 4 (`00/01/02/04`) have surviving,
judged evidence; 3 (`03/05/06`, all `valid` at generation time) are
`artifact_lost`; 1 (`07`) is `empty_pool`. **This primary report
therefore describes qwen_skill's 4 surviving, judged units — biased
toward its denser units, per the stated bias direction — not the full
original 8.**

**`b1_comparison_status` (auxiliary, asymmetric) —** unchanged by this
pass: `qwen_b1` remains n=1 (`head-local-00` only), failed
(`empty_final_after_process_completion`), no further trials planned.
No superiority claim made here either.

**`full_comparison_status` (not applicable) —** unchanged: `qwen_full`
has zero completed generation trials; its role remains the
capability-gap diagnosis only, not a data point in this table.

**New in this pass, not originally in the preregistered three fields but
directly relevant:** `claude_skill`'s own absolute results — 17 of 20
distinct clusters are high-confidence (85%), raw `issue_should_be_created`
19/20 (95%). `claude_skill` is not the primary arm of this experiment
(that remains `qwen_skill`, per `QWEN_SKILL_ARM_AMENDMENT.md`), and its
same-family judge-bias risk (`CLAUDE_SKILL_ARM_AMENDMENT.md`) is not
resolved by this pass — no claim of `claude_skill`'s superiority over
`qwen_skill` is made from these numbers; the gap between 95%/85% and
73%/60% is at least as plausibly a same-family-judge leniency effect as
a genuine capability difference, and this report does not adjudicate
between those explanations.

## Spend

Per-call ceilings: head-local-00 $4.00, head-local-01 $3.50,
head-local-02 $3.50, head-local-04 $1.50 — **worst-case exposure $12.50**.
All 4 calls completed normally (exit 0, full valid output), which would
not happen if a ceiling had been hit mid-response — actual spend is
below ceiling for all 4, consistent with every previous call in this
program. No exact billed figure is available (`--output-format text`
surfaces no cost field; the ephemeral credential directory is deleted
immediately after each call, per the mandatory security procedure).

**Remaining budget floor:** pre-pass floor was "at least $21 of $30"
(`INTERIM_JUDGE_REPORT.md`). Treating this pass's spend conservatively
at its full $12.50 ceiling: **at least $8.50 of the original $30 cap
remains.** Actual remaining balance is very likely higher, for the same
reason actual spend is likely below ceiling; the account's own usage
dashboard is authoritative for an exact figure.

## What this report does not claim

No statistical significance. No verified-defect count (judge disposition
is non-authoritative per ADR 0035 — no upstream issue, branch, commit, or
PR was created from any of this). No generalization beyond the 4 judged
units, which are themselves a biased subsample of the original 8 (see
`ARTIFACT_LOSS_AMENDMENT.md`). No resolution of the `claude_skill`
same-family judge-bias question (planned, not-yet-executed: a
post-2026-08-20 GPT-family cross-validation pass).

**Addendum, 2026-08-19:** the operator decided not to run the planned
cross-validation pass named above — see
`../../CODEX_CROSS_VALIDATION_AMENDMENT.md` §8 for the decision, its
reasoning, and what remains unresolved as a result. In short: the
primary arm (`qwen_skill`) is already cross-family judged, so the pass
would not have tested anything about this report's primary result;
`claude_skill`'s same-family judge-bias direction and magnitude remain
unmeasured, unchanged from what this report already states above.
