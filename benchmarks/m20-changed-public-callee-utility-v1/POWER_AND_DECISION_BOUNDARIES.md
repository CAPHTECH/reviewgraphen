# m20 descriptive effect and decision boundaries

## 1. Registered frozen-corpus analysis

Let A be the baseline arm and B the ReviewGraphen arm. One analysis unit is one
base→head commit cluster. The four paired cells are:

| Cell | A | B | Symbol |
| --- | ---: | ---: | --- |
| both complete | 1 | 1 | `n11` |
| ReviewGraphen only | 0 | 1 | `b` |
| baseline only | 1 | 0 | `c` |
| neither completes | 0 | 0 | `n00` |

`n = n11 + b + c + n00`. The registered paired effect is

\[
\widehat\Delta = \widehat P(B=1)-\widehat P(A=1)=\frac{b-c}{n}.
\]

Here completion means `usable_grounded_disposition_completed`, not merely valid
JSON. Cell values come only from the generated primary records of the
hash-frozen executable evaluator specified in
[EVALUATOR_SPEC.md](EVALUATOR_SPEC.md); this document does not reimplement its
packet, loss, normalization, scoring, or judge-batch algorithms. The required
utility judge is a primary usability proxy rather than defect truth, so the
endpoint remains dependent on the frozen judge model/rubric.

The 10/40-commit samples contain adjacent first-parent commits from only three
repositories. Discordances can share code, authors, development series, and
model difficulty. They are not registered as independent Bernoulli trials.
Consequently m20 has no confirmatory p-value, nominal alpha, confidence
interval, or population-power claim. Its primary decision is a preregistered
**frozen-corpus descriptive operational gate**.

The report MUST publish all four cells and `Delta` overall and separately for
every repository. It MUST also publish three leave-one-repository-out
recalculations of the cells and `Delta`. Repository-specific and leave-one-out
results are sensitivity descriptions only; no rectangle is rescaled to their
smaller `n`, and no inferential p-value is attached.

## 2. Operational rectangles

| Stage | Required rectangle | Minimum overall `Delta` | Meaning |
| --- | --- | ---: | --- |
| 1 | `b >= 8` and `c <= 1` out of 10 | 0.700 | very large descriptive advantage in the frozen pilot sample |
| 2A | `b >= 18` and `c <= 7` out of 40 | 0.275 | large descriptive advantage in the frozen cumulative sample |

The rectangle is the entire primary rule. A favorable judge result, safety
result, or reference tail probability cannot rescue a cell outside it.
Stage 2A is reached only after Stage 1 passes and cannot rescue a Stage 1
failure.

This remains an empirical gate: it can show a large, preregistered improvement
in usable audited dispositions on the frozen corpus. The correction narrows the
claim to that corpus; it does not weaken the observed-effect threshold or turn
the result into a population estimate.

## 3. Independence-reference arithmetic only

For continuity with the design audit, m20 retains one explicitly
non-inferential calculation. If, contrary to this corpus design, the `m=b+c`
discordant directions were independent `Binomial(m, 0.5)` observations, the
conventional two-sided tail would be

\[
r=\min\left(1,
2\sum_{k=0}^{\min(b,c)}{m \choose k}2^{-m}\right).
\]

`r` is labeled `independence_reference_tail_probability`. It is not an m20
p-value, is not compared with alpha, and is not part of success or advance.

At the Stage 1 corner `b=8,c=1`, `m=9`:

\[
r=2[{9\choose0}+{9\choose1}]2^{-9}=20/512=0.0390625.
\]

At the Stage 2A corner `b=18,c=7`, `m=25`:

\[
r=2\sum_{k=0}^{7}{25\choose k}2^{-25}
=1{,}452{,}412/33{,}554{,}432
=0.043285250663757324.
\]

Thus A2/07's arithmetic values `.03906` and `.04329` were calculated
correctly. D1-B4 is nevertheless correct that they are not nominal exact
p-values for serially dependent m20 commits.

The rectangles are deliberately more conservative than this hypothetical
tail rule. For example, `b=7,c=0` at 10 pairs and `b=17,c=6` at 40 pairs are
outside the frozen rectangles even though their independence-reference tails
would be below `.05`.

## 4. Stage 0: seven deterministic gates

