#!/usr/bin/env bash
# Six agentic trials, strictly alternating skill/noskill, one at a time.
#
# Sequential only: never two requests in flight. Any upstream failure stops
# the series; no trial is retried and no trial is replaced.
set -uo pipefail

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
exp="$root/benchmarks/m9-agentic-local-v1"
runs=/tmp/m9-runs
log=/tmp/m9-series.log
mkdir -p "$runs"

note() { printf '%s %s\n' "$(date -Is)" "$*" | tee -a "$log"; }

note "SERIES START"
for index in 1 2 3; do
  for arm in skill noskill; do
    trial="${arm}-${index}"
    rm -rf "${runs:?}/$trial"
    note "START $trial"
    bash "$exp/scripts/run_trial.sh" "$arm" "$trial" "$runs/$trial" >> "$log" 2>&1
    outcome=$(cat "$runs/$trial/loop-outcome" 2>/dev/null || echo unknown)
    note "LOOP $trial outcome=$outcome elapsed=$(cat "$runs/$trial/elapsed-seconds" 2>/dev/null)s"

    python3 "$exp/scripts/analyse_loop.py" "$runs/$trial" > "$runs/$trial/loop-summary.txt" 2>&1
    python3 "$exp/scripts/verify_trial.py" "$runs/$trial" > "$runs/$trial/verify.log" 2>&1
    verdict=$(python3 -c "import json;print(json.load(open('$runs/$trial/verification.json'))['verdict'])" 2>/dev/null || echo unknown)
    note "VERIFY $trial verdict=$verdict"

    # Any model other than the frozen one is a hard stop.
    models=$(python3 -c "import json;print(','.join(json.load(open('$runs/$trial/loop-behaviour.json'))['model_ids_seen']))" 2>/dev/null || echo "")
    if [[ -n "$models" && "$models" != "qwen3.8:27b-mlx" ]]; then
      note "STOP $trial unexpected model ids: $models"
      exit 66
    fi
    bash "$exp/scripts/archive_trials.sh" >/dev/null 2>&1 || true
  done
done
note "SERIES COMPLETE: 6 trials"
