#!/usr/bin/env bash
# Materializes one trial's isolated working copy.
#
# Differences from m8's equivalent, both deliberate:
#
# 1. The harness-owned acceptance test is NOT installed. In m8 the model
#    never had a filesystem, so the test could sit in the tree harmlessly.
#    Here the agent can read and write files, so the acceptance test is kept
#    out of the tree entirely and only added afterwards, by the verifier, to
#    a copy the agent can no longer touch.
# 2. The pinned revision (05573bb) predates benchmarks/m8-impl-local-v1, so a
#    `git archive` of it contains neither the acceptance test nor any earlier
#    result. Verified, not assumed: the archive is checked below.
#
# usage: make_scratch.sh <fresh-scratch-dir> [--with-skill]
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 <fresh-scratch-dir> [--with-skill]" >&2
  exit 64
fi

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
m8="$root/benchmarks/m8-impl-local-v1"
scratch=$1
with_skill=${2:-}

case "$scratch" in
  /tmp/*) ;;
  *) echo "refusing scratch outside /tmp: $scratch" >&2; exit 64 ;;
esac
if [[ -e "$scratch" ]]; then
  echo "scratch directory must be fresh: $scratch" >&2
  exit 64
fi

revision=$(cat "$m8/task/PINNED_REVISION")
mkdir -p "$scratch"
git -C "$root" archive "$revision" | tar -x -C "$scratch"

# The acceptance test and every prior m8 artifact must be absent.
if [[ -e "$scratch/benchmarks" ]]; then
  echo "pinned archive unexpectedly contains benchmarks/; refusing" >&2
  exit 65
fi
if grep -rl "m8_extern_block_shadow" "$scratch" >/dev/null 2>&1; then
  echo "acceptance test leaked into the working copy; refusing" >&2
  exit 65
fi

if [[ "$with_skill" == "--with-skill" ]]; then
  mkdir -p "$scratch/.claude/skills/reviewgraphen-implementation-methodology"
  cp "$m8/skill/IMPLEMENTATION_SKILL.md" \
     "$scratch/.claude/skills/reviewgraphen-implementation-methodology/SKILL.md"
fi

echo "$scratch"
