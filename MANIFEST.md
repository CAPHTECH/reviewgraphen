# ReviewGraphen v0.1.0 Manifest

> Status: public alpha release candidate
> Updated: 2026-09-15

## Entry points

- [`README.md`](README.md): product boundary, supported platforms and install.
- [`docs/index.md`](docs/index.md): design and contract reading routes.
- [`docs/23_current_capability_status.md`](docs/23_current_capability_status.md):
  dated record of implemented and experimental capabilities.
- [`AGENTS.md`](AGENTS.md): implementation invariants.
- [`DEVELOPMENT.md`](DEVELOPMENT.md): local verification.
- [`RELEASE.md`](RELEASE.md): release procedure and supported binary targets.
- [`SECURITY.md`](SECURITY.md): vulnerability reporting and support policy.

## Shipped surfaces

- The `reviewgraphen` CLI with its closed `review`, `schema` and `--version`
  surface.
- Versioned schemas, fixtures and reference scenarios.
- Linux x86-64 and Apple Silicon macOS release archives.
- [`install.sh`](install.sh), which installs the CLI and the checked-in
  ReviewGraphen skill for Codex and Claude Code from checksum-verified GitHub
  Release assets.
- Experimental benchmark binaries and contracts for structural-sloppiness and
  responsibility-family research. These remain non-authoritative and do not
  extend the production CLI.

## Design record

The numbered documents under `docs/` define the conceptual and operational
model. The complete ADR set under [`docs/adr/`](docs/adr/) is authoritative for
accepted decisions; the current series runs through ADR 0051. Program facts,
Review claims and Evidence remain separate, and benchmark findings are not
promoted to accepted product state.

## Deliberately excluded from the release

- Build outputs and local `.reviewgraphen*` run directories.
- Untracked orchestration transcripts, model outputs and scratch repositories
  under `tmp/`.
- Local benchmark runs that are not referenced fixtures or frozen public
  research artifacts.
- Credentials, provider configuration and machine-local caches.

The git tree for the release tag, not the developer worktree, is the release
manifest. See [`VALIDATION.md`](VALIDATION.md) for the checks applied to it.
