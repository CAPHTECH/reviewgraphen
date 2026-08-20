#!/usr/bin/env python3
"""Verifies one agentic trial.

The agent could write anywhere in its checkout, so verification does not run
in the agent's tree. It builds a fresh tree from the pinned revision, copies
in **only** `crates/reviewgraphen-ingest/src/rust.rs`, and installs the
harness-owned acceptance test there. Consequences, both intended:

- test-gaming is mechanically impossible: any edit the agent made to a test,
  or to any other file, is discarded rather than trusted;
- every such edit is still recorded, from the pre/post manifests, as
  `out_of_scope_changed_files` — discarded is not the same as unnoticed.

A trial whose loop ended mid-edit is still verified, because "what state did
it leave the tree in" is exactly the question. Its verdict is prefixed
`loop_incomplete_*` so it can never be read as a code failure.

Private CARGO_TARGET_DIR per trial (m8 AMENDMENT-005).

usage: verify_trial.py <result-dir>
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path("/home/rizumita/workspace/reviewgraphen")
M8 = ROOT / "benchmarks/m8-impl-local-v1"
EXP = ROOT / "benchmarks/m10-target-context-local-v1"
TARGET_FILE = "crates/reviewgraphen-ingest/src/rust.rs"
ACCEPTANCE = "crates/reviewgraphen-ingest/tests/m8_extern_block_shadow.rs"
TRUSTED_CARGO = "/home/rizumita/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo"


def run(command: list[str], target_dir: str, cwd: Path | None = None) -> dict:
    completed = subprocess.run(
        command,
        cwd=cwd,
        capture_output=True,
        text=True,
        env={
            "PATH": "/home/rizumita/.cargo/bin:/usr/local/bin:/usr/bin:/bin",
            "HOME": "/home/rizumita",
            "CARGO_TARGET_DIR": target_dir,
            "REVIEWGRAPHEN_TRUSTED_CARGO": TRUSTED_CARGO,
        },
    )
    return {
        "command": command,
        "exit_code": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def manifest(path: Path) -> dict[str, str]:
    entries: dict[str, str] = {}
    if not path.exists():
        return entries
    for line in path.read_text().splitlines():
        digest, _, name = line.partition("  ")
        if name:
            entries[name.strip()] = digest
    return entries


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: verify_trial.py <result-dir>")
    result = Path(sys.argv[1])
    trial = result.name
    report: dict = {
        "schema": "reviewgraphen.benchmark.m10_trial_verification.v1",
        "trial": trial,
        "loop_outcome": (result / "loop-outcome").read_text().strip()
        if (result / "loop-outcome").exists()
        else "unknown",
        "elapsed_seconds": int((result / "elapsed-seconds").read_text().strip())
        if (result / "elapsed-seconds").exists()
        else None,
    }

    before = manifest(result / "pre-loop-manifest.txt")
    after = manifest(result / "post-loop-manifest.txt")
    changed = sorted(
        name for name in set(before) | set(after) if before.get(name) != after.get(name)
    )
    report["changed_files"] = changed
    report["target_file_changed"] = f"./{TARGET_FILE}" in changed
    out_of_scope = [name for name in changed if name != f"./{TARGET_FILE}"]
    report["out_of_scope_changed_files"] = out_of_scope
    report["out_of_scope_change_count"] = len(out_of_scope)
    report["touched_a_test_file"] = any(
        "/tests/" in name or name.endswith("_test.rs") for name in out_of_scope
    )

    worktree = result / "worktree"
    patched = worktree / TARGET_FILE
    if not patched.exists():
        finish(result, report, "target_file_missing")

    verify_tree = Path(f"/tmp/m10-verify-{trial}")
    target_dir = f"/tmp/m10-verify-target-{trial}"
    subprocess.run(["rm", "-rf", str(verify_tree), target_dir], check=False)
    os.makedirs(target_dir, exist_ok=True)
    made = run(["bash", str(EXP / "scripts/make_scratch.sh"), str(verify_tree)], target_dir)
    report["verify_tree_created"] = made["exit_code"] == 0
    if made["exit_code"] != 0:
        report["verify_tree_error"] = made["stderr"]
        finish(result, report, "verify_tree_failed")

    pinned = (verify_tree / TARGET_FILE).read_text(encoding="utf-8")
    agent_version = patched.read_text(encoding="utf-8")
    report["result_unchanged_from_pinned"] = agent_version == pinned
    (result / "pinned-rust.rs").write_text(pinned, encoding="utf-8")
    (verify_tree / TARGET_FILE).write_text(agent_version, encoding="utf-8")
    (result / "patched-rust.rs").write_text(agent_version, encoding="utf-8")

    accept_src = M8 / "task2/m8_extern_block_shadow.rs"
    (verify_tree / ACCEPTANCE).parent.mkdir(parents=True, exist_ok=True)
    (verify_tree / ACCEPTANCE).write_text(
        accept_src.read_text(encoding="utf-8"), encoding="utf-8"
    )

    diff = run(
        ["diff", "-u", str(result / "pinned-rust.rs"), str(verify_tree / TARGET_FILE)],
        target_dir,
    )
    (result / "applied.diff").write_text(diff["stdout"], encoding="utf-8")
    report["diff_line_count"] = len(diff["stdout"].splitlines())

    build = run(["cargo", "build", "-p", "reviewgraphen-ingest"], target_dir, verify_tree)
    report["compiles"] = build["exit_code"] == 0
    report["build_errors"] = [
        line for line in build["stderr"].splitlines() if line.startswith("error")
    ][:6]
    report["build"] = build

    if report["compiles"]:
        test = run(["cargo", "test", "-p", "reviewgraphen-ingest"], target_dir, verify_tree)
        report["tests_pass"] = test["exit_code"] == 0
        report["test_result_lines"] = re.findall(
            r"test result: (\w+)\. (\d+) passed; (\d+) failed", test["stdout"] + test["stderr"]
        )
        report["test"] = test
        clippy = run(
            ["cargo", "clippy", "-p", "reviewgraphen-ingest", "--all-targets"],
            target_dir,
            verify_tree,
        )
        report["clippy_exit"] = clippy["exit_code"]
        report["clippy_warnings"] = len(re.findall(r"^warning: ", clippy["stderr"], re.M))
    else:
        report["tests_pass"] = None

    subprocess.run(["rm", "-rf", str(verify_tree), target_dir], check=False)

    loop_outcome = report["loop_outcome"]
    if loop_outcome != "completed":
        verdict = f"{loop_outcome}"
    elif report["result_unchanged_from_pinned"]:
        verdict = "no_change_made"
    elif not report["compiles"]:
        verdict = "does_not_compile"
    elif not report["tests_pass"]:
        verdict = "tests_failed"
    else:
        verdict = "verified"
    finish(result, report, verdict)


def finish(result: Path, report: dict, verdict: str) -> None:
    report["verdict"] = verdict
    (result / "verification.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    summary = {
        key: value
        for key, value in report.items()
        if key not in {"build", "test"}
    }
    print(json.dumps(summary, indent=2, sort_keys=True))
    print(f"VERDICT: {verdict}")


if __name__ == "__main__":
    main()
