# ADR 0043: Experimental responsibility-family decision support

- Status: Experimental, benchmark-only
- Date: 2026-09-14

## Context

ADR 0042 tracks reinspection only after an external actor has decided that a
set of implementations forms a responsibility family. That leaves the most
important abstraction boundary outside ReviewGraphen: whether implementations
share a contract and change reason, whether purpose-specific constraints can be
preserved, and whether to share production code, share conformance tests, or
stay separate.

Syntax similarity alone is not sufficient. A model opinion alone is not
Evidence or Verification, and a deterministic proposal must not become family
acceptance.

## Decision

Add a benchmark-only v1 decision-support layer. An externally supplied
candidate binds a snapshot, proposed family and common contract, extractor,
members, purpose constraints, sources and unknowns. The tool deterministically
enumerates a finite obligation universe covering common-contract membership,
common change reason, shared-conformance feasibility, separation rationale,
and per-member preservation of purpose constraints, typed errors,
compatibility, performance and shared-validator feasibility.

An assessment must contain exactly one result for every obligation. Decisive
`supports` and `opposes` results require source, Evidence and Verification IDs.
The tool then proposes one of `shared_validator`, `shared_conformance_test`,
`intentional_separation`, or `inconclusive` using a closed rule table. The
proposal binds canonical hashes of the candidate, obligation universe and
assessment and is recomputed exactly by validation.

All candidate, obligation, assessment and proposal records are non-authority.
No output accepts a family, changes Program facts, executes target code, or
claims semantic equivalence. Human acceptance remains external.

## Selection rules

- Propose a shared validator only when the common contract, common change
  reason, shared conformance and every member-preservation check support it,
  with no supported separation rationale.
- Otherwise propose shared conformance tests when the common contract, common
  change reason, shared conformance and purpose-constraint preservation are all
  supported, with no supported separation rationale.
- Propose intentional separation only when a separation rationale is supported
  and either common-contract membership or the common change reason is opposed.
- Missing evidence, conflict, incomplete closure, or every other combination is
  inconclusive or rejected as malformed input; it is never guessed through.

## Limits

The initial trial uses the already studied five path validators. This does not
validate candidate discovery, human acceptance quality, semantic equivalence,
or utility on another repository or responsibility class.
