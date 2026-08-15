# M7 HEAD issue-worthiness study: interrupted

- Decision date: 2026-08-15
- Status: stopped before judge calibration execution
- Last implementation commit: `ec073c7`

The operator replaced this study with a preregistered 2×3 model-capability by
review-scaffold experiment. The new primary endpoint returns to the
mechanically scored known-target detection outcome in `m7-real-v1`; it does not
depend on an AI issue-worthiness disposition.

The attempted calibration runner invocation was refused before it completed a
model call. A post-interruption process and output audit found no calibration
runner/reviewer process and no record under
`/tmp/m7-head-issue-calibration-runs`. Consequently this study has no judge
calibration observations, no calibration score, and no production result. It
must not be described as a calibration failure or success.

The accepted ADR, preregistration, deterministic packet builder, scorer,
isolated runner, and schema are retained as a record of the abandoned design.
They are not inputs to the replacement experiment unless a later decision says
so explicitly.
