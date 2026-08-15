# M7 HEAD Issue-Worthiness Study v1

This additive benchmark measures cross-family AI dispositions about whether
static-review findings merit local issue tracking. The label
`issue_should_be_created` is a model disposition only. The benchmark never
creates an upstream issue, branch, commit, patch submission, or pull request.

The experiment replaces the failed test-generation verifier as the production
endpoint. It does not rewrite `m7-head-v1`: both its original 1/20 calibration
and post-hoc test-only 2/20 rescore remain the recorded reason for changing the
measurement instrument.

Execution is gated by a 40-case matched judge calibration built from the twenty
presence-verified `m7-real-v1` defects. Production begins only if both Codex
and Claude judges meet every preregistered sensitivity, specificity, balanced
accuracy, and unable-rate threshold.

If calibration passes, every production arm uses the same two generator
families and opposite-family judges:

| Generator | Judge |
| --- | --- |
| Codex CLI | Claude CLI |
| Claude CLI | Codex CLI |

The judge input omits arm and generator identity. All model outputs remain
non-authority observations with byte-exact process records.
