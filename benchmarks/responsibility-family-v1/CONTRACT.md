# Responsibility-family decision and reinspection contract

Status: experimental benchmark API v1; non-authoritative.

`snapshot` creates a ProgramSpace using the production ingest adapter.
`discover-exact` consumes accepted, snapshot-bound Rust symbol anchors and
groups equal normalized function bodies under its declared source-path and
accepted test-scope profile. `discover-near` groups accepted token-shape hashes
after excluding exact-only groups. `discover-signals` uses accepted callable,
signature and operation syntax terms to enumerate different-shape pairs under
the versioned selective-frequency and operation-Jaccard rule. Reports preserve
accepted, eligible, excluded, unknown and candidate denominators and expose the
signals that caused enumeration. Every group or pair remains candidate-only:
syntax similarity and shared vocabulary are not a common contract, a common
change reason, Evidence, Verification or family acceptance.

`search-responsibility` consumes a snapshot-bound planned-responsibility
contract and searches the same accepted callable, signature and operation
facts. It retains absent and high-frequency query terms in the coverage
denominator, applies no shape exclusion or top-N cutoff, and emits a complete
candidate set under its fixed v1 threshold. Natural-language clause content is
bound by the canonical contract hash but is not evaluated. Each candidate
therefore carries every clause ID as unverified, and
`candidate_count * clause_count` is an explicit verification denominator. A
candidate is not proof that an implementation exists or conforms.

The decision path accepts an externally enumerated candidate and creates a
finite universe containing three family checks and six checks per member. An
assessment input must cover that universe exactly. `supports` and `opposes`
require source IDs plus Evidence and Verification IDs. The deterministic rule
table can propose a shared validator, shared conformance tests, intentional
separation, or inconclusive. A proposal is not family acceptance.

`plan(before, after)` compares two closed responsibility-family state records.
The records are declared external bases, not accepted facts. Each binds a family
ID, snapshot, decision basis, common contract, anchor extractor, sorted members,
purpose constraints, source IDs, and unknowns.

The plan has one obligation per affected member. A common contract, decision
basis, or extractor change affects every member in the union. Anchor or purpose-
constraint changes affect that member. Additions and removals remain explicit.
Snapshot change without any of these changes preserves the member.

Members count responsibility implementations, not downstream endpoints. When
a shared-validator response collapses duplicate implementations, the old
members are removed and the shared implementation is added. Endpoint coverage
remains a separate conformance-test denominator and must not be inferred from
the smaller family denominator.

The plan is ordered by obligation ID, binds canonical hashes of both complete
inputs, declares its before/current denominator and unknowns, and has fixed
non-authority fields. `validate_plan` recomputes it exactly. The benchmark plan
accepts, verifies, human-accepts, or signs off nothing and executes no target
code. Separately, Core's `reviewgraphen.responsibility_family_state.v1`
represents an externally human-accepted family only when a proposal hash and
nonempty Evidence and Verification IDs are supplied. It remains non-verified
and non-sign-off, separate from ProgramSpace, and may be stored as canonical
immutable CAS data.
