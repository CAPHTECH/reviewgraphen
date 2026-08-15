# M7 real v2 presence and calibration-stage report

Status: stopped at the preregistered presence threshold on 2026-08-15.

## Outcome

The frozen candidate frame contained 84 structural candidates. The strict presence runner processed all 84 sequentially and found 28 eligible and 56 ineligible candidates, with no infrastructure-error result in the retained run. The preregistration requires at least 30 presence-eligible candidates before assigning the first 30 to calibration. Therefore no calibration unit was assigned, no B1 calibration trial was run, and no holdout unit was assigned.

This result does not answer whether B1 lies in the target 40--70% difficulty band. The calibration stage did not reach model execution. It establishes that the frozen FSL date range, unused-fix exclusions, structural screen, and strict mechanical presence oracle yield too few candidates for the preregistered two-stage design.

## Mechanical presence result

| Quantity | Observed |
| --- | ---: |
| Structural candidates | 84 |
| Processed | 84 |
| Presence eligible | 28 |
| Presence ineligible | 56 |
| Infrastructure errors in retained run | 0 |
| Calibration assignments | 0 |
| Holdout assignments | 0 |

Among the 56 ineligible candidates, 44 had fix-side exact tests pass and the same exact tests also pass on the parent, 11 had at least one fix/parent prebuild precondition failure, and one had a fix-side runtime failure. These are exclusive candidate-level categories used only to explain exclusion; they do not alter the preregistered oracle.

The retained runner first builds each affected integration-test target with `cargo test --no-run` on both fix and parent. It executes exact tests only when both builds succeed. Presence requires an exact added regression test to pass on the fix and fail at runtime on the parent; timeout and build failure do not count as presence.

## Reproducibility record

- Source repository: `/home/rizumita/github/fsl`, read-only and clean after execution.
- Frozen candidate frame: `private/candidate-frame.json`.
- Retained index: `private/presence/presence-index.json`.
- Retained index SHA-256: `61c0a27f4013b8877e47ac80866931a78b2513e28d78adbbfd10606206363a99`.
- Retained artifact-tree SHA-256: `7a394df68678ff3ff3e1c5d251e8d5479df5a688c400faf111528b29dcca5914`, computed from sorted `sha256sum` records over relative file paths.
- Cargo: `cargo 1.95.0 (f2d3ce0bd 2026-03-21)`.
- Rustc: `rustc 1.95.0 (59807616e 2026-04-14)`.
- Build concurrency: `CARGO_BUILD_JOBS=2`; candidates were processed sequentially.

The two infrastructure-invalid attempts and the superseded no-prebuild attempt are recorded in `private/invalid-presence-attempts.json`; zero eligibility decisions were reused from them.

## Consequence for the benchmark

The preregistered calibration stage has failed its minimum-population condition. Proceeding to difficulty modeling, power analysis based on a calibrated holdout, corpus construction, or three-arm execution would silently change the design and would not produce the requested unbiased measurement instrument. A successor design needs a larger disjoint source population (for example a wider repository/date scope or another repository) and must preregister that scope before observing B1 outcomes.

No existing M7 result bundle was changed.
