#!/usr/bin/env python3
"""Reconstructs what the agentic loop actually did, from the stream-json log.

Every field carries a `provenance` entry saying whether it is counted from
actual stream EVENTS (trustworthy) or from backend-reported USAGE metadata
(suspect on this backend). That distinction was added after two measurement
faults were found in the first version's output, both established from the
retained stream rather than by reasoning:

FAULT A -- `cargo_invocations` counted the SUBSTRING "cargo".
    The agent's commands referenced `~/.cargo/registry/src/.../syn-2.0.119`
    while reading syn's source. All 8 counted "cargo invocations" in
    skill-1 were that path. The real count was ZERO: it never compiled.
    Fix: match `cargo` as a command word after stripping path-like
    `.cargo` occurrences.

FAULT B -- token totals were summed from per-turn `usage`.
    This backend sends exactly `{"input_tokens": N, "output_tokens": 0}`
    per turn. `output_tokens` is PRESENT AND ZERO -- not absent, not lost
    in aggregation -- and there is no separate reasoning key. Summing it
    gave 0 while 45 assistant events and 13 tool calls plainly required
    output. Worse, `input_tokens` is cumulative context re-reported every
    turn, and several stream events share one API call's usage, so summing
    it triple-counted: 3,818,353 against the result event's authoritative
    1,269,205.
    Fix: token totals come from the terminal `result` event, which Claude
    Code computes itself (`output_tokens` 20,264 for skill-1). Per-turn
    stream usage is retained separately, clearly labelled, as evidence of
    what the backend does and does not report.

Also recorded distinctly: `assistant_stream_events` (raw event count) and
`logical_turns` (the client's own `num_turns`). One logical turn can emit
several assistant events, so the two differ -- 45 against 23 in skill-1.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

TARGET_FILE = "crates/reviewgraphen-ingest/src/rust.rs"
ERROR_PATTERN = re.compile(r"error\[E\d{4}\]|^error(:|\[)", re.M)
# `cargo` as a command word, not as part of a path such as ~/.cargo/registry.
CARGO_COMMAND = re.compile(r"(?:^|[;&|(]|\s)cargo\s", re.M)
CARGO_PATH = re.compile(r"[\w./~-]*\.cargo[\w./-]*")


def invokes_cargo(command: str) -> bool:
    return bool(CARGO_COMMAND.search(CARGO_PATH.sub(" ", command)))


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: analyse_loop.py <result-dir>")
    result = Path(sys.argv[1])
    stream = result / "stream.jsonl"
    if not stream.exists():
        gz = result / "stream.jsonl.gz"
        if gz.exists():
            import gzip

            text = gzip.open(gz, "rt", encoding="utf-8", errors="replace").read()
        else:
            raise SystemExit(f"no stream in {result}")
    else:
        text = stream.read_text(encoding="utf-8", errors="replace")

    events = []
    for line in text.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            continue

    report: dict = {
        "schema": "reviewgraphen.benchmark.m10_loop_behaviour.v1",
        "trial": result.name,
        "stream_events": len(events),
    }

    assistant_events = 0
    tool_calls: dict[str, int] = {}
    bash_commands: list[str] = []
    edits_to_target = 0
    timeline: list[str] = []
    per_turn_usage: list[dict] = []
    model_ids: set[str] = set()
    result_event: dict | None = None

    for event in events:
        kind = event.get("type")
        if kind == "assistant":
            message = event.get("message") or {}
            assistant_events += 1
            if message.get("model"):
                model_ids.add(message["model"])
            usage = message.get("usage")
            if usage is not None:
                per_turn_usage.append(usage)
            for block in message.get("content") or []:
                if block.get("type") != "tool_use":
                    continue
                name = block.get("name", "unknown")
                tool_calls[name] = tool_calls.get(name, 0) + 1
                payload = block.get("input") or {}
                if name == "Bash":
                    command = str(payload.get("command", ""))
                    bash_commands.append(command)
                    if invokes_cargo(command):
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
                if ERROR_PATTERN.search(json.dumps(block.get("content"))):
                    timeline.append("compiler_error_seen")
        elif kind == "result":
            result_event = event

    cargo_calls = [c for c in bash_commands if invokes_cargo(c)]
    report["from_stream_events"] = {
        "provenance": "counted from tool_use / tool_result blocks in the transcript -- trustworthy",
        "assistant_stream_events": assistant_events,
        "tool_calls": dict(sorted(tool_calls.items())),
        "tool_calls_total": sum(tool_calls.values()),
        "bash_calls": len(bash_commands),
        "cargo_invocations": len(cargo_calls),
        "cargo_build_invocations": sum(1 for c in cargo_calls if " build" in c),
        "cargo_test_invocations": sum(1 for c in cargo_calls if " test" in c),
        "cargo_check_invocations": sum(1 for c in cargo_calls if " check" in c),
        "bash_commands_mentioning_cargo_path_only": sum(
            1 for c in bash_commands if "cargo" in c and not invokes_cargo(c)
        ),
        "edits_to_target_file": edits_to_target,
        "compiler_errors_observed": timeline.count("compiler_error_seen"),
        "model_ids_seen": sorted(model_ids),
    }

    # Ordering fact, from events.
    saw_error_then_edited = False
    seen_error = False
    pairs = 0
    for step in timeline:
        if step == "compiler_error_seen":
            seen_error = True
        elif step == "edit_target" and seen_error:
            saw_error_then_edited = True
            pairs += 1
            seen_error = False
    report["from_stream_events"]["saw_error_then_edited"] = saw_error_then_edited
    report["from_stream_events"]["error_then_edit_pairs"] = pairs

    authoritative = (result_event or {}).get("usage") or {}
    model_usage = (result_event or {}).get("modelUsage") or {}
    report["tokens_authoritative"] = {
        "provenance": "the client's terminal `result` event, which Claude Code computes itself -- the only trustworthy token source on this backend",
        "input_tokens": authoritative.get("input_tokens"),
        "output_tokens": authoritative.get("output_tokens"),
        "cache_read_input_tokens": authoritative.get("cache_read_input_tokens"),
        "cache_creation_input_tokens": authoritative.get("cache_creation_input_tokens"),
        "thinking_tokens": (authoritative.get("output_tokens_details") or {}).get(
            "thinking_tokens"
        ),
        "model_usage": model_usage,
    }

    inputs = [u.get("input_tokens") for u in per_turn_usage if u.get("input_tokens") is not None]
    outputs = [u.get("output_tokens") for u in per_turn_usage if "output_tokens" in u]
    report["tokens_backend_per_turn"] = {
        "provenance": "raw per-turn `usage` from the backend -- SUSPECT, retained as evidence of what it reports",
        "keys_ever_seen": sorted({k for u in per_turn_usage for k in u}),
        "turns_with_usage": len(per_turn_usage),
        "output_tokens_all_zero": bool(outputs) and all(o == 0 for o in outputs),
        "input_tokens_min": min(inputs) if inputs else None,
        "input_tokens_max": max(inputs) if inputs else None,
        "input_tokens_last": inputs[-1] if inputs else None,
        "input_tokens_monotonic_non_decreasing": all(
            inputs[i] <= inputs[i + 1] for i in range(len(inputs) - 1)
        )
        if inputs
        else None,
        "input_tokens_naive_sum_DO_NOT_USE": sum(inputs) if inputs else None,
        "why_the_naive_sum_is_wrong": "each turn's input_tokens re-reports the whole accumulated context, and several stream events share one API call's usage, so summing multiply-counts the same prefix",
    }

    if result_event is not None:
        report["result_event"] = {
            "subtype": result_event.get("subtype"),
            "is_error": result_event.get("is_error"),
            "logical_turns": result_event.get("num_turns"),
            "duration_ms": result_event.get("duration_ms"),
            "stop_reason": result_event.get("stop_reason"),
            "api_error_status": result_event.get("api_error_status"),
            "final_text": (result_event.get("result") or "")[:2000],
        }

    report["timeline"] = timeline
    (result / "loop-behaviour.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    compact = {k: v for k, v in report.items() if k not in {"timeline"}}
    if "result_event" in compact:
        compact["result_event"] = {
            k: v for k, v in compact["result_event"].items() if k != "final_text"
        }
    print(json.dumps(compact, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
