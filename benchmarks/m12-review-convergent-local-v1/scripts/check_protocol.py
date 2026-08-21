#!/usr/bin/env python3
from __future__ import annotations

import json
import pathlib
import sys


result = pathlib.Path(sys.argv[1])
all_calls: list[dict] = []


def walk(value):
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from walk(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk(child)


for line in (result / "stream.jsonl").read_text(errors="replace").splitlines():
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        continue
    for value in walk(event):
        name = value.get("name")
        if value.get("type") == "tool_use" and isinstance(name, str):
            all_calls.append({"name": name, "input": value.get("input") or {}})

review_tool_names = {"Bash", "Read", "Write", "Edit", "Grep", "Glob"}
calls = [call for call in all_calls if call["name"] in review_tool_names]
first = all_calls[0] if all_calls else None
first_command = (first or {}).get("input", {}).get("command")
first_ok = (first or {}).get("name") == "Bash" and first_command == "reviewgraphen-context lower_compose"
write_indexes = [
    index
    for index, call in enumerate(calls, start=1)
    if call["name"] == "Write"
    and (
        call["input"].get("file_path") == "review.json"
        or call["input"].get("file_path", "").endswith("/review.json")
    )
]
checkpoint_ok = bool(write_indexes) and min(write_indexes) <= 5
count_ok = len(calls) <= 8
record = {
    "schema": "reviewgraphen.benchmark.m12_protocol_check.v1",
    "first_tool": first,
    "first_tool_compliant": first_ok,
    "tool_call_count": len(calls),
    "all_tool_call_count": len(all_calls),
    "skill_tool_call_count": sum(call["name"] == "Skill" for call in all_calls),
    "tool_count_compliant": count_ok,
    "review_write_tool_indexes": write_indexes,
    "checkpoint_compliant": checkpoint_ok,
    "compliant": first_ok and count_ok and checkpoint_ok,
    "note": "The maximum-three-hypothesis and no-new-hypothesis-after-checkpoint rules are prompt constraints and require stream-content review; this checker does not promote them to deterministic facts.",
}
(result / "protocol-check.json").write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")
raise SystemExit(0 if record["compliant"] else 1)
