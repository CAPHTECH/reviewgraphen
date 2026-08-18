#!/usr/bin/env python3
"""Writes a reference candidate (a known-correct solution) in the candidate
output format, so the harness itself can be validated end to end before any
server time is spent.

This is a harness self-test, NOT an arm and NOT a comparator. It proves three
things and only these three: the task is solvable within the stated
constraints, the edit-application path works, and a correct edit really does
turn the red acceptance test green.

usage: make_reference_candidate.py <fresh-result-dir>
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

OLD = """        [
            command,
            request_flag,
            request_path,
            artifacts_flag,
            artifact_root,
        ] if command == "review"
            && request_flag == "--request"
            && artifacts_flag == "--artifacts" =>
        {
            generic_review(Path::new(request_path), Path::new(artifact_root))
        }
"""

NEW = """        [command, first_flag, first_value, second_flag, second_value]
            if command == "review"
                && ((first_flag == "--request" && second_flag == "--artifacts")
                    || (first_flag == "--artifacts" && second_flag == "--request")) =>
        {
            let (request_path, artifact_root) = if first_flag == "--request" {
                (first_value, second_value)
            } else {
                (second_value, first_value)
            };
            generic_review(Path::new(request_path), Path::new(artifact_root))
        }
"""

CANDIDATE = {
    "schema": "reviewgraphen.benchmark.implementation_candidate_output.v1",
    "outcome": "completed",
    "edits": [
        {"file": "crates/reviewgraphen-cli/src/lib.rs", "old": OLD, "new": NEW}
    ],
    "limitations": ["harness self-test; not an experimental arm"],
}


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: make_reference_candidate.py <fresh-result-dir>")
    result_dir = Path(sys.argv[1])
    result_dir.mkdir(parents=True, exist_ok=True)
    (result_dir / "final-content.txt").write_text(
        "```json\n" + json.dumps(CANDIDATE, indent=2) + "\n```\n", encoding="utf-8"
    )
    print(result_dir)


if __name__ == "__main__":
    main()
