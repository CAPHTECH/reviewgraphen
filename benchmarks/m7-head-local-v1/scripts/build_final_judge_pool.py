#!/usr/bin/env python3
"""Builds the final m7-head-local-v1 blind judge pool, per JUDGE_PROTOCOL.md
sections 2-5, from POOL_SOURCE_MANIFEST.json only.

No directory globbing, no "latest file," no filesystem search of any kind.
Every candidate.json is named explicitly in the manifest with an expected
SHA-256; a mismatch aborts (does not silently proceed), so this script
cannot pick a different file than the one the manifest names, regardless
of what else exists on disk or when it was run relative to other attempts.

Output, per unit with a nonempty pool:
  <output_dir>/<unit_id>/agent_input/00-instructions.md
  <output_dir>/<unit_id>/agent_input/01-findings.json  (judge-visible)
  <output_dir>/<unit_id>/agent_input/sources/<path>...
  <output_dir>/<unit_id>/truth.json                     (NOT under agent_input/)

truth.json is written as a sibling of agent_input/, never inside it, so it
is trivial to avoid ever mounting it into the judge's sandbox (the judge
call only ever admits the agent_input/ subtree as input_root).
"""

from __future__ import annotations

import hashlib
import json
import shutil
import sys
from collections import defaultdict
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPTS_DIR = Path(__file__).resolve().parent
JUDGE_PROTOCOL = SCRIPTS_DIR.parent / "JUDGE_PROTOCOL.md"
MANIFEST_PATH = SCRIPTS_DIR.parent / "diagnostics" / "final-judge-pool" / "POOL_SOURCE_MANIFEST.json"
RECOVERED_SOURCES_ROOT = Path("/tmp/m7-head-local-v1-recovered-sources")
JUDGE_OUTPUT_SCHEMA = SCRIPTS_DIR.parent / "schemas" / "reviewgraphen.benchmark.head_local_judge_output.v1.schema.json"

sys.path.insert(0, str(SCRIPTS_DIR))
from scan_forbidden_markers import ForbiddenMarkerError, scan_files  # noqa: E402

KNOWN_PREFIXES = ("agent_input/source/", "source/")


def canonical_json_bytes(value) -> bytes:
    """Matches reviewgraphen_core::canonical_json: recursively sort object
    keys, compact separators, no ASCII-escaping of non-ASCII text."""
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def extract_frozen_prompt() -> str:
    """Extracts 00-instructions.md's exact text from JUDGE_PROTOCOL.md's own
    fenced block, rather than a hand-retyped copy that could drift or
    contain a transcription error."""
    text = JUDGE_PROTOCOL.read_text(encoding="utf-8")
    marker = "## 5. Judge prompt (frozen, verbatim)"
    idx = text.index(marker)
    after = text[idx:]
    # Locate the first ``` fence after the marker, then its matching close.
    first_fence = after.index("```")
    body_start = after.index("\n", first_fence) + 1
    closing_fence = after.index("```", body_start)
    prompt = after[body_start:closing_fence]
    if prompt.endswith("\n"):
        prompt = prompt[:-1]
    return prompt


def stripped_output_schema_bytes() -> bytes:
    """$schema/$id removed from a copy used only for the CLI --json-schema
    argument (and, per the already-approved interim precedent recorded in
    diagnostics/interim-judge-head-local-00-01-02/, materialized into the
    judge's own prompt like every other input file) -- the canonical
    schema file on disk is never modified."""
    schema = json.loads(JUDGE_OUTPUT_SCHEMA.read_bytes())
    schema.pop("$schema", None)
    schema.pop("$id", None)
    return canonical_json_bytes(schema)


def normalize_path(path: str) -> str:
    for prefix in KNOWN_PREFIXES:
        if path.startswith(prefix):
            return path[len(prefix):]
    raise ValueError(f"unrecognized location path prefix, cannot normalize: {path!r}")


def load_manifest() -> dict:
    return json.loads(MANIFEST_PATH.read_bytes())


