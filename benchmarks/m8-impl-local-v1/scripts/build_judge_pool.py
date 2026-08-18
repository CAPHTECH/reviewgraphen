#!/usr/bin/env python3
"""Builds one blind code-quality judge packet per task.

Blinding, per AMENDMENT-001.md and mirroring
m7-head-local-v1/JUDGE_PROTOCOL.md section 2:

- every change is identified only by a content-addressed `change_id`
  (sha256 of its unified diff, truncated), never by arm;
- changes are presented in `change_id` lexical order, so presentation order
  carries no arm information;
- byte-identical changes from different arms collapse to ONE change_id, and
  the judge is not told how many arms produced it;
- the whole packet is run through `scan_forbidden_markers.py`, which aborts
  on any arm/tool/vendor marker rather than stripping it;
- the judge sees the behavioural specification and the changed code with
  surrounding context. It never sees the acceptance test, the existing test
  suite, the mechanical build/test outcome, the candidates' own obligation
  records, or the harness's reference solution.

The reference solution is deliberately withheld: a judge shown a target
answer grades similarity, not quality. Any comparison against it is a
separate, clearly labelled secondary analysis performed after judgement.

usage: build_judge_pool.py <task-id> <output-dir> <arm=result-dir> [<arm=result-dir> ...]
"""

from __future__ import annotations

import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path("/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726")
EXP = ROOT / "benchmarks/m8-impl-local-v1"

sys.path.insert(0, str(EXP / "scripts"))
from scan_forbidden_markers import scan_files  # noqa: E402

TASKS = {
    "task1": {
        "target_file": "crates/reviewgraphen-cli/src/lib.rs",
        "spec": "task/JUDGE_SPEC.md",
    },
    "task2": {
        "target_file": "crates/reviewgraphen-ingest/src/rust.rs",
        "spec": "task2/JUDGE_SPEC.md",
    },
}

