#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import pathlib
import re

ROOT = pathlib.Path("/home/rizumita/workspace/reviewgraphen")
EXP = ROOT / "benchmarks/m16-qwen-chained-reviewgraphen-v1"
review = json.loads((EXP / "runs/8bit-high-r1/review.json").read_text())
findings = []
for item in review["findings"]:
    canonical = json.dumps(item, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
    finding_id = "finding:" + hashlib.sha256(canonical).hexdigest()[:32]
    locations = []
    for source_id in item["source_ids"]:
        match = re.fullmatch(r"(.+):(\d+)-(\d+)", source_id)
        if not match:
            continue
        locations.append({"path": match.group(1), "start_line": int(match.group(2)), "end_line": int(match.group(3))})
    findings.append(
        {
            "finding_id": finding_id,
            "locations": locations,
            "mechanism_tags": [],
            "severity": item["severity"],
            "rationale": item["title"] + ". " + item["description"],
        }
    )
output = {"schema": "reviewgraphen.benchmark.m16_blind_findings.v1", "unit_id": "m16-completed-01", "findings": findings}
(EXP / "judge/01-findings.json").write_text(json.dumps(output, indent=2, ensure_ascii=False) + "\n")

