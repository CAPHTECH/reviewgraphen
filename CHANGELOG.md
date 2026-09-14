# Changelog

All notable changes to ReviewGraphen are documented here. The project follows
Semantic Versioning while the public contract remains pre-1.0.

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

[0.1.0]: https://github.com/CAPHTECH/reviewgraphen/releases/tag/v0.1.0
