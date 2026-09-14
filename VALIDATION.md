# Release Validation

> Release candidate: 0.1.0
> Validation date: 2026-09-15

## Verified locally

- `scripts/ci.sh fast`: bundle validation, CI admission checks, installer
  tests, rustfmt, clippy with warnings denied, 1,204 nextest tests and all doc
  tests passed; 2 nextest tests were explicitly skipped by profile.
- `scripts/ci.sh deny`: advisories, dependency bans, licenses and sources
  passed. Duplicate dependency versions remain warnings under the declared
  policy.
- `python3 scripts/validate_bundle.py`: schema, reference, Markdown and fixture
  validation passed.
- `cargo test --locked -p reviewgraphen-cli`: 10 tests passed (6 unit, 4
  integration); doc tests passed.
- `cargo check --locked --target aarch64-apple-darwin -p reviewgraphen-cli`:
  Apple Silicon macOS CLI cross-check passed.
- `cargo check --locked --tests --target aarch64-apple-darwin -p
  reviewgraphen-cli`: macOS CLI test targets compiled.
- `sh scripts/test-install.sh`: a synthetic Release installed the CLI and the
  same skill for Codex and Claude Code; latest-version resolution, unmanaged
  skill refusal, backup-preserving force update and checksum rejection passed.
- A release-layout dry run packaged the release-built Linux binary and the
  repository skill, then installed all three targets with `install.sh` into
  temporary destinations. The installed CLI reported `reviewgraphen 0.1.0`,
  and both installed skill files were byte-identical to the repository source.
- `git diff --check`: passed.

The CLI safe-input test was also fault-injected by removing `O_NOFOLLOW`; its
symlink rejection test failed, and passed again after restoring the guard.

## Required remote release evidence

The release is not complete until GitHub Actions passes both the Linux fast job
and the native `macos-15` Apple Silicon job on the release commit. The tag
workflow must then build both archives, upload sibling SHA-256 files, the
installer, version marker and skill archive, and publish no partial draft.

## Claim boundary

These results establish only the commands and snapshots stated above. They do
not establish complete defect detection, reviewer superiority, Intel macOS
support, or macOS support for the Linux-only durable Store and live-provider
isolation boundaries.
