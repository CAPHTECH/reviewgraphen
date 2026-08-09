# Development Harness

`scripts/ci.sh` is the single supported verification entry point. It does not
implement ReviewGraphen product behavior; the Rust workspace includes the
`reviewgraphen-core` M0/M1 domain crate and the intentional double-submit
counterexample fixture.

## Prerequisites

`rust-toolchain.toml` pins Rust 1.95.0 with rustfmt and Clippy. Install the
Python validator dependency once:

```bash
python3 -m pip install --requirement requirements/ci.txt
```

The fast gate also needs nextest:

```bash
cargo install cargo-nextest --locked --version 0.9.137
```

The scheduled gate additionally needs these tools and the Rust LLVM component:

```bash
cargo install cargo-deny --locked --version 0.18.7
cargo install cargo-llvm-cov --locked --version 0.8.6
rustup component add llvm-tools-preview
```

## Verification modes

```bash
scripts/ci.sh fast       # Python bundle validator, rustfmt, Clippy, nextest, doc tests
scripts/ci.sh coverage   # LLVM lcov at target/llvm-cov/lcov.info
scripts/ci.sh deny       # dependency advisories, licenses, bans, and sources
scripts/ci.sh heavy      # deny + coverage; used after the fast CI job
scripts/ci.sh scheduled  # fast + deny + coverage
```

The fast mode is used for pull requests. The heavy mode runs after fast on the
weekly schedule and may also be selected through GitHub Actions' manual dispatch.
The local scheduled mode runs both tiers. The Python validator
already runs `cargo test` for the reference fixture; nextest independently runs
the workspace test suite. A pass means the fixture reproduced its documented
counterexample, not that payment processing is safe.

The checked-in fixture is source evidence and is deliberately excluded from the
formatter because this harness must not rewrite existing evidence artifacts.
Every future product source is formatted with Rust 2024 before linting.
The M0/M1 `reviewgraphen-core` crate is formatted and checked by the workspace
gate. The counterexample fixture is not used to manufacture a coverage score.

## Adding a product crate

Do not add a crate until its domain contract and acceptance tests exist. Then:

1. Add it to `workspace.members`.
2. Set `lints.workspace = true` in its package manifest.
3. Use `proptest.workspace = true` and `insta.workspace = true` from
   `dev-dependencies` when the contract requires property or snapshot tests.
4. Add only tests that exercise a stated invariant, schema boundary, or report
   projection; do not add harness-only smoke tests.

`proptest` stores minimized failure cases under `proptest-regressions/` beside
the owning test source. Review and commit each generated seed. `scripts/ci.sh
fast` rejects uncommitted regression seeds or snapshots after a successful test
run.

`insta` snapshots are ordinary, reviewed test fixtures. CI verifies existing
snapshots but never updates them. Install `cargo-insta` only when the first
snapshot test is introduced, and use its review workflow before committing an
accepted snapshot.

## Rust toolchain via mise (Cargo tool admission for `reviewgraphen-ingest`)

`reviewgraphen-ingest`'s own trust boundary never resolves a `cargo`
executable on its own -- it never searches `PATH` and never spawns
`rustup`/`mise`/`asdf` (see the "Cargo tool admission" section of
[`docs/20_m2_ingestion_contract.md`](docs/20_m2_ingestion_contract.md) and
[ADR 0012](docs/adr/0012-mise-host-admitted-cargo-toolchain.md)). `mise.toml`
and `scripts/resolve-trusted-cargo.sh` are the external harness that admits
one, outside the crate, for `crates/reviewgraphen-ingest/tests/m2.rs`'s Cargo
metadata tests.

First-time setup, once per clone, is always explicit -- neither `mise.toml`
nor `scripts/resolve-trusted-cargo.sh` ever installs or downloads anything on
its own:

```bash
mise trust      # accept this repository's mise.toml
mise install    # install rust@1.95.0 (pinned in mise.toml, matching rust-toolchain.toml)
```

After that, run the ingestion tests through the mise-admitted toolchain:

```bash
mise run test-ingest
```

This runs `scripts/resolve-trusted-cargo.sh` to resolve the absolute,
mise-installed `rust@1.95.0` `cargo` executable (verified via `mise exec
rust@1.95.0 -- rustc --print sysroot`, never `mise which cargo`, which
resolves to a rustup-style proxy shim instead of the fixed toolchain binary),
exports it as `REVIEWGRAPHEN_TRUSTED_CARGO`, and runs `cargo test -p
reviewgraphen-ingest` with it. `tests/m2.rs`'s `trusted_test_cargo_admission`
helper *requires* `REVIEWGRAPHEN_TRUSTED_CARGO` -- there is no `CARGO`
environment-variable fallback, since `cargo test`'s own `$CARGO` can itself
be a rustup/mise multiplexer shim's own binary path rather than one fixed,
independently verified toolchain. Most of `tests/m2.rs`'s tests do not need
it: `TempGitRepository::request()` defaults to `CargoToolAdmission::
Disabled`, so a plain `cargo test -p reviewgraphen-ingest` (no mise) already
runs every test that does not itself assert on a Cargo-metadata-derived
fact. `mise run test-ingest` is required only for the tests that call the
separate `request_with_trusted_cargo()` helper.

Neither `mise run trusted-cargo-path` nor `mise run test-ingest` ever
installs `rust@1.95.0`: both fail with an explicit error directing you back
to `mise install` if it is not already present. `mise.toml`'s
`[settings]` (`auto_install`, `exec_auto_install`, `not_found_auto_install`,
`task.run_auto_install`, all `false`) and `scripts/resolve-trusted-cargo.sh`'s own exported
`MISE_*_AUTO_INSTALL=false`/`MISE_OFFLINE=true` environment together keep
every `mise`-mediated command in this harness from auto-installing, whether
invoked through a mise task or the script standalone. `mise.toml`'s
`[tools]` table pins only the version (`rust = "1.95.0"`); `profile =
"minimal"` and the `clippy`/`rustfmt` components come from
`rust-toolchain.toml` instead, which pins the same 1.95.0, via mise's Rust
backend's rust-toolchain.toml support (host mise 2026.1.1 rejects the
extended `{ version, profile, components }` table form for the rust tool
as an invalid value type). mise's Rust backend is rustup-managed: `mise
install` invokes rustup to install the pinned version/profile and add
those components, not a standalone Rust distribution mise builds itself.

Every allow-listed `cargo` subprocess `reviewgraphen-ingest` spawns (`cargo
--version`, `cargo metadata`) runs with its environment cleared first
(`git::cargo_command`, `Command::env_clear()`) and only `CARGO_HOME`
(inside the private staged snapshot), `CARGO_NET_OFFLINE=true`,
`CARGO_TERM_COLOR=never`, and `LC_ALL=C` set back -- not even `PATH`, since
the admitted executable always runs by its already-canonicalized absolute
path. Ambient `RUSTUP_TOOLCHAIN`, `MISE_*`, `RUSTC`, `RUSTFLAGS`, and
`CARGO_TARGET_DIR` can never reach it.

## Deferred checks

`cargo-mutants`, `cargo-fuzz`, and Miri are deferred until they have a real
target and interpretation policy. Their admission conditions and future
`scripts/ci.sh` extension points are recorded in
[`ADR 0009`](docs/adr/0009-rust-development-harness.md).
