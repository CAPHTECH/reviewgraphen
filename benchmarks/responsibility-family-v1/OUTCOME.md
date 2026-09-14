# ReviewGraphen responsibility-family research outcome

Date: 2026-09-14. Scope: this repository and the experimental benchmark-only
responsibility-family surface.

## Verified use

ReviewGraphen is useful here as a bounded maintenance loop:

1. enumerate exact-body candidate signals from accepted snapshot anchors;
2. assess common contract, common change reason and purpose-specific limits;
3. choose among shared validator, shared conformance or intentional separation;
4. keep the proposal non-authoritative until an external decision;
5. execute tests and mutations outside the proposal generator;
6. retain implementation denominator, unknowns and snapshot-to-snapshot
   reinspection obligations.

It is not validated as a general semantic clone detector or as an autonomous
family acceptance mechanism.

## Requirement audit

| Requirement | Current evidence | Result |
| --- | --- | --- |
| Common contract and purpose-specific constraints | Five path members bind one base path contract and distinct drive-prefix, backslash, NUL, length and typed-error constraints | represented |
| Compare three maintenance options | The closed decision table emits `shared_validator`, `shared_conformance_test`, `intentional_separation` or `inconclusive` from 3 family plus 6/member obligations | implemented and tested |
| Repeat cross-cutting mutations | Path fixture historical run changed 4/10 caught to 10/10; one fresh M6 mutation was reproduced. Severity and exclusion-ID tests each caught one fresh mapping/preimage mutation | mutation-sensitive in checked cases |
| Preserve typed errors, compatibility and performance constraints | These are explicit per-member obligations. Path policy retained separate implementations; severity shared only the allocation-free text mapping; exclusion ID stayed separate while adding compatibility conformance | checked only at the stated source/test scope |
| Hold denominator, unknowns and reinspection state | Candidate, decision universe, assessment, proposal and before/after family states bind snapshot, extractor, members, unknowns and hashes. Severity reinspection recorded 2 implementations becoming 1 | implemented and deterministic |
| Discover an unknown duplicate | `discover-exact` produced 51 groups from 1,861 eligible functions and surfaced the previously unregistered exclusion-ID pair at rank 3; it led to ADR 0046 and a mutation-sensitive conformance test | demonstrated once |
| Do not confuse syntax with responsibility | Body-equal groups remain candidate-only. Top-ranked v4/v5 and `#[cfg(test)]` copies were not automatically abstracted | preserved |
| Update operator skill | The skill includes exact discovery, external near-clone provenance, decision, reinspection, denominator and test-scope-loss guidance; quick validation passed | complete |

## Three observed decisions

- Five path validators: `shared_conformance_test`. Their common base predicate
  changes together, but purpose-specific validation and error contracts oppose
  one shared validator.
- Two Severity wire mappings: `shared_validator`. Both used the same public
  enum, strings and change reason; `Severity::as_str` removed the duplicated
  match while leaving writers separate.
- Two exclusion-ID preimages: `shared_conformance_test`. The overlap must remain
  byte-compatible, but future rule-neutral evolution, performance and shared
  implementation safety were unresolved.

These differing outcomes are the important result: exact similarity does not
force one abstraction response.

## Discovery measurements

The c074761 accepted-anchor run was deterministic across two executions. Input
SHA-256 was
`02fa8e49b5b3be356e5f8de5272a1133159183c906155bd3502f218a782f83ba`;
report SHA-256 was
`99a48b1290b8e7db9a474d6d4e5d6f6bb15cd2c1d1ef275e2f5e0e0cb8d14659`.

- accepted Rust anchors: 23,369;
- eligible functions: 1,861;
- exact-body candidate groups: 51;
- candidate members: 123;
- unknown test scope excluded: 4,698.

The known Severity duplicate ranked 26. The actionable exclusion-ID candidate
ranked 3. Ranks 1 and 4 exposed a fact gap: standalone `#[cfg(test)]` helpers
can have `test_function=false`. Therefore rank is triage, not confidence or
severity.

## Remaining unknowns

- [U] Recall for non-exact Type-2/Type-3 responsibility duplication is not
  measured.
- [U] Method coverage is weak because accepted method facts lack test-scope
  classification in this snapshot.
- [U] The other 50 exact-body groups have not received complete decision
  assessments; no corpus-wide precision claim is made.
- [U] Evidence and Verification IDs in the benchmark assessment are external
  references, not admitted product-store records.
- [R] The next highest-value extension is an accepted-fact test-compilation
  scope and a separate normalized near-clone enumerator. This fails to be the
  next priority if exact-body assessment yield is too low on another repository.

## Recommended product boundary

Keep candidate enumeration, family decision, external acceptance and executed
verification as distinct stages. Promote the exact-body enumerator only after
its test-scope fact is repaired and a held-out repository measures candidate
yield. The decision and reinspection layers are already useful without that
promotion because their non-authority and denominator boundaries are explicit.
