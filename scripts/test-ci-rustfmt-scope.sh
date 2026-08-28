#!/usr/bin/env bash
# Regression checks for the tracked product-source formatter boundary in ci.sh.
set -euo pipefail

readonly TEST_ROOT_DIR="${CI_GATE_TEST_ROOT_DIR:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)}"

# shellcheck source=ci.sh
source "$TEST_ROOT_DIR/scripts/ci.sh"

test_root="$(mktemp -d)"
trap 'rm -rf -- "$test_root"' EXIT
mkdir -p "$test_root/bin" "$test_root/tmp"
export TMPDIR="$test_root/tmp"
export TEST_RUSTFMT_ARGUMENTS="$test_root/rustfmt-arguments"
cat > "$test_root/bin/rustfmt" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$TEST_RUSTFMT_ARGUMENTS"
exit "${TEST_RUSTFMT_EXIT_STATUS:-0}"
EOF
chmod 755 "$test_root/bin/rustfmt"

(
  export PATH="$test_root/bin:$PATH"
  run_rustfmt
)

[[ -f "$TEST_RUSTFMT_ARGUMENTS" ]] || {
  printf '%s\n' 'rustfmt scope: argument record is missing' >&2
  exit 1
}
grep -Fxq -- '--edition' "$TEST_RUSTFMT_ARGUMENTS" || {
  printf '%s\n' 'rustfmt scope: --edition is absent' >&2
  exit 1
}
grep -Fxq -- '2024' "$TEST_RUSTFMT_ARGUMENTS" || {
  printf '%s\n' 'rustfmt scope: Rust 2024 is absent' >&2
  exit 1
}
grep -Fxq -- '--check' "$TEST_RUSTFMT_ARGUMENTS" || {
  printf '%s\n' 'rustfmt scope: --check is absent' >&2
  exit 1
}
grep -Fxq -- 'crates/reviewgraphen-core/src/lib.rs' "$TEST_RUSTFMT_ARGUMENTS" || {
  printf '%s\n' 'rustfmt scope: representative tracked product source is absent' >&2
  exit 1
}

# The stub itself must be able to fail: otherwise a lost rustfmt status would
# make this scope test pass.
if (
  export PATH="$test_root/bin:$PATH"
  export TEST_RUSTFMT_EXIT_STATUS=73
  run_rustfmt
); then
  printf '%s\n' 'run_rustfmt accepted an injected rustfmt failure' >&2
  exit 1
else
  rustfmt_status=$?
  if [[ "$rustfmt_status" -ne 73 ]]; then
    printf 'run_rustfmt propagated rustfmt status %s, expected 73\n' \
      "$rustfmt_status" >&2
    exit "$rustfmt_status"
  fi
fi

# G1: trackedness is isolated from root membership. This miniature repository
# has one tracked product source and one untracked Rust source in the same
# crates/ tree; the latter must not reach rustfmt.
tracked_only_repo="$test_root/tracked-only-repo"
mkdir -p "$tracked_only_repo/crates/product/src"
printf '%s\n' 'pub fn tracked_product() {}' \
  > "$tracked_only_repo/crates/product/src/lib.rs"
printf '%s\n' 'pub fn untracked_pollution(  ) {}' \
  > "$tracked_only_repo/crates/untracked-pollution.rs"
git -C "$tracked_only_repo" init --quiet
git -C "$tracked_only_repo" add crates/product/src/lib.rs
export TEST_RUSTFMT_ARGUMENTS="$test_root/tracked-only-rustfmt-arguments"
(
  cd "$tracked_only_repo"
  validate_product_workspace_layout() { :; }
  export PATH="$test_root/bin:$PATH"
  run_rustfmt
)
[[ -f "$TEST_RUSTFMT_ARGUMENTS" ]] || {
  printf '%s\n' 'G1: args file missing' >&2
  exit 1
}
grep -Fxq -- 'crates/product/src/lib.rs' "$TEST_RUSTFMT_ARGUMENTS" || {
  printf '%s\n' 'G1: tracked product file absent' >&2
  exit 1
}
if grep -Fxq -- 'crates/untracked-pollution.rs' "$TEST_RUSTFMT_ARGUMENTS"; then
  printf '%s\n' 'G1: untracked pollution included' >&2
  exit 1
else
  grep_status=$?
  if [[ "$grep_status" -ne 1 ]]; then
    printf 'tracked-only assertion failed: grep exit=%s\n' "$grep_status" >&2
    exit "$grep_status"
  fi
fi

# The fixture exclusion is separately isolated: both files are tracked and in
# crates/, so the only changed axis is the tests/fixtures evidence class.
fixture_repo="$test_root/fixture-repo"
mkdir -p "$fixture_repo/crates/product/src" \
  "$fixture_repo/crates/product/tests/fixtures"
printf '%s\n' 'pub fn tracked_product() {}' \
  > "$fixture_repo/crates/product/src/lib.rs"
printf '%s\n' 'pub fn external_fixture(  ) {}' \
  > "$fixture_repo/crates/product/tests/fixtures/external.rs"
git -C "$fixture_repo" init --quiet
git -C "$fixture_repo" add crates/product/src/lib.rs \
  crates/product/tests/fixtures/external.rs
