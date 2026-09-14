# ADR 0049: Enumerate responsibility-signal pair candidates

- Status: Experimental
- Date: 2026-09-14

## Context

Exact-body discovery finds identical implementations and near-body discovery
finds identifier- and literal-renamed copies with the same complete structural
shape. They cannot surface a pair after statements or control flow have been
inserted, removed, or reordered. That is an important candidate source for
duplicated maintenance responsibility, but syntax cannot establish semantic
equivalence, a common contract, or a common change reason.

## Decision

The Rust ingest adapter emits an additional accepted syntax fact for every
function and method:

- `responsibility_signals`, extracted by
  `reviewgraphen.ingest.rust-responsibility-signals@1`;
- three sorted, unique namespaces: words from the declared callable name,
  terminal identifiers from signature type paths, and terminal identifiers
  from syntactic function and method calls.

These are syntax observations, not responsibility facts. Comments, formatting,
literal values, type resolution, macro expansion, alias resolution and dynamic
dispatch do not contribute semantic authority. The extractor contract is bound
into the ProgramSpace adapter-set hash.

Add benchmark-only `discover-signals`. It emits unordered two-member candidate
pairs and never transitively clusters them. Both members must pass the existing
production/test-scope profile, carry the accepted shape and signal facts, and
have different responsibility-shape hashes. A pair must share at least one
selective callable-or-signature term, at least two selective operation terms,
and an operation-term Jaccard similarity of at least 600,000 parts per million.
A selective term occurs in at least two eligible members and no more than
`max(32, ceil(eligible_members / 5))`; these fixed rules are versioned as
`reviewgraphen.benchmark.rust-responsibility-signal-pairs@1`.

Candidate IDs bind the snapshot, rule, extractor, and sorted member IDs.
Reports expose the exact matched terms, complete input and pair denominators,
ignored high-frequency terms, exclusions, information loss and unknowns. There
is no hidden top-N or output cap. Ranking is triage only and cannot affect IDs.

## Authority boundary

Every result is `candidate_only` and non-authoritative. The command does not
create a `ResponsibilityFamilyCandidate`, because syntax cannot supply its
proposed common contract or family decision. Selected pairs must still pass the
existing decision obligations for common contract, common change reason,
purpose constraints, typed errors, compatibility, performance, shared
conformance and shared-validator fitness.

## Compatibility

Exact report v2, near report v1, responsibility-family state v1, the decision
schemas and the production CLI remain unchanged. Old ProgramSpaces remain
readable; signal discovery counts their missing accepted fact as an unknown
exclusion. Newly ingested ProgramSpaces intentionally receive a different
adapter-set hash because their accepted fact set has changed.

## Consequences

Some structurally different implementations can now be surfaced without an
LLM enumerating the candidate universe. Generic names and common helper calls
can still create false positives, and same-responsibility implementations with
no shared retained vocabulary remain invisible. Real utility requires a
source-reviewed candidate to pass the existing decision path and a maintenance
change or conformance mutation to improve the declared test denominator.

The 600,000 ppm threshold was fixed after a calibration run on the FSL snapshot
`38f97bfdaf5a7d251de62dd43e37ab4b41e4ef73`. The initial two-channel rule
emitted 65,965 pairs from 2,917 eligible functions (4,252,089 distinct-shape
pairs). Replaying the same accepted facts at candidate thresholds from 200,000
through 700,000 ppm produced respectively 14,950, 5,372, 2,522, 1,314, 596 and
272 pairs. We selected 600,000 before inspecting the resulting 596 source
pairs. This is calibration, not held-out utility evidence; the snapshot must
not later be presented as an unbiased evaluation corpus for this threshold.
