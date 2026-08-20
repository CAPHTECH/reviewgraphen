#!/usr/bin/env bash
# Materializes one isolated implementation trial.
# usage: make_scratch.sh <fresh-scratch-dir> [--with-projection]
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 <fresh-scratch-dir> [--with-projection]" >&2
  exit 64
fi

root=/home/rizumita/workspace/reviewgraphen
m8="$root/benchmarks/m8-impl-local-v1"
exp="$root/benchmarks/m10-target-context-local-v1"
scratch=$1
with_projection=${2:-}

case "$scratch" in
  /tmp/*) ;;
  *) echo "refusing scratch outside /tmp: $scratch" >&2; exit 64 ;;
esac
if [[ -e "$scratch" ]]; then
  echo "scratch directory must be fresh: $scratch" >&2
  exit 64
fi

revision=$(cat "$m8/task/PINNED_REVISION")
mkdir -p "$scratch"
git -C "$root" archive "$revision" | tar -x -C "$scratch"
rm -rf "$scratch/benchmarks"
if [[ -e "$scratch/benchmarks" ]]; then
  echo "failed to remove benchmarks/ from the working copy" >&2
  exit 65
fi
if rg -l "m8_extern_block_shadow" "$scratch" >/dev/null 2>&1; then
  echo "acceptance test leaked into the working copy" >&2
  exit 65
fi

if [[ "$with_projection" == "--with-projection" ]]; then
  mkdir -p "$scratch/.reviewgraphen/bin"
  cp "$exp/projection/visit_block.json" "$scratch/.reviewgraphen/visit_block.json"
  cp "$exp/scripts/reviewgraphen-context" "$scratch/.reviewgraphen/bin/reviewgraphen-context"
  chmod 755 "$scratch/.reviewgraphen/bin/reviewgraphen-context"
fi

echo "$scratch"
