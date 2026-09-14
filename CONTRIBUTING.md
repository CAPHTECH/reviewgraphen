# Contributing to ReviewGraphen

ReviewGraphen welcomes focused bug reports, reproducible evaluation results,
documentation corrections and patches that preserve its trust boundaries.

## Before opening a change

1. Read `AGENTS.md`, `README.md`, the conceptual model and the ADR governing
   the area you will change.
2. Open an issue for a schema break, public-type change, new authority path or
   security-boundary change before implementation.
3. Keep one commit centered on one design or contract decision; include its
   tests, fixtures, schema and documentation together.

## Verification

Use the pinned toolchain and run:

```bash
scripts/ci.sh fast
```

Changes to platform-specific CLI behavior must also pass the matching GitHub
Actions job. A passing test proves only the property its assertion exercises;
record untested platforms and boundaries explicitly.

## Certificate of Origin

Contributions use the Developer Certificate of Origin 1.1. Sign every commit
with `git commit -s` to certify that you have the right to submit it under this
repository's license. ReviewGraphen uses DCO sign-off and does not require a
separate contributor license agreement.

## AI-assisted contributions

The human contributor remains responsible for authorship rights, provenance,
license compatibility and verification. Identify material generated-source or
external-source provenance in the pull request. Model confidence is never a
substitute for Evidence or Verification.

## Security reports

Follow `SECURITY.md`; do not place vulnerability details in a public issue.
