#!/usr/bin/env python3
"""Abort-not-redact forbidden-marker scan for m7-head-local-v1 judge packets.

Implements JUDGE_PROTOCOL.md section 2's blinding scan. Every file destined
for a judge input packet (00-instructions.md, 01-findings.json, sources/*)
must be checked before the packet is sent; any match raises instead of being
silently stripped, matching the discipline already used in
m7-head-issue-v1/scripts/prepare_judge_calibration.py.
"""

from __future__ import annotations

import sys
from pathlib import Path

# All-lowercase technical identifiers: exact-substring, case-sensitive,
# because these are arm/tool/path names that only ever appear in this
# exact lowercase form in packet content (paths, arm labels, the Codex
# CLI adapter name used as qwen_skill's harness -- "codex" here is a tool
# name, not a model family, and stays case-sensitive for that reason;
# see CASE_INSENSITIVE_MARKERS below for the model-family self-reference
# risk, which is a separate concern checked separately).
CASE_SENSITIVE_MARKERS: tuple[bytes, ...] = (
    b"b1_free_form",
    b"full_reviewgraphen",
    b"qwen",
    b"codex",
    b"lm-studio",
    b"local_id",
)

# Natural-language words a generator's or judge's free-text content could
# plausibly self-reference in any capitalization (title case, sentence
# case, all caps). Checked case-insensitively.
#
# claude/opus/anthropic: added when the claude_skill arm introduced a
# generator that is itself Claude opus.
#
# codex/gpt/openai: added when a codex-backend judge was introduced
# (CLAUDE_SKILL cross-validation prep, 2026-08-19), so that this scan
# protects both model lineages symmetrically -- a judge or generator
# from either family gets the same self-reference protection, not just
# whichever family happened to be added first. "codex" here is
# deliberately duplicated from CASE_SENSITIVE_MARKERS: the lowercase
# exact-match version above guards the tool-name/path-identifier sense
# (e.g. "codex-profile"), and this case-insensitive version separately
# guards "Codex" appearing as a natural-language self-reference in free
# text (e.g. a judge's own rationale/notes field), which the
# case-sensitive check alone would miss in title case or sentence case.
CASE_INSENSITIVE_MARKERS: tuple[bytes, ...] = (
    b"claude",
    b"opus",
    b"anthropic",
    b"codex",
    b"gpt",
    b"openai",
)


class ForbiddenMarkerError(RuntimeError):
    def __init__(self, path: str, marker: str) -> None:
        super().__init__(f"forbidden marker {marker!r} found in {path}")
        self.path = path
        self.marker = marker


def scan_bytes(path: str, data: bytes) -> None:
    """Raise ForbiddenMarkerError on the first match; otherwise return None."""
    for marker in CASE_SENSITIVE_MARKERS:
        if marker in data:
            raise ForbiddenMarkerError(path, marker.decode())
    lowered = data.lower()
    for marker in CASE_INSENSITIVE_MARKERS:
        if marker in lowered:
            raise ForbiddenMarkerError(path, marker.decode())


def scan_files(files: dict[str, bytes]) -> None:
    """Scan every (path, content) pair; raises on the first forbidden match."""
    for path, data in files.items():
        scan_bytes(path, data)


def scan_directory(root: Path) -> None:
    """Scan every regular file under root (used for ad hoc/manual checks)."""
    files = {
        str(path.relative_to(root)): path.read_bytes()
        for path in sorted(root.rglob("*"))
        if path.is_file()
    }
    scan_files(files)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: scan_forbidden_markers.py <directory>")
    root = Path(sys.argv[1]).resolve()
    if not root.is_dir():
        raise SystemExit(f"not a directory: {root}")
    scan_directory(root)
    print(f"ok: no forbidden marker found under {root}")


if __name__ == "__main__":
    main()
