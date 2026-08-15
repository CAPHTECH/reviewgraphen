#!/usr/bin/env python3
"""Prepare frozen control-only schema-probe inputs without invoking a model."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess


REPLICATE = 3
STAGE_2_RANKS = (4, 7, 14, 17)


def sha256(path: pathlib.Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def admitted_size(root: pathlib.Path) -> tuple[int, int]:
    total = 0
    count = 0
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise RuntimeError(f"symlink in prepared input: {path}")
        if path.is_file():
            total += path.stat().st_size
            count += 1
    if count == 0:
        raise RuntimeError(f"empty prepared input: {root}")
    return total, count


def trial_record(snapshot: str, arm: str, root: pathlib.Path) -> dict[str, object]:
    manifest = root / "manifest.json"
    value = json.loads(manifest.read_text())
    total, count = admitted_size(root)
    return {
        "snapshot_id": snapshot,
        "arm": arm,
        "trial_id": value["trial_id"],
        "input_root": str(root),
        "admitted_input_bytes": total,
        "admitted_file_count": count,
        "manifest_sha256": sha256(manifest),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("repository_root", type=pathlib.Path)
    parser.add_argument("prepared_b1_g3", type=pathlib.Path)
    parser.add_argument("prepared_full", type=pathlib.Path)
    parser.add_argument("plan_output", type=pathlib.Path)
    args = parser.parse_args()
    root = args.repository_root.resolve(strict=True)
    public = root / "benchmarks/m7-real-v1/public"
    units = root / "benchmarks/m7-real-v1/private/units"
    config = root / "benchmarks/m7-local-factorial-v1/execution-config.local.json"
    benchmark = root / "target/debug/reviewgraphen-benchmark"
    for output in (args.prepared_b1_g3, args.prepared_full, args.plan_output):
        if output.exists():
            raise RuntimeError(f"refusing existing output: {output}")
    subprocess.run(
        [str(benchmark), "prepare-real", str(public), str(units), str(args.prepared_b1_g3), str(config), str(REPLICATE)],
        check=True,
    )
    subprocess.run(
        [str(benchmark), "prepare-real-full", str(public), str(units), str(args.prepared_full), str(config), str(REPLICATE)],
        check=True,
    )
    controls: list[str] = []
    for unit_path in sorted(units.glob("real-unit-*.json")):
        unit = json.loads(unit_path.read_text())
        controls.append(unit["control_trial_unit_id"])
    if len(controls) != 20 or len(set(controls)) != 20:
        raise RuntimeError("expected exactly twenty distinct controls")
    sizes: list[tuple[int, str]] = []
    all_trials: dict[str, list[dict[str, object]]] = {}
    for snapshot in controls:
        b1 = args.prepared_b1_g3 / snapshot / "b1" / f"replicate-{REPLICATE}"
        total, _ = admitted_size(b1)
        sizes.append((total, snapshot))
        all_trials[snapshot] = [
            trial_record(snapshot, "b1_free_form", b1),
            trial_record(snapshot, "g3_proxy", args.prepared_b1_g3 / snapshot / "g3_proxy" / f"replicate-{REPLICATE}"),
            trial_record(snapshot, "full_reviewgraphen", args.prepared_full / snapshot / "full_review_graphen" / f"replicate-{REPLICATE}"),
        ]
    sizes.sort()
    stage_1_ids = [sizes[0][1], sizes[-1][1]]
    remaining = [entry for entry in sizes if entry[1] not in stage_1_ids]
    stage_2_ids = [remaining[rank - 1][1] for rank in STAGE_2_RANKS]
    plan = {
        "schema": "reviewgraphen.benchmark.m7_local_schema_probe_plan.v1",
        "replicate_index": REPLICATE,
        "selection_basis": "total regular-file bytes admitted by ProcessReviewerInput for B1 control trial; ties by snapshot_id",
        "all_control_b1_sizes": [
            {"snapshot_id": snapshot, "admitted_input_bytes": size} for size, snapshot in sizes
        ],
        "stage_1": {"snapshot_ids": stage_1_ids, "trials": [trial for snapshot in stage_1_ids for trial in all_trials[snapshot]]},
        "stage_2_if_expanded": {"remaining_one_based_ranks": list(STAGE_2_RANKS), "snapshot_ids": stage_2_ids, "trials": [trial for snapshot in stage_2_ids for trial in all_trials[snapshot]]},
    }
    args.plan_output.write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
