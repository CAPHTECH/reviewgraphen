#!/usr/bin/env python3
"""Maps each trial to the content-addressed `change_id` its diff was judged
under, so the frozen analysis can attach verdicts to trials.

Withheld from every judge call; produced only after judging is complete.

usage: build_replication_truth.py <output-json>
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

POOLS = Path("/tmp/m8-rep-judge")
RESULTS = Path("/tmp/m8-rep-judge-results")
RUNS = Path("/tmp/m8-impl-local-v1-runs")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: build_replication_truth.py <output-json>")

    # pool directory name is the result-file hash; its sibling truth file
    # carries the change_id the judge actually saw.
    # Each judge result directory is named for the result-file hash of the
    # single change it judged; the change_id the judge actually saw is inside
    # its own judgment. (build_judge_pool writes one shared truth filename per
    # task, so the per-pool copies overwrote one another -- the judgments are
    # the durable mapping.)
    resultfile_to_change: dict[str, str] = {}
    for judgment_path in sorted(RESULTS.glob("*/judgment.json")):
        resultfile = judgment_path.parent.name
        record = json.loads(judgment_path.read_text())
        for entry in record.get("judgments", []):
            resultfile_to_change[resultfile] = entry["change_id"]

    change_to_trials: dict[str, list[str]] = {}
    trial_to_change: dict[str, str] = {}
    unjudged: list[str] = []
    for source in sorted(RUNS.glob("rep-*")):
        patched = source / "patched-lib.rs"
        if not patched.exists():
            unjudged.append(source.name)
            continue
        resultfile = hashlib.sha256(patched.read_bytes()).hexdigest()[:16]
        change_id = resultfile_to_change.get(resultfile)
        if change_id is None:
            unjudged.append(source.name)
            continue
        change_to_trials.setdefault(change_id, []).append(source.name)
        trial_to_change[source.name] = change_id

    Path(sys.argv[1]).write_text(
        json.dumps(
            {
                "schema": "reviewgraphen.benchmark.m8_replication_truth.v1",
                "note": "Never admitted to any judge call.",
                "change_to_trials": change_to_trials,
                "trial_to_change": trial_to_change,
                "trials_without_a_judged_change": unjudged,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
    print(Path(sys.argv[1]).read_text())


if __name__ == "__main__":
    main()
