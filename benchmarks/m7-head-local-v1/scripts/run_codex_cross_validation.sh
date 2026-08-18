#!/usr/bin/env bash
# Runs the codex-backend cross-validation judge pass over the same
# 4-unit pool the Claude judge pass used, per CODEX_CROSS_VALIDATION_AMENDMENT.md.
# NOT for unattended use: no --max-budget-usd-equivalent exists for the
# Codex CLI (checked, see the amendment's §6), so this script pauses
# after each unit rather than running all 4 back to back unattended.
#
# usage: run_codex_cross_validation.sh <pool-root> <results-root> <repo-diagnostics-dest> [--smoke-test-only]
set -euo pipefail

POOL_ROOT=${1:?usage: run_codex_cross_validation.sh <pool-root> <results-root> <repo-diagnostics-dest> [--smoke-test-only]}
RESULTS_ROOT=${2:?usage: run_codex_cross_validation.sh <pool-root> <results-root> <repo-diagnostics-dest> [--smoke-test-only]}
REPO_DEST=${3:?usage: run_codex_cross_validation.sh <pool-root> <results-root> <repo-diagnostics-dest> [--smoke-test-only]}
SMOKE_TEST_ONLY=${4:-}

REPO_ROOT=/home/rizumita/workspace/reviewgraphen
BWRAP=/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap
CODEX_BIN=/home/rizumita/.local/share/mise/installs/codex/0.147.0/bin/codex
BENCHMARK_BIN="$REPO_ROOT/target/debug/reviewgraphen-benchmark"

MODEL="gpt-5.6-sol"
EFFORT="high"

mkdir -p "$RESULTS_ROOT" "$REPO_DEST"

run_one_unit() {
  local unit_id="$1"
  local input_root="$2"

  local output_root="$RESULTS_ROOT/$unit_id/process-output"
  local record_output="$RESULTS_ROOT/$unit_id/process-record.json"
  mkdir -p "$RESULTS_ROOT/$unit_id"

  local cred_home
  cred_home=$(mktemp -d /tmp/m7-head-local-v1-codex-judge-cred.XXXXXX)
  chmod 700 "$cred_home"
  cp ~/.codex/auth.json "$cred_home/auth.json"
  cleanup() { rm -rf "$cred_home"; }
  trap cleanup EXIT

  echo "START unit=$unit_id model=$MODEL effort=$EFFORT"
  set +e
  "$BENCHMARK_BIN" run-process-reviewer-constrained codex \
    "$input_root" "output-schema.json" \
    "$output_root" "$record_output" \
    "$BWRAP" "$cred_home" "$CODEX_BIN" "$MODEL" "$EFFORT" \
    2> "$RESULTS_ROOT/$unit_id/stderr.log"
  local exit_code=$?
  set -e
  cleanup
  trap - EXIT

  echo "DONE unit=$unit_id exit_code=$exit_code"
  if [ "$exit_code" -ne 0 ]; then
    cat "$RESULTS_ROOT/$unit_id/stderr.log"
    return "$exit_code"
  fi

  # Copy immediately -- do not wait for the whole batch (post-/tmp-loss rule).
  mkdir -p "$REPO_DEST/$unit_id"
  cp -r "$RESULTS_ROOT/$unit_id/." "$REPO_DEST/$unit_id/"
  echo "COPIED unit=$unit_id to $REPO_DEST/$unit_id"
}

if [ "$SMOKE_TEST_ONLY" = "--smoke-test-only" ]; then
  echo "Smoke test: build a trivial synthetic 1-file/1-finding packet and run it."
  echo "This script does not construct that packet -- build it by hand first"
  echo "(00-instructions.md with any short instruction text, 01-findings.json"
  echo "with one synthetic finding, output-schema.json = the real, unmodified"
  echo "schemas/reviewgraphen.benchmark.head_local_judge_output.v1.schema.json,"
  echo "sources/dummy.rs with a few lines of trivial code), then pass its"
  echo "agent_input/ directory as POOL_ROOT/<unit_id> with a single unit_id."
  echo "Required per JUDGE_PROTOCOL.md section 8 before any real unit below."
  exit 0
fi

echo "Real units. No --max-budget-usd-equivalent exists for codex (see"
echo "CODEX_CROSS_VALIDATION_AMENDMENT.md section 6) -- running one unit at a"
echo "time, not as an unattended batch. Check each result before continuing."
echo

for unit_id in head-local-00 head-local-01 head-local-02 head-local-04; do
  if [ -f "$RESULTS_ROOT/$unit_id/process-record.json" ]; then
    echo "SKIP unit=$unit_id (already completed in a prior invocation of this script)"
    continue
  fi
  run_one_unit "$unit_id" "$POOL_ROOT/$unit_id/agent_input"
  echo "--- pausing after $unit_id; inspect the result, then re-run this script"
  echo "    to continue with the next not-yet-completed unit ---"
  break
done
