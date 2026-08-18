# AMENDMENT-001 — blind code-quality judge, and task 2

Date: 2026-08-19.
Status: frozen before the first judge call and before the first task-2
generation request. `preregistration.json` is **not** edited; this file is
the amendment record, exactly as that document's `criteria_freeze` clause
requires.

## 0. What had already happened when this amendment was written — read first

**Task 1's two generation requests had already been issued and verified
before this amendment existed.** This amendment therefore does **not**
precede task 1's results, and nothing in it may be read as if it did.
Specifically, at the time of writing:

| Fact | Value |
| --- | --- |
| task-1 methodology arm generation | complete, `generation_ok` |
| task-1 baseline arm generation | complete, `generation_ok` |
| task-1 both arms applied, built, tested | complete, both `verdict: verified` |
| task-1 blind judge call | **not yet made** |
| task-2 anything | **not yet built or run** |

Consequences, stated plainly rather than smoothed over:

1. The judge protocol in section 1 is frozen **before the first judge
   call**, which is the property that makes a blind judgement meaningful.
   It is *not* frozen before the diffs existed, and the author of this
   amendment has seen both task-1 diffs. Blinding constrains what the
   *judge* sees; it cannot un-see what the harness author saw. The judge
   dimensions in section 1 were dictated by the coordinator, not chosen by
   the author after inspecting the diffs, which is the only real protection
   here and is recorded as such rather than claimed as more than it is.
2. Task 2 in section 2 is preregistered before any task-2 generation, so
   its arms are genuinely unprejudiced.
3. Task 1's mechanical result is reported as what it is: a feasibility
   gate that the preregistration already said could not answer the transfer
   question, now additionally known to be non-discriminating (section 2.0).

## 1. Fourth question: blind code-quality judgement

### 1.1 Why

Questions 1-3 (terminates / compiles / tests pass) cannot see the failure
that matters most for implementation work: **passing tests do not mean the
code is any good.** A change can compile, pass every test, and be
unacceptable — most sharply by *gaming the test*: special-casing the
inputs an acceptance test uses, or narrowing a condition until only the
tested path is affected. That is precisely the failure questions 1-3 are
blind to, and it is why this judge exists.

### 1.2 Who judges, and the bias declaration

The judge is Claude (opus), judging changes produced by a different model
family. **This is cross-family: unlike `m7-head-local-v1`'s `claude_skill`
arm, where the judge and one of the judged arms were the same family, there
is no same-family bias to declare here.** That is a genuine strength of this
design and is recorded as such. What remains is the ordinary limitation that
a single judge is a single judge.

### 1.3 Blinding

Structurally the same as `m7-head-local-v1/JUDGE_PROTOCOL.md` section 2:

- **Content-addressed IDs.** Each change is identified only by
  `change_id = sha256(unified diff)[:16]`. Diff header lines carrying
  temp-file paths are stripped before hashing, since those paths encode arm
  names.
- **Hash-sorted presentation.** Changes appear in `change_id` lexical
  order, so position carries no arm information.
- **Identical changes collapse.** Two arms producing a byte-identical diff
  yield one `change_id`, and the judge is not told how many arms produced
  it, nor how many arms exist.
- **Abort, never redact.** `scripts/scan_forbidden_markers.py` runs over
  the assembled packet at build time and again immediately before sending;
  any match raises and stops the run. Its marker list extends m7's with
  this experiment's own arm vocabulary: `qwen`, `baseline`, `methodology`,
  `skill` (case-insensitive for the last three, alongside the inherited
  `claude`/`opus`/`anthropic`).
- **`--no-session-persistence` and `--tools ""`.** The judge has no memory
  of any other call and no filesystem access, so it cannot discover arm
  labels, the acceptance test, the mechanical outcome, or anything else.
- **What the judge never sees:** which arm produced a change, the
  acceptance test, the existing test suite, whether anything compiled or
  passed, the candidates' own obligation records (their vocabulary alone
  would identify the arm), and the harness's reference solution.

### 1.4 No anchoring on the reference solution

`make_reference_candidate*.py` exists only as a harness self-test. It is
**never** shown to the judge. A judge told "here is the right answer" grades
similarity rather than quality and would mark a different-but-good solution
down. Any comparison against the reference is a separate, clearly labelled
secondary analysis performed *after* the blind judgement, never an input to
it.

### 1.5 Judged dimensions

Each with a verdict and a justification that must quote the actual lines it
reasons about.

