# ADR 0005: LLM Output Is a Reviewable Claim

- Status: Accepted for v0.1 design
- Date: 2026-08-07

## Context

LLM output is probabilistic, sensitive to prompt/context and capable of unsupported conclusions. A second model agreement does not turn a statement into a program fact. At the same time, LLMs can identify semantic risks that deterministic tools cannot express.

The system needs to preserve LLM utility without granting it silent epistemic authority.

## Decision

Every parsed LLM conclusion enters ReviewSpace as a `ReviewClaim` with `proposed` disposition unless an explicit policy says otherwise for a narrowly defined operation.

A claim records:

- obligation IDs and polarity;
- source grounding;
- rationale and limitations;
- reviewer/model/prompt/Envelope versions;
- raw output artifact hash;
- candidate confidence, if supplied;
- required evidence or verification next step.

Acceptance, verification and finding projection are separate operations. Confidence alone never changes disposition or verification outcome.

## Consequences

### Positive

- Hallucinations cannot silently become accepted facts.
- Different reviewer claims can coexist and conflict.
- Verification strategies can be evaluated separately from generation.
- Human decisions and policy exceptions remain explicit.
- Raw model output can be retained or deleted without losing canonical structure.

### Negative

- The workflow is less immediate than posting comments directly.
- Many claims remain inconclusive or unsupported.
- Parsing and grounding validation may reject useful prose.
- Human/policy decision surfaces are required for accepted findings.

## Alternatives considered

### A. Auto-accept claims above a confidence threshold

Rejected. Model confidence is not a proof or calibrated authority across properties.

### B. Use majority vote among agents

Rejected as acceptance policy. Correlated models/prompts can agree on the same unsupported claim.

### C. Treat model output as advisory text only

Rejected because it loses structured provenance, evidence binding, staleness and coverage progression.

### D. Require deterministic proof for every claim

Rejected as too restrictive. Some review judgments are semantic and require human or defeasible evidence.

## Invariants

- Parse failure never becomes `issue_absent`.
- A proposed claim cannot mutate ProgramSpace accepted facts.
- Verification outcome references a verifier and scope.
- Acceptance references an authority/policy and rationale.
- Model confidence and severity are separate values.
- A rejected or superseded claim remains in history.

## Revisit triggers

- A formally verified model/tool class can produce proof-carrying claims under a defined property.
- Organization policy grants a deterministic analyzer direct accepted-fact authority for a bounded record type.
