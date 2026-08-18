# m8-impl-local-v1 — results

Conditions were not changed after seeing any result. `preregistration.json`
and `AMENDMENT-001.md` are the frozen design; this file records outcomes
only.

**Campaign status: STOPPED by the standing no-retry order.** Task 2's
treatment arm died server-side; task 2's control arm was never issued. See
section 4. Nothing here is a retry.

## 1. Task 1 — `review-flag-order` (feasibility floor)

One function in one 13,035-byte file. `AMENDMENT-001.md` section 2.0 states
in advance that this task cannot answer whether the process transfers,
because a single-file task has no unbounded search to bound.

### 1.1 Generation

| | treatment (with the frozen implementation skill) | control (no skill) |
| --- | ---: | ---: |
| packet bytes | 37,234 | 23,119 |
| provider input tokens | 9,530 | 6,054 |
| cached input tokens | 0 | 0 |
| elapsed | **8.76 min** (525.6 s) | **7.11 min** (426.5 s) |
| provider output tokens | 12,888 | 10,930 |
| of which reasoning | 10,168 | 9,899 |
| **reasoning share of output** | **78.9%** | **90.6%** |
| final content bytes | 11,256 | 4,337 |
| message item created | **yes** | **yes** |
| `response.status` | `completed` | `completed` |
| `incomplete_details` | null | null |
| failure class | `generation_ok` | `generation_ok` |

Both arms terminated. Neither came close to the 131,072-token cap.

### 1.2 Mechanical verification

Both candidates were schema-valid, their anchors matched exactly and
uniquely, and their edits applied to a fresh `git archive` scratch copy.

| | treatment | control |
| --- | --- | --- |
| `cargo build -p reviewgraphen-cli` | exit 0, 0 warnings | exit 0, 0 warnings |
| `cargo clippy -p reviewgraphen-cli --all-targets` | exit 0, 0 warnings | exit 0, 0 warnings |
| `cargo test -p reviewgraphen-cli` | 3 unit + 6 acceptance passed, 0 failed | 3 unit + 6 acceptance passed, 0 failed |
| verdict | `verified` | `verified` |

### 1.3 The two arms produced a byte-identical diff

Both emitted the same 12-line added match arm. Content-addressing collapsed
them to one `change_id` (`1b3e1e8b79ad9952`), and
`runs/judge-task1/truth.json` records that both arms map to it.

This is exactly the false-positive case `AMENDMENT-001.md` section 2.0
predicted in advance: on a task that never needed the method, both arms
succeed and the comparison says nothing. **Task 1 is confirmed
non-discriminating, empirically and not only by argument.**

### 1.4 Evidence discipline (secondary preregistered measure)

The treatment arm emitted **6 obligations**, evidence statuses
`{requested, claimed}` — **zero `verified`**. It stated in its own
limitations that "no compiler or test runner was available in this session,
so `verified` is unavailable for every obligation", and closed by naming the
`environment valid` and `independent` verification facets of
`docs/10` section 6 as unmet, adding that its analysis "is the same model
reasoning about its own edit, not an independent mechanism". Its per-
obligation `evidence_requirement` fields named the exact cargo commands and
test names that would settle each one.

The control arm emitted **3 obligations**, all `claimed`, also zero
`verified`.

Reading this honestly: the discipline held in the treatment arm, but the
control arm also never claimed `verified`. **The shared output schema names
the `obligations` and `evidence_status` fields for both arms, so part of the
methodology leaked into the control by construction.** This is a design
defect in the control, disclosed here, not a clean null result.

### 1.5 Blind code-quality judgement (question 4)

One call, Claude opus, `--no-session-persistence --tools ""`, 55 s. Cross-
family. The judge saw the specification and the diff with 25 lines of
context each side; it did not see the arm, the acceptance test, the
mechanical outcome, or the reference solution. The abort-not-redact marker
scan passed at build time and again immediately before sending.

`change_id 1b3e1e8b79ad9952` (both arms):

| dimension | verdict |
| --- | --- |
| `test_gaming` | **`none`** |
| `scope_discipline` | `clean` |
| `convention_fit` | `fits` |
| `spec_implied_cases_not_covered` | `none` |
| `hidden_coupling_or_fragility` | **`some`** |
| `comprehensibility` | `clear` |
| `overall` | **`acceptable_as_is`** |

