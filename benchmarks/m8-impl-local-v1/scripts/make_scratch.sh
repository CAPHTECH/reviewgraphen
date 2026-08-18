#!/usr/bin/env bash
# Materializes an isolated, tracked-files-only copy of the pinned revision
# into a fresh scratch directory, and installs the harness-owned acceptance
# test. No candidate ever writes into the real worktree; every candidate
# edit is applied to a copy produced by this script.
#
# usage: make_scratch.sh <fresh-scratch-dir>
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <fresh-scratch-dir>" >&2
  exit 64
fi

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
scratch=$1
case "$scratch" in
  /tmp/*) ;;
  *) echo "refusing scratch outside /tmp: $scratch" >&2; exit 64 ;;
esac
if [[ -e "$scratch" ]]; then
  echo "scratch directory must be fresh: $scratch" >&2
  exit 64
fi

mkdir -p "$scratch"
git -C "$root" archive "$(cat "$root/benchmarks/m8-impl-local-v1/task/PINNED_REVISION")" | tar -x -C "$scratch"
mkdir -p "$scratch/crates/reviewgraphen-cli/tests"
cp "$root/benchmarks/m8-impl-local-v1/task/review_flag_order.rs" \
   "$scratch/crates/reviewgraphen-cli/tests/review_flag_order.rs"
echo "$scratch"
