#!/usr/bin/env bash
# Launches the series driver in its OWN SESSION, detached from whatever
# launched it.
#
# Why this file exists. The v4 `noskill-1` trial died mid-loop, ~11 minutes
# in, with no loop-outcome and no verification -- not the model, not the
# server, not the cap. The driver had been started with `nohup ... &`, and
# `nohup` only ignores SIGHUP: it leaves the process in the launcher's
# process group, so a group-targeted kill takes the whole tree down.
#
# Measured, not assumed. A `setsid`-launched child was started from a
# background task, the task was allowed to complete, and the child was still
# running afterwards in its own session and group:
#
#     STILL ALIVE: 1486495 pgid 1486495 sid 1486495 elapsed 00:14
#
# `setsid` puts the driver in a fresh session and process group, so a group
# kill aimed at the launcher cannot reach it. The driver's own pgid is
# recorded so halt.sh can still stop it deliberately.
set -uo pipefail

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
exp="$root/benchmarks/m9-agentic-local-v1"
runs=/tmp/m9v4-runs
mkdir -p "$runs"

setsid bash "$exp/scripts/run_series.sh" </dev/null >/tmp/m9v4-driver.log 2>&1 &
sleep 1
driver=$(ps -eo pid,args --no-headers | awk '/[r]un_series\.sh/ {print $1; exit}')
if [[ -n "$driver" ]]; then
  ps -o pgid= -p "$driver" | tr -d ' ' > "$runs/driver.pgid"
  echo "driver pid=$driver pgid=$(cat "$runs/driver.pgid") sid=$(ps -o sid= -p "$driver" | tr -d ' ')"
else
  echo "driver did not start" >&2
  exit 1
fi
