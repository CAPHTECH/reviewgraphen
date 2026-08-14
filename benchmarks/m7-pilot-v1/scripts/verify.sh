#!/usr/bin/env bash
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
public="$root/public"
private="$root/private"
commitments="$root/commitments.v1.json"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

mapfile -t manifests < <(find "$public" -type f -path '*/base/Cargo.toml' -o -type f -path '*/head/Cargo.toml' | sort)
if [[ ${#manifests[@]} -ne 16 ]]; then
  echo "expected 16 revision manifests, found ${#manifests[@]}" >&2
  exit 1
fi

for manifest in "${manifests[@]}"; do
  cargo fmt --manifest-path "$manifest" --check
  unit="$scratch/$(printf '%s' "$manifest" | sha256sum | awk '{print $1}')"
  mkdir "$unit"
  cp -R "$(dirname -- "$manifest")"/. "$unit"
  CARGO_TARGET_DIR="$scratch/cargo-target" cargo test --manifest-path "$unit/Cargo.toml"
done

python3 - "$root" "$commitments" <<'PY'
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import tempfile

root = pathlib.Path(sys.argv[1])
commitments = json.loads(pathlib.Path(sys.argv[2]).read_text())
expected = {entry["case_id"]: entry["oracle_sha256"] for entry in commitments["entries"]}
oracle_paths = sorted((root / "private").glob("*.json"))
if len(expected) != 8 or len(oracle_paths) != 8:
    raise SystemExit("expected eight committed private records")

required = {"root_id", "tree_hash", "path", "file_sha256", "symbol", "start_line", "end_line", "span_sha256", "mechanism_tags", "severity"}
for oracle_path in oracle_paths:
    raw = oracle_path.read_bytes()
    digest = "sha256:" + hashlib.sha256(raw).hexdigest()
    record = json.loads(raw)
    if record.get("schema") != "reviewgraphen.benchmark.oracle.v1":
        raise SystemExit(f"invalid oracle schema: {oracle_path.name}")
    unit_id = record.get("unit_id")
    if expected.pop(unit_id, None) != digest:
        raise SystemExit(f"commitment mismatch: {oracle_path.name}")
    case_dir = oracle_path.stem
    head = root / "public" / case_dir / "head"
    packet = json.loads((root / "public" / case_dir / "packet.json").read_text())
    if packet.get("case_id") != unit_id:
        raise SystemExit(f"packet/oracle mismatch: {case_dir}")
    for finding in record["roots"]:
        if set(finding) != required or finding["start_line"] > finding["end_line"]:
            raise SystemExit(f"invalid oracle root: {oracle_path.name}")
        source = head / finding["path"]
        contents = source.read_bytes()
        if finding["file_sha256"] != "sha256:" + hashlib.sha256(contents).hexdigest():
            raise SystemExit(f"file anchor mismatch: {oracle_path.name}")
        lines = contents.splitlines(keepends=True)
        span = b"".join(lines[finding["start_line"] - 1:finding["end_line"]])
        if finding["span_sha256"] != "sha256:" + hashlib.sha256(span).hexdigest():
            raise SystemExit(f"span anchor mismatch: {oracle_path.name}")
        with tempfile.TemporaryDirectory() as temporary:
            temporary_path = pathlib.Path(temporary)
            for child in head.iterdir():
                if child.name == "target":
                    continue
                target = temporary_path / child.name
                if child.is_dir():
                    subprocess.run(["cp", "-R", str(child), str(target)], check=True)
                else:
                    target.write_bytes(child.read_bytes())
            subprocess.run(["git", "init", "-q"], cwd=temporary_path, check=True)
            subprocess.run(["git", "add", "."], cwd=temporary_path, check=True)
            actual_tree = subprocess.check_output(["git", "write-tree"], cwd=temporary_path, text=True).strip()
        if finding["tree_hash"] != "git:" + actual_tree:
            raise SystemExit(f"tree anchor mismatch: {oracle_path.name}")
if expected:
    raise SystemExit("missing private record for commitment")
PY

echo "m7 pilot verification passed"
