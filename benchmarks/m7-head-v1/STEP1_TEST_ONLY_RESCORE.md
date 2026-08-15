# M7 HEAD v1 — Step 1 test-only rescore

- Date: 2026-08-15
- Rescore ID: `test-only-rescore-1`
- Frozen source attempt: `private/calibration/`
- Semantic attempts added: 0
- Pre-registered continuation threshold: 13/20
- Measured result: 2/20
- Decision: **stop before production review**

## Why this additive rescore exists

Attempt 1 required both a generated regression test and an applicable generated
production fix. It produced 1 verified, 6 not-verified, and 13 unverifiable
results. Thirteen of the 19 failures stopped at unified-diff handling: nine at
`git apply --numstat` and four at `git apply --check`.

After seeing that result, the experiment changed its construct: measure whether
the already-generated test discriminates the known defective parent from the
canonical fix, without measuring diff formatting or generated-fix quality.
This was a post-hoc construct-validity correction. It was not part of the
attempt-1 preregistration, and this report does not present it as if it were.
Attempt 1 and all of its private evidence remain unchanged.

ADR 0034 and `rescore-preregistration.json` fixed the new reconstruction
contract and 13/20 threshold before this rescore was executed. No model was
called again.

## Test reconstruction

All 20 frozen records contained one nonempty generated-test patch. The closed
added-file parser reconstructed 20/20 test sources without `git apply` or
manual repair. Seven patches declared a hunk line count different from the
actual added-line count; this mismatch was recorded but, as preregistered, did
not alter the reconstructed source.

Each exact source was then:

1. built and run on the known defective parent; and
2. built and run on the canonical fix.

A result counted as verified only when the parent build succeeded and the test
failed at runtime, while the canonical-fix build and test both succeeded.
Compilation failure never counted.

## Measured result

| Disposition | Count |
| --- | ---: |
| verified | 2 |
| not_verified | 18 |
| unverifiable | 0 |
| total | 20 |

The verified units were `calibration-12` and `calibration-14`. Failure
reasons were:

| Reason | Count |
| --- | ---: |
| generated test did not pass on the canonical fix | 14 |
| generated test did not build on the canonical fix | 4 |

The observed test-only success rate was 2/20 (10%), below the preregistered
13/20 (65%) threshold. `proceed_to_head_review` is false.

Of the 13 attempt-1 cases blocked by diff handling, all 13 became mechanically
executable under the additive-source reconstruction, but only
`calibration-14` became newly verified. Diff formatting therefore did block
the original measurement path, but removing it increased verified yield by
only one unit, from 1/20 to 2/20. The other twelve formerly format-blocked tests
did not satisfy the canonical-fix observation.

This measures the frozen generator under this calibration and prompt. It does
not establish a general upper bound for all regression-test generators.

## Preserved evidence

The additive private evidence is under
`private/calibration-rescore-test-only/`:

- `rescore-summary.json`: complete deterministic aggregate and all result
  references;
- `results/`: 20 per-unit dispositions, source hashes, hunk-count observations,
  commands, exit statuses, and output hashes; and
- `artifacts.tar.gz.base64`: exact reconstructed test sources, replayed
  proposal JSON, and Cargo stdout/stderr, with archive hashes in
  `artifacts-archive.json`.

The summary SHA-256 was
`29082601d83a02d9accf9a51fc751fc04c4432c23d3d90a8343084ff7412ee1f`
both before and after deterministic regeneration.

## Consequence

The continuation threshold was not met, so Steps 2–5 were not run. No
B1/G3-proxy/full ReviewGraphen current-HEAD review, current-HEAD mechanical
classification, `.fsl` specification matching, per-arm precision, or overlap
measurement was performed. Those quantities remain **not measured**, not zero.

The separate production canonical-fix trust-root gap identified by ADR 0034 was
therefore not exercised.
