#!/usr/bin/env bash
# Six trials, projection/control alternating, one request at a time.
set -uo pipefail

root=/home/rizumita/workspace/reviewgraphen
exp="$root/benchmarks/m10-target-context-local-v1"
runs=/tmp/m10-runs
log=/tmp/m10-series.log
mkdir -p "$runs"
note() { printf '%s %s\n' "$(date -Is)" "$*" | tee -a "$log"; }

note "SERIES START"
for index in 1 2 3; do
  for arm in projection control; do
    trial="${arm}-${index}"
    if [[ -f "$runs/$trial/verification.json" ]]; then
      note "SKIP $trial already complete"
      continue
    fi
    rm -rf "${runs:?}/$trial"
    note "START $trial"
    bash "$exp/scripts/run_trial.sh" "$arm" "$trial" "$runs/$trial" >> "$log" 2>&1
    trial_status=$?
    if (( trial_status == 66 )); then
      note "STOP $trial refused before generation"
      printf 'NOT A TRIAL. A pre-flight identity gate refused before generation.\n' \
        > "$runs/$trial/NOT-A-TRIAL.md"
      exit 66
    fi
    python3 "$exp/scripts/analyse_loop.py" "$runs/$trial" > "$runs/$trial/loop-summary.txt" 2>&1
    python3 "$exp/scripts/check_projection_use.py" "$runs/$trial"
    python3 "$exp/scripts/verify_trial.py" "$runs/$trial" > "$runs/$trial/verify.log" 2>&1
    python3 "$exp/scripts/output_profile.py" "$runs/$trial" >/dev/null 2>&1 || true
    verdict=$(python3 -c "import json;print(json.load(open('$runs/$trial/verification.json'))['verdict'])" 2>/dev/null || echo unknown)
    used=$(python3 -c "import json;print(json.load(open('$runs/$trial/projection-use.json'))['used'])" 2>/dev/null || echo unknown)
    note "VERIFY $trial verdict=$verdict projection_used=$used"

    gate=$(python3 -c "import json;print(json.load(open('$runs/$trial/backend-identity.json'))['gate'])" 2>/dev/null || echo missing)
    if [[ "$gate" != "passed" ]]; then
      note "STOP $trial backend identity gate=$gate"
      exit 66
    fi
    python3 "$exp/scripts/check_cache_credit.py" "$runs/$trial" > "$runs/$trial/cache-credit.log" 2>&1
    cache_verdict=$(python3 -c "import json;print(json.load(open('$runs/$trial/cache-credit.json'))['verdict'])" 2>/dev/null || echo unknown)
    note "CACHE $trial verdict=$cache_verdict"
    if [[ "$cache_verdict" != "cache_credited" ]]; then
      note "STOP $trial cache not credited"
      exit 67
    fi
    python3 "$exp/scripts/detect_truncated_tail.py" "$runs/$trial" > "$runs/$trial/truncated-tail.log" 2>&1
    tail_suspect=$(python3 -c "import json;print(json.load(open('$runs/$trial/truncated-tail.json'))['suspected'])" 2>/dev/null || echo unknown)
    note "TAIL $trial suspected=$tail_suspect"
    if [[ "$tail_suspect" == "True" ]]; then
      note "STOP $trial truncated-tail signature detected"
      exit 68
    fi
    if [[ "$arm" == "projection" && "$used" != "True" ]]; then
      note "STOP $trial projection intervention was not used"
      exit 69
    fi
    bash "$exp/scripts/archive_trials.sh" >/dev/null 2>&1 || true
  done
done
note "SERIES COMPLETE: 6 trials"
