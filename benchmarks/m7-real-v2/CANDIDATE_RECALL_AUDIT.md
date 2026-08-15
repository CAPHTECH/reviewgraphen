# M7 real v2 candidate-recall audit

Date: 2026-08-15. Reviewer outcomes observed during this audit: none.

## Result

The frozen candidate frame did not exhaust every regression-test form in the
FSL history. It exhausted newly named Rust integration-test functions among
subjects beginning with `fix`, but omitted strengthened existing integration
tests and inline unit tests.

| Population | Count |
| --- | ---: |
| Frozen newly named integration-test candidates | 84 |
| Additional strengthened existing integration-test candidates | 15 |
| Additional inline-unit-test candidates | 6 |
| Inline-only after overlap with strengthened candidates | 3 |
| Union upper bound over the three identified forms | 102 |
| Non-`fix` subjects with production changes and added integration tests | 75 |

The 15 strengthened-test candidates can use the three-observation oracle in ADR
0032 and are the next mechanical supplement. The three inline-only candidates
are not eligible until production/test code can be separated mechanically.
The 75 non-`fix` subjects are not treated as defect fixes merely because a new
test fails on their parent; many are feature additions where that behavior is
expected.

## Impact on feasibility

The audit corrects the earlier statement that the first 84 candidates were the
entire mechanically accessible history. Nevertheless, even the optimistic 102
candidate union is below ADR 0031's minimum of 113 presence-eligible units (30
calibration plus 83 conservative powered holdout units), before any presence or
difficulty-band exclusions. Thus ADR 0031 remains infeasible, but the additional
forms are useful evidence for designing a successor without outcome-driven
subject relabeling.

The machine-readable audit in `private/candidate-recall-audit.json` retains the
candidate OIDs and exact test selectors. It is private selection evidence and
must not enter reviewer packets.
