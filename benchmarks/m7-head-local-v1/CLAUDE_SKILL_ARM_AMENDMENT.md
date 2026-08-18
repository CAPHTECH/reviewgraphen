# claude_skill arm amendment

Status: frozen before any `claude_skill` generation call. Operator-directed
addition, run in parallel with the ongoing `qwen_skill` 9-unit generation
(which this amendment does not pause, alter, or otherwise touch). Amends
`preregistration.json`; does not remove or rewrite anything already frozen
there, including `QWEN_SKILL_ARM_AMENDMENT.md`.

## What this arm is, and is not

`claude_skill` is the identical scaffold as `qwen_skill` — the frozen
`.claude/skills/reviewgraphen-methodology/SKILL.md` body, verbatim, as the
packet's instruction text — executed by a different generator model
(`claude opus`, `effort: high`) instead of `qwen3.8:27b-mlx`, on 3 of the
9 units instead of all 9.

It is **not** a replacement for or an update to `qwen_skill`. It is not a
larger arm added to the same generator; it is a second, independent
generator run through the same frozen methodology, so that this
experiment can eventually say something about the methodology itself
somewhat independent of which model executes it — subject to the
same-family judge risk recorded below, which is not eliminated by this
arm's addition.

## Scope: 3 units, not 9

`head-local-00`, `head-local-01`, `head-local-02` only. This is narrower
than `qwen_skill`'s n=9 and is recorded as an asymmetry, not silently
generalized. Rationale: these are the same 3 units already carried
through the interim mechanical (`diagnostics/interim-mechanical-verification/`)
and interim judge (`diagnostics/interim-judge-head-local-00-01-02/`)
passes, so `claude_skill`'s findings on these units land in a pool that
already has independently-verified location data and an (interim,
qwen-only) judge disposition to compare against — without requiring a
fourth analysis pass invented solely for this arm. Extending
`claude_skill` to the remaining 6 units is not ruled out but is not
committed to by this amendment; if it happens, it is a separate,
dated addition.

## Packets: byte-identical to qwen_skill's, reused not rebuilt

`claude_skill` uses the exact same packet directories `qwen_skill` used
for these 3 units
(`/tmp/m7-head-local-v1-skill-prepared/head-local-{00,01,02}/agent_input/`),
verified immediately before this amendment was written:

| unit | `instruction.txt` SHA-256 |
| --- | --- |
| head-local-00 | `a43d6984f9b58d5e47256afed68cddc1598ae84d6fc564e05f1cf462228d670a` |
| head-local-01 | `a43d6984f9b58d5e47256afed68cddc1598ae84d6fc564e05f1cf462228d670a` |
| head-local-02 | `a43d6984f9b58d5e47256afed68cddc1598ae84d6fc564e05f1cf462228d670a` |

All three match the frozen skill body hash recorded at commit `a092c32`
and repeated in `QWEN_SKILL_ARM_AMENDMENT.md`. `git log -1` on
`.claude/skills/reviewgraphen-methodology/SKILL.md` still shows `a092c32`
as the last commit touching that file — it has not been altered since
freezing. Each unit's `agent_input/` also carries the same
`candidate-output.schema.json` and `source/` tree `qwen_skill` was given;
no new packet-construction code is needed, and none is added.

## Generation execution condition

- **Backend:** `reviewgraphen-benchmark run-process-reviewer-constrained claude`
  (the same generic isolated Claude CLI backend the judge uses, distinct
  from the codex-profile adapter `qwen_skill` uses — `claude_skill` was
  never going to reuse the LM Studio/codex-profile path).
- **Model:** `opus`. **Effort:** `high`.
- **Isolation:** `--no-session-persistence`, `--tools ""` — unconditional
  on `ProcessReviewerBackend::ClaudeCli` regardless of budget flag,
  confirmed by reading `crates/reviewgraphen-reviewer/src/process.rs`'s
  command-construction match arms before this amendment was written; not
  a new guarantee, the same one the judge backend already relies on.
- **Provider-constrained:** true (`--json-schema` against
  `candidate-output.schema.json`, same schema as every other arm — no
  schema change).
