# m8-impl-local-v1 — task-2 replication, 5 trials per arm

Supplements `RESULTS.md` and `RESULTS-002.md`. Design frozen in
`AMENDMENT-003.md` before the first request; `AMENDMENT-004.md` and
`AMENDMENT-005.md` record two harness defects found during the series and
the uniform rules applied in response.

**Series completed: 10 of 10 trials, zero upstream failures, zero
retries, one START per cell.** Every trial created a message item,
finished `status: completed`, and was generated against a server-confirmed
`qwen3.8-27b-mlx` with both packet hashes verified immediately before each
request.

## 0. Three things asked for, answered first

### 0.1 `PROBE ... =fail` means the probe DETECTED the condition

First reading. `pass` = `cargo test` exit 0 = the probe's assertion held.
`fail` = non-zero = the assertion was violated, or the tree did not build.
The probe asserts the *desired* fail-closed behaviour, so `fail` on a tree
that builds means the naked call **was** resolved — the fail-open condition
is present.

Confirmed on `rep-baseline-3`, which compiled and passed all 120 tests:
an assertion panic at `m8_foreign_macro_probe.rs:174`, not a build failure.
The polarity is not inverted anywhere.

**Consequently `AMENDMENT-003.md` section 1 is withdrawn and the judge's
`_ => {}` finding stands.** My earlier refutation of it was wrong. Root
cause, demonstrated deliberately in `AMENDMENT-005.md`: every scratch tree
builds a package named `reviewgraphen-ingest` `0.1.0`, all of them shared
one `CARGO_TARGET_DIR`, and cargo served one tree's compiled library to
another. The pinned tree measured `FAILED, FAILED, FAILED`, then measured
`ok, ok, ok` immediately after a different tree was built into the same
target dir, with no change to its source.

Re-measured with a private target dir per tree, three runs each, all
unanimous:

| tree | macro probe | safe-fn probe |
| --- | --- | --- |
| pinned revision | fail | fail |
| harness reference fix (`_ => conservative_unresolved = true`) | **pass** | **pass** |
| all 9 replication candidates | fail | fail |
| completed n=1 task-2 treatment and control | fail | fail |

The probe discriminates cleanly. Routing unrecognized `syn::ForeignItem`
variants to a no-op does leave a naked call matched to a module-level
function. **The blind judge, with no execution environment, was right; I
had a compiler, ran it once, and was wrong.**

### 0.2 All 10 trials are in the repository

`runs/replication/` holds every trial: generation metrics, request body,
final content, candidate, applied diff, patched file, advertised model
list, probe logs, both verifications, and the gzipped raw SSE stream with
its SHA-256. Plus `SERIES.log`, the 9 judge results, the truth mapping, and
both analysis outputs. 206 files, committed incrementally as each trial
completed, not batched.

### 0.3 The in-series tally the coordinator read has moved by one trial

The coordinator's table (methodology 1/5) is the **in-series** tally.
`rep-methodology-1`'s in-series failure was
`git::cargo_admission_tests::a_valid_trusted_executable_is_admitted_...`
failing with `Text file busy (os error 26)` — a test that copies the
admitted `cargo` binary and execs the copy, in `git.rs`, unreachable from
any change to `rust.rs`. `AMENDMENT-004.md` preregistered uniform
post-series re-verification **before nine of the ten trials had been
verified**, and it was applied to all ten identically. On re-verification
that trial passes.

**Authoritative tally: methodology 2/5, control 4/5.** The direction is
unchanged and this does not soften it.

## 1. Frozen criteria first, before any interpretation

| axis | methodology | control | frozen rule | verdict |
| --- | ---: | ---: | --- | --- |
| `compiles` | 2/5 | 4/5 | accepted only at >=4/5 vs <=1/5 | **not accepted** |
| `tests_pass` | 2/5 | 4/5 | accepted only at >=4/5 vs <=1/5 | **not accepted** |
| every judged dimension | — | — | accepted only on complete separation | **noise** |

Within-arm variance: **5 distinct resulting files in each arm, out of 5
trials.** No two trials in either arm produced the same file. Per
`AMENDMENT-003.md` section 5 this triggers the clause that within-arm
variance swamps any between-arm difference short of complete separation, so
**no directional claim is made on any judged dimension.**

`test_gaming` is `none` in all 9 judged changes, both arms.

### 1.1 The frozen criteria do not cleanly cover this result — said plainly

The mechanical rule was written as `>=4/5 versus <=1/5`. The observed split
is **4 versus 2**, which fails that bar while being a large practical gap in
the direction opposite to the hypothesis. The rule was calibrated as a
demanding bar for claiming the methodology *helps*; applied symmetrically it
is equally demanding for claiming it *hurts*, and 4-vs-2 does not clear it.

