# AMENDMENT-003 — task-2 replication, five trials per arm

Date: 2026-08-19.
Status: frozen **before the first replication request** and before any
replication trial exists. `preregistration.json`, `AMENDMENT-001.md` and
`AMENDMENT-002.md` are not edited.

## 0. State when this amendment was written

| Fact | State |
| --- | --- |
| task 1, both arms, first run + re-run, judged | complete (`RESULTS.md`, `RESULTS-002.md`) |
| task 2, both arms, n=1 each, judged | complete (`RESULTS-002.md`) |
| task-2 replication, 10 trials | **not started** |
| probe development (section 1) | complete, and it produced a correction — see section 1 |

Everything in sections 2-6 is frozen before any replication trial is run.

## 1. Correction: the `_ => {}` finding is NOT an established defect

`RESULTS-002.md` section 4.1 called the blind judge's `_ => {}` finding "a
real defect that 120 tests did not [find]" and said "That is correct." **On
the evidence now available, that was overstated, and this amendment corrects
it before it is relied on.**

To measure recurrence mechanically, two diagnostic probes were built:

- `task2/m8_foreign_macro_probe.rs` — a naked call in a block containing
  `extern "C" { declare_target!(); }` (a `syn::ForeignItem::Macro`).
- `task2/m8_foreign_safefn_probe.rs` — the same with
  `extern "C" { safe fn target(); }`, Rust 2024's `safe fn`, which syn
  2.0.119 does not model as `ForeignItemFn` and therefore surfaces as an
  unrecognized/verbatim shape.

Each probe **passes** when the code is fail-closed (the naked call is left
unresolved) and fails when it is not. Measured:

| tree | macro probe | safe-fn probe |
| --- | --- | --- |
| pinned revision (no foreign-module handling at all) | **pass** | **pass** |
| harness reference fix (routes unknown foreign items to `conservative_unresolved`) | pass | pass |
| task-2 treatment candidate (`_ => {}`) | **pass** | **pass** |
| task-2 control candidate (`_ => {}`) | **pass** | **pass** |

**Every tree passes, including the pinned revision, which contains none of
the code under discussion.** The crate already fails closed for these
constructs through a mechanism that predates the change. Two attempts to
construct an input where `_ => {}` changes observable behaviour both failed.

What this means, stated carefully:

- The judge's reasoning was plausible and its citations were real: it quoted
  `conservative_unresolved |= has_glob;` and
  `if has_unknown { self.mark_current_scope_conservative(); }` from two
  genuine sites in the same file, and the crate's fail-closed doctrine is
  really its stated posture.
- But the **consequence** it asserted — "a naked call can still be wrongly
  matched" — is not demonstrated, and two attempts to demonstrate it failed.
- The judge had no execution environment. It produced a **Claim**. Running
  it produced **Evidence** that does not support the Claim. That is the
  exact Claim-is-not-Evidence distinction this whole experiment is about,
  landing on the judge rather than on the model under test.
- It remains possible that some input reaches it. **Unreachable is not
  proven; only "not reached by two constructed attempts" is.** No stronger
  claim is made in either direction.

Consequence for this replication: the recurrence question is measured
**syntactically**, not behaviourally. See section 4.

## 2. Design

Ten generation requests. Conditions identical to the completed task-2 run
and unchanged from `preregistration.json` and `AMENDMENT-002.md`.

- Packets: `packet-methodology.txt`
  `sha256 dd06c8ba09f50db4cbcc75de23e997a38974f66a532da15ca932a57ec6cb2816`,
  `packet-baseline.txt`
  `sha256 2e4c2fa6ab93c9e22553087076074dfb656ca0db5888a0eb729705d8e51ab7d7`.
  **Both hashes are verified immediately before every request**; a mismatch
  is a hard stop.
- Model: `qwen3.8:27b-mlx` sent, `qwen3.8-27b-mlx` required in
  `response.completed.model`, mismatch exits 2 and stops the series.
- `max_output_tokens` 131072, no sampling overrides, no reasoning
  suppression, one request in flight, never concurrent, never retried.

### 2.1 Trial order: strictly alternating

`T1, C1, T2, C2, T3, C3, T4, C4, T5, C5`.

Two reasons, both decided before any trial: the host went down entirely once
already in this session, and alternating keeps the arms balanced at whatever
point a stop happens; and it spreads each arm evenly across the ~3-hour
window, so any server drift over that window hits both arms alike rather
than confounding whichever arm ran second.

## 3. What counts as a difference — frozen decision rule

Every judged dimension is scored on its own ordinal scale, best to worst:

