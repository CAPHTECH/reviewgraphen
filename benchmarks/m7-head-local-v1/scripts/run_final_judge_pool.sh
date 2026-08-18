#!/usr/bin/env bash
# Runs the final blind judge pass over the 4-unit pool built by
# build_final_judge_pool.py (POOL_SOURCE_MANIFEST.json v2, after the
# /tmp data loss -- see RECOVERY.md), per JUDGE_PROTOCOL.md. Mirrors the
# credential-copy-and-delete discipline of AUTH_AND_BUDGET_AMENDMENT.md
# (fresh mode-700 temp dir per call, deleted immediately after).
#
# Per the operator's post-loss operational change (RECOVERY.md), each
# unit's result is copied into the repo's diagnostics/ tree immediately
# after that unit's call returns -- not batched at the end -- so a
# second /tmp loss mid-run cannot destroy anything already settled.
set -euo pipefail

POOL_ROOT=${1:?usage: run_final_judge_pool.sh <pool-root> <results-root>}
RESULTS_ROOT=${2:?usage: run_final_judge_pool.sh <pool-root> <results-root>}
REPO_DEST=${3:?usage: run_final_judge_pool.sh <pool-root> <results-root> <repo-diagnostics-dest>}

REPO_ROOT=/home/rizumita/workspace/reviewgraphen
BWRAP=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
CLAUDE_BIN=$(readlink -f /home/rizumita/.local/share/mise/installs/claude/latest/claude)
BENCHMARK_BIN="$REPO_ROOT/target/debug/reviewgraphen-benchmark"

mkdir -p "$RESULTS_ROOT" "$REPO_DEST"

# unit_id:per-call-budget-usd. Only the 4 units with a surviving pool
# after the /tmp loss (head-local-03/05/06 are artifact_lost).
UNITS=(
  "head-local-00:4"
  "head-local-01:3.5"
  "head-local-02:3.5"
  "head-local-04:1.5"
)

for entry in "${UNITS[@]}"; do
  unit_id="${entry%%:*}"
  budget="${entry##*:}"
  # input_root MUST be agent_input/ itself, not its parent -- the parent
  # also contains truth.json (a sibling, deliberately kept outside
  # agent_input/ so it can never be admitted here). admit_current walks
  # everything under input_root; pointing it at the parent would leak
  # truth.json's arm-identity mapping into the judge's materialized
  # prompt.
  input_root="$POOL_ROOT/$unit_id/agent_input"
  output_root="$RESULTS_ROOT/$unit_id/process-output"
  record_output="$RESULTS_ROOT/$unit_id/process-record.json"
  mkdir -p "$RESULTS_ROOT/$unit_id"

  cred_home=$(mktemp -d /tmp/m7-head-local-v1-final-judge-cred.XXXXXX)
  chmod 700 "$cred_home"
  cp ~/.claude/.credentials.json "$cred_home/.credentials.json"
  cleanup() { rm -rf "$cred_home"; }
  trap cleanup EXIT

  echo "START unit=$unit_id budget=\$$budget"
  set +e
  REVIEWGRAPHEN_CLAUDE_MAX_BUDGET_USD="$budget" \
    "$BENCHMARK_BIN" run-process-reviewer-constrained claude \
    "$input_root" "output-schema.json" \
    "$output_root" "$record_output" \
    "$BWRAP" "$cred_home" "$CLAUDE_BIN" opus high \
    2> "$RESULTS_ROOT/$unit_id/stderr.log"
  exit_code=$?
  set -e
  cleanup
  trap - EXIT

  echo "DONE unit=$unit_id exit_code=$exit_code"
  if [ "$exit_code" -ne 0 ]; then
    cat "$RESULTS_ROOT/$unit_id/stderr.log"
  fi

  # Copy this unit's result into the repo immediately -- do not wait for
  # the whole batch to finish, per the post-loss operational change.
  mkdir -p "$REPO_DEST/$unit_id"
  cp -r "$RESULTS_ROOT/$unit_id/." "$REPO_DEST/$unit_id/"
  echo "COPIED unit=$unit_id to $REPO_DEST/$unit_id"
done
echo "ALL_DONE"
