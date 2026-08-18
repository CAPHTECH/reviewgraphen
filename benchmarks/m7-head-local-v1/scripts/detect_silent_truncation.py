#!/usr/bin/env python3
"""Detects `upstream_silent_truncation`: an upstream response that reports
`status: "completed"` with no error and no `incomplete_details`, yet never
produced a `message`-type output item — i.e., the server claims success
while the model's turn never reached a final answer.

Distinct from every other upstream failure class already in this
experiment's taxonomy (upstream_server_stream_incomplete,
upstream_model_crash, upstream_stream_closed_before_completion): those all
report failure explicitly (a 5xx, a disconnect, a crash message). This is
the only one that reports success. See
diagnostics/empty-final-content-investigation-head-local-04-08/REPORT.md
for the investigation that established this class, and
RESTART_PROTOCOL_AND_UPSTREAM_HISTORY.md section 6 for the amendment that
introduced it.

Three necessary conditions, all checked directly from the response.completed
event in the raw SSE stream (not inferred from adapter.stderr, which is
ambiguous — see the process.rs `raw response size` known-defect note):

1. response.completed: status == "completed", error is null,
   incomplete_details is null (the server claims a clean finish).
2. response.completed.output contains no item of type "message" (the
   model never produced any final-answer content, structured or not —
   not even an explicit `outcome: abstained` JSON, which would still
   require a message item to carry the text).
3. usage.output_tokens is well under max_output_tokens (SILENT_TRUNCATION_
   MARGIN, an operator-chosen conservative threshold, not a discovered
   constant — default 0.95, i.e. "not visibly exhausting the budget").
   This rules out genuine budget exhaustion masquerading as the same
   shape; a response that legitimately ran out of room would be expected
   to sit close to the cap.
"""

from __future__ import annotations

import gzip
import json
import sys
from pathlib import Path

SILENT_TRUNCATION_MARGIN = 0.95


def find_response_completed(sse_path: Path) -> dict | None:
    opener = gzip.open if sse_path.suffix == ".gz" else open
    with opener(sse_path, "rt", encoding="utf-8") as handle:
        for line in handle:
            if not line.startswith("data: "):
                continue
            try:
                event = json.loads(line[len("data: "):])
            except json.JSONDecodeError:
                continue
            if event.get("type") == "response.completed":
                return event.get("response")
    return None


def is_silent_truncation(sse_path: Path, margin: float = SILENT_TRUNCATION_MARGIN) -> bool:
    response = find_response_completed(sse_path)
    if response is None:
        return False
    if response.get("status") != "completed":
        return False
    if response.get("error") is not None:
        return False
    if response.get("incomplete_details") is not None:
        return False
    output_types = {item.get("type") for item in response.get("output", [])}
    if "message" in output_types:
        return False
    usage = response.get("usage") or {}
    output_tokens = usage.get("output_tokens")
    max_output_tokens = response.get("max_output_tokens")
    if not isinstance(output_tokens, int) or not isinstance(max_output_tokens, int) or max_output_tokens <= 0:
        return False
    if output_tokens >= margin * max_output_tokens:
        return False
    return True


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: detect_silent_truncation.py <provider-response.sse[.gz]>")
    path = Path(sys.argv[1])
    if not path.is_file():
        raise SystemExit(f"not a file: {path}")
    result = is_silent_truncation(path)
    print("true" if result else "false")
    sys.exit(0 if result else 1)


if __name__ == "__main__":
    main()