| Field | Verdicts | Question |
| --- | --- | --- |
| `test_gaming` | `none` / `suspected` / `present` | Does it special-case the inputs a test would use, or narrow a condition so only a tested path is affected, satisfying the letter of the spec while leaving the general rule unimplemented? |
| `scope_discipline` | `clean` / `minor_excess` / `excess` | Does the diff change only what the spec requires? Unrelated edits, drive-by reformatting, or touched files the task never mentioned are defects. |
| `convention_fit` | `fits` / `mixed` / `foreign` | Does it read like the code around it? |
| `spec_implied_cases_not_covered` | `none` / `some` / `many` | Cases the spec's own words imply that the change does not handle. The acceptance test is one witness, not the specification. |
| `hidden_coupling_or_fragility` | `none` / `some` / `serious` | Distant dependencies, ordering assumptions, duplicated invariants that can drift. |
| `comprehensibility` | `clear` / `adequate` / `opaque` | Could a maintainer who did not write it understand why it is correct? |
| `overall` | `acceptable_as_is` / `acceptable_with_changes` / `not_acceptable` | Would you accept this into the repository as it stands? |

`test_gaming` is a first-class output field, not a note.

### 1.6 One judge call per task

One call judges every distinct change for that task together, so the judge
can weigh them against each other without knowing which is which.

## 2. Task 2: small diff, large context

### 2.0 Why task 1 cannot answer the transfer question

The effect measured in `m7-head-local-v1` was not "the process improved the
model's judgement". It was that a 120,000-byte packet sent the bare model
into unbounded rumination until its output budget was gone, and enumerating
obligations first made it converge. **A single-file task has no unbounded
search to bound.** Task 1 is one function in one 13,035-byte file. Its
result therefore cannot distinguish transfer from no-transfer in either
direction, and must not be reported as if it could:

- both arms succeeding would be a false positive (the task never needed the
  method);
- the treatment arm failing would be a false negative (the method's own
  enumerate/project/claim/evidence overhead landing on a task that did not
  need it).

Task 1 is retained as a **feasibility floor**: can this model implement at
all, under this output schema, in this harness. It is cheap, and if it had
failed there would have been no point running anything harder.

**Already known at the time of writing, and recorded here because it bears
directly on this section:** both task-1 arms produced a **byte-identical
diff**, and both reached `verdict: verified`. That is the false-positive
case above, observed. Task 1 is confirmed non-discriminating, empirically
and not only by argument.

### 2.1 The shape task 2 must have

A change of a few lines whose correctness depends on things not visible in
the file being edited: distant invariants, many call sites, contracts
enforced by tests elsewhere, conventions spread across the codebase. That is
where enumerating "what must this change satisfy, and what must it preserve"
before writing pays, and where a naive local edit is wrong for reasons a
local reading cannot see.

### 2.2 The chosen task

**Block-local foreign-module shadowing in the Rust ingestion adapter.**

- Changeable file: `crates/reviewgraphen-ingest/src/rust.rs`, 2,059 lines
  (~80 KB), given to the model in full. The packet is therefore the
  m7-scale large-context packet task 1 deliberately was not.
- The defect is real and previously unfixed:
  `FunctionBodyVisitor::visit_block` hoists block-local `fn`, `const`,
  `static`, tuple/unit `struct` and `use` items as shadows, but a block-local
  `extern "C" { fn target(); }` falls into its catch-all arm. Its foreign
  `fn`/`static` declarations are in the **value namespace**, so a naked
  `target()` call in that block is today still matched to a same-named
  module-level function. That match is unsound.
- Verified red before freezing: 3 of the 5 harness-owned acceptance tests
  fail on the pinned revision, 2 pass.
- Verified solvable before freezing: a reference fix turns all 5 green with
  all 115 pre-existing `reviewgraphen-ingest` tests still passing, zero
  build warnings and zero clippy warnings.

### 2.3 What makes it large-context — the distant constraints a naive edit violates

Named explicitly, as required:

1. **Over-conservatism is a failure, not a safe default.** The locally
   safest edit — "a block containing any foreign module is conservatively
   unresolved", reusing the glob-`use` mechanism sitting a few lines away —
   satisfies the shadowing requirement and *fails*
   `an_extern_block_not_naming_the_called_symbol_does_not_block_resolution`.
   Nothing in the edited region says so. This is the central trap.
