#!/usr/bin/env python3
"""Retry only the frozen 8bit/medium final-review prompt with a 1200s limit."""

from __future__ import annotations

import importlib.util
import json
import pathlib
import subprocess
import sys
from typing import Any


ROOT = pathlib.Path("/home/rizumita/workspace/reviewgraphen")
EXP = ROOT / "benchmarks/m18-8bit-low-checkpoint-review-v1"
SOURCE = ROOT / "benchmarks/m17-casegraphen-controlled-review-v1/scripts/run_arm.py"
PREDECESSOR = EXP / "diagnostics/resident-8bit-medium-timeout600-r1/run"
EXPECTED_PROMPT_SHA256 = "94fb0aa9cc444458a9ca41785272166241c66eb599e8cf10ba5272eb1d8ef219"
CARDS = ROOT / "benchmarks/m15-qwen-intelligent-reviewgraphen-v1/cards"
SELECTED_CARDS = ["target-body", "statement-rewrite", "compose-rewrite"]

SPEC = importlib.util.spec_from_file_location("m17_checkpoint_final1200", SOURCE)
if SPEC is None or SPEC.loader is None:
    raise SystemExit("cannot load frozen m17 checkpoint runtime")
RUNTIME = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNTIME)
RUNTIME.EXP = EXP


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("usage: run_medium_final1200.py RESULT_DIR")
    result_dir = pathlib.Path(sys.argv[1]).resolve()
    if result_dir.exists():
        raise SystemExit("result directory must be fresh")
    result_dir.mkdir(parents=True)

    identity_out = result_dir / "backend-identity.json"
    identity = subprocess.run(
        [
            "python3",
            str(RUNTIME.BASE / "scripts/check_backend_identity.py"),
            (EXP / "PINNED_BACKEND_IDENTITY").read_text().strip(),
            (EXP / "PINNED_BACKEND_HEALTH").read_text().strip(),
            str(identity_out),
        ],
        check=False,
    )
    if identity.returncode != 0:
        raise SystemExit(66)

    prompt_path = PREDECESSOR / "node_8bit-medium-r1_final-review.prompt.txt"
    prompt = prompt_path.read_text()
    if RUNTIME.sha256(prompt.encode()) != EXPECTED_PROMPT_SHA256:
        raise SystemExit("predecessor final prompt hash mismatch")

    overview = json.loads((CARDS / "overview.json").read_text())
    received = [json.loads((CARDS / f"{card_id}.json").read_text()) for card_id in SELECTED_CARDS]
    projection_ids = [overview["projection_id"]] + [card["projection_id"] for card in received]
    source_ids = {source_id for card in received for source_id in card["source_ids"]}
    trace: list[dict[str, Any]] = [
        {
            "node_id": "node:8bit-medium-final1200-r1:reuse-selections",
            "executor_class": "frozen-runtime-handoff",
            "predecessor_runtime_report_sha256": "4d8fd6b16afbc34323567d7c927d172ea5c7f12f7bd3375d36de4ef6b5134429",
            "prompt_sha256": EXPECTED_PROMPT_SHA256,
            "selected_cards": SELECTED_CARDS,
            "projection_ids": projection_ids,
            "status": 0,
        }
    ]
    try:
        review = RUNTIME.run_qwen(
            node_id="node:8bit-medium-final1200-r1:final-review",
            prompt=prompt,
            model="Qwen3.8-27B-MLX-8bit",
            effort="medium",
            timeout=1200,
            result_dir=result_dir,
            trace=trace,
            resume=False,
        )
        RUNTIME.validate_review(review, projection_ids, source_ids)
        (result_dir / "review.json").write_bytes(RUNTIME.canonical(review))
        outcome = "completed"
        exit_code = 0
    except Exception as error:  # noqa: BLE001
        outcome = "incomplete"
        exit_code = 1
        trace.append(
            {
                "node_id": "runtime:8bit-medium-final1200-r1",
                "status": 1,
                "error": f"{type(error).__name__}: {error}",
            }
        )

    trace_path = result_dir / "runtime-node-reports.jsonl"
    trace_path.write_bytes(b"".join(RUNTIME.canonical(item) for item in trace))
    review_path = result_dir / "review.json"
    result = {
        "schema": "reviewgraphen.benchmark.m18_post_timeout_recovery_result.v1",
        "trial": "8bit-medium-final1200-r1",
        "requested_model": "Qwen3.8-27B-MLX-8bit",
        "requested_effort": "medium",
        "outcome": outcome,
        "selected_cards": SELECTED_CARDS,
        "projection_ids": projection_ids,
        "runtime_report_sha256": RUNTIME.sha256(trace_path.read_bytes()),
        "review_sha256": RUNTIME.sha256(review_path.read_bytes()) if review_path.exists() else None,
        "authority": "runtime observation; unreviewed and not accepted evidence",
    }
    (result_dir / "result.json").write_bytes(RUNTIME.canonical(result))
    print(json.dumps(result, sort_keys=True))
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
