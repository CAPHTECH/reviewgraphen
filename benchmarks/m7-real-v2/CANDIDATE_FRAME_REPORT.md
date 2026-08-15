# M7 real v2 candidate-frame report

- Source head: `e589014d1655b1224f5b83a7f2de99532a0dcdba`
- Source status: clean and read-only
- History window: 2026-06-11 through 2026-08-09
- Prior `m7-real-v1` fix exclusions: 20
- First-parent records inspected: 635
- Structural candidates before presence execution: 84
- Excluded before presence execution: 551
- Added exact integration-test options across the 84 candidates: 426

The structural filter required a fix-prefixed commit subject, at least one
production Rust path present in both parent and fix, at least one changed Rust
integration-test file, at least one newly added exact test function, and a
successful blind projection for both revisions. Commit subjects are retained
only in the private frame for audit and are not model inputs or difficulty
features.

This is not yet the mechanically eligible set. Each of the 426 exact test
options must still be tried under the frozen parent-fails/fix-passes contract.
The calibration/holdout split will be applied only after that presence step.
With 84 structural candidates, the absolute upper bound is 54 holdout units
after the preregistered 30-unit calibration set; presence failures can only
lower it. Feasibility therefore remains unresolved at this stage.

Two independent enumerations produced byte-identical `candidate-frame.json`.
