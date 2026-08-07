# ADR 0009: Establish a Minimal Rust Development Harness Before Product Crates

- Status: Accepted for v0.1 implementation
- Date: 2026-08-07

## Context

ReviewGraphen has an implementation-ready design bundle and a deliberately
vulnerable Rust reference fixture, but no product crate yet. Starting product
logic without a pinned toolchain, reproducible quality gates, dependency policy,
or a single CI entry point would make the first implementation contracts harder
to reproduce and audit.

The current fixture is meaningful executable evidence for the double-submit
scenario. It must stay distinct from ReviewGraphen product logic: its passing
test reproduces a counterexample and does not establish the business invariant.

## Decision

Create a virtual root Cargo workspace that registers only the existing fixture
until a ReviewGraphen crate has a defined contract. Pin Rust 1.95.0 with
`rust-toolchain.toml`, centralize future test dependencies (`proptest` and
`insta`) and workspace lints in the root manifest, and make `scripts/ci.sh` the
only verification entry point.

The formatter and product workspace use Rust's 2024 edition. The CI entry point
formats every product source but deliberately excludes the fixture: it is
evidence linked from the design bundle and must remain byte-for-byte unchanged.

The fast gate runs:

```text
Python bundle/schema/semantic validation (including its fixture test)
rustfmt check
Clippy with warnings denied
nextest unit/integration tests
cargo doc tests
```

The scheduled/manual-heavy gate additionally runs Cargo dependency policy and
LLVM coverage. It depends on the fast job instead of running fast checks twice.
GitHub Actions invokes only these entry-point modes, with read-only repository
permission.

`proptest-regressions/**/*.txt` and `snapshots/` are version-controlled test
artifacts. Generated regression seeds cause the entry point to fail until they
are reviewed and committed. Snapshot updates are checked the same way; CI never
accepts or rewrites them.

## Consequences

### Positive

- The design bundle, fixture, and future Rust crates share one pinned toolchain
  and visible validation policy.
- The intentionally vulnerable fixture remains executable without being
  represented as a product correctness test.
- Future deterministic logic can add property and snapshot tests without a new
  test-framework decision.
- Heavy checks do not lengthen normal pull-request feedback.
- Dependency, coverage, and test output are available as separately scoped
  evidence rather than a single ambiguous CI result.

### Negative

- The virtual workspace has no ReviewGraphen product crate until a domain
  contract is implemented.
- Local fast verification requires `cargo-nextest`; scheduled verification also
  requires `cargo-deny`, `cargo-llvm-cov`, and `llvm-tools-preview`.
- The fixture is executed by both the Python validator and nextest, favoring
  independent validator evidence over minimum runtime.

## Deferred extension points

The following are intentionally not installed or run yet. No product crate or
fuzz target exists that makes their result meaningful.

| Tool | Admission condition | Scheduled entry-point addition |
| --- | --- | --- |
| `cargo-mutants` | Deterministic core behavior has mutation score expectations and explicit equivalent-mutant policy. | Add a `mutants` mode to `scripts/ci.sh`; upload its report as an artifact. |
| `cargo-fuzz` | A security-relevant parser, normalization boundary, or event decoder has a structured fuzz target and seed corpus. | Add `fuzz/` with tracked targets/corpus and a bounded scheduled run. |
| Miri | A crate has unsafe code, aliasing-sensitive state, or concurrency behavior with a defined supported target. | Add a nightly-only `miri` mode separate from the stable fast gate. |

## Invariants

- CI scripts never interpret a passing counterexample fixture as a sign-off.
- Coverage is explicitly skipped before product Rust sources exist; once active,
  an empty report fails the gate.
- `proptest` failure persistence and insta snapshots are not ignored by Git.
- Future workspace members opt into `lints.workspace = true`.
- Tool installation and CI execution stay outside ReviewGraphen reviewer
  capabilities; this harness does not grant arbitrary shell execution.
- Product logic, schemas, and fixture semantics require their own contract tests;
  this ADR does not justify dummy tests.

## Alternatives considered

### A. Add an empty product crate with smoke tests

Rejected. It would create tests without domain behavior and falsely suggest that
ReviewGraphen implementation has started.

### B. Keep the fixture outside the workspace

Rejected. That would split Rust formatting, linting, and test configuration from
the only checked-in executable reference scenario.

### C. Run every quality tool on every pull request

Rejected. Dependency policy and coverage are valuable but are materially slower
and do not need to block rapid feedback when a scheduled/manual gate records
them.
