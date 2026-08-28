# CI gate repair — requirement recovery

Stage: B (normal), as assigned. This task changes a verification gate rather
than a public API, schema, persistent state, or product type. It nevertheless
has explicit non-change surfaces and requires a checkpoint.

## 1. Goal

Repair the retained-link and product-source-selection defects without turning a
genuine product failure into success. Show by focused fault injection that the
gate rejects the applicable real failures and ignores untracked worktree
pollution; the clean full-`fast` completion is excluded from this task after
the unrelated nextest timeout observation.

## 2. Observed behavior

- Observed on `0558383a758e6833acd176213bf657480db45eed`: the worktree was
  clean at the start of recovery. `python3 scripts/validate_bundle.py` exited
  1 before rustfmt, reporting the first missing m20 → m18 link.
- Observed: the validator is fail-fast for Markdown links
  (`scripts/validate_bundle.py:1429-1454`), and is the first fast stage
  (`scripts/ci.sh:92-96`).
- Observed by reproducing the validator's declared-root/link-resolution rules:
  five links are missing, all from
  `benchmarks/m20-changed-public-callee-utility-v1/MODEL_PIN_RATIONALE.md`:
  lines 34, 35, 38, and 44 target an untracked/nonexistent m18 directory;
  line 55 targets nonexistent `m17-casegraphen-controlled-review-v1/RESULT.md`.
  The validator itself reports only the first because it raises immediately.
- Observed: `run_rustfmt()` finds every `*.rs` in the checkout except the
  double-submit fixture and `target` (`scripts/ci.sh:69-90`). It therefore
  observes benchmark evidence and untracked files, not only product source.
- Observed: `rustfmt --edition 2024 --check` over the 77 files under `crates/`
  after excluding `crates/**/tests/fixtures/**` exited 0. The three CaseGraphen
  fixture files make the full 86-file `crates/` selection fail; the fixture
  provenance is recorded in
  `crates/reviewgraphen-runtime/tests/fixtures/real-subject-pairs.v1.json:41-54`.
- Observed: `bash scripts/test-ci-admission.sh` exits 0 and internally proves
  rejection of a newline-bearing admitted Cargo path (`scripts/test-ci-admission.sh:33-41`).
- Observed on this machine: `cargo-nextest` is not installed in the system
  Cargo bin directory, so an unmodified `scripts/ci.sh fast` cannot reach a
  complete local pass unless its PATH includes the separately installed
  `/tmp/ci-gate-tools/bin/cargo-nextest`. This is a host-tool availability
  fact, not a `ci.sh` contract change; CI installs the pinned command before
  invoking fast (`.github/workflows/ci.yml:62-66`).
- Observed: `.github/workflows/ci.yml:65-66` runs `./scripts/ci.sh fast`
  directly; no `continue-on-error`, `|| true`, or `set +e` form was found in
  `.github/workflows/ci.yml` or `scripts/` for a validation stage. The coverage
  artifact upload is explicitly non-gating (`.github/workflows/ci.yml:114-120`).
- Inference/unknown: whether the five missing benchmark artifacts should be
  committed, or their frozen m20 prose changed to a different retained source,
  is a content-owner decision. It is not inferred from the technical failure.

## 3. Explicit requirements

> `scripts/ci.sh fast` が
>
> 1. **clean な tracked checkout で exit 0 になる**（GitHub CI が緑になる）
> 2. **product source の実際の欠陥では確実に exit 1 になる**（故障注入で証明する）
> 3. **未追跡の作業ツリー汚染には反応しない**

> **gate を緩めて failure を消さないでください。**「product source を走査対象から外す」は修正ですが、「落ちるファイルを個別に除外リストへ足す」は、理由を記録しない限り隠蔽です。

> `crates/` の product source を format しないでください。

> **外部由来ソース（casegraphen fixture、benchmark の patched-*.rs）を rustfmt で書き換えないでください。**

> **どちらを採るかが決まるまで、欠陥 1 の実装には入らないでください。**

> 各段（`validate_bundle.py` / `test-ci-admission.sh` / `rustfmt` / `clippy` / `nextest` / `cargo test --doc` / `check_reviewed_test_artifacts`）について、**わざと壊した状態で非ゼロ終了することを確認**し、**注入した故障の内容を記録**してください。

## 4. Recovered constraints