- **No session persistence, no prior turns, no repository tool access** —
  the model sees only the packet's `agent_input/` contents, exactly as
  `qwen_skill`'s generator did (modulo the different underlying CLI/model).
- **Obligation cap:** unchanged from the skill body itself — at most 8
  self-enumerated obligations per unit, the same experiment-side cap
  `QWEN_SKILL_ARM_AMENDMENT.md` recorded for `qwen_skill` (it is part of
  the frozen instruction text both arms receive, not a per-arm setting).
- **Retries:** 0. **Concurrency:** each of the 3 calls issued sequentially
  by the operator, but this arm's calls may interleave in wall-clock time
  with the still-running `qwen_skill` generation (different backend,
  different endpoint — LM Studio vs the Claude CLI — so no shared
  resource contention is introduced by running them concurrently).

## Budget: separate from, and does not draw down, the $30 judge cap

`AUTH_AND_BUDGET_AMENDMENT.md`'s `--max-budget-usd 30` is scoped to judge
calls only (`run-process-reviewer-constrained claude` invoked as the
*judge*, per that amendment's own text: "set on every real judge
invocation"). `claude_skill` calls are *generation*, not judging, and use
a new, independently-tracked cap: **`--max-budget-usd` totaling $8 across
this arm's 3 generation calls.**

Because `--no-session-persistence` means each call is a fresh process
with no cross-call state, the CLI's own budget flag can only enforce a
**per-invocation** ceiling — it cannot itself track cumulative spend
across 3 separate process invocations (the same reasoning already applied
in `diagnostics/interim-judge-head-local-00-01-02/INTERIM_JUDGE_REPORT.md`
when the interim judge pass used `--max-budget-usd 3` per call against
the $30 *total*, rather than passing $30 on every call). Applying the
same discipline here: $8 total ÷ 3 calls = $2.666..., so **`--max-budget-usd 2.66`
is set on each of the 3 calls**, giving a worst-case total exposure of
3 × $2.66 = $7.98, under the $8 total. If any one call's actual spend is
well under $2.66 (expected, based on the interim judge calls' behavior —
none of those approached their $3 ceiling), the realized total will be
lower still; this is not adjusted upward mid-run to "use up" the
remainder.

**Handling budget exhaustion**, decided before any real call, matching
`AUTH_AND_BUDGET_AMENDMENT.md`'s existing policy for the judge budget: if
a `claude_skill` generation call is refused or fails because it would
exceed its $2.66 per-call ceiling, that unit is recorded as
`generation_incomplete` (a new outcome, analogous to `judge_incomplete`)
and excluded from `claude_skill`'s metrics for that unit; no retry is
issued; the other units already completed keep their results unchanged;
this arm's addition does not touch, pause, or draw from the $30 judge
budget in any way.

**Both remaining balances are tracked and reported independently** after
this arm's 3 calls: the $30 judge cap's remaining balance (currently
tracked in `INTERIM_JUDGE_REPORT.md`'s Spend section — at least $21 of
$30 remaining, by that report's conservative accounting) is untouched by
this amendment; the new $8 generation cap's remaining balance is reported
fresh after the 3 `claude_skill` calls complete, with the same
byte-based-estimate-not-billed-figure honesty the interim judge report
already applied, since `--output-format text` still does not surface an
exact cost/usage field for this CLI path.

## Known risk: same-family judge bias, recorded but not preempted

Per ADR 0035's design philosophy (cross-family generator/judge pairing to
reduce correlated error), `qwen_skill`'s judge (Claude `opus`/`high`) is a
**different model family** from its generator (`qwen3.8:27b-mlx`).
`claude_skill`'s judge is **the same family, and in fact the same
specific model and effort tier** as its own generator — both are
`claude opus` at `effort: high`. This is a strictly stronger
same-provenance overlap than a generic "same vendor" note would capture.

**This amendment makes no claim about the magnitude or direction of any
resulting bias.** It is plausible that a same-family judge is more
lenient toward same-family generation (shared blind spots, shared
stylistic priors read as more "reasonable"), or more critical (sharper
ability to spot a same-family generator's characteristic overclaiming),
or that the effect is negligible at this judge effort tier — none of
these is asserted. What is recorded is only that the risk exists and is
structurally different from, and larger in principle than, the risk
already disclosed for `qwen_skill` in `preregistration.json`
`known_limitations`.

**This risk is not preempted by this amendment** — not by using a
different judge model for `claude_skill`'s findings, not by adding a
disclosure field to the judge output schema, not by any other mitigation
applied before generation. The blind judge protocol
(`JUDGE_PROTOCOL.md`) runs completely unchanged: the judge still never
learns which arm or generator produced a given pooled finding (blinding
is symmetric across arms, including this one), and `truth.json` still
records `claude_skill` as a contributing arm exactly like any other,
never revealed to the judge sandbox.

**Planned, not-yet-executed mitigation:** a GPT-family cross-validation
pass, to be run after 2026-08-20 (the date the operator's Codex rate
limit resets, per `QWEN_SKILL_ARM_AMENDMENT.md`'s "Extensibility for a
later codex+skill comparison" section). That pass would judge the same
pooled finding set (or a `codex_skill`-extended version of it) with a
non-Claude judge, to see whether `claude_skill`'s disposition tally
changes under a judge with no shared provenance. Until that pass exists,
`claude_skill`'s judged results are reported with this asymmetry stated
plainly alongside them — every table or count that includes
`claude_skill` findings carries this caveat, not buried in a single
limitations paragraph.

## Pooling: claude_skill joins qwen_skill's per-unit pool for the FINAL judge — not the completed interim pool

`claude_skill`'s 3 units' findings are added to the **same per-unit
pool** `qwen_skill` contributes to, for the **single primary blind judge
pass over all 9 units** that runs after `qwen_skill` generation
completes (`preregistration.json` `judge.interim_analysis_guardrail`
item 3). For `head-local-00`/`01`/`02` specifically, that final pool will
contain both arms' findings, hash-sorted together per
`JUDGE_PROTOCOL.md` section 2-4, exactly as `qwen_b1`+`qwen_full` or any
other two-arm unit would be pooled.

**This is explicitly a different pool from, and does not retroactively
change, the already-completed qwen-only interim judge pass**
(`diagnostics/interim-judge-head-local-00-01-02/`, 13 findings, all
`qwen_skill`-only, judged and reported before this amendment existed).
That interim result stands as recorded — exploratory, `qwen_skill`-only,
already labeled as not the primary result. Nothing in this amendment
edits, re-runs, or supersedes it. Any report that discusses both must
state which pool (interim qwen-only vs final all-arm) a given number
comes from; the two are never averaged or silently merged.

## Extraction, schema, disposition semantics: unchanged

`ADR 0037`'s extraction contract, `candidate-output.schema.json`, and the
skill's own "Output mapping" (self-assigned `packet_id`, claim-polarity
`disposition`, `findings` only for `issue_present`) all apply to
`claude_skill` exactly as they already do to `qwen_skill` — the packets
are byte-identical, so the model-facing contract cannot differ.

## Reported metrics: extended, not redefined

Every metric in `preregistration.json` `reported_metrics` that is
computed "per arm" now has a `claude_skill` row alongside `qwen_b1` /
`qwen_full` / `qwen_skill`, computed only from `claude_skill`'s
final-pool judge results (once that pass runs), scoped to its 3 units —
never extrapolated to the 6 units it did not run on. No new metric
definition is introduced by this amendment.

## Does not interfere with the ongoing qwen_skill generation

`claude_skill` calls use the Claude CLI backend against the operator's
Anthropic account; `qwen_skill` calls use the codex-profile adapter
against the LM Studio endpoint at `http://192.168.68.71:11999`. Different
processes, different backends, different network endpoints, no shared
mutable state (`--no-session-persistence` on the Claude side; the LM
Studio side is unaffected by anything this amendment does). The
already-running `qwen_skill` batch (currently past `head-local-04`,
proceeding through the remaining units) is not paused, redirected, or
otherwise touched to make room for this arm.
