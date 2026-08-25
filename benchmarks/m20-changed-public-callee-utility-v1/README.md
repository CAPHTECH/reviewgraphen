# m20 changed-public-callee paired utility evaluation

m20 asks whether a frozen ReviewGraphen packet for
`relation.changed_public_callee@1` makes a fixed reviewer more likely to return
a usable, auditable disposition than a free-form base/head-diff review. The
property is `rust.callee_contract_review@1`; it is a review question about a
changed exact-`pub` callee over an accepted local `calls` relation with
`resolution=syntactic_unique`, not a fact that a contract is broken. The
denominator profile is the ADR-defined, hash-frozen `rust.production.v1`.

This is an **open development evaluation**. The implementation and evaluation
material are readable by the same Unix identity on the same machine. There is
no access boundary, organizational blind, holdout validity, or basis for
generalizing to Rust repositories. The study retains controls that remain
meaningful on one machine: implementation-before-corpus ordering, arm-neutral
contracts, content-hash commitments, result sealing before label disclosure,
leakage scans, and deterministic replay.

## Unit, endpoint, and descriptive estimand

One analysis unit is one base→head **commit cluster**. Multiple obligations,
relations, claims, edges, or findings in a commit are correlated and never
increase `n`. Adjacent commits within a repository may also be dependent;
repository is a reporting/blocking stratum, not an independent observation.
At most one deterministically ranked D obligation is used in a commit's model
packet. Every other obligation remains in the audit denominator as
`not_sampled_for_model_evaluation`.

The primary endpoint is
`usable_grounded_disposition_completed(commit, arm)`. Its value is taken only
from the generated primary-score record of the hash-frozen Python evaluator;
prose cannot rescore a cell. The evaluator owns packet construction, exact
source-body closure, loss opportunity, text normalization, mechanical scoring,
and the required utility-judge reconciliation. The endpoint measures a usable-
audit proxy, **not truth, verification, evidence support, or human acceptance**.

The evaluator is one production pipeline from authenticated repository,
base/head commit, and frozen-obligation selectors through both reviewer calls,
one judge call, and two primary cells. Its only untrusted scoring inputs are raw
reviewer/judge invocation results. Packet, inventory, loss, binding,
opportunity, candidate, batch, permutation, and score remain immutable
in-process values. They are written for audit but never read back by production;
`verify-run` is a separate non-authoritative hostile-artifact checker.

Reviewer packets expose each exact admitted Rust excerpt as directly readable
strict-UTF-8 text in a closed content-addressed payload table. The payload/
exact-path source exception
preserves user code without exposing evaluator-owned D rule/property/relation/
endpoint/subject metadata. Primary abstention is available only when the
evaluator reconstructs equal question/reason opportunity in both arms. One
90-second arm-hidden batch scores two opaque, hash-closed candidates and a
sealed reverse map restores the two arm results. Exact algorithms and schemas
are specified only in [EVALUATOR_SPEC.md](EVALUATOR_SPEC.md).

This directory is presently at single-pipeline evaluator-design status. Model
execution is forbidden until the independent implementation satisfies every
named D4/D5 attack oracle, the unchanged original 52 vectors, and 22 atomic
amendment vectors, reproduces full-run
fixtures, and `preregistration.json` contains non-null evaluator bundle,
execution, fixture, vector, and registered attack-manifest (`mutation_manifest`)
freeze hashes. Aggregate test counts or a displayed pass number are not
acceptance evidence.

The bundle hash is reproducible from canonical clone bytes and excludes the
host runtime. Behavioral replay additionally requires CPython 3.13.5 with
Unicode database 15.1.0 because NFKC, casefold, and Unicode categories affect
scoring. An incompatible runtime must fail verification before execution;
exact executable hash and platform are recorded only as provenance. Thus
clone-only artifact verification is portable, while gate #9 behavioral replay
is conditional on provisioning the documented runtime.

With A = baseline and B = ReviewGraphen, the frozen-corpus paired effect is
`(b-c)/n`, where `b` is B-only completion and `c` is A-only completion. All
four paired cells are published overall, by repository, and with each
repository left out. Because serial commits are not independent Bernoulli
trials, m20 makes no exact-p, alpha, confidence-interval, significance, or
population-power claim. Findings are never independent trials.

## Arms

- **A — baseline:** free-form review of the profile-defined base/head
  production diff. It receives the common inventory/loss contract but no D
  relation, obligation universe, endpoint annotations, or subject-first
  projection.
- **B — ReviewGraphen:** source bytes constructed from one D obligation and its
  `context.subject_windows@3` packet. Reviewer-visible roles remain the generic
  `changed`, `context`, or `support`; obligation, caller/callee, endpoint,
  subject, and universe identities exist only in the hidden audit binding.

