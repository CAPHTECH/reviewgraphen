#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <prepared-trial-dir> <fresh-result-dir>" >&2
  exit 64
fi
: "${OLLAMA_PRIV_API_KEY:?OLLAMA_PRIV_API_KEY must be set}"
: "${M7_V2_SHAPER_LOG:?M7_V2_SHAPER_LOG must be set}"

trial_dir=$(realpath -e -- "$1")
result_dir=$2
case "$trial_dir" in
  /tmp/m7-local-factorial-v2-prepared-b1/snapshot-*/b1/replicate-4|\
  /tmp/m7-local-factorial-v2-prepared-full/snapshot-*/full_review_graphen/replicate-4) ;;
  *) echo "refusing input outside frozen v2 preparation: $trial_dir" >&2; exit 64 ;;
esac
case "$result_dir" in
  /tmp/m7-local-factorial-v2-runs/*) ;;
  *) echo "refusing result outside frozen v2 run root: $result_dir" >&2; exit 64 ;;
esac
if [[ -e "$result_dir" ]]; then
  echo "result directory must be fresh: $result_dir" >&2
  exit 64
fi
if ! curl -fsS http://127.0.0.1:12080/healthz >/dev/null; then
  echo "request shaper is not ready" >&2
  exit 69
fi
mkdir -p -- "$result_dir"

root=/home/rizumita/workspace/reviewgraphen
profile_source="$root/benchmarks/m7-local-factorial-v2/profile/ollama-priv-v2.config.toml"
profile_hash=e31b7a2a4040ccb52f4ecf3c5a828d78e926bba932ee4ca2c06d0294a1bb2be9
credential_home=$(mktemp -d /tmp/m7-local-factorial-v2-codex-home.XXXXXX)
cleanup() { rm -rf -- "$credential_home"; }
trap cleanup EXIT INT TERM
chmod 700 "$credential_home"
cp -- "$profile_source" "$credential_home/ollama-priv-v2.config.toml"
observed_profile_hash=$(sha256sum "$credential_home/ollama-priv-v2.config.toml" | awk '{print $1}')
if [[ "$observed_profile_hash" != "$profile_hash" ]]; then
  echo "profile hash drift" >&2
  exit 65
fi

benchmark="$root/target/debug/reviewgraphen-benchmark"
bwrap=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
codex=/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex
before_requests=$(wc -l < "$M7_V2_SHAPER_LOG")
started_epoch=$(date +%s)
set +e
"$benchmark" run-process-reviewer-codex-profile \
  ollama-priv-v2 OLLAMA_PRIV_API_KEY "$trial_dir" agent_input/candidate-output.schema.json \
  "$result_dir/process-output" "$result_dir/process-record.json" \
  "$bwrap" "$credential_home" "$codex" qwen3.8:27b-mlx high \
  >"$result_dir/adapter.stdout" 2>"$result_dir/adapter.stderr"
process_status=$?
set -e
finished_epoch=$(date +%s)
printf '%s\n' "$process_status" > "$result_dir/process-status"
printf '%s\n' "$((finished_epoch-started_epoch))" > "$result_dir/elapsed-seconds"
after_requests=$(wc -l < "$M7_V2_SHAPER_LOG")
if (( after_requests != before_requests + 1 )); then
  echo "expected exactly one shaped Responses request" >&2
  exit 70
fi
sed -n "${after_requests}p" "$M7_V2_SHAPER_LOG" > "$result_dir/transport-record.json"
jq -e '
  .model == "qwen3.8:27b-mlx" and
  .model_context_window == 262144 and
  .max_output_tokens == 65536 and
  .upstream_status == 200
' "$result_dir/transport-record.json" >/dev/null
if (( process_status != 0 )); then
  exit "$process_status"
fi
jq -e '
  .backend.provider == "ollama-priv-v2" and
  .backend.model == "qwen3.8:27b-mlx" and
  .backend.inference_settings.model_context_window == "262144" and
  .backend.inference_settings.max_output_tokens == "65536"
' "$result_dir/process-record.json" >/dev/null

jq -j '.raw_response' "$result_dir/process-record.json" > "$result_dir/candidate.json"
set +e
"$benchmark" validate candidate "$result_dir/candidate.json" \
  >"$result_dir/candidate-validation.stdout" 2>"$result_dir/candidate-validation.stderr"
candidate_status=$?
set -e
printf '%s\n' "$candidate_status" > "$result_dir/candidate-status"
if (( candidate_status != 0 )); then
  exit "$candidate_status"
fi
set +e
"$benchmark" collect "$trial_dir/manifest.json" "$result_dir/candidate.json" "$result_dir/collection.json" \
  >"$result_dir/collection.stdout" 2>"$result_dir/collection.stderr"
collection_status=$?
set -e
printf '%s\n' "$collection_status" > "$result_dir/collection-status"
exit "$collection_status"
