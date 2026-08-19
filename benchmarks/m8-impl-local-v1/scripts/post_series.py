#!/usr/bin/env python3
"""Post-series quiescent phase, per AMENDMENT-004.md.

Two jobs, applied uniformly to every tree with no exceptions:

1. Re-verify every completed trial once, byte-identical procedure, nothing
   else running. The post-series result is the reported one; the in-series
   result is retained; disagreement is flagged `flaky_verification`.
2. Run each diagnostic probe THREE times on every tree. Unanimity is
   required; any disagreement is recorded as `indeterminate` and no claim is
   made from it in either direction. The same triplicate is applied to the
   four reference trees AMENDMENT-003 section 1 measured only once.

Nothing here is a gate and nothing here can rescue one trial selectively:
every completed trial goes through the identical path.

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
CARGO_TARGET_DIR = "/tmp/claude-1000/-home-rizumita-workspace-reviewgraphen/0d42d984-55de-424b-ad9a-e8b8f298b722/scratchpad/cargo-target"
PROBES = ("m8_foreign_macro_probe", "m8_foreign_safefn_probe")
ENV = {
    "PATH": "/home/rizumita/.cargo/bin:/usr/local/bin:/usr/bin:/bin",
    "HOME": "/home/rizumita",
    "CARGO_TARGET_DIR": CARGO_TARGET_DIR,
    "REVIEWGRAPHEN_TRUSTED_CARGO": TRUSTED_CARGO,
}


def sh(command: list[str], cwd: Path | None = None) -> int:
    return subprocess.run(command, cwd=cwd, capture_output=True, text=True, env=ENV).returncode


def probe_triplicate(scratch: Path) -> dict[str, str]:
    """Three runs per probe. Unanimity required, else `indeterminate`."""
    results: dict[str, str] = {}
    for probe in PROBES:
        source = EXP / "task2" / f"{probe}.rs"
        destination = scratch / "crates/reviewgraphen-ingest/tests" / f"{probe}.rs"
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(source.read_text(encoding="utf-8"), encoding="utf-8")
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
                ]
            )
            outcomes.append("pass" if code == 0 else "fail")
        results[probe] = outcomes[0] if len(set(outcomes)) == 1 else "indeterminate"
        results[f"{probe}_runs"] = outcomes
    return results


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: post_series.py <output-json>")
    report: dict = {
        "schema": "reviewgraphen.benchmark.m8_post_series.v1",
        "trials": {},
        "reference_trees": {},
    }

    trials = sorted(p.name for p in RUNS.glob("rep-*") if (p / "candidate.json").exists()
                    or (p / "final-content.txt").exists())
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
            ]
        )
        post = json.loads((result_dir / "verification.json").read_text())
        entry = {
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
        if scratch.exists():
            entry.update(probe_triplicate(scratch))
            patched = scratch / "crates/reviewgraphen-ingest/src/rust.rs"
            entry["result_sha256"] = subprocess.run(
                ["sha256sum", str(patched)], capture_output=True, text=True
            ).stdout.split()[0]
            subprocess.run(["rm", "-rf", str(scratch)], check=False)
        report["trials"][trial] = entry
        print(json.dumps({trial: entry}, indent=2, sort_keys=True), flush=True)

    # The four trees AMENDMENT-003 section 1 measured with single runs.
    references = {
        "pinned": None,
        "reference_fix": "reference",
        "task2_treatment_n1": "/tmp/m8-impl-local-v1-runs/task2-rerun-methodology",
        "task2_control_n1": "/tmp/m8-impl-local-v1-runs/task2-rerun-baseline",
    }
    for name, source in references.items():
        scratch = Path(f"/tmp/m8-post-ref-{name}")
        subprocess.run(["rm", "-rf", str(scratch)], check=False)
        sh(["bash", str(EXP / "scripts/make_scratch.sh"), str(scratch)])
        if source == "reference":
            sh(
                [
                    "python3",
                    str(EXP / "scripts/make_reference_candidate_task2.py"),
                    "/tmp/m8-post-ref-candidate",
                ]
            )
            subprocess.run(["rm", "-rf", str(scratch)], check=False)
            sh(
                [
                    "python3",
                    str(EXP / "scripts/apply_and_verify_task2.py"),
                    "/tmp/m8-post-ref-candidate",
                    str(scratch),
                ]
            )
        elif source is not None:
            subprocess.run(["rm", "-rf", str(scratch)], check=False)
            sh(
                [
                    "python3",
                    str(EXP / "scripts/apply_and_verify_task2.py"),
                    source,
                    str(scratch),
                ]
            )
        if scratch.exists():
            report["reference_trees"][name] = probe_triplicate(scratch)
            subprocess.run(["rm", "-rf", str(scratch)], check=False)
        print(json.dumps({name: report["reference_trees"].get(name)}, indent=2), flush=True)

    Path(sys.argv[1]).write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
