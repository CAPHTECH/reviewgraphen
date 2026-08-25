# Draft preregistration — unsafe-boundary Relation-to-Report utility

Status: **draft to be frozen before implementation**. After freeze, changes are
append-only amendments made before any affected arm is observed. The analysis
code, eligibility code, schemas, model/provider revision, budgets, hashes, and
randomization seed are part of the frozen bundle.

## 1. Question and estimand

For a real Rust bug-introducing change that intersects explicit unsafe syntax,
does the proposed ReviewGraphen slice reduce missed mechanically anchored defect
roots relative to free-form review by the same model under the same resource
budget, without unacceptable false-positive or completion cost?

The estimand is a paired difference across **independent commit units**, not a
difference across model replicates. Mechanism conformance, report validity, and
reproducible unknown/loss display are required gates, not substitutes for defect
utility.

## 2. Frozen corpus

### 2.1 Visible calibration/development ranges — forbidden from final inference

These ranges are already exposed in this repository and therefore cannot be a
holdout:

- `ymm-oss/fsl`: `d4e2c09413e14436f62fc65969a0fecccb5e57b6` through
  `e589014d1655b1224f5b83a7f2de99532a0dcdba` (the 2026-06-11 through
  2026-08-09 range used by M7-real-v2). Existing candidate/oracle/judge material
  may be used only to debug the instrument, never to choose prompts/rules based
  on final outcomes.
- `tokio-rs/axum`: `e4550d23..97def959`, already named and probed in
  `docs/24_context_projection_feasibility_for_implementation.md:176-184`.
- Local ReviewGraphen fixtures and all `benchmarks/m7-*` through `m19-*` are
  development-only. No historical judge rationale is admissible reviewer input.

### 2.2 Evaluator-only holdout ranges

The final candidate census is the union of these real public histories:

