# Serialized Report Trust-Boundary Root-Cause Analysis

Date: 2026-08-14

Scope: the seven `broken` cases recorded by the Phase B adversarial run in
`verification/full-reviewgraphen-v1/adversarial-results.json`.

This is an investigation result. It does not claim that the defects have been
repaired.

## Finding

All seven cases share one architectural boundary error: a detached serialized
V5 report is presented as a policy input even though it is not authenticated
against the Store revision from which the authoritative report was produced.

That boundary error contains three independently repairable validation gaps.
It is therefore not a single implementation root cause that can be repaired by
one body-hash comparison.

| Root | Missing verification | Trusted declaration | Cases |
| --- | --- | --- | --- |
| R1 tuple integrity and provenance | Tuple body hashes, derived IDs, event IDs/sequences, and Store membership are not recomputed or authenticated | Serialized decision and evidence bodies; serialized gate ID/body hash | `authority.serialized_decision_tamper`, `evidence.serialized_body_tamper`, part of `gate.coherent_local_forgery` |
| R2 authoritative denominator closure | Coverage denominator is not compared with the scenario selection, durable plan waves, or Store universe | `coverage.denominator_obligation_ids` and coverage-local counts | `denominator.omission`, `denominator.inflation`, `denominator.substitution`, part of `freshness.forged_pass` |
| R3 derived freshness and gate replay | Native evidence closure, freshness, blockers, incomplete inputs, reasons, status, sources, gate ID, and gate hash are not re-derived | Coverage native/verified/fresh arrays and locally coherent gate fields | `freshness.forged_pass`, `gate.coherent_local_forgery` |

## Code evidence

### R1: tuple integrity and provenance

- `schemas/reviewgraphen.report.v5.schema.json:2140-2163` requires an evidence
  tuple to contain a syntactically valid `body_hash`, but has no constraint that
  hashes `body` or binds the row to a Store event.
- `schemas/reviewgraphen.report.v5.schema.json:2218-2243` does the same for a
  decision tuple. `actor` and `authority_id` are only shape-checked.
- `crates/reviewgraphen-report/src/report_v5.rs:212-236` reads `result` only to
  validate action tuples and cardinality obstructions. It does not inspect the
  decision, evidence, binding, or verification rows.
- `crates/reviewgraphen-report/src/report_v5.rs:3996-4044` shows that report
  production derives the gate ID and body hash. The serialized consumer does
  not repeat those derivations.

Required repair locations and approximate impact:

- Add typed deserialization and canonical body/identity verification for every
  V5 tuple family, not only decisions and evidence.
- Reuse or expose the canonical ID/hash derivation routines instead of
  duplicating their rules in the CLI.
- Bind the checked tuples to an authenticated Store revision or signature.
  Recomputing an unkeyed hash detects a stale retained hash, but an attacker who
  can rewrite both body and hash can still produce a self-consistent forgery.
- Expected impact: report validation, Core DTO conversion/ID APIs, all V5 tuple
  contract tests, and the eventual Store-bound gate command.

### R2: authoritative denominator closure

- `crates/reviewgraphen-report/src/report_v5.rs:114-175` requires a non-empty
  serialized denominator and checks only that each serialized coverage axis is
  a subset with a matching count. It does not compare the denominator with the
  scenario selection or durable plan.
- `schemas/reviewgraphen.report.v5.schema.json:2639-2641` separately exposes
  `scenario.selected_obligation_ids`; no semantic equality check connects it to
  `coverage.denominator_obligation_ids` at
  `schemas/reviewgraphen.report.v5.schema.json:2967-2969`.
- The authoritative producer does perform a stronger check:
  `crates/reviewgraphen-report/src/report_v5.rs:1316-1333` reconstructs durable
  plan selection, requires exact request/plan equality, and checks it against
  the target universe.

Required repair locations and approximate impact:

- At minimum, semantic validation must cross-check duplicated scenario and
  coverage fields. That detects the current retained-source mutations but is
  not an authenticity root.
- A trusted gate must reconstruct the denominator from the exact durable plan
  and universe in an authenticated Store revision.
- Expected impact: V5 semantic validation, scenario/coverage contract tests,
  report reconstruction, and Store revision selection.

### R3: derived freshness and gate replay

- `crates/reviewgraphen-report/src/report_v5.rs:155-175` checks only equality of
  three serialized coverage arrays; it does not derive native passes.
- The producer derives native passes through execution, claim, verification,
  evidence, and binding rows at
  `crates/reviewgraphen-report/src/report_v5.rs:3020-3093`.
- `crates/reviewgraphen-report/src/report_v5.rs:493-532` checks only local
  status/blocking/incomplete/reason consistency and sorted source IDs.
- The producer derives native freshness at
  `crates/reviewgraphen-report/src/report_v5.rs:3850-3862`, gate status at
  `crates/reviewgraphen-report/src/report_v5.rs:3883-3914`, reasons at
  `crates/reviewgraphen-report/src/report_v5.rs:3915-3968`, and identity/hash at
  `crates/reviewgraphen-report/src/report_v5.rs:3996-4044`.
- `crates/reviewgraphen-cli/src/lib.rs:144-167` validates only schema and local
  semantics, then returns the serialized `/gate/status`.

Required repair locations and approximate impact:

- Refactor the pure typed gate reduction so both report production and a
  Store-bound verifier use exactly the same implementation.
- Reconstruct every reducer input from authenticated Store rows, regenerate the
  canonical report/gate, and compare it with the supplied artifact if one is
  supplied.
- Expected impact: report reduction APIs, CLI, Store replay/revision opening,
  fixtures, and adversarial regression tests.

## Seven-case attribution

1. `authority.serialized_decision_tamper`: R1. Mutated authority declarations
   are shape-valid; their tuple identity, hash, and Store origin are unchecked.
2. `evidence.serialized_body_tamper`: R1. Mutated subjects are shape-valid; the
   tuple hash and evidence-binding closure are unchecked.
3. `denominator.omission`: R2. Coverage-local sets and counts can be repaired
   without checking the durable selection.
4. `denominator.inflation`: R2. A fabricated obligation can be added without a
   plan/universe membership check.
5. `denominator.substitution`: R2. Coverage-local replacement is not compared
   with the independent scenario/Store source.
6. `freshness.forged_pass`: R2 and R3. Serialized coverage can declare all
   denominator members fresh and the gate can accept that declaration without
   replaying the native evidence closure.
7. `gate.coherent_local_forgery`: R1 and R3. A locally coherent pass retains an
   invalid gate ID/hash, and neither identity nor policy reduction is replayed.

## Boundary consequence

ADR 0024 explicitly described `gate <report.json>` as a thin adapter at
`docs/adr/0024-fixed-offline-cli-vertical-slice.md:46-49`. That behavior is
internally consistent with the ADR, but the command surface resembles a
verification gate while possessing no trusted verification basis. A detached
report can be schema-valid and internally self-consistent without being
authentic. The immediate correction must therefore remove that pass-producing
trust boundary or require a separately trusted Store/signature basis.
