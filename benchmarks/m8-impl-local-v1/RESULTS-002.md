# m8-impl-local-v1 — results after AMENDMENT-002

Supplements `RESULTS.md`; does not replace it. The first task-1 run stays
reported there exactly as recorded. Per `AMENDMENT-002.md` section 6, every
task-1 number below is labelled first-run or re-run.

Conditions were not changed after seeing any result. Four generation
requests, sequential, one in flight, none retried. **Model identity verified
as `qwen3.8-27b-mlx` on all four**, against a server whose advertised list
was `qwen3.8-27b-mlx`, `qwen3.8-27b-mlx@4bit`, `qwen3.8-27b-mlx@8bit`,
`qwen3.8-27b-mtp`, `text-embedding-nomic-embed-text-v1.5`. No `@8bit`,
`@4bit`, or `mtp` variant was used.

## 1. All four cells now have data

| | task 1 treatment | task 1 control | task 2 treatment | task 2 control |
| --- | --- | --- | --- | --- |
| | (re-run) | (re-run, **clean contract**) | | (**clean contract**) |
| terminates | yes | yes | yes | yes |
| elapsed | 6.84 min | 6.92 min | **17.22 min** | **14.65 min** |
| input tokens | 9,530 | 5,839 | 24,782 | 21,091 |
| cached input tokens | 0 | 0 | 2,048 | 0 |
| output tokens | 9,787 | 10,553 | 22,598 | 19,522 |
| reasoning tokens | 7,247 | 10,298 | 19,579 | 19,103 |
| reasoning share | 74.0% | **97.6%** | 86.6% | **97.9%** |
| final content bytes | 10,698 | 996 | 12,604 | 1,705 |
| message item created | yes | yes | yes | yes |
| compiles (0 warnings) | yes | yes | yes | yes |
| clippy (0 warnings) | yes | yes | yes | yes |
| tests pass | 3+6, 0 failed | 3+6, 0 failed | **120, 0 failed** | **120, 0 failed** |
| verdict | `verified` | `verified` | `verified` | `verified` |

Task-2 test breakdown, both arms: 67 unit + 8 `extraction_report_schema` +
40 `m2` + 5 harness-owned acceptance = 120 passed, 0 failed.

**Nothing failed anywhere.** The one earlier failure —
`upstream_stream_closed_before_completion` at 134 s — was network-layer, as
the operator independently established, and is reported in `RESULTS.md`
section 2.1. It is not a model result and is not counted here.

### 1.1 The reasoning-share figure is an artefact of output demand

The control's reasoning share (97.6% / 97.9%) looks dramatically worse than
the treatment's (74.0% / 86.6%). **It is not.** Absolute reasoning tokens are
nearly identical within each task — 10,298 vs 7,247 on task 1, 19,103 vs
19,579 on task 2. What changed is the denominator: the clean control is
asked for an edit and nothing else, so its final content is 996–1,705 bytes
against the treatment's 10,698–12,604.

This is exactly the confound `AMENDMENT-002.md` section 1.3 recorded in
advance, and it cuts the other way too: the control is also **faster**
(14.65 vs 17.22 min on task 2), and part of that is the smaller output
demand rather than the absence of the method. **Reasoning share should not
be compared across arms under this design.** Absolute reasoning tokens can
be.

## 2. Task 1 replicated, under a clean control

All four task-1 candidates — first-run treatment, first-run control, re-run
treatment, re-run clean control — produced a **byte-identical resulting
file**, `sha256 6a2ac918158ee19abe7ab3caddeab9327cce2242fbb1a277ed6fa44568e63bf9`.

The clean control reached it by a *different anchor* (it anchored on the
following `schema list` arm and re-emitted it, rather than on the preceding
arm), which is direct evidence that the convergence is on the change itself,
not on a copied answer shape.

This is the stronger statement the re-run was run to get: under a control
with no methodology vocabulary at all, task 1 still does not discriminate.
Task 1 is a feasibility floor and nothing more, now with n=2 per arm.

## 3. Task 2 — both arms passed, and the changes are genuinely different

Both hoist a block-local foreign module's `fn`/`static` idents, and both
avoid the central trap: neither marked the whole block conservative, so
`an_extern_block_not_naming_the_called_symbol_does_not_block_resolution`
passes for both.

The changes are not identical. Distinct `change_id`s:
`fe6e01320b1f9c84` (treatment), `8b6a4b6e707df101` (control).

- **Treatment** added a seven-line explanatory comment above the new arm and
  left the existing doc comment untouched.
- **Control** rewrote the existing doc comment to mention foreign modules,
  and added the arm without a comment of its own.

## 4. Blind judgement of task 2

One call, Claude opus, `--no-session-persistence --tools ""`, 75 s, cross-
family, marker scan passed twice. Two changes, hash-ordered, arms withheld.

| dimension | treatment `fe6e0132…` | control `8b6a4b6e…` |
| --- | --- | --- |
| `test_gaming` | `none` | `none` |
| `scope_discipline` | `clean` | `clean` |
| `convention_fit` | **`fits`** | **`mixed`** |
| `spec_implied_cases_not_covered` | `some` | `some` |
| `hidden_coupling_or_fragility` | `some` | `some` |
| `comprehensibility` | `clear` | `clear` |
| `overall` | `acceptable_as_is` | `acceptable_as_is` |

### 4.1 The judge found a real defect that 120 tests did not — in both arms

Unprompted, and in both changes independently, the judge identified the
`_ => {}` catch-all over `syn::ForeignItem`:

