#!/usr/bin/env python3
"""Run sequential parent-fails/fix-passes checks in disposable FSL clones."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import signal
import shutil
import subprocess
import tempfile
from pathlib import Path


FSL = Path("/home/rizumita/github/fsl")
MANIFEST = "rust/Cargo.toml"
TIMEOUT_STATUS = 124
INFRASTRUCTURE_PATTERNS = (
    b"failed to download from",
    b"failed to get `",
    b"could not resolve host",
    b"could not resolve hostname",
    b"no space left on device",
    b"out of diskspace",
)


class InfrastructureError(RuntimeError):
    pass


def canonical(value: object) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode()


def sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def infrastructure_failure(stderr: bytes) -> bool:
    lowered = stderr.lower()
    return any(pattern in lowered for pattern in INFRASTRUCTURE_PATTERNS)


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_bytes(canonical(value))
    temporary.replace(path)


def run(
    argv: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    stdin: bytes | None = None,
    timeout: int = 600,
) -> tuple[int, bytes, bytes]:
    process = subprocess.Popen(
        argv,
        cwd=cwd,
        env=env,
        stdin=subprocess.PIPE if stdin is not None else subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    try:
        stdout, stderr = process.communicate(input=stdin, timeout=timeout)
        return process.returncode, stdout, stderr
    except subprocess.TimeoutExpired as error:
        os.killpg(process.pid, signal.SIGKILL)
        stdout, stderr = process.communicate()
        return (
            TIMEOUT_STATUS,
            (error.stdout or b"") + stdout,
            (error.stderr or b"")
            + stderr
            + b"\n[reviewgraphen presence timeout]\n",
        )


def git_source(*args: str) -> bytes:
    status, stdout, stderr = run(["git", "-C", str(FSL), *args], timeout=120)
    if status != 0:
        raise RuntimeError(stderr.decode(errors="replace"))
    return stdout


def test_support_paths(parent: str, fix: str, crates: set[str]) -> list[str]:
    output = git_source("diff", "--name-only", "--find-renames=0", parent, fix)
    prefixes = tuple(f"rust/{crate}/tests/" for crate in sorted(crates))
    paths = sorted(
        line
        for line in output.decode().splitlines()
        if line.startswith(prefixes)
    )
    if not paths:
        raise RuntimeError("no test-only backport paths")
    return paths


def cargo_command(cargo: Path, option: dict[str, str]) -> list[str]:
    return [
        str(cargo),
        "test",
        "--quiet",
        "--manifest-path",
        MANIFEST,
        "-p",
        option["package"],
        "--test",
        option["test_bin"],
        option["test_name"],
        "--",
        "--exact",
    ]


def cargo_build_command(cargo: Path, option: dict[str, str]) -> list[str]:
    return [
        str(cargo),
        "test",
        "--quiet",
        "--manifest-path",
        MANIFEST,
        "-p",
        option["package"],
        "--test",
        option["test_bin"],
        "--no-run",
    ]


def execution(
    commit: str,
    tree: str,
    command: list[str],
    status: int,
    stdout: bytes,
    stderr: bytes,
) -> dict[str, object]:
    combined = b"STDOUT\n" + stdout + b"STDERR\n" + stderr
    return {
        "commit_hash": "git:" + commit,
        "tree_hash": "git:" + tree,
        "command": ["cargo", *command[1:]],
        "working_directory": "repository",
        "exit_status": status,
        "stdout_sha256": sha256(stdout),
        "stderr_sha256": sha256(stderr),
        "combined_artifact_sha256": sha256(combined),
    }


def process_candidate(
    record: dict[str, object],
    cargo: Path,
    target_dir: Path,
    artifact_dir: Path,
    work_parent: Path,
) -> dict[str, object]:
    fix = str(record["fix_commit"]).removeprefix("git:")
    parent = str(record["parent_commit"]).removeprefix("git:")
    options = list(record["test_options"])
    crates = {str(option["test_path"]).split("/", 2)[1] for option in options}
    paths = test_support_paths(parent, fix, crates)
    if any("/src/" in path for path in paths):
        raise RuntimeError("test backport included production source")
    patch = git_source("diff", "--binary", "--find-renames=0", parent, fix, "--", *paths)
    if not patch:
        raise RuntimeError("empty test-only backport")

    work_root = Path(
        tempfile.mkdtemp(prefix="m7-real-v2-presence-", dir=work_parent)
    )
    fix_clone = work_root / "fix"
    parent_clone = work_root / "parent"
    attempts: list[dict[str, object]] = []
    builds: list[dict[str, object]] = []
    baseline_attempts: list[dict[str, object]] = []
    baseline_builds: list[dict[str, object]] = []
    selected: dict[str, object] | None = None
    try:
        for clone, revision in ((fix_clone, fix), (parent_clone, parent)):
            status, _, stderr = run(
                ["git", "clone", "--shared", "--no-checkout", "--quiet", str(FSL), str(clone)],
                timeout=120,
            )
            if status != 0:
                raise InfrastructureError(
                    f"clone failed: {sha256(stderr)}"
                )
            status, _, stderr = run(
                ["git", "-C", str(clone), "checkout", "--detach", "--quiet", revision],
                timeout=120,
            )
            if status != 0:
                raise InfrastructureError(
                    f"checkout failed: {sha256(stderr)}"
                )
        environment = os.environ.copy()
        environment["CARGO_TARGET_DIR"] = str(target_dir)
        environment["CARGO_BUILD_JOBS"] = "2"
        fix_tree = git_source("rev-parse", f"{fix}^{{tree}}").decode().strip()
        parent_tree = git_source("rev-parse", f"{parent}^{{tree}}").decode().strip()
        groups: dict[tuple[str, str], list[dict[str, str]]] = {}
        for option_value in options:
            option = {str(key): str(value) for key, value in option_value.items()}
            groups.setdefault((option["package"], option["test_bin"]), []).append(option)
        baseline_statuses: dict[tuple[str, str, str], int] = {}
        baseline_executions: dict[tuple[str, str, str], dict[str, object]] = {}
        baseline_outputs: dict[tuple[str, str, str], tuple[bytes, bytes]] = {}
        for group_key in sorted(groups):
            strengthened = [
                option
                for option in groups[group_key]
                if option.get("test_change_kind") == "strengthened_existing"
            ]
            if not strengthened:
                continue
            build_command = cargo_build_command(cargo, strengthened[0])
            build_status, build_stdout, build_stderr = run(
                build_command, cwd=parent_clone, env=environment, timeout=1800
            )
            if infrastructure_failure(build_stderr):
                raise InfrastructureError(
                    f"baseline parent build infrastructure failure for {fix}: {sha256(build_stderr)}"
                )
            baseline_builds.append(
                {
                    "package": group_key[0],
                    "test_bin": group_key[1],
                    "exit_status": build_status,
                    "stdout_sha256": sha256(build_stdout),
                    "stderr_sha256": sha256(build_stderr),
                }
            )
            if build_status != 0:
                continue
            for option in strengthened:
                command = cargo_command(cargo, option)
                baseline_status, baseline_stdout, baseline_stderr = run(
                    command, cwd=parent_clone, env=environment
                )
                if infrastructure_failure(baseline_stderr):
                    raise InfrastructureError(
                        f"baseline parent test infrastructure failure for {fix}: {sha256(baseline_stderr)}"
                    )
                key = (option["package"], option["test_bin"], option["test_name"])
                baseline_statuses[key] = baseline_status
                baseline_executions[key] = execution(
                    parent,
                    parent_tree,
                    command,
                    baseline_status,
                    baseline_stdout,
                    baseline_stderr,
                )
                baseline_outputs[key] = (baseline_stdout, baseline_stderr)
                baseline_attempts.append(
                    {
                        "test_selector": f'{option["test_bin"]}::{option["test_name"]}',
                        "exit_status": baseline_status,
                        "stdout_sha256": sha256(baseline_stdout),
                        "stderr_sha256": sha256(baseline_stderr),
                    }
                )

        status, _, stderr = run(
            ["git", "-C", str(parent_clone), "apply", "--whitespace=nowarn", "-"],
            stdin=patch,
            timeout=120,
        )
        if status != 0:
            raise RuntimeError(f"test backport failed: {stderr.decode(errors='replace')}")

        for group_key in sorted(groups):
            group_options = groups[group_key]
            build_command = cargo_build_command(cargo, group_options[0])
            fix_build_status, fix_build_stdout, fix_build_stderr = run(
                build_command, cwd=fix_clone, env=environment, timeout=1800
            )
            if infrastructure_failure(fix_build_stderr):
                raise InfrastructureError(
                    f"fix build infrastructure failure for {fix}: {sha256(fix_build_stderr)}"
                )
            parent_build_status = -1
            parent_build_stdout = b""
            parent_build_stderr = b""
            if fix_build_status == 0:
                parent_build_status, parent_build_stdout, parent_build_stderr = run(
                    build_command, cwd=parent_clone, env=environment, timeout=1800
                )
                if infrastructure_failure(parent_build_stderr):
                    raise InfrastructureError(
                        f"parent build infrastructure failure for {fix}: {sha256(parent_build_stderr)}"
                    )
            builds.append(
                {
                    "package": group_key[0],
                    "test_bin": group_key[1],
                    "fix_exit_status": fix_build_status,
                    "parent_exit_status": parent_build_status,
                    "fix_stdout_sha256": sha256(fix_build_stdout),
                    "fix_stderr_sha256": sha256(fix_build_stderr),
                    "parent_stdout_sha256": sha256(parent_build_stdout),
                    "parent_stderr_sha256": sha256(parent_build_stderr),
                }
            )
            if fix_build_status != 0 or parent_build_status != 0:
                continue
            for option in group_options:
                change_kind = option.get("test_change_kind", "added_exact")
                baseline_key = (
                    option["package"], option["test_bin"], option["test_name"]
                )
                if (
                    change_kind == "strengthened_existing"
                    and baseline_statuses.get(baseline_key) != 0
                ):
                    continue
                command = cargo_command(cargo, option)
                fix_status, fix_stdout, fix_stderr = run(
                    command, cwd=fix_clone, env=environment
                )
                if infrastructure_failure(fix_stderr):
                    raise InfrastructureError(
                        f"fix test infrastructure failure for {fix}: {sha256(fix_stderr)}"
                    )
                parent_status = -1
                parent_stdout = b""
                parent_stderr = b""
                if fix_status == 0:
                    parent_status, parent_stdout, parent_stderr = run(
                        command, cwd=parent_clone, env=environment
                    )
                    if infrastructure_failure(parent_stderr):
                        raise InfrastructureError(
                            f"parent test infrastructure failure for {fix}: {sha256(parent_stderr)}"
                        )
                attempt = {
                    "test_selector": f'{option["test_bin"]}::{option["test_name"]}',
                    "fix_exit_status": fix_status,
                    "parent_exit_status": parent_status,
                    "fix_stdout_sha256": sha256(fix_stdout),
                    "fix_stderr_sha256": sha256(fix_stderr),
                    "parent_stdout_sha256": sha256(parent_stdout),
                    "parent_stderr_sha256": sha256(parent_stderr),
                }
                attempts.append(attempt)
                if fix_status == 0 and parent_status not in (-1, 0, TIMEOUT_STATUS):
                    slug = hashlib.sha256(bytes.fromhex(fix)).hexdigest()
                    selected_dir = artifact_dir / slug
                    selected_dir.mkdir(parents=True, exist_ok=True)
                    (selected_dir / "fix.stdout").write_bytes(fix_stdout)
                    (selected_dir / "fix.stderr").write_bytes(fix_stderr)
                    (selected_dir / "parent.stdout").write_bytes(parent_stdout)
                    (selected_dir / "parent.stderr").write_bytes(parent_stderr)
                    if change_kind == "strengthened_existing":
                        original_stdout, original_stderr = baseline_outputs[baseline_key]
                        (selected_dir / "original-parent.stdout").write_bytes(
                            original_stdout
                        )
                        (selected_dir / "original-parent.stderr").write_bytes(
                            original_stderr
                        )
                    selected = {
                        "schema": "reviewgraphen.benchmark.regression_presence_evidence.v1",
                        "test_selector": f'{option["test_bin"]}::{option["test_name"]}',
                        "test_path": option["test_path"],
                        "test_source_sha256": option.get(
                            "test_source_sha256", option.get("fix_test_source_sha256")
                        ),
                        "test_change_kind": change_kind,
                        "strategy": (
                            "strengthened_regression_test_backported_to_parent"
                            if change_kind == "strengthened_existing"
                            else "fix_regression_test_backported_to_parent"
                        ),
                        "backported_paths": paths,
                        "parent_run": execution(
                            parent,
                            parent_tree,
                            command,
                            parent_status,
                            parent_stdout,
                            parent_stderr,
                        ),
                        "fix_run": execution(
                            fix,
                            fix_tree,
                            command,
                            fix_status,
                            fix_stdout,
                            fix_stderr,
                        ),
                        "artifact_directory": slug,
                    }
                    if change_kind == "strengthened_existing":
                        selected["original_parent_run"] = baseline_executions[
                            baseline_key
                        ]
                    break
            if selected is not None:
                break
    finally:
        shutil.rmtree(work_root, ignore_errors=True)

    return {
        "schema": "reviewgraphen.benchmark.m7_real_v2_presence_result.v1",
        "fix_commit": "git:" + fix,
        "parent_commit": "git:" + parent,
        "split_hash": record["split_hash"],
        "status": "eligible" if selected is not None else "ineligible",
        "attempts": attempts,
        "builds": builds,
        "baseline_attempts": baseline_attempts,
        "baseline_builds": baseline_builds,
        "presence_evidence": selected,
        "features": record["features"],
        "production_paths": record["production_paths"],
    }


def finalize(frame: dict[str, object], output: Path) -> None:
    candidates = sorted(
        (
            record
            for record in frame["records"]
            if record["frame_status"] == "candidate"
        ),
        key=lambda value: str(value["split_hash"]),
    )
    results: list[dict[str, object]] = []
    for record in candidates:
        fix = str(record["fix_commit"]).removeprefix("git:")
        path = output / "results" / f"{fix}.json"
        if path.is_file():
            results.append(json.loads(path.read_bytes()))
    eligible = sorted(
        (result for result in results if result["status"] == "eligible"),
        key=lambda value: str(value["split_hash"]),
    )
    calibration = eligible[:30] if len(eligible) >= 30 else []
    holdout = eligible[30:] if len(eligible) >= 30 else []
    write_json(
        output / "presence-index.json",
        {
            "schema": "reviewgraphen.benchmark.m7_real_v2_presence_index.v1",
            "structural_candidate_count": len(candidates),
            "processed_count": len(results),
            "eligible_count": len(eligible),
            "ineligible_count": len(results) - len(eligible),
            "complete": len(results) == len(candidates),
            "calibration_count": len(calibration),
            "holdout_count": len(holdout),
            "calibration_fix_commits": [value["fix_commit"] for value in calibration],
            "holdout_fix_commits": [value["fix_commit"] for value in holdout],
            "results": results,
        },
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--frame", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cargo", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--work-root", type=Path, required=True)
    parser.add_argument("--max-new", type=int)
    args = parser.parse_args()
    if not args.cargo.is_absolute() or not args.cargo.is_file():
        raise SystemExit("--cargo must be an absolute existing file")
    if not args.target_dir.is_absolute() or not args.work_root.is_absolute():
        raise SystemExit("--target-dir and --work-root must be absolute")
    frame = json.loads(args.frame.read_bytes())
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "results").mkdir(exist_ok=True)
    (args.output / "artifacts").mkdir(exist_ok=True)
    args.target_dir.mkdir(parents=True, exist_ok=True)
    args.work_root.mkdir(parents=True, exist_ok=True)
    candidates = sorted(
        (
            record
            for record in frame["records"]
            if record["frame_status"] == "candidate"
        ),
        key=lambda value: str(value["split_hash"]),
    )
    completed = 0
    for ordinal, record in enumerate(candidates, 1):
        fix = str(record["fix_commit"]).removeprefix("git:")
        destination = args.output / "results" / f"{fix}.json"
        if destination.is_file():
            continue
        if args.max_new is not None and completed >= args.max_new:
            break
        print(f"presence {ordinal}/{len(candidates)} {fix}", flush=True)
        try:
            result = process_candidate(
                record,
                args.cargo,
                args.target_dir,
                args.output / "artifacts",
                args.work_root,
            )
        except InfrastructureError as error:
            print(f"infrastructure_error {fix} {error}", flush=True)
            raise
        except RuntimeError as error:
            result = {
                "schema": "reviewgraphen.benchmark.m7_real_v2_presence_result.v1",
                "fix_commit": record["fix_commit"],
                "parent_commit": record["parent_commit"],
                "split_hash": record["split_hash"],
                "status": "ineligible",
                "attempts": [],
                "presence_evidence": None,
                "features": record["features"],
                "production_paths": record["production_paths"],
                "ineligibility_error": str(error),
            }
        write_json(destination, result)
        print(f"result {fix} {result['status']}", flush=True)
        completed += 1
        finalize(frame, args.output)
    finalize(frame, args.output)


if __name__ == "__main__":
    main()
