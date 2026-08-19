#!/usr/bin/env bash
# Copies trial artifacts out of /tmp into the repository after every trial,
# never as a final batch step. This program has already lost three units'
# candidates to a /tmp clear.
#
# The agent's full working copy is NOT committed (it is a whole repository
# checkout); its diff, its patched file, and the pre/post manifests are,
# which is everything needed to reconstruct what it did.
set -uo pipefail

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
dest="$root/benchmarks/m9-agentic-local-v1/runs"
mkdir -p "$dest"

for src in /tmp/m9-runs/*; do
  [[ -d "$src" ]] || continue
  trial=$(basename "$src")
  out="$dest/$trial"
  mkdir -p "$out"
  for file in loop-outcome claude-status elapsed-seconds \
              pre-loop-manifest.txt post-loop-manifest.txt \
              verification.json loop-behaviour.json loop-summary.txt \
              applied.diff patched-rust.rs claude.stderr verify.log; do
    [[ -f "$src/$file" ]] && cp -u "$src/$file" "$out/$file"
  done
  # The transcript is the irreplaceable artifact and is large; store it gzipped.
  if [[ -f "$src/stream.jsonl" && ! -f "$out/stream.jsonl.gz" ]]; then
    gzip -9 -c "$src/stream.jsonl" > "$out/stream.jsonl.gz"
    sha256sum "$out/stream.jsonl.gz" | awk '{print $1}' > "$out/stream.jsonl.gz.sha256"
  fi
done

find "$dest" -type f | wc -l