- `scripts/ci.sh` is the sole supported local and CI verification entry point
  (`scripts/ci.sh:1-13`; `DEVELOPMENT.md:3-6`; ADR 0009,
  `docs/adr/0009-rust-development-harness.md:20-43`). The repair must retain
  this invocation boundary.
- Fast stages are ordered and fail-fast: bundle validation, admission test,
  trusted-Cargo admission, rustfmt, Clippy, nextest, doctests, then reviewed
  artifacts (`scripts/ci.sh:92-108`). A test of an individual function does
  not replace a full `fast` pass.
- The counterexample fixture is source evidence and must remain byte-for-byte
  unchanged, so it is deliberately outside formatting (`scripts/ci.sh:73-75`;
  `DEVELOPMENT.md:48-52`; ADR 0009,
  `docs/adr/0009-rust-development-harness.md:26-28`).
- External CaseGraphen fixture files are source slices with recorded external
  provenance, not product code (`crates/reviewgraphen-runtime/tests/fixtures/real-subject-pairs.v1.json:41-54`).
  The formatter scope must exclude this class as a class, not as individual
  failing filenames.
- The current Cargo workspace contains product crates and the intentionally
  excluded double-submit fixture (`Cargo.toml:1-19`). `cargo fmt --all` would
  include the fixture, so it cannot satisfy the evidence-preservation boundary.
- The bundle validator is defined to validate Markdown relative links in
  `schemas/`, the m20 bundle, and `docs/`
  (`scripts/validate_bundle.py:23-34`, `1429-1454`), as declared to users in
  `MANIFEST.md:75-90`. Broken retained links are validator defects, not an
  allowed skip.
- m20 calls this rationale its sole prose evidence narrative and binds it at
  protocol freeze (`benchmarks/m20-changed-public-callee-utility-v1/PROTOCOL.md:67-70`,
  `351-354`, `403-406`;
  `benchmarks/m20-changed-public-callee-utility-v1/EVALUATOR_SPEC.md:77-83`).
  Rewriting the citations or wording is a content decision, even if it does
  not change the evaluator's current freeze manifest.
- Uncommitted proptest regressions and insta snapshots must fail the gate
  (`scripts/ci.sh:50-67`; `DEVELOPMENT.md:65-73`). This guard is intentionally
  scoped to those reviewed executable artifacts, not a general dirty-tree ban.
- Review-feedback constraint (checkpoint): the root question is every script
  that chooses product source by scanning the worktree, not only rustfmt. A
  scan of the complete `scripts/` set (`ci.sh`, `validate_bundle.py`,
  `test-ci-admission.sh`, `resolve-trusted-cargo.sh`, and
  `custodian_sign_anchor.py`) found exactly two such selections: rustfmt and
  coverage in `scripts/ci.sh`. `validate_bundle.py` recursively scans only its
  declared bundle roots (`scripts/validate_bundle.py:23-34`, `65-72`), which
  is a different document-validation contract; the other three scripts do not
  select Rust/product sources. Both selected sites must share one helper.
- Review-feedback constraint (blocker): **検査の失敗経路が「ツールが無ければ
  黙って通る」形になっていてはならない。** Any new assertion must distinguish
  an expected negative result from a missing/failed tool. In particular,
  commands in an `if` condition and producers behind process substitution need
  an explicit nonzero propagation path; `set -e` alone does not provide it.
- Blocker follow-up sweep: searched the complete
  `scripts/test-ci-rustfmt-scope.sh` and the changed `scripts/ci.sh` surface
  for conditional external commands, undeclared `rg`/`jq`/`which`, `|| true`,
  `set +e`, workflow-tolerance equivalents, and subshell/process-substitution
  failure loss. Before repair it found two fail-open classes: one `rg` negative
  assertion and one shared `git ls-files` producer used by rustfmt and coverage.
  After repair the count is **0**: direct required-tool calls are fail-fast;
  the `grep` negative assertion accepts only exit 1 and propagates every other
  status; `git ls-files` writes and checks a temporary NUL list before either
  consumer uses it. The only retained conditional external checks are (a)
  `command -v mise`, which exits 127 when absent; (b) the existing documented
  non-Git artifact-guard return; (c) checked `git ls-files`; and (d) checked
  cleanup `rm`. The three test subshells are simple commands under `set -e`, so
  their nonzero statuses propagate to the script.
