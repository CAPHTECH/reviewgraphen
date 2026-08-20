#!/usr/bin/env python3
"""Record whether a trial actually invoked the frozen projection command."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path


def walk(value):
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from walk(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk(child)


def main() -> None:
    result = Path(sys.argv[1])
    commands = []
    selectors = (
        "visit_block",
        "crate::crates::reviewgraphen-ingest::src::rust::visit_block::FunctionBodyVisitor",
    )
    selector_pattern = "|".join(
        re.escape(form)
        for selector in selectors
        for form in (selector, f"'{selector}'", f'"{selector}"')
    )
    for line in (result / "stream.jsonl").read_text(errors="replace").splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        for value in walk(event):
            if value.get("name") != "Bash":
                continue
            command = value.get("input", {}).get("command")
            if isinstance(command, str) and re.search(
                rf"reviewgraphen-context\s+(?:{selector_pattern})(?:\s|$)",
                command,
            ):
                commands.append(command)
    record = {
        "schema": "reviewgraphen.benchmark.m10_projection_use.v1",
        "call_count": len(commands),
        "commands": commands,
        "used": bool(commands),
    }
    (result / "projection-use.json").write_text(
        json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
