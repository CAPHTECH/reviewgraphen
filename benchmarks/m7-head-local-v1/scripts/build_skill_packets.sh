#!/usr/bin/env bash
set -euo pipefail

# Builds the qwen_skill arm's agent_input packet for every unit in
# units.json: the frozen reviewgraphen-methodology skill body (verbatim,
# post-frontmatter) as instruction.txt, the unchanged candidate-output
# schema, and the unit's raw source files (same source set as the qwen_b1
# arm). Read-only against the pinned fsl HEAD commit (git show only).
# Per QWEN_SKILL_ARM_AMENDMENT.md.

root=/home/rizumita/workspace/reviewgraphen
units_json="$root/benchmarks/m7-head-local-v1/units.json"
skill_file="$root/.claude/skills/reviewgraphen-methodology/SKILL.md"
schema_file="$root/schemas/reviewgraphen.benchmark.candidate_output.v1.schema.json"
fsl=/home/rizumita/github/fsl
prepared_root=/tmp/m7-head-local-v1-skill-prepared

expected_skill_body_sha256=a43d6984f9b58d5e47256afed68cddc1598ae84d6fc564e05f1cf462228d670a

if [[ -e "$prepared_root" ]]; then
  echo "prepared root must be fresh: $prepared_root" >&2
  exit 64
fi
mkdir -p "$prepared_root"

skill_body="$prepared_root/.skill-body.txt"
awk 'BEGIN{c=0} /^---$/{c++; next} c>=2{print}' "$skill_file" > "$skill_body"
observed_hash=$(sha256sum "$skill_body" | awk '{print $1}')
if [[ "$observed_hash" != "$expected_skill_body_sha256" ]]; then
  echo "skill body hash drift: expected $expected_skill_body_sha256 got $observed_hash" >&2
  exit 65
fi

head_commit=$(jq -r '.head_commit' "$units_json")
count=$(jq '.units | length' "$units_json")
for ((i = 0; i < count; i++)); do
  unit_id=$(jq -r ".units[$i].unit_id" "$units_json")
  echo "building skill packet for $unit_id"
  dest="$prepared_root/$unit_id/agent_input"
  mkdir -p "$dest/source"
  cp "$skill_body" "$dest/instruction.txt"
  cp "$schema_file" "$dest/candidate-output.schema.json"
  while IFS= read -r path; do
    out="$dest/source/$path"
    mkdir -p "$(dirname "$out")"
    git -C "$fsl" show "$head_commit:$path" > "$out"
  done < <(jq -r ".units[$i].files[].path" "$units_json")
done

echo "DONE built $count qwen_skill packets under $prepared_root"
