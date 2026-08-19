# Codex cross-validation amendment — prepared 2026-08-19, execution authorized separately

Status: **preparation only. Not executed.** Every condition below is
frozen before any codex judge call, per this experiment's own amendment
discipline (any change after seeing a result requires a dated amendment
before the next call — here, nothing has run yet, so this entire
document is the pre-registration). The operator's codex usage limit
resets 2026-08-20 12:33; execution happens after that, on the operator's
explicit instruction, not automatically.

## 1. Purpose

`CLAUDE_SKILL_ARM_AMENDMENT.md` recorded a known, unresolved risk: the
final judge pass (`FINAL_JUDGE_REPORT.md`) used a Claude judge
(`opus`/`high`) to disposition findings from both `qwen_skill` (a
different model family — cross-family, per ADR 0035's design) and
`claude_skill` (the same family and specific model/effort tier as the
judge — same-family, structurally stronger correlated-error risk). That
amendment explicitly deferred resolution to "a post-2026-08-20
GPT-family cross-validation pass." This document is that pass's
preparation.

**What this pass tests:** whether the existing, already-reported Claude
judge dispositions are robust to the judge's own model family — i.e.,
does a cross-family judge (relative to `claude_skill`, and also
cross-family relative to `qwen_skill`) broadly agree with Claude's
dispositions on the identical 35 findings? **What this pass does not
do:** replace, average with, or otherwise blend into the existing
primary result. `FINAL_JUDGE_REPORT.md`'s numbers stand as the
experiment's primary judged result regardless of this pass's outcome —
see §5.

## 2. Scope: identical pool, identical everything except the judge backend

- **Units:** the same 4 (`head-local-00`, `01`, `02`, `04`) — not 7. The
  operator's own message describing this preparation said "7 unit";
  checked directly against `diagnostics/final-judge-pool/POOL_SOURCE_MANIFEST.json`
  and the actual committed judge results
  (`diagnostics/final-judge-pool/results/`): there are 4 distinct units
  with a judge call, matching `FINAL_JUDGE_REPORT.md`. The finding total
  the operator cited (qwen 15 + claude 20 = 35) is correct; the unit
  count is not. Recorded here rather than silently corrected, per this
  experiment's established discipline.
- **Findings:** the identical 35, identical `finding_id` values (the
  packets are rebuilt byte-for-byte from the same manifest via
  `scripts/build_final_judge_pool.py` — no regeneration, no
  re-derivation).
- **Order:** identical hash-sort per unit (unchanged from
  `JUDGE_PROTOCOL.md` §2).
- **Prompt:** the identical frozen `00-instructions.md`, extracted
  programmatically from `JUDGE_PROTOCOL.md` §5 by the same code path
  used for the Claude pass — byte-identical, not retyped.
- **Sources:** the identical reconstructed source trees
  (`scripts/recover_unit_sources.py`, hash-verified against
  `units.json`).
- **What changes:** only the judge backend (`codex` instead of
  `claude`) and the model/effort parameters appropriate to that backend
  (§4).

## 3. Blinding — extended, verified, symmetric

`scripts/scan_forbidden_markers.py` now includes `codex`, `gpt`, and
`openai` as case-insensitive self-reference markers, alongside the
existing `claude`/`opus`/`anthropic` set added for the `claude_skill`
arm — verified by test (`test_scan_forbidden_markers.py`, 27/27 cases
pass, including 7 new codex/gpt/openai-specific cases) and re-checked
against the real, already-built packets for all 4 units with zero false
positives. This closes the asymmetry the operator flagged: a
codex-family judge or generator now gets the same self-reference
protection the claude-family additions already had, not a narrower
version of it. `truth.json` remains outside `agent_input/` and is never
mounted into the judge sandbox, unchanged.

## 4. Execution parameters

- **Backend:** `reviewgraphen-benchmark run-process-reviewer-constrained
  codex` — the generic isolated Codex CLI backend (not the
  `-codex-profile` variant `qwen_skill` used, which points at the local
  LM Studio server; the generic backend with no profile points at the
  operator's own real Codex/OpenAI account, confirmed by reading
  `crates/reviewgraphen-reviewer/src/process.rs`: with `profile: None`,
  the command passes `--ignore-user-config` and sets the model
  explicitly via `-m`, then `--output-schema` for provider-constrained
  structured output — the same file-path-based mechanism `qwen_skill`
  used, not Claude's inline-JSON `--json-schema`, so no `$schema`/`$id`
  stripping is needed for this backend).
- **Model:** `gpt-5.6-sol` — the operator's own already-configured
  default in `~/.codex/config.toml`, used because it is the account's
  existing deliberate choice, not picked by this document. State a
  different model explicitly before running if a different one is
  wanted.
- **Effort:** `high` — chosen for parity with the Claude judge pass
  (`opus`/`high`), not codex's own config default (`medium`), so the
  comparison deliberately does not conflate a reasoning-effort
  difference with a model-family difference.
- **Isolation:** `--no-session-persistence`-equivalent is not a
  documented codex flag; isolation is provided the same way `qwen_skill`
  generation was isolated — `bwrap` sandbox, `--ephemeral`,
  `--skip-git-repo-check`, no tool access (`--dangerously-bypass-approvals-and-sandbox`
  combined with the disabled-feature list already used for every codex
  call in this program), a fresh, single-use `CODEX_HOME` per call.
- **Credentials:** a fresh mode-700 temp directory per call, containing
  only `~/.codex/auth.json` (not the full `~/.codex/` directory, which
  is 400+ MB of unrelated logs/cache/history) copied in immediately
  before the call and deleted immediately after — mirroring the
  established Claude judge credential discipline exactly.
- **No `--json-schema` stripping needed** (§4, backend note), but the
  schema's structural compatibility with `--output-schema` for this
  specific backend has not been tested end-to-end in this repository
  before. Per `JUDGE_PROTOCOL.md` §8's own discipline, **a synthetic
  smoke test with a trivial 1-file, 1-finding input is the required
  first step tomorrow, before any of the 4 real calls** — not skipped
  just because the packet format is already proven for Claude.

## 5. Disagreement-handling policy — decided now, before any result exists

This is the part the operator specifically flagged as consequential, so
it is stated precisely and will not be revised after seeing results.

**The Claude judge pass remains the experiment's sole primary result.**
`FINAL_JUDGE_REPORT.md`'s dispositions, quality distributions,
clustering, and `distinct_issue_worthy_count` are not recomputed,
averaged, or blended with anything from this pass. This pass produces a
**second, independent observation**, reported alongside the first, never
merged into it.

**No selective adoption.** Every one of the 35 findings gets both
judges' dispositions reported, unconditionally — agreements and
disagreements alike. **Adopting only the findings where both judges
agree, and treating the rest as "unresolved" or excluding them from any
count, is explicitly ruled out here as arbitrary** — an outcome could
otherwise be made to look more or less robust by choosing which
disagreements to discard. Nothing is discarded.

**The primary output of this comparison is the agreement rate itself,
computed three ways:**

1. **Overall**: the fraction of the 35 findings where both judges reach
   the same disposition bucket (`issue_should_be_created` /
   `should_not_be_created` / `unable_to_determine`).
2. **Per arm**: the same fraction computed separately for `qwen_skill`'s
   15 findings and `claude_skill`'s 20 findings. **This is the actual
   test of the same-family bias question** — if agreement is
   materially lower specifically for `claude_skill`'s findings than for
   `qwen_skill`'s, that is evidence consistent with same-family judge
   leniency (or, in the other direction, same-family severity); if
   agreement is similar across both arms, that is evidence against a
   material same-family effect in this instance. Neither direction is
   assumed before the numbers exist.
3. **Per direction of disagreement**: for every finding where the judges
   disagree, both dispositions and both judges' `quality.notes` are
   reported side by side, not summarized away. A pattern where Codex is
   specifically more skeptical of `claude_skill`'s findings (or the
   reverse) will be visible in this table even if the aggregate rate
   looks unremarkable.

