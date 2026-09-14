# ADR 0045: Enumerate exact-body responsibility candidates

- Status: Experimental, benchmark-only
- Date: 2026-09-14

## Context

The first near-clone screen discarded exact-shape pairs and consequently missed
the duplicated `Severity` wire-text mapping. Responsibility-family decision
support can structure a supplied candidate but cannot currently enumerate one.
This makes its useful workflow depend on an unbound external search step.

Body equality is not semantic equivalence and does not establish a common
contract or change reason. It is nevertheless a cheap, deterministic candidate
signal already available in accepted Rust symbol anchors.

## Decision

Add a benchmark-only exact-body discovery report over accepted
`RustSymbolAnchorV1` facts. The profile includes Rust functions and methods
under `crates/*/src/**/*.rs` and excludes only symbols whose accepted
`test_function` fact is true. It groups equal normalized body hashes, retains
each member's distinct signature hash and source location, exposes all
selection denominators and losses, and remains non-authoritative.

Symbols without an accepted `test_function` fact are excluded with their own
denominator rather than guessed non-test. In the current extractor this mainly
limits method coverage. A standalone `#[cfg(test)]` helper can currently have a
false `test_function` fact, so it can remain a candidate; the report declares
that upstream loss instead of calling the profile production-complete.

The output is a candidate inventory, not a `ResponsibilityFamilyCandidate`.
Common contract, change reason, purpose constraints and unknowns must still be
assessed through the existing decision path before any maintenance proposal.

## Consequences

Candidate discovery becomes reproducible and snapshot-bound without reopening
source files. Tiny getters and unrelated same-body functions can be false
positives; this is accepted at enumeration and must be rejected by the decision
stage. Macro-expanded functions and symbols without accepted anchors remain
unknown rather than inferred absent.
