#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import re
import sys

result = pathlib.Path(sys.argv[1])
inventory = json.loads(pathlib.Path(sys.argv[2]).read_text())
card_by_id = {item["card_id"]: item for item in inventory["cards"]}
calls: list[dict] = []
models: set[str] = set()
for line in (result / "stream.jsonl").read_text(errors="replace").splitlines():
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        continue
    if event.get("type") != "assistant":
        continue
    message = event.get("message") or {}
    model = message.get("model")
    if isinstance(model, str) and model != "<synthetic>":
        models.add(model)
    for block in message.get("content") or []:
        if block.get("type") == "tool_use":
            calls.append({"name": block.get("name"), "input": block.get("input") or {}})

projection_ids = []
source_ids = set()
request_keys = []
allowed_bash = True
for call in calls:
    if call["name"] != "Bash":
        continue
    command = call["input"].get("command")
    if command == "reviewgraphen-context overview lower_compose":
        request_keys.append("overview")
        projection_ids.append(inventory["overview"]["projection_id"])
    else:
        match = re.fullmatch(r"reviewgraphen-context expand ([a-z0-9-]+)", command or "")
        if not match or match.group(1) not in card_by_id:
            allowed_bash = False
            continue
        card = card_by_id[match.group(1)]
        request_keys.append(card["card_id"])
        projection_ids.append(card["projection_id"])
        source_ids.add(card["source_id"])

report = None
report_error = None
if (result / "review.json").exists():
    try:
        report = json.loads((result / "review.json").read_text())
    except Exception as error:  # noqa: BLE001
        report_error = f"{type(error).__name__}: {error}"

finding_keys = {"title", "severity", "description", "source_ids", "evidence_status"}
report_keys = {"schema", "summary", "projection_ids", "findings", "abstentions", "stopped_reason", "information_loss"}
findings_valid = isinstance(report, dict) and isinstance(report.get("findings"), list) and len(report["findings"]) <= 3
grounded = True
if findings_valid:
    for finding in report["findings"]:
        valid = (
            isinstance(finding, dict) and set(finding) == finding_keys
            and finding.get("severity") in {"critical", "high", "medium", "low"}
            and finding.get("evidence_status") in {"source_supported", "unverified"}
            and isinstance(finding.get("source_ids"), list) and bool(finding["source_ids"])
            and set(finding["source_ids"]).issubset(source_ids)
            and all(isinstance(finding.get(key), str) and finding[key].strip() for key in ("title", "description"))
        )
        grounded = grounded and valid
else:
    grounded = False

report_valid = bool(
    isinstance(report, dict) and set(report) == report_keys
    and report.get("schema") == "reviewgraphen.benchmark.intelligent_review_output.v1"
    and isinstance(report.get("summary"), str) and bool(report["summary"].strip())
    and report.get("projection_ids") == projection_ids
    and findings_valid and grounded
    and isinstance(report.get("abstentions"), list) and all(isinstance(item, str) for item in report["abstentions"])
    and report.get("stopped_reason") in {"sufficient_context", "budget_exhausted", "no_actionable_finding"}
    and isinstance(report.get("information_loss"), list) and bool(report["information_loss"])
    and all(isinstance(item, str) and item.strip() for item in report["information_loss"])
)
bash_calls = [call for call in calls if call["name"] == "Bash"]
write_calls = [call for call in calls if call["name"] == "Write"]
protocol = bool(
    calls and calls[0]["name"] == "Bash"
    and calls[0]["input"].get("command") == "reviewgraphen-context overview lower_compose"
    and allowed_bash
    and len(request_keys) == len(set(request_keys))
    and len(bash_calls) <= 6
    and len(calls) <= 7
    and len(write_calls) == 1
    and calls[-1]["name"] == "Write"
    and str(calls[-1]["input"].get("file_path", "")).endswith("review.json")
)
analysis = {
    "schema": "reviewgraphen.benchmark.m15_trial_analysis.v1",
    "tool_calls": calls,
    "tool_call_count": len(calls),
    "bash_call_count": len(bash_calls),
    "reviewgraphen_call_count": len(request_keys),
    "request_keys": request_keys,
    "projection_ids_received": projection_ids,
    "source_ids_received": sorted(source_ids),
    "unique_requests": len(request_keys) == len(set(request_keys)),
    "only_allowed_bash": allowed_bash,
    "protocol_compliant": protocol,
    "report_valid": report_valid,
    "findings_grounded_in_received_sources": grounded,
    "finding_count": len(report.get("findings", [])) if isinstance(report, dict) else 0,
    "assistant_models": sorted(models),
    "report_error": report_error,
    "success": protocol and report_valid,
}
(result / "analysis.json").write_text(json.dumps(analysis, indent=2, sort_keys=True) + "\n")
raise SystemExit(0 if analysis["success"] else 1)
