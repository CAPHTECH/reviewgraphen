#!/usr/bin/env python3
"""Builds the two m8-impl-local-v1 request packets.

Both arms receive byte-identical task text, byte-identical source, a
byte-identical acceptance test, and a byte-identical output contract. The
*only* difference between them is that the `methodology` packet additionally
carries the frozen implementation methodology skill body verbatim, and a
sentence instructing the model to follow it.

That single difference is the manipulated variable. Anything else that
differed would make the comparison uninterpretable.

usage: build_packets.py <output-dir>
"""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path("/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726")
EXP = ROOT / "benchmarks/m8-impl-local-v1"
TARGET_FILE = "crates/reviewgraphen-cli/src/lib.rs"
TEST_FILE = "crates/reviewgraphen-cli/tests/review_flag_order.rs"

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
      "file": "crates/reviewgraphen-cli/src/lib.rs",
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

- `file` must be exactly `crates/reviewgraphen-cli/src/lib.rs`. No other
  file may appear.
- `old` must be an **exact, byte-for-byte, contiguous excerpt** of the
  current contents of that file as given to you below, including
  indentation, and must occur **exactly once** in it. It will be replaced
  by `new` by literal string substitution. If `old` does not match exactly
  and uniquely, the edit is discarded and the run counts as producing no
  applicable change.
- Keep `old` as small as possible while still unique.
- Alternatively, a single edit entry may carry `"full_content"` instead of
  `"old"`/`"new"`, holding the complete new text of the file. Use this only
  if you cannot produce a reliable excerpt; it is more output.
- `obligations` and `limitations` are optional; `schema`, `outcome`, and
  `edits` are required. If you decide not to attempt the change, emit
  `"outcome": "abstained"` with an `"abstention_reason"` and an empty
  `edits` array.
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
        raise SystemExit("usage: build_packets.py <output-dir>")
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)

    revision = (EXP / "task/PINNED_REVISION").read_text().strip()
    source = subprocess.run(
        ["git", "-C", str(ROOT), "show", f"{revision}:{TARGET_FILE}"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    task = (EXP / "task/TASK.md").read_text(encoding="utf-8")
    test = (EXP / "task/review_flag_order.rs").read_text(encoding="utf-8")
    skill = (EXP / "skill/IMPLEMENTATION_SKILL.md").read_text(encoding="utf-8")

    common = (
        TASK_PREAMBLE
        + task
        + f"\n\n# Current contents of `{TARGET_FILE}` (revision {revision})\n\n"
        + "```rust\n"
        + source
        + "```\n\n"
        + f"# Acceptance test `{TEST_FILE}` (read-only, already present)\n\n"
        + "```rust\n"
        + test
        + "```\n\n"
        + OUTPUT_CONTRACT
    )

    packets = {
        "baseline": common,
        "methodology": METHODOLOGY_PREAMBLE + skill + "\n\n---\n\n" + common,
    }

    manifest = {
        "schema": "reviewgraphen.benchmark.m8_impl_local_v1_packet_manifest.v1",
        "pinned_revision": revision,
        "target_file": TARGET_FILE,
        "target_file_sha256": sha256(source),
        "acceptance_test_file": TEST_FILE,
        "acceptance_test_sha256": sha256(test),
        "task_sha256": sha256(task),
        "skill_sha256": sha256(skill),
        "output_contract_sha256": sha256(OUTPUT_CONTRACT),
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
