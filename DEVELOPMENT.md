# Development Harness

`scripts/ci.sh` is the single supported verification entry point. It does not
implement ReviewGraphen product behavior; the only current Rust workspace member
is the intentional double-submit counterexample fixture.

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
Coverage is explicitly skipped while the repository has no product Rust source.
The first product crate activates the lcov report, and an empty report then fails
the gate. The counterexample fixture is not used to manufacture a coverage score.

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

## Deferred checks

`cargo-mutants`, `cargo-fuzz`, and Miri are deferred until they have a real
target and interpretation policy. Their admission conditions and future
`scripts/ci.sh` extension points are recorded in
[`ADR 0009`](docs/adr/0009-rust-development-harness.md).