- **G1 — negative assertions separate their claimed axis.** The scope test now
  has two isolated miniature repositories: (1) one tracked
  `crates/product/src/lib.rs` plus one **untracked**
  `crates/untracked-pollution.rs`, which proves tracked-only selection without
  changing the product root; and (2) two tracked `crates/` files differing only
  by `tests/fixtures/`, which proves the external-evidence-class exclusion.
  Sweep result: **2 negative input-membership assertions, 2 isolated; 0
  remaining confounded assertions.** The prior benchmark-path injection was
  the sole confounded assertion and is replaced, not retained.
- **G2 — every scope-test stub/fake has an observed failure path.** Sweep
  result: **5 replacements, 5 checked**: `rustfmt` returns injected 73 and
  `run_rustfmt` returns 73; `load_product_rust_sources` returns 74 and
  `run_coverage` returns 74; `require_cargo_subcommand` returns 75 and
  `run_coverage` returns 75; the `cargo llvm-cov` fake returns 76 and
  `run_coverage` returns 76; and the `cargo metadata` fake is exercised once
  for the accepted `crates/`/evidence layout and once for a rejected external
  product member (exit 1). There is no `collect` replacement in this script
  (**0 occurrences**). Success-only fakes from the prior version are not
  retained. `run_rustfmt` and `run_coverage` now explicitly return source,
  prerequisite, and tool failures even when called from an `if` condition.
- **G2 follow-up — assertions themselves are fail-closed.** The initial
  ad-hoc direct G1 command incorrectly collapsed both a matching and a
  non-matching `grep` into status 1. It was not committed and is discarded as
  evidence. The current direct harness uses three independent predicates:
  argument-record existence, tracked-source presence, and untracked-source
  absence; the latter captures `grep` status inside its `else` branch. Sweep
  result: **20 assertions, 20 falsified one at a time, 20 nonzero exits; 0
  false conditions passed.** The falsified assertions were: initial argument
  record; edition, 2024, check, and representative source; rustfmt status;
  G1 record/tracked/absence; fixture record/tracked/absence; accepted layout,
  rejected layout, and metadata status; empty-coverage diagnostic and
  `llvm-cov` command; then selector, cargo-subcommand, and cargo statuses.
  Each mutation was an isolated copy of
  `scripts/test-ci-rustfmt-scope.sh` run as
  `CI_GATE_TEST_ROOT="$PWD" bash "$MUTATED_COPY"`; each recorded exit 1.
  No assertion that failed to reject its false state remains.
- **G3 — test writes are private.** Sweep result: **18 direct write
  destinations, 0 outside the `mktemp -d` root**:
  `$test_root/{bin,tmp}/`, `$test_root/bin/rustfmt`,
  `$test_root/rustfmt-arguments`,
  `$test_root/tracked-only-repo/crates/product/src/lib.rs`,
  `$test_root/tracked-only-repo/crates/untracked-pollution.rs`,
  `$test_root/tracked-only-repo/.git/**`,
  `$test_root/tracked-only-rustfmt-arguments`,
  `$test_root/fixture-repo/crates/product/src/lib.rs`,
  `$test_root/fixture-repo/crates/product/tests/fixtures/external.rs`,
  `$test_root/fixture-repo/.git/**`,
  `$test_root/fixture-rustfmt-arguments`,
  `$test_root/coverage-empty-output`,
  `$test_root/coverage-cargo-arguments`,
  `$test_root/coverage-nonempty/`,
  `$test_root/coverage-nonempty/lcov.info`,
  `$test_root/tmp/reviewgraphen-workspace-metadata.*`, and
  `$test_root/tmp/reviewgraphen-product-rust-sources.*`.
  The coverage fake receives an explicit private output path; it never writes
  `target/llvm-cov/lcov.info`. `cargo metadata` and `rustfmt` are read/check
  calls; no repository evidence path is written by the script.
- **G4 — gate assumptions are both enforced and canonical.** The gate now
  obtains workspace-member manifest paths from `cargo metadata --locked
  --no-deps`, permits product members only under `crates/`, and permits exactly
  `examples/double-submit-payment/fixture` as byte-preserved external evidence.
  A different external member fails before source selection. `DEVELOPMENT.md`
  records that layout and Bash >= 4.3. Sweep result: **4 new/relied-on
  assumptions, 4 canonicalized** — Bash >= 4.3 for `local -n`; Python 3 for
  the existing validator and metadata projection; Cargo/Rust 1.95 for the
  workspace and metadata query; and the `crates/` product-root/one external
  evidence-member layout. The first three are prerequisites; the fourth is in
  the formatter/gate contract. No unrecorded directory-placement assumption
  remains.
