#!/usr/bin/env bash
# Copies replication trial artifacts out of /tmp into the repository.
#
# Run after EVERY completed trial, never as a final batch step. The failure
# this guards against has already happened once in this program: a reboot
# cleared /tmp mid-experiment and three units' candidates were unrecoverable
# because only a manifest of paths had been committed, not the content
# (m7-head-local-v1 ARTIFACT_LOSS_AMENDMENT.md, and the RECOVERY.md
# operational change that followed it).
#
# Idempotent: safe to run repeatedly while the series is still going.
set -uo pipefail

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
dest="$root/benchmarks/m8-impl-local-v1/runs/replication"
mkdir -p "$dest"

for src in /tmp/m8-impl-local-v1-runs/rep-*; do
  [[ -d "$src" ]] || continue
  trial=$(basename "$src")
  out="$dest/$trial"
  mkdir -p "$out"
  for file in generation-metrics.json request-body.json final-content.txt \
              candidate.json verification.json verification-in-series.json \
              applied.diff patched-lib.rs advertised-models.json \
              m8_foreign_macro_probe.result m8_foreign_safefn_probe.result \
              m8_foreign_macro_probe.log m8_foreign_safefn_probe.log; do
    [[ -f "$src/$file" ]] && cp -u "$src/$file" "$out/$file"
  done
  # The raw SSE stream is the only irreplaceable provenance artifact.
  if [[ -f "$src/provider-response.sse" && ! -f "$out/provider-response.sse.gz" ]]; then
    gzip -9 -c "$src/provider-response.sse" > "$out/provider-response.sse.gz"
    sha256sum "$out/provider-response.sse.gz" | awk '{print $1}' \
      > "$out/provider-response.sse.gz.sha256"
  fi
done

find "$dest" -type f | wc -l