2. **The obstruction ledger.** An unresolved call must be retained as a
   typed `RelationUnresolved` obstruction naming the enclosing function, not
   silently dropped. That contract is enforced in `tests/m2.rs`, which the
   model never sees.
3. **Scope lifetime.** The shadow must not leak past its block — enforced by
   a distant test that counts resolved edges exactly, because "at least one
   resolved edge exists" would miss the regression.
4. **Snapshot-level fact and ID invariants.** `tests/m2.rs` asserts exact
   artifact/relation sets, and separately that snapshot and fact IDs are
   independent of the diff base revision. A change that perturbs the
   relation set moves IDs and breaks assertions in a file the model cannot
   read.
5. **115 unseen tests total**, across unit tests and two integration
   suites, all of which must still pass.

### 2.4 Acceptance test is not shown to the candidate

Task 1's packet contained the acceptance test verbatim, to remove
specification ambiguity from a feasibility gate. **Task 2's packet does
not.** Showing it would hand over precisely the preservation constraints
this experiment is trying to see whether obligation enumeration surfaces on
its own. The required behaviour is instead stated in full prose in
`task2/TASK2.md`, at the level a precise ticket would — so a failure is
attributable to the model, not to an under-specified request, while the
oracle stays withheld.

### 2.5 Verification gate for task 2

```text
cargo build  -p reviewgraphen-ingest      -> question 2
cargo test   -p reviewgraphen-ingest      -> question 3
cargo clippy -p reviewgraphen-ingest --all-targets   -> recorded, not a gate
```

`REVIEWGRAPHEN_TRUSTED_CARGO` is set to the pinned 1.95.0 toolchain binary,
because 10 of `tests/m2.rs`'s 40 tests require an externally admitted cargo
(docs/adr/0012) and fail without it **on the pinned revision too** — leaving
it unset would make the question-3 gate meaningless. Confirmed before
freezing: with it set, all 115 tests pass on the pinned revision.

### 2.6 Ordering

Task 1 first (already run). Task 2 second, and only because task 1 cleared
the feasibility floor: both arms produced a schema-valid, compiling,
test-passing candidate.

## 3. Preregistered interpretation

Frozen before the first judge call and before any task-2 result exists.

### 3.1 What counts as "the methodology transferred"

All of the following, on **task 2**:

1. the treatment arm terminates with a message item and non-empty final
   content; **and**
2. its change compiles; **and**
3. the full `reviewgraphen-ingest` suite passes; **and**
4. the blind judge returns `test_gaming: none` for it; **and**
5. the blind judge returns `overall` of `acceptable_as_is` or
   `acceptable_with_changes` for it; **and**
6. the control arm fails at least one of 1-5, or produces a materially
   different change that the judge rates worse on at least one dimension.

**Passing tests with `test_gaming` at `suspected` or `present` is not a
success.** It is recorded as `mechanically_passing_but_gamed` and reported
in its own column.

### 3.2 How mechanical/judge disagreement is reported

A change that passes every mechanical check but is judged
`not_acceptable`, or is flagged for test-gaming, is **the most informative
single outcome this experiment can produce**. It is reported as its own
category, `mechanical_pass_judge_reject`, with both the mechanical numbers
and the judge's cited lines quoted in full. It is never collapsed into
either the pass column or the fail column, and it is never described as a
pass with a caveat.

The reverse — a change the judge likes that does not compile or does not
pass — is reported as `judge_accept_mechanical_fail` and is treated as
evidence about the judge's limits, not about the code.

### 3.3 If both arms fail task 2

Recorded as `both_arms_failed`, and interpreted as **evidence about the
task's difficulty for this model, not as evidence about the process**. A
floor result cannot separate the arms any more than a ceiling result can.
In that case the reported conclusion is "cannot tell from this design", and
the recommended next step is a task calibrated between task 1 and task 2,
not a stronger claim.

### 3.4 If nothing separates

"Cannot tell from n=1" remains a preregistered acceptable answer and is the
required conclusion whenever the arms do not separate on the criteria in
3.1. n is 1 per arm per task. Two tasks do not make n=2 for a hypothesis
about a class of tasks; they make two single observations of different
shapes. No amount of narrative may be used to promote that into a directional
finding.

## 4. Unchanged

Everything else in `preregistration.json` stands: execution conditions,
stopping rules, isolation, the no-retry standing order, the one-request-in-
flight rule, and the maximum of 3 generation requests **per task**.
