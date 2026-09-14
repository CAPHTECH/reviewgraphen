# Path-policy decision evidence index

This file names the external observations used by the experimental decision
assessment. It is not accepted Evidence in the ReviewGraphen product store.

- `evidence:path-policy-source-reading`: the five named function bodies and
  their purpose-specific return/error contracts were read at snapshot
  `c0747614c2c20a51771f6f6495f196682e3912fd`.
- `verification:path-policy-source-reading`: the shared base predicate and the
  listed differences were compared across all five members.
- `evidence:path-policy-cross-mutation`: two shared-contract mutations survived
  6 of 10 endpoint/test cells before adding the shared conformance fixture and
  0 of 10 afterward.
- `verification:path-policy-cross-mutation`: the before/after mutation suite was
  executed once for the five-member experiment. The observation and the current
  one-mutation reproduction are summarized in `VERIFICATION.md`; the original
  ten-cell raw scratch logs were not promoted into this repository.

The last limitation means this index supports a benchmark decision proposal,
not product Verification or family acceptance.

## Severity wire-text candidate

- `evidence:severity-wire-exact-clone`: an exact-match source scan found the
  two exhaustive matches over the same public `Severity` type at snapshot
  `c0747614`.
- `verification:severity-wire-source-reading`: direct reading confirmed that
  both matches emit `info`, `low`, `medium`, `high`, and `critical`, and that
  their callers write those values into canonical planner JSON and index-v5
  projection JSON respectively.
- `evidence:severity-wire-contract-tests`: the owning-package tests and the
  mutation-sensitive shared-helper test summarized in `VERIFICATION.md`.
- `verification:severity-wire-contract-tests`: the unchanged wire strings and
  the detected, deliberately changed mapping summarized in `VERIFICATION.md`.

The exact-match scan is an external candidate enumerator. ReviewGraphen begins
at candidate registration and does not receive discovery credit for this item.

The later `discover-exact` implementation independently rediscovered the same
pair from accepted `RustSymbolAnchorV1` facts. That later run is ReviewGraphen
candidate-discovery evidence and is summarized separately in `VERIFICATION.md`;
it does not retroactively change the provenance of the original candidate.

## Exclusion-ID preimage candidate

- `evidence:exclusion-id-source-reading`: the accepted-anchor exact-body report
  ranked the legacy-D and rule-neutral exclusion-ID functions third, and direct
  source reading confirmed identical nine-field stable-ID preimages.
- `verification:exclusion-id-source-reading`: the rule-neutral API explicitly
  promises to retain the legacy D preimage, while validation and record wrapper
  contracts remain separate.
- `evidence:exclusion-id-conformance`: the shared conformance test result after
  the decision proposal is applied.
- `verification:exclusion-id-conformance`: the mutation-sensitive comparison of
  legacy and rule-neutral record identity after execution.
