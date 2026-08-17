#!/usr/bin/env bash
set -euo pipefail

# Mirrors m7-local-factorial-v3/scripts/run_trial.sh exactly for the
# generation execution condition (same profile, same shaper, same
# reasoning_effort binding, same ADR 0037 extraction contract, same
# candidate schema, same failure_class taxonomy). Differs only in: (a) the
# admitted input path pattern, since m7-head-local-v1 has its own freshly
# built packets rather than the frozen v2 corpus, and (b) it stops after
# candidate schema validation — there is no `collect` step, because there
# is no oracle/manifest for this experiment (see preregistration.json
# "scoring").

if [[ ( $# -ne 2 && $# -ne 3 ) || ( $# -eq 3 && "$3" != high && "$3" != none ) ]]; then
  echo "usage: $0 <prepared-trial-dir> <fresh-result-dir> [high|none]" >&2
  exit 64
fi
: "${OLLAMA_PRIV_API_KEY:?OLLAMA_PRIV_API_KEY must be set}"
: "${M7_HEAD_LOCAL_SHAPER_LOG:?M7_HEAD_LOCAL_SHAPER_LOG must be set}"
: "${M7_HEAD_LOCAL_SHAPER_CAPTURE_DIR:?M7_HEAD_LOCAL_SHAPER_CAPTURE_DIR must be set}"
: "${M7_HEAD_LOCAL_TRIAL_KEY:?M7_HEAD_LOCAL_TRIAL_KEY must be set}"
if [[ ! "$M7_HEAD_LOCAL_TRIAL_KEY" =~ ^[A-Za-z0-9._:-]{1,128}$ ]]; then
  echo "invalid trial routing key" >&2
  exit 64
fi

trial_dir=$(realpath -e -- "$1")
result_dir=$2
reasoning_effort=${M7_HEAD_LOCAL_REASONING_EFFORT:-${3:-high}}
case "$trial_dir" in
  /tmp/m7-head-local-v1-prepared/head-local-*/b1|\
  /tmp/m7-head-local-v1-prepared/head-local-*/full|\
  /tmp/m7-head-local-v1-skill-prepared/head-local-*) ;;
  *) echo "refusing input outside m7-head-local-v1 prepared packets: $trial_dir" >&2; exit 64 ;;
esac
case "$result_dir" in
  /tmp/m7-head-local-v1-runs/*) ;;
  *) echo "refusing result outside m7-head-local-v1 run root: $result_dir" >&2; exit 64 ;;
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
profile_source="$root/benchmarks/m7-local-factorial-v3/profile/lm-studio-cch-v3.config.toml"
profile_hash=0fe8cda539e2d74e17e98cbef5742791e0cfd0de812a753cfa1746c9b176d79e
credential_home=$(mktemp -d /tmp/m7-head-local-v1-codex-home.XXXXXX)
cleanup() { rm -rf -- "$credential_home"; }
trap cleanup EXIT INT TERM
chmod 700 "$credential_home"
observed_source_profile_hash=$(sha256sum "$profile_source" | awk '{print $1}')
if [[ "$observed_source_profile_hash" != "$profile_hash" ]]; then
  echo "profile hash drift" >&2
  exit 65
fi
sed "s/__M7_V3_TRIAL_KEY__/$M7_HEAD_LOCAL_TRIAL_KEY/" "$profile_source" \
  > "$credential_home/lm-studio-cch-v3.config.toml"
effective_profile_hash=$(sha256sum "$credential_home/lm-studio-cch-v3.config.toml" | awk '{print $1}')
jq -n \
  --arg trial_key "$M7_HEAD_LOCAL_TRIAL_KEY" \
  --arg source_profile_sha256 "sha256:$observed_source_profile_hash" \
  --arg effective_profile_sha256 "sha256:$effective_profile_hash" \
  '{
    schema: "reviewgraphen.benchmark.parallel_trial_routing.v1",
    trial_key: $trial_key,
    shaper_endpoint: "http://127.0.0.1:12080/v1/",
    source_profile_sha256: $source_profile_sha256,
    effective_profile_sha256: $effective_profile_sha256,
    routing_header_forwarded_upstream: false
  }' > "$result_dir/routing-record.json"

benchmark="$root/target/debug/reviewgraphen-benchmark"
bwrap=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
codex=/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex
started_nanoseconds=$(date +%s%N)
set +e
"$benchmark" run-process-reviewer-codex-profile \
  lm-studio-cch-v3 OLLAMA_PRIV_API_KEY "$trial_dir" agent_input/candidate-output.schema.json \
  "$result_dir/process-output" "$result_dir/process-record.json" \
  "$bwrap" "$credential_home" "$codex" qwen3.8:27b-mlx "$reasoning_effort" \
  >"$result_dir/adapter.stdout" 2>"$result_dir/adapter.stderr"
process_status=$?
set -e
finished_nanoseconds=$(date +%s%N)
elapsed_milliseconds=$(((finished_nanoseconds-started_nanoseconds)/1000000))
printf '%s\n' "$process_status" > "$result_dir/process-status"
printf '%s\n' "$elapsed_milliseconds" > "$result_dir/elapsed-milliseconds"
for _ in {1..50}; do
  request_count=$(jq -c --arg key "$M7_HEAD_LOCAL_TRIAL_KEY" 'select(.trial_key == $key)' \
    "$M7_HEAD_LOCAL_SHAPER_LOG" | wc -l)
  if (( request_count >= 1 )); then
    break
  fi
  sleep 0.1
done
request_count=$(jq -c --arg key "$M7_HEAD_LOCAL_TRIAL_KEY" 'select(.trial_key == $key)' \
  "$M7_HEAD_LOCAL_SHAPER_LOG" | wc -l)
if (( request_count < 1 )); then
  echo "expected at least one shaped Responses request" >&2
  exit 70
fi
jq -c --arg key "$M7_HEAD_LOCAL_TRIAL_KEY" 'select(.trial_key == $key)' \
  "$M7_HEAD_LOCAL_SHAPER_LOG" > "$result_dir/transport-record.jsonl"
tail -n1 "$result_dir/transport-record.jsonl" > "$result_dir/transport-record.json"
jq -e --arg reasoning_effort "$reasoning_effort" '
  .model == "qwen3.8:27b-mlx" and
  .model_context_window == 262144 and
  .max_output_tokens == 65536 and
  (.upstream_status | type == "number") and
  (.response_complete | type == "boolean") and
  (.raw_response_artifact | test("^response-[0-9]{6}\\.sse\\.gz$")) and
  (.raw_response_compressed_sha256 | test("^sha256:[0-9a-f]{64}$"))
' "$result_dir/transport-record.json" >/dev/null
raw_artifact=$(jq -r '.raw_response_artifact' "$result_dir/transport-record.json")
raw_source="$M7_HEAD_LOCAL_SHAPER_CAPTURE_DIR/$raw_artifact"
mkdir -p "$result_dir/provider-responses"
while IFS= read -r transport; do
  artifact=$(jq -r '.raw_response_artifact' <<<"$transport")
  source="$M7_HEAD_LOCAL_SHAPER_CAPTURE_DIR/$artifact"
  if [[ ! -f "$source" ]] || ! gzip -t -- "$source"; then
    echo "provider raw response artifact is missing or corrupt: $artifact" >&2
    exit 70
  fi
  expected_hash=$(jq -r '.raw_response_compressed_sha256 | sub("^sha256:"; "")' <<<"$transport")
  observed_hash=$(sha256sum "$source" | awk '{print $1}')
  if [[ "$observed_hash" != "$expected_hash" ]]; then
    echo "provider raw response artifact hash mismatch: $artifact" >&2
    exit 70
  fi
  cp -- "$source" "$result_dir/provider-responses/$artifact"
done < "$result_dir/transport-record.jsonl"
cp -- "$raw_source" "$result_dir/provider-response.sse.gz"
upstream_status=$(jq -r '.upstream_status' "$result_dir/transport-record.json")
admitted_input_bytes=$(find "$trial_dir" -type f -printf '%s\n' | awk '{total += $1} END {print total + 0}')
final_content_bytes=null
empty_final=false
if [[ -f "$result_dir/process-output/raw-response.json" ]]; then
  final_content_bytes=$(wc -c < "$result_dir/process-output/raw-response.json")
  if (( final_content_bytes == 0 )); then
    empty_final=true
  fi
elif (( process_status != 0 )) && rg -q 'raw response size 0 is outside' "$result_dir/adapter.stderr"; then
  final_content_bytes=0
  empty_final=true
fi
write_metrics() {
  local failure_class=$1
  jq -n \
    --slurpfile transport "$result_dir/transport-record.json" \
    --argjson admitted_input_bytes "$admitted_input_bytes" \
    --argjson elapsed_milliseconds "$elapsed_milliseconds" \
    --argjson request_count "$request_count" \
    --argjson final_content_bytes "$final_content_bytes" \
    --argjson empty_final "$empty_final" \
    --arg failure_class "$failure_class" \
    '{
      schema: "reviewgraphen.benchmark.local_generation_metrics.v1",
      admitted_input_bytes: $admitted_input_bytes,
      provider_input_tokens: $transport[0].provider_input_tokens,
      cached_input_tokens: $transport[0].cached_input_tokens,
      provider_output_tokens: $transport[0].provider_output_tokens,
      provider_reported_reasoning_tokens: $transport[0].provider_reported_reasoning_tokens,
      thinking_tokens: $transport[0].thinking_tokens,
      provider_non_reasoning_output_tokens: $transport[0].provider_non_reasoning_output_tokens,
      request_count: $request_count,
      final_content_tokens: $transport[0].final_content_tokens,
      token_attribution: $transport[0].token_attribution,
      reasoning_delta_events: $transport[0].reasoning_delta_events,
      reasoning_delta_utf8_bytes: $transport[0].reasoning_delta_utf8_bytes,
      output_text_delta_events: $transport[0].output_text_delta_events,
      output_text_delta_utf8_bytes: $transport[0].output_text_delta_utf8_bytes,
      final_content_bytes: $final_content_bytes,
      empty_final: $empty_final,
      elapsed_milliseconds: $elapsed_milliseconds,
      elapsed_seconds: ($elapsed_milliseconds / 1000),
      failure_class: $failure_class
    }' > "$result_dir/generation-metrics.json"
}
if (( upstream_status >= 500 )); then
  write_metrics upstream_server_failure
  exit 71
fi
if (( process_status != 0 )); then
  if rg -q 'stream (disconnected|closed) before (completion|response\.completed)' \
    "$result_dir/adapter.stderr"; then
    if rg -qi 'model has crashed' "$result_dir/adapter.stderr"; then
      write_metrics upstream_model_crash
    else
      write_metrics upstream_server_stream_incomplete
    fi
    exit 71
  elif [[ "$empty_final" == true ]] && jq -e '
    (.provider_input_tokens | type == "number") and
    (.provider_output_tokens | type == "number") and
    (.provider_input_tokens + .provider_output_tokens >= .model_context_window)
  ' "$result_dir/transport-record.json" >/dev/null; then
    write_metrics context_budget_exhausted_before_final
    exit 72
  elif [[ "$empty_final" == true ]]; then
    write_metrics empty_final_after_process_completion
  else
    write_metrics transport_or_process_invalid
  fi
  exit "$process_status"
fi
jq -e --arg reasoning_effort "$reasoning_effort" '
  .backend.provider == "lm-studio-cch-v3" and
  .backend.model == "qwen3.8:27b-mlx" and
  .backend.inference_settings.model_context_window == "262144" and
  .backend.inference_settings.max_output_tokens == "65536" and
  .backend.inference_settings.reasoning_effort == $reasoning_effort
' "$result_dir/process-record.json" >/dev/null

jq -j '.raw_response' "$result_dir/process-record.json" > "$result_dir/raw-response-unextracted.txt"
python3 "$root/benchmarks/m7-local-factorial-v3/scripts/extract_candidate_json.py" \
  "$result_dir/raw-response-unextracted.txt" \
  >"$result_dir/candidate.json" 2>"$result_dir/extraction.stderr"
set +e
"$benchmark" validate candidate "$result_dir/candidate.json" \
  >"$result_dir/candidate-validation.stdout" 2>"$result_dir/candidate-validation.stderr"
candidate_status=$?
set -e
printf '%s\n' "$candidate_status" > "$result_dir/candidate-status"
if (( candidate_status != 0 )); then
  write_metrics candidate_schema_invalid
  exit "$candidate_status"
fi
write_metrics valid
exit 0