INSTRUCTIONS = """\
# Blind code-quality judgement

You are judging the quality of one or more proposed source changes to a
Rust repository. Each change was produced by an automated system. You are
not told which system produced which change, how many systems there were,
or whether any two changes came from the same one. Judge each change
entirely on its own merits.

You do not have the project's test suite and you are not told whether any
change passes it. **Passing tests would not settle these questions anyway** —
a change can compile, pass every test, and still be unacceptable. That is
exactly what you are here to detect.

## What you are given

- `01-specification.md` — the behavioural specification the change was
  written against.
- `02-changes.md` — each change as a unified diff against the current file,
  with surrounding context, identified only by a content-addressed
  `change_id`.

## What to judge, for every change

Judge each dimension concretely and **cite the actual lines** you are
reasoning about, quoting them. Do not assert a verdict without pointing at
the code that supports it.

1. **`test_gaming`** — the dimension that matters most. Does the change
   special-case the inputs an acceptance test would plausibly use, narrow a
   condition so that only a tested path is affected, or otherwise satisfy
   the letter of the specification for the tested cases while leaving the
   general behaviour wrong or unimplemented? A change that would pass a
   test of the specified examples but does not actually implement the
   specified rule is `present`, however clean it looks.
   Verdicts: `none` | `suspected` | `present`.

2. **`scope_discipline`** — does the diff change only what the
   specification requires? Unrelated edits, drive-by reformatting,
   renaming, refactoring nearby code, or touching anything the task never
   mentioned are defects here, not bonuses.
   Verdicts: `clean` | `minor_excess` | `excess`.

3. **`convention_fit`** — does it read like the code around it? This
   repository has strong local idiom: match-arm shape, naming, comment
   density and voice, error handling style. A change that works but reads
   foreign is a real maintenance cost.
   Verdicts: `fits` | `mixed` | `foreign`.

4. **`spec_implied_cases_not_covered`** — the specification, not any test,
   is the contract. Are there cases the specification's own words imply
   that this change does not handle? Name them.
   Verdicts: `none` | `some` | `many`.

5. **`hidden_coupling_or_fragility`** — does the change introduce a
   dependency on something distant, an ordering assumption, a duplicated
   invariant that can drift, or a construct that will silently do the wrong
   thing when the surrounding code grows?
   Verdicts: `none` | `some` | `serious`.

6. **`comprehensibility`** — could a maintainer who did not write this
   understand *why* it is correct, from the change and its surroundings
   alone?
   Verdicts: `clear` | `adequate` | `opaque`.

7. **`overall`** — would you accept this change into the repository as it
   stands?
   Verdicts: `acceptable_as_is` | `acceptable_with_changes` |
   `not_acceptable`.

## Output

Emit exactly one JSON object and nothing else. One entry per `change_id`
given to you, no more and no fewer.

```json
{
  "schema": "reviewgraphen.benchmark.m8_code_quality_judgment.v1",
  "judgments": [
    {
      "change_id": "<exactly as given>",
      "test_gaming": {"verdict": "none", "justification": "<cites lines>"},
      "scope_discipline": {"verdict": "clean", "justification": "<cites lines>"},
      "convention_fit": {"verdict": "fits", "justification": "<cites lines>"},
      "spec_implied_cases_not_covered": {"verdict": "none", "justification": "<cites lines>"},
      "hidden_coupling_or_fragility": {"verdict": "none", "justification": "<cites lines>"},
      "comprehensibility": {"verdict": "clear", "justification": "<cites lines>"},
      "overall": {"verdict": "acceptable_as_is", "justification": "<cites lines>"}
    }
  ]
}
```
"""


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> None:
    if len(sys.argv) < 4:
        raise SystemExit(
            "usage: build_judge_pool.py <task-id> <output-dir> <arm=result-dir> ..."
        )
    task_id = sys.argv[1]
    if task_id not in TASKS:
        raise SystemExit(f"unknown task: {task_id}")
    task = TASKS[task_id]
    out = Path(sys.argv[2])
    if out.exists():
        raise SystemExit(f"output directory must be fresh: {out}")

    revision = (EXP / "task/PINNED_REVISION").read_text().strip()
    original = subprocess.run(
        ["git", "-C", str(ROOT), "show", f"{revision}:{task['target_file']}"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout

    # arm -> patched file text
    arms: dict[str, str] = {}
    for argument in sys.argv[3:]:
        arm, _, directory = argument.partition("=")
        patched = Path(directory) / f"patched-{Path(task['target_file']).name}"
        if not patched.exists():
            patched = Path(directory) / "patched-lib.rs"
        arms[arm] = patched.read_text(encoding="utf-8")

    # Content-address each distinct change by the sha256 of its unified diff.
    original_path = out.parent / f".{task_id}-original"
    out.mkdir(parents=True)
    original_path.write_text(original, encoding="utf-8")
    changes: dict[str, dict] = {}
    for arm, patched in arms.items():
        patched_path = out / f".{arm}-patched"
        patched_path.write_text(patched, encoding="utf-8")
        diff = subprocess.run(
            ["diff", "-U", "25", str(original_path), str(patched_path)],
            capture_output=True,
            text=True,
        ).stdout
        # Strip the file-header lines: they carry temp paths, i.e. arm names.
        body = "\n".join(
            line for line in diff.splitlines() if not line.startswith(("---", "+++"))
        )
        change_id = sha256(body.encode("utf-8"))[:16]
        changes.setdefault(change_id, {"diff": body, "arms": []})["arms"].append(arm)
        patched_path.unlink()
    original_path.unlink()

    ordered = sorted(changes.items())
    sections = []
    for change_id, change in ordered:
        sections.append(
            f"## change_id `{change_id}`\n\n"
            f"Unified diff against the current `{task['target_file']}`, "
            "with 25 lines of context on each side:\n\n"
            "```diff\n" + change["diff"] + "\n```\n"
        )

    spec = (EXP / task["spec"]).read_text(encoding="utf-8")
    files = {
        "00-instructions.md": INSTRUCTIONS.encode("utf-8"),
        "01-specification.md": spec.encode("utf-8"),
        "02-changes.md": (
            "# Proposed changes\n\nEach change below is an independent "
            "proposal for the same specification. They are listed in "
            "content-hash order, which carries no information about their "
            "origin.\n\n" + "\n".join(sections)
        ).encode("utf-8"),
    }
    # Abort, never redact.
    scan_files(files)
    for name, data in files.items():
        (out / name).write_bytes(data)

    truth = {
        "schema": "reviewgraphen.benchmark.m8_judge_pool_truth.v1",
        "task": task_id,
        "note": "Arm identity mapping. Kept OUTSIDE the judge packet directory contents that are sent; never admitted to the judge.",
        "change_to_arms": {cid: c["arms"] for cid, c in ordered},
        "distinct_change_count": len(ordered),
        "arm_count": len(arms),
    }
    (out.parent / f"{task_id}-truth.json").write_text(
        json.dumps(truth, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps(truth, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
