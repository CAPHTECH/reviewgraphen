#!/usr/bin/env python3
"""Create the checked-in compact projection of an M7 result staging tree."""

from __future__ import annotations

import argparse
import base64
import json
import re
import shutil
from pathlib import Path


def canonical(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def extract_commands(data: bytes, trial_slug: str) -> list[dict[str, object]]:
    lines = data.decode(errors="replace").splitlines()
    records: list[dict[str, object]] = []
    index = 0
    while index < len(lines):
        if lines[index] != "exec":
            index += 1
            continue
        index += 1
        block: list[str] = []
        cwd = None
        while index < len(lines):
            match = re.search(r" in (/[^ ]+)$", lines[index])
            if match:
                cwd = match.group(1)
                block.append(lines[index][: match.start()])
                index += 1
                break
            block.append(lines[index])
            index += 1
        if cwd is None:
            raise SystemExit(f"unterminated trial command transcript: {trial_slug}")
        records.append(
            {
                "schema": "reviewgraphen.benchmark.tool_command_record.v1",
                "trial_slug": trial_slug,
                "sequence": len(records) + 1,
                "cwd": cwd,
                "command": "\n".join(block),
            }
        )
    return records


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--staging", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise SystemExit(f"output already exists: {args.output}")
    args.output.mkdir(parents=True)

    files = [
        "execution-config.json",
        "summary.json",
        "collections.json",
        "trial-records.json",
        "agent-input-index.json",
    ]
    directories = [
        "manifests",
        "candidates",
        "collections",
        "raw-responses",
        "validation",
        "private",
        "adjudication/per-trial-private",
        "adjudication/per-trial-public",
        "adjudication/private",
        "adjudication/raw-responses",
        "adjudication/public/batches",
    ]
    adjudication_files = [
        "adjudication/public/items.json",
        "adjudication/public/decisions.json",
        "adjudication/public/batch-index.json",
        "adjudication/tool-commands.jsonl",
        "adjudication/execution-record.json",
    ]
    for relative in files + adjudication_files:
        source = args.staging / relative
        destination = args.output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
    for relative in directories:
        shutil.copytree(args.staging / relative, args.output / relative)
    excerpt_root = args.output / "adjudication" / "public" / "batches"
    for excerpt in excerpt_root.rglob("*.txt"):
        if "excerpts" not in excerpt.parts:
            continue
        encoded = excerpt.with_suffix(excerpt.suffix + ".b64")
        encoded.write_bytes(base64.b64encode(excerpt.read_bytes()) + b"\n")
        excerpt.unlink()
    for empty in (args.output / "validation").rglob("*"):
        if empty.is_file() and empty.stat().st_size == 0:
            empty.unlink()

    commands: list[dict[str, object]] = []
    telemetry = args.staging / "telemetry"
    for stderr_path in sorted(telemetry.glob("*.stderr.log")):
        slug = stderr_path.name.removesuffix(".stderr.log")
        trial_commands = extract_commands(stderr_path.read_bytes(), slug)
        if any(command["cwd"] != "/workspace" for command in trial_commands):
            raise SystemExit(f"trial command escaped workspace: {slug}")
        commands.extend(trial_commands)
    (args.output / "tool-commands.jsonl").write_bytes(
        b"\n".join(canonical(command) for command in commands) + b"\n"
    )
    (args.output / "package-record.json").write_bytes(
        canonical(
            {
                "schema": "reviewgraphen.benchmark.real_result_package.v1",
                "raw_responses_included": True,
                "tool_commands_included": True,
                "trial_tool_command_count": len(commands),
                "full_tool_transcripts_omitted_from_git": True,
                "full_tool_transcript_hashes_recorded": True,
                "full_adjudication_source_contexts_omitted_from_git": True,
                "exact_bounded_adjudication_batches_included": True,
                "bounded_excerpt_storage_encoding": "base64",
            }
        )
    )


if __name__ == "__main__":
    main()
