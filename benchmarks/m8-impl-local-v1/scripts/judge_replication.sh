#!/usr/bin/env bash
# One blind judge call per DISTINCT change, per AMENDMENT-003.md section 7.
#
# Not one call per pool: with nine distinct changes a single call would grade
# them relative to one another, so the verdicts would not be independent --
# and a 5-versus-5 comparison requires that they are. It would also let the
# judge notice clustering among near-identical diffs, which is an
# arm-identity leak the content-addressed IDs otherwise prevent.
#
# Each pool contains exactly one change. Trials whose resulting file is
# byte-identical share a change_id, are judged once, and that verdict is
# attributed to each such trial.
set -uo pipefail

root=/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726
exp="$root/benchmarks/m8-impl-local-v1"
pools=/tmp/m8-rep-judge
results=/tmp/m8-rep-judge-results
rm -rf "$pools" "$results"
mkdir -p "$pools" "$results"

declare -A SEEN=()
for src in /tmp/m8-impl-local-v1-runs/rep-*; do
  trial=$(basename "$src")
  [[ -f "$src/patched-lib.rs" ]] || { echo "SKIP $trial (no applicable edit)"; continue; }
  hash=$(sha256sum "$src/patched-lib.rs" | cut -c1-16)
  if [[ -n "${SEEN[$hash]:-}" ]]; then
    echo "DEDUP $trial shares change with ${SEEN[$hash]}"
    continue
  fi
  SEEN[$hash]=$trial
  python3 "$exp/scripts/build_judge_pool.py" task2 "$pools/$hash" "anon=$src" \
    > "$pools/$hash-build.json" 2>&1 || { echo "POOL FAIL $trial"; continue; }
  bash "$exp/scripts/run_judge.sh" "$pools/$hash" "$results/$hash" \
    && echo "JUDGED $trial ($hash)" || echo "JUDGE FAIL $trial ($hash)"
done

# Trial -> change_id mapping, withheld from every judge call.
python3 - "$results" <<'PY'
import hashlib, json, sys
from pathlib import Path
mapping = {}
for src in sorted(Path("/tmp/m8-impl-local-v1-runs").glob("rep-*")):
    patched = src / "patched-lib.rs"
    if not patched.exists():
        continue
    digest = hashlib.sha256(patched.read_bytes()).hexdigest()[:16]
    mapping.setdefault(digest, []).append(src.name)
Path("/tmp/m8-rep-judge-truth.json").write_text(
    json.dumps(
        {
            "schema": "reviewgraphen.benchmark.m8_replication_truth.v1",
            "note": "result-file hash -> trials. Never admitted to any judge call.",
            "resultfile_to_trials": mapping,
        },
        indent=2,
        sort_keys=True,
    )
    + "\n"
)
print(json.dumps(mapping, indent=2, sort_keys=True))
PY
