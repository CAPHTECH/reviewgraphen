#!/usr/bin/env bash
# Copies run artifacts out of /tmp into the repository, per the operational
# change m7-head-local-v1/diagnostics/final-judge-pool/RECOVERY.md adopted
# after that experiment lost its /tmp working tree: settle each result into
# version control as soon as it exists, never batch at the end.
set -euo pipefail

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
dest="$root/benchmarks/m8-impl-local-v1/runs"
mkdir -p "$dest"

for run in methodology baseline task2-methodology; do
  src=/tmp/m8-impl-local-v1-runs/$run
  [[ -d "$src" ]] || continue
  mkdir -p "$dest/$run"
  for file in generation-metrics.json request-body.json final-content.txt \
              candidate.json verification.json applied.diff; do
    [[ -f "$src/$file" ]] && cp "$src/$file" "$dest/$run/$file"
  done
  if [[ -f "$src/provider-response.sse" ]]; then
    gzip -9 -c "$src/provider-response.sse" > "$dest/$run/provider-response.sse.gz"
    sha256sum "$dest/$run/provider-response.sse.gz" | awk '{print $1}' \
      > "$dest/$run/provider-response.sse.gz.sha256"
  fi
done

# Packet manifests: what was actually sent, hash-pinned.
for task in "" "-t2"; do
  manifest=/tmp/m8-impl-local-v1${task}-packets/packet-manifest.json
  [[ -f "$manifest" ]] || manifest=/tmp/m8${task}-packets/packet-manifest.json
  [[ -f "$manifest" ]] && cp "$manifest" "$dest/packet-manifest${task:-"-task1"}.json"
done

# Judge pool and result, including the withheld truth mapping.
if [[ -d /tmp/m8-judge/task1 ]]; then
  mkdir -p "$dest/judge-task1/pool"
  cp /tmp/m8-judge/task1/*.md "$dest/judge-task1/pool/"
  cp /tmp/m8-judge/task1-truth.json "$dest/judge-task1/truth.json"
fi
if [[ -d /tmp/m8-judge-results/task1 ]]; then
  mkdir -p "$dest/judge-task1"
  cp /tmp/m8-judge-results/task1/judgment.json "$dest/judge-task1/judgment.json"
  cp /tmp/m8-judge-results/task1/judge-output.txt "$dest/judge-task1/judge-output.txt"
  cp /tmp/m8-judge-results/task1/prompt.sha256 "$dest/judge-task1/prompt.sha256"
  cp /tmp/m8-judge-results/task1/judge-elapsed-seconds "$dest/judge-task1/judge-elapsed-seconds"
fi

find "$dest" -type f | sort
