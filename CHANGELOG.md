# Changelog

All notable changes to ReviewGraphen are documented here. The project follows
Semantic Versioning while the public contract remains pre-1.0.

## [0.2.0] - 2026-09-26

### Added

- Source review v6 (`reviewgraphen.source_review_request.v6`, ADR 0053): one
  change-driven obligation route for Rust, TypeScript and Kotlin. For a
  base..target change it emits deferred obligations for changed public
  functions **and methods**, every resolved caller of a changed callable
  (labelled `exact` / `name_only` / `ambiguous`), removed public callables,
  and gaps for sources that could not be fully analyzed. Output is
  machine-readable JSON only (`source-review.run.v1.json`).
- Common-v5 generic review for TypeScript
  (`reviewgraphen.generic_review_request.v5`, registry r2), with its
  extraction, ingestion, run and artifact-manifest schemas.
- Rust generic review v4 production profile with the public-function Node
  obligation (ADR 0040).

### Changed

- Artifact roots are written descriptor-relative. Symlinked components,
  swapped parents and foreign or non-empty roots are refused, and a failed
  unwind is reported as `unwind incomplete` instead of claimed as removed.
- Git reads ignore host and user Git configuration and replacement refs,
  never lazily fetch from a partial-clone promisor, and pin the diff
  algorithm.
- Legacy capability-gap obligations are emitted only for rules that have
  an eligible source candidate.
- Rust policy v4 changes policy-derived identifiers. TypeScript rows whose
  read failed now report `bytes_read: false`.
- **Breaking:** the v5 route accepts only TypeScript registry-r2 requests.
  Registry-r1 and Kotlin requests are refused with exit 3. Use source review
  v6 for Kotlin and for changed-callee relations.

### Known limitations

- Source review v6 resolves method calls by name without type inference, so
  such edges are `name_only` or `ambiguous`. Rust method calls are never
  `exact`. See ADR 0053 for the full list.
- Compile-red contracts for the unimplemented TypeScript I2 admission build
  only with `--cfg reviewgraphen_unimplemented_contracts`.

## [0.1.0] - 2026-09-15

### Added

- Obligation-driven review contracts separating Program facts, Review claims,
  Evidence, Verification and human acceptance.
- Deterministic Rust/Git ingestion, bounded context projection, coverage,
  obstruction and snapshot-staleness records.
- Provider-free and verified-replay `review` paths plus embedded schema
  inspection and validation in the `reviewgraphen` CLI.
- Experimental, non-authoritative responsibility-family discovery, decision
  support, conformance and reinspection research surfaces.
- Native Apple Silicon macOS CLI build support alongside the complete Linux
  implementation.
- Checksum-verified release installer for the CLI and the ReviewGraphen skill
  for Codex and Claude Code.

### Known limitations

- ReviewGraphen does not claim complete defect detection or demonstrated
  superiority over ordinary human or LLM review.
- Durable Store operations, Store-bound report-v4/v5 semantic validation and
  bubblewrap-backed live-provider isolation remain Linux-only.
- macOS support is CI-verified on macOS 15 Apple Silicon; Intel macOS is not in
  the supported matrix.
- Responsibility-family discovery supplies snapshot-bound candidates, not
  accepted semantic equivalence or automatic abstraction decisions.

[0.2.0]: https://github.com/CAPHTECH/reviewgraphen/releases/tag/v0.2.0
[0.1.0]: https://github.com/CAPHTECH/reviewgraphen/releases/tag/v0.1.0