Let `C` be the exact sorted set of all 300 commit-cluster IDs. For each `c` in
`C`, define `A_c` as the exact set of substantive D
obligation IDs whose applicability is exactly `applicable` after the frozen
profile's explicit exclusions. Target-support-`unknown` obligations,
rule-level gap obligations, and `ExclusionRecord` IDs are not members of
`A_c`, but remain in their own published denominators. Let
`A = union_c A_c`; snapshot-bound obligation identity makes these sets
disjoint across clusters.

Let `D_c` be the exact subset of `A_c` whose plan status is `deferred`, and let
`D=union_c D_c`. Every numerator ID MUST close to `A`; no count-only substitute
is allowed. If `A` is empty the fraction is defined as zero, although the
separate prevalence gate then fails.

Let `S_c` be the exact subset of `A_c` whose selected packet admits both the
exact caller and exact callee subject windows, each closing to its required
source ID and full subject span, and let `S=union_c S_c`. Every ID in `A\S`
MUST close to a typed subject-loss/unknown record with source and recovery
reference. Profile-excluded candidates are outside `A` and cannot be remainder.

Stage 0 passes only if all seven gates pass:

1. **Prevalence:** at least 45/300 clusters have nonempty `A_c`, and
   `|A| >= 60`.
2. **Subject retention:** `20*|S| >= 19*|A|`.
3. **Bounded context:** over the same applicable packets,
   `median(admitted_source_bytes) <=
   0.50 * median(whole_changed_production_files_bytes)` and nearest-rank p90
   admitted bytes are at most 65,536.
4. **Determinism:** two clean builds match every registered deterministic field
   and canonical byte for 100% of clusters.
5. **Enumeration honesty:** partial direct-call enumeration, its limitations,
   enumeration-obstruction IDs, and the D rule-level gap remain visible for
   100% of repository snapshots.
6. **Fan-out:** sort the 300 `(c, |A_c|)` pairs by
   `(|A_c|, c)`, including a zero for every zero-applicable-
   obligation commit. The 1-indexed nearest-rank p95 is the count at
   `ceil(.95*300)=285`; it MUST be at most 50.
7. **Deferred fraction:** `|D|/|A| <= .05`, evaluated without floating-point
   ambiguity as `20*|D| <= |A|`.

Subject-retention fixtures are `|A|=60,|S|=57` (exact pass) and `|S|=56`
(one-ID-lower failure). Fan-out boundary fixtures are frozen as follows: a 300-count vector whose
285th sorted value is exactly 50 passes; changing that one value to 51 (`+1`)
fails. Deferred fixtures use the exact sets `|A|=60, |D|=3` (passes at `.05`)
and the `+1` numerator `|D|=4` (fails). Gate 1 guarantees a nonzero deferred
denominator. No top-50 cap, silent drop, post-result filter, or count without
exact IDs may simulate either gate.

The seven gates are a conjunction. Any one failure stops all model execution,
is reported as slice failure, and prohibits the phrase "feasibility success."

## 5. Hash sample and control sensitivity

Model eligibility applies the same sole enforced input-budget predicate to
each arm: `admitted_source_bytes <= 65,536`. Both arms exactly at the boundary
pass. If either arm is 65,537 bytes or more, the pair has the sealed typed
terminal state `model_ineligible` with null arm results, zero reviewer/judge
calls, CLI exit 0, and no entry in `n`, `b`, `c`, or `n00`. The `+1` fixture
must demonstrate that behavior, and no post-outcome replacement is permitted.

There is no evaluator-enforced input-token ceiling or tokenizer preflight.
Backend tokenizer and usage data are observation-only and cannot affect model
eligibility, ordering, scoring, failure codes, or decisions. The common
requested maximum-output value is 12,000; its empirical basis is recorded only
in `MODEL_PIN_RATIONALE.md`. Consequently, m20 enforces equal admitted-source-byte ceilings but
cannot claim equal token budgets, equal information, equal serialized request
bytes, or equal cost.

Every model-eligible commit is ordered once by the frozen public sampling hash,
without consulting its control label. Stage 1 uses the first 10; Stage 2A adds
the next 30. Labels never alter membership or order.

