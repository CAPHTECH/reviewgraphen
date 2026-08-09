#!/usr/bin/env bash
# The only supported local and CI entry point for repository verification.
set -euo pipefail

readonly ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly MODE="${1:-fast}"

if [[ "$#" -gt 1 ]]; then
  printf 'usage: %s [fast|coverage|deny|heavy|scheduled]\n' "$0" >&2
  exit 64
fi

cd "$ROOT_DIR"

require_cargo_subcommand() {
  local subcommand="$1"
  local install_hint="$2"

  if ! cargo "$subcommand" --version >/dev/null 2>&1; then
    printf 'missing cargo-%s; %s\n' "$subcommand" "$install_hint" >&2
    exit 127
  fi
}

admit_ingest_test_cargo() {
  local trusted_cargo="${REVIEWGRAPHEN_TRUSTED_CARGO:-}"

  # `reviewgraphen-ingest` must never discover Cargo itself. The fast gate is
  # its external test harness, so CI supplies an already-admitted absolute
  # path and local runs use the explicit mise harness only when one was not
  # supplied. The mise task has auto-install disabled and fails closed if the
  # pinned toolchain was not installed explicitly beforehand.
  if [[ -z "$trusted_cargo" ]]; then
    if ! command -v mise >/dev/null 2>&1; then
      printf '%s\n' \
        'missing REVIEWGRAPHEN_TRUSTED_CARGO; export a host-admitted absolute Cargo path, or run `mise install` once and retry (see DEVELOPMENT.md)' >&2
      exit 127
    fi
    trusted_cargo="$(mise run trusted-cargo-path)"
  fi

  if [[ "$trusted_cargo" == *$'\r'* || "$trusted_cargo" == *$'\n'* || "$trusted_cargo" != /* || ! -f "$trusted_cargo" || ! -x "$trusted_cargo" || "$(basename -- "$trusted_cargo")" != "cargo" ]]; then
    printf 'invalid REVIEWGRAPHEN_TRUSTED_CARGO: %s\n' "$trusted_cargo" >&2
    exit 64
  fi

  export REVIEWGRAPHEN_TRUSTED_CARGO="$trusted_cargo"
}

check_reviewed_test_artifacts() {
  local changes

  if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    return
  fi

  changes="$(git status --porcelain -- \
    ':(glob)**/proptest-regressions/**/*.txt' \
    ':(glob)**/snapshots/**/*.snap' \
    ':(glob)**/snapshots/**/*.snap.new')"
  if [[ -n "$changes" ]]; then
    printf '%s\n' 'uncommitted proptest regression seed(s) or insta snapshot(s) detected:' >&2
    printf '%s\n' "$changes" >&2
    printf '%s\n' 'Review and commit these executable test artifacts before considering the run clean.' >&2
    exit 1
  fi
}

run_rustfmt() {
  local -a product_sources=()
  local source_path

  # The fixture is source evidence from the design bundle and must not be
  # rewritten by the development harness. Future implementation sources use the
  # workspace's 2024 edition and are formatted here.
  while IFS= read -r source_path; do
    product_sources+=("$source_path")
  done < <(
    find . -type f -name '*.rs' \
      ! -path './examples/double-submit-payment/fixture/*' \
      ! -path './target/*' \
      -print | sort
  )
  if [[ "${#product_sources[@]}" -eq 0 ]]; then
    printf '%s\n' 'rustfmt: no product Rust sources to check'
    return
  fi

  rustfmt --edition 2024 --check "${product_sources[@]}"
}

run_fast() {
  python3 scripts/validate_bundle.py
  bash scripts/test-ci-admission.sh
  admit_ingest_test_cargo
  run_rustfmt
  cargo clippy --workspace --all-targets --all-features -- \
    -D warnings \
    -D clippy::dbg_macro \
    -D clippy::todo \
    -D clippy::unimplemented
  require_cargo_subcommand \
    nextest \
    'install with: cargo install cargo-nextest --locked --version 0.9.137'
  cargo nextest run --workspace --all-features --profile ci
  cargo test --workspace --all-features --doc
  check_reviewed_test_artifacts
}

run_coverage() {
  require_cargo_subcommand \
    llvm-cov \
    'install with: cargo install cargo-llvm-cov --locked --version 0.8.6'
  if ! find . -type f -name '*.rs' \
    ! -path './examples/double-submit-payment/fixture/*' \
    ! -path './target/*' \
    -print -quit | grep -q .; then
    printf '%s\n' 'coverage: skipped (no product Rust sources)'
    return
  fi
  mkdir -p target/llvm-cov
  cargo llvm-cov --workspace --all-targets --all-features \
    --lcov --output-path target/llvm-cov/lcov.info
  if [[ ! -s target/llvm-cov/lcov.info ]]; then
    printf '%s\n' 'coverage report is empty' >&2
    exit 1
  fi
}

run_deny() {
  require_cargo_subcommand \
    deny \
    'install with: cargo install cargo-deny --locked --version 0.18.7'
  cargo deny check --config deny.toml
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  case "$MODE" in
    fast)
      run_fast
      ;;
    coverage)
      run_coverage
      ;;
    deny)
      run_deny
      ;;
    heavy)
      run_deny
      run_coverage
      ;;
    scheduled)
      run_fast
      run_deny
      run_coverage
      ;;
    *)
      printf 'usage: %s [fast|coverage|deny|heavy|scheduled]\n' "$0" >&2
      exit 64
      ;;
  esac
fi
