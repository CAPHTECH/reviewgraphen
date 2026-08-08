# ADR 0012: mise as the Host-Admitted Cargo Toolchain for `reviewgraphen-ingest` Tests

- Status: Accepted for v0.1 implementation
- Date: 2026-08-09
- Scope: `mise.toml`, `scripts/resolve-trusted-cargo.sh`, and
  `crates/reviewgraphen-ingest/tests/m2.rs`'s `trusted_test_cargo_admission`
  helper. Does not change `reviewgraphen-ingest`'s own public contract:
  `CargoToolAdmission`, `IngestConfig.cargo_admission`, and the strict host
  admission model that "Cargo tool admission" in
  [`docs/20_m2_ingestion_contract.md`](../20_m2_ingestion_contract.md) already
  documents are unchanged. This ADR only fixes *what supplies* the
  caller-admitted `TrustedExecutable` path for this repository's own test
  suite.

## Context

`reviewgraphen-ingest` deliberately never resolves a `cargo` executable on
its own: it never searches `PATH` and never spawns `rustup`/`mise`/`asdf`,
because even a read-only runtime probe (for example `rustup which cargo` or
`mise which cargo`) can itself touch the toolchain on an unaudited host --
for example by resolving a `rust-toolchain.toml` override to a toolchain not
yet installed. `IngestConfig.cargo_admission` instead requires an external
caller to admit an already-verified absolute `cargo` path via
`CargoToolAdmission::TrustedExecutable`.

`crates/reviewgraphen-ingest/tests/m2.rs` needs a concrete admitted `cargo`
for its Cargo-metadata-exercising tests. Before this harness, its
`trusted_test_cargo_admission` helper used only the `CARGO` environment
variable `cargo test` itself sets. That is a real admission boundary --
`cargo test` names the exact binary running the test process -- but it gives
no independent, host-level verification step: whatever `cargo` a developer's
shell happens to invoke `cargo test` with is accepted as-is, with no separate
"trust this toolchain" action, and no way to pin exactly which toolchain
version the test's Cargo metadata assertions run against independent of a
developer's ambient shell configuration.

This repository is not attempting to solve toolchain management in general.
The goal is narrower: give `tests/m2.rs` an external, explicit-consent
resolution path to the *one* pinned toolchain (`rust-toolchain.toml`'s Rust
1.95.0) that is stronger than "whatever `cargo test` happened to run with,"
while never weakening `reviewgraphen-ingest`'s own "never resolve, never
install" trust boundary -- the resolution must happen entirely outside the
crate, exactly like every other `TrustedExecutable` caller.

mise is already a plausible choice for pinning and installing that toolchain
locally: it reads `rust-toolchain.toml`-adjacent version pins, requires an
explicit `mise trust`/`mise install` step per repository, and can run project
tasks without polluting a developer's global shell `PATH`. But mise itself
must not become a second, implicit auto-installer sitting just outside the
crate's boundary -- that would just move the "audited runtime probe can touch
the toolchain" risk this crate already rejected into `scripts/`, not remove
it.

## Decision

Add a `mise.toml`-based external harness, entirely outside
`reviewgraphen-ingest`'s own crate boundary, that admits one pinned `cargo`
executable for this repository's own test suite:

