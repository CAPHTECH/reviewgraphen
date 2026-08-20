#!/usr/bin/env bash
set -uo pipefail

root=/home/rizumita/workspace/reviewgraphen
dest="$root/benchmarks/m10-target-context-local-v1/runs"
mkdir -p "$dest"

for src in /tmp/m10-runs/*; do
  [[ -d "$src" ]] || continue
  trial=$(basename "$src")
  out="$dest/$trial"
  mkdir -p "$out"
  for file in loop-outcome claude-status elapsed-seconds \
              pre-loop-manifest.txt post-loop-manifest.txt \
              verification.json loop-behaviour.json loop-summary.txt \
              applied.diff patched-rust.rs pinned-rust.rs claude.stderr verify.log \
              backend-identity.json trial.pgid cache-credit.json \
              truncated-tail.json projection-use.json output-profile.json; do
    [[ -f "$src/$file" ]] && cp -u "$src/$file" "$out/$file"
  done
  if [[ -f "$src/stream.jsonl" && ! -f "$out/stream.jsonl.gz" ]]; then
    gzip -9 -c "$src/stream.jsonl" > "$out/stream.jsonl.gz"
    sha256sum "$out/stream.jsonl.gz" | awk '{print $1}' > "$out/stream.jsonl.gz.sha256"
  fi
done
