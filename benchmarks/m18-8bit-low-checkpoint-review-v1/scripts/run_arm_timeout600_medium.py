#!/usr/bin/env python3
"""Run resident 8bit/medium with 600-second model-node timeouts."""

from __future__ import annotations

import importlib.util
import pathlib


ROOT = pathlib.Path("/home/rizumita/workspace/reviewgraphen")
SOURCE = ROOT / "benchmarks/m17-casegraphen-controlled-review-v1/scripts/run_arm.py"
SPEC = importlib.util.spec_from_file_location("m17_checkpoint_runtime_8bit_medium", SOURCE)
if SPEC is None or SPEC.loader is None:
    raise SystemExit("cannot load frozen m17 checkpoint runtime")
RUNTIME = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNTIME)
RUNTIME.EXP = ROOT / "benchmarks/m18-8bit-low-checkpoint-review-v1"
RUNTIME.ARMS = {"8bit-medium-r1": ("Qwen3.8-27B-MLX-8bit", "medium")}
RUNTIME.SELECT_TIMEOUT = 600
RUNTIME.FINAL_TIMEOUT = 600


if __name__ == "__main__":
    raise SystemExit(RUNTIME.main())
