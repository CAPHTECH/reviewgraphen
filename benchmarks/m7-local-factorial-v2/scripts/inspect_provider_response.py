#!/usr/bin/env python3
"""Summarize a retained Responses SSE stream without discarding the raw bytes."""

from __future__ import annotations

import argparse
import collections
import hashlib
import json
import pathlib
import zlib


def sha256(value: bytes) -> str:
    return "sha256:" + hashlib.sha256(value).hexdigest()


def decompress_capture(value: bytes) -> tuple[bytes, bool]:
    decoder = zlib.decompressobj(16 + zlib.MAX_WBITS)
    raw = decoder.decompress(value)
    if decoder.eof:
        raw += decoder.flush()
    return raw, decoder.eof


def text_fields(value: object, path: str = "") -> list[tuple[str, str]]:
    found: list[tuple[str, str]] = []
    if isinstance(value, dict):
        for key, child in value.items():
            child_path = f"{path}.{key}" if path else key
            if key in {"text", "delta", "content", "reasoning", "summary"} and isinstance(
                child, str
            ):
                found.append((child_path, child))
            else:
                found.extend(text_fields(child, child_path))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            found.extend(text_fields(child, f"{path}[{index}]"))
    return found


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("response", type=pathlib.Path)
    args = parser.parse_args()
    compressed = args.response.read_bytes()
    raw, gzip_complete = decompress_capture(compressed)
    events: collections.Counter[str] = collections.Counter()
    text_by_event: dict[str, dict[str, int]] = collections.defaultdict(
        lambda: {"occurrences": 0, "utf8_bytes": 0}
    )
    completed: dict[str, object] | None = None
    invalid_data_lines = 0
    data_lines = 0
    for line in raw.splitlines():
        if not line.startswith(b"data:"):
            continue
        payload = line[5:].strip()
        if not payload or payload == b"[DONE]":
            continue
        data_lines += 1
        try:
            event = json.loads(payload)
        except json.JSONDecodeError:
            invalid_data_lines += 1
            continue
        if not isinstance(event, dict):
            invalid_data_lines += 1
            continue
        event_type = event.get("type") if isinstance(event.get("type"), str) else "<missing>"
        events[event_type] += 1
        for field_path, value in text_fields(event):
            key = f"{event_type}:{field_path}"
            text_by_event[key]["occurrences"] += 1
            text_by_event[key]["utf8_bytes"] += len(value.encode("utf-8"))
        if event_type == "response.completed" and isinstance(event.get("response"), dict):
            completed = event["response"]

    output_items = []
    if completed is not None and isinstance(completed.get("output"), list):
        for item in completed["output"]:
            if not isinstance(item, dict):
                output_items.append({"json_type": type(item).__name__})
                continue
            content = item.get("content")
            output_items.append(
                {
                    "type": item.get("type"),
                    "role": item.get("role"),
                    "status": item.get("status"),
                    "content_types": [
                        child.get("type") if isinstance(child, dict) else type(child).__name__
                        for child in content
                    ]
                    if isinstance(content, list)
                    else None,
                    "text_fields": [
                        {
                            "path": field_path,
                            "utf8_bytes": len(value.encode("utf-8")),
                            "sha256": sha256(value.encode("utf-8")),
                        }
                        for field_path, value in text_fields(item)
                    ],
                }
            )

    print(
        json.dumps(
            {
                "schema": "reviewgraphen.benchmark.provider_response_inspection.v1",
                "artifact": args.response.name,
                "compressed_bytes": args.response.stat().st_size,
                "compressed_sha256": sha256(compressed),
                "gzip_complete": gzip_complete,
                "raw_bytes": len(raw),
                "raw_sha256": sha256(raw),
                "data_lines": data_lines,
                "invalid_data_lines": invalid_data_lines,
                "event_counts": dict(sorted(events.items())),
                "text_fields_by_event": dict(sorted(text_by_event.items())),
                "completed_response": {
                    "present": completed is not None,
                    "id": completed.get("id") if completed else None,
                    "status": completed.get("status") if completed else None,
                    "incomplete_details": completed.get("incomplete_details")
                    if completed
                    else None,
                    "error": completed.get("error") if completed else None,
                    "usage": completed.get("usage") if completed else None,
                    "output_items": output_items,
                },
            },
            indent=2,
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