The one non-clean dimension, quoted: "`generic_review(Path::new(request_path),
Path::new(artifact_root))` now exists verbatim in two arms; if the call's
argument order or wrapping ever changes, both copies must change together
and nothing in the file enforces that — a small duplicated invariant that
can drift."

On `test_gaming` the judge reasoned from the code rather than asserting: the
new arm "is a positional pattern over the whole slice, not a special case
for particular values ... It matches every five-token
`review --artifacts X --request Y` line regardless of what X and Y are,
including flag-looking tokens".

No `mechanical_pass_judge_reject` case arose on task 1.

## 2. Task 2 — `extern-block-shadow` (the discriminating task)

2,059-line file given in full; 115 unseen tests; the locally safest edit
fails a distant test. Verified red (3 of 5 acceptance tests fail on the
pinned revision) and solvable (reference fix: 5/5 plus 115/115, zero build
and clippy warnings) before freezing.

### 2.1 Treatment arm — server-side death, no result

| | value |
| --- | --- |
| packet bytes | 102,298 (≈26,000 input tokens at task 1's measured 3.9 bytes/token) |
| request issued | 2026-08-19T05:21:41+09:00 |
| stream died | 2026-08-19T05:23:55+09:00 |
| elapsed | 134.066 s (2.23 min) |
| HTTP status | 200 (headers received) |
| streaming before death | 43 `response.reasoning_text.delta` events, 0 `output_text` events |
| transport error | `ConnectionResetError: [Errno 104] Connection reset by peer` |
| `response.completed` | never sent |
| message item created | **no** |
| failure class | **`upstream_stream_closed_before_completion`** |

Per the preregistered denominator rule, `upstream_*` classes are server-side
and excluded from any semantic denominator. **This is not a model failure
and must not be reported as one.** It is the same family as the 23.9-minute
`stream closed before response.completed` entry in the local-qwen skill's
server failure playbook.

### 2.2 Control arm — never issued

Stopped before it started, per `preregistration.json`
`stopping_rules`: "If any arm fails with an `upstream_*` class, stop the
entire campaign immediately. Do not start the next arm."

### 2.3 Task 2 outcome

**No data.** Task 2 answers none of the four questions. It is not a
`both_arms_failed` result either: one arm never ran and the other never
reached the model's own budget.

## 3. The four questions, as far as they were answered

| | task 1 treatment | task 1 control | task 2 treatment | task 2 control |
| --- | --- | --- | --- | --- |
| 1. terminates | yes, 8.76 min | yes, 7.11 min | **no — server-side** | not issued |
| 2. compiles | yes | yes | — | — |
| 3. tests pass | yes | yes | — | — |
| 4. judged quality | `acceptable_as_is`, `test_gaming: none` | identical change | — | — |

## 4. Server failure report (standing order)

Reported as the local-qwen skill's no-retry rule requires:

1. **Time the symptom was noticed:** 2026-08-19T05:24 +09:00, on the
   background task's completion notification; the stream itself died at
   2026-08-19T05:23:55+09:00.
2. **Approximate token count of the request:** ~26,000 input tokens
   (102,298-byte packet); output was under ~200 tokens, all reasoning.
3. **Client:** `python3` `urllib` issuing a single non-streamingly-retried
   POST to `http://192.168.68.71:11999/v1/responses`. Not Codex CLI, not
   Claude Code.

Not retried. No further generation request was issued after it. A control
endpoint probe (`/v1/models`) answered HTTP 200 in 0.475 s at
2026-08-19T05:24:28+09:00 — recorded only because the skill states
explicitly that a responsive control endpoint is **not** evidence that
generation is healthy.

Resumption requires the operator independently confirming recovery with a
minimal request of their own; that would be a fresh first attempt, not a
retry.

## 5. Honest read

**Cannot tell from this design whether the methodology transfers.**

What is actually established:

- The local model *can* do bounded implementation work. On a small,
  precisely specified task it terminated in under 9 minutes, produced a
  schema-valid anchored edit that applied cleanly, compiled with zero
  warnings, passed every test, and was judged `acceptable_as_is` with no
  test-gaming by a blind cross-family judge. That is a real feasibility
  floor and it is cleared.
- The economics look **far better than the review campaign predicted**:
  8.76 min versus 50-80 min, 12,888 output tokens versus 67,000-77,000, and
  a reasoning share of 78.9% versus 97-99%. The cost argument against
  iteration is materially weaker on this evidence than it was on m7's.
- The methodology's own discipline held where it was exercised: six
  obligations, zero false `verified`, explicit naming of the unmet
  verification facets.

What is **not** established, and must not be inferred:

- Whether the methodology helps. Both arms produced a byte-identical diff on
  task 1, which was preregistered as unable to discriminate and is now
  observed not to. The only task designed to discriminate never produced a
  result.
- Whether the control arm is a clean control at all. The shared output
  schema names `obligations` and `evidence_status`, so the control was
  partially instrumented with the treatment's vocabulary (section 1.4).
- Anything about an edit-compile-fix loop. No arm was ever given a compiler.
- Anything about ReviewGraphen the implementation. As in m7, this measures a
  methodology only.

n is 1 per arm per task, and one of the four cells is empty.

## 6. What to run next

In priority order.

1. **Re-run task 2, both arms, after the operator confirms the server has
   recovered.** Everything is built, frozen, hash-pinned, and red/green
   verified; it is two generation requests of roughly 10-15 minutes each.
   This is the single highest-value action and it is the only one that can
   answer the question the experiment was designed around.
2. **Fix the control arm's contamination.** Give the control a minimal
   output schema with `edits` only, and drop `obligations`/`evidence_status`
   from it, so the evidence-discipline measure has a real null.
3. **Add the edit-compile-fix loop arm.** The measured economics no longer
   forbid it: at ~9 minutes and ~13k output tokens per turn, a 3-turn loop
   is well under an hour, not the 2.5-4 hours the preregistration budgeted
   against. The conditional single repair turn is already built
   (`scripts/build_repair_packet.py`) and was never triggered.
4. **A task calibrated between task 1 and task 2.** Task 1 is too easy to
   separate arms; task 2's difficulty is unmeasured because it never ran. A
   middle rung would bound where the method starts to matter.
5. **More than one judge, or a second judged task.** One blind judge on one
   change is a single observation, however well blinded.