Both arms use only `Qwen3.8-27B-MLX-4bit`. Effective reasoning effort is the
mlx-dspark server default `low`; requests MUST omit `reasoning_effort` and MUST
NOT send an override. The 8-bit, `ornith-*`, and `xhigh` alternatives are not
permitted. The evidence and its limitations are recorded once in
[MODEL_PIN_RATIONALE.md](MODEL_PIN_RATIONALE.md).

Both arms use a 900-second hard timeout, no tools, no repository mount, the
same exact 65,536 admitted-source-byte ceiling, at most three claims, and the
same schema. Both request the frozen backend maximum-output value 12,000; its
selection basis is recorded only in `MODEL_PIN_RATIONALE.md`. Equal input budget is
enforced only by the source-byte ceiling; packets are not padded. There is no
evaluator-enforced input-token ceiling or tokenizer preflight, so m20 cannot
claim equal token budgets, information, serialized request bytes, or cost.
Complete packet/component bytes and the deterministic byte-budget record are
sealed. Backend tokenizer and usage data, when supplied, are sealed only as
non-recomputable observations and never affect eligibility or scoring.

Both arms at exactly 65,536 admitted-source bytes are eligible. If either arm
has 65,537 bytes or more, the whole pair terminates before sampling or calls as
sealed `model_ineligible`: null arm results, zero reviewer/judge calls, CLI exit
0, and no entry in `n`, `b`, `c`, or `n00`. An eligible request later rejected
for backend context length is instead an ordinary post-launch arm failure (`0`)
without retry. Arm order uses the preregistered public seed. The evaluator, not
the implementation agent, builds both packets.

## Stages and decisions

| Stage | Scale | Advance/success boundary | `cumulative_model_ceiling` / `wall_clock_envelope` |
| --- | ---: | --- | ---: |
| 0 | exactly 3 repositories × 100 first-parent commit clusters | all seven deterministic gates pass | 0 h / deterministic |
| 1 | first 10 model-eligible commits in one hash order | descriptive gate `b >= 8`, `c <= 1`; all continuing gates pass | 5.25 h / 6 h cumulative |
| 2A | first 40 cumulative commits in the same order | descriptive gate `b >= 18`, `c <= 7`; Stage 1 and all continuing gates pass | 21 h / 24 h cumulative |

Stage 0's seven gates cover prevalence, exact subject retention, bounded context,
determinism, enumeration honesty, fan-out p95 ≤50 over all 300 counts
(including zeros), and deferred applicable-ID fraction ≤5%. Either product
gate failing is slice failure. For cluster `c`, `A_c` is the exact applicable D
obligation-ID set, `S_c` its exact subset whose packet admits both full exact
caller and callee subject spans, and `D_c` its exact deferred subset; `A`, `S`,
and `D` are their unions. Subject retention is `20|S| >= 19|A|`: `57/60`
passes and `56/60` fails. IDs in `A\S` require typed subject-loss/unknown
records and cannot be profile exclusions. Fan-out sorts `(c, |A_c|)` by count then ID and uses count rank
285: 50 passes and `+1` = 51 fails. Deferred uses exact integer comparison
`20|D| <= |A|`: `3/60` passes and `+1` = `4/60` fails.

The fixed model sample is chosen without control labels. Two evaluators label
every model-eligible candidate before arm outcomes; labels never change membership even
though the sampling seed is public. Stage 1/2A require at least 2/8 agreed controls respectively
to measure safety. The safety gate fails when ReviewGraphen judge-positive
clean commits exceed baseline by more than one.

Stage 2B / 72 hours is not authorized. A post-launch timeout, parse failure,
malformed output, invented ID, policy violation, or missing disposition is `0`
with no retry. A backend mismatch or unavailable endpoint before launch stops
the stage. A missing or below-threshold primary utility-judge result is also
`0`; secondary defect and efficiency summaries cannot rescue the descriptive
primary rectangle.

## Limits

The study cannot estimate population precision or recall. The required utility
judge is non-authority proxy evidence, not defect truth, and introduces frozen
judge/model dependence into the primary endpoint. Exact-public syntax does not
prove externally reachable API status, and accepted syntactic-unique local
calls are not a complete call graph. `direct_calls=partial`, its limitation,
enumeration obstructions, and the rule-level gap remain visible. If FSL,
CaseGraphen, or ReviewGraphen is selected, its history was already inspected
during development and gains no holdout validity.

Execution order and arm-neutral contracts are in [PROTOCOL.md](PROTOCOL.md).
Executable evaluator design, reference vectors, generation rules, and freeze
procedure are in [EVALUATOR_SPEC.md](EVALUATOR_SPEC.md).
The seven gates, descriptive rectangles, independence-reference arithmetic,
and 21-model-hour/24-wall-hour accounting are in
[POWER_AND_DECISION_BOUNDARIES.md](POWER_AND_DECISION_BOUNDARIES.md).
