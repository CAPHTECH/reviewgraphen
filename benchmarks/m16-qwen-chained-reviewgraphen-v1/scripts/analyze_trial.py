#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import re
import sys

result = pathlib.Path(sys.argv[1])
inventory = json.loads(pathlib.Path(sys.argv[2]).read_text())
cards = {item["card_id"]: item for item in inventory["cards"]}
expected_tokens = ["m16-next-1-7c91", "m16-next-2-b4e6", "m16-next-3-2ad8"]
calls = []
models = set()
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
            calls.append({"name": block.get("name"), "input": block.get("input") or {}, "message_id": message.get("id")})

projection_ids = []
source_ids = set()
requests = []
allowed_bash = True
tokens_valid = True
expansion_index = 0
tool_messages = []
for call in calls:
    tool_messages.append(call.get("message_id"))
    if call["name"] != "Bash":
        continue
    command = call["input"].get("command")
    if command == "reviewgraphen-context overview lower_compose" and not requests:
        requests.append("overview")
        projection_ids.append(inventory["overview"]["projection_id"])
        continue
    match = re.fullmatch(r"reviewgraphen-context expand ([a-z0-9-]+) ([A-Za-z0-9-]+)", command or "")
    if not match or match.group(1) not in cards:
        allowed_bash = False
        continue
    if expansion_index >= len(expected_tokens) or match.group(2) != expected_tokens[expansion_index]:
        tokens_valid = False
    card = cards[match.group(1)]
    requests.append(card["card_id"])
    projection_ids.append(card["projection_id"])
    source_ids.add(card["source_id"])
    expansion_index += 1

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
        grounded = grounded and bool(
            isinstance(finding, dict) and set(finding) == finding_keys
            and finding.get("severity") in {"critical", "high", "medium", "low"}
            and finding.get("evidence_status") in {"source_supported", "unverified"}
            and isinstance(finding.get("source_ids"), list) and bool(finding["source_ids"])
            and set(finding["source_ids"]).issubset(source_ids)
            and all(isinstance(finding.get(key), str) and finding[key].strip() for key in ("title", "description"))
        )
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
)
bash_calls = [call for call in calls if call["name"] == "Bash"]
write_calls = [call for call in calls if call["name"] == "Write"]
sequential_messages = len(tool_messages) == len(set(tool_messages))
protocol = bool(
    calls and calls[0]["name"] == "Bash"
    and calls[0]["input"].get("command") == "reviewgraphen-context overview lower_compose"
    and allowed_bash and tokens_valid and sequential_messages
    and len(requests) == len(set(requests)) and len(bash_calls) <= 4 and len(calls) <= 5
    and len(write_calls) == 1 and calls[-1]["name"] == "Write"
    and str(calls[-1]["input"].get("file_path", "")).endswith("review.json")
)
analysis = {
    "schema": "reviewgraphen.benchmark.m16_trial_analysis.v1",
    "tool_calls": calls,
    "tool_call_count": len(calls),
    "bash_call_count": len(bash_calls),
    "reviewgraphen_call_count": len(requests),
    "request_keys": requests,
    "projection_ids_received": projection_ids,
    "source_ids_received": sorted(source_ids),
    "unique_requests": len(requests) == len(set(requests)),
    "only_allowed_bash": allowed_bash,
    "tokens_valid": tokens_valid,
    "one_tool_per_assistant_message": sequential_messages,
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
