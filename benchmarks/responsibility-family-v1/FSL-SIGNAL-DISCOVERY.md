# FSL responsibility-signal discovery calibration

Status: benchmark-only, non-authoritative calibration; 2026-09-14.

## Bound input

- repository: local FSL worktree, Git objects read at the target revision;
- base: `a370321195f7c46d5609d82102d46c3462541c0f`;
- target: `38f97bfdaf5a7d251de62dd43e37ab4b41e4ef73`;
- accepted Rust symbols: 6,453;
- eligible production functions/methods: 2,917;
- test-scope exclusions: 284;
- missing shape, signal or test-scope facts: 0.

The ReviewGraphen repository itself was attempted first. Ingest refused before
candidate discovery because a tracked 3,040,280-byte gzip artifact exceeded
the fixed 2,097,152-byte per-file admission bound. No result from that attempt
is counted here.

## Threshold calibration

The first rule (one callable-or-signature term and two operation terms) emitted
65,965 candidates from 4,252,089 evaluated distinct-shape pairs. That output is
superseded. Replaying the same accepted facts with operation Jaccard thresholds
of 200,000, 300,000, 400,000, 500,000, 600,000 and 700,000 ppm yielded 14,950,
5,372, 2,522, 1,314, 596 and 272 pairs. ADR 0049 freezes 600,000 ppm. This
snapshot must not be reused as a held-out evaluation of that threshold.

Two complete runs of the frozen rule produced identical report bytes:

```text
sha256:355af06cbb64f7de29651db6280abb5f5b3d28865e999bf277de8d9a393d4b58
```

The report contains 596 pair candidates and 1,192 candidate-member
occurrences. Candidate IDs do not contain rank.

## Decisive source reading

Rank 7 is candidate
`responsibility-family-candidate-signal:sha256:64c9ddf8db155b51763a824a76c35e08ff1c38ea359a5a51b558c3268fc65e18`:

- `rust/fslc/src/main.rs:2447-2504`, `load_approvals`;
- `rust/fslc/src/main.rs:2827-2870`, `load_approvals_for_check`.

Their accepted responsibility-shape and normalized-body hashes differ. The
pair shares the callable words `load` and `approvals`, three signature terms,
16 selective operation terms, and 842,105 ppm operation Jaccard.

Direct reading at the target revision shows a plausible common responsibility:
both functions read approval files, hash their bytes, decode versioned records,
require the `requirements_document` target, collect display records, sort the
per-file digests and hash the joined list. The purpose-specific difference is
explicit: `load_approvals` is used by document generation and verifies signed
records against a trust store; `load_approvals_for_check` reproduces already
admitted display text and intentionally does not verify the signature.

This reading makes the pair suitable for the existing responsibility-family
decision obligations. It does not decide whether to extract a shared loader,
share only conformance tests, or preserve intentional separation.

## Verification and limits

- Extractor integration tests passed 3/3 after a calibrated `cfg` false-positive
  fix; `cfg(not(test))`, `feature = "contest"` and `any(test, unix)` no longer
  become test-only facts.
- Discovery tests passed 16/16. A threshold fault and a selective-frequency
  union fault were each observed by a failing focused test before repair.
- The CLI created two fresh reports and refused an existing output with exit 5.
- The report schema rejects authority escalation and candidates below the
  subject, operation-count or Jaccard minima.

`[U]` The remaining 595 pairs were not source-reviewed. External type
resolution, aliases, macro expansion, dynamic dispatch, cross-language
responsibility and same-responsibility implementations without shared retained
vocabulary remain outside this candidate universe. No abstraction or target
code change was made, and no maintenance-improvement claim follows from this
calibration.
