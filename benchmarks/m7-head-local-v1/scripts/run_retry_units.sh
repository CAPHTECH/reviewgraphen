#!/usr/bin/env bash
set -euo pipefail

# Re-runs a specific, non-contiguous set of units under the unchanged
# qwen_skill execution condition, for units whose prior attempt was
# excluded (an upstream_* failure class, zero semantic attempts consumed
# per RESTART_PROTOCOL_AND_UPSTREAM_HISTORY.md section 4). Bounded at 2
# consecutive same-unit attempts; a unit failing identically twice is
# marked upstream_blocked and excluded, never a 3rd request.
#
# Otherwise mirrors run_batch_skill.sh's shaper setup exactly (same
# profile, same shaper, same execution condition) but iterates over an
# explicit unit-id list instead of a contiguous index range.

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <attempt-label> <unit_id> [<unit_id> ...]" >&2
  exit 64
fi
: "${OLLAMA_PRIV_API_KEY:?OLLAMA_PRIV_API_KEY must be set}"
: "${M7_HEAD_LOCAL_REASONING_EFFORT:?M7_HEAD_LOCAL_REASONING_EFFORT must be set}"
attempt=$1
shift
unit_ids=("$@")

root=/home/rizumita/workspace/reviewgraphen
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
python3 "$root/benchmarks/m7-head-local-v1/scripts/request_shaper.py" --log "$shaper_log" --capture-dir "$capture_dir" \
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

is_excluded_upstream_class() {
  case "$1" in
    upstream_server_failure|upstream_server_stream_incomplete|upstream_model_crash| \
    upstream_stream_closed_before_completion|upstream_silent_truncation) return 0 ;;
    *) return 1 ;;
  esac
}

trial_index=0
for unit_id in "${unit_ids[@]}"; do
  trial_dir="$prepared_root/$unit_id"
  attempt_number=1
  while :; do
    result="$batch_root/${unit_id}-attempt${attempt_number}/skill"
    export M7_HEAD_LOCAL_TRIAL_KEY="$attempt-$unit_id-a$attempt_number"
    printf 'RETRY_START attempt=%s unit=%s attempt_number=%d\n' "$attempt" "$unit_id" "$attempt_number"
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
    printf 'RETRY_DONE attempt=%s unit=%s attempt_number=%d outcome=%s trial_status=%d\n' \
      "$attempt" "$unit_id" "$attempt_number" "$outcome" "$trial_status"
    ((trial_index += 1))
    if (( trial_status == 70 )); then
      printf 'RETRY_ABORT reason=request_count_mismatch unit=%s\n' "$unit_id"
      exit 70
    fi
    if is_excluded_upstream_class "$outcome"; then
      if (( attempt_number >= 2 )); then
        printf 'UPSTREAM_BLOCKED unit=%s after %d consecutive %s\n' "$unit_id" "$attempt_number" "$outcome"
        break
      fi
      ((attempt_number += 1))
      continue
    fi
    printf 'UNIT_SETTLED unit=%s final_outcome=%s attempts_used=%d\n' "$unit_id" "$outcome" "$attempt_number"
    break
  done
done
printf 'RETRY_BATCH_DONE attempt=%s\n' "$attempt"
