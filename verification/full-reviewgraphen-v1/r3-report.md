# R3 freshness and gate replay verification

Date: 2026-08-14

## Scope

R3 replaces coverage/gate declaration checks with complete report-local replay.
The V5 semantic validator reconstructs native evidence witnesses, current
decision-bound findings, rerun state, unresolved mappings, unsupported impact,
current M5 inputs, extraction limitations, and cardinality obstructions. It
passes those reconstructed inputs to the same `reduce_incremental_gate_v5`
function used by report production and requires exact equality of the complete
gate object. This covers freshness, blockers, incomplete inputs, reasons,
status, sources, gate ID, and gate body hash.

The fixed `double-submit` producer report passed this validator. Two explicit
negative regression assertions were added to the CLI test: a locally coherent
pass rewrite and a coverage-local freshness promotion are both rejected.

## Measured progression

| Stage | broken | not_broken |
| --- | ---: | ---: |
| Original Phase B | 7 | 4 |
| After detached gate withdrawal | 5 | 6 |
| After R1 | 3 | 8 |
| After R2 | 0 | 11 |
| After R3 | 0 | 11 |

No verdict changed at R3 because both target cases were already `not_broken`
after the detached gate command was withdrawn. The mechanism did change:

- `freshness.forged_pass`: schema validation changed from exit 0 after R2 to
  exit 3 after R3.
- `gate.coherent_local_forgery`: schema validation changed from exit 0 after R2
  to exit 3 after R3.

The other nine cases remained `not_broken`. The final unchanged adversarial
runner measured 0 broken and 11 not_broken cases. The detached `gate`
invocation still exits 2; the R3 evidence is the independent semantic-validator
exit 3, not the command withdrawal.

## Verification command decision

No pass-producing verification command is reintroduced. R1--R3 establish
report-local consistency, but an unkeyed, detached report is not authenticated:
an editor able to replace the whole artifact can recompute its hashes. The
repository still lacks the immutable trusted Store-revision DTO and signed
attestation trust policy required by ADR 0028. `schema validate` remains a
non-authoritative consistency check and never returns a gate policy result.

A future command must use a distinct authenticated interface, require an
operator-trusted Store revision or accepted signed attestation, replay the Store
rows, and refuse before policy evaluation when that basis is absent.

## Limits

The result shows only that the named mutations did not break the tested
properties. It does not authenticate arbitrary detached reports, and it is not
a general safety claim.
