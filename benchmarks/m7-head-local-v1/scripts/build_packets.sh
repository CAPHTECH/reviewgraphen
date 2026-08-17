#!/usr/bin/env bash
set -euo pipefail

# Builds b1 and full_reviewgraphen agent_input packets for every unit in
# units.json, by staging a fresh single-commit Git snapshot per unit
# (base: empty commit; head: the unit's exact HEAD-pinned files, read via
# `git show <commit>:<path>` against the frozen fsl HEAD) and calling the
# oracle-free `prepare-head-local-unit` subcommand. Read-only against
# /home/rizumita/github/fsl: only `git show` is used, never a working-tree
# checkout or write.

root=/home/rizumita/workspace/reviewgraphen
units_json="$root/benchmarks/m7-head-local-v1/units.json"
fsl=/home/rizumita/github/fsl
benchmark="$root/target/debug/reviewgraphen-benchmark"
prepared_root=/tmp/m7-head-local-v1-prepared

if [[ -e "$prepared_root" ]]; then
  echo "prepared root must be fresh: $prepared_root" >&2
  exit 64
fi
mkdir -p "$prepared_root"

head_commit=$(jq -r '.head_commit' "$units_json")

count=$(jq '.units | length' "$units_json")
for ((i = 0; i < count; i++)); do
  unit_id=$(jq -r ".units[$i].unit_id" "$units_json")
  echo "building packets for $unit_id"
  stage="$prepared_root/$unit_id-stage"
  mkdir -p "$stage/repository"
  git -C "$stage/repository" init -q
  git -C "$stage/repository" -c user.name="ReviewGraphen M7" -c user.email="m7@example.invalid" \
    commit -q --allow-empty -m empty
  base=$(git -C "$stage/repository" rev-parse HEAD)
  while IFS= read -r path; do
    dest="$stage/repository/$path"
    mkdir -p "$(dirname "$dest")"
    git -C "$fsl" show "$head_commit:$path" > "$dest"
  done < <(jq -r ".units[$i].files[].path" "$units_json")
  git -C "$stage/repository" add -- .
  git -C "$stage/repository" -c user.name="ReviewGraphen M7" -c user.email="m7@example.invalid" \
    commit -q -m snapshot
  head_rev=$(git -C "$stage/repository" rev-parse HEAD)

  output="$prepared_root/$unit_id"
  "$benchmark" prepare-head-local-unit "$unit_id" "$stage" "$stage/repository" "$base" "$head_rev" "$output" \
    > "$prepared_root/$unit_id.prepare-record.json"
done

echo "DONE built $count unit packet sets under $prepared_root"