def verify_and_load_candidate(entry: dict) -> dict:
    path_str = entry["path"]
    path = Path(path_str) if Path(path_str).is_absolute() else REPO_ROOT / path_str
    if not path.is_file():
        raise SystemExit(f"manifest entry missing file: {path}")
    data = path.read_bytes()
    observed = hashlib.sha256(data).hexdigest()
    if observed != entry["sha256"]:
        raise SystemExit(
            f"manifest hash mismatch for {entry['unit_id']}/{entry['arm']}: "
            f"expected {entry['sha256']}, got {observed} at {path}"
        )
    candidate = json.loads(data)
    if len(candidate["findings"]) != entry["findings"]:
        raise SystemExit(
            f"manifest finding-count mismatch for {entry['unit_id']}/{entry['arm']}: "
            f"expected {entry['findings']}, got {len(candidate['findings'])}"
        )
    return candidate


def finding_id(unit_id: str, locations: list, mechanism_tags: list, rationale: str, severity) -> str:
    preimage = {
        "unit_id": unit_id,
        "locations": locations,
        "mechanism_tags": mechanism_tags,
        "rationale": rationale,
        "severity": severity,
    }
    digest = hashlib.sha256(canonical_json_bytes(preimage)).hexdigest()
    return "finding:" + digest[:32]


def records_from_raw_candidate(unit_id: str, entry: dict) -> list[dict]:
    candidate = verify_and_load_candidate(entry)
    records = []
    for f in candidate["findings"]:
        normalized_locations = [
            {**loc, "path": normalize_path(loc["path"])} for loc in f["locations"]
        ]
        severity = f.get("severity")
        fid = finding_id(unit_id, normalized_locations, f["mechanism_tags"], f["rationale"], severity)
        records.append({
            "finding_id": fid,
            "locations": normalized_locations,
            "mechanism_tags": f["mechanism_tags"],
            "severity": severity,
            "rationale": f["rationale"],
        })
    return records


def records_from_recovered_pooled_findings(unit_id: str, entry: dict) -> list[dict]:
    """The manifest points at an already-pooled/blinded findings file
    (recovered from an earlier judge pass after the original candidate.json
    was lost -- see RECOVERY.md). Its findings already have normalized
    paths and a computed finding_id; re-verify self-consistency here too
    (redundant with the recovery-time check, cheap, and this is exactly
    the kind of place where staying paranoid costs nothing)."""
    path_str = entry["path"]
    path = Path(path_str) if Path(path_str).is_absolute() else REPO_ROOT / path_str
    if not path.is_file():
        raise SystemExit(f"manifest entry missing file: {path}")
    data = path.read_bytes()
    observed = hashlib.sha256(data).hexdigest()
    if observed != entry["sha256"]:
        raise SystemExit(
            f"manifest hash mismatch for {entry['unit_id']}/{entry['arm']} (recovered_pooled_findings): "
            f"expected {entry['sha256']}, got {observed} at {path}"
        )
    pooled = json.loads(data)
    if len(pooled["findings"]) != entry["findings"]:
        raise SystemExit(
            f"manifest finding-count mismatch for {entry['unit_id']}/{entry['arm']}: "
            f"expected {entry['findings']}, got {len(pooled['findings'])}"
        )
    records = []
    for f in pooled["findings"]:
        recomputed = finding_id(unit_id, f["locations"], f["mechanism_tags"], f["rationale"], f.get("severity"))
        if recomputed != f["finding_id"]:
            raise SystemExit(
                f"finding_id self-consistency check failed for {unit_id}: "
                f"stored {f['finding_id']}, recomputed {recomputed}"
            )
        records.append({
            "finding_id": f["finding_id"],
            "locations": f["locations"],
            "mechanism_tags": f["mechanism_tags"],
            "severity": f.get("severity"),
            "rationale": f["rationale"],
        })
    return records


