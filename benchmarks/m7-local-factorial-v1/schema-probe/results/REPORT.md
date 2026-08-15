# M7 local-factorial schema probe result

## Decision

Stage 1 completed all six frozen control trials. Four of six trials (66.7%)
produced a candidate that passed the exact candidate schema, trial binding, and
where applicable obligation-denominator checks. The preregistered rule stops at
four or fewer compliant outputs. Stage 2 and the sixty positive trials were
therefore not run.

This stop is based only on protocol/schema feasibility, not elapsed time.

## Observations

| arm | compliant / attempted | noncompliant observation |
| --- | ---: | --- |
| B1 | 1 / 2 | one completed process left an empty final response |
| G3-proxy | 1 / 2 | one input had 8,131,409 characters against Codex fallback metadata's 1,048,576-character maximum |
| full ReviewGraphen | 2 / 2 | none at the candidate-output layer |

Only the final full trial produced a complete successful process record during
its original execution. The first three trials produced schema-valid raw final
messages, but the then-current adapter rejected Codex's `error` item before it
distinguished that non-tool model-list diagnostic from tool events. Those
trials were not rerun and are not retroactively called successful process
records. The adapter now admits only that diagnostic item in addition to
reasoning and agent messages; actual and unknown tool items remain rejected.

Elapsed seconds are recorded per trial in `summary.json` solely for
reproduction. They did not affect selection, stopping, or corpus size.

## What was not established

No positive target trial ran. Consequently this probe did not measure whether
ReviewGraphen finds correctness defects, whether the local model finds them,
or whether scaffolding changes local-model detection. The answer to both study
questions remains **not measured** in v1.

The intended 20-unit local row would in any case be descriptive. FSL has 28
presence-eligible units while the conservative target is 83, so this repository
alone cannot support an interaction significance claim. If a future protocol
version makes local execution measurable, applying the same presence oracle
across additional Rust repositories is the recorded route toward that larger
denominator.

Any response repair, normalization, schema relaxation, G3 projection change,
or output-limit change requires a new version and preregistration. The frozen
v1 result is not rewritten.
