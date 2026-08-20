#!/usr/bin/env bash
set -uo pipefail

root=/home/rizumita/workspace/reviewgraphen
exp="$root/benchmarks/m10-target-context-local-v1"
runs=/tmp/m10-runs
mkdir -p "$runs"
setsid bash "$exp/scripts/run_series.sh" </dev/null >/tmp/m10-driver.log 2>&1 &
sleep 1
driver=$(ps -eo pid,args --no-headers | awk '/[m]10-target-context-local-v1\/scripts\/run_series\.sh/ {print $1; exit}')
if [[ -n "$driver" ]]; then
  ps -o pgid= -p "$driver" | tr -d ' ' > "$runs/driver.pgid"
  echo "driver pid=$driver pgid=$(cat "$runs/driver.pgid")"
else
  echo "driver did not start" >&2
  exit 1
fi
