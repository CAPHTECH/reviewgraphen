#!/usr/bin/env bash
set -euo pipefail

: "${OLLAMA_PRIV_API_KEY:?OLLAMA_PRIV_API_KEY must be set}"
root=/home/rizumita/workspace/reviewgraphen
attempt=reasoning-none-probe-replacement-1
batch_root=/tmp/m7-local-factorial-v2-runs/$attempt
input=/tmp/m7-local-factorial-v2-prepared-b1/snapshot-06/b1/replicate-4
result="$batch_root/snapshot-06/b1_free_form"
if [[ -e "$batch_root" || ! -d "$input" ]]; then
  echo "replacement output must be fresh and frozen input must exist" >&2
  exit 64
fi
mkdir -p -- "$batch_root/server-health"
curl -fsS --connect-timeout 3 --max-time 5 \
  http://192.168.68.71:11999/api/version > "$batch_root/server-health/version.json"
curl -fsS --connect-timeout 3 --max-time 5 \
  http://192.168.68.71:11999/api/ps > "$batch_root/server-health/ps.json"
shaper_log="$batch_root/request-shaper.jsonl"
capture_dir="$batch_root/provider-responses"
python3 "$root/benchmarks/m7-local-factorial-v2/scripts/request_shaper.py" \
  --log "$shaper_log" --capture-dir "$capture_dir" \
  >"$batch_root/request-shaper.stdout" 2>"$batch_root/request-shaper.stderr" &
shaper_pid=$!
cleanup() {
  kill "$shaper_pid" 2>/dev/null || true
  wait "$shaper_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM
for _ in {1..50}; do
  if curl -fsS http://127.0.0.1:12080/healthz >/dev/null 2>&1; then
    export M7_V2_SHAPER_LOG="$shaper_log"
    export M7_V2_SHAPER_CAPTURE_DIR="$capture_dir"
    printf 'START probe=reasoning-none-replacement-1 snapshot=snapshot-06 arm=b1_free_form completed=0 valid=0 protocol_invalid=0\n'
    set +e
    bash "$root/benchmarks/m7-local-factorial-v2/scripts/run_trial.sh" "$input" "$result" none
    status=$?
    set -e
    printf 'DONE probe=reasoning-none-replacement-1 status=%d completed=1\n' "$status"
    exit "$status"
  fi
  sleep 0.2
done
echo "request shaper failed to start" >&2
exit 69
