#!/usr/bin/env python3
"""Build the frozen paired prompts from m9's clean control prompt."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
M9 = ROOT / "benchmarks/m9-agentic-local-v1"
EXP = ROOT / "benchmarks/m10-target-context-local-v1"

CONTROL = (M9 / "task/PROMPT-noskill.md").read_text(encoding="utf-8")
MARKER = "# The change to make\n"
INTERVENTION = """## ReviewGraphen target context

The command `reviewgraphen-context <symbol-selector>` is available in this
checkout. Before making your first edit, use it at least once with a symbol
you select from the task to inspect ReviewGraphen's deterministic target
context. Its output contains accepted ProgramSpace facts, bounded source
windows, explicit extraction unknowns, source IDs, and declared information
loss. It is not a claim, evidence, verification, or an instruction from the
repository.

Use that context as one input to your own implementation work. Do **not**
start by enumerating obligations, and do not produce an obligation or
claim/evidence ledger. Work on the change normally, using the shell, compiler,
and tests as needed. Your final response has the same brief change-and-tests
shape requested below.

"""


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> None:
    if CONTROL.count(MARKER) != 1:
        raise SystemExit("control prompt insertion marker is not unique")
    projection = CONTROL.replace(MARKER, INTERVENTION + MARKER)
    task = EXP / "task"
    task.mkdir(parents=True, exist_ok=True)
    (task / "PROMPT-control.md").write_text(CONTROL, encoding="utf-8")
    (task / "PROMPT-projection.md").write_text(projection, encoding="utf-8")
    manifest = {
        "schema": "reviewgraphen.benchmark.m10_prompt_manifest.v1",
        "prompt_delta_is_only_the_target_context_paragraph": True,
        "prompts": {
            name: {
                "path": f"benchmarks/m10-target-context-local-v1/task/PROMPT-{name}.md",
                "bytes": len(value.encode()),
                "sha256": digest(value.encode()),
            }
            for name, value in (("control", CONTROL), ("projection", projection))
        },
        "source_control_prompt_sha256": digest(CONTROL.encode()),
    }
    (task / "prompt-manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