- **`mise.toml` disables every mise auto-install path**: `[settings]`
  `auto_install = false`, `exec_auto_install = false`,
  `not_found_auto_install = false`, and `task_run_auto_install = false` --
  the last of these because mise's task-activation resolver script exports
  `MISE_TASK_RUN_AUTO_INSTALL=false` only after task activation itself has
  already begun, so setup must remain a separate, explicit `mise install`
  step rather than something a task invocation could trigger. `[tools]`
  pins the version explicitly,
  `rust = "1.95.0"`, matching `rust-toolchain.toml`'s `channel`. `profile =
  "minimal"` and `components = ["clippy", "rustfmt"]` are not repeated in
  `mise.toml`: host mise 2026.1.1 rejects the extended `{ version, profile,
  components }` table form for the `rust` tool as an invalid value type, so
  this ADR instead relies on mise's Rust backend's rust-toolchain.toml
  support, reading `profile`/`components` from `rust-toolchain.toml` itself
  (already pinned to the same 1.95.0). mise's Rust backend is
  rustup-managed -- `mise install` invokes rustup to install this exact
  version/profile and add these components, it is not a standalone Rust
  distribution mise builds itself -- but never installs on its own; setup is
  always the developer's own explicit action, documented in
  `DEVELOPMENT.md`.
- **`scripts/resolve-trusted-cargo.sh` resolves, but never installs, the
  pinned `cargo`.** It runs `mise exec rust@1.95.0 -- rustc --print sysroot`
  (with `MISE_OFFLINE=true` and every mise/rustup auto-install environment
  variable forced `false`, belt-and-suspenders alongside `mise.toml`'s own
  `[settings]`) to find the toolchain's real sysroot, derives `<sysroot>/bin/
  cargo`, canonicalizes it without relying on a non-portable `readlink -f`/
  `realpath`, and verifies the result is an existing, executable, regular
  file whose basename is exactly `cargo`. `mise which cargo` is deliberately
  never used: it resolves to a rustup-style proxy that re-dispatches based on
  whichever toolchain is active at call time, not the one fixed rust@1.95.0
  binary this script must name.
- **Two mise tasks expose this, both failing (never installing) when
  rust@1.95.0 is absent:** `trusted-cargo-path` prints the resolved path;
  `test-ingest` additionally exports it as `REVIEWGRAPHEN_TRUSTED_CARGO` and
  runs `cargo test -p reviewgraphen-ingest` through it. Neither task
  `depends` on a setup task -- there is no implicit "install if missing"
  fallback path a runtime task could silently take.
- **`tests/m2.rs`'s `trusted_test_cargo_admission` requires
  `REVIEWGRAPHEN_TRUSTED_CARGO`.** There is deliberately no `CARGO`
  environment-variable fallback: `cargo test`'s own `$CARGO` names whatever
  binary is running the test process, which -- when `cargo` on `PATH`
  resolves to a rustup/mise multiplexer shim -- can itself be that shim's
  own binary path rather than one fixed, independently verified toolchain;
  canonicalizing alone cannot distinguish a shim from a genuine toolchain
  binary, so it is not treated as an admission boundary. `mise run
  test-ingest` is therefore the official entry point for any test that
  needs a real, externally-admitted `cargo`. Most of `tests/m2.rs`'s tests
  do not: `TempGitRepository::request()` defaults to `CargoToolAdmission::
  Disabled` and runs under a plain `cargo test -p reviewgraphen-ingest`
  without mise at all; only tests whose own assertions require a real Cargo
  metadata result call the separate `request_with_trusted_cargo()` helper,
  which requires `REVIEWGRAPHEN_TRUSTED_CARGO`. The admitted path is
  canonicalized and checked to have the literal basename `cargo`, matching
  `admit_cargo_executable`'s own posture in
  `crates/reviewgraphen-ingest/src/git.rs`.
- **`reviewgraphen-ingest`'s own code is untouched.** It still never reads
  `mise.toml`, never spawns `mise`, and never reads `REVIEWGRAPHEN_TRUSTED_CARGO`
  itself -- only the test harness above does, and only to build an ordinary
  `CargoToolAdmission::TrustedExecutable(path)` value the crate already
  accepts from any caller. Every allow-listed `cargo` subprocess this crate
  spawns (`cargo --version`, `cargo metadata`) runs through
  `git::cargo_command`, which clears the child's entire environment
  (`Command::env_clear()`) before setting only `CARGO_HOME` (inside the
  staged snapshot), `CARGO_NET_OFFLINE=true`, `CARGO_TERM_COLOR=never`, and
  `LC_ALL=C` -- not even `PATH`, since the admitted executable is always
  invoked by its already-canonicalized absolute path. This closes a gap
  where `PATH`, `RUSTUP_TOOLCHAIN`, any `MISE_*` variable, `RUSTC`,
  `RUSTFLAGS`, or an ambient `CARGO_*` could previously reach the admitted
  binary unmodified and silently select a different toolchain, target
  directory, or registry for the same admitted executable and staged
  snapshot; `cargo_resolver_policy_fingerprint()`'s `version` was bumped to
  `3` for this reason, so `adapter_set_hash` changes if this contract's
  shape ever does.

## Consequences

### Positive

- `mise run test-ingest` gives a reproducible, explicitly-installed toolchain
  path for `reviewgraphen-ingest`'s Cargo-metadata tests, independent of
  whatever `cargo` a developer's shell happens to default to.
- Every mise-mediated auto-install path -- including task activation, via
  `task_run_auto_install = false` -- is disabled at both the project
  (`mise.toml`) and script (`MISE_*_AUTO_INSTALL`/`MISE_OFFLINE`) level, so
  this harness cannot silently install or touch the toolchain the way the
  crate's own now-rejected automatic `rustup`/`mise` resolution could have.
- `reviewgraphen-ingest`'s trust boundary is unchanged: this harness is just
  another external caller of `CargoToolAdmission::TrustedExecutable`, exactly
  like any future CI/reviewer harness would be.
- A direct `cargo test -p reviewgraphen-ingest` still works without mise for
  every test that does not need a real Cargo metadata result (the
  `CargoToolAdmission::Disabled` default), since only the tests that call
  `request_with_trusted_cargo()` require `REVIEWGRAPHEN_TRUSTED_CARGO`.

### Negative

- Running `mise run test-ingest` requires a one-time `mise trust`/`mise
  install` per clone; forgetting either step fails closed (the resolver
  script errors out) rather than installing anything automatically.
- Any test that calls `request_with_trusted_cargo()` (Cargo-metadata-path
  tests, and the two clone-identity/adversarial-config tests that compare
  their result against one) cannot run at all without `mise run
  test-ingest` -- there is no weaker fallback for those specific tests,
  unlike the pre-ADR `CARGO`-variable behavior this ADR replaces.
- This harness is coupled to mise's current `[settings]`/task schema (for
  example the removed `[settings.task]` table format an earlier mise release
  used); a future mise release changing its config schema again would need a
  corresponding `mise.toml` update, exactly as this ADR's own predecessor
  configuration already needed one.

## Alternatives considered

### A. Resolve `cargo` from `PATH`/`rustup` inside `reviewgraphen-ingest` itself

Rejected before this ADR (see `docs/20_m2_ingestion_contract.md`'s "Cargo
tool admission" section): even a read-only `rustup which cargo` probe can
touch the toolchain on an unaudited host, so no automatic resolution from
inside the crate can be made safe.

### B. Use the `CARGO` environment variable `cargo test` sets as a fallback

Rejected, including as a fallback. It gives no independent, explicit
per-repository consent step and no way to pin the toolchain version tests
run against separate from a developer's ambient shell `cargo` -- when
`cargo` on `PATH` resolves to a rustup/mise multiplexer shim, `$CARGO` can
itself be that shim's own binary path, re-dispatching to whichever
toolchain is active at call time, and canonicalizing alone cannot tell a
shim from a genuine fixed-toolchain binary. `trusted_test_cargo_admission`
now requires `REVIEWGRAPHEN_TRUSTED_CARGO` unconditionally; tests that do
not need a real Cargo metadata result instead use
`TempGitRepository::request()`'s `CargoToolAdmission::Disabled` default, so
a direct `cargo test -p reviewgraphen-ingest` without mise remains possible
for those tests without relying on `$CARGO` as a trust boundary.

### C. Use `mise which cargo`

Rejected. `mise which cargo` resolves to a rustup-style proxy shim that
re-dispatches based on whichever toolchain is active at call time, not the
one fixed `rust@1.95.0` `cargo` binary this harness must name and verify
(`scripts/resolve-trusted-cargo.sh`'s own header comment records this in
detail).

### D. Let mise tasks auto-install the toolchain on first use

Rejected. That would recreate, one layer up in `scripts/`, the same "an
audited-adjacent path can still touch the toolchain automatically" risk
`reviewgraphen-ingest`'s own strict host admission model was designed to
eliminate. Every mise auto-install path is instead disabled, and setup is
always the developer's own explicit `mise install`.
