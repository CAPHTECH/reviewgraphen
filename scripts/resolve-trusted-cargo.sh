#!/usr/bin/env bash
# Resolves the mise-admitted, host-installed `cargo` executable for the
# rust@1.95.0 toolchain pinned in mise.toml / rust-toolchain.toml, without
# ever installing or downloading anything.
#
# This is an external harness, entirely outside reviewgraphen-ingest's own
# trust boundary: production `ingest()` never reads this script, never
# spawns mise/rustup/asdf, and never searches PATH for `cargo` on its own
# (see docs/20_m2_ingestion_contract.md's "Cargo tool admission" section and
# docs/adr/0012-mise-host-admitted-cargo-toolchain.md). Only the single
# absolute path this script prints on success is meant to be handed to a
# caller that builds `CargoToolAdmission::TrustedExecutable` from it.
#
# `mise which cargo` is deliberately never used here: it resolves to a
# rustup-style proxy shim that re-dispatches based on whichever toolchain is
# active at call time, not the one fixed rust@1.95.0 `cargo` binary this
# script must name and verify. `rustc --print sysroot`'s `bin/cargo` is the
# real, toolchain-specific executable mise itself installed.
set -euo pipefail

readonly RUST_TOOLCHAIN="rust@1.95.0"

# Belt-and-suspenders: safe to run standalone, without relying on this
# process having inherited mise.toml's project-level [settings].
export MISE_AUTO_INSTALL=false
export MISE_EXEC_AUTO_INSTALL=false
export MISE_TASK_RUN_AUTO_INSTALL=false
export MISE_NOT_FOUND_AUTO_INSTALL=false
export MISE_OFFLINE=true
export RUSTUP_AUTO_INSTALL=0

fail() {
  printf 'resolve-trusted-cargo: %s\n' "$1" >&2
  exit 1
}

# Fully resolves symlinks in $1 without relying on GNU `readlink -f` or a
# guaranteed `realpath` (not present on every macOS/Linux install), mirroring
# what `admit_cargo_executable` requires of an admitted path via
# `fs::canonicalize` in crates/reviewgraphen-ingest/src/git.rs.
canonicalize_file() {
  local target="$1"
  local dir base link hops=0
  while :; do
    hops=$((hops + 1))
    if ((hops > 40)); then
      return 1
    fi
    dir="$(cd -P -- "$(dirname -- "$target")" 2>/dev/null && pwd -P)" || return 1
    base="$(basename -- "$target")"
    target="$dir/$base"
    if [[ -L "$target" ]]; then
      link="$(readlink -- "$target")" || return 1
      case "$link" in
        /*) target="$link" ;;
        *) target="$dir/$link" ;;
      esac
      continue
    fi
    break
  done
  printf '%s\n' "$target"
}

sysroot="$(mise exec "$RUST_TOOLCHAIN" -- rustc --print sysroot)" || fail \
  "mise could not run rustc for $RUST_TOOLCHAIN; run \`mise install\` (see DEVELOPMENT.md) first -- this script never installs anything itself"

[[ -n "$sysroot" ]] || fail "rustc --print sysroot returned an empty sysroot for $RUST_TOOLCHAIN"

candidate="$sysroot/bin/cargo"
[[ "$candidate" == /* ]] || fail "resolved cargo path is not absolute: $candidate"

canonical="$(canonicalize_file "$candidate")" || fail \
  "could not canonicalize resolved cargo path: $candidate"
[[ -e "$canonical" ]] || fail "resolved cargo path does not exist: $canonical"
[[ -f "$canonical" ]] || fail "resolved cargo path is not a regular file: $canonical"
[[ -x "$canonical" ]] || fail "resolved cargo path is not executable: $canonical"
[[ "$(basename -- "$canonical")" == "cargo" ]] || fail \
  "resolved executable's basename is not cargo: $canonical"

printf '%s\n' "$canonical"
