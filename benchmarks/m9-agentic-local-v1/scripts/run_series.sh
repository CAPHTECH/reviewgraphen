#!/usr/bin/env bash
# Six agentic trials, strictly alternating skill/noskill, one at a time.
#
# Sequential only: never two requests in flight. Any upstream failure stops
# the series; no trial is retried and no trial is replaced.
set -uo pipefail

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
exp="$root/benchmarks/m9-agentic-local-v1"
runs=/tmp/m9v2-runs
log=/tmp/m9v2-series.log
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

    # The echoed model id is RECORDED, never gated on: this backend reflects
    # whatever id the request sent, so that field passed through an entire
    # backend swap. The gate is the /v1/models identity hash, checked by
    # run_trial.sh before every trial.
    models=$(python3 -c "import json;print(','.join(json.load(open('$runs/$trial/loop-behaviour.json'))['model_ids_seen']))" 2>/dev/null || echo "")
    note "ECHOED-MODEL $trial $models (recorded, not a gate)"
    gate=$(python3 -c "import json;print(json.load(open('$runs/$trial/backend-identity.json'))['gate'])" 2>/dev/null || echo missing)
    if [[ "$gate" != "passed" ]]; then
      note "STOP $trial backend identity gate=$gate"
      exit 66
    fi
    bash "$exp/scripts/archive_trials.sh" >/dev/null 2>&1 || true
  done
done
note "SERIES COMPLETE: 6 trials"
