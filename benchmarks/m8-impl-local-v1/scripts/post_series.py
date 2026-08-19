#!/usr/bin/env python3
"""Post-series quiescent phase, per AMENDMENT-004.md and AMENDMENT-005.md.

Applied uniformly to every tree, with no exceptions and no selective use:

1. Re-verify every completed trial once, byte-identical procedure, nothing
   else running. The post-series result is the reported one; the in-series
   result is retained; disagreement is flagged `flaky_verification`.
2. Run each diagnostic probe THREE times on every tree. Unanimity is
   required; disagreement is recorded as `indeterminate` and no claim is
   made from it in either direction.
3. **Every tree gets a private `CARGO_TARGET_DIR`.** Every scratch tree
   builds a package named `reviewgraphen-ingest` version `0.1.0`; with one
   shared target dir, cargo served one tree's compiled library to another
   and produced measurements that were simply wrong (AMENDMENT-005 section
   3, where the effect is demonstrated deliberately). This costs a full
   rebuild per tree.

usage: post_series.py <output-json>
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path("/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726")
EXP = ROOT / "benchmarks/m8-impl-local-v1"
RUNS = Path("/tmp/m8-impl-local-v1-runs")
TRUSTED_CARGO = "/home/rizumita/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo"
CARGO_TARGET_ROOT = Path("/tmp/m8-post-targets")
PROBES = ("m8_foreign_macro_probe", "m8_foreign_safefn_probe")


def env_for(tree: str) -> dict:
    target = CARGO_TARGET_ROOT / tree
    target.mkdir(parents=True, exist_ok=True)
    return {
        "PATH": "/home/rizumita/.cargo/bin:/usr/local/bin:/usr/bin:/bin",
        "HOME": "/home/rizumita",
        "CARGO_TARGET_DIR": str(target),
        "REVIEWGRAPHEN_TRUSTED_CARGO": TRUSTED_CARGO,
    }


def sh(command: list[str], tree: str) -> int:
    return subprocess.run(
        command, capture_output=True, text=True, env=env_for(tree)
    ).returncode


def probe_triplicate(scratch: Path, tree: str) -> dict[str, object]:
    results: dict[str, object] = {}
    for probe in PROBES:
        destination = scratch / "crates/reviewgraphen-ingest/tests" / f"{probe}.rs"
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(
            (EXP / "task2" / f"{probe}.rs").read_text(encoding="utf-8"), encoding="utf-8"
        )
        outcomes = []
        for _ in range(3):
            code = sh(
                [
                    "cargo",
                    "test",
                    "--manifest-path",
                    str(scratch / "Cargo.toml"),
                    "-p",
                    "reviewgraphen-ingest",
                    "--test",
                    probe,
                ],
                tree,
            )
            outcomes.append("pass" if code == 0 else "fail")
        results[probe] = outcomes[0] if len(set(outcomes)) == 1 else "indeterminate"
        results[f"{probe}_runs"] = outcomes
    return results


def catchall_shape(diff_text: str) -> str:
    """AMENDMENT-003 section 4's syntactic measure. Read off the diff; no
    execution is involved, so it is unaffected by AMENDMENT-005's defect."""
    added = "\n".join(
        line[1:] for line in diff_text.splitlines() if line.startswith("+")
    )
    if "ForeignItem" not in added and "ForeignMod" not in added:
        return "absent"
    if "conservative_unresolved = true" in added or "conservative_unresolved |= true" in added:
        return "fail_closed"
    if "_ => {}" in added or "_ => ()" in added:
        return "fail_open"
    return "unclassified"


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: post_series.py <output-json>")
    report: dict = {
        "schema": "reviewgraphen.benchmark.m8_post_series.v1",
        "private_cargo_target_dir_per_tree": True,
        "trials": {},
        "reference_trees": {},
    }

    trials = sorted(
        p.name for p in RUNS.glob("rep-*") if (p / "final-content.txt").exists()
    )
    for trial in trials:
        result_dir = RUNS / trial
        in_series = None
        path = result_dir / "verification.json"
        if path.exists():
            in_series = json.loads(path.read_text())
            (result_dir / "verification-in-series.json").write_text(
                json.dumps(in_series, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
        scratch = Path(f"/tmp/m8-post-{trial}")
        subprocess.run(["rm", "-rf", str(scratch)], check=False)
        sh(
            [
                "python3",
                str(EXP / "scripts/apply_and_verify_task2.py"),
                str(result_dir),
                str(scratch),
            ],
            trial,
        )
        post = json.loads((result_dir / "verification.json").read_text())
        entry: dict = {
            "in_series_verdict": (in_series or {}).get("verdict"),
            "post_series_verdict": post.get("verdict"),
            "compiles": post.get("compiles"),
            "tests_pass": post.get("tests_pass"),
            "test_result_lines": post.get("test_result_lines"),
            "obligation_count": post.get("candidate_obligation_count"),
            "declares_verified": post.get("candidate_declares_verified"),
            "schema_validated_against": post.get("schema_validated_against"),
        }
        entry["flaky_verification"] = (
            in_series is not None
            and entry["in_series_verdict"] != entry["post_series_verdict"]
        )
        diff_path = result_dir / "applied.diff"
        entry["foreign_item_catchall"] = (
            catchall_shape(diff_path.read_text(encoding="utf-8"))
            if diff_path.exists()
            else "no_diff"
        )
        if scratch.exists():
            entry.update(probe_triplicate(scratch, trial))
            patched = scratch / "crates/reviewgraphen-ingest/src/rust.rs"
            if patched.exists():
                entry["result_sha256"] = subprocess.run(
                    ["sha256sum", str(patched)], capture_output=True, text=True
                ).stdout.split()[0]
            subprocess.run(["rm", "-rf", str(scratch)], check=False)
        report["trials"][trial] = entry
        print(json.dumps({trial: entry}, indent=2, sort_keys=True), flush=True)

    # The trees AMENDMENT-003 section 1 measured with single runs, plus the
    # completed task-2 pair, all re-measured under the fixed harness.
    references = {
        "pinned": None,
        "reference_fix": "reference",
        "task2_treatment_n1": str(RUNS / "task2-rerun-methodology"),
        "task2_control_n1": str(RUNS / "task2-rerun-baseline"),
    }
    for name, source in references.items():
        tree = f"ref-{name}"
        scratch = Path(f"/tmp/m8-post-{tree}")
        subprocess.run(["rm", "-rf", str(scratch)], check=False)
        if source is None:
            sh(["bash", str(EXP / "scripts/make_scratch.sh"), str(scratch)], tree)
        elif source == "reference":
            subprocess.run(["rm", "-rf", "/tmp/m8-post-ref-candidate"], check=False)
            sh(
                [
                    "python3",
                    str(EXP / "scripts/make_reference_candidate_task2.py"),
                    "/tmp/m8-post-ref-candidate",
                ],
                tree,
            )
            sh(
                [
                    "python3",
                    str(EXP / "scripts/apply_and_verify_task2.py"),
                    "/tmp/m8-post-ref-candidate",
                    str(scratch),
                ],
                tree,
            )
        else:
            sh(
                [
                    "python3",
                    str(EXP / "scripts/apply_and_verify_task2.py"),
                    source,
                    str(scratch),
                ],
                tree,
            )
        if scratch.exists():
            report["reference_trees"][name] = probe_triplicate(scratch, tree)
            subprocess.run(["rm", "-rf", str(scratch)], check=False)
        print(
            json.dumps({name: report["reference_trees"].get(name)}, indent=2), flush=True
        )

    Path(sys.argv[1]).write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