> "`_ => {}` inside the inner match silently drops `syn::ForeignItem::Macro`
> and `syn::ForeignItem::Verbatim`. A macro invocation inside
> `extern "C" { ... }` can expand to `fn` or `static` declarations whose
> names this pass cannot enumerate — precisely the situation the crate
> elsewhere answers with conservatism (`conservative_unresolved |= has_glob;`
> for glob `use`, and `if has_unknown { self.mark_current_scope_conservative(); }`
> for patterns). Here an unenumerable foreign declaration yields no shadow
> and no conservatism, so a naked call can still be wrongly matched — the
> same unsound match the spec's opening paragraph calls out."

That is correct, it is the crate's own documented fail-closed doctrine, no
test in the suite catches it, and **both arms have it**. The harness's
reference solution does not (it routes unrecognized foreign items to
`conservative_unresolved`), but the reference was never shown to the judge
and is not what the judge compared against — it reasoned from the
specification and the surrounding code.

This is the single most valuable observation in the experiment: a change can
compile, pass 120 tests, be judged `acceptable_as_is`, and still carry a
soundness gap that only reading the code finds.

### 4.2 The only judged difference, stated at its true size

`convention_fit`: `fits` versus `mixed`. The judge's reason for the control's
downgrade is a comment-wrapping blemish it quoted:

> "the inserted text leaves `/// and every name a block-local `use` item
> introduces, is hoisted -- bound for the` noticeably wider than every
> neighbouring comment line ... rustfmt will not re-wrap a comment, so this
> ragged line is what lands."

**Post-hoc check, clearly not a preregistered criterion:** `rustfmt --check`
on both candidates' `rust.rs` exits 0. The judge was right that rustfmt does
not rewrap comments; the blemish is real to a reader and invisible to the
formatter.

## 5. Against the preregistered transfer criteria

`AMENDMENT-001.md` section 3.1 requires all six, on task 2:

| # | criterion | result |
| --- | --- | --- |
| 1 | treatment terminates with a message item and non-empty content | **met** |
| 2 | its change compiles | **met** |
| 3 | full `reviewgraphen-ingest` suite passes | **met** (120/120) |
| 4 | judge returns `test_gaming: none` | **met** |
| 5 | judge returns `acceptable_as_is` or `acceptable_with_changes` | **met** (`acceptable_as_is`) |
| 6 | control fails one of 1-5, **or** produces a materially different change the judge rates worse on at least one dimension | **met, narrowly** |

Criterion 6 is met by its second clause: the control produced a materially
different change (distinct `change_id`) and the judge rated it worse on
`convention_fit`.

**By the letter of the criteria frozen before any task-2 result existed,
the methodology transferred. The margin is one dimension, one grade, on
comment formatting.** Both statements are true and neither may be dropped.

I am not raising the bar after the fact — that would be exactly the
criteria-drift the preregistration forbids. I am reporting the size of the
effect the criteria actually caught.

### 5.1 What the result is not

- Not evidence the method prevents defects. The substantive gap in section
  4.1 is **identical in both arms**. The method did not catch it.
- Not `mechanically_passing_but_gamed`: `test_gaming` is `none` for both.
- Not `mechanical_pass_judge_reject`: both are `acceptable_as_is`. The
  closest the experiment came to that category is section 4.1 — a real
  defect found by reading, in changes the judge nonetheless accepted.
- Not a latency or termination claim: section 1.1's confound forbids it.

## 6. Honest read

**The preregistered criterion for transfer is met, and the evidence behind
it is thin.**

Established:

- The model does bounded implementation work reliably. Four for four:
  terminated, applied cleanly, compiled with zero warnings, clippy clean,
  every test green — including on a 102 KB packet over a 2,059-line file
  with 115 unseen tests it had to avoid breaking.
- **The economics do not forbid iteration.** Worst case measured is 17.22
  minutes and 22,598 output tokens. A three-turn edit-compile-fix loop is
  under an hour, not the 2.5–4 hours the original preregistration budgeted
  against.
- Task 1 does not discriminate, replicated under a clean control, four
  candidates converging on one byte-identical file.
- Task 2 does discriminate, but only on `convention_fit`.
- A blind cross-family judge found a genuine soundness gap that 120 tests
  missed — and found it in both arms.

Not established:

- That the methodology improves correctness. On the one task designed to
  test it, both arms passed everything and shared the same real defect.
- Any effect size worth acting on. One dimension, one grade, n=1 per arm per
  task.
- Anything about an edit-compile-fix loop; no arm was given a compiler.
- Anything about ReviewGraphen the implementation.

n is 1 per arm on task 2 and 2 per arm on task 1. Two tasks are two single
observations of different shapes, not n=2 for a hypothesis about a class of
tasks.

## 7. What to run next

The operator asked to see task 2 before anything else starts. In priority
order once seen:

1. **Repeat task 2, several times per arm.** The whole result rests on one
   trial per arm and a one-grade difference on one dimension. At ~15-17
   minutes a trial, five per arm is under three hours and would say whether
   `convention_fit` is a real tendency or a coin flip.
2. **The edit-compile-fix loop arm.** The measured cost now permits it, the
   repair-packet builder is written and has never been triggered, and it is
   the only way to test the hypothesis that made implementation look
   different from review in the first place — that Evidence can actually
   reach `verified`.
3. **Feed section 4.1's defect back as a task.** Both arms failed to apply
   the crate's own fail-closed doctrine to unknown foreign items. Whether
   naming that doctrine in the specification changes the outcome is a
   sharper, cheaper question than any of the above.
4. **A second judge, or a second judge family.** One judge produced the most
   valuable observation here; its verdicts currently have no cross-check.