- **G5 — reproducible fault records.** Every row below now gives the complete
  gate-stage command (and the focused extra filter where used). The new G1/G2
  injections are executable subcases of `bash scripts/test-ci-rustfmt-scope.sh`
  and retain their injected statuses in its assertions.

## 5. Affected contracts

- `scripts/ci.sh`: fast-stage ordering, failure propagation, rustfmt source
  selection, coverage source-presence selection, and reviewed-artifact guard.
- `.github/workflows/ci.yml`: direct CI caller and fast/heavy dependencies.
- `DEVELOPMENT.md`: public statement of the sole entry point and formatter
  scope/evidence boundary.
- `scripts/validate_bundle.py`: declared Markdown-link contract (no change is
  proposed before a content decision).
- `MANIFEST.md`: declaration of bundle validator behavior (no change currently
  proposed).
- `Cargo.toml`: workspace-member/evidence-fixture boundary (no change proposed).
- `benchmarks/m20-changed-public-callee-utility-v1/MODEL_PIN_RATIONALE.md`,
  `PROTOCOL.md`, and `EVALUATOR_SPEC.md`: frozen benchmark evidence contract;
  any link repair here is blocked on content-owner direction.
- ADR 0009 and ADR 0012: harness and admitted-Cargo constraints; no ADR change
  is proposed because this changes neither public product contract nor schema.

## 6. Scope

Planned changes after checkpoint, subject to the link decision:

- `scripts/ci.sh`: replace whole-worktree Rust discovery with deterministic,
  tracked product-source discovery under the current workspace product roots,
  excluding the documented external-fixture class; share that discovery with
  `run_coverage()` so fast and heavy/scheduled modes cannot silently diverge.
- A focused shell regression script under `scripts/` for formatter scope and
  failure propagation, if one is needed to make the scope mechanically
  repeatable.
- `DEVELOPMENT.md`: state the resulting product/evidence formatter boundary.
- Only after an explicit content decision: the selected m20/m17/m18 evidence
  files or the m20 rationale links/prose necessary to make every retained link
  valid.

Not changing:

- all Rust product sources under `crates/` (including formatting bytes);
- external source evidence in `crates/**/tests/fixtures/**`, all benchmark
  `patched-*.rs`, and `examples/double-submit-payment/fixture/**`;
- public APIs, Rust types, schemas, event/persistent state, Cargo package
  membership, toolchain pinning, CI permission model, or reviewer capabilities;
- workflow tolerance semantics (`continue-on-error`, `|| true`) and the
  validator's declared validation roots;
- m20 frozen rationale/protocol/evaluator documents until the user chooses the
  evidence-publication versus citation-revision path.

## 7. Compatibility

- A formatted product source must still cause rustfmt to exit nonzero; direct
  rustfmt invocation is the enforced checker (`scripts/ci.sh:89`).
- The fast CI caller must continue to fail a PR/push when `fast` fails
  (`.github/workflows/ci.yml:29-66`).
- The external Cargo admission test keeps its three regression branches and
  must remain a fast stage (`scripts/test-ci-admission.sh:17-41`; ADR 0012,
  `docs/adr/0012-mise-host-admitted-cargo-toolchain.md:115-125`).
- Clippy, nextest, doctests, and reviewed-test-artifact checking remain
  workspace/contract checks with their current flags and order
  (`scripts/ci.sh:97-107`).
- `run_coverage()` must use the same product-source discovery as rustfmt before
  deciding whether coverage is applicable; scheduled/heavy invokes it from CI
  (`scripts/ci.sh:110-128`; `.github/workflows/ci.yml:68-75`, `105-112`).
- The double-submit fixture stays executable evidence but is never a product
  safety sign-off (`DEVELOPMENT.md:39-52`; ADR 0009,
  `docs/adr/0009-rust-development-harness.md:14-16`).

## 8. Verification

Every injection is temporary, made in an isolated `/tmp` worktree or a
transactionally cleaned temporary file, and must produce the stated nonzero
exit before cleanup. The final clean checkout must then pass the matching stage.

