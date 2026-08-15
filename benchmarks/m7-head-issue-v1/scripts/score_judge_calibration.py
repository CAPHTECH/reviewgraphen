#!/usr/bin/env python3
"""Strictly replay and score both M7 HEAD issue judges."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
from pathlib import Path

DISPOSITIONS = {"issue_should_be_created", "should_not_be_created", "unable_to_decide"}


def canonical(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def replay(binary: Path, record: Path) -> tuple[bytes, dict[str, object]]:
    record_value = json.loads(record.read_bytes())
    result = subprocess.run([str(binary), "replay-process-reviewer", str(record)], check=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode != 0:
        raise RuntimeError(result.stderr.decode(errors="replace"))
    if sha256(result.stdout) != record_value["raw_response_hash"]:
        raise RuntimeError(f"replay hash mismatch: {record}")
    return result.stdout, record_value


def parse_output(data: bytes, batch: dict[str, object]) -> list[dict[str, str]]:
    value = json.loads(data)
    if not isinstance(value, dict) or set(value) != {"schema", "batch_id", "judgments"}:
        raise RuntimeError("judge output has unknown or missing top-level fields")
    if value["schema"] != "reviewgraphen.benchmark.issue_judge_batch.v1" or value["batch_id"] != batch["batch_id"]:
        raise RuntimeError("judge output identity mismatch")
    if not isinstance(value["judgments"], list):
        raise RuntimeError("judgments must be an array")
    parsed = []
    for row in value["judgments"]:
        if not isinstance(row, dict) or set(row) != {"candidate_id", "disposition", "rationale"}:
            raise RuntimeError("judgment has unknown or missing fields")
        if row["disposition"] not in DISPOSITIONS:
            raise RuntimeError("unknown disposition")
        if not isinstance(row["rationale"], str) or not 1 <= len(row["rationale"]) <= 2048:
            raise RuntimeError("invalid rationale")
        parsed.append(row)
    expected = batch["candidate_ids"]
    observed = [row["candidate_id"] for row in parsed]
    if len(observed) != len(set(observed)) or set(observed) != set(expected):
        raise RuntimeError("judge output does not cover the exact candidate set")
    return parsed


def score_judge(judge: str, inventory: dict[str, object], truth: dict[str, dict[str, object]], runs: Path, binary: Path) -> dict[str, object]:
    judgments, backend_records = [], []
    expected_backend = {
        "codex": ("codex_cli", "openai", "gpt-5.6-sol", {"reasoning_effort": "high"}),
        "claude": ("claude_cli", "anthropic", "opus", {"effort": "high"}),
    }[judge]
    for batch in inventory["batches"]:
        record_path = runs / judge / batch["batch_id"] / "record.json"
        raw, record = replay(binary, record_path)
        backend = record["backend"]
        observed_backend = (backend["kind"], backend["provider"], backend["model"], backend["inference_settings"])
        if observed_backend != expected_backend:
            raise RuntimeError(f"unexpected judge backend assignment: {record_path}")
        if record["input_files"] != batch["input_inventory"]:
            raise RuntimeError(f"judge input inventory mismatch: {record_path}")
        backend_records.append(record["backend"])
        for row in parse_output(raw, batch):
            private = truth[row["candidate_id"]]
            judgments.append({**row, "label": private["label"], "benchmark_unit_id": private["benchmark_unit_id"], "record_path": str(record_path.relative_to(runs)), "raw_response_hash": record["raw_response_hash"]})
    positive = [row for row in judgments if row["label"] == "positive"]
    negative = [row for row in judgments if row["label"] == "negative"]
    tp = sum(row["disposition"] == "issue_should_be_created" for row in positive)
    fn = sum(row["disposition"] == "should_not_be_created" for row in positive)
    pu = sum(row["disposition"] == "unable_to_decide" for row in positive)
    tn = sum(row["disposition"] == "should_not_be_created" for row in negative)
    fp = sum(row["disposition"] == "issue_should_be_created" for row in negative)
    nu = sum(row["disposition"] == "unable_to_decide" for row in negative)
    sensitivity, specificity = tp / len(positive), tn / len(negative)
    balanced, unable = (sensitivity + specificity) / 2, (pu + nu) / len(judgments)
    checks = {"positive_sensitivity": sensitivity >= 0.75, "negative_specificity": specificity >= 0.75, "balanced_accuracy": balanced >= 0.75, "unable_rate": unable <= 0.20}
    return {
        "judge": judge,
        "backend_records": backend_records,
        "confusion": {"true_positive": tp, "false_negative": fn, "positive_unable": pu, "true_negative": tn, "false_positive": fp, "negative_unable": nu},
        "metrics": {"positive_sensitivity": sensitivity, "negative_specificity": specificity, "balanced_accuracy": balanced, "unable_rate": unable},
        "threshold_checks": checks,
        "passed": all(checks.values()),
        "judgments": sorted(judgments, key=lambda row: row["candidate_id"])
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("prepared", type=Path)
    parser.add_argument("runs", type=Path)
    parser.add_argument("benchmark_binary", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    inventory = json.loads((args.prepared / "calibration-inventory.json").read_bytes())
    private = json.loads((args.prepared / "calibration-truth.private.json").read_bytes())
    truth = {row["candidate_id"]: row for row in private["cases"]}
    if len(truth) != 40 or sum(row["label"] == "positive" for row in truth.values()) != 20 or sum(row["label"] == "negative" for row in truth.values()) != 20:
        raise RuntimeError("calibration truth is not the frozen twenty-pair design")
    judges = [score_judge(name, inventory, truth, args.runs, args.benchmark_binary) for name in ("codex", "claude")]
    result = {
        "schema": "reviewgraphen.benchmark.issue_judge_calibration_result.v1",
        "thresholds": {"positive_sensitivity_minimum": 0.75, "negative_specificity_minimum": 0.75, "balanced_accuracy_minimum": 0.75, "unable_rate_maximum": 0.20},
        "judges": judges,
        "proceed_to_production": all(row["passed"] for row in judges)
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(canonical(result))
    print(json.dumps({"proceed_to_production": result["proceed_to_production"], "judges": [{"judge": row["judge"], "passed": row["passed"], **row["metrics"]} for row in judges]}, indent=2))


if __name__ == "__main__":
    main()
