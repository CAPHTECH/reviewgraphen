# ADR 0031: Discriminating M7 Real-Regression Benchmark

- Status: Accepted
- Date: 2026-08-15

## Context

The four measured `m7-real-v1` conditions each detected one of twenty known
regressions. The observed floor cannot distinguish an ineffective scaffold
from an underpowered corpus. Finding volume also fell from 115 to 47 and then
to 14/17 without a defect-level cross-arm reconciliation, so those counts do
not distinguish noise removal from missed valid defects.

Selecting final units because B1 detected them would condition the comparison
on B1 success. Reusing any unit already shown to a model would also contaminate
the final measurement. A replacement therefore needs a disposable calibration
sample, an untouched holdout, an a-priori power target, and defect-level blind
cross-arm adjudication.

## Decision

`m7-real-v2` is additive and never rewrites an earlier corpus or result.

### Candidate frame and mechanical eligibility

The frame is the read-only FSL first-parent, non-merge history dated
2026-06-11 through 2026-08-09 inclusive. A candidate is eligible only when all
of the following are recorded before any B1 outcome is observed:

1. its fix OID is absent from every `m7-real-v1` unit;
2. it has at least one production Rust hunk and an added regression test;
3. the exact added test can be backported to the parent without production fix
   hunks;
4. the same argv and relative working directory exit nonzero on that parent
   and zero on the fix;
5. a code-only location oracle can be cut from a production fix hunk; and
6. blind source projection can exclude tests, commit/issue/branch metadata,
   private oracles, and prior candidate outputs.

Eligibility failure is terminal and typed. Candidates are never replaced as a
function of a reviewer outcome.

### Calibration/holdout separation

After eligibility, candidates are ordered by
`SHA-256("reviewgraphen.m7-real-v2.split.v1\\0" || raw_fix_oid)`. The first 30
are calibration-only; every remainder is holdout. The split salt, size, and
ordered OIDs are published before B1 runs. Calibration units are permanently
ineligible for the final corpus.

B1 runs twice on every calibration unit. Both runs use independently derived
opaque trial IDs but the same arm-neutral production projection. No diff,
regression test, commit message, issue, branch, oracle, mechanism label, or
prior output enters either packet.

The feature extractor is frozen before outcomes and emits only:

- production changed lines, file count, hunk count, and crate count;
- whether production changes cross crates;
- the largest production file's fraction of changed lines;
- regression-test changed lines and the test/production changed-line ratio;
- projected production file count and byte count.

Messages, issue data, branch data, test contents, oracle roots, mechanism
ontology, candidate text, and model outcomes are prohibited features.

A binomial logistic model with a fixed L2 penalty of 1.0 is fit only to the 30
calibration units, with two target-detection trials per unit. Numeric features
are standardized using calibration statistics only. Holdout units with a
predicted B1 probability outside `[0.40, 0.70]` are ineligible; eligible
holdout units are selected only by the already-frozen split hash. The band is
never widened after outcomes. If calibration has no out-of-sample predictive
signal or the band contains fewer than the powered sample size, the benchmark
stops as infeasible.

### Power contract

Before selecting final units, the benchmark computes exact two-sided paired
McNemar power at alpha 0.05 for a 0.30 absolute detection-rate improvement and
power 0.80. The calculation publishes the assumed B1 rate, scaffold rate, both
discordant-cell probabilities, and sensitivity over every admissible
discordance range. The final `n` is the smallest integer meeting the declared
conservative case. Unit-replicate reuse does not count as additional `n`.

### Final arms and Q2 reconciliation

Every final unit has one positive parent and one matched-fix control under ADR
0026. B1, G3-proxy, and full ReviewGraphen receive arm-neutral snapshots under
one replicate and fixed model configuration. Positive target detection remains
the Q1 endpoint.

For Q2, all non-target findings are first linked within the same snapshot.
Deterministic candidate edges use normalized overlapping/nearby locations and
mechanism intersection. A role- and arm-blind linkage adjudication resolves
ambiguous components. A separate blind defect adjudication labels each linked
cluster `valid_novel_defect`, `false_positive`, `insufficient`, or `duplicate`.
Private reconciliation restores the arm-emission bitmap only after decisions.
The report decomposes findings omitted by full ReviewGraphen into valid defects
missed, false positives removed, duplicates collapsed, and unresolved items.

Model dispositions remain research labels, not accepted facts, verified
evidence, or human acceptance.

## Consequences

- A successful run can test a 30-point paired recall improvement at the
  declared power and can quantify whether full-only omissions are noise or
  valid missed defects.
- A failed calibration or insufficient holdout is reported as infeasible; it
  is not repaired by post-outcome corpus selection.
- The study still estimates performance on retrospectively fixed defects, not
  discovery prevalence for unknown future bugs.
- Existing `m7-pilot-v1`, `m7-pilot-v2`, `m7-real-v1`, and full ReviewGraphen
  replicate results retain their original meanings.
