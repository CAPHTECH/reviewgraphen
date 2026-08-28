#!/usr/bin/env python3
"""Run an 8bit/medium final review over frozen 4bit/low selections."""

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
PREDECESSOR = ROOT / "benchmarks/m17-casegraphen-controlled-review-v1/runs/4bit-low-r1"
EXPECTED_PROMPT_SHA256 = "bac06a4c145ba00eb1e5f5ddd2e90400305d45f7930cdba6738742f1ecf1b0aa"
EXPECTED_RUNTIME_SHA256 = "03e549390dcd5030340d178aad9884ca5979db350b9e33923255d4fe523c1286"
CARDS = ROOT / "benchmarks/m15-qwen-intelligent-reviewgraphen-v1/cards"
SELECTED_CARDS = ["target-body", "statement-rewrite", "expression-rewrite"]

SPEC = importlib.util.spec_from_file_location("m17_checkpoint_hybrid_final", SOURCE)
if SPEC is None or SPEC.loader is None:
    raise SystemExit("cannot load frozen m17 checkpoint runtime")
RUNTIME = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNTIME)
RUNTIME.EXP = EXP


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit(
            "usage: run_hybrid_4bit_select_8bit_medium_final1200.py RESULT_DIR"
        )
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

    predecessor_runtime = PREDECESSOR / "runtime-node-reports.jsonl"
    if RUNTIME.sha256(predecessor_runtime.read_bytes()) != EXPECTED_RUNTIME_SHA256:
        raise SystemExit("4bit selector runtime hash mismatch")
    prompt_path = PREDECESSOR / "node_4bit-low-r1_final-review.prompt.txt"
    prompt = prompt_path.read_text()
    if RUNTIME.sha256(prompt.encode()) != EXPECTED_PROMPT_SHA256:
        raise SystemExit("4bit-derived final prompt hash mismatch")

    overview = json.loads((CARDS / "overview.json").read_text())
    received = [
        json.loads((CARDS / f"{card_id}.json").read_text())
        for card_id in SELECTED_CARDS
    ]
    projection_ids = [overview["projection_id"]] + [
        card["projection_id"] for card in received
    ]
    source_ids = {
        source_id for card in received for source_id in card["source_ids"]
    }
    trace: list[dict[str, Any]] = [
        {
            "node_id": "node:hybrid-r1:reuse-4bit-low-selections",
            "executor_class": "frozen-runtime-handoff",
            "selector_model": "Qwen3.8-27B-MLX-4bit",
            "selector_effort": "low",
            "predecessor_runtime_report_sha256": EXPECTED_RUNTIME_SHA256,
            "selector_elapsed_seconds": 392.463,
            "prompt_sha256": EXPECTED_PROMPT_SHA256,
            "selected_cards": SELECTED_CARDS,
            "projection_ids": projection_ids,
            "status": 0,
        }
    ]
    try:
        review = RUNTIME.run_qwen(
            node_id="node:hybrid-r1:8bit-medium-final-review",
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
                "node_id": "runtime:hybrid-r1",
                "status": 1,
                "error": f"{type(error).__name__}: {error}",
            }
        )

    trace_path = result_dir / "runtime-node-reports.jsonl"
    trace_path.write_bytes(b"".join(RUNTIME.canonical(item) for item in trace))
    review_path = result_dir / "review.json"
    final_elapsed = next(
        (
            entry.get("elapsed_seconds")
            for entry in trace
            if entry["node_id"] == "node:hybrid-r1:8bit-medium-final-review"
        ),
        None,
    )
    result = {
        "schema": "reviewgraphen.benchmark.m18_hybrid_review_result.v1",
        "trial": "hybrid-4bit-low-select-8bit-medium-final1200-r1",
        "selector_model": "Qwen3.8-27B-MLX-4bit",
        "selector_effort": "low",
        "final_model": "Qwen3.8-27B-MLX-8bit",
        "final_effort": "medium",
        "outcome": outcome,
        "selected_cards": SELECTED_CARDS,
        "projection_ids": projection_ids,
        "selector_elapsed_seconds": 392.463,
        "final_elapsed_seconds": final_elapsed,
        "effective_hybrid_elapsed_seconds": (
            round(392.463 + final_elapsed, 3)
            if isinstance(final_elapsed, (int, float))
            else None
        ),
        "runtime_report_sha256": RUNTIME.sha256(trace_path.read_bytes()),
        "review_sha256": (
            RUNTIME.sha256(review_path.read_bytes()) if review_path.exists() else None
        ),
        "authority": "runtime observation; unreviewed and not accepted evidence",
    }
    (result_dir / "result.json").write_bytes(RUNTIME.canonical(result))
    print(json.dumps(result, sort_keys=True))
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
