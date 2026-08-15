# M7 HEAD v1 — Step 1 verifier calibration result

- Date: 2026-08-15
- Repository under review: `/home/rizumita/github/fsl` (read-only)
- Calibration source: the 20 positive-parent units frozen by `m7-real-v1`
- Backend: Codex CLI `codex-exec@0.147.0`
- Model: `gpt-5.6-sol`
- Reasoning effort: `high`
- Semantic attempts: one per unit
- Decision: **stop before production review**

This experiment is software quality assurance for correctness defects in an
owned Rust project. All work remained local to the owned repositories. No
upstream report, issue, patch, or pull request was created.

## Pre-registered success criterion

The generator had to produce a new regression test and a production-only fix.
A unit counted as verified only when the exact generated test:

1. built and failed at runtime on the known defective parent;
2. built and passed on the canonical fix revision;
3. built and passed on the parent with the generated production fix; and
4. remained byte-identical when the generated fix was applied.

Compilation failure did not count as evidence of the known correctness defect.
The pre-registered continuation threshold was at least 9 verified units out of
20.

## Measured result

| Disposition | Count |
| --- | ---: |
| verified | 1 |
| not_verified | 6 |
| unverifiable | 13 |
| total | 20 |

The one verified unit was `calibration-12`. Failure reasons were:

| Reason | Count |
| --- | ---: |
| proposed patch could not be parsed by `git apply --numstat` | 9 |
| proposed patch failed `git apply --check` | 4 |
| generated test did not pass on the canonical fix | 4 |
| generated test did not build on the parent | 1 |
| generated test did not build on the canonical fix | 1 |

The observed verified rate was 1/20 (5%). The continuation criterion was not
met, so `proceed_to_head_review` is false. Once 14 units had been processed,
even six successes in the six remaining units could only reach 7/20; all 20
were nevertheless completed to measure the failure distribution.

This result shows that this generator/verifier composition did not function
well enough on the known-defect calibration set to support production
measurement. It does not establish that regression-test generation is
impossible in general, nor does it measure the yield of B1, G3-proxy, or full
ReviewGraphen on FSL HEAD.

## Infrastructure attempt excluded from the denominator

The first invocation of `calibration-01` failed before model generation
because the generated provider schema omitted explicit string types for two
constrained fields. The provider returned `invalid_json_schema`. The schema
was corrected without changing its accepted values, the failed invocation was
recorded as an infrastructure attempt, and the fixed 20-unit run was executed.
It did not consume a semantic attempt.

There were no provider content-classification refusals in the fixed run. All 20
records used one symmetric condition: Codex CLI, `gpt-5.6-sol`, high effort,
and `reviewgraphen.process_reviewer.bwrap-no-tools.v1`.

## Preserved evidence

Private calibration evidence is under
`private/calibration/`:

- `calibration-inventory.private.json`: exact unit, revision, packet, and
  reviewer-visible input hashes;
- `generation/`: all 20 non-authority process records, raw responses, exit
  statuses, and adapter logs;
- `verification/`: generated proposals and patches, command-output bytes and
  hashes, per-unit dispositions, and `calibration-summary.json`.

The separate
`private/calibration-infrastructure-attempts/attempt-01-invalid-provider-schema/`
directory preserves the excluded infrastructure failure.

## Consequence for later steps

Per the stop condition, Steps 2–5 were not run. In particular:

- no B1/G3-proxy/full ReviewGraphen review of FSL HEAD was started;
- no current-HEAD candidate was classified;
- no `.fsl` specification matching was performed; and
- no per-arm precision, overlap, or model/mechanical-disposition comparison can
  be reported.

Those quantities remain **not measured**, rather than zero.