I am not stretching it to fit. **By the frozen criteria, nothing is
accepted, in either direction.** What is reported below the line is
observation, explicitly not an accepted finding.

## 2. The failures, which are the substance

### 2.1 Generation records — the budget hypothesis is refuted

| trial | min | output tok | reasoning tok | final bytes | message item | post verdict |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| baseline-1 | 13.77 | 18,554 | 18,190 | 1,607 | yes | verified |
| baseline-2 | 9.57 | 14,557 | 14,169 | 1,852 | yes | verified |
| baseline-3 | 19.80 | 24,313 | 23,997 | 1,190 | yes | verified |
| baseline-4 | 14.35 | 21,562 | 21,134 | 1,923 | yes | verified |
| baseline-5 | 12.28 | 18,532 | 17,522 | 4,083 | yes | **edit_anchor_did_not_match** |
| methodology-1 | 18.37 | 24,396 | 21,305 | 12,909 | yes | verified |
| methodology-2 | 12.26 | 18,255 | 15,364 | 11,896 | yes | **does_not_compile** |
| methodology-3 | 9.27 | 13,905 | 10,948 | 12,063 | yes | **does_not_compile** |
| methodology-4 | 14.54 | 21,512 | 18,384 | 13,027 | yes | verified |
| methodology-5 | 8.18 | 12,297 | 9,499 | 11,816 | yes | **does_not_compile** |

**The hypothesis that the procedural surface consumed the budget before the
edit was finished is not supported.**

- Every trial in both arms produced a message item and finished
  `status: completed`. No truncation, no `incomplete_details`.
- The highest output in the whole series is 24,396 tokens against a cap of
  **131,072**. Nothing came within a factor of five of the ceiling.
- The three failing methodology trials used **fewer** tokens than the two
  passing ones, not more: reasoning 15,364 / 10,948 / 9,499 for the
  failures against 21,305 / 18,384 for the passes. Within that arm the two
  successes hold the two highest reasoning counts and the three failures the
  three lowest — a clean separation, though at n=5 and found after the fact.
- Final content is ~11.8–13.0 KB in all five methodology trials including
  the failures, so the full JSON envelope — obligations, evidence statuses,
  limitations and the edit — was emitted every time. Nothing was cut off
  mid-edit.

The failing runs did **less** work overall, not more. That is the opposite
shape from crowding-out.

### 2.2 What actually went wrong: `syn` API knowledge, in all three cases

| trial | compiler error |
| --- | --- |
| methodology-2 | `E0609: no field 'ident' on type '&ForeignItemFn'` — wrote `foreign_fn.ident`, needed `foreign_fn.sig.ident` |
| methodology-3 | `E0599: no variant or associated item named 'Const' found for enum 'ForeignItem'` — invented a variant |
| methodology-5 | `E0026: variant 'syn::ForeignItem::Fn' does not have a field named 'sig'` (and the same for `Static`/`ident`) — used struct-variant patterns for tuple variants |

All three are the same class: **the exact shape of a dependency's API that
was not in the packet.** The model was given all 2,059 lines of `rust.rs`
but no `syn` source. The correct idiom, which every control candidate used,
is `ForeignItem::Fn(f) => f.sig.ident` and `ForeignItem::Static(s) => s.ident`.

These are not wrong designs, misread constraints, or mis-anchored edits.
Every one of the three otherwise implements the specified rule, hoists the
right names, and would be correct with the field access fixed.

### 2.3 Is there a mechanism linking the methodology to those errors?

Checked, and **the obvious mechanism does not hold.** Only one of the three
failures (methodology-3, the invented `Const` variant) reflects broader
variant coverage. The other two reference exactly `Fn` and `Static`, the
same two every control candidate used, and differ only in field access.

| arm | trials referencing exactly `Fn`,`Static` | added code lines (median) |
| --- | --- | ---: |
| control | 4 of 4 with a diff | 12 |
| methodology | 4 of 5 | 13 |

So the two arms wrote the same-sized change against the same two variants,
and one arm got the field access right four times out of four while the
other got it right twice out of five.

**I have no mechanism the data supports.** One hypothesis consistent with it,
and untested: the treatment emits ~12 KB of final content of which the edit
is a small fraction, while the control emits ~1.7 KB that is almost entirely
the edit — so the edit competes for care with six obligation records. That
is a different claim from budget exhaustion, which section 2.1 refutes, and
it is offered as a hypothesis to test, not a conclusion. Testing it would
need an arm that runs the full procedure and then emits *only* the edit.

### 2.4 `tests_failed` treated separately

One occurrence, `rep-methodology-1`, and it was the harness race of section
0.3, not a code failure. On uniform post-series re-verification it passes,
with 67 unit + 8 + 40 `m2` + 5 acceptance tests green. It is flagged
`flaky_verification` in the record and is the only trial in the series whose
two verifications disagree.

