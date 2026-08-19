#!/usr/bin/env python3
"""Applies AMENDMENT-003.md's frozen decision rules to the replication.

The rules are not re-derived here; they are transcribed. Nothing in this
script chooses a threshold — every threshold below is quoted from the
amendment, which was committed before the first replication request.

usage: analyse_replication.py <post-series-json> <judgment-dir> <truth-json> <output-json>
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

# AMENDMENT-003 section 3: ordinal scales, best (0) to worst (2).
SCALES = {
    "test_gaming": ["none", "suspected", "present"],
    "scope_discipline": ["clean", "minor_excess", "excess"],
    "convention_fit": ["fits", "mixed", "foreign"],
    "spec_implied_cases_not_covered": ["none", "some", "many"],
    "hidden_coupling_or_fragility": ["none", "some", "serious"],
    "comprehensibility": ["clear", "adequate", "opaque"],
    "overall": ["acceptable_as_is", "acceptable_with_changes", "not_acceptable"],
}


def classify(treatment: list[int], control: list[int]) -> str:
    """AMENDMENT-003 section 3, verbatim thresholds."""
    if not treatment or not control:
        return "insufficient_data"
    if max(treatment) < min(control) or max(control) < min(treatment):
        return "ACCEPTED_complete_separation"
    best_t = sum(1 for value in treatment if value == 0)
    best_c = sum(1 for value in control if value == 0)
    if abs(best_t - best_c) >= 3:
        return "suggestive"
    return "noise"


def main() -> None:
    if len(sys.argv) != 5:
        raise SystemExit(
            "usage: analyse_replication.py <post-series-json> <judgment-dir> <truth-json> <output-json>"
        )
    post = json.loads(Path(sys.argv[1]).read_text())
    judgment_dir = Path(sys.argv[2])
    truth = json.loads(Path(sys.argv[3]).read_text())

    # change_id -> judgment, from one call per distinct change.
    verdicts: dict[str, dict] = {}
    for path in sorted(judgment_dir.glob("*/judgment.json")):
        for entry in json.loads(path.read_text())["judgments"]:
            verdicts[entry["change_id"]] = entry

    arms = {"methodology": [], "baseline": []}
    for trial, record in sorted(post["trials"].items()):
        arm = "methodology" if "methodology" in trial else "baseline"
        arms[arm].append((trial, record))

    report: dict = {
        "schema": "reviewgraphen.benchmark.m8_replication_analysis.v1",
        "rules_source": "AMENDMENT-003.md sections 3-5 (frozen before the first replication request)",
        "completed_trials": {arm: len(trials) for arm, trials in arms.items()},
    }

    # AMENDMENT-003 section 6: below 3 completed trials in either arm, no
    # comparative rule is applied at all.
    if min(len(trials) for trials in arms.values()) < 3:
        report["result"] = "series_incomplete"
        report["note"] = (
            "Fewer than 3 completed trials in at least one arm; per AMENDMENT-003 "
            "section 6 the section 3 and 5 rules are not applied and no comparative "
            "claim is made."
        )
        Path(sys.argv[4]).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
        print(json.dumps(report, indent=2, sort_keys=True))
        return

    # Mechanical outcomes: accepted only at >=4/5 vs <=1/5.
    mechanical = {}
    for key in ("compiles", "tests_pass"):
        counts = {
            arm: sum(1 for _, record in trials if record.get(key) is True)
            for arm, trials in arms.items()
        }
        counts["accepted_difference"] = (
            max(counts["methodology"], counts["baseline"]) >= 4
            and min(counts["methodology"], counts["baseline"]) <= 1
        )
        mechanical[key] = counts
    report["mechanical"] = mechanical

    # AMENDMENT-003 section 5: within-arm variance.
    distinct = {
        arm: sorted({record.get("result_sha256") for _, record in trials if record.get("result_sha256")})
        for arm, trials in arms.items()
    }
    report["distinct_result_files"] = {arm: len(values) for arm, values in distinct.items()}
    variance_swamps = any(len(values) >= 3 for values in distinct.values())
    report["within_arm_variance_swamps_non_separated_dimensions"] = variance_swamps

    # Judged dimensions, over trials whose change was judged.
    change_of_trial = {}
    for change_id, trials in (truth.get("change_to_trials") or {}).items():
        for trial in trials:
            change_of_trial[trial] = change_id

    dimensions: dict[str, dict] = {}
    for dimension, scale in SCALES.items():
        scores = {"methodology": [], "baseline": []}
        for arm, trials in arms.items():
            for trial, _ in trials:
                change_id = change_of_trial.get(trial)
                entry = verdicts.get(change_id) if change_id else None
                if not entry:
                    continue
                verdict = entry.get(dimension, {}).get("verdict")
                if verdict in scale:
                    scores[arm].append(scale.index(verdict))
        outcome = classify(scores["methodology"], scores["baseline"])
        if variance_swamps and outcome != "ACCEPTED_complete_separation":
            outcome = f"{outcome}_no_directional_claim_within_arm_variance"
        dimensions[dimension] = {
            "methodology_scores": scores["methodology"],
            "baseline_scores": scores["baseline"],
            "outcome": outcome,
        }
    report["dimensions"] = dimensions

    # AMENDMENT-003 section 4, syntactic recurrence + probe status. Probe
    # results are only interpretable on a tree that compiled at all.
    probes = {}
    for arm, trials in arms.items():
        probes[arm] = {
            trial: {
                "compiles": record.get("compiles"),
                "macro": record.get("m8_foreign_macro_probe")
                if record.get("compiles")
                else "not_applicable_did_not_compile",
                "safefn": record.get("m8_foreign_safefn_probe")
                if record.get("compiles")
                else "not_applicable_did_not_compile",
            }
            for trial, record in trials
        }
    report["probes"] = probes
    report["reference_tree_probes"] = post.get("reference_trees")

    Path(sys.argv[4]).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
