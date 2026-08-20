#!/usr/bin/env bash
# Stops every trial process and PROVES it, or exits non-zero.
#
# Written after a halt reported success while a trial kept generating for
# another twelve minutes against a swapped backend. Three faults caused
# that; all three are addressed here and each is named at its fix:
#
#  1. SELF-MATCH. `pkill -f "claude --print"` matches the argv of the shell
#     running the pkill, because that string sits inside its own command
#     line. The killing shell terminated itself mid-sequence, so the later
#     kills never ran. Confirmed empirically: `pgrep -af TOKEN` lists both
#     the zsh wrapper and the bash child of the very command containing
#     TOKEN.
#     Fix: never pattern-match on argv text supplied inline. Kill recorded
#     process GROUPS by id, and exclude this script's own group from every
#     sweep.
#
#  2. NO DESCENDANT KILL. Killing run_trial.sh left `timeout` -> `bwrap` ->
#     `claude` running; they were never signalled.
#     Fix: run_trial.sh launches under `setsid`, records its PGID, and this
#     script signals the whole group with `kill -- -PGID`.
#
#  3. TRUNCATED VERIFICATION. The check was `ps | grep | head -10`, and
#     "none running" was read off a list cut at ten lines. This is the fault
#     that turned a failed halt into a success report.
#     Fix: verification is a COUNT, never eyeballed, never truncated, and a
#     non-zero count makes this script exit non-zero.
#  4. A RECORDED GROUP ID IS NOT ENOUGH. Observed live during the v2 series:
#     GNU `timeout` puts its managed command in a NEW process group, so the
#     recorded pgid covered `bwrap` (1126010) but `timeout` and `claude` sat
#     in group 1126011. `kill -- -<recorded>` would have left the client
#     running -- fault 2 in a new costume.
#     Fix: derive a group id from EVERY survivor found by path and kill
#     those groups too, not just the recorded one.
#
#  5. THE DRIVER ITSELF must be stoppable. It now runs in its own session
#     (scripts/launch_series.sh) so a group kill aimed at whatever
#     launched it cannot reach it -- which is the point -- but that also
#     means this script has to target it deliberately. Its pgid is
#     recorded at <runs>/driver.pgid and swept with the trial groups.
#
# `--dry-run` prints what would be signalled and sends nothing.
set -uo pipefail

dry_run=0
if [[ "${1:-}" == "--dry-run" ]]; then
  dry_run=1
  shift
fi
runs=${1:-/tmp/m9v2-runs}
own_pgid=$(ps -o pgid= -p $$ | tr -d ' ')

survivors() {
  ps -eo pid,pgid,etime,args --no-headers 2>/dev/null \
    | awk -v own="$own_pgid" '$2 != own && (/codex-resources\/bwrap/ || /(^| |\/)claude --print/)'
}

for signal in TERM TERM KILL; do
  for pgid_file in "$runs"/driver.pgid "$runs"/*/trial.pgid; do
    [[ -f "$pgid_file" ]] || continue
    pgid=$(tr -d ' \n' < "$pgid_file")
    [[ -n "$pgid" && "$pgid" != "$own_pgid" ]] || continue
    if (( dry_run )); then
      echo "would send SIG$signal to recorded group $pgid"
    elif kill -"$signal" -- "-$pgid" 2>/dev/null; then
      echo "sent SIG$signal to recorded group $pgid"
    fi
  done
  # Every survivor's OWN process group, which is not necessarily the
  # recorded one (see fault 4), then the survivor itself as a backstop.
  while read -r pid pgid _rest; do
    [[ -n "$pgid" && "$pgid" != "$own_pgid" ]] || continue
    if (( dry_run )); then
      echo "would send SIG$signal to group $pgid (survivor pid $pid)"
    elif kill -"$signal" -- "-$pgid" 2>/dev/null; then
      echo "sent SIG$signal to group $pgid (survivor pid $pid)"
    fi
  done < <(survivors)
  while read -r pid _rest; do
    [[ -n "$pid" ]] || continue
    if (( dry_run )); then
      echo "would send SIG$signal to pid $pid"
    elif kill -"$signal" "$pid" 2>/dev/null; then
      echo "sent SIG$signal to pid $pid"
    fi
  done < <(survivors)
  if (( dry_run )); then
    break
  fi
  sleep 3
  if [[ -z "$(survivors)" ]]; then
    break
  fi
done

if (( dry_run )); then
  echo "--- dry run: survivors that would be targeted ---"
  survivors
  echo "--- end of list ---"
  echo "survivor_count=$(survivors | wc -l)"
  exit 0
fi

echo "--- survivor list (complete, untruncated) ---"
survivors
echo "--- end of list ---"
remaining=$(survivors | wc -l)
echo "survivor_count=$remaining"
if (( remaining != 0 )); then
  echo "HALT FAILED: $remaining process(es) still running" >&2
  exit 1
fi
echo "HALT VERIFIED: zero trial processes"
