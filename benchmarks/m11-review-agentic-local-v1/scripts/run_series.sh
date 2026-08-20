#!/usr/bin/env bash
set -euo pipefail
root=/home/rizumita/workspace/reviewgraphen
exp=$root/benchmarks/m11-review-agentic-local-v1
for spec in reviewgraphen:reviewgraphen-1 control:control-1 reviewgraphen:reviewgraphen-2 control:control-2 reviewgraphen:reviewgraphen-3 control:control-3; do
  arm=${spec%%:*}
  trial=${spec##*:}
  bash "$exp/scripts/run_trial.sh" "$arm" "$trial" "$exp/runs/$trial"
done

