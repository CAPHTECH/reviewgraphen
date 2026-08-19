#!/usr/bin/env bash
# Records its own PGID, then execs the real command in the same process.
#
# Run under `setsid`, this process is the session and group leader, so `$$`
# IS the process-group id, and `exec` keeps that pid/pgid for the sandbox and
# everything it spawns. That gives halt.sh one id that reliably covers the
# whole trial. Reading `$!` of `setsid --wait` would have recorded setsid's
# own pid instead, which is not the leader when setsid forks.
set -uo pipefail
printf '%s\n' "$$" > "$1"
shift
exec "$@"
