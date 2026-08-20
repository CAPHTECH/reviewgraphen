#!/usr/bin/env bash
# One fresh, tool-free, no-session-persistence judge call per distinct change.
set -euo pipefail

root=/home/rizumita/workspace/reviewgraphen
exp="$root/benchmarks/m10-target-context-local-v1"
packets=/tmp/m10-judge-packets
results=/tmp/m10-judge-results
scanner="$root/benchmarks/m8-impl-local-v1/scripts/scan_forbidden_markers.py"
extract="$root/benchmarks/m8-impl-local-v1/scripts/extract_candidate_json.py"
claude=$(readlink -f /home/rizumita/.local/share/mise/installs/claude/latest/claude)

[[ ! -e "$results" ]] || { echo "result directory must be fresh: $results" >&2; exit 64; }
mkdir -p "$results"

for pool in "$packets"/*; do
  change_id=$(basename "$pool")
  python3 "$scanner" "$pool" >/dev/null
  out="$results/$change_id"
  mkdir "$out"
  cat "$pool/00-instructions.md" "$pool/01-specification.md" "$pool/02-change.md" > "$out/prompt.txt"
  sha256sum "$out/prompt.txt" > "$out/prompt.sha256"
  started=$(date +%s)
  set +e
  "$claude" --print --no-session-persistence --tools "" --model opus \
    < "$out/prompt.txt" > "$out/judge-output.txt" 2> "$out/judge-stderr.txt"
  status=$?
  set -e
  finished=$(date +%s)
  printf '%s\n' "$status" > "$out/judge-status"
  printf '%s\n' "$((finished - started))" > "$out/judge-elapsed-seconds"
  [[ "$status" == 0 ]] || { echo "judge failed: $change_id status=$status" >&2; exit "$status"; }
  python3 "$extract" "$out/judge-output.txt" > "$out/judgment.json"
  jq -e --arg id "$change_id" \
    '.schema == "reviewgraphen.benchmark.m8_code_quality_judgment.v1" and (.judgments|length)==1 and .judgments[0].change_id==$id' \
    "$out/judgment.json" >/dev/null
  echo "judged $change_id elapsed=$((finished - started))s"
done