def build_unit_pool(unit_id: str, arm_entries: list[tuple[str, dict]], instructions: str, output_dir: Path) -> dict:
    pooled_by_id: dict[str, dict] = {}
    contributing_arms: dict[str, set[str]] = defaultdict(set)

    for arm, entry in arm_entries:
        source_type = entry.get("source_type", "raw_candidate")
        if source_type == "raw_candidate":
            records = records_from_raw_candidate(unit_id, entry)
        elif source_type == "recovered_pooled_findings":
            records = records_from_recovered_pooled_findings(unit_id, entry)
        else:
            raise SystemExit(f"unknown source_type {source_type!r} for {unit_id}/{arm}")
        for record in records:
            fid = record["finding_id"]
            if fid in pooled_by_id and pooled_by_id[fid] != record:
                raise SystemExit(f"finding_id collision with differing content for {unit_id}: {fid}")
            pooled_by_id[fid] = record
            contributing_arms[fid].add(arm)

    if not pooled_by_id:
        return {"unit_id": unit_id, "judge_call": "not_issued", "reason": "empty_pool"}

    sorted_ids = sorted(pooled_by_id.keys())
    findings_payload = {
        "schema": "reviewgraphen.benchmark.head_local_judge_input.v1",
        "unit_id": unit_id,
        "findings": [pooled_by_id[fid] for fid in sorted_ids],
    }

    packet_root = output_dir / unit_id / "agent_input"
    packet_root.mkdir(parents=True, exist_ok=True)
    findings_bytes = canonical_json_bytes(findings_payload)
    instructions_bytes = instructions.encode("utf-8")

    # Copy the unit's source tree, reconstructed from the fsl repo at the
    # frozen revision and hash-verified against units.json (the original
    # /tmp-prepared packets were destroyed; see RECOVERY.md). One source
    # tree per unit, not per arm (same_file_set_both_arms).
    prepared_source = RECOVERED_SOURCES_ROOT / unit_id
    sources_dest = packet_root / "sources"
    if sources_dest.exists():
        shutil.rmtree(sources_dest)
    shutil.copytree(prepared_source, sources_dest)

    schema_bytes = stripped_output_schema_bytes()
    packet_files = {
        "00-instructions.md": instructions_bytes,
        "01-findings.json": findings_bytes,
        "output-schema.json": schema_bytes,
    }
    for src_file in sources_dest.rglob("*"):
        if src_file.is_file():
            rel = "sources/" + str(src_file.relative_to(sources_dest))
            packet_files[rel] = src_file.read_bytes()

    try:
        scan_files(packet_files)
    except ForbiddenMarkerError as error:
        raise SystemExit(f"forbidden marker scan failed for {unit_id}: {error}") from error

    (packet_root / "00-instructions.md").write_bytes(instructions_bytes)
    (packet_root / "01-findings.json").write_bytes(findings_bytes)
    (packet_root / "output-schema.json").write_bytes(schema_bytes)

    truth = {
        "schema": "reviewgraphen.benchmark.head_local_judge_truth.v1",
        "unit_id": unit_id,
        "entries": [
            {
                "finding_id": fid,
                "contributing_arms": sorted(contributing_arms[fid]),
            }
            for fid in sorted_ids
        ],
    }
    (output_dir / unit_id / "truth.json").write_bytes(canonical_json_bytes(truth))

    return {
        "unit_id": unit_id,
        "judge_call": "issued",
        "pool_size": len(sorted_ids),
        "packet_root": str(packet_root),
    }


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: build_final_judge_pool.py <fresh-output-dir>")
    output_dir = Path(sys.argv[1]).resolve()
    if output_dir.exists():
        raise SystemExit(f"output dir must be fresh: {output_dir}")
    output_dir.mkdir(parents=True)

    manifest = load_manifest()
    instructions = extract_frozen_prompt()

    by_unit: dict[str, list[tuple[str, dict]]] = defaultdict(list)
    for entry in manifest["entries"]:
        by_unit[entry["unit_id"]].append((entry["arm"], entry))

    results = []
    for unit_id in sorted(by_unit):
        result = build_unit_pool(unit_id, by_unit[unit_id], instructions, output_dir)
        results.append(result)

    summary = {
        "schema": "reviewgraphen.benchmark.final_judge_pool_build_summary.v1",
        "units": results,
        "issued_count": sum(1 for r in results if r.get("judge_call") == "issued"),
        "not_issued_count": sum(1 for r in results if r.get("judge_call") == "not_issued"),
    }
    (output_dir / "build-summary.json").write_bytes(canonical_json_bytes(summary))
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