**No new claim of ground truth.** Neither judge is authoritative (ADR
0035, unchanged). Where the two disagree, this document does not assert
which one is "right" — it reports that they disagree and lets the
per-arm breakdown speak to whether the disagreement pattern correlates
with same-family risk. Duplicate-clustering (`duplicate_of`) differences
between the two judges are reported the same way — side by side, not
merged into a single cluster count.

**What would change the primary result, and what would not.** A finding
of *no material per-arm difference in agreement rate* would be reported
as: the same-family risk disclosed in `CLAUDE_SKILL_ARM_AMENDMENT.md`
did not, in this instance, produce a detectably different disposition
pattern — stated that plainly, not as "the risk is resolved" (n=20 for
`claude_skill` is still small, and one cross-validation pass is one
data point). A finding of a *material* per-arm difference would be
reported as: the same-family risk appears to have affected
`claude_skill`'s dispositions in the direction observed — stated
plainly, and `FINAL_JUDGE_REPORT.md` would get a dated addendum pointing
to this finding, **not a rewrite of its recorded numbers**, per the
same append-only discipline used throughout this experiment.

## 6. Budget — estimate, not a hard cap

**No `--max-budget-usd`-equivalent flag exists for the Codex CLI**,
checked directly (`codex exec --help`, `codex --help`; no
budget/cost/spend/limit flag found anywhere). Unlike every Claude call
in this experiment, **there is no technical enforcement mechanism
available for this pass.** This is stated plainly rather than implying
a safeguard exists that does not.

**Token-scale estimate**, measured directly from the actual built
packets and the actual Claude judge outputs for the same content (not
guessed):

| Unit | Input packet bytes | Claude output bytes (same content, for scale) |
| --- | --- | --- |
| head-local-00 | 108,511 | 12,075 |
| head-local-01 | 99,145 | 9,461 |
| head-local-02 | 134,141 | 8,918 |
| head-local-04 | 89,317 | 2,528 |
| **Total** | **431,114** | **32,982** |

