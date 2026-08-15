#!/usr/bin/env python3
"""Prepare frozen v2 probe and positive plans without invoking a model."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess


REPLICATE = 4
STAGE_2_RANKS = (6, 13)


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


def trial_record(
    snapshot: str,
    arm: str,
    root: pathlib.Path,
    benchmark_unit_id: str | None = None,
) -> dict[str, object]:
    manifest = root / "manifest.json"
    value = json.loads(manifest.read_text())
    total, count = admitted_size(root)
    record: dict[str, object] = {
        "snapshot_id": snapshot,
        "arm": arm,
        "trial_id": value["trial_id"],
        "input_root": str(root),
        "admitted_input_bytes": total,
        "admitted_file_count": count,
        "manifest_sha256": sha256(manifest),
    }
    if benchmark_unit_id is not None:
        record["benchmark_unit_id"] = benchmark_unit_id
    return record


def arm_records(
    snapshot: str,
    prepared_b1: pathlib.Path,
    prepared_full: pathlib.Path,
    benchmark_unit_id: str | None = None,
    reverse: bool = False,
) -> list[dict[str, object]]:
    records = [
        trial_record(
            snapshot,
            "b1_free_form",
            prepared_b1 / snapshot / "b1" / f"replicate-{REPLICATE}",
            benchmark_unit_id,
        ),
        trial_record(
            snapshot,
            "full_reviewgraphen",
            prepared_full / snapshot / "full_review_graphen" / f"replicate-{REPLICATE}",
            benchmark_unit_id,
        ),
    ]
    return list(reversed(records)) if reverse else records


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("repository_root", type=pathlib.Path)
    parser.add_argument("prepared_b1", type=pathlib.Path)
    parser.add_argument("prepared_full", type=pathlib.Path)
    parser.add_argument("plan_output", type=pathlib.Path)
    args = parser.parse_args()
    root = args.repository_root.resolve(strict=True)
    public = root / "benchmarks/m7-real-v1/public"
    units_dir = root / "benchmarks/m7-real-v1/private/units"
    config = root / "benchmarks/m7-local-factorial-v2/execution-config.local.json"
    benchmark = root / "target/debug/reviewgraphen-benchmark"
    for output in (args.prepared_b1, args.prepared_full, args.plan_output):
        if output.exists():
            raise RuntimeError(f"refusing existing output: {output}")
    subprocess.run(
        [str(benchmark), "prepare-real", str(public), str(units_dir), str(args.prepared_b1), str(config), str(REPLICATE)],
        check=True,
    )
    subprocess.run(
        [str(benchmark), "prepare-real-full", str(public), str(units_dir), str(args.prepared_full), str(config), str(REPLICATE)],
        check=True,
    )
    units = [json.loads(path.read_text()) for path in sorted(units_dir.glob("real-unit-*.json"))]
    if len(units) != 20:
        raise RuntimeError("expected exactly twenty units")
    controls = [unit["control_trial_unit_id"] for unit in units]
    positives = [unit["positive_trial_unit_id"] for unit in units]
    if len(set(controls)) != 20 or len(set(positives)) != 20:
        raise RuntimeError("expected distinct positive and control snapshots")
    sizes = sorted(
        (
            admitted_size(args.prepared_b1 / snapshot / "b1" / f"replicate-{REPLICATE}")[0],
            snapshot,
        )
        for snapshot in controls
    )
    stage_1_ids = [sizes[0][1], sizes[-1][1]]
    remaining = [entry for entry in sizes if entry[1] not in stage_1_ids]
    stage_2_ids = [remaining[rank - 1][1] for rank in STAGE_2_RANKS]
    positive_trials: list[dict[str, object]] = []
    for index, unit in enumerate(units):
        positive_trials.extend(
            arm_records(
                unit["positive_trial_unit_id"],
                args.prepared_b1,
                args.prepared_full,
                unit["benchmark_unit_id"],
                reverse=index % 2 == 1,
            )
        )
    plan = {
        "schema": "reviewgraphen.benchmark.m7_local_v2_plan.v1",
        "replicate_index": REPLICATE,
        "selection_basis": "B1 admitted bytes; ties by snapshot_id",
        "all_control_b1_sizes": [
            {"snapshot_id": snapshot, "admitted_input_bytes": size} for size, snapshot in sizes
        ],
        "stage_1": {
            "snapshot_ids": stage_1_ids,
            "trials": [
                trial
                for snapshot in stage_1_ids
                for trial in arm_records(snapshot, args.prepared_b1, args.prepared_full)
            ],
        },
        "stage_2_if_expanded": {
            "remaining_one_based_ranks": list(STAGE_2_RANKS),
            "snapshot_ids": stage_2_ids,
            "trials": [
                trial
                for snapshot in stage_2_ids
                for trial in arm_records(snapshot, args.prepared_b1, args.prepared_full)
            ],
        },
        "positive": {"trials": positive_trials},
    }
    args.plan_output.write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
