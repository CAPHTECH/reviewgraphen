#!/usr/bin/env python3
"""Audit candidate forms omitted by the frozen M7 real v2 frame."""

from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from collections import Counter
from pathlib import Path


DEFAULT_ROOT = Path(__file__).resolve().parents[3]


def load_enumerator(root: Path):
    enumerator = root / "benchmarks/m7-real-v2/scripts/enumerate_candidates.py"
    spec = importlib.util.spec_from_file_location("m7_v2_enumerator", enumerator)
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
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit(f"output exists: {args.output}")

    module = load_enumerator(args.repository_root)
    frame_path = args.frame or (
        args.repository_root / "benchmarks/m7-real-v2/private/candidate-frame.json"
    )
    frame = json.loads(frame_path.read_bytes())
    reason_counts: Counter[str] = Counter()
    strengthened: list[dict[str, object]] = []
    inline: list[dict[str, object]] = []
    nonfix_with_added_integration = 0
    unused_fix_with_production = 0

    for record in frame["records"]:
        reasons = set(record["exclusion_reasons"])
        reason_counts.update(reasons)
        has_production = bool(record["production_paths"])
        is_unused_fix = (
            "not_fix_subject" not in reasons
            and "used_by_m7_real_v1" not in reasons
        )
        if (
            "not_fix_subject" in reasons
            and has_production
            and record["test_options"]
        ):
            nonfix_with_added_integration += 1
        if not is_unused_fix or not has_production:
            continue
        unused_fix_with_production += 1
        parent = str(record["parent_commit"]).removeprefix("git:")
        fix = str(record["fix_commit"]).removeprefix("git:")

        strengthened_options: list[dict[str, str]] = []
        for path in record["test_paths"]:
            match = module.INTEGRATION_TEST_PATH.match(path)
            if match is None:
                continue
            crate_dir, relative_bin = match.groups()
            package = module.package_name(fix, crate_dir)
            if package is None or "/" in relative_bin:
                continue
            before = module.test_functions(module.blob(parent, path))
            after = module.test_functions(module.blob(fix, path))
            for name in sorted(before & after):
                strengthened_options.append(
                    {
                        "package": package,
                        "test_bin": relative_bin,
                        "test_name": name,
                        "test_path": path,
                        "test_change_kind": "strengthened_existing",
                    }
                )
        if strengthened_options and not record["test_options"]:
            strengthened.append(
                {
                    "fix_commit": record["fix_commit"],
                    "parent_commit": record["parent_commit"],
                    "split_hash": record["split_hash"],
                    "options": strengthened_options,
                }
            )

        inline_options: list[dict[str, object]] = []
        for path in record["production_paths"]:
            before = module.test_functions(module.blob(parent, path))
            after = module.test_functions(module.blob(fix, path))
            added = sorted(after - before)
            if added:
                inline_options.append({"path": path, "test_names": added})
        if inline_options and record["frame_status"] != "candidate":
            inline.append(
                {
                    "fix_commit": record["fix_commit"],
                    "also_strengthened_candidate": any(
                        value["fix_commit"] == record["fix_commit"]
                        for value in strengthened
                    ),
                    "options": inline_options,
                }
            )

    strengthened_ids = {str(value["fix_commit"]) for value in strengthened}
    inline_only_ids = {
        str(value["fix_commit"])
        for value in inline
        if str(value["fix_commit"]) not in strengthened_ids
    }
    current = int(frame["frame_counts"].get("candidate", 0))
    result = {
        "schema": "reviewgraphen.benchmark.m7_real_v2_candidate_recall_audit.v1",
        "frozen_frame_sha256": module.sha256(frame_path.read_bytes()),
        "reason_counts": dict(sorted(reason_counts.items())),
        "unused_fix_with_production_rust_count": unused_fix_with_production,
        "current_added_integration_candidate_count": current,
        "strengthened_existing_integration_candidate_count": len(strengthened),
        "added_inline_unit_candidate_count": len(inline),
        "added_inline_unit_only_candidate_count": len(inline_only_ids),
        "identified_test_form_union_upper_bound": (
            current + len(strengthened_ids) + len(inline_only_ids)
        ),
        "nonfix_subject_with_production_and_added_integration_count": (
            nonfix_with_added_integration
        ),
        "nonfix_subject_candidates_are_eligible": False,
        "strengthened_existing_integration": strengthened,
        "added_inline_unit": inline,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(
        json.dumps(result, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    )


if __name__ == "__main__":
    main()
