#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import sys

EXPECTED_NONCE = "m13-rg-projection-receipt-7f91c2"
EXPECTED_PROJECTION = "benchmark-target-context:sha256:82272361a1f20d32bb4580f27626994fec18549ab6b1c78db5bcada984778177"

result = pathlib.Path(sys.argv[1])
calls: list[dict] = []
assistant_models: set[str] = set()

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
        assistant_models.add(model)
    for block in message.get("content") or []:
        if block.get("type") == "tool_use":
            calls.append({"name": block.get("name"), "input": block.get("input") or {}})

report = None
report_error = None
probe_path = result / "probe.json"
if probe_path.exists():
    try:
        report = json.loads(probe_path.read_text())
    except Exception as error:  # noqa: BLE001
        report_error = f"{type(error).__name__}: {error}"

expected_keys = {
    "schema",
    "observed_nonce",
    "projection_id",
    "recognized_as_completed_tool_result",
    "should_repeat_context_call",
    "summary",
}
report_valid = bool(
    isinstance(report, dict)
    and set(report) == expected_keys
    and report.get("schema") == "reviewgraphen.benchmark.projection_recognition_output.v1"
    and report.get("observed_nonce") == EXPECTED_NONCE
    and report.get("projection_id") == EXPECTED_PROJECTION
    and report.get("recognized_as_completed_tool_result") is True
    and report.get("should_repeat_context_call") is False
    and isinstance(report.get("summary"), str)
    and bool(report["summary"].strip())
)

commands = [call["input"].get("command") for call in calls if call["name"] == "Bash"]
write_paths = [call["input"].get("file_path") for call in calls if call["name"] == "Write"]
sequence_ok = (
    len(calls) == 2
    and calls[0]["name"] == "Bash"
    and calls[0]["input"].get("command") == "reviewgraphen-context-probe"
    and calls[1]["name"] == "Write"
    and (calls[1]["input"].get("file_path") == "probe.json" or calls[1]["input"].get("file_path", "").endswith("/probe.json"))
)

record = {
    "schema": "reviewgraphen.benchmark.m13_trial_analysis.v1",
    "tool_calls": calls,
    "tool_call_count": len(calls),
    "context_call_count": commands.count("reviewgraphen-context-probe"),
    "write_paths": write_paths,
    "assistant_models": sorted(assistant_models),
    "sequence_compliant": sequence_ok,
    "report_valid": report_valid,
    "report_error": report_error,
    "success": sequence_ok and report_valid,
}
(result / "analysis.json").write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")
raise SystemExit(0 if record["success"] else 1)
