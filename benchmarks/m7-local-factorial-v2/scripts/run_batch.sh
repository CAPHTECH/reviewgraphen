#!/usr/bin/env bash
set -u

if [[ $# -ne 2 || ! "$2" =~ ^[0-9]+$ ]]; then
  echo "usage: $0 <stage_1|stage_2_if_expanded|positive> <zero-based-start-index>" >&2
  exit 64
fi
: "${OLLAMA_PRIV_API_KEY:?OLLAMA_PRIV_API_KEY must be set}"
mode=$1
start=$2
if [[ "$mode" != stage_1 && "$mode" != stage_2_if_expanded && "$mode" != positive ]]; then
  echo "invalid v2 batch mode" >&2
  exit 64
fi

root=/home/rizumita/workspace/reviewgraphen
plan="$root/benchmarks/m7-local-factorial-v2/plan.private.json"
runner="$root/benchmarks/m7-local-factorial-v2/scripts/run_trial.sh"
run_root=/tmp/m7-local-factorial-v2-runs
mkdir -p -- "$run_root"
shaper_log="$run_root/request-shaper-${mode}-${start}.jsonl"
if [[ -e "$shaper_log" ]]; then
  echo "request shaper log must be fresh: $shaper_log" >&2
  exit 64
fi
python3 "$root/benchmarks/m7-local-factorial-v2/scripts/request_shaper.py" --log "$shaper_log" \
  >"$run_root/request-shaper-${mode}-${start}.stdout" \
  2>"$run_root/request-shaper-${mode}-${start}.stderr" &
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
export M7_V2_SHAPER_LOG="$shaper_log"

index=0
completed=0
valid=0
invalid=0
consecutive_invalid=0
while IFS=$'\t' read -r snapshot arm input; do
  if (( index < start )); then
    ((index += 1))
    continue
  fi
  result="$run_root/$mode/$snapshot/$arm"
  printf 'START index=%d snapshot=%s arm=%s completed=%d valid=%d protocol_invalid=%d\n' \
    "$index" "$snapshot" "$arm" "$completed" "$valid" "$invalid"
  bash "$runner" "$input" "$result"
  trial_status=$?
  if (( trial_status == 0 )); then
    ((valid += 1))
    consecutive_invalid=0
    outcome=valid
  else
    ((invalid += 1))
    ((consecutive_invalid += 1))
    outcome=protocol_or_process_invalid
  fi
  ((completed += 1))
  printf 'DONE index=%d snapshot=%s arm=%s outcome=%s completed=%d valid=%d protocol_invalid=%d\n' \
    "$index" "$snapshot" "$arm" "$outcome" "$completed" "$valid" "$invalid"
  ((index += 1))
  if (( trial_status == 70 )); then
    printf 'BATCH_STOP reason=request_count_mismatch completed=%d valid=%d protocol_invalid=%d\n' \
      "$completed" "$valid" "$invalid"
    exit 70
  fi
  if (( consecutive_invalid >= 3 )); then
    printf 'BATCH_STOP reason=three_consecutive_invalid completed=%d valid=%d protocol_invalid=%d\n' \
      "$completed" "$valid" "$invalid"
    exit 75
  fi
done < <(jq -r --arg mode "$mode" '.[$mode].trials[] | [.snapshot_id,.arm,.input_root] | @tsv' "$plan")
printf 'BATCH_DONE mode=%s completed=%d valid=%d protocol_invalid=%d\n' \
  "$mode" "$completed" "$valid" "$invalid"
