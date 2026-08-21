#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import sys

result = pathlib.Path(sys.argv[1])
expected = json.loads((result / "expected.json").read_text())
calls: list[dict] = []
tool_results: list[dict] = []
assistant_models: set[str] = set()

for line in (result / "stream.jsonl").read_text(errors="replace").splitlines():
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        continue
    message = event.get("message") or {}
    if event.get("type") == "assistant":
        model = message.get("model")
        if isinstance(model, str) and model != "<synthetic>":
            assistant_models.add(model)
        for block in message.get("content") or []:
            if block.get("type") == "tool_use":
                calls.append({"id": block.get("id"), "name": block.get("name"), "input": block.get("input") or {}})
    if event.get("type") == "user":
        for block in message.get("content") or []:
            if block.get("type") == "tool_result":
                content = block.get("content")
                text = content if isinstance(content, str) else json.dumps(content)
                tool_results.append(
                    {
                        "tool_use_id": block.get("tool_use_id"),
                        "is_error": block.get("is_error"),
                        "content_chars": len(text),
                        "contains_header_nonce": expected["header_nonce"] in text,
                        "contains_trailer_nonce": expected["trailer_nonce"] in text,
                        "contains_truncation_marker": "truncated" in text.lower(),
                        "starts_with_exit_code": text.startswith("Exit code"),
                    }
                )

report = None
report_error = None
probe_path = result / "probe.json"
if probe_path.exists():
    try:
        report = json.loads(probe_path.read_text())
    except Exception as error:  # noqa: BLE001
        report_error = f"{type(error).__name__}: {error}"

expected_keys = {
    "schema", "observed_header_nonce", "observed_trailer_nonce", "projection_id",
    "recognized_as_completed_tool_result", "should_repeat_context_call",
    "result_appeared_truncated", "result_appeared_error", "summary",
}
first_result = tool_results[0] if tool_results else {}
expected_trailer = expected["trailer_nonce"] if first_result.get("contains_trailer_nonce") else None
expected_truncated = bool(first_result.get("contains_truncation_marker") or not first_result.get("contains_trailer_nonce"))
expected_error = bool(first_result.get("is_error") or first_result.get("starts_with_exit_code"))
report_valid = bool(
    isinstance(report, dict)
    and set(report) == expected_keys
    and report.get("schema") == "reviewgraphen.benchmark.projection_payload_recognition_output.v1"
    and report.get("observed_header_nonce") == expected["header_nonce"]
    and report.get("observed_trailer_nonce") == expected_trailer
    and report.get("projection_id") == expected["projection_id"]
    and report.get("recognized_as_completed_tool_result") is True
    and report.get("should_repeat_context_call") is False
    and report.get("result_appeared_truncated") is expected_truncated
    and report.get("result_appeared_error") is expected_error
    and isinstance(report.get("summary"), str)
    and bool(report["summary"].strip())
)
commands = [call["input"].get("command") for call in calls if call["name"] == "Bash"]
sequence_ok = bool(
    len(calls) == 2
    and calls[0]["name"] == "Bash"
    and calls[0]["input"].get("command") == "reviewgraphen-context-probe"
    and calls[1]["name"] == "Write"
    and str(calls[1]["input"].get("file_path", "")).endswith("probe.json")
)
record = {
    "schema": "reviewgraphen.benchmark.m14_trial_analysis.v1",
    "variant": expected["variant"],
    "payload_bytes": expected["bytes"],
    "payload_sha256": expected["sha256"],
    "tool_calls": calls,
    "tool_call_count": len(calls),
    "context_call_count": commands.count("reviewgraphen-context-probe"),
    "first_tool_result": first_result or None,
    "assistant_models": sorted(assistant_models),
    "sequence_compliant": sequence_ok,
    "report_valid": report_valid,
    "report_error": report_error,
    "success": sequence_ok and report_valid,
}
(result / "analysis.json").write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")
raise SystemExit(0 if record["success"] else 1)

