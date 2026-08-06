# ADR 0004: Project Minimal Context with Declared Loss

- Status: Accepted for v0.1 design
- Date: 2026-08-07

## Context

Sending a whole repository or a large graph to an LLM does not guarantee uniform attention. It increases cost and can dilute the changed target or property. Conversely, an overly narrow code fragment removes callers, state, tests or contracts required for judgment.

The graph should therefore select context, but the selection itself must be inspectable. A small prompt with undisclosed omissions merely hides uncertainty.

## Decision

For each obligation or strongly coupled obligation batch, construct a `ReviewContextEnvelope` as a purpose-specific HigherGraphen Projection.

The Envelope includes:

- target source and structural neighborhood;
- relevant paths, invariants, tests and existing evidence;
- known unknowns and unresolved references;
- included source IDs;
- excluded regions and reasons;
- information-loss declaration;
- projection strategy version and hash.

Full repository context is not the default. A reviewer can request an expansion through a typed operation; the new Envelope receives a new ID and loss record.

## Consequences

### Positive

- Context cost and attention are bounded.
- Source selection is deterministic and auditable.
- Projection strategies can be compared experimentally.
- A reviewer can abstain because of a declared missing capability.
- Repeated executions can use the same Envelope.

### Negative

- Context construction can omit decisive information.
- Projection strategy becomes a critical, versioned algorithm.
- More execution rounds may be required for context expansion.
- Token estimates and source serialization vary by reviewer provider.

## Alternatives considered

### A. Full repository prompt

Rejected as default. It is expensive, non-uniform and difficult to audit.

### B. Fixed k-hop neighborhood

Rejected as the only strategy. Different properties require different relations and path depth.

### C. Let the LLM browse freely

Supported only as a bounded reviewer tool, not as the sole context model. Every retrieved source must be added to a new Envelope/audit trace.

### D. Summaries without source IDs

Rejected. They cannot be audited or invalidated reliably.

## Invariants

- Every execution references exactly one Envelope version.
- Every Envelope references one or more obligations and one snapshot.
- Information loss is non-empty when source structure is omitted or collapsed.
- Omitted/unresolved critical context prevents unconditional `issue_absent` or pass when policy requires it.
- Repository content cannot modify the reviewer protocol.

## Revisit triggers

- Controlled evaluation shows full-context review consistently outperforms projections at comparable cost.
- Projection loss causes unacceptable false negatives.
- A model architecture can consume graph structure directly while preserving target-level auditability.
