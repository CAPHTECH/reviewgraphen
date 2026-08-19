#!/usr/bin/env bash
# Six agentic trials, strictly alternating skill/noskill, one at a time.
#
# Sequential only: never two requests in flight. Any upstream failure stops
# the series; no trial is retried and no trial is replaced.
set -uo pipefail

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
exp="$root/benchmarks/m9-agentic-local-v1"
runs=/tmp/m9v3-runs
log=/tmp/m9v3-series.log
mkdir -p "$runs"

note() { printf '%s %s\n' "$(date -Is)" "$*" | tee -a "$log"; }

note "SERIES START"
for index in 1 2 3; do
  for arm in skill noskill; do
    trial="${arm}-${index}"
    rm -rf "${runs:?}/$trial"
    note "START $trial"
    bash "$exp/scripts/run_trial.sh" "$arm" "$trial" "$runs/$trial" >> "$log" 2>&1
    trial_status=$?
    # run_trial.sh refuses before issuing any request when a pre-flight gate
    # fails (exit 66). Verifying a trial that never ran produced a misleading
    # `verdict=target_file_missing` once; skip straight to the stop instead.
    if (( trial_status == 66 )); then
      note "STOP $trial refused by a pre-flight gate -- no generation request was issued"
      printf 'NOT A TRIAL. run_trial.sh refused before issuing any generation\nrequest, because a pre-flight gate failed. No stream exists. Excluded\nfrom every count.\n' \
        > "$runs/$trial/NOT-A-TRIAL.md"
      rm -f "$runs/$trial/verification.json" "$runs/$trial/loop-summary.txt" "$runs/$trial/verify.log"
      exit 66
    fi
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

    # In-flight cache-credit check. The identity
    # sum(per-turn contexts) == input_tokens total does NOT discriminate --
    # measured identical in both cache states -- so the discriminator is
    # wall clock against the full-recompute projection.
    python3 "$exp/scripts/check_cache_credit.py" "$runs/$trial" \
      > "$runs/$trial/cache-credit.log" 2>&1
    cache_verdict=$(python3 -c "import json;print(json.load(open('$runs/$trial/cache-credit.json'))['verdict'])" 2>/dev/null || echo unknown)
    cache_ratio=$(python3 -c "import json;print(json.load(open('$runs/$trial/cache-credit.json'))['ratio_elapsed_over_projection'])" 2>/dev/null || echo '?')
    note "CACHE $trial verdict=$cache_verdict ratio=$cache_ratio (threshold 0.6)"
    if [[ "$cache_verdict" != "cache_credited" ]]; then
      note "STOP $trial cache not credited -- this trial measures the OLD condition"
      exit 67
    fi

    # Backend truncated-tail bug: the local patch can be lost silently on a
    # package update, so this runs on every trial.
    python3 "$exp/scripts/detect_truncated_tail.py" "$runs/$trial" \
      > "$runs/$trial/truncated-tail.log" 2>&1
    tail_suspect=$(python3 -c "import json;print(json.load(open('$runs/$trial/truncated-tail.json'))['suspected'])" 2>/dev/null || echo unknown)
    note "TAIL $trial suspected=$tail_suspect"
    if [[ "$tail_suspect" == "True" ]]; then
      note "STOP $trial truncated-tail signature detected"
      exit 68
    fi
    bash "$exp/scripts/archive_trials.sh" >/dev/null 2>&1 || true
  done
done
note "SERIES COMPLETE: 6 trials"
