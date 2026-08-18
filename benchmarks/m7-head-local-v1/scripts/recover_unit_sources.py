#!/usr/bin/env python3
"""Reconstructs source trees for m7-head-local-v1 units from the fsl repo
at the frozen revision, hash-verified against units.json.

The original /tmp-prepared packets were destroyed in a PC restart (see
diagnostics/final-judge-pool/RECOVERY.md); this recovers the same bytes
deterministically from fsl's own git history, so the pipeline does not
depend on any /tmp state surviving between sessions. Writes only to the
given output directory (default /tmp/...) -- never touches the fsl
checkout's working tree (only `git show`, a metadata-only read).
"""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
UNITS_JSON = REPO_ROOT / "benchmarks" / "m7-head-local-v1" / "units.json"
FSL = Path("/home/rizumita/github/fsl")
DEFAULT_OUTPUT = Path("/tmp/m7-head-local-v1-recovered-sources")


def main() -> None:
    if len(sys.argv) not in (2, 3):
        raise SystemExit("usage: recover_unit_sources.py <unit_id>[,<unit_id>...] [output-dir]")
    wanted = set(sys.argv[1].split(","))
    output = Path(sys.argv[2]) if len(sys.argv) == 3 else DEFAULT_OUTPUT

    units = json.loads(UNITS_JSON.read_bytes())
    revision = units["head_commit"]

    verified = 0
    for unit in units["units"]:
        if unit["unit_id"] not in wanted:
            continue
        for f in unit["files"]:
            blob = subprocess.run(
                ["git", "-C", str(FSL), "show", f"{revision}:{f['path']}"],
                check=True, stdout=subprocess.PIPE,
            ).stdout
            observed_hash = hashlib.sha256(blob).hexdigest()
            if observed_hash != f["sha256"]:
                raise SystemExit(
                    f"HASH MISMATCH {unit['unit_id']} {f['path']}: "
                    f"expected {f['sha256']}, got {observed_hash}"
                )
            if len(blob) != f["bytes"]:
                raise SystemExit(f"BYTE-LEN MISMATCH {unit['unit_id']} {f['path']}")
            out_path = output / unit["unit_id"] / f["path"]
            out_path.parent.mkdir(parents=True, exist_ok=True)
            out_path.write_bytes(blob)
            verified += 1

    print(f"recovered and hash-verified {verified} files under {output}")


if __name__ == "__main__":
    main()
