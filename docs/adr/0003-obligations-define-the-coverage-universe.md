# ADR 0003: Obligations Define the Coverage Universe

- Status: Accepted for v0.1 design
- Date: 2026-08-07

## Context

“Reviewed the repository” has no engineering meaning unless the denominator is known. File counts, prompt calls, comments and tokens do not say which properties or relations were considered. AST node traversal improves enumeration but still misses relation, path and invariant responsibilities.

Coverage must answer both:

1. What was required for this snapshot/profile?
2. How far did each required item progress?

## Decision

A versioned `ReviewObligation Universe` is the denominator for review coverage.

An obligation binds:

```text
target
property
context requirement
evidence requirement
risk and applicability
profile/rule/extractor/snapshot provenance
```

The universe descriptor fixes snapshot, profile, rule set hash, extractor capability set, policy and exclusions. Coverage is reported across separate stages: generated, visited, completed, evidence-supported, verified and fresh-verified.

Raw count and risk-weighted coverage are always reported together where weighting is used.

## Consequences

### Positive

- Unseen work becomes explicit.
- Different runs can be compared against a declared denominator.
- Node, relation, path and invariant coverage remain distinguishable.
- Budget scheduling can optimize over an explicit frontier.
- Coverage does not depend on reviewer self-report.

### Negative

- Obligation generation becomes a new source of omissions.
- Universe changes complicate trend comparison.
- Rules and applicability must be versioned and explainable.
- Large repositories may produce too many low-value obligations.

## Alternatives considered

### A. File × rule matrix

Useful as a baseline but rejected as the full model. Semantic relations, execution paths and global invariants do not map cleanly to files.

### B. AST node coverage

Rejected as sufficient coverage. It enumerates syntax but not interaction properties.

### C. Free-form agent exploration with visited-file log

Rejected. It records where the agent went, not what responsibility was discharged.

### D. Finding count

Rejected. Finding count rewards noise and has no denominator.

## Invariants

- Every coverage value references one universe ID.
- Percentage is never stored without numerator and denominator or explicit scalar semantics.
- Exclusions are versioned records, not silent deletion.
- Missing extractor capability is a limitation/obstruction, not zero applicable obligations.
- Completed does not imply evidence-supported or verified.
- Current gate uses fresh coverage.

## Revisit triggers

- Empirical evaluation shows obligation-generation omissions dominate all benefits.
- A more suitable denominator, such as property-state pairs or proof obligations, subsumes the current model.
