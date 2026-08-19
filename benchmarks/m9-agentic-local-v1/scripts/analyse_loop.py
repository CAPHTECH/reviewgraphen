#!/usr/bin/env python3
"""Reconstructs what the agentic loop actually did, from the stream-json log.

This is the practical-utility evidence the single-shot harness could not
produce: how many turns, how many tool calls, how often it invoked cargo,
and -- the question that motivated this experiment -- **whether it read a
compiler error and then edited in response to it.**

`saw_error_then_edited` is counted as: a Bash result containing a rustc
diagnostic (`error[E….]` or `error:` from cargo), followed later in the
stream by an Edit/Write to the target file. That is an ordering fact from
the transcript, not an inference about intent, and it is reported as such.

usage: analyse_loop.py <result-dir>
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

TARGET_FILE = "crates/reviewgraphen-ingest/src/rust.rs"
ERROR_PATTERN = re.compile(r"error\[E\d{4}\]|^error(:|\[)", re.M)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: analyse_loop.py <result-dir>")
    result = Path(sys.argv[1])
    stream = result / "stream.jsonl"

    events = []
    for line in stream.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue

    report: dict = {
        "schema": "reviewgraphen.benchmark.m9_loop_behaviour.v1",
        "trial": result.name,
        "stream_events": len(events),
    }

    assistant_turns = 0
    tool_calls: dict[str, int] = {}
    bash_commands: list[str] = []
    edits_to_target = 0
    timeline: list[str] = []
    usage_total = {"input_tokens": 0, "output_tokens": 0}
    api_errors: list[str] = []
    model_ids: set[str] = set()

    for event in events:
        kind = event.get("type")
        if kind == "assistant":
            message = event.get("message") or {}
            assistant_turns += 1
            if message.get("model"):
                model_ids.add(message["model"])
            usage = message.get("usage") or {}
            usage_total["input_tokens"] += usage.get("input_tokens") or 0
            usage_total["output_tokens"] += usage.get("output_tokens") or 0
            for block in message.get("content") or []:
                if block.get("type") != "tool_use":
                    continue
                name = block.get("name", "unknown")
                tool_calls[name] = tool_calls.get(name, 0) + 1
                payload = block.get("input") or {}
                if name == "Bash":
                    command = str(payload.get("command", ""))
                    bash_commands.append(command)
                    if "cargo" in command:
                        timeline.append("cargo")
                if name in {"Edit", "Write", "NotebookEdit"}:
                    if TARGET_FILE in str(payload.get("file_path", "")):
                        edits_to_target += 1
                        timeline.append("edit_target")
        elif kind == "user":
            message = event.get("message") or {}
            for block in message.get("content") or []:
                if block.get("type") != "tool_result":
                    continue
                text = json.dumps(block.get("content"))
                if ERROR_PATTERN.search(text):
                    timeline.append("compiler_error_seen")
        elif kind == "result":
            report["result_subtype"] = event.get("subtype")
            report["result_is_error"] = event.get("is_error")
            report["num_turns_reported"] = event.get("num_turns")
            report["duration_ms"] = event.get("duration_ms")
            report["stop_reason"] = event.get("stop_reason")
            report["api_error_status"] = event.get("api_error_status")
            report["final_text"] = (event.get("result") or "")[:2000]
        if event.get("is_error") and kind not in {"result"}:
            api_errors.append(str(event.get("subtype") or kind))

    cargo_calls = sum(1 for command in bash_commands if "cargo" in command)
    report.update(
        {
            "assistant_turns": assistant_turns,
            "tool_calls": dict(sorted(tool_calls.items())),
            "tool_calls_total": sum(tool_calls.values()),
            "bash_calls": len(bash_commands),
            "cargo_invocations": cargo_calls,
            "cargo_build_invocations": sum(
                1 for c in bash_commands if "cargo" in c and "build" in c
            ),
            "cargo_test_invocations": sum(
                1 for c in bash_commands if "cargo" in c and "test" in c
            ),
            "edits_to_target_file": edits_to_target,
            "model_ids_seen": sorted(model_ids),
            "reported_usage_totals": usage_total,
            "api_errors": api_errors,
        }
    )

    # Ordering fact: was a compiler error observed before a later edit?
    saw_error_then_edited = False
    seen_error = False
    error_then_edit_pairs = 0
    for step in timeline:
        if step == "compiler_error_seen":
            seen_error = True
        elif step == "edit_target" and seen_error:
            saw_error_then_edited = True
            error_then_edit_pairs += 1
            seen_error = False
    report["compiler_errors_observed"] = timeline.count("compiler_error_seen")
    report["saw_error_then_edited"] = saw_error_then_edited
    report["error_then_edit_pairs"] = error_then_edit_pairs
    report["timeline"] = timeline

    (result / "loop-behaviour.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    compact = {
        key: value for key, value in report.items() if key not in {"timeline", "final_text"}
    }
    print(json.dumps(compact, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