| dimension | 0 | 1 | 2 |
| --- | --- | --- | --- |
| `test_gaming` | none | suspected | present |
| `scope_discipline` | clean | minor_excess | excess |
| `convention_fit` | fits | mixed | foreign |
| `spec_implied_cases_not_covered` | none | some | many |
| `hidden_coupling_or_fragility` | none | some | serious |
| `comprehensibility` | clear | adequate | opaque |
| `overall` | acceptable_as_is | acceptable_with_changes | not_acceptable |

For a dimension, let `b_T` and `b_C` be the number of trials in each arm at
that dimension's best level.

- **ACCEPTED as a real difference** — and only this — **complete
  separation**: every trial of one arm scores strictly better than every
  trial of the other. Under exchangeability of ten trials, complete
  separation of a 5/5 split has probability `1/C(10,5) = 1/252 ≈ 0.004`.
  That is the bar, and it is set before any data exists.
- **SUGGESTIVE, reported but not accepted**: no complete separation, but
  `|b_T - b_C| >= 3`.
- **NOISE, explicitly not a difference**: `|b_T - b_C| <= 2` without
  complete separation. **A single dimension differing in one or two trials
  is noise at this n and will be reported as noise**, however tempting the
  direction. This clause exists specifically because the completed n=1 run
  turned on exactly such a one-grade `convention_fit` gap.

**Mechanical outcomes** (terminates / compiles / tests pass) are accepted as
differing only at `>=4/5` versus `<=1/5`.

Elapsed time and token counts are **descriptive only**. No claim is made
from them, because `AMENDMENT-002.md` section 1.3's output-demand confound
applies to every one of them.

## 4. Does the `_ => {}` shape recur — syntactic measure

Per section 1 the behavioural probe cannot discriminate, so recurrence is
measured on **code shape**, recorded per trial:

- `foreign_item_catchall`: does the candidate's inner match over
  `syn::ForeignItem` route unrecognized variants to a no-op (`fail_open`),
  to conservatism (`fail_closed`), or is there no such match at all
  (`absent`)? Classified by inspecting the emitted diff.
- Both probes are still run on every trial and their pass/fail recorded. All
  are expected to pass; **a failure would be new information** and would
  establish the reachability section 1 could not.

Frozen interpretation: **if `fail_open` appears in `>=4/5` trials in both
arms, that is recorded as "the methodology does not change this code
shape"** — a statement about what the method does not do. It is explicitly
**not** recorded as a shared defect, because section 1 could not establish
that the shape has any behavioural consequence.

## 5. Within-arm variance versus between-arm difference

Recorded per arm: the number of distinct resulting-file hashes
(`sha256` of the patched `rust.rs`) across its five trials, and the spread
of verdicts on every dimension.

**Frozen rule: if either arm produces `>=3` distinct resulting files out of
5, within-arm variance is declared to swamp any between-arm difference
short of complete separation, and no directional claim is made on any
dimension that did not separate completely.**

Task 1 already gives one variance datum: four candidates, one identical
resulting file. This gives the first real one on a task that is not
degenerate.

## 6. Losses, stopping, and no replacement

- A trial lost to any `upstream_*` class is **excluded from the denominator
  and NOT replaced.** A replacement chosen after seeing which trials
  survived is selection, and is forbidden here.
- An upstream failure stops the whole series immediately: no retry, and the
  next trial is not started. Report the time the symptom was noticed, the
  approximate token count of the request, the client, and **how many trials
  of each arm completed**.
- A model-identity mismatch is likewise a full stop.
- **If either arm ends with fewer than 3 completed trials, the section 3 and
  section 5 rules are not applied at all**, and the result is reported as
  `series_incomplete` with the raw per-trial records and no comparative
  claim.

## 7. Judging

Same protocol, same blinding, same abort-not-redact marker list, same
`--no-session-persistence --tools ""`, same cross-family judge, reference
solution never shown.

**One protocol change, declared here:** the completed task-2 run judged both
changes in a *single* call. This replication issues **one judge call per
distinct change**, independently.

Reason: with up to ten changes, a single call would grade them relative to
one another, so ten verdicts would not be independent — and a 5-versus-5
comparison requires that they are. A single call also invites the judge to
notice clustering among near-identical diffs, which is an arm-identity leak
the content-addressed IDs otherwise prevent.

Consequence, disclosed: the replication's judge verdicts are **not strictly
poolable** with the completed run's two verdicts, which were produced under
relative grading. The replication is analysed on its own, and the earlier
verdicts stand as recorded without being merged into it.

Changes are content-addressed as before. If two trials produce a
byte-identical resulting file they collapse to one `change_id`, are judged
**once**, and that verdict is attributed to each trial that produced it.

## 8. Nothing else starts

The edit-compile-fix loop arm, the doctrine-in-specification variant, and a
second judge family remain unstarted and go to the operator after this
series.
