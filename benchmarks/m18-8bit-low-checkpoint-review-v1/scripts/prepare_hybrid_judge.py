#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import pathlib
import re


ROOT = pathlib.Path("/home/rizumita/workspace/reviewgraphen")
DIAGNOSTIC = (
    ROOT
    / "benchmarks/m18-8bit-low-checkpoint-review-v1"
    / "diagnostics/hybrid-4bit-low-select-8bit-medium-final1200-r1"
)
review = json.loads((DIAGNOSTIC / "run/review.json").read_text())
findings = []
for item in review["findings"]:
    canonical = json.dumps(
        item, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode()
    finding_id = "finding:" + hashlib.sha256(canonical).hexdigest()[:32]
    locations = []
    for source_id in item["source_ids"]:
        match = re.fullmatch(r"(.+):(\d+)-(\d+)", source_id)
        if match:
            locations.append(
                {
                    "path": match.group(1),
                    "start_line": int(match.group(2)),
                    "end_line": int(match.group(3)),
                }
            )
    findings.append(
        {
            "finding_id": finding_id,
            "locations": locations,
            "mechanism_tags": [],
            "severity": item["severity"],
            "rationale": item["title"] + ". " + item["description"],
        }
    )
findings.sort(key=lambda item: item["finding_id"])
output = {
    "schema": "reviewgraphen.benchmark.m17_blind_findings.v1",
    "unit_id": "m18-hybrid-r1",
    "findings": findings,
}
judge = DIAGNOSTIC / "judge"
(judge / "01-findings.json").write_text(
    json.dumps(output, indent=2, ensure_ascii=False) + "\n"
)
print(
    json.dumps(
        {
            "finding_count": len(findings),
            "finding_ids": [item["finding_id"] for item in findings],
        }
    )
)
