#!/usr/bin/env bash
set -euo pipefail
if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: make_scratch.sh <fresh-dir> [--with-reviewgraphen]" >&2
  exit 64
fi
out=$1
mode=${2:-}
[[ ! -e "$out" ]]
source_root=/home/rizumita/github/fsl
mkdir -p "$out/rust/fsl-core/src/bin"
cp "$source_root/rust/fsl-core/src/bin/fsl-parse-kernel.rs" "$out/rust/fsl-core/src/bin/"
cp "$source_root/rust/fsl-core/src/compose.rs" "$out/rust/fsl-core/src/"
cp "$source_root/rust/fsl-core/src/db.rs" "$out/rust/fsl-core/src/"
cp "$source_root/rust/fsl-core/src/diagnostics.rs" "$out/rust/fsl-core/src/"
if [[ "$mode" == "--with-reviewgraphen" ]]; then
  mkdir -p "$out/.reviewgraphen/bin"
  cp /home/rizumita/workspace/reviewgraphen/benchmarks/m11-review-agentic-local-v1/scripts/reviewgraphen-context "$out/.reviewgraphen/bin/"
  chmod 755 "$out/.reviewgraphen/bin/reviewgraphen-context"
elif [[ -n "$mode" ]]; then
  echo "unknown mode: $mode" >&2
  exit 64
fi

