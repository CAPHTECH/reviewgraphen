#!/usr/bin/env python3
import json
import pathlib
import sys

result = pathlib.Path(sys.argv[1])
calls = []

def walk(v):
    if isinstance(v, dict):
        yield v
        for child in v.values():
            yield from walk(child)
    elif isinstance(v, list):
        for child in v:
            yield from walk(child)

for line in (result / "stream.jsonl").read_text(errors="replace").splitlines():
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        continue
    for value in walk(event):
        name = value.get("name")
        if name in {"Bash", "Read", "Write", "Edit", "Grep", "Glob"}:
            calls.append({"name": name, "input": value.get("input")})

first = calls[0] if calls else None
command = ((first or {}).get("input") or {}).get("command")
compliant = (
    (first or {}).get("name") == "Bash"
    and isinstance(command, str)
    and "reviewgraphen-context lower_compose" in command
)
record = {
    "schema": "reviewgraphen.benchmark.m11_first_tool.v1",
    "first_tool": first,
    "tool_call_count": len(calls),
    "compliant": compliant,
}
(result / "first-tool.json").write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")
raise SystemExit(0 if compliant else 1)

