# ADR 0026: M7 Real Regression Corpus Contract

- Status: Accepted
- Date: 2026-08-13

## Context

The injected pilot corpus uses an empty-root control: every candidate on that
control can be reported as a false-positive proxy. A real fix commit has a
narrower interpretation. It establishes that one known regression is absent,
but unrelated defects can remain. Treating every finding on the fix revision as
a false positive would turn incomplete ground truth into an incorrect precision
claim.

The existing benchmark artifact family is already frozen as protocol v2 and is
bound into the checked-in `m7-pilot-v2` result. The real corpus must not mutate
that meaning or retroactively reinterpret its scores.

## Decision

`m7-real-v1` uses an additive artifact family:

- `real_unit.v1` privately binds one fix commit to its first parent, two opaque
  trial-unit IDs, exact commit/tree hashes, the frozen mechanism ontology, and
  machine-recorded regression-test evidence. It also binds one shared selected
  production-path set and projection-policy version plus each revision's exact
  source-inventory hash. The positive snapshot is the parent and the control
  snapshot is the fix; neither is a base-to-head change-review unit.
- `regression_presence_evidence.v1` requires the same argv and relative working
  directory on both revisions, a nonzero parent exit status, a zero fix exit
  status, the backported fix-test source hash, and stdout, stderr, and combined
  artifact hashes. This is evidence that the selected regression is present in
  the parent and absent in the fix; it is not evidence that either revision has
  no other defects.
- `real_oracle.v1` is private. A positive oracle contains one or more target
  roots taken only from production-code fix hunks. A matched-fix control has no
  target root and instead carries code-hunk scope anchors for classifying a
  candidate as a target-area allegation. Every anchor binds the exact tree,
  file hash, symbol, line range, span hash, and mechanism IDs.
- `real_trial_inventory.v1` privately maps opaque trial IDs to positive/control
  roles and requires all four cells (two revisions by two arms) for each
  replicate. Role, commit message, issue metadata, branch name, regression test,
  oracle, and reconciliation never enter reviewer input.
- `real_score.v1` measures target-root detection only on the positive revision.
  On the control revision, every candidate remains
  `unlabeled_requires_adjudication`, including candidates overlapping a target
  scope anchor. The latter are counted as target-anchor allegations, never as
  false positives or known defects.
- `real_run_summary.v1` reports the positive target denominator and paired arm
  delta separately from control unlabeled-findings and target-anchor-allegation
  counts. It has no precision or false-positive field.

Reviewer packets require a separate deterministic `prepare-real` path. It must
project the selected production files from exactly one target snapshot, derive
ProgramSpace and G3 obligations from that same snapshot, and give B1 the same
source projection under a neutral packaging transform. It omits change diffs,
commit metadata, issue metadata, branch names, and regression tests. The pilot
`prepare-pilot` base/head path is not eligible for real units.

The shared arm-neutral `candidate_output.v1`, packet-ID equality checks,
collector-owned protocol-invalid outcome, blind adjudication IDs, protocol v2,
and `mechanism_ontology.v1` remain unchanged. The eight-ID ontology is assessed
against selected real bugs before any extension. If extension is later required,
a new ontology version and compatible artifact boundary are required; existing
pilot-v2 results remain interpreted under ontology v1.

## Consequences

- Pilot-v2 manifests, oracles, scores, summaries, and raw results remain valid
  without migration. They must not be parsed as real-corpus artifacts.
- A real positive recall denominator is justified by machine-recorded
  parent-fails/fix-passes evidence plus a code-only location oracle.
- Control findings measure reviewer output under target absence but do not
  estimate repository-wide precision without separate blind adjudication.
- These records are non-authority research artifacts and cannot enter accepted
  ProgramSpace facts, Review claims, Evidence, findings, coverage, or sign-off.
