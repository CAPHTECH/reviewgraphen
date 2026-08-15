#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: run_judge_calibration.sh <prepared-root> <fresh-runs-root> <benchmark-binary>" >&2
  exit 2
fi
prepared=$1
runs=$2
benchmark_binary=$3
bwrap=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
codex=/home/rizumita/.local/share/mise/installs/codex/latest/bin/codex
claude=/home/rizumita/.local/share/mise/installs/claude/latest/claude
if [[ ! "$prepared" = /* || ! "$runs" = /* || ! "$benchmark_binary" = /* ]]; then
  echo "all paths must be absolute" >&2
  exit 2
fi
if [[ -e "$runs" ]]; then
  echo "runs root must be fresh; implicit retry is forbidden" >&2
  exit 2
fi
credential_root=$(mktemp -d /tmp/m7-head-issue-credentials.XXXXXX)
cleanup() { rm -rf "$credential_root"; }
trap cleanup EXIT
mkdir -p "$credential_root/codex" "$credential_root/claude" "$runs"
cp /home/rizumita/.codex/auth.json "$credential_root/codex/auth.json"
for name in installation_id models_cache.json; do
  if [[ -f "/home/rizumita/.codex/$name" ]]; then cp "/home/rizumita/.codex/$name" "$credential_root/codex/$name"; fi
done
cp /home/rizumita/.claude/.credentials.json "$credential_root/claude/.credentials.json"
mapfile -t batches < <(jq -r '.batches[].batch_id' "$prepared/calibration-inventory.json")
for judge in codex claude; do
  for batch in "${batches[@]}"; do
    run_dir="$runs/$judge/$batch"
    mkdir -p "$run_dir"
    if [[ "$judge" == codex ]]; then
      backend=$codex; model=gpt-5.6-sol; effort=high; credentials="$credential_root/codex"
    else
      backend=$claude; model=opus; effort=high; credentials="$credential_root/claude"
    fi
    "$benchmark_binary" run-process-reviewer-constrained "$judge" "$prepared/batches/$batch/input" output.schema.json "$run_dir/output" "$run_dir/record.json" "$bwrap" "$credentials" "$backend" "$model" "$effort"
  done
done
