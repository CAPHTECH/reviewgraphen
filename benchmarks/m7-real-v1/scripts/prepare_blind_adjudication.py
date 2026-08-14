#!/usr/bin/env python3
"""Build role-blind, excerpt-bounded adjudication batches."""

from __future__ import annotations

import argparse
import json
import shutil
from pathlib import Path


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    )


def clusters(items: list[dict[str, object]]) -> list[list[dict[str, object]]]:
    rows: list[tuple[str, int, int, str, dict[str, object]]] = []
    for item in items:
        first = item["finding"]["locations"][0]  # type: ignore[index]
        rows.append(
            (
                str(first["path"]),
                int(first["start_line"]),
                int(first["end_line"]),
                str(item["item_id"]),
                item,
            )
        )
    result: list[list[dict[str, object]]] = []
    current: list[dict[str, object]] = []
    current_path = ""
    current_end = -1
    for path, start, end, _, item in sorted(rows):
        if current and (path != current_path or start > current_end + 20):
            result.append(current)
            current = []
        current.append(item)
        current_path = path
        current_end = max(current_end, end) if len(current) > 1 else end
    if current:
        result.append(current)
    return result


def pack(grouped: list[list[dict[str, object]]], target: int) -> list[list[dict[str, object]]]:
    batches: list[list[dict[str, object]]] = []
    current: list[dict[str, object]] = []
    for group in grouped:
        if current and len(current) + len(group) > target:
            batches.append(current)
            current = []
        current.extend(group)
    if current:
        batches.append(current)
    return batches


def merged_windows(locations: list[dict[str, object]], total: int, margin: int) -> list[tuple[int, int]]:
    windows = sorted(
        (max(1, int(location["start_line"]) - margin), min(total, int(location["end_line"]) + margin))
        for location in locations
    )
    merged: list[tuple[int, int]] = []
    for start, end in windows:
        if merged and start <= merged[-1][1] + 1:
            merged[-1] = (merged[-1][0], max(merged[-1][1], end))
        else:
            merged.append((start, end))
    return merged


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--finalized", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--batch-size", type=int, default=18)
    parser.add_argument("--context-lines", type=int, default=60)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit(f"output already exists: {args.output}")
    if args.batch_size < 1 or args.context_lines < 0:
        raise SystemExit("invalid batching bounds")
    items_path = args.finalized / "adjudication" / "public" / "items.json"
    items = json.loads(items_path.read_text())
    batches = pack(clusters(items), args.batch_size)
    assigned: list[str] = []
    for number, batch in enumerate(batches, 1):
        batch_dir = args.output / f"batch-{number:02d}"
        write_json(batch_dir / "items.json", batch)
        item_ids = [str(item["item_id"]) for item in batch]
        assigned.extend(item_ids)
        decision = {
            "type": "object",
            "additionalProperties": False,
            "required": ["schema", "item_id", "disposition", "rationale"],
            "properties": {
                "schema": {
                    "type": "string",
                    "const": "reviewgraphen.benchmark.blind_adjudication.v1",
                },
                "item_id": {"type": "string", "enum": item_ids},
                "disposition": {
                    "type": "string",
                    "enum": ["valid_novel_defect", "duplicate", "false_positive", "insufficient"]
                },
                "rationale": {"type": "string", "minLength": 1, "maxLength": 16384},
            },
        }
        schema = {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": False,
            "required": ["decisions"],
            "properties": {
                "decisions": {
                    "type": "array",
                    "minItems": len(batch),
                    "maxItems": len(batch),
                    "items": decision,
                }
            },
        }
        write_json(batch_dir / "decision-output.schema.json", schema)
        (batch_dir / "instructions.txt").write_text(
            "Independently adjudicate every item using only the supplied role-blind data.\n"
            "The mechanism tags state the alleged failure classes; candidate prose was intentionally withheld.\n"
            "Use valid_novel_defect only when the excerpts alone support a concrete defect.\n"
            "Use false_positive when the alleged mechanisms are contradicted by the code, and insufficient when omitted context prevents a reliable decision.\n"
            "For materially identical items in this batch, keep the lexicographically smallest item_id as the substantive decision and mark the others duplicate.\n"
            "Never use matches_known_root. Do not infer or mention an arm, revision role, oracle, model, trial, commit, issue, or branch.\n"
            "Return exactly one decision per item inside the decisions array, and only the JSON object required by decision-output.schema.json.\n"
        )
        for item in batch:
            item_id = str(item["item_id"])
            digest = item_id.removeprefix("adj:sha256:")
            public_context = (
                args.finalized / "adjudication" / "public" / "contexts" / digest
            )
            item_dir = batch_dir / "contexts" / digest
            write_json(item_dir / "item.json", item)
            by_path: dict[str, list[dict[str, object]]] = {}
            for location in item["finding"]["locations"]:  # type: ignore[index]
                by_path.setdefault(str(location["path"]), []).append(location)
            for source_path, locations in sorted(by_path.items()):
                source = public_context / "source" / source_path
                lines = source.read_text().splitlines()
                output: list[str] = []
                for start, end in merged_windows(locations, len(lines), args.context_lines):
                    output.append(f"--- {source_path}:{start}-{end} ---")
                    output.extend(
                        f"{line_number:06d}  {lines[line_number - 1]}"
                        for line_number in range(start, end + 1)
                    )
                destination = item_dir / "excerpts" / f"{source_path}.txt"
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_text("\n".join(output) + "\n")
    expected = sorted(str(item["item_id"]) for item in items)
    if sorted(assigned) != expected or len(assigned) != len(set(assigned)):
        raise SystemExit("adjudication batch assignment mismatch")
    write_json(
        args.output / "batch-index.json",
        {
            "schema": "reviewgraphen.benchmark.real_adjudication_batch_index.v1",
            "item_count": len(items),
            "batch_count": len(batches),
            "target_batch_size": args.batch_size,
            "context_lines": args.context_lines,
            "batches": [
                {
                    "batch_id": f"batch-{number:02d}",
                    "item_ids": [str(item["item_id"]) for item in batch],
                }
                for number, batch in enumerate(batches, 1)
            ],
        },
    )


if __name__ == "__main__":
    main()
