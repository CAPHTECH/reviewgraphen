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

validate_product_workspace_layout() {
  local manifest_paths member_manifest member_root metadata_file status

  if ! command -v cargo >/dev/null 2>&1; then
    printf '%s\n' 'missing cargo; cannot validate product workspace-member roots' >&2
    return 127
  fi

  metadata_file="$(mktemp "${TMPDIR:-/tmp}/reviewgraphen-workspace-metadata.XXXXXX")"
  if cargo metadata --locked --no-deps --format-version 1 > "$metadata_file"; then
    :
  else
    status=$?
    rm -f -- "$metadata_file"
    printf '%s\n' 'cargo metadata failed; cannot validate product workspace-member roots' >&2
    return "$status"
  fi

  manifest_paths="$(python3 -c '
import json
import sys

metadata = json.load(sys.stdin)
workspace_members = set(metadata["workspace_members"])
for package in metadata["packages"]:
    if package["id"] in workspace_members:
        print(package["manifest_path"])
' < "$metadata_file")" || {
    status=$?
    rm -f -- "$metadata_file"
    printf '%s\n' 'cargo metadata failed; cannot validate product workspace-member roots' >&2
    return "$status"
  }
  if ! rm -f -- "$metadata_file"; then
    printf '%s\n' 'cannot remove temporary workspace metadata' >&2
    return 1
  fi

  while IFS= read -r member_manifest; do
    [[ -z "$member_manifest" ]] && continue
    if [[ "$member_manifest" != "$ROOT_DIR/"*/Cargo.toml ]]; then
      printf 'workspace member manifest is outside this checkout: %s\n' \
        "$member_manifest" >&2
      return 1
    fi

    member_root="${member_manifest#"$ROOT_DIR"/}"
    member_root="${member_root%/Cargo.toml}"
    case "$member_root" in
      crates/*)
        ;;
      examples/double-submit-payment/fixture)
        # This sole non-product member is executable counterexample evidence.
        ;;
      *)
        printf '%s\n' \
          "workspace member is outside crates/: $member_root (only examples/double-submit-payment/fixture is external evidence)" >&2
        return 1
        ;;
    esac
  done <<< "$manifest_paths"
}

load_product_rust_sources() {
  local result_name="$1"
  local -n result="$result_name"
  local source_list source_path status

  result=()
  if validate_product_workspace_layout; then
    :
  else
    status=$?
    return "$status"
  fi

  if ! command -v git >/dev/null 2>&1; then
    printf '%s\n' 'missing git; cannot select tracked product Rust sources' >&2
    return 127
  fi

  # Only tracked source under product crate roots is gate input. External source
  # fixtures preserve upstream evidence bytes, so the entire fixtures class is
  # excluded rather than maintaining a list of individual failing files.
  source_list="$(mktemp "${TMPDIR:-/tmp}/reviewgraphen-product-rust-sources.XXXXXX")"
  if git ls-files -z -- \
    ':(glob)crates/**/*.rs' \
    ':(exclude,glob)crates/**/tests/fixtures/**' > "$source_list"; then
    :
  else
    status=$?
    rm -f -- "$source_list"
    printf '%s\n' 'git ls-files failed; cannot select tracked product Rust sources' >&2
    return "$status"
  fi

  while IFS= read -r -d '' source_path; do
    result+=("$source_path")
  done < "$source_list"
  if ! rm -f -- "$source_list"; then
    printf '%s\n' 'cannot remove temporary product Rust source list' >&2
    return 1
  fi
}

run_rustfmt() {
  local -a product_sources=()
  local status

  if load_product_rust_sources product_sources; then
    :
  else
    status=$?
    return "$status"
  fi
  if [[ "${#product_sources[@]}" -eq 0 ]]; then
    printf '%s\n' 'rustfmt: no product Rust sources to check'
    return
  fi

  rustfmt --edition 2024 --check "${product_sources[@]}"
}

run_fast() {
  python3 scripts/validate_bundle.py
  bash scripts/test-ci-admission.sh
  bash scripts/test-ci-rustfmt-scope.sh
  sh scripts/test-install.sh
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
  local -a product_sources=()
  local coverage_output_path="${1:-target/llvm-cov/lcov.info}"
  local coverage_output_dir status

  if require_cargo_subcommand \
    llvm-cov \
    'install with: cargo install cargo-llvm-cov --locked --version 0.8.6'; then
    :
  else
    status=$?
    return "$status"
  fi
  if load_product_rust_sources product_sources; then
    :
  else
    status=$?
    return "$status"
  fi
  if [[ "${#product_sources[@]}" -eq 0 ]]; then
    printf '%s\n' 'coverage: skipped (no product Rust sources)'
    return
  fi
  coverage_output_dir="$(dirname -- "$coverage_output_path")"
  mkdir -p -- "$coverage_output_dir"
  if cargo llvm-cov --workspace --all-targets --all-features \
    --lcov --output-path "$coverage_output_path"; then
    :
  else
    status=$?
    return "$status"
  fi
  if [[ ! -s "$coverage_output_path" ]]; then
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
