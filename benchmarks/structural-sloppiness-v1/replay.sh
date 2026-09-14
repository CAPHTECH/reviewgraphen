#!/usr/bin/env bash
# Replays the pinned historical calibration from four operator-captured inputs.
# No target code runs; the supplied analyzer is the only executable under test.
set -euo pipefail

if [[ $# != 6 ]]; then
  echo 'usage: bash replay.sh ANALYZER OLD_ZERO OLD_POSITIVE FIXED CURRENT FRESH_OUTPUT_DIR' >&2
  exit 64
fi
slop_analyzer="$1"
slop_inputs=("$2" "$3" "$4" "$5")
slop_labels=(old-not-exercised old-positive fixed current)
mkdir -- "$6"
slop_output="$(cd -- "$6" && pwd -P)"

project_report() {
  local label="$1" input="$2" report="$3" input_hash report_hash
  input_hash="$(sha256sum -- "$input" | cut -d ' ' -f 1)"
  report_hash="$(sha256sum -- "$report" | cut -d ' ' -f 1)"
  jq --arg label "$label" --arg input_hash "$input_hash" --arg report_hash "$report_hash" '
    {
      format: "structural-sloppiness-trial-projection@1",
      case: $label,
      raw_input_sha256: $input_hash,
      source_report: {sha256: $report_hash, retained_in_repository: false},
      input, contract, authority,
      scope: {
        status: .scope.status,
        eligible_count: .scope.eligible_count,
        excluded_count: .scope.excluded_count,
        excluded_by_reason: .scope.excluded_by_reason,
        capabilities: [.scope.capabilities[] | {capability, state, source_id_count: (.source_ids | length)}],
        limitation_count: (.scope.limitation_ids | length)
      },
      observed_facts, candidate_claims,
      obstructions: [.obstructions[] | {id, kind, statement, source_id_count: (.source_ids | length), limitation_count: (.limitation_ids | length)}],
      projection_loss: [
        "Not an AnalysisReport; validate the full source report instead.",
        "Capability and obstruction source-ID lists and limitation-ID lists are omitted; their counts and the full report hash remain.",
        "Unrelated ProgramSpace facts, source code, and limitation details are not reproduced."
      ]
    }' "$report" > "$slop_output/$label.projection.json"
}

for i in "${!slop_labels[@]}"; do
  label="${slop_labels[$i]}"
  report="$slop_output/$label.report.json"
  "$slop_analyzer" analyze --input "${slop_inputs[$i]}" --output "$report"
  "$slop_analyzer" validate --input "${slop_inputs[$i]}" --report "$report"
  project_report "$label" "${slop_inputs[$i]}" "$report"
done

jq -e '.scope.status == "not_exercised" and .scope.eligible_count == 0 and (.candidate_claims | length) == 0' "$slop_output/old-not-exercised.report.json"
jq -e '.scope.eligible_count == 3 and (.candidate_claims | length) == 3' "$slop_output/old-positive.report.json"
for label in fixed current; do
  jq -e '.scope.eligible_count == 3 and (.candidate_claims | length) == 0' "$slop_output/$label.report.json"
done
jq -n -e --slurpfile old "$slop_output/old-positive.report.json" --slurpfile fixed "$slop_output/fixed.report.json" --slurpfile current "$slop_output/current.report.json" '
  ($old[0].observed_facts | map(.symbol_id)) == ($fixed[0].observed_facts | map(.symbol_id)) and
  ($old[0].observed_facts | map(.symbol_id)) == ($current[0].observed_facts | map(.symbol_id))'

# Synthetic endpoint-only mutation of real input; preserve the ProgramSpace
# permutation invariant by changing target_ids AND ordered_target_ids together.
symbol="$(jq -r '.observed_facts[0].symbol_id' "$slop_output/current.report.json")"
other="$(jq -r '.observed_facts[1].symbol_id' "$slop_output/current.report.json")"
bridge="$(jq -er '.observed_facts[0].matching_contains_relation_ids | if length == 1 then .[0] else error("expected exactly one bridge") end' "$slop_output/current.report.json")"
jq -c --arg bridge "$bridge" --arg symbol "$symbol" --arg other "$other" '
  .relations |= map(if .id == $bridge then
    .target_ids |= map(if . == $symbol then $other else . end) |
    if has("ordered_target_ids") then
      .ordered_target_ids |= map(if . == $symbol then $other else . end)
    else . end
  else . end)' "${slop_inputs[3]}" > "$slop_output/current-rewired.program.json"
mutated="$slop_output/current-rewired.program.json"
report="$slop_output/current-rewired.report.json"
"$slop_analyzer" analyze --input "$mutated" --output "$report"
"$slop_analyzer" validate --input "$mutated" --report "$report"
jq -e '.scope.eligible_count == 3 and (.candidate_claims | length) == 1' "$report"
project_report current-rewired "$mutated" "$report"

"$slop_analyzer" flat --input "${slop_inputs[3]}" --output "$slop_output/current.flat.json"
"$slop_analyzer" flat --input "$mutated" --output "$slop_output/current-rewired.flat.json"
cmp -- "$slop_output/current.flat.json" "$slop_output/current-rewired.flat.json"
sha256sum -- "$slop_output/current.flat.json" "$slop_output/current-rewired.flat.json"

set +e
"$slop_analyzer" validate --input "$mutated" --report "$slop_output/current.report.json"
stale_status=$?
set -e
[[ "$stale_status" == 4 ]]

jq -s '{format: "structural-sloppiness-trial-summary@1", cases: .}' \
  "$slop_output/old-not-exercised.projection.json" \
  "$slop_output/old-positive.projection.json" \
  "$slop_output/fixed.projection.json" \
  "$slop_output/current.projection.json" \
  "$slop_output/current-rewired.projection.json" > "$slop_output/summary.json"
jq '[.cases[] | {case, status: .scope.status, eligible: .scope.eligible_count, candidates: (.candidate_claims | length)}]' "$slop_output/summary.json"
