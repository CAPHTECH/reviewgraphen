#!/usr/bin/env python3
"""Builds the two arm prompts from m8's frozen task-2 specification.

The behavioural requirement is reused byte-for-byte from
`benchmarks/m8-impl-local-v1/task2/TASK2.md`, so this experiment asks about
the same change the single-shot series asked about. Only the framing
differs: there the model emitted a JSON edit, here it has a shell.

The two prompts are identical except for one paragraph naming the skill.
That paragraph is the entire manipulated variable.

usage: build_prompts.py
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path

ROOT = Path("/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726")
M8 = ROOT / "benchmarks/m8-impl-local-v1"
EXP = ROOT / "benchmarks/m9-agentic-local-v1"

FRAMING = """\
# Your task

You are working in a checkout of a Rust workspace at the current directory.
You have a shell. `cargo` is available and the toolchain is already
installed. You can build, run tests, and read any file in this checkout.

Work until the change is done, then stop.

## Ground rules

- You may change exactly one file: `crates/reviewgraphen-ingest/src/rust.rs`.
- **Do not add, delete, or modify any test, in this crate or any other.**
  Your change is checked by an acceptance test you have not been given and
  cannot see. Editing tests cannot help you and will be detected.
- Do not add dependencies and do not change any `Cargo.toml`.
- The existing test suite is in the checkout and you may run it.

"""

SKILL_PARAGRAPH = """\
## Methodology

A skill named `reviewgraphen-implementation-methodology` is available in this
checkout. **Load it and follow it** for this task, in its stage order.

"""

CLOSING = """\

## When you are done

Leave the working tree in the state you want checked. Then stop and briefly
state what you changed and what you ran.
"""


def sha256(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def main() -> None:
    task = (M8 / "task2/TASK2.md").read_text(encoding="utf-8")

    # Two sentences of TASK2.md describe the single-shot harness rather than
    # the change, and are false here: the agent CAN see the existing suite.
    # Replaced, not deleted, so the difference is visible in the record.
    task = task.replace(
        "You are not shown the acceptance test or the existing suite. You may not add\nor change any test.",
        "You are not shown the acceptance test. You may not add or change any test.",
    )
    task = task.replace(
        "# Implementation task 2 (identical text in every arm)",
        "# The change to make",
    )

    prompts = {
        "noskill": FRAMING + task + CLOSING,
        "skill": FRAMING + SKILL_PARAGRAPH + task + CLOSING,
    }

    manifest = {
        "schema": "reviewgraphen.benchmark.m9_prompt_manifest.v1",
        "source_task": "benchmarks/m8-impl-local-v1/task2/TASK2.md",
        "source_task_sha256": sha256((M8 / "task2/TASK2.md").read_text(encoding="utf-8")),
        "skill_body_sha256": sha256(
            (M8 / "skill/IMPLEMENTATION_SKILL.md").read_text(encoding="utf-8")
        ),
        "prompts": {},
    }
    task_dir = EXP / "task"
    task_dir.mkdir(parents=True, exist_ok=True)
    for arm, body in prompts.items():
        path = task_dir / f"PROMPT-{arm}.md"
        path.write_text(body, encoding="utf-8")
        manifest["prompts"][arm] = {
            "path": str(path.relative_to(ROOT)),
            "bytes": len(body.encode("utf-8")),
            "sha256": sha256(body),
        }
    manifest["prompt_delta_is_only_the_skill_paragraph"] = (
        prompts["skill"].replace(SKILL_PARAGRAPH, "") == prompts["noskill"]
    )
    (task_dir / "prompt-manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(manifest, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
