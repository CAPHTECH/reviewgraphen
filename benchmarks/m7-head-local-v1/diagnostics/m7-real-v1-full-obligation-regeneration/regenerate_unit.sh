#!/usr/bin/env bash
set -euo pipefail

# Regenerates a single m7-real-v1 full_review_graphen unit's agent_input
# packet (b1 and full) from its manifest, for obligation-content
# verification only. Read-only against benchmarks/m7-real-v1/ and against
# the live fsl repository (git show only). Writes only under
# <output-dir>, which must be fresh. Does not modify any m7-real-v1
# record.
#
# This does NOT achieve full byte-for-byte packet identity (see
# REGENERATION_REPORT.md's "What was not achieved" section) - 6 of 17
# files matched exactly for snapshot-01, including the load-bearing
# mechanism-ontology.json. It is sufficient to confirm obligation content
# (property_id / applicability_status / applicability_reasons), which is
# independent of the tree-identity details that didn't reproduce exactly.
#
# Usage: regenerate_unit.sh <snapshot-id, e.g. snapshot-01> <output-dir>
#
# Requires: benchmarks/m7-real-v1/results/full-reviewgraphen-replicate-2/
# manifests/<snapshot-id>.json to exist (gives input_tree_hash and
# source_inventory), and benchmarks/m7-real-v1/scripts/build_corpus.py's
# projected_blob() redaction (imported directly, not reimplemented).

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <snapshot-id> <output-dir>" >&2
  exit 64
fi
snapshot_id=$1
output_dir=$2

root=/home/rizumita/workspace/reviewgraphen
manifest="$root/benchmarks/m7-real-v1/results/full-reviewgraphen-replicate-2/manifests/$snapshot_id.json"
fsl=/home/rizumita/github/fsl
benchmark="$root/target/debug/reviewgraphen-benchmark"

if [[ ! -f "$manifest" ]]; then
  echo "no manifest for $snapshot_id: $manifest" >&2
  exit 64
fi
if [[ -e "$output_dir" ]]; then
  echo "output dir must be fresh: $output_dir" >&2
  exit 64
fi

tree_hash=$(jq -r '.input_tree_hash | sub("^git:"; "")' "$manifest")
stage=$(mktemp -d /tmp/m7-real-v1-regen.XXXXXX)
mkdir -p "$stage/repository"

python3 - "$manifest" "$tree_hash" "$stage/repository" <<'PYEOF'
import sys, json, importlib.util, os, hashlib

manifest_path, tree_hash, repo_dir = sys.argv[1:4]
root = "/home/rizumita/workspace/reviewgraphen"
spec = importlib.util.spec_from_file_location(
    "m7_real_builder", f"{root}/benchmarks/m7-real-v1/scripts/build_corpus.py"
)
mod = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = mod
spec.loader.exec_module(mod)

manifest = json.load(open(manifest_path))
mismatches = []
for entry in manifest["source_inventory"]:
    path = entry["path"]
    expected = entry["content_hash"].removeprefix("sha256:")
    data = mod.projected_blob(tree_hash, path)
    got = hashlib.sha256(data).hexdigest()
    dest = os.path.join(repo_dir, path)
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    with open(dest, "wb") as f:
        f.write(data)
    if got != expected:
        mismatches.append((path, expected, got))

if mismatches:
    for path, expected, got in mismatches:
        print(f"CONTENT HASH MISMATCH: {path} expected={expected} got={got}", file=sys.stderr)
    sys.exit(1)
print(f"content-verified {len(manifest['source_inventory'])} files for {manifest['unit_id']}")
PYEOF

cd "$stage/repository"
git init -q
git -c user.name="ReviewGraphen M7" -c user.email="m7@example.invalid" \
  commit -q --allow-empty -m empty
base=$(git rev-parse HEAD)
git add -A -- .
git -c user.name="ReviewGraphen M7" -c user.email="m7@example.invalid" \
  commit -q -m snapshot
head_rev=$(git rev-parse HEAD)

"$benchmark" prepare-head-local-unit \
  "diagnostic-m7-real-$snapshot_id" "$stage" "$stage/repository" "$base" "$head_rev" "$output_dir"

rm -rf "$stage"
echo "obligations written to $output_dir/full/agent_input/obligations.json"
