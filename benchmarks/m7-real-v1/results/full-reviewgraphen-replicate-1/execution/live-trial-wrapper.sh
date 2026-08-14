#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 8 ]]; then
  echo "usage: $0 <codex|claude> <input-root> <output-root> <record-output> <credential-home> <backend-executable> <model> <effort>" >&2
  exit 2
fi

exec /home/rizumita/workspace/reviewgraphen/target/debug/reviewgraphen-benchmark \
  run-process-reviewer "$1" "$2" agent_input/candidate-output.schema.json \
  "$3" "$4" \
  /home/rizumita/.local/share/mise/installs/codex/0.147.0/codex-resources/bwrap \
  "$5" "$6" "$7" "$8"
