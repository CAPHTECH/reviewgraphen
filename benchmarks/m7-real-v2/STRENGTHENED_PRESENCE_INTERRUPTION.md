# Strengthened-presence interruption record

Status: intentionally interrupted on 2026-08-15 by an explicit experiment
policy change.

The additive strengthened-test supplement contained 14 mechanically runnable
candidates. Eleven completed before interruption: five satisfied the three
observation oracle and six did not. The twelfth candidate was in progress and
has no retained result; candidates twelve through fourteen are unprocessed.
The index therefore records `complete=false`, `processed_count=11`,
`eligible_count=5`, and `ineligible_count=6`.

The run was stopped because ADR 0031's prospective power analysis already
established that the known-fix design requires a conservative final `n=83`,
while the original strict-presence population was 28. Incrementally enlarging
that population cannot make the preregistered calibration-plus-holdout design
feasible. No completed result was used to select or replace another candidate.

The retained partial evidence is in
`private/presence-strengthened-interrupted/`. Its index SHA-256 is
`d56b51a2fe9dab78a52051203f56d0340774a179a083a3a5ebf08851e3e2f384`.
The artifact-tree SHA-256 is
`3ff1f18b7da48046e271963c6e7cec7e52cb4eaa8b941e7fdd316250a8b5e8c9`,
computed from sorted `sha256sum` records over relative paths.

This partial run is not a corpus, calibration sample, powered result, or source
of model outcomes. Existing M7 artifacts remain unchanged.
