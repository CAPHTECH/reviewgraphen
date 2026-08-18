#!/usr/bin/env python3
"""Abort-not-redact forbidden-marker scan for m8-impl-local-v1 judge packets.

Derived from m7-head-local-v1/scripts/scan_forbidden_markers.py, with the
arm labels of THIS experiment added per AMENDMENT-001.md: an arm label
leaking through a filename, a comment, or a rationale would void the blind
comparison, so a match aborts the run rather than being stripped.
"""

from __future__ import annotations

import sys
from pathlib import Path

# All-lowercase technical identifiers: exact-substring, case-sensitive.
CASE_SENSITIVE_MARKERS: tuple[bytes, ...] = (
    b"b1_free_form",
    b"full_reviewgraphen",
    b"qwen",
    b"codex",
    b"lm-studio",
    b"local_id",
)

# Words a generator or a path could self-reference in any capitalization.
# `claude`/`opus`/`anthropic` are inherited from m7 (the judge is itself
# Claude, and must not be told a candidate was too). `baseline`,
# `methodology`, and `skill` are this experiment's own arm vocabulary.
CASE_INSENSITIVE_MARKERS: tuple[bytes, ...] = (
    b"claude",
    b"opus",
    b"anthropic",
    b"baseline",
    b"methodology",
    b"skill",
)


class ForbiddenMarkerError(RuntimeError):
    def __init__(self, path: str, marker: str) -> None:
        super().__init__(f"forbidden marker {marker!r} found in {path}")
        self.path = path
        self.marker = marker


def scan_bytes(path: str, data: bytes) -> None:
    for marker in CASE_SENSITIVE_MARKERS:
        if marker in data:
            raise ForbiddenMarkerError(path, marker.decode())
    lowered = data.lower()
    for marker in CASE_INSENSITIVE_MARKERS:
        if marker in lowered:
            raise ForbiddenMarkerError(path, marker.decode())


def scan_files(files: dict[str, bytes]) -> None:
    for path, data in files.items():
        scan_bytes(path, data)


def scan_directory(root: Path) -> None:
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
