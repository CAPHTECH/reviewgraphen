# ADR 0046: Share exclusion-ID conformance, not implementation

- Status: Experimental implementation
- Date: 2026-09-14

## Context

Accepted-anchor discovery ranked the legacy-D and rule-neutral exclusion-ID
functions third because their normalized bodies are identical. Direct reading
confirmed the same nine-field StableId preimage and an explicit compatibility
promise, but did not establish performance preservation or the safety of a
shared implementation if the rule-neutral bindings later evolve.

The responsibility-family decision trial enumerated 15 obligations and
proposed `shared_conformance_test`: eight supports, one opposed separation
rationale and six unresolved safety obligations.

## Decision

Keep the two validation and record-construction paths separate. Add one
cross-path conformance test asserting that the overlapping changed-public-callee
input produces identical IDs and retained fields.

## Consequences

The compatibility promise is now executable without forcing the legacy and
rule-neutral types into one abstraction. A future shared-helper proposal must
first resolve the compatibility, performance and evolution unknowns rather
than using body equality as its justification.
