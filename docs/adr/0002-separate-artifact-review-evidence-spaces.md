# ADR 0002: Separate Artifact, Review, and Evidence Spaces

- Status: Accepted for v0.1 design
- Date: 2026-08-07

## Context

A code element, an AI statement about that element, and a test result supporting the statement are different kinds of object. Combining them into one graph or record makes several unsafe transitions easy:

- AI inference appears as a program fact.
- A high-confidence claim appears verified.
- A test attached to a file appears to verify every property of the file.
- A review comment appears to be an accepted finding.
- A stale source fact silently invalidates evidence without propagating state.

HigherGraphen already distinguishes structure, evidence, provenance and review status. ReviewGraphen needs a sharper operational boundary because the same target will accumulate multiple claims and evidence across snapshots.

## Decision

Use three logically separate spaces:

```text
ArtifactSpace / ProgramSpace
  accepted and inferred structure of the review target

ReviewSpace
  obligations, plans, executions, claims, decisions, coverage, obstructions

EvidenceSpace
  tests, traces, static results, witnesses and evidence bindings
```

Cross-space relations use stable references and explicit relation types such as `reviews`, `targets`, `supports`, `refutes`, `depends_on` and `verified_by`.

Physical storage may share one event log or database, but logical schemas and validation boundaries remain distinct.

## Consequences

### Positive

- Accepted facts, inferences and evidence cannot be confused by default.
- Multiple reviewers and evidence items can coexist for one target.
- Staleness can propagate through explicit dependencies.
- Audit and research exports can select each layer independently.
- Human decisions remain visible rather than overwriting claims.

### Negative

- More IDs, references and cross-record validation are required.
- Query and report construction are more complex.
- A simple finding requires a trace chain rather than one object.
- Store migrations must preserve cross-space referential integrity.

## Alternatives considered

### A. One property graph with type labels only

Rejected as the public conceptual model. It can be a storage implementation, but type labels alone do not enforce authority, lifecycle and acceptance boundaries.

### B. Embed evidence inside claims

Rejected. Evidence may support multiple claims, have independent validity and become stale separately.

### C. Store only final findings

Rejected. This loses uncovered obligations, abstentions, rejected claims and the basis for confidence in a finding.

## Invariants

- A ProgramSpace fact cannot be created by an LLM reviewer without inferred/candidate provenance.
- A ReviewClaim cannot become accepted only because evidence exists; a policy or decision is required.
- A Verification references a claim, verifier and scoped evidence.
- A Finding projection traces to at least one claim.
- Cross-space dangling references are invalid.

## Revisit triggers

- Cross-space separation makes required queries impractical and a typed single-space model can enforce the same invariants.
- A future HigherGraphen primitive provides a stronger native multi-space authority model.
