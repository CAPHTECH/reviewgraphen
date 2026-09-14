# FSL kernel-digest maintenance improvement

Status: one applied, mutation-sensitive shared-conformance improvement. This is
not a general utility claim.

## Bound candidate

- FSL base snapshot: `38f97bfdaf5a7d251de62dd43e37ab4b41e4ef73`
- Near candidate signal:
  `responsibility-family-candidate-signal:sha256:fcb83daf1fc7d966bc93175f54a7874bf25eda30915803ec6a9cd9d07f6309c3`
- Members: `fsl_tools::document_digest::normalized_kernel_ast` and
  `fslc::approval::normalized_ast`
- Common contract: `fixtures/fsl-kernel-digest-contract.v1.json`
- Decision denominator: 2 members, 15 obligations
- Validated proposal: `shared_conformance_test` (8 supports, 3 opposes, 4
  not-applicable/unresolved obligations)

The shared-validator option was rejected because FSL's accepted design requires
the approval and requirements-document producers to remain independently
implemented. Intentional separation without a shared test was rejected because
both producers expose the same `fsl-kernel-ast-v1+sha256` identity and change for
the same normalization/framing contract.

## Applied change

The FSL worktree adds one cross-producer test in
`rust/fslc/src/approval.rs`. `fsl-tools` exposes its existing
`spec_digest_from_kernel` function so the test compares complete digest
bytes for the same nontrivial lowered kernel. Production implementations remain
separate. `docs/DESIGN-document-requirement-claim-ir.md` now records that
boundary and `changelog.d/333-kernel-digest-conformance.required.md` records the
new required control.

## Before/after measurement

Mutation: change only the approval producer's framing byte from `0x00` to
`0x01` in `spec_digest_kernel`.

| State | Existing approval tests | Shared conformance control | Surviving mutation |
| --- | ---: | ---: | ---: |
| Before | 5/5 pass | absent | 1 |
| After, same mutation | 5/5 pass | fails with unequal complete digests | 0 |
| After, mutation reverted | unchanged | pass twice | n/a |

The rejecting run observed approval digest
`sha256:76e9eebef911effc153536ab6c04e24f30cea471f639ae9777237e5a159f062f`
versus document digest
`sha256:1fea18f1b3ecbfcc68661be41c57ae00d14f55604cd1f280c22d09944e3f839f`.
This establishes that the new control detects this one cross-producer framing
drift; it does not establish completeness over every possible normalization
mutation.

## Verification scope

- focused new test: pass twice on the actual changed FSL worktree;
- the same focused test: failed under the isolated framing mutation;
- five pre-existing approval tests: passed under that mutation;
- Rust formatting, changelog fragment validation, and `git diff --check`: pass;
- `fslc-rust` binary Clippy with warnings denied: pass on the actual changed
  FSL worktree (with its build output redirected to the disposable target).

One attempted full `fslc-rust` package test run was interrupted after the
focused result was already established; it exited 130 and is not reported as a
green package gate. The full native integration gate, performance, and every
other digest caller remain `[U]`.
