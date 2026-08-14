# ADR 0028: Authenticated Gate Trust Boundary

Status: Accepted

## Context

ADR 0024 exposed `gate <report.json>` as a thin adapter from the serialized V5
`gate.status` field to a process exit code. The adapter checked the JSON schema
and report-local semantic invariants, but it had no separately trusted Store
root, Store revision, report signature, or attestation key.

The Phase B adversarial suite demonstrated seven accepted mutations at this
boundary. In particular, rewriting a blocked report to a locally coherent pass
while retaining its old gate ID and body hash produced exit code 0. Root-cause
analysis separates the defect into tuple integrity/provenance, authoritative
denominator closure, and replay of freshness/gate reduction. A body-hash check
alone cannot authenticate a detached artifact because an attacker able to edit
the artifact can also recompute an unkeyed hash.

## Decision

### Immediate boundary

Withdraw `gate <report.json>`. An unsupported `gate` invocation is rejected as
invalid CLI syntax with exit code 2 before opening its path. No current command
returns pass from a detached report.

We choose withdrawal rather than immediately accepting a Store path because the
current CLI has no public contract for selecting and opening an operator-trusted
Store root at an exact immutable revision. Adding a path argument without that
revision and trust contract would move, not close, the ambiguity.

`schema validate <report.json>` remains a bounded structural and report-local
semantic check. It is explicitly not report authentication and cannot produce a
gate exit status.

### Future authenticated gate

A future gate command may be introduced only when it has one of these trust
bases:

1. **Preferred local basis — trusted Store revision.** The operator supplies a
   trusted Store root and an exact revision coordinate. The coordinate includes
   the run IDs, terminal event count/offset, terminal tail hash, index snapshot
   hash, and terminal proof identity required by the report contract. The gate
   opens that revision using Store validation, reconstructs typed rows, and
   regenerates the canonical report and gate with the production reducer.
2. **Signed attestation basis.** A versioned envelope contains the canonical
   report digest, Store revision coordinates above, schema/policy/reducer
   versions, signing key ID, and signature. Verification uses an explicitly
   configured trust policy. A valid signature authenticates the envelope; it
   does not replace semantic replay unless the trust policy explicitly grants
   the signer authority to attest that replay.

A plain content hash or report-provided Store path is not a trust basis.

### Detached report-local replay status

R1 through R3 now implement the three report-local consistency layers in the
V5 semantic validator. Decision and evidence bodies have canonical body hashes;
the coverage denominator is closed against the scenario projection; and the
validator reconstructs the native evidence closure and every gate reducer input
available in V5, then calls the same `reduce_incremental_gate_v5` function as
the producer and requires equality of the complete serialized gate, including
status, blockers, incomplete IDs, reasons, sources, ID, and body hash.

This makes detached V5 reports self-consistent, not self-authenticating. An
editor can still coherently replace the artifact and its unkeyed hashes, and V5
does not carry a trusted Store revision or signature. Consequently the removed
`gate <report.json>` command is not restored. `schema validate` may reject a
locally inconsistent report, but it does not emit a policy pass result.

### Required replay layers

The future consumer must perform all three layers, in order:

1. **Canonical tuple integrity.** Deserialize every event tuple into its typed
   form; recompute body hashes, identity hashes, stable IDs, gate ID, and gate
   body hash; verify event identity/sequence against the trusted revision.
2. **Authoritative projection closure.** Reconstruct the obligation denominator
   from the durable plan waves and target universe. Require exact equality with
   scenario selection and coverage denominator. Reconstruct evidence,
   verification, binding, decision, finding, staleness, and rerun closures from
   Store rows rather than accepting duplicated report fields.
3. **Shared policy reduction.** Use the same pure reducer as report production
   to derive native/fresh coverage, blockers, incomplete IDs, reasons, source
   IDs, status, gate ID, and gate body hash. Regenerate canonical report bytes
   and compare their digest with an optional supplied report artifact.

The reducer and identity code must be shared library code. A second CLI-specific
implementation is not an acceptable verification basis.

### Versioning and proposed interface

The existing V5 report remains a projection and is not retroactively promoted
to a self-authenticating envelope. The future interface is provisionally:

```text
reviewgraphen gate --store <trusted-root> --revision <revision.json> [--report <report.json>]
```

If signed transport is required, add a separately versioned attestation
envelope rather than silently changing V5 semantics. The exact revision DTO,
signature algorithm, key trust policy, migration, and exit-code restoration
require a follow-up ADR before implementation.

## Invariants

- No gate returns pass without a trusted Store revision or accepted signature
  trust basis.
- A report-provided field cannot select or broaden its own authority.
- Model/reviewer output remains non-authoritative and cannot become a decision,
  evidence verification, or gate input without the existing typed authority
  transitions.
- Stale evidence cannot receive native/fresh credit through serialized coverage
  declarations.
- Schema validation remains distinct from authentication and policy evaluation.
- A `not_broken` adversarial result means only that the named attack did not
  break the tested property; it is not a general safety claim.

## Consequences

- Existing consumers of the detached gate receive exit code 2 and must stop or
  use an independently trusted workflow; this is intentionally fail-closed.
- V5 reports remain useful deterministic audit projections, but their possession
  alone grants no authority.
- The future implementation has a larger blast radius than a hash comparison:
  Store revision APIs, typed report deserialization, shared reduction APIs,
  attestation/version schemas, fixtures, migration docs, and adversarial tests.
- The original seven Phase B mutations are retained as immutable before
  evidence. R1--R3 results are additive measurements; they do not retroactively
  rewrite those findings.
- Report-local replay does not satisfy either authenticated trust basis. A
  pass-producing command remains future work until the Store revision or signed
  attestation contract is implemented.
