# Post-experiment: upstream constraint change

Status: post-experiment operator decision. This directory
(`upstream-issues/`) is deliberately **outside** `benchmarks/m7-head-local-v1/`
and is not part of that experiment's record — `preregistration.json` and
every other m7-head-local-v1 document are unchanged by this. The
experiment is complete; this is separate, subsequent work using its
outputs.

## What changed

Throughout `m7-head-local-v1`, `no_upstream_effects` was a hard
constraint: `no_issue_creation`, `no_branch_creation`, `no_commit`,
`no_patch_submission`, `no_pull_request` — see `preregistration.json`
`target.no_upstream_effects`. The operator has confirmed admin access to
`ymm-oss/fsl` and, by their own decision, lifted **one** of these five
constraints:

- **Issue creation on `ymm-oss/fsl` is now permitted**, subject to
  operator approval per issue before posting (see
  `upstream-issues/fsl-m7-head-local-v1-findings/`).

**Everything else in `no_upstream_effects` remains in force, unchanged:**
no branch creation, no commit, no patch, no pull request against `fsl`,
by this agent, under any circumstance, for this work or any future
work in this directory unless the operator lifts that constraint
explicitly and separately, the same way this one was lifted.

## Why this is recorded here, not in the experiment

The operator was explicit: this is a **post-experiment** decision, made
after `m7-head-local-v1` already concluded (final judge pass committed
at `fa142fc`). Recording it inside `benchmarks/m7-head-local-v1/` would
misrepresent it as part of that experiment's preregistered design, which
it is not — the experiment's own `no_upstream_effects` block is
preserved exactly as it was when the experiment ran.
