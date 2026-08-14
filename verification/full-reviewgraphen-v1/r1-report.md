# R1 tuple integrity verification

Date: 2026-08-14

## Scope

R1 changes the V5 report tuple contract so the report producer emits
`body_hash = sha256(canonical(body))` for `TypedReportEventTupleV5`. The report
semantic consumer rechecks that hash for decision and evidence bodies, and
checks their event tuple shape before any report-local policy fields are used.

The existing Core witness hash was an event-envelope hash for these inherited
rows. A detached report did not contain that envelope preimage, so a consumer
could not independently verify it. The producer/consumer contract was aligned
to a self-verifiable DTO-body hash. Core's decision/evidence DTOs do not expose
`Deserialize`, so this stage does not claim full Core typed reconstruction or
Store membership authentication; those remain explicit work for the later
authenticated replay design.

## Measured progression

| Stage | broken | not_broken | Observation |
| --- | ---: | ---: | --- |
| Original Phase B before any remediation | 7 | 4 | preserved in `adversarial-results.json` |
| After detached gate withdrawal (Step 2) | 5 | 6 | preserved in `gate-trust-boundary-after.json` |
| After R1 | 3 | 8 | `r1-adversarial-results.json` |

R1 changed these cases from broken to not_broken:

- `authority.serialized_decision_tamper`: schema validation exit 3.
- `evidence.serialized_body_tamper`: schema validation exit 3.

`gate.coherent_local_forgery` remained not_broken because Step 2 already made
detached gate invocation unsupported (exit 2); R1 did not claim to repair its
policy derivation. The three denominator cases remain broken and are deferred
to R2.

The runner also measured two fixed review executions with byte-identical
output. A `not_broken` result means only that the named attack did not break the
tested property; it is not a general safety claim.
