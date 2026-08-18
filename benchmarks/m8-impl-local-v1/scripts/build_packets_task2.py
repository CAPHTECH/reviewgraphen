#!/usr/bin/env python3
"""Builds the two task-2 packets.

Same single manipulated variable as task 1: only the methodology packet
carries the frozen implementation methodology skill body.

One deliberate difference from task 1, preregistered in
AMENDMENT-001.md: the acceptance test is NOT included in the packet. On
task 1 the test was shown to remove specification ambiguity from a
feasibility gate. On task 2 showing it would hand over the very preservation
constraints the experiment is trying to see whether obligation enumeration
surfaces on its own. The required behaviour is instead stated in full prose
in TASK2.md, at the level a precise ticket would.

usage: build_packets_task2.py <output-dir>
"""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path("/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726")
EXP = ROOT / "benchmarks/m8-impl-local-v1"
TARGET_FILE = "crates/reviewgraphen-ingest/src/rust.rs"

OUTPUT_CONTRACT = """\
# Output contract

Emit exactly one JSON object and nothing else after it. No prose after the
JSON. If you use a fenced code block, use one ```json fence containing only
the object.

Schema `reviewgraphen.benchmark.implementation_candidate_output.v1`:

```json
{
  "schema": "reviewgraphen.benchmark.implementation_candidate_output.v1",
  "outcome": "completed",
  "edits": [
    {
      "file": "crates/reviewgraphen-ingest/src/rust.rs",
      "old": "<an exact, contiguous, unique excerpt of the current file>",
      "new": "<the text that replaces it>"
    }
  ],
  "obligations": [
    {
      "id": "obligation-1",
      "target_kind": "Node | Relation | Subgraph | Path | Invariant | Morphism",
      "target": "<what is being checked>",
      "property": "<a named property>",
      "context_requirement": "<what source this needed>",
      "evidence_requirement": "<the exact procedure whose result would settle it>",
      "risk": "<impact / exposure / likelihood>",
      "provenance": "<why this obligation exists>",
      "claim": "satisfied | not_satisfied | inconclusive | not_applicable | conflict",
      "evidence_status": "verified | claimed | requested",
      "note": "<what you actually checked, and its bound>"
    }
  ],
  "limitations": ["<what this change is not checked against>"]
}
```

Rules for `edits`:

- `file` must be exactly `crates/reviewgraphen-ingest/src/rust.rs`. No other
  file may appear.
- `old` must be an **exact, byte-for-byte, contiguous excerpt** of the
  current contents of that file as given to you below, including
  indentation, and must occur **exactly once** in it. It will be replaced
  by `new` by literal string substitution. If `old` does not match exactly
  and uniquely, the edit is discarded and the run counts as producing no
  applicable change.
- Keep `old` as small as possible while still unique. The file is large; do
  not restate it.
- `obligations` and `limitations` are optional; `schema`, `outcome`, and
  `edits` are required. If you decide not to attempt the change, emit
  `"outcome": "abstained"` with an `"abstention_reason"` and an empty
  `edits` array.
"""

CONTROL_OUTPUT_CONTRACT = """\
# Output contract

Emit exactly one JSON object and nothing else after it. No prose after the
JSON. If you use a fenced code block, use one ```json fence containing only
the object.

Schema `reviewgraphen.benchmark.implementation_candidate_edit.v1`:

```json
{
  "schema": "reviewgraphen.benchmark.implementation_candidate_edit.v1",
  "outcome": "completed",
  "edits": [
    {
      "file": "crates/reviewgraphen-ingest/src/rust.rs",
      "old": "<an exact, contiguous, unique excerpt of the current file>",
      "new": "<the text that replaces it>"
    }
  ]
}
```

Rules for `edits`:

- `file` must be exactly `crates/reviewgraphen-ingest/src/rust.rs`. No other
  file may appear.
- `old` must be an **exact, byte-for-byte, contiguous excerpt** of the
  current contents of that file as given to you below, including
  indentation, and must occur **exactly once** in it. It will be replaced
  by `new` by literal string substitution. If `old` does not match exactly
  and uniquely, the edit is discarded and the run counts as producing no
  applicable change.
- Keep `old` as small as possible while still unique. The file is large; do
  not restate it.
- If you decide not to attempt the change, emit `"outcome": "abstained"`
  with an `"abstention_reason"` and an empty `edits` array.

No other fields are permitted.
"""

METHODOLOGY_PREAMBLE = """\
# Methodology you must follow

Execute the methodology below, in its stage order, on the task that follows
it. It is a methodology, not a tool; you perform every stage yourself.

"""

TASK_PREAMBLE = """\
# Your task

You are implementing one specified change in a Rust repository. You have no
shell, no compiler, and no test runner in this session: you cannot run
anything. You will emit a description of the change; a separate harness will
apply it to an isolated copy of the repository and run the build and the
tests.

"""


def sha256(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: build_packets_task2.py <output-dir>")
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)

    revision = (EXP / "task/PINNED_REVISION").read_text().strip()
    source = subprocess.run(
        ["git", "-C", str(ROOT), "show", f"{revision}:{TARGET_FILE}"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    task = (EXP / "task2/TASK2.md").read_text(encoding="utf-8")
    test = (EXP / "task2/m8_extern_block_shadow.rs").read_text(encoding="utf-8")
    skill = (EXP / "skill/IMPLEMENTATION_SKILL.md").read_text(encoding="utf-8")

    shared = (
        TASK_PREAMBLE
        + task
        + f"\n\n# Current contents of `{TARGET_FILE}` (revision {revision})\n\n"
        + "```rust\n"
        + source
        + "```\n\n"
    )

    # See build_packets.py: the control's contract carries the edit and
    # nothing else, so no methodology vocabulary leaks into it.
    packets = {
        "baseline": shared + CONTROL_OUTPUT_CONTRACT,
        "methodology": (
            METHODOLOGY_PREAMBLE + skill + "\n\n---\n\n" + shared + OUTPUT_CONTRACT
        ),
    }

    manifest = {
        "schema": "reviewgraphen.benchmark.m8_impl_local_v1_packet_manifest.v1",
        "task": "task2-extern-block-shadow",
        "pinned_revision": revision,
        "target_file": TARGET_FILE,
        "target_file_sha256": sha256(source),
        "target_file_bytes": len(source.encode("utf-8")),
        "acceptance_test_file": "crates/reviewgraphen-ingest/tests/m8_extern_block_shadow.rs",
        "acceptance_test_sha256": sha256(test),
        "acceptance_test_included_in_packet": False,
        "task_sha256": sha256(task),
        "skill_sha256": sha256(skill),
        "output_contract_sha256": sha256(OUTPUT_CONTRACT),
        "control_output_contract_sha256": sha256(CONTROL_OUTPUT_CONTRACT),
        "packets": {},
    }
    for name, body in packets.items():
        path = out / f"packet-{name}.txt"
        path.write_text(body, encoding="utf-8")
        manifest["packets"][name] = {
            "path": str(path),
            "bytes": len(body.encode("utf-8")),
            "sha256": sha256(body),
        }
    (out / "packet-manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(manifest, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
