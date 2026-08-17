#!/usr/bin/env bash
set -euo pipefail

# Sequential qwen_skill generation batch: 9 units in unit_index order, per
# QWEN_SKILL_ARM_AMENDMENT.md. Otherwise identical to run_batch_full_only.sh
# (same shaper, profile, execution condition, stop conditions). Does not
# stop on candidate_schema_invalid/empty_final/etc: those are recorded and
# the batch continues, per the operator's instruction that a reasoning-
# budget failure is itself a measured result, not a reason to stop early.

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <attempt-label>" >&2
  exit 64
fi
: "${OLLAMA_PRIV_API_KEY:?OLLAMA_PRIV_API_KEY must be set}"
: "${M7_HEAD_LOCAL_REASONING_EFFORT:?M7_HEAD_LOCAL_REASONING_EFFORT must be set}"
attempt=$1

root=/home/rizumita/workspace/reviewgraphen
units_json="$root/benchmarks/m7-head-local-v1/units.json"
runner="$root/benchmarks/m7-head-local-v1/scripts/run_trial.sh"
prepared_root=/tmp/m7-head-local-v1-skill-prepared
run_root=/tmp/m7-head-local-v1-runs
batch_root="$run_root/$attempt"
mkdir -p -- "$batch_root"

if ! curl -fsS --connect-timeout 3 --max-time 5 \
  http://192.168.68.71:11999/api/version > "$batch_root/version.json" ||
   ! curl -fsS --connect-timeout 3 --max-time 5 \
  http://192.168.68.71:11999/api/ps > "$batch_root/ps.json"; then
  echo "upstream server health preflight failed" >&2
  exit 71
fi

shaper_log="$batch_root/request-shaper.jsonl"
capture_dir="$batch_root/request-shaper-responses"
if [[ -e "$shaper_log" ]]; then
  echo "request shaper log must be fresh: $shaper_log" >&2
  exit 64
fi
python3 "$root/benchmarks/m7-local-factorial-v3/scripts/request_shaper.py" --log "$shaper_log" --capture-dir "$capture_dir" \
  >"$batch_root/request-shaper.stdout" \
  2>"$batch_root/request-shaper.stderr" &
shaper_pid=$!
cleanup() {
  kill "$shaper_pid" 2>/dev/null || true
  wait "$shaper_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM
ready=0
for _ in {1..50}; do
  if curl -fsS http://127.0.0.1:12080/healthz >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 0.2
done
if (( ready == 0 )); then
  echo "request shaper failed to start" >&2
  exit 69
fi
export M7_HEAD_LOCAL_SHAPER_LOG="$shaper_log"
export M7_HEAD_LOCAL_SHAPER_CAPTURE_DIR="$capture_dir"

count=$(jq '.units | length' "$units_json")
trial_index=0
for ((i = 0; i < count; i++)); do
  unit_id=$(jq -r ".units[$i].unit_id" "$units_json")
  trial_dir="$prepared_root/$unit_id"
  result="$batch_root/$unit_id/skill"
  export M7_HEAD_LOCAL_TRIAL_KEY="$attempt-$trial_index"
  printf 'START attempt=%s unit=%s arm=skill trial_index=%d\n' "$attempt" "$unit_id" "$trial_index"
  set +e
  bash "$runner" "$trial_dir" "$result"
  trial_status=$?
  set -e
  outcome=unknown
  if [[ -f "$result/generation-metrics.json" ]]; then
    outcome=$(jq -r '.failure_class' "$result/generation-metrics.json")
  elif (( trial_status == 70 )); then
    outcome=request_count_mismatch
  fi
  printf 'DONE attempt=%s unit=%s arm=skill trial_index=%d outcome=%s trial_status=%d\n' \
    "$attempt" "$unit_id" "$trial_index" "$outcome" "$trial_status"
  ((trial_index += 1))
  if (( trial_status == 70 )); then
    printf 'BATCH_STOP reason=request_count_mismatch unit=%s\n' "$unit_id"
    exit 70
  fi
  if (( trial_status == 71 )); then
    printf 'BATCH_STOP reason=upstream_server_failure unit=%s outcome=%s\n' "$unit_id" "$outcome"
    exit 71
  fi
  printf 'UNIT_DONE attempt=%s unit=%s completed_index=%d total_units=%d\n' "$attempt" "$unit_id" "$((i + 1))" "$count"
done
printf 'BATCH_DONE attempt=%s\n' "$attempt"
