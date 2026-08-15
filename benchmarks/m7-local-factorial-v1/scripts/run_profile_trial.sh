#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <prepared-trial-dir> <fresh-result-dir>" >&2
  exit 64
fi
if [[ -z ${OLLAMA_PRIV_API_KEY-} ]]; then
  echo "OLLAMA_PRIV_API_KEY must be present in the parent environment" >&2
  exit 64
fi

trial_dir=$(realpath -e -- "$1")
result_dir=$2
case "$trial_dir" in
  /tmp/m7-local-factorial-prepared-b1g3/snapshot-*/b1/replicate-3|\
  /tmp/m7-local-factorial-prepared-b1g3/snapshot-*/g3_proxy/replicate-3|\
  /tmp/m7-local-factorial-prepared-full/snapshot-*/full_review_graphen/replicate-3) ;;
  *) echo "refusing input outside frozen local-factorial preparation: $trial_dir" >&2; exit 64 ;;
esac
case "$result_dir" in
  /tmp/m7-local-factorial-runs/*) ;;
  *) echo "refusing result outside /tmp/m7-local-factorial-runs: $result_dir" >&2; exit 64 ;;
esac
if [[ -e "$result_dir" ]]; then
  echo "result directory must be fresh: $result_dir" >&2
  exit 64
fi
mkdir -p -- "$result_dir"

credential_home=$(mktemp -d /tmp/m7-local-factorial-codex-home.XXXXXX)
cleanup() { rm -rf -- "$credential_home"; }
trap cleanup EXIT INT TERM
chmod 700 "$credential_home"
cp -- /home/rizumita/.codex/ollama-priv.config.toml "$credential_home/ollama-priv.config.toml"
observed_profile_hash=$(sha256sum "$credential_home/ollama-priv.config.toml" | awk '{print $1}')
if [[ "$observed_profile_hash" != b37ae2f0f9403f7b9664e9b7d73408572822a9ee8ce814714032d75f4e3201db ]]; then
  echo "profile hash drift" >&2
  exit 65
fi

benchmark=/home/rizumita/workspace/reviewgraphen/target/debug/reviewgraphen-benchmark
bwrap=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
codex=/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex
set +e
"$benchmark" run-process-reviewer-codex-profile \
  ollama-priv OLLAMA_PRIV_API_KEY "$trial_dir" agent_input/candidate-output.schema.json \
  "$result_dir/process-output" "$result_dir/process-record.json" \
  "$bwrap" "$credential_home" "$codex" qwen3.8:27b-mlx high \
  >"$result_dir/adapter.stdout" 2>"$result_dir/adapter.stderr"
process_status=$?
set -e
printf '%s\n' "$process_status" > "$result_dir/process-status"
if (( process_status != 0 )); then
  exit "$process_status"
fi

jq -r '.raw_response' "$result_dir/process-record.json" > "$result_dir/candidate.json"
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