| Stage | Pass command | Deliberate fault | Expected failure |
| --- | --- | --- | --- |
| bundle validator | `python3 scripts/validate_bundle.py` | Add one temporary missing relative link inside a validator-root Markdown file after the retained-link decision is fixed. | exit 1, `broken link` names the injected target. |
| admission test | `bash scripts/test-ci-admission.sh` | In an isolated copy, make the test's valid-path assertion false (or make its stub emit an invalid path without updating the expected rejection). | exit nonzero; `run_fast` must not advance beyond this stage. |
| rustfmt | source `ci.sh`; `run_rustfmt` | Make one product `crates/.../src/*.rs` file deliberately non-formatted. | rustfmt exit 1/123 and names that product file. |
| rustfmt pollution immunity | source `ci.sh`; `run_rustfmt` | Add a deliberately non-formatted untracked `*.rs` outside product scope and retain an external fixture. | exit 0; neither path is formatted or reported. |
| Clippy | `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::dbg_macro -D clippy::todo -D clippy::unimplemented` | Introduce a temporary `dbg!` in a product test/source. | nonzero with denied `clippy::dbg_macro`. |
| nextest | `cargo nextest run --workspace --all-features --profile ci` | Add a temporary failing product test. | nonzero and failing test identity appears. |
| doctests | `cargo test --workspace --all-features --doc` | Add a temporary compile-failing Rust doc example in a product crate. | nonzero and doctest failure appears. |
| reviewed artifacts | source `ci.sh`; `check_reviewed_test_artifacts` | Create an uncommitted `**/snapshots/*.snap.new` or `**/proptest-regressions/**/*.txt`. | exit 1 and status path appears. |

The original final-pass plan was `./scripts/ci.sh fast` with an explicitly
admitted Cargo path. The user subsequently removed complete `fast` execution
from this task's completion condition because the independently owned nextest
timeout blocker prevents it. Coverage/deny are not fast stages and are out of
this repair's required validator column unless the change affects their
duplicate source-discovery code.

### Fault-injection results (isolated clone at `d899ceb`)

`$CLONE` below was the isolated clone; every command starts in that clone and
all injected files were removed before the recorded clean check. These are the
complete gate-stage commands, including the focused filter where the full stage
was followed by a focused reproduction.

| Stage | Complete command | Injected fault | Observed result |
| --- | --- | --- | --- |
| bundle validator | `cd "$CLONE" && python3 scripts/validate_bundle.py` | Added `docs/ci-gate-injection.md` with a missing relative target. | exit 1; `broken link` named `docs/does-not-exist.md`. |
| admission test / fast propagation | `cd "$CLONE" && REVIEWGRAPHEN_TRUSTED_CARGO="$TRUSTED_CARGO" bash scripts/ci.sh fast` | Added an explicit diagnostic and `exit 1` to `test-ci-admission.sh`. | exit 1 after the bundle pass; diagnostic was `injected test-ci-admission failure`. |
| rustfmt | `cd "$CLONE" && bash -c 'source scripts/ci.sh; run_rustfmt'` | Added unformatted valid Rust `const CI_GATE_RUSTFMT_INJECTION: ()=();` to `crates/reviewgraphen-core/src/lib.rs`. | exit 1; rustfmt named that file and the required spacing diff. |
| rustfmt pollution immunity (superseded) | `cd "$CLONE" && bash -c 'source scripts/ci.sh; run_rustfmt'` | Added untracked unformatted `benchmarks/ci-gate-untracked-pollution.rs`. | exit 0; the file was absent from rustfmt input. This test was replaced by the G1 same-root injection below because its axes were confounded. |
| Clippy | `cd "$CLONE" && cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::dbg_macro -D clippy::todo -D clippy::unimplemented` | Added a dead-code-allowed function containing `dbg!(())` to product source. | exit 101; `clippy::dbg_macro` was denied (and `let_unit_value` was also denied by `-D warnings`). |
| nextest | `cd "$CLONE" && cargo nextest run --workspace --all-features --profile ci -E 'test(ci_gate_nextest_injection)'` | Added a failing `ci_gate_nextest_injection` product test. The full workspace stage reported the failing test; this recorded focused command uses the same workspace/features/profile flags. | exit 100; panic diagnostic was `injected nextest failure`. |
| doctest | `cd "$CLONE" && cargo test --workspace --all-features --doc CiGateDoctestInjection` | Added public `CiGateDoctestInjection` with `let _: () = 1;` in a Rust doc example. | exit 101; rustc reported `E0308 expected (), found integer`. |
| reviewed artifacts | `cd "$CLONE" && bash -c 'source scripts/ci.sh; check_reviewed_test_artifacts'` | Added untracked `crates/reviewgraphen-core/snapshots/ci-gate-injection.snap.new`. | exit 1; the reviewed-artifact guard named that path. |
| scope-test tool availability (superseded) | `cd "$CLONE" && PATH="$GREP_127_WRAPPER:$PATH" bash scripts/test-ci-rustfmt-scope.sh` | Prepend a wrapper that returned 127 only for the negative `grep -E` assertion while allowing fixed `grep -F` checks. | exit 127; `rustfmt scope assertion failed: grep exit=127`. The `grep -E` assertion no longer exists; G1 uses explicit exact-path assertions. |

