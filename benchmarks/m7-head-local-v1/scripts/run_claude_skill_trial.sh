#!/usr/bin/env bash
# Runs one claude_skill generation trial (CLAUDE_SKILL_ARM_AMENDMENT.md).
# Usage: run_claude_skill_trial.sh <unit_id> <input_root> <run_root> <max_budget_usd>
set -euo pipefail

UNIT_ID="$1"
INPUT_ROOT="$2"
RUN_ROOT="$3"
BUDGET="$4"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
BWRAP="/home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap"
CLAUDE_BIN="$(readlink -f /home/rizumita/.local/share/mise/installs/claude/latest/claude)"
BENCHMARK_BIN="$REPO_ROOT/target/debug/reviewgraphen-benchmark"

OUTPUT_ROOT="$RUN_ROOT/$UNIT_ID/process-output"
RECORD_OUTPUT="$RUN_ROOT/$UNIT_ID/process-record.json"
mkdir -p "$RUN_ROOT/$UNIT_ID"

CRED_HOME="$(mktemp -d /tmp/m7-head-local-v1-claude-skill-cred.XXXXXX)"
chmod 700 "$CRED_HOME"
cp ~/.claude/.credentials.json "$CRED_HOME/.credentials.json"

cleanup() {
  rm -rf "$CRED_HOME"
}
trap cleanup EXIT

# Unconstrained (no --json-schema): candidate-output.schema.json has a
# top-level allOf for its abstained/parse_failure conditional, which
# Claude's tool-input-schema rejects outright ("does not support oneOf,
# anyOf, allOf at the top level", confirmed via a real, zero-cost 400 on
# 2026-08-18). Matches the precedent already used for every generation
# arm in this benchmark family (qwen_b1/qwen_full/qwen_skill all run via
# the unconstrained run-process-reviewer-codex-profile, never
# -constrained) — schema conformance is checked downstream via
# `validate candidate`, per ADR 0037's extraction contract, not enforced
# provider-side. Only the judge (a different, top-level-allOf-free
# schema) uses -constrained.
set +e
REVIEWGRAPHEN_CLAUDE_MAX_BUDGET_USD="$BUDGET" \
  "$BENCHMARK_BIN" run-process-reviewer claude \
  "$INPUT_ROOT" "agent_input/candidate-output.schema.json" \
  "$OUTPUT_ROOT" "$RECORD_OUTPUT" \
  "$BWRAP" "$CRED_HOME" "$CLAUDE_BIN" opus high \
  2> "$RUN_ROOT/$UNIT_ID/stderr.log"
EXIT_CODE=$?
set -e

echo "unit=$UNIT_ID exit_code=$EXIT_CODE record_exists=$([ -f "$RECORD_OUTPUT" ] && echo yes || echo no)"
if [ "$EXIT_CODE" -ne 0 ]; then
  cat "$RUN_ROOT/$UNIT_ID/stderr.log"
fi
exit "$EXIT_CODE"
