#!/usr/bin/env python3
"""Generate and validate the frozen target-context research artifact."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
EXP = ROOT / "benchmarks/m10-target-context-local-v1"
REVISION = "05573bb6b3491b6849512e75e055d4cf6279b606"


def main() -> None:
    command = [
        str(ROOT / "target/debug/reviewgraphen-benchmark"),
        "project-target-context",
        str(ROOT.parent),
        str(ROOT),
        "reviewgraphen-m10",
        REVISION,
        REVISION,
        "visit_block",
    ]
    completed = subprocess.run(command, check=True, capture_output=True)
    value = json.loads(completed.stdout)
    if value["schema"] != "reviewgraphen.benchmark.target_context.v1":
        raise SystemExit("unexpected projection schema")
    if value["target"]["label"].split("::")[-2] != "visit_block":
        raise SystemExit("projection did not select visit_block")
    windows = [window for source in value["sources"] for window in source["windows"]]
    if not any("fn visit_block" in window["text"] for window in windows):
        raise SystemExit("projection omitted its selected target")
    output = EXP / "projection/visit_block.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(completed.stdout.rstrip(b"\n") + b"\n")


if __name__ == "__main__":
    main()
