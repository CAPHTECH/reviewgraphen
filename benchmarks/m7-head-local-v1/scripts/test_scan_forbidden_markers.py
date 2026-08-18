#!/usr/bin/env python3
"""Proves scan_forbidden_markers actually catches what it claims to.

Run directly: python3 test_scan_forbidden_markers.py
Exits non-zero and prints the first failing case on any assertion failure.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "scan_forbidden_markers", Path(__file__).resolve().parent / "scan_forbidden_markers.py"
)
assert SPEC is not None and SPEC.loader is not None
scan_forbidden_markers = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(scan_forbidden_markers)

scan_bytes = scan_forbidden_markers.scan_bytes
scan_files = scan_forbidden_markers.scan_files
ForbiddenMarkerError = scan_forbidden_markers.ForbiddenMarkerError

CASES_MUST_CATCH: list[tuple[str, bytes]] = [
    ("plain lowercase claude", b"As Claude opus I reviewed this code carefully."),
    ("lowercase claude only", b"the rationale mentions claude in passing"),
    ("title case Claude", b"Claude judges qwen's findings"),
    ("all caps CLAUDE", b"CLAUDE OPUS REVIEW"),
    ("opus lowercase embedded in sentence", b"running opus at high effort produced this"),
    ("Opus title case", b"Opus reasoned about the double-submit path"),
    ("anthropic lowercase", b"trained by anthropic on safety-relevant data"),
    ("Anthropic title case", b"Anthropic's Claude model wrote this rationale"),
    ("self-referential rationale, realistic finding text",
     b'{"rationale": "As an AI assistant made by Anthropic (Claude), I noticed a bug"}'),
    ("qwen lowercase (pre-existing marker)", b"qwen3.8:27b-mlx produced this"),
    ("codex lowercase (pre-existing marker)", b"codex cli invoked this"),
    ("lm-studio (pre-existing marker)", b"served via lm-studio"),
    ("b1_free_form (pre-existing marker)", b"packet arm=b1_free_form"),
    ("full_reviewgraphen (pre-existing marker)", b"scaffold=full_reviewgraphen"),
    ("local_id (pre-existing marker)", b'"local_id": "finding-1"'),
    ("title case Codex, natural-language self-reference", b"Codex judged this finding carefully"),
    ("all caps CODEX", b"CODEX REVIEW OUTPUT"),
    ("gpt lowercase embedded in sentence", b"running gpt at high reasoning effort produced this"),
    ("GPT title/caps", b"GPT-5.4 reasoned about the double-submit path"),
    ("openai lowercase", b"a model trained by openai on safety-relevant data"),
    ("OpenAI title case", b"OpenAI's Codex model wrote this rationale"),
    ("self-referential rationale, codex-judge realistic text",
     b'{"notes": "As Codex, built by OpenAI, I confirmed this finding via GPT reasoning"}'),
]

CASES_MUST_PASS: list[tuple[str, bytes]] = [
    ("ordinary Rust source", b"fn double_submit_guard(token: &str) -> bool { true }"),
    ("ordinary finding rationale, no self-reference",
     b'{"rationale": "The separator token collides with user input under condition X"}'),
    ("word containing a marker as a substring but not standalone... "
     "still must be treated as substring match per spec (documented, not a false case)",
     b"unrelated text with no markers at all here"),
]


def run() -> int:
    failures: list[str] = []

    for label, payload in CASES_MUST_CATCH:
        try:
            scan_bytes(f"<must-catch: {label}>", payload)
        except ForbiddenMarkerError:
            pass
        else:
            failures.append(f"FAILED TO CATCH: {label!r} -> {payload!r}")

    for label, payload in CASES_MUST_PASS:
        try:
            scan_bytes(f"<must-pass: {label}>", payload)
        except ForbiddenMarkerError as error:
            failures.append(f"FALSE POSITIVE: {label!r} -> {payload!r} ({error})")

    # scan_files must raise on the first offending file in a multi-file dict,
    # proving the packet-level entry point (not just scan_bytes) is wired.
    try:
        scan_files({
            "00-instructions.md": b"judge instructions, nothing forbidden",
            "01-findings.json": b'{"rationale": "As Claude opus, I found this issue"}',
            "sources/foo.rs": b"fn foo() {}",
        })
    except ForbiddenMarkerError as error:
        if error.path != "01-findings.json":
            failures.append(f"scan_files raised on wrong file: {error.path}")
    else:
        failures.append("scan_files failed to catch a self-referential finding rationale")

    # scan_files over an all-clean packet must not raise.
    try:
        scan_files({
            "00-instructions.md": b"judge instructions, nothing forbidden",
            "01-findings.json": b'{"rationale": "The mutex is dropped before the guard clause"}',
            "sources/foo.rs": b"fn foo() {}",
        })
    except ForbiddenMarkerError as error:
        failures.append(f"scan_files false positive on clean packet: {error}")

    if failures:
        print(f"{len(failures)} FAILURE(S):")
        for failure in failures:
            print(f"  - {failure}")
        return 1

    total = len(CASES_MUST_CATCH) + len(CASES_MUST_PASS) + 2
    print(f"ok: all {total} cases behaved as expected "
          f"({len(CASES_MUST_CATCH)} correctly caught, "
          f"{len(CASES_MUST_PASS)} correctly passed, 2 scan_files integration checks)")
    return 0


if __name__ == "__main__":
    sys.exit(run())
