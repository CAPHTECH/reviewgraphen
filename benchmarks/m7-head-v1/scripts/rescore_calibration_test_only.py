#!/usr/bin/env python3
"""Rescore frozen M7 HEAD calibration records using test-only observations."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import tempfile
from pathlib import Path

import verify_calibration as common


RESULT_SCHEMA = "reviewgraphen.benchmark.m7_head_test_only_rescore_result.v1"
SUMMARY_SCHEMA = "reviewgraphen.benchmark.m7_head_test_only_rescore_summary.v1"
MINIMUM_TO_PROCEED = 13
HUNK = re.compile(r"@@ -0,0 \+1,([0-9]+) @@")
PROPOSAL_KEYS = common.REQUIRED_KEYS


def reconstruct_test(
    proposal: dict[str, object],
    unit: dict[str, object],
) -> tuple[dict[str, str], bytes, int]:
    patch = proposal["test_patch"]
    if not isinstance(patch, str) or not patch or "\r" in patch:
        raise common.ProposalError("test_source_shape")
    if patch.endswith("\n"):
        patch = patch[:-1]
    lines = patch.split("\n")
    target = next(
        (
            value
            for value in unit["allowed_test_targets"]
            if value["test_target_id"] == proposal["test_target_id"]
        ),
        None,
    )
    if target is None:
        raise common.ProposalError("test_target")
    path = target["test_path"]
    expected = [
        f"diff --git a/{path} b/{path}",
        "new file mode 100644",
        "--- /dev/null",
        f"+++ b/{path}",
    ]
    if len(lines) < 6 or lines[:4] != expected:
        raise common.ProposalError("test_source_headers")
    match = HUNK.fullmatch(lines[4])
    if match is None:
        raise common.ProposalError("test_source_hunk")
    payload = lines[5:]
    if not payload or any(not line.startswith("+") for line in payload):
        raise common.ProposalError("test_source_payload")
    source = ("\n".join(line[1:] for line in payload) + "\n").encode()
    if len(source) > common.MAX_PATCH_BYTES:
        raise common.ProposalError("test_source_size")
    return target, source, int(match.group(1))


def classify(observations: dict[str, object]) -> tuple[str, str]:
    for label in ("parent", "canonical_fix"):
        value = observations.get(label)
        if not isinstance(value, dict) or "build" not in value:
            return "not_verified", f"{label}_test_not_built"
        if value["build"]["exit_status"] != 0:
            return "not_verified", f"{label}_test_not_built"
        if not value.get("selector_present"):
            return "not_verified", f"{label}_selector_missing"
        if "test" not in value:
            return "not_verified", f"{label}_test_not_run"
    if observations["parent"]["test"]["exit_status"] in (0, common.TIMEOUT_STATUS):
        return "not_verified", "parent_did_not_fail_at_runtime"
    if observations["canonical_fix"]["test"]["exit_status"] != 0:
        return "not_verified", "canonical_fix_did_not_pass"
    return "verified", "both_test_only_observations_satisfied"


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
        "rescore_id": "test-only-rescore-1",
        "calibration_id": calibration_id,
        "proposal_id": unit["proposal_id"],
    }
    try:
        raw = common.replay_record(benchmark, record)
        proposal = common.validate_proposal(json.loads(raw), unit)
    except (common.ProposalError, json.JSONDecodeError) as error:
        return {**base, "disposition": "unverifiable", "reason": str(error)}
    base["proposal_sha256"] = common.sha256(common.canonical(proposal))
    if proposal["outcome"] != "proposal":
        return {
            **base,
            "disposition": "unverifiable",
            "reason": "generator_abstained",
        }
    try:
        target, source, declared_lines = reconstruct_test(proposal, unit)
    except common.ProposalError as error:
        return {**base, "disposition": "unverifiable", "reason": str(error)}

    artifacts = artifact_root / calibration_id
    artifacts.mkdir(parents=True, exist_ok=True)
    (artifacts / "generated-test.rs").write_bytes(source)
    common.write_json(artifacts / "proposal.json", proposal)

    work_root = Path(tempfile.mkdtemp(prefix=f"{calibration_id}-", dir=work_parent))
    parent_repo = work_root / "parent"
    canonical_repo = work_root / "canonical-fix"
    observations: dict[str, object] = {}
    try:
        parent = str(unit["parent_commit"]).removeprefix("git:")
        fix = str(unit["fix_commit"]).removeprefix("git:")
        common.clone_at(parent_repo, parent)
        common.clone_at(canonical_repo, fix)
        for repository in (parent_repo, canonical_repo):
            if (
                common.run(
                    [
                        "git",
                        "-C",
                        str(repository),
                        "cat-file",
                        "-e",
                        f'HEAD:{target["test_path"]}',
                    ]
                )[0]
                == 0
            ):
                raise common.ProposalError("generated_test_path_already_exists")
            destination = repository / target["test_path"]
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(source)

        environment = os.environ.copy()
        environment["CARGO_TARGET_DIR"] = str(target_dir)
        environment["CARGO_BUILD_JOBS"] = "2"
        observations["parent"] = common.observe(
            "parent", parent_repo, target, cargo, environment, artifacts
        )
        observations["canonical_fix"] = common.observe(
            "canonical-fix", canonical_repo, target, cargo, environment, artifacts
        )
        disposition, reason = classify(observations)
        return {
            **base,
            "disposition": disposition,
            "reason": reason,
            "test_target_id": target["test_target_id"],
            "test_path": target["test_path"],
            "test_source_sha256": common.sha256(source),
            "declared_hunk_line_count": declared_lines,
            "reconstructed_line_count": source.count(b"\n"),
            "hunk_count_matched": declared_lines == source.count(b"\n"),
            "observations": observations,
        }
    except common.InfrastructureError as error:
        return {
            **base,
            "disposition": "unverifiable",
            "reason": "infrastructure",
            "detail": str(error),
        }
    except (common.ProposalError, OSError, UnicodeDecodeError) as error:
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
    complete = len(results) == len(inventory["units"])
    summary = {
        "schema": SUMMARY_SCHEMA,
        "rescore_id": "test-only-rescore-1",
        "source_attempt": "m7-head-v1-calibration-attempt-1",
        "unit_count": len(inventory["units"]),
        "processed_count": len(results),
        "counts": counts,
        "minimum_verified_to_proceed": MINIMUM_TO_PROCEED,
        "complete": complete,
        "proceed_to_head_review": complete
        and counts["verified"] >= MINIMUM_TO_PROCEED,
        "semantic_attempts_added": 0,
        "results": results,
    }
    common.write_json(output / "rescore-summary.json", summary)
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
    if inventory.get("unit_count") != 20:
        raise SystemExit("rescore requires the frozen twenty-unit inventory")
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
                "rescore_id": "test-only-rescore-1",
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
        common.write_json(destination, result)
        print(
            f"{calibration_id} {result['disposition']} {result['reason']}",
            flush=True,
        )
        completed += 1
        finalize(inventory, args.output)
    finalize(inventory, args.output)


if __name__ == "__main__":
    main()
