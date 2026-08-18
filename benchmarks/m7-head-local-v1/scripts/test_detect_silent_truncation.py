#!/usr/bin/env python3
"""Proves detect_silent_truncation classifies real recorded responses
correctly: the two units the operator confirmed as silent truncation
(head-local-04, head-local-08, both from m7-head-local-v1-skill-gen-3)
must classify True; a genuinely valid unit from the same batch
(head-local-07) must classify False.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

FIXTURES = Path(__file__).resolve().parent / "fixtures" / "silent-truncation-detection"

SPEC = importlib.util.spec_from_file_location(
    "detect_silent_truncation", Path(__file__).resolve().parent / "detect_silent_truncation.py"
)
assert SPEC is not None and SPEC.loader is not None
detect_silent_truncation = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(detect_silent_truncation)

is_silent_truncation = detect_silent_truncation.is_silent_truncation

CASES = [
    ("head-local-04-positive.sse.gz", True),
    ("head-local-08-positive.sse.gz", True),
    ("head-local-07-negative.sse.gz", False),
]


def run() -> int:
    failures = []
    for filename, expected in CASES:
        path = FIXTURES / filename
        if not path.is_file():
            failures.append(f"MISSING FIXTURE: {path}")
            continue
        actual = is_silent_truncation(path)
        if actual != expected:
            failures.append(f"{filename}: expected {expected}, got {actual}")
        else:
            print(f"ok: {filename} -> {actual} (expected {expected})")
    if failures:
        print(f"{len(failures)} FAILURE(S):")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print(f"ok: all {len(CASES)} fixture cases classified correctly")
    return 0


if __name__ == "__main__":
    sys.exit(run())