### 2.5 The control's single failure counts against the control

`rep-baseline-5` emitted two edits; the first anchored on the `use syn::{…}`
import block and reproduced it inexactly — `old_occurrences: 0`.

Judged against the frozen rules rather than decided now: the packet's own
output contract, identical in both arms, states that `old` "must be an
**exact, byte-for-byte, contiguous excerpt** … If `old` does not match
exactly and uniquely, the edit is discarded and **the run counts as
producing no applicable change**." That is a candidate-side requirement the
model was told about, not a harness malfunction — the harness did exactly
what it was specified to do, and rejected loudly rather than silently
misapplying. `AMENDMENT-003.md` provides no exemption for it.

**So it counts as a failure of the control arm**, and the 4/5 figure already
includes it. Worth noting separately that another control candidate
(`rep-baseline-1`) edited the same import block successfully, so the task
did not force this failure.

## 3. Blind judgement, 9 distinct changes, one independent call each

| trial | test_gaming | scope | convention | spec gaps | coupling | comprehens. | overall |
| --- | --- | --- | --- | --- | --- | --- | --- |
| baseline-1 | none | clean | mixed | none | none | adequate | acceptable_with_changes |
| baseline-2 | none | clean | fits | none | none | clear | acceptable_as_is |
| baseline-3 | none | clean | mixed | none | none | adequate | acceptable_with_changes |
| baseline-4 | none | clean | mixed | some | some | clear | acceptable_as_is |
| methodology-1 | none | clean | fits | none | some | clear | acceptable_as_is |
| methodology-2 | none | clean | fits | none | none | clear | **acceptable_as_is** |
| methodology-3 | none | minor_excess | mixed | none | some | adequate | **acceptable_with_changes** |
| methodology-4 | none | clean | mixed | none | none | clear | acceptable_as_is |
| methodology-5 | none | clean | mixed | none | none | adequate | **not_acceptable** |

### 3.1 `judge_accept_mechanical_fail` — the judge accepted 2 of 3 non-compiling changes

`AMENDMENT-001.md` section 3.2 names this category in advance. Two
methodology candidates that **do not compile** were rated
`acceptable_as_is` and `acceptable_with_changes`. Only `methodology-5` was
caught, at `not_acceptable`.

This is a real limit on judge-only quality assessment, and it is the exact
mirror of section 0.1: there, a judge with no compiler was right and my
instrument was wrong; here, a judge with no compiler accepted code that does
not build. **Neither reading alone is sufficient.** A blind judge and a
mechanical gate fail in different directions, and this series produced one
clear instance of each.

## 4. Honest read

**By the frozen criteria: nothing accepted, in either direction.**

Below the line, as observation rather than accepted finding:

- The control passed 4 of 5 and the treatment 2 of 5. That is a reversal of
  the n=1 result, which had both arms passing with the treatment ahead on a
  single comment-formatting grade. The n=1 reading should not be relied on;
  this supersedes it as the better-powered observation, without meeting the
  bar for a claim.
- All three treatment failures are `syn` API errors of the same class, and
  every one would compile with a corrected field access. **No treatment
  failure was a wrong design or a misread specification.**
- Budget exhaustion is refuted, not merely unsupported: no run exceeded 19%
  of the output cap, all ten produced a message item, and the failing runs
  reasoned *less* than the passing ones.
- Within-arm variance is maximal — 10 trials, 9 distinct changes, no repeat
  in either arm. Any comparison at n=5 against that variance is weak by
  construction, and the frozen rule anticipated this.
- The methodology's evidence discipline held again: zero obligations
  claimed `verified` in any of the five treatment trials.

What this does **not** establish: that the methodology causes compile
failures. 4-vs-2 at n=5 against 9-distinct-changes variance does not carry
that, and section 2.3 found no supporting mechanism.

## 5. Further experiments — named, not started

Per scope, these are named for the operator to decide. Nothing is started.

1. **Emit-only-the-edit arm.** Run the full methodology procedure but have
   the model emit only the edit, with obligations discarded. This is the one
   test that separates section 2.3's hypothesis — that the edit competes for
   care with the obligation records — from arm-difference-by-chance. It is
   the highest-value next run and costs 5 trials.
2. **Give both arms the `syn` API surface.** All three treatment failures
   are ignorance of a dependency's exact shape. Adding the relevant `syn`
   type definitions to both packets tests whether the arm difference
   survives when that failure mode is removed.
3. **More trials, both arms.** 4-vs-2 does not clear the frozen bar; 10 per
   arm would, if the effect is real at this size.
4. **A second judge family.** Section 3.1 shows a single blind judge
   accepting non-compiling code. A second judge, or judge-plus-compiler,
   would bound that.
5. **The loop arm and the doctrine-in-specification variant**, still
   unstarted from earlier rounds.
