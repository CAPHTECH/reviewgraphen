#!/usr/bin/env bash
set -euo pipefail

if [[ $# -eq 1 && "$1" == "--batch-generic-v2-r2" ]]; then
  prepared=/tmp/m7-real-generic-v2-prepared-r2
  runs=/tmp/m7-real-generic-v2-run-r2
  records=/tmp/m7-real-generic-v2-records-r2
  credentials=/tmp/reviewgraphen-generic-v2-codex-home
  codex=/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex
  mkdir -p -- "$runs" "$records"
  for input in "$prepared"/snapshot-*/full_review_graphen/replicate-2; do
    snapshot=$(basename "$(dirname "$(dirname "$input")")")
    record="$records/$snapshot.json"
    candidate="$runs/$snapshot-candidate.json"
    collection="$runs/$snapshot-collection.json"
    if [[ -f "$record" && -f "$candidate" && -f "$collection" ]]; then
      printf 'SKIP %s\n' "$snapshot"
      continue
    fi
    output="$runs/$snapshot-output"
    "$0" codex "$input" "$output" "$record" "$credentials" "$codex" gpt-5.6-sol high
    jq -r '.raw_response' "$record" > "$candidate"
    /home/rizumita/workspace/reviewgraphen/target/debug/reviewgraphen-benchmark \
      validate candidate "$candidate" >/dev/null
    /home/rizumita/workspace/reviewgraphen/target/debug/reviewgraphen-benchmark \
      collect "$input/manifest.json" "$candidate" "$collection" >/dev/null
    printf 'DONE %s\n' "$snapshot"
  done
  exit 0
fi

if [[ $# -ne 8 ]]; then
  echo "usage: $0 <codex|claude> <input-root> <output-root> <record-output> <credential-home> <backend-executable> <model> <effort>" >&2
  exit 2
fi

exec /home/rizumita/workspace/reviewgraphen/target/debug/reviewgraphen-benchmark \
  run-process-reviewer "$1" "$2" agent_input/candidate-output.schema.json \
  "$3" "$4" \
  /home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap \
  "$5" "$6" "$7" "$8"
