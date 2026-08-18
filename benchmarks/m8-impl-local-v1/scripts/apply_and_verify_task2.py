#!/usr/bin/env python3
"""Applies one candidate's edits to a fresh isolated scratch copy and runs
the two checks that constitute this experiment's Evidence.

The candidate never touches the real worktree. A malformed, non-matching, or
truncated candidate can therefore leave nothing half-edited: the scratch copy
is created fresh here, and is the only thing ever written.

Steps, in order, each recorded whether or not the previous one succeeded:

1. extract one JSON object from the model's final content (ADR 0037
   extraction contract, script reused verbatim from m7-local-factorial-v3);
2. validate it against the candidate schema;
3. apply its edits by exact, unique literal substitution;
4. `cargo build -p reviewgraphen-cli`  -> question 2;
5. `cargo test  -p reviewgraphen-cli`  -> question 3;
6. `cargo clippy -p reviewgraphen-cli` -> recorded, not a gate.

usage: apply_and_verify_task2.py <result-dir> <fresh-scratch-dir>
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path("/home/rizumita/workspace/reviewgraphen/.claude/worktrees/agent-abbf383d1b02d8726")
EXP = ROOT / "benchmarks/m8-impl-local-v1"
TARGET_FILE = "crates/reviewgraphen-ingest/src/rust.rs"
TRUSTED_CARGO = "/home/rizumita/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo"
CARGO_TARGET_DIR = "/tmp/claude-1000/-home-rizumita-workspace-reviewgraphen/0d42d984-55de-424b-ad9a-e8b8f298b722/scratchpad/cargo-target"
EXPECTED_TESTS = 5
FAILING_TEST_ON_PINNED_REVISION = "block_local_extern_fn_shadows_a_same_named_module_function"

sys.path.insert(0, str(EXP / "scripts"))
from extract_candidate_json import extract  # noqa: E402


def run(command: list[str], cwd: Path | None = None) -> dict:
    completed = subprocess.run(
        command,
        cwd=cwd,
        capture_output=True,
        text=True,
        env={
            "PATH": "/home/rizumita/.cargo/bin:/usr/local/bin:/usr/bin:/bin",
            "HOME": "/home/rizumita",
            "CARGO_TARGET_DIR": CARGO_TARGET_DIR,
            # Ten of tests/m2.rs's forty tests require an externally admitted
            # cargo (docs/adr/0012); without it they fail on the pinned
            # revision too, which would make the Q3 gate meaningless. The
            # pinned 1.95.0 toolchain binary is used, exactly as
            # scripts/resolve-trusted-cargo.sh resolves it.
            "REVIEWGRAPHEN_TRUSTED_CARGO": TRUSTED_CARGO,
        },
    )
    return {
        "command": command,
        "exit_code": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: apply_and_verify_task2.py <result-dir> <fresh-scratch-dir>")
    result_dir = Path(sys.argv[1])
    scratch = Path(sys.argv[2])

    report: dict = {
        "schema": "reviewgraphen.benchmark.m8_impl_local_verification.v1",
        "result_dir": str(result_dir),
        "scratch_dir": str(scratch),
    }

    final = (result_dir / "final-content.txt").read_text(encoding="utf-8")
    if final.strip() == "":
        finish(result_dir, report, "no_final_content")

    try:
        candidate_text = extract(final)
        candidate = json.loads(candidate_text)
    except Exception as error:  # noqa: BLE001
        report["extraction_error"] = f"{type(error).__name__}: {error}"
        finish(result_dir, report, "candidate_not_extractable")

    (result_dir / "candidate.json").write_text(
        json.dumps(candidate, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    report["candidate_outcome"] = candidate.get("outcome")
    report["candidate_obligation_count"] = len(candidate.get("obligations") or [])
    report["candidate_evidence_statuses"] = sorted(
        {o.get("evidence_status") for o in (candidate.get("obligations") or [])}
    )
    report["candidate_declares_verified"] = any(
        o.get("evidence_status") == "verified" for o in (candidate.get("obligations") or [])
    )

    # The two arms answer to different contracts: the treatment's full
    # schema, and the control's minimal edits-only one. The candidate's own
    # `schema` field selects which, so a control that invented obligation
    # fields would fail validation rather than pass unnoticed.
    schema_file = {
        "reviewgraphen.benchmark.implementation_candidate_output.v1": "schemas/implementation-candidate-output.schema.json",
        "reviewgraphen.benchmark.implementation_candidate_edit.v1": "schemas/implementation-candidate-edit.schema.json",
    }.get(candidate.get("schema"), "schemas/implementation-candidate-output.schema.json")
    report["schema_validated_against"] = schema_file
    schema_check = run(
        [
            "python3",
            "-c",
            "import json,sys,jsonschema;"
            "schema=json.load(open(sys.argv[1]));"
            "instance=json.load(open(sys.argv[2]));"
            "jsonschema.validate(instance,schema);"
            "print('valid')",
            str(EXP / schema_file),
            str(result_dir / "candidate.json"),
        ]
    )
    report["schema_validation"] = schema_check

    edits = candidate.get("edits") or []
    if not edits:
        finish(result_dir, report, "no_edits_emitted")

    scratch_made = run(["bash", str(EXP / "scripts/make_scratch.sh"), str(scratch)])
    report["scratch_creation"] = scratch_made
    if scratch_made["exit_code"] != 0:
        finish(result_dir, report, "scratch_creation_failed")
    # make_scratch.sh installs task 1's acceptance test; task 2 needs its own.
    (scratch / "crates/reviewgraphen-ingest/tests").mkdir(parents=True, exist_ok=True)
    (scratch / "crates/reviewgraphen-ingest/tests/m8_extern_block_shadow.rs").write_text(
        (EXP / "task2/m8_extern_block_shadow.rs").read_text(encoding="utf-8"),
        encoding="utf-8",
    )

    target = scratch / TARGET_FILE
    text = target.read_text(encoding="utf-8")
    applied = []
    for index, edit in enumerate(edits):
        record: dict = {"index": index, "file": edit.get("file")}
        if edit.get("file") != TARGET_FILE:
            record["status"] = "rejected_wrong_file"
            applied.append(record)
            report["edit_application"] = applied
            finish(result_dir, report, "edit_touched_forbidden_file")
        if "full_content" in edit and edit["full_content"] is not None:
            text = edit["full_content"]
            record["status"] = "applied_full_content"
            record["bytes"] = len(text.encode("utf-8"))
            applied.append(record)
            continue
        old = edit.get("old")
        new = edit.get("new")
        if not isinstance(old, str) or not isinstance(new, str):
            record["status"] = "rejected_missing_old_or_new"
            applied.append(record)
            report["edit_application"] = applied
            finish(result_dir, report, "edit_malformed")
        occurrences = text.count(old)
        record["old_occurrences"] = occurrences
        record["old_bytes"] = len(old.encode("utf-8"))
        record["new_bytes"] = len(new.encode("utf-8"))
        if occurrences != 1:
            record["status"] = "rejected_anchor_not_unique"
            applied.append(record)
            report["edit_application"] = applied
            finish(result_dir, report, "edit_anchor_did_not_match")
        text = text.replace(old, new, 1)
        record["status"] = "applied"
        applied.append(record)

    report["edit_application"] = applied
    target.write_text(text, encoding="utf-8")
    (result_dir / "patched-lib.rs").write_text(text, encoding="utf-8")

    diff = run(["diff", "-u", str(ROOT / TARGET_FILE), str(target)])
    report["diff_line_count"] = len(diff["stdout"].splitlines())
    (result_dir / "applied.diff").write_text(diff["stdout"], encoding="utf-8")

    build = run(["cargo", "build", "-p", "reviewgraphen-ingest"], cwd=scratch)
    report["build"] = build
    report["compiles"] = build["exit_code"] == 0
    report["build_warning_count"] = len(re.findall(r"^warning: ", build["stderr"], re.M))

    if not report["compiles"]:
        finish(result_dir, report, "does_not_compile")

    test = run(["cargo", "test", "-p", "reviewgraphen-ingest"], cwd=scratch)
    report["test"] = test
    report["tests_pass"] = test["exit_code"] == 0
    summary = re.findall(
        r"test result: (\w+)\. (\d+) passed; (\d+) failed", test["stdout"] + test["stderr"]
    )
    report["test_result_lines"] = summary
    report["acceptance_test_still_failing"] = FAILING_TEST_ON_PINNED_REVISION in (
        test["stdout"] + test["stderr"]
    ) and not report["tests_pass"]

    clippy = run(
        ["cargo", "clippy", "-p", "reviewgraphen-ingest", "--all-targets"], cwd=scratch
    )
    report["clippy"] = {
        "exit_code": clippy["exit_code"],
        "warning_count": len(re.findall(r"^warning: ", clippy["stderr"], re.M)),
    }

    finish(result_dir, report, "verified" if report["tests_pass"] else "tests_failed")


def finish(result_dir: Path, report: dict, verdict: str) -> None:
    report["verdict"] = verdict
    (result_dir / "verification.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(json.dumps({k: v for k, v in report.items() if k not in {"build", "test"}}, indent=2, sort_keys=True))
    print(f"VERDICT: {verdict}")
    sys.exit(0 if verdict == "verified" else 1)


if __name__ == "__main__":
    main()
