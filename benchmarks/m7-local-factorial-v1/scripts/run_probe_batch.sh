#!/usr/bin/env bash
set -u

if [[ $# -ne 2 || ! "$2" =~ ^[0-9]+$ ]]; then
  echo "usage: $0 <stage_1|stage_2_if_expanded> <zero-based-start-index>" >&2
  exit 64
fi
: "${OLLAMA_PRIV_API_KEY:?OLLAMA_PRIV_API_KEY must be set}"
stage=$1
start=$2
if [[ "$stage" != stage_1 && "$stage" != stage_2_if_expanded ]]; then
  echo "invalid frozen probe stage" >&2
  exit 64
fi

root=/home/rizumita/workspace/reviewgraphen
plan="$root/benchmarks/m7-local-factorial-v1/schema-probe/plan.json"
runner="$root/benchmarks/m7-local-factorial-v1/scripts/run_profile_trial.sh"
result_stage=${stage/stage_/stage-}
result_stage=${result_stage/_if_expanded/}
index=0
valid=0
invalid=0
while IFS=$'\t' read -r snapshot arm input; do
  if (( index < start )); then
    ((index += 1))
    continue
  fi
  result="/tmp/m7-local-factorial-runs/$result_stage/$snapshot/$arm"
  printf 'START index=%d snapshot=%s arm=%s valid=%d invalid=%d\n' "$index" "$snapshot" "$arm" "$valid" "$invalid"
  if bash "$runner" "$input" "$result"; then
    ((valid += 1))
    outcome=valid
  else
    ((invalid += 1))
    outcome=protocol_or_process_invalid
  fi
  printf 'DONE index=%d snapshot=%s arm=%s outcome=%s valid=%d invalid=%d\n' "$index" "$snapshot" "$arm" "$outcome" "$valid" "$invalid"
  ((index += 1))
done < <(jq -r --arg stage "$stage" '.[$stage].trials[] | [.snapshot_id,.arm,.input_root] | @tsv' "$plan")
printf 'BATCH_DONE stage=%s completed=%d valid=%d invalid=%d\n' "$stage" "$((index-start))" "$valid" "$invalid"
exit 0
