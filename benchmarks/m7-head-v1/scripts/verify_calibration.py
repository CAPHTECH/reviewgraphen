#!/usr/bin/env python3
"""Mechanically verify blind test/fix-generator calibration proposals."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import signal
import shutil
import subprocess
import tempfile
from pathlib import Path, PurePosixPath


FSL = Path("/home/rizumita/github/fsl")
PROPOSAL_SCHEMA = "reviewgraphen.benchmark.test_fix_proposal.v1"
RESULT_SCHEMA = "reviewgraphen.benchmark.m7_head_calibration_result.v1"
TIMEOUT_STATUS = 124
MAX_PATCH_BYTES = 262_144
REQUIRED_KEYS = {
    "schema",
    "proposal_id",
    "outcome",
    "test_target_id",
    "test_patch",
    "fix_patch",
    "expected_behavior",
    "unverifiable_reason",
    "spec_queries",
}


class InfrastructureError(RuntimeError):
    pass


class ProposalError(RuntimeError):
    pass


def sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def canonical(value: object) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(canonical(value))


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
            (error.stderr or b"") + stderr + b"\n[m7-head verifier timeout]\n",
        )


def infrastructure_failure(stderr: bytes) -> bool:
    text = stderr.decode(errors="replace").lower()
    needles = (
        "no space left on device",
        "failed to get `",
        "failed to download",
        "could not resolve host",
        "network failure",
        "connection reset",
        "connection refused",
        "timed out while downloading",
    )
    return any(value in text for value in needles)


def validate_proposal(value: object, unit: dict[str, object]) -> dict[str, object]:
    if not isinstance(value, dict) or set(value) != REQUIRED_KEYS:
        raise ProposalError("proposal_shape")
    if value["schema"] != PROPOSAL_SCHEMA or value["proposal_id"] != unit["proposal_id"]:
        raise ProposalError("proposal_identity")
    for key in (
        "proposal_id",
        "outcome",
        "test_target_id",
        "test_patch",
        "fix_patch",
        "expected_behavior",
        "unverifiable_reason",
    ):
        if not isinstance(value[key], str):
            raise ProposalError("proposal_string_type")
    if (
        not isinstance(value["spec_queries"], list)
        or len(value["spec_queries"]) > 16
        or any(not isinstance(item, str) or len(item) > 512 for item in value["spec_queries"])
    ):
        raise ProposalError("proposal_spec_queries")
    if value["outcome"] == "unverifiable":
        if any(value[key] for key in ("test_target_id", "test_patch", "fix_patch", "expected_behavior")):
            raise ProposalError("unverifiable_carries_proposal")
        if not value["unverifiable_reason"]:
            raise ProposalError("unverifiable_without_reason")
        return value
    if value["outcome"] != "proposal":
        raise ProposalError("proposal_outcome")
    if value["unverifiable_reason"]:
        raise ProposalError("proposal_carries_unverifiable_reason")
    if any(not value[key] for key in ("test_target_id", "test_patch", "fix_patch", "expected_behavior")):
        raise ProposalError("proposal_missing_content")
    if any(len(value[key].encode()) > MAX_PATCH_BYTES for key in ("test_patch", "fix_patch")):
        raise ProposalError("proposal_patch_size")
    if value["test_target_id"] not in {
        target["test_target_id"] for target in unit["allowed_test_targets"]
    }:
        raise ProposalError("proposal_test_target")
    return value


def replay_record(benchmark: Path, record: Path) -> bytes:
    status, stdout, stderr = run([str(benchmark), "replay-process-reviewer", str(record)])
    if status != 0:
        raise ProposalError("process_record_replay")
    try:
        stored = json.loads(record.read_bytes())["raw_response"].encode()
    except (json.JSONDecodeError, KeyError, AttributeError):
        raise ProposalError("process_record_shape") from None
    if stdout != stored:
        raise ProposalError("process_record_replay_bytes")
    return stdout


def clone_at(destination: Path, revision: str) -> None:
    status, _, stderr = run(
        ["git", "clone", "--shared", "--no-checkout", "--quiet", str(FSL), str(destination)],
        timeout=120,
    )
    if status != 0:
        raise InfrastructureError(f"clone:{sha256(stderr)}")
    status, _, stderr = run(
        ["git", "-C", str(destination), "checkout", "--detach", "--quiet", revision],
        timeout=120,
    )
    if status != 0:
        raise InfrastructureError(f"checkout:{sha256(stderr)}")


def normalized_path(value: str) -> bool:
    path = PurePosixPath(value)
    return bool(value) and not path.is_absolute() and all(part not in ("", ".", "..") for part in path.parts)


def patch_paths(repository: Path, patch: bytes) -> set[str]:
    status, stdout, _ = run(
        ["git", "-C", str(repository), "apply", "--numstat", "-"],
        stdin=patch,
        timeout=120,
    )
    if status != 0:
        raise ProposalError("patch_numstat")
    paths: set[str] = set()
    for line in stdout.decode(errors="strict").splitlines():
        fields = line.split("\t", 2)
        if len(fields) != 3 or fields[0] == "-" or fields[1] == "-":
            raise ProposalError("patch_numstat_shape")
        path = fields[2]
        if not normalized_path(path):
            raise ProposalError("patch_path")
        paths.add(path)
    if not paths:
        raise ProposalError("empty_patch")
    return paths


def apply_patch(repository: Path, patch: bytes) -> None:
    status, _, _ = run(
        ["git", "-C", str(repository), "apply", "--check", "-"],
        stdin=patch,
        timeout=120,
    )
    if status != 0:
        raise ProposalError("patch_check")
    status, _, _ = run(
        ["git", "-C", str(repository), "apply", "--whitespace=nowarn", "-"],
        stdin=patch,
        timeout=120,
    )
    if status != 0:
        raise ProposalError("patch_apply")


def execution(command: list[str], status: int, stdout: bytes, stderr: bytes) -> dict[str, object]:
    return {
        "command": ["cargo", *command[1:]],
        "exit_status": status,
        "stdout_sha256": sha256(stdout),
        "stderr_sha256": sha256(stderr),
        "combined_sha256": sha256(b"STDOUT\n" + stdout + b"STDERR\n" + stderr),
    }


def observe(
    label: str,
    repository: Path,
    target: dict[str, str],
    cargo: Path,
    environment: dict[str, str],
    artifacts: Path,
) -> dict[str, object]:
    common = [
        str(cargo),
        "test",
        "--quiet",
        "--manifest-path",
        "rust/Cargo.toml",
        "-p",
        target["package"],
        "--test",
        target["test_bin"],
    ]
    phases: dict[str, object] = {}
    commands = {
        "build": [*common, "--no-run"],
        "list": [*common, target["test_name"], "--", "--exact", "--list"],
        "test": [*common, target["test_name"], "--", "--exact"],
    }
    for phase, command in commands.items():
        status, stdout, stderr = run(
            command,
            cwd=repository,
            env=environment,
            timeout=1800 if phase == "build" else 600,
        )
        if infrastructure_failure(stderr):
            raise InfrastructureError(f"{label}_{phase}:{sha256(stderr)}")
        phases[phase] = execution(command, status, stdout, stderr)
        (artifacts / f"{label}.{phase}.stdout").write_bytes(stdout)
        (artifacts / f"{label}.{phase}.stderr").write_bytes(stderr)
        if phase == "build" and status != 0:
            break
        if phase == "list":
            expected = f'{target["test_name"]}: test'
            listed = {line.strip() for line in stdout.decode(errors="replace").splitlines()}
            phases["selector_present"] = expected in listed
            if status != 0 or expected not in listed:
                break
    return phases


def classify_observations(observations: dict[str, object]) -> tuple[str, str]:
    for label in ("parent", "canonical_fix", "proposed_fix"):
        value = observations.get(label)
        if not isinstance(value, dict) or "build" not in value:
            return "not_verified", f"{label}_test_not_built"
        if value["build"]["exit_status"] != 0:
            return "not_verified", f"{label}_test_not_built"
        if not value.get("selector_present"):
            return "not_verified", f"{label}_selector_missing"
        if "test" not in value:
            return "not_verified", f"{label}_test_not_run"
    if observations["parent"]["test"]["exit_status"] in (0, TIMEOUT_STATUS):
        return "not_verified", "parent_did_not_fail_at_runtime"
    if observations["canonical_fix"]["test"]["exit_status"] != 0:
        return "not_verified", "canonical_fix_did_not_pass"
    if observations["proposed_fix"]["test"]["exit_status"] != 0:
        return "not_verified", "proposed_fix_did_not_pass"
    return "verified", "all_three_observations_satisfied"


def verify_unit(
    unit: dict[str, object],
    record: Path,
    benchmark: Path,
    cargo: Path,
    target_dir: Path,
    work_parent: Path,
    artifact_root: Path,
) -> dict[str, object]:
    calibration_id = str(unit["calibration_id"])
    base = {
        "schema": RESULT_SCHEMA,
        "calibration_id": calibration_id,
        "proposal_id": unit["proposal_id"],
    }
    try:
        raw = replay_record(benchmark, record)
        proposal = validate_proposal(json.loads(raw), unit)
    except (ProposalError, json.JSONDecodeError) as error:
        return {**base, "disposition": "unverifiable", "reason": str(error)}
    artifacts = artifact_root / calibration_id
    artifacts.mkdir(parents=True, exist_ok=True)
    write_json(artifacts / "proposal.json", proposal)
    base["proposal_sha256"] = sha256(canonical(proposal))
    base["proposal_outcome"] = proposal["outcome"]
    if proposal["outcome"] == "unverifiable":
        return {
            **base,
            "disposition": "unverifiable",
            "reason": "generator_abstained",
            "generator_reason": proposal["unverifiable_reason"],
        }
    target = next(
        value
        for value in unit["allowed_test_targets"]
        if value["test_target_id"] == proposal["test_target_id"]
    )
    test_patch = proposal["test_patch"].encode()
    fix_patch = proposal["fix_patch"].encode()
    (artifacts / "test.patch").write_bytes(test_patch)
    (artifacts / "fix.patch").write_bytes(fix_patch)
    work_root = Path(tempfile.mkdtemp(prefix=f"{calibration_id}-", dir=work_parent))
    parent_repo = work_root / "parent"
    canonical_repo = work_root / "canonical-fix"
    proposed_repo = work_root / "proposed-fix"
    observations: dict[str, object] = {}
    try:
        parent = str(unit["parent_commit"]).removeprefix("git:")
        fix = str(unit["fix_commit"]).removeprefix("git:")
        clone_at(parent_repo, parent)
        clone_at(canonical_repo, fix)
        clone_at(proposed_repo, parent)
        if any(
            run(["git", "-C", str(repo), "cat-file", "-e", f'HEAD:{target["test_path"]}'])[0]
            == 0
            for repo in (parent_repo, canonical_repo)
        ):
            raise ProposalError("generated_test_path_already_exists")
        if patch_paths(parent_repo, test_patch) != {target["test_path"]}:
            raise ProposalError("test_patch_path_boundary")
        allowed_fix_paths = set(unit["selected_production_paths"])
        fix_paths = patch_paths(proposed_repo, fix_patch)
        if not fix_paths or not fix_paths <= allowed_fix_paths:
            raise ProposalError("fix_patch_path_boundary")
        for repo in (parent_repo, canonical_repo, proposed_repo):
            apply_patch(repo, test_patch)
        generated_test = proposed_repo / target["test_path"]
        before_fix_hash = sha256(generated_test.read_bytes())
        apply_patch(proposed_repo, fix_patch)
        if sha256(generated_test.read_bytes()) != before_fix_hash:
            raise ProposalError("fix_patch_changed_test")
        environment = os.environ.copy()
        environment["CARGO_TARGET_DIR"] = str(target_dir)
        environment["CARGO_BUILD_JOBS"] = "2"
        observations["parent"] = observe(
            "parent", parent_repo, target, cargo, environment, artifacts
        )
        observations["canonical_fix"] = observe(
            "canonical-fix", canonical_repo, target, cargo, environment, artifacts
        )
        observations["proposed_fix"] = observe(
            "proposed-fix", proposed_repo, target, cargo, environment, artifacts
        )
        disposition, reason = classify_observations(observations)
        return {
            **base,
            "disposition": disposition,
            "reason": reason,
            "test_target_id": target["test_target_id"],
            "test_path": target["test_path"],
            "test_patch_sha256": sha256(test_patch),
            "fix_patch_sha256": sha256(fix_patch),
            "expected_behavior": proposal["expected_behavior"],
            "spec_queries": proposal["spec_queries"],
            "observations": observations,
        }
    except InfrastructureError as error:
        return {**base, "disposition": "unverifiable", "reason": "infrastructure", "detail": str(error)}
    except (ProposalError, OSError, UnicodeDecodeError) as error:
        return {**base, "disposition": "unverifiable", "reason": str(error)}
    finally:
        shutil.rmtree(work_root, ignore_errors=True)


def finalize(inventory: dict[str, object], output: Path) -> dict[str, object]:
    results = []
    for unit in inventory["units"]:
        path = output / "results" / f'{unit["calibration_id"]}.json'
        if path.is_file():
            results.append(json.loads(path.read_bytes()))
    counts = {value: 0 for value in ("verified", "not_verified", "unverifiable")}
    for result in results:
        counts[result["disposition"]] += 1
    summary = {
        "schema": "reviewgraphen.benchmark.m7_head_calibration_summary.v1",
        "unit_count": len(inventory["units"]),
        "processed_count": len(results),
        "counts": counts,
        "minimum_verified_to_proceed": 9,
        "complete": len(results) == len(inventory["units"]),
        "proceed_to_head_review": len(results) == len(inventory["units"])
        and counts["verified"] >= 9,
        "results": results,
    }
    write_json(output / "calibration-summary.json", summary)
    return summary


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--inventory", type=Path, required=True)
    parser.add_argument("--records-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--benchmark-bin", type=Path, required=True)
    parser.add_argument("--cargo", type=Path, required=True)
    parser.add_argument("--target-dir", type=Path, required=True)
    parser.add_argument("--work-root", type=Path, required=True)
    parser.add_argument("--max-new", type=int)
    args = parser.parse_args()
    for path in (args.output, args.target_dir, args.work_root):
        if not path.is_absolute():
            raise SystemExit("output, target-dir, and work-root must be absolute")
        path.mkdir(parents=True, exist_ok=True)
    if not args.benchmark_bin.is_absolute() or not args.benchmark_bin.is_file():
        raise SystemExit("benchmark-bin must be an absolute existing file")
    if not args.cargo.is_absolute() or not args.cargo.is_file():
        raise SystemExit("cargo must be an absolute existing file")
    inventory = json.loads(args.inventory.read_bytes())
    (args.output / "results").mkdir(exist_ok=True)
    (args.output / "artifacts").mkdir(exist_ok=True)
    completed = 0
    for unit in inventory["units"]:
        calibration_id = str(unit["calibration_id"])
        destination = args.output / "results" / f"{calibration_id}.json"
        if destination.is_file():
            continue
        if args.max_new is not None and completed >= args.max_new:
            break
        record = args.records_root / calibration_id / "record.json"
        if not record.is_file():
            result = {
                "schema": RESULT_SCHEMA,
                "calibration_id": calibration_id,
                "proposal_id": unit["proposal_id"],
                "disposition": "unverifiable",
                "reason": "missing_process_record",
            }
        else:
            result = verify_unit(
                unit,
                record,
                args.benchmark_bin,
                args.cargo,
                args.target_dir,
                args.work_root,
                args.output / "artifacts",
            )
        write_json(destination, result)
        print(f"{calibration_id} {result['disposition']} {result['reason']}", flush=True)
        completed += 1
        finalize(inventory, args.output)
    finalize(inventory, args.output)


if __name__ == "__main__":
    main()