Before any arm outcome, two evaluators who did not implement the slice
independently label every model-eligible commit under the frozen source-only
rubric and seal all labels/rationales. Agreement `clean_refactor_control` forms the safety
subset; agreement `not_control` does not. Disagreement or missing second review
is `control_status_unresolved`, remains in the primary sample, and is not
silently moved between strata. Knowledge of a public hash rank has no effect:
labels cannot alter membership or order.

Stage 1 needs at least two agreed controls among its fixed 10, and Stage 2A
needs at least eight among its fixed 40, to make the registered safety gate
measurable. Insufficiency does not alter the primary cells, but blocks advance
or final success as `insufficient_control_observations`.

## 6. Stage decisions

Stage 1 advances only when:

- the portable single-pipeline evaluator-bundle hash, semantic execution hash,
  generated-full-run fixture, reference-vector, and attack-manifest hashes are
  non-null; every named attack oracle and verification pass under CPython
  3.13.5 with Unicode database 15.1.0;
- all seven Stage 0 gates pass;
- the fixed 10-pair sample has `b>=8,c<=1`;
- repository-specific and leave-one-repository-out cells are published;
- backend identity/health and leakage gates pass;
- all 10 judge batches are sealed;
- at least two agreed controls occur in the fixed sample; and
- `J_B_clean <= J_A_clean + 1`.

Stage 2A adds 30 fresh hash-ranked pairs. Final m20 success requires the Stage
1 advance event and, over all 40 pairs, `b>=18,c<=7`, all sensitivity tables,
continuing backend/leakage integrity, 40 sealed judge batches, at least eight
agreed controls, and `J_B_clean <= J_A_clean + 1`.

A pre-stage backend failure means the stage did not launch. Once its pair
manifest is sealed and the stage launches, timeout, parse failure, missing
output, or invalid output is scored by the frozen evaluator without retry. A
missing or invalid two-candidate primary utility-judge batch makes both arm
cells `0`. Control-adequacy failure does not alter scored
primary cells but blocks advance.

## 7. Authorized time ceilings

All reviewer time arithmetic applies only to `Qwen3.8-27B-MLX-4bit` with the
mlx-dspark server-default `low` effort and no `reasoning_effort` request
override. The selection basis and its non-holdout, n=1-style limitations are
recorded once in [MODEL_PIN_RATIONALE.md](MODEL_PIN_RATIONALE.md). No 8-bit,
`ornith-*`, or `xhigh` reviewer run is authorized.

| Stage | Reviewer model | Judge model | `cumulative_model_ceiling` | Reserve | `wall_clock_envelope` |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1 | `20*900 = 18,000 s` | `10*90 = 900 s` | 18,900 s = 5.25 h | 2,700 s = 0.75 h | 21,600 s = 6 h |
| 2A | `80*900 = 72,000 s` | `40*90 = 3,600 s` | 75,600 s = 21 h | 10,800 s = 3 h | 86,400 s = 24 h |

Stage 2A adds 18 `wall_clock_envelope` hours to Stage 1: 15 reviewer-model hours, 0.75
judge-model hours, and 2.25 reserve hours. Execution remains serial
(`concurrency=1`). The 24-hour authorization is a cumulative
`wall_clock_envelope` containing at most 21 `cumulative_model_ceiling` hours; it is not 24 model-hours plus
reserve. Identity checks, validation, sealing, failures, and launch overhead
consume reserve and do not authorize extra calls.

Stage 2B and 72 hours are unauthorized. Unused time never authorizes reruns,
post-hoc pairs, threshold changes, or sample replacement.

## 8. Interpretation ceiling

No joint outcome distribution or independent repository sample supports a
population-power calculation. Ten pairs can only clear an enormous descriptive
effect rectangle; 40 can clear a large one. The utility judge is a required
primary non-authority proxy. The separate defect projection and clean-control
safety gate are secondary non-authority evidence and cannot rescue primary.
Findings are correlated within a commit and never increase `n`.

The study can satisfy its practical empirical gate by demonstrating the frozen
rectangles with transparent repository and leave-one-out sensitivity. Because
the usability judge is a necessary primary predicate, success remains specific
to its frozen model/rubric and may include correlated model-family error. It
cannot claim significance, population precision/recall, a general Rust effect,
holdout validity, verification, human acceptance, or defect truth.
