# Responsibility-family v1 trial

This benchmark-only trial structures a responsibility-family decision proposal
and turns an externally accepted decision into deterministic reinspection
obligations. It can snapshot a Rust repository and enumerate exact-body,
Type-2-like near-body, and structurally different responsibility-signal pair
candidates from accepted Rust facts, but it does not infer candidate
responsibility or accept a family.

Snapshot and candidate inventories:

```console
cargo run -p reviewgraphen-benchmark \
  --bin reviewgraphen-responsibility-family -- snapshot \
  --workspace /admitted/workspace --repository /admitted/workspace/fsl \
  --identity fsl/held-out@1 --base COMMIT --target COMMIT \
  --output /tmp/program-space.json

cargo run -p reviewgraphen-benchmark \
  --bin reviewgraphen-responsibility-family -- discover-exact \
  --program-space /path/to/program-space.json \
  --output /tmp/exact-body-candidates.json

cargo run -p reviewgraphen-benchmark \
  --bin reviewgraphen-responsibility-family -- discover-near \
  --program-space /path/to/program-space.json \
  --output /tmp/near-body-candidates.json

cargo run -p reviewgraphen-benchmark \
  --bin reviewgraphen-responsibility-family -- discover-signals \
  --program-space /path/to/program-space.json \
  --output /tmp/responsibility-signal-candidates.json

cargo run -p reviewgraphen-benchmark \
  --bin reviewgraphen-responsibility-family -- search-responsibility \
  --program-space /path/to/program-space.json \
  --contract /path/to/planned-responsibility-contract.json \
  --output /tmp/responsibility-search-report.json
```

The inventory is ranked by the smallest member span, largest first. It groups
body hashes without using function names or signature differences. Its
`information_loss`, `unknowns` and denominator are part of the result. Exact v2
and near v1 require the accepted `reviewgraphen.ingest.rust-test-scope@1` fact;
near v1 additionally requires the accepted responsibility-shape fact and
excludes exact-only groups explicitly. See `FSL-HELD-OUT.md` for the first
cross-repository measurement.

Signal discovery requires different responsibility-shape hashes and two
independent vocabulary channels: at least one shared callable-or-signature
term, at least two shared operation terms, and at least 600,000 ppm Jaccard
similarity over selective operation terms. It emits pairs rather than
transitive clusters and exposes the exact matched terms. On the FSL snapshot
used above, the calibrated rule emitted 596 pairs from 2,917 eligible
functions; two runs were byte-identical. See `FSL-SIGNAL-DISCOVERY.md`. These
are triage candidates, not semantic-equivalence or family decisions.

`search-responsibility` runs the same direction in reverse: before implementing
a planned responsibility, describe its clauses and callable, signature and
operation terms with
`reviewgraphen.benchmark.planned_responsibility_contract.v1`. The command
searches all eligible accepted Rust functions and methods, including different-
shape bodies, and reports every candidate meeting the fixed selective-signal
threshold. Absent and high-frequency query terms remain in the denominator.
Clause prose is not evaluated: every candidate carries the complete
`unverified_clause_ids` frontier. The result therefore means “inspect these
existing implementations before adding another one”, not “the feature already
exists” or “the contract is satisfied”. See ADR 0050.

`FSL-MAINTENANCE-IMPROVEMENT.md` follows one of those near candidates through
the 15-obligation decision path and into an applied FSL shared-conformance test.
The same one-producer framing mutation survives the five prior focused tests and
is killed by the new control. This is one bounded maintenance improvement, not
a claim over the remaining candidate inventory.

For the five path validators, the decision fixture produces 33 obligations.
The checked assessment supports the shared base contract, common change reason,
purpose constraints and shared conformance, opposes erasing typed-error
differences, and leaves compatibility, performance and shared-validator details
unverified. The resulting non-authoritative option is
`shared_conformance_test`.

The two fixtures bind the five path-policy implementations observed at snapshot
`c0747614`. The second fixture changes only the token anchor of the M6 member.
The expected plan therefore has denominator 5 before and after, one
`member_anchor_changed` obligation for M6, and four preserved members.

Run:

```console
cargo run -p reviewgraphen-benchmark \
  --bin reviewgraphen-responsibility-family -- decision-obligations \
  --candidate benchmarks/responsibility-family-v1/fixtures/path-policy-candidate.json \
  --output /tmp/path-policy-universe.json

cargo run -p reviewgraphen-benchmark \
  --bin reviewgraphen-responsibility-family -- decision-assess \
  --candidate benchmarks/responsibility-family-v1/fixtures/path-policy-candidate.json \
  --input benchmarks/responsibility-family-v1/fixtures/path-policy-assessment-input.json \
  --output /tmp/path-policy-assessment.json

cargo run -p reviewgraphen-benchmark \
  --bin reviewgraphen-responsibility-family -- decision-propose \
  --candidate benchmarks/responsibility-family-v1/fixtures/path-policy-candidate.json \
  --assessment /tmp/path-policy-assessment.json \
  --output /tmp/path-policy-proposal.json

cargo run -p reviewgraphen-benchmark \
  --bin reviewgraphen-responsibility-family -- plan \
  --before benchmarks/responsibility-family-v1/fixtures/path-policy-before.json \
  --after benchmarks/responsibility-family-v1/fixtures/path-policy-m6-changed.json \
  --output /tmp/path-policy-plan.json

cargo run -p reviewgraphen-benchmark \
  --bin reviewgraphen-responsibility-family -- validate \
  --before benchmarks/responsibility-family-v1/fixtures/path-policy-before.json \
  --after benchmarks/responsibility-family-v1/fixtures/path-policy-m6-changed.json \
  --plan /tmp/path-policy-plan.json
```

Output is non-authoritative. Tests, mutation execution, family acceptance, and
human sign-off remain separate steps.

After external human acceptance, the separate product schema
`reviewgraphen.responsibility_family_state.v1` can retain the contract,
decision, members, purpose constraints, denominators, Evidence, Verification,
and unknowns in the verified CAS. Candidate and proposal files cannot be stored
as that state without satisfying Core's acceptance boundary.

The selected shared-conformance response is now applied in the working tree as
`fixtures/snapshot-relative-path-contract.v1.json` plus one `#[cfg(test)]`
consumer per member. See `VERIFICATION.md` for the exact checked scope,
mutation-sensitive result and candidate-discovery limitation.

The severity-wire follow-up exercises an externally enumerated exact clone:

```console
cargo run -p reviewgraphen-benchmark \
  --bin reviewgraphen-responsibility-family -- decision-obligations \
  --candidate benchmarks/responsibility-family-v1/fixtures/severity-wire-candidate.json \
  --output /tmp/severity-wire-universe.json

cargo run -p reviewgraphen-benchmark \
  --bin reviewgraphen-responsibility-family -- plan \
  --before benchmarks/responsibility-family-v1/fixtures/severity-wire-before.json \
  --after benchmarks/responsibility-family-v1/fixtures/severity-wire-after.json \
  --output /tmp/severity-wire-reinspection.json
```

Its decision proposal is `shared_validator`; ADR 0044 applies that response.
The reinspection state records that two duplicate implementations became one
shared implementation without treating the proposal as family acceptance.

The exact-body inventory's rank-3 exclusion-ID group is a second decision
fixture. It resolves to `shared_conformance_test`, not a shared implementation,
because compatibility, performance and future rule-neutral evolution remain
unresolved. ADR 0046 records that bounded response.
