#!/usr/bin/env python3
"""Validate and attach blinded adjudication artifacts to an M7 staging tree."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import tempfile
from collections import Counter
from pathlib import Path


def canonical(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(canonical(value))


def sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def extract_commands(data: bytes, batch_id: str) -> list[dict[str, object]]:
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
            raise SystemExit(f"unterminated command transcript: {batch_id}")
        records.append(
            {
                "schema": "reviewgraphen.benchmark.tool_command_record.v1",
                "batch_id": batch_id,
                "sequence": len(records) + 1,
                "cwd": cwd,
                "command": "\n".join(block),
            }
        )
    return records


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--staging", type=Path, required=True)
    parser.add_argument("--inputs", type=Path, required=True)
    parser.add_argument("--runs", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--preparer", type=Path, required=True)
    parser.add_argument("--runner", type=Path, required=True)
    args = parser.parse_args()

    public_root = args.staging / "adjudication" / "public"
    private_root = args.staging / "adjudication" / "private"
    items = json.loads((public_root / "items.json").read_bytes())
    item_ids = {str(item["item_id"]) for item in items}
    reconciliation = json.loads((private_root / "reconciliation.json").read_bytes())
    mapping = {str(item["item_id"]): item for item in reconciliation}
    if set(mapping) != item_ids:
        raise SystemExit("public/private adjudication item mismatch")
    batch_index = json.loads((args.inputs / "batch-index.json").read_bytes())

    decisions: list[dict[str, object]] = []
    run_records: list[dict[str, object]] = []
    tool_commands: list[dict[str, object]] = []
    copied_batches = public_root / "batches"
    if copied_batches.exists():
        raise SystemExit("adjudication batches already attached")
    for batch in batch_index["batches"]:
        batch_id = str(batch["batch_id"])
        expected_ids = {str(item_id) for item_id in batch["item_ids"]}
        input_dir = args.inputs / batch_id
        run_dir = args.runs / batch_id
        status = int((run_dir / "exit-status").read_text().strip())
        if status != 0:
            raise SystemExit(f"adjudication runner failure: {batch_id}")
        raw_path = run_dir / "decisions.raw.json"
        raw = raw_path.read_bytes()
        value = json.loads(raw)
        batch_decisions = value.get("decisions") if isinstance(value, dict) else None
        if not isinstance(batch_decisions, list):
            raise SystemExit(f"invalid adjudication result: {batch_id}")
        actual_ids = [str(decision.get("item_id")) for decision in batch_decisions]
        if set(actual_ids) != expected_ids or len(actual_ids) != len(set(actual_ids)):
            raise SystemExit(f"adjudication result item mismatch: {batch_id}")
        for decision in batch_decisions:
            with tempfile.NamedTemporaryFile() as probe:
                probe.write(canonical(decision))
                probe.flush()
                checked = subprocess.run(
                    [
                        str(args.binary),
                        "validate-blind-adjudication",
                        str(private_root / "reconciliation.json"),
                        probe.name,
                    ],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.PIPE,
                    check=False,
                )
            if checked.returncode:
                raise SystemExit(
                    f"adjudication import failed: {batch_id}: {checked.stderr.decode(errors='replace')}"
                )
            decisions.append(decision)
        shutil.copytree(input_dir, copied_batches / batch_id)
        raw_output = args.staging / "adjudication" / "raw-responses" / f"{batch_id}.json"
        raw_output.parent.mkdir(parents=True, exist_ok=True)
        raw_output.write_bytes(raw)
        stderr_bytes = (run_dir / "stderr.log").read_bytes()
        events_bytes = (run_dir / "events.jsonl").read_bytes()
        commands = extract_commands(stderr_bytes, batch_id)
        if any(command["cwd"] != "/workspace" for command in commands):
            raise SystemExit(f"adjudicator command escaped workspace: {batch_id}")
        tool_commands.extend(commands)
        token_match = re.search(rb"tokens used\s*\n([0-9,]+)", stderr_bytes)
        run_records.append(
            {
                "batch_id": batch_id,
                "runner_exit_status": status,
                "raw_response_sha256": sha256(raw),
                "events_sha256": sha256(events_bytes),
                "tool_transcript_sha256": sha256(stderr_bytes),
                "tool_command_count": len(commands),
                "reported_tokens_used": (
                    int(token_match.group(1).replace(b",", b"")) if token_match else None
                ),
            }
        )

    decisions.sort(key=lambda decision: str(decision["item_id"]))
    if {str(decision["item_id"]) for decision in decisions} != item_ids or len(decisions) != len(item_ids):
        raise SystemExit("global adjudication decision mismatch")
    write_json(public_root / "decisions.json", {"decisions": decisions})
    shutil.copy2(args.inputs / "batch-index.json", public_root / "batch-index.json")
    commands_path = args.staging / "adjudication" / "tool-commands.jsonl"
    commands_path.write_bytes(b"\n".join(canonical(command) for command in tool_commands) + b"\n")
    write_json(
        args.staging / "adjudication" / "execution-record.json",
        {
            "schema": "reviewgraphen.benchmark.real_adjudication_execution.v1",
            "provider": "openai",
            "model": "gpt-5.6-sol",
            "model_revision": "unknown",
            "reasoning_effort": "high",
            "runner": "codex-cli",
            "role_blind": True,
            "candidate_rationale_withheld": True,
            "batch_count": len(run_records),
            "decision_count": len(decisions),
            "isolated_runner_sha256": sha256(args.runner.read_bytes()),
            "batch_preparer_sha256": sha256(args.preparer.read_bytes()),
            "tool_command_count": len(tool_commands),
            "outside_workspace_cwd_count": 0,
            "runs": sorted(run_records, key=lambda record: str(record["batch_id"])),
        },
    )

    scores: dict[str, dict[str, object]] = {}
    for score_path in (args.staging / "private" / "scores").glob("*.json"):
        score = json.loads(score_path.read_bytes())
        scores[str(score["trial_id"])] = score
    counts: Counter[tuple[str, str, str]] = Counter()
    for decision in decisions:
        trial_id = str(mapping[str(decision["item_id"])]["trial_id"])
        score = scores[trial_id]
        counts[(str(score["revision_role"]), str(score["arm"]), str(decision["disposition"]))] += 1
    write_json(
        private_root / "decision-summary.json",
        {
            "schema": "reviewgraphen.benchmark.real_adjudication_summary.v1",
            "decision_count": len(decisions),
            "counts": [
                {"revision_role": role, "arm": arm, "disposition": disposition, "count": count}
                for (role, arm, disposition), count in sorted(counts.items())
            ],
        },
    )


if __name__ == "__main__":
    main()