At the ~3.7 bytes/token heuristic used elsewhere in this experiment:
**~116,500 input tokens, ~8,900 output tokens** across the 4 real calls
combined, **as a floor** — this is Claude's own non-reasoning output
size for equivalent content, not a reasoning-inclusive figure. `gpt-5.6-sol`
at `effort: high` is a reasoning-capable model; its actual output token
count (reasoning + final text) for the same task is very likely
substantially higher than this floor, by an unknown multiplier — no
comparable reasoning-effort data exists yet in this program for this
specific model to estimate that multiplier from.

**No dollar estimate is given here.** Current per-token pricing for
`gpt-5.6-sol` was not verified by this document and is not guessed —
the operator's own account/billing page is the authoritative source for
that, and the operator asked to set the dollar cap themselves once given
this token-scale estimate. **Given the lack of a hard technical cap,**
the practical safeguard for tomorrow's run is procedural, not automatic:
the same two-strikes-and-stop discipline already used throughout this
experiment (§7), plus manually checking each call's actual reported
usage (if the CLI surfaces one) before proceeding to the next unit,
rather than launching all 4 unattended.

## 7. Execution steps for tomorrow (not run yet)

1. Confirm the operator's codex usage limit has actually reset (a
   minimal, non-benchmark request, matching the "confirm recovery before
   resuming" discipline already used for the LM Studio server in
   `RESTART_PROTOCOL_AND_UPSTREAM_HISTORY.md` §4).
2. Rebuild the pool fresh:
   `python3 benchmarks/m7-head-local-v1/scripts/recover_unit_sources.py head-local-00,head-local-01,head-local-02,head-local-04`
   then `python3 benchmarks/m7-head-local-v1/scripts/build_final_judge_pool.py <fresh-dir>`
   — deterministic, reproducible from git alone (§2).
3. **Required smoke test** (§4): a synthetic 1-file, 1-finding input
   through the same codex judge invocation shape, confirming
   `--output-schema` accepts the real schema file and the sandbox/auth
   setup actually works, before spending anything on the real 4 units.
4. Run the 4 real units sequentially, one at a time, checking each
   result before starting the next (no unattended batch, given §6's
   lack of a hard budget cap).
5. **Copy each unit's result into the repo immediately after that
   unit's call returns** — the operational rule adopted after the
   `/tmp` data loss (`diagnostics/final-judge-pool/RECOVERY.md`),
   applied here from the start rather than retrofitted.
6. Compute the three agreement statistics in §5 and write the
   comparison report — new document, does not edit
   `FINAL_JUDGE_REPORT.md`.
7. Report to the operator. No further judge calls, no retries on a
   failed unit beyond the two-strikes rule already governing every
   other part of this experiment.

## 8. Decision, 2026-08-19: not executed

The operator decided this pass will not be run. Everything above is
left exactly as prepared — nothing in sections 1-7 is deleted or
rewritten, so that this pass can be executed later, reproducibly from
git alone, if that decision changes.

**Reasoning, as given by the operator:** the primary arm of this
experiment, `qwen_skill`, is generated by qwen and judged by Claude —
already cross-family. The same-family bias this pass was designed to
test for does not exist on the primary arm in the first place; there is
nothing there for a codex cross-validation to check. The only place a
same-family relationship exists is `claude_skill` (generated by Claude,
judged by Claude) — a secondary, `n=3`-unit arm
(`CLAUDE_SKILL_ARM_AMENDMENT.md`) that never carried a superiority claim
over `qwen_skill` or anything else. Running this pass would have spent
real, uncapped money (§6) testing a bias that cannot affect the
experiment's primary result, to resolve a question about a secondary
arm whose own preregistration already disclaims comparative claims.

**What this leaves genuinely unresolved, stated plainly:** `claude_skill`'s
judged dispositions remain same-family (`CLAUDE_SKILL_ARM_AMENDMENT.md`'s
"Known risk" section, unchanged and still accurate) — the direction and
magnitude of any resulting bias were never measured, by this pass or
any other. **No claim that `claude_skill` outperforms `qwen_skill` can
be made from this experiment**, and none was ever made — see
`FINAL_JUDGE_REPORT.md`'s own "What this report does not claim" section
and its explicit statement that the gap between `claude_skill`'s and
`qwen_skill`'s numbers "is at least as plausibly a same-family-judge
leniency effect as a genuine capability difference." That limitation
stands exactly as already written; this decision does not newly create
it, only leaves it unresolved rather than testing it.

**What remains usable, exactly as built:** `scripts/run_codex_cross_validation.sh`,
`scripts/analyze_codex_cross_validation.py`, the `codex`/`gpt`/`openai`
forbidden-marker additions (`scripts/scan_forbidden_markers.py`, tested,
27/27), and `preregistration.json`'s `judge.codex_cross_validation`
field are all preserved. If this pass is ever run, sections 1-7 above
are still the correct preparation — only this section changes, with a
new dated note recording when and why the decision was reversed.
