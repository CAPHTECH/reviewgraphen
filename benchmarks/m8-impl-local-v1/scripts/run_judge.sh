#!/usr/bin/env bash
# Runs one blind code-quality judge call over a pool built by
# build_judge_pool.py, per AMENDMENT-001.md.
#
# Blinding mechanics mirror m7-head-local-v1/JUDGE_PROTOCOL.md:
#   --no-session-persistence  no memory of any other judge call, in this
#                             experiment or any other
#   --tools ""                the judge cannot read the filesystem, so it
#                             cannot discover arm labels, the acceptance
#                             test, the mechanical outcome, or the
#                             reference solution
# The packet is passed on stdin; nothing outside it is reachable.
#
# usage: run_judge.sh <pool-dir> <fresh-result-dir>
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <pool-dir> <fresh-result-dir>" >&2
  exit 64
fi
pool=$1
result=$2
if [[ -e "$result" ]]; then
  echo "result directory must be fresh: $result" >&2
  exit 64
fi

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
exp="$root/benchmarks/m8-impl-local-v1"
claude=$(readlink -f /home/rizumita/.local/share/mise/installs/claude/latest/claude)

# Abort-not-redact scan immediately before sending, not only at build time.
python3 "$exp/scripts/scan_forbidden_markers.py" "$pool"

mkdir -p "$result"
cat "$pool/00-instructions.md" "$pool/01-specification.md" "$pool/02-changes.md" \
  > "$result/prompt.txt"
sha256sum "$result/prompt.txt" > "$result/prompt.sha256"

started=$(date +%s)
set +e
"$claude" --print --no-session-persistence --tools "" --model opus \
  < "$result/prompt.txt" > "$result/judge-output.txt" 2> "$result/judge-stderr.txt"
status=$?
set -e
finished=$(date +%s)
printf '%s\n' "$status" > "$result/judge-status"
printf '%s\n' "$((finished - started))" > "$result/judge-elapsed-seconds"

python3 "$exp/scripts/extract_candidate_json.py" "$result/judge-output.txt" \
  > "$result/judgment.json"
echo "judge status=$status elapsed=$((finished - started))s"
