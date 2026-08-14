# Detached gate trust-boundary remediation

Date: 2026-08-14

## Immediate decision

The detached `gate <report.json>` command was withdrawn. The alternative—adding
a Store-root argument immediately—was rejected because the current CLI has no
contract for an operator-trusted immutable Store revision. Accepting an
arbitrary path would not establish authenticity.

The closed CLI parser now rejects every `gate` invocation with exit code 2
before reading the named path. `schema validate` remains available but is only
a structural and report-local consistency check; it cannot return a gate pass.

ADR 0028 specifies the future Store-replay or signed-attestation boundary. Its
implementation is deferred because Step 1 found three independent validation
roots rather than one repairable root. In particular, hash recomputation alone
would not close denominator authority or authenticate an attacker-recomputed
artifact.

## Regression method

The same Phase B mutation runner was executed against the updated binary. The
original `adversarial-results.json` is retained unchanged as the before
observation. The after observation is
`gate-trust-boundary-after.json`.

Measured summaries:

| Observation | broken | not_broken | SHA-256 |
| --- | ---: | ---: | --- |
| before (`adversarial-results.json`) | 7 | 4 | `eec7a6803174e6c2fb7f0f155221bbdb94c3f9e4d60aad4cc7660bde096dac6e` |
| after (`gate-trust-boundary-after.json`) | 5 | 6 | `1dc2e59f7e27c9fefceae852b0cfa52e918c3fbe9d9a0a2ed2895b85ba757b7d` |

The Rust CLI regression test also supplies a serialized pass-shaped object to
the withdrawn command and checks exit 2, empty stdout, absence of the old usage
surface, and identical rejection for a nonexistent path. This establishes that
dispatch rejects before report I/O.

## Interpretation

The expected immediate change is limited:

- `freshness.forged_pass` and `gate.coherent_local_forgery` change from
  `broken` to `not_broken`; both measured schema exit 0 and gate exit 2, so no
  detached input produced a gate pass.
- Authority tuple, evidence tuple, and the three denominator mutations remain
  `broken` under their original property: detached `schema validate` still
  accepts them with exit 0. Their gate invocations now exit 2, but their
  Store-authenticated schema/semantic repair is designed, not implemented.

`not_broken` here means only that the named attack did not break the measured
property. It is not a claim of general safety.