- [`crossbeam-rs/crossbeam`](https://github.com/crossbeam-rs/crossbeam): tag
  `crossbeam-0.8.2` through tag `crossbeam-0.8.4`. The official changelog names
  both releases.
- [`tokio-rs/bytes`](https://github.com/tokio-rs/bytes): tag `v1.5.0` through tag
  `v1.11.1`. The official changelog names both endpoints.

At preregistration freeze, an evaluator who does not implement the slice must:

1. resolve every annotated tag to its peeled commit SHA;
2. record remote URL, tag object/commit SHA, complete object-pack SHA-256, and
   `git fsck` result in an evaluator-only manifest;
3. fail the study rather than substitute a range if an endpoint cannot be
   resolved exactly; and
4. publish only the range-level manifest hash before implementation. Individual
   eligible commits, tests, roots, rationales, and splits remain encrypted or in
   an access-controlled evaluator path unavailable to implementers/reviewers.

Public source cannot be made cryptographically unknowable, so this is an
organizational blind: implementers sign a no-inspection declaration for the two
holdout histories after freeze, and their execution environment has neither
holdout Git objects nor network access. Any observed holdout commit before both
arms complete invalidates that unit.

### 2.3 Deterministic eligibility and world anchor

A final positive unit is admitted only if all of the following code-owned steps
succeed:

1. A fix commit in the frozen range changes production `.rs` and adds or changes
   an exact Rust test.
2. With both revisions prebuilt by the same pinned toolchain, the exact test
   passes at the fix and fails **at runtime** (not build/timeout) at its first
   parent. This follows the stronger existing presence discipline recorded at
   `benchmarks/m7-real-v2/PRESENCE_CALIBRATION_REPORT.md:23-30`.
3. Deterministic bisection with that exact test identifies one first-bad commit
   in the frozen ancestry. The test passes at the selected parent (base) and
   fails at the first-bad commit (head).
4. The base→head production diff intersects a `syn`-extractable explicit unsafe
   function, block, or impl relation under the frozen extractor contract.
5. An evaluator maps the failing test to one or more defect root spans in the
   head tree. The mapping is independently reviewed before any model run and
   stored with source/test hashes. Ambiguous root mapping excludes the unit.

The reviewer sees only base/head production source and diff. The later fix,
added regression test, commit/PR/issue message, bisection transcript, root span,
and rationale are evaluator-private.

A negative control is the corresponding fixed production tree or a matched
clean change from the same crate/size band for which the exact hidden test
passes and the unsafe relation remains present. Controls measure unsupported
generic warnings and false positives; they are not added to the positive-unit
recall denominator.

### 2.4 Holdout population stop rule

Candidate enumeration and mechanical presence run before any model call. If
fewer than **83 eligible positive commit units** exist across the frozen ranges,
the population-powered efficacy experiment stops and reports the census and
exclusion reasons. No range widening, threshold relaxation, or favorable subset
is allowed. A feasibility study may still run on every eligible unit, but it
must not report population precision/recall or a confirmatory p-value.

This conservative `n=83` is inherited from the existing exact paired-McNemar
power analysis for a 0.30 absolute improvement, two-sided α=0.05, power 0.80,
and worst feasible discordance (`benchmarks/m7-real-v2/POWER_ANALYSIS_REPORT.md:7-9`).
Reusing it avoids an implementation-aware favorable re-powering. The final
study also draws **20 negative controls**, stratified by repository and diff-size
quartile; if 20 do not exist, the false-positive gate is descriptive only and
the overall result cannot be “practical success.”

## 3. Arms and budget equality

Each unit is run in both arms with three replicates. Replicates quantify model
variance; they do not increase `n`. Arm order and replicate seeds are generated
from one preregistered public randomization seed after unit IDs are
content-addressed.

### A — free-form baseline

- Exact same provider, model and model revision, reasoning effort, temperature,
  seed support, system safety policy, wall deadline, and output schema as B.
- Receives repository identity, base/head production diff, and a neutral request
  to find actionable correctness defects.
- May select source excerpts through a generic read-only, source-ID-returning
  interface. It receives no obligations, graph edges, projection ranking,
  capability gaps, or oracle.

### B — ReviewGraphen slice

- Receives the frozen unsafe Relation obligation and ReviewGraphen controller.
- Selects/receives subject-preserving bounded projections with source IDs,
  included/excluded/unknown/unresolved/loss records and projection hash.
- Uses the same structured claim/abstention schema and cannot execute tests or
  shell commands.

### Equal resource envelope

Per replicate, frozen before implementation:

- maximum 6 source-selection rounds;
- maximum 96 KiB admitted source bytes, counting repeated bytes again;
- maximum 30,000 input and 4,000 output tokens (provider tokenizer recorded);
- maximum 600 seconds wall time;
- no network, shell, test, commit message, issue/PR body, test diff, fix, oracle,
  or previous replicate;
- one final structured response or typed abstention.

The baseline may choose different bytes, but not more bytes/calls/time/tokens.
Both source interfaces emit source IDs and losses so budget and grounding are
auditable. Provider failure receives at most one retry under a frozen retry
class; the original failure remains in the denominator.

## 4. Frozen outcomes and denominators

### Primary outcome — missed-defect reduction

For each positive commit unit and arm, `detected=1` when at least two of three
replicates contain a grounded claim that an evaluator, blind to arm, maps to the
pre-frozen root and failure mechanism. Otherwise it is 0, including abstention,
malformed, timeout, provider failure, or completed claims outside the root.

- Denominator: exactly all 83 admitted positive commit units.
- Statistic: `mean(detected_B - detected_A)` and exact two-sided paired McNemar
  test at α=0.05.
- This is root recall for the frozen unsafe-change population, not repository-
  wide recall and not general Rust defect recall.

### Secondary outcomes — frozen, no promotion to primary

1. **False positives:** mean number of grounded issue-present claims per
   negative control that blind adjudication rejects or cannot connect to an
   independent failure anchor. Denominator is all 20 controls × 3 replicates,
   with unit-clustered intervals.
2. **Completion:** structured valid response or typed abstention within budget.
   Denominator is every scheduled replicate, including provider failure and
   timeout.
3. **Judge-positive efficiency:** blind `issue_should_be_created` claims per
   million total tokens and per model-hour. This is a quality proxy only.
4. **Reproducible unknown-region visualization:** exact equality of obligation
   denominator IDs, included/excluded/unknown/unresolved/loss records, source
   IDs, and projection hashes across two deterministic non-model rebuilds of
   every unit. Denominator is every unit × every scheduled obligation.
5. **Grounding/protocol violations:** claims with target/source IDs outside the
   envelope, output repair attempts, hidden-input marker leakage, or any tool
   event. Denominator is every response.
6. **Verification yield:** counts of `passed`, `failed`, `unsupported`,
   `inconclusive`, `timeout`, and `stale`, each over the full proposed-claim
   denominator. Passing a generic Cargo test is not automatically verification
   of the claim; the frozen descriptor/property binding must match.

No finding-count, comment-count, confidence threshold, file-read count, or judge
label alone is coverage.

## 5. Blind adjudication and truth boundaries

### Root matching

Two independent adjudicators receive a normalized candidate claim, its cited
source bytes, and the head production tree—not arm identity, obligations,
projection prose, fix, hidden test, commit/issue text, existing rationale, or
other candidates. They decide whether the claim describes the frozen failure
mechanism/root. Disagreement goes to a third adjudicator. Adjudicator IDs and
order are recorded.

### Issue-worthiness judge

A separately frozen judge model receives normalized candidate claims and the
same head source, with arm and oracle hidden. Labels are
`issue_should_be_created`, `reject`, or `unable_to_determine`. The prompt,
model/revision, settings, raw response hash, parsed label, and failures are
recorded.

**The judge is not truth.** It measures expected review usefulness. Primary
truth comes from the exact pass/fail world anchor plus independently frozen root
mapping. Judge-positive efficiency stays secondary.

### Mechanical verification

After reviewer output is sealed, the evaluator may:

1. replay the hidden exact test at the content-addressed base/head/fix;
2. run only preregistered code-owned verifier descriptors (for example pinned
   `cargo test` and, where the original project already uses it, pinned Miri);
3. bind raw stdout/stderr/exit/timeout/toolchain/source hashes as Evidence; and
4. produce typed Verification only where the descriptor's property and subjects
   match. Otherwise the outcome is `unsupported` or `inconclusive`.

No mechanical outcome directly creates a human decision or accepted finding.
Evidence from a different snapshot is stale and excluded from current sign-off.

## 6. Oracle-leakage controls

- Separate mounts: `/public-input` for reviewer packets; `/private-oracle` is
  available only to enumeration/adjudication and is never mounted in reviewer or
  implementation containers.
- Packet inventory is closed and hash-bound. Paths/content containing case-
  insensitive `oracle`, `ground_truth`, `expected`, `rationale`, issue/PR body,
  commit message, hidden-test path/name, later-fix SHA, or benchmark target ID
  markers cause pre-run rejection. The existing process input boundary already
  rejects oracle/private/ground-truth markers
  (`docs/adr/0030-generic-review-orchestration.md:69-80`).
- Only the bug-introducing base/head production trees are present. `.git`, test
  changes, later commits, benchmark directories, and historical candidate/judge
  artifacts are absent.
- Reviewer prompts are generated from frozen templates; no hand-edited unit
  rationale. All prompt and inventory hashes are published after unblinding.
- Unit aliases are random and arm-neutral. The judge sees neither original
  candidate ordering nor ReviewGraphen-specific fields.
- A leakage canary scan and packet rebuild run before every model invocation.
  Any leak invalidates the unit in both arms; it is never silently repackaged.

## 7. Preregistered success/failure decision

The slice is a **practical success** only if every gate passes:

1. **Primary efficacy:** ReviewGraphen minus baseline unit recall is at least
   `+0.30` absolute and exact paired McNemar `p < 0.05`.
2. **False-positive non-inferiority:** the upper bound of the preregistered
   unit-clustered 95% interval for `FP_B - FP_A` is at most `+0.10` claims per
   negative control.
3. **Completion:** B completes at least 90% of replicates and is no more than 5
   percentage points below A.
4. **Deterministic transparency:** 100% of non-model rebuilds reproduce the
   denominator and projection/unknown/loss hashes byte-for-byte.
5. **Boundary integrity:** zero out-of-scope source/target claims accepted by the
   parser, zero tool/shell events, zero oracle leaks, zero confidence-based
   promotion, and every report remains `trusted_pass=false` absent a separate
   human decision.
6. **Product completion:** provider-free quickstart and one real adapter each
   produce schema-valid audit JSON plus a Markdown projection; verifier
   unsupported/timeout/nonzero/stale negative cases are retained and visible.

If any gate fails, report which gate failed. Secondary judge-positive/token or
latency improvements cannot rescue a failed primary or integrity gate. If the
83-unit census fails, the allowed conclusion is feasibility/mechanism evidence
only—no population precision/recall, no confirmatory efficacy claim.

## 8. Frozen publication package

Publish after unblinding: preregistration and amendments; resolved range
manifest; enumeration/exclusion counts; unit and arm manifests; exact prompts;
model/provider/toolchain identity; raw response and verifier artifact hashes;
parsed claims; source/projection inventories; randomization; adjudicator/judge
records; analysis code; and every denominator. Private issue text or secrets may
be redacted only through a declared projection with source IDs, meaningful
information-loss records, and recovery policy.
