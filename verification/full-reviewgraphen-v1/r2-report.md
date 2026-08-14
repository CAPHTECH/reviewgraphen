# R2 denominator closure verification

Date: 2026-08-14

## Scope

R2 adds report-local authoritative closure checks for the duplicated V5
denominator projection:

- `coverage.denominator_obligation_ids` must exactly equal
  `scenario.selected_obligation_ids`;
- `coverage.selected` is checked against the actual denominator length;
- `coverage.universe_id` must equal `scenario.universe_id`.

The producer already constructs scenario selection from the durable plan waves
and checks that selection against the Store universe before report generation.
R2 makes a detached consumer reject coverage-local edits that do not also alter
that independent scenario projection. It does not authenticate an attacker who
can coherently rewrite every duplicated projection and recompute every hash;
that requires the Store revision or signed attestation basis designed by ADR
0028.

## Measured progression

| Stage | broken | not_broken |
| --- | ---: | ---: |
| Original Phase B | 7 | 4 |
| After detached gate withdrawal | 5 | 6 |
| After R1 | 3 | 8 |
| After R2 | 0 | 11 |

R2 changed these cases from broken to not_broken, each with schema validation
exit 3:

- `denominator.omission`
- `denominator.inflation`
- `denominator.substitution`

The aggregate zero does not establish R3. `freshness.forged_pass` and
`gate.coherent_local_forgery` were already not_broken because detached gate
invocation is unsupported. Freshness and gate policy have not yet been
re-derived by the serialized consumer at this stage.

A `not_broken` result means only that the named attack did not break the tested
property; it is not a general safety claim.
