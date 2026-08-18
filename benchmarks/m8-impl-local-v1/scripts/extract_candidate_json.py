#!/usr/bin/env python3
"""Extract one candidate object without changing its JSON meaning."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

FENCE = re.compile(r"```(?:json)?[ \t]*\n?(.*?)```", re.IGNORECASE | re.DOTALL)


def first_object(text: str, start: int = 0) -> str | None:
    decoder = json.JSONDecoder()
    for pos in range(start, len(text)):
        if text[pos] != "{":
            continue
        try:
            value, end = decoder.raw_decode(text, pos)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict):
            return text[pos:end]
    return None


def extract(text: str) -> str:
    for match in FENCE.finditer(text):
        candidate = first_object(match.group(1))
        if candidate is not None:
            return candidate
    candidate = first_object(text)
    if candidate is None:
        raise ValueError("no well-formed JSON object found")
    return candidate


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: extract_candidate_json.py <response>")
    text = Path(sys.argv[1]).read_text(encoding="utf-8")
    sys.stdout.write(extract(text))
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
