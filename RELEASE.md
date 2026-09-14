# Release process

## Supported binary targets

- `x86_64-unknown-linux-gnu`: complete CLI and Linux durable Store boundary.
- `aarch64-apple-darwin`: portable CLI boundary on macOS 15+ Apple Silicon.

The release archive contains `reviewgraphen`, `LICENSE`, `NOTICE` and
`README.md`. Every archive has a sibling SHA-256 file.

Each release also contains `install.sh`, its checksum, a version marker and a
versioned `reviewgraphen-skill` archive. The installer supports both binary
targets and installs the same checked-in skill for Codex and Claude Code.

## Release gate

1. Confirm `CHANGELOG.md` and the workspace version describe the same release.
2. Run `scripts/ci.sh fast` and `scripts/ci.sh deny` on Linux.
3. Confirm the macOS CLI GitHub Actions job is green on the release commit.
4. Confirm `git diff --check`, the publication audit, secret scan and embedded
   third-party attribution checks are clean.
5. Confirm the release commit is on `main` and all required checks succeeded.
6. Create the annotated tag `vX.Y.Z` and push it. The Release workflow creates
   a draft, uploads both platform archives and checksums, and publishes only
   after every build succeeds.

Do not manually publish a partial draft. A failed build leaves the draft
unpublished for diagnosis.
