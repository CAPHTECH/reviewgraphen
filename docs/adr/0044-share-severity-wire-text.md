# ADR 0044: Share the Severity wire-text mapping

- Status: Experimental implementation
- Date: 2026-09-14

## Context

An exact-match source scan found two exhaustive mappings from the public core
`Severity` type to the same five lowercase wire strings. Core planning used one
mapping for bounded canonical JSON, while store index-v5 used the other for
projection JSON. They have the same contract and change reason; their distinct
writers, schemas and error paths remain purpose-specific.

The responsibility-family decision trial enumerated 15 obligations and
proposed `shared_validator` with 14 supports, one opposed separation rationale
and no unresolved obligation. The proposal is non-authoritative; this ADR is
the explicit implementation decision.

## Decision

Add allocation-free `Severity::as_str` in core and make both consumers use it.
Keep their JSON writers, typed errors, schema versions and bounds separate.
Bind the helper to the existing serde lowercase contract with a five-variant
test.

## Consequences

Adding or renaming a severity variant now has one exhaustive text mapping. The
compiler and the shared contract test protect both consumers against an
incomplete update. The change adds a public method but changes no serialized
value or accepted input.

Candidate discovery remains external: ReviewGraphen structured and validated
the decision after the exact-match scan supplied the candidate.