export TEST_RUSTFMT_ARGUMENTS="$test_root/fixture-rustfmt-arguments"
(
  cd "$fixture_repo"
  validate_product_workspace_layout() { :; }
  export PATH="$test_root/bin:$PATH"
  run_rustfmt
)
[[ -f "$TEST_RUSTFMT_ARGUMENTS" ]] || {
  printf '%s\n' 'fixture scope: args file missing' >&2
  exit 1
}
grep -Fxq -- 'crates/product/src/lib.rs' "$TEST_RUSTFMT_ARGUMENTS" || {
  printf '%s\n' 'fixture scope: tracked product file absent' >&2
  exit 1
}
if grep -Fxq -- 'crates/product/tests/fixtures/external.rs' \
  "$TEST_RUSTFMT_ARGUMENTS"; then
  printf '%s\n' 'fixture scope: tracked external fixture evidence included' >&2
  exit 1
else
  grep_status=$?
  if [[ "$grep_status" -ne 1 ]]; then
    printf 'fixture-class assertion failed: grep exit=%s\n' "$grep_status" >&2
    exit "$grep_status"
  fi
fi

# The layout assertion rejects a product member outside crates/ while admitting
# the single documented external evidence fixture.
(
  cargo() {
    printf '%s\n' \
      '{"workspace_members":["product","evidence"],"packages":[{"id":"product","manifest_path":"'"$TEST_ROOT_DIR"'/crates/product/Cargo.toml"},{"id":"evidence","manifest_path":"'"$TEST_ROOT_DIR"'/examples/double-submit-payment/fixture/Cargo.toml"}]}'
  }
  validate_product_workspace_layout
)
if (
  cargo() {
    printf '%s\n' \
      '{"workspace_members":["product"],"packages":[{"id":"product","manifest_path":"'"$TEST_ROOT_DIR"'/outside-product/Cargo.toml"}]}'
  }
  validate_product_workspace_layout
); then
  printf '%s\n' 'workspace layout accepted a product member outside crates/' >&2
  exit 1
else
  layout_status=$?
  if [[ "$layout_status" -ne 1 ]]; then
    printf 'workspace layout failure status %s, expected 1\n' "$layout_status" >&2
    exit "$layout_status"
  fi
fi
if (
  cargo() { return 77; }
  validate_product_workspace_layout
); then
  printf '%s\n' 'workspace layout accepted an injected cargo metadata failure' >&2
  exit 1
else
  metadata_status=$?
  if [[ "$metadata_status" -ne 77 ]]; then
    printf 'workspace metadata failure status %s, expected 77\n' \
      "$metadata_status" >&2
    exit "$metadata_status"
  fi
fi

# Coverage has no product source exactly when the shared selector has none.
(
  load_product_rust_sources() {
    local -n result="$1"
    result=()
  }
  require_cargo_subcommand() { :; }
  run_coverage "$test_root/coverage-empty/lcov.info" \
    > "$test_root/coverage-empty-output"
)
grep -Fx -- 'coverage: skipped (no product Rust sources)' \
  "$test_root/coverage-empty-output" >/dev/null

# A nonempty shared selection starts the existing workspace coverage command.
(
  load_product_rust_sources() {
    local -n result="$1"
    result=('crates/reviewgraphen-core/src/lib.rs')
  }
  require_cargo_subcommand() { :; }
  cargo() {
    printf '%s\n' "$@" > "$test_root/coverage-cargo-arguments"
    local output_path=""
    while [[ "$#" -gt 0 ]]; do
      if [[ "$1" == '--output-path' ]]; then
        output_path="$2"
        break
      fi
      shift
    done
    if [[ "${TEST_CARGO_EXIT_STATUS:-0}" -eq 0 ]]; then
      mkdir -p -- "$(dirname -- "$output_path")"
      printf '%s\n' 'injected coverage report' > "$output_path"
    fi
    return "${TEST_CARGO_EXIT_STATUS:-0}"
  }
  run_coverage "$test_root/coverage-nonempty/lcov.info"
)
grep -Fx -- 'llvm-cov' "$test_root/coverage-cargo-arguments" >/dev/null

# Each replacement used above has a checked failure path. The selector and
# cargo-subcommand stubs are made to fail independently; cargo then fails after
# a nonempty selection. All writes remain under test_root.
if (
  require_cargo_subcommand() { :; }
  load_product_rust_sources() { return 74; }
  run_coverage "$test_root/coverage-load-failure/lcov.info"
); then
  printf '%s\n' 'run_coverage accepted an injected selector failure' >&2
  exit 1
else
  load_status=$?
  [[ "$load_status" -eq 74 ]] || exit "$load_status"
fi

if (
  require_cargo_subcommand() { return 75; }
  run_coverage "$test_root/coverage-require-failure/lcov.info"
); then
  printf '%s\n' 'run_coverage accepted an injected cargo-subcommand failure' >&2
  exit 1
else
  require_status=$?
  [[ "$require_status" -eq 75 ]] || exit "$require_status"
fi

if (
  load_product_rust_sources() {
    local -n result="$1"
    result=('crates/reviewgraphen-core/src/lib.rs')
  }
  require_cargo_subcommand() { :; }
  cargo() { return 76; }
  run_coverage "$test_root/coverage-cargo-failure/lcov.info"
); then
  printf '%s\n' 'run_coverage accepted an injected cargo failure' >&2
  exit 1
else
  cargo_status=$?
  [[ "$cargo_status" -eq 76 ]] || exit "$cargo_status"
fi