### G1/G2 fault-injection results (current branch)

| Stage | Complete command | Injected fault | Observed result |
| --- | --- | --- | --- |
| G1 tracked-only selector | `bash scripts/test-ci-rustfmt-scope.sh` | The script creates a temporary Git repository with tracked `crates/product/src/lib.rs` and untracked `crates/untracked-pollution.rs`, then invokes `run_rustfmt` through its argument-recording stub. | exit 0 for the test; the assertion confirms the tracked source is present and the untracked same-root file is absent. |
| G1/G2 direct harness | `bash scripts/test-ci-rustfmt-scope.sh` | Its G1 subcase uses a tracked product file and an untracked same-root Rust file; its G2 subcase makes rustfmt return 73. | G1 asserts argument-record presence, tracked-source presence, and untracked-source absence independently. G2 asserts `run_rustfmt` returns 73. The earlier status-collapsing ad-hoc form is discarded. |
| G2 rustfmt propagation | `bash scripts/test-ci-rustfmt-scope.sh` | The rustfmt stub returns 73 after recording its arguments. | exit 0 for the test; its assertion confirms `run_rustfmt` exits 73. |
| G2 coverage replacement propagation | `bash scripts/test-ci-rustfmt-scope.sh` | Selector, cargo-subcommand, and cargo stubs separately return 74, 75, and 76. | exit 0 for the test; its assertions confirm `run_coverage` returns 74, 75, and 76 respectively. |

All injected paths and product-source edits were removed from the isolated
clone before clean verification; `git status --porcelain` there was empty.

### Out-of-scope nextest timeout observation

At gate commit `53c8acd`, clean `./scripts/ci.sh fast` was run with the CI
nextest version on PATH and an absolute mise-admitted Cargo path. It exited
**100** in `cargo nextest`: 1,110 tests passed, and these two tests timed out
at the profile's 120-second limit:

- `reviewgraphen-report::generic_non_authority v3_positive_d_pair_is_projected_from_basis_bound_run`
- `reviewgraphen-runtime::generic_v3 v3_positive_pair_reaches_context_construction`

To separate this from the present repair, the same nextest command was run in
a clean detached clone at base `0558383a758e6833acd176213bf657480db45eed`.
It also exited 100 and timed out the same two test identities at 120 seconds.
That base-only reproduction did not supply `REVIEWGRAPHEN_TRUSTED_CARGO`, so
ten ingestion tests additionally failed their separate admission precondition;
they do not alter the relative current-versus-base comparison.

This original observation was made under high host load: the user recorded
load average **12.57 on 16 cores** (with two other concurrent Cargo tests).
Therefore the original observation has an unresolved load confound, and the
absolute claim that either test intrinsically exceeds 120 seconds is
**unconfirmed**. Before the subsequent priority change arrived, the first
focused command had already started at one-minute load 2.71 and ended exit 100
at nextest's `120.125s` timeout; the second focused command was interrupted
after its `>60s` slow notice when that change arrived. These uncontrolled
partial measurements do not classify, resolve, or expand this gate-repair
task. No timeout/profile relaxation was made. Test performance remains a
separate task if its owner chooses to investigate it.

## Closure condition

The content owner selected retained evidence: commit the complete m18 package
and only m17 `RESULT.md`, without changing frozen m20 prose. The retained-link
scan is zero-broken, the shared tracked product-source selector protects the
evidence classes, and the focused fault-injection column is the completion
evidence for this task. The out-of-scope nextest performance observation does
not authorize a timeout/profile change.
