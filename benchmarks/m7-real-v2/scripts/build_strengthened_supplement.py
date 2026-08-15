#!/usr/bin/env python3
"""Build the frozen strengthened-test candidate supplement."""

from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from pathlib import Path


DEFAULT_ROOT = Path(__file__).resolve().parents[3]


def load_enumerator(root: Path):
    path = root / "benchmarks/m7-real-v2/scripts/enumerate_candidates.py"
    spec = importlib.util.spec_from_file_location("m7_v2_enumerator", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load frozen enumerator")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository-root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--frame", type=Path)
    parser.add_argument("--audit", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit(f"output exists: {args.output}")
    root = args.repository_root
    frame_path = args.frame or root / "benchmarks/m7-real-v2/private/candidate-frame.json"
    audit_path = args.audit or root / "benchmarks/m7-real-v2/private/candidate-recall-audit.json"
    frame = json.loads(frame_path.read_bytes())
    audit = json.loads(audit_path.read_bytes())
    module = load_enumerator(root)
    by_fix = {str(value["fix_commit"]): value for value in frame["records"]}
    records: list[dict[str, object]] = []
    counts: dict[str, int] = {}

    for supplement in audit["strengthened_existing_integration"]:
        original = by_fix[str(supplement["fix_commit"])]
        fix = str(original["fix_commit"]).removeprefix("git:")
        parent = str(original["parent_commit"]).removeprefix("git:")
        status = "candidate"
        reasons: list[str] = []
        if original["projection_error_private"] is not None:
            status = "excluded"
            reasons.append("blind_projection_failed")
        options: list[dict[str, str]] = []
        for option in supplement["options"]:
            path = str(option["test_path"])
            before = module.blob(parent, path)
            after = module.blob(fix, path)
            if before is None or after is None:
                status = "excluded"
                reasons.append("test_path_missing_on_parent_or_fix")
                continue
            options.append(
                {
                    **option,
                    "parent_test_source_sha256": module.sha256(before),
                    "fix_test_source_sha256": module.sha256(after),
                }
            )
        if not options:
            status = "excluded"
            reasons.append("no_strengthened_exact_test_option")
        record = {
            "schema": "reviewgraphen.benchmark.m7_real_v2_strengthened_candidate.v1",
            "fix_commit": original["fix_commit"],
            "parent_commit": original["parent_commit"],
            "commit_date": original["commit_date"],
            "subject_private": original["subject_private"],
            "frame_status": status,
            "exclusion_reasons": sorted(set(reasons)),
            "production_paths": original["production_paths"],
            "test_paths": original["test_paths"],
            "test_options": sorted(
                options,
                key=lambda value: (
                    value["test_path"], value["test_bin"], value["test_name"]
                ),
            ),
            "features": original["features"],
            "split_hash": original["split_hash"],
            "projection_error_private": original["projection_error_private"],
        }
        records.append(record)
        counts[status] = counts.get(status, 0) + 1
    records.sort(key=lambda value: str(value["split_hash"]))
    result = {
        "schema": "reviewgraphen.benchmark.m7_real_v2_strengthened_candidate_frame.v1",
        "source_candidate_frame_sha256": module.sha256(frame_path.read_bytes()),
        "source_recall_audit_sha256": module.sha256(audit_path.read_bytes()),
        "record_count": len(records),
        "frame_counts": counts,
        "records": records,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    )


if __name__ == "__main__":
    main()
