#!/usr/bin/env python3
"""Build one arm-blind packet per distinct m10 resulting file."""

from __future__ import annotations

import difflib
import hashlib
import json
import shutil
from pathlib import Path


ROOT = Path("/home/rizumita/workspace/reviewgraphen")
EXP = ROOT / "benchmarks/m10-target-context-local-v1"
RUNS = EXP / "runs"
OUT = Path("/tmp/m10-judge-packets")
SPEC = ROOT / "benchmarks/m8-impl-local-v1/task2/JUDGE_SPEC.md"
SCANNER_ROOT = ROOT / "benchmarks/m8-impl-local-v1/scripts"

INSTRUCTIONS = """# Blind code-quality judgement

You are judging one proposed source change to a Rust repository. The change
was produced by an automated system. You are not told which experimental
condition produced it, the mechanical outcome, or any reference solution.
Judge the change entirely on its own merits and cite the actual changed lines.

For every dimension below, use exactly one allowed verdict:

1. `test_gaming`: `none` | `suspected` | `present`
2. `scope_discipline`: `clean` | `minor_excess` | `excess`
3. `convention_fit`: `fits` | `mixed` | `foreign`
4. `spec_implied_cases_not_covered`: `none` | `some` | `many`
5. `hidden_coupling_or_fragility`: `none` | `some` | `serious`
6. `comprehensibility`: `clear` | `adequate` | `opaque`
7. `overall`: `acceptable_as_is` | `acceptable_with_changes` |
   `not_acceptable`

Emit exactly one JSON object and nothing else:

```json
{
  "schema": "reviewgraphen.benchmark.m8_code_quality_judgment.v1",
  "judgments": [{
    "change_id": "<exactly as given>",
    "test_gaming": {"verdict": "none", "justification": "<line-citing reason>"},
    "scope_discipline": {"verdict": "clean", "justification": "<line-citing reason>"},
    "convention_fit": {"verdict": "fits", "justification": "<line-citing reason>"},
    "spec_implied_cases_not_covered": {"verdict": "none", "justification": "<line-citing reason>"},
    "hidden_coupling_or_fragility": {"verdict": "none", "justification": "<line-citing reason>"},
    "comprehensibility": {"verdict": "clear", "justification": "<line-citing reason>"},
    "overall": {"verdict": "acceptable_as_is", "justification": "<line-citing reason>"}
  }]
}
```
"""


def digest(data: str) -> str:
    return hashlib.sha256(data.encode()).hexdigest()[:16]


def main() -> None:
    if OUT.exists():
        raise SystemExit(f"output directory must be fresh: {OUT}")
    OUT.mkdir(parents=True)
    baseline = (next(iter(sorted(RUNS.iterdir()))) / "pinned-rust.rs").read_text()
    changes: dict[str, dict[str, object]] = {}
    for trial in sorted(RUNS.iterdir()):
        if not trial.is_dir():
            continue
        patched = (trial / "patched-rust.rs").read_text()
        body = "\n".join(
            line
            for line in difflib.unified_diff(
                baseline.splitlines(), patched.splitlines(), n=25, lineterm=""
            )
            if not line.startswith(("---", "+++"))
        )
        change_id = digest(body)
        entry = changes.setdefault(change_id, {"diff": body, "trials": []})
        entry["trials"].append(trial.name)

    for change_id, entry in sorted(changes.items()):
        pool = OUT / change_id
        pool.mkdir()
        (pool / "00-instructions.md").write_text(INSTRUCTIONS)
        (pool / "01-specification.md").write_text(SPEC.read_text())
        (pool / "02-change.md").write_text(
            "# Proposed change\n\n"
            f"## change_id `{change_id}`\n\n"
            "Unified diff with 25 lines of context:\n\n```diff\n"
            f"{entry['diff']}\n```\n"
        )

    # The scanner aborts on leaks; it never redacts packet content.
    import sys

    sys.path.insert(0, str(SCANNER_ROOT))
    from scan_forbidden_markers import scan_directory

    for pool in sorted(OUT.iterdir()):
        scan_directory(pool)

    truth = {
        "schema": "reviewgraphen.benchmark.m10_judge_truth.v1",
        "note": "Withheld from every judge call.",
        "change_to_trials": {
            change_id: entry["trials"] for change_id, entry in sorted(changes.items())
        },
        "distinct_change_count": len(changes),
    }
    (OUT.parent / "m10-judge-truth.json").write_text(
        json.dumps(truth, indent=2, sort_keys=True) + "\n"
    )
    print(json.dumps(truth, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
