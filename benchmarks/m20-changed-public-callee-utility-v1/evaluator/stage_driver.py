"""Authenticated closed-root drivers for m20 controls, Stage 1, and Stage 2A."""
from __future__ import annotations

import os
import shutil
import stat
import time
from pathlib import Path

from .artifacts import verify_run
from .canonical import canonical_bytes, hash_json, parse_json_bytes, sha256_bytes, stable_id
from .pipeline import CONTEXT_HASH, CONTEXT_POLICY_ID, PipelineError, RUN

EXPERIMENT_ID = "m20-changed-public-callee-utility-v1"
REVIEWER_ADAPTER = "m20.fixed-reviewer-process.v1"
JUDGE_ADAPTER = "m20.fixed-judge-process.v1"
REVIEWER_PATH = "/usr/local/bin/m20-reviewer-backend"
JUDGE_PATH = "/usr/local/bin/m20-judge-backend"
REVIEWER_OUTPUT_TOKENS = 12_000
REVIEWER_TIMEOUT_SECONDS = 900
JUDGE_TIMEOUT_SECONDS = 90
STAGE1_WALL_SECONDS = 21_600
STAGE1_MODEL_SECONDS = 18_900
STAGE2A_WALL_SECONDS = 86_400
STAGE2A_MODEL_SECONDS = 75_600
ARM_SEED = "m20-arm-order-v1"
JUDGE_SEED = "m20-judge-permutation-v1"
_LABELS = {"clean_refactor_control", "not_control", "unable"}


def _closed(value, fields, code):
    if not isinstance(value, dict) or set(value) != set(fields):
        raise PipelineError(code, 2)
    return value


def _read(path: Path, code="authenticated_json_invalid") -> tuple[dict, bytes]:
    if path.is_symlink() or not path.is_file():
        raise PipelineError("authenticated_path_invalid", 2)
    raw = path.read_bytes()
    try:
        value = parse_json_bytes(raw)
    except ValueError as error:
        raise PipelineError(code, 2) from error
    if canonical_bytes(value) != raw:
        raise PipelineError("authenticated_json_noncanonical", 2)
    return value, raw


def _safe_root(path: Path, code="stage_root_invalid") -> Path:
    if path.is_symlink() or not path.is_dir() or not path.is_absolute():
        raise PipelineError(code, 2)
    return path


def _files(root: Path) -> dict[str, bytes]:
    output = {}
    for path in root.rglob("*"):
        if path.is_symlink():
            raise PipelineError("artifact_symlink_invalid", 2)
        if path.is_dir():
            continue
        if not path.is_file():
            raise PipelineError("artifact_type_invalid", 2)
        relative = path.relative_to(root).as_posix()
        if relative in output:
            raise PipelineError("artifact_path_duplicate", 2)
        output[relative] = path.read_bytes()
    return output


def _manifest_rows(files: dict[str, bytes]) -> list[dict]:
    rows = [{"path": path, "byte_length": len(data), "sha256": sha256_bytes(data)} for path, data in sorted(files.items(), key=lambda item: item[0].encode())]
    rows.append({"path": "artifact-manifest.v1.json", "byte_length": None, "sha256": "self-described-by-manifest-bytes"})
    return rows


def _write_manifest(root: Path, schema: str) -> dict:
    files = _files(root)
    if "artifact-manifest.v1.json" in files:
        raise PipelineError("artifact_manifest_preexists", 4)
    value = {"schema": schema, "files": _manifest_rows(files)}
    (root / "artifact-manifest.v1.json").write_bytes(canonical_bytes(value))
    return value


def _verify_manifest(root: Path, expected_schema: str) -> tuple[dict, str]:
    files = _files(root)
    raw = files.get("artifact-manifest.v1.json")
    if raw is None:
        raise PipelineError("artifact_manifest_missing", 2)
    try:
        manifest = parse_json_bytes(raw)
    except ValueError as error:
        raise PipelineError("artifact_manifest_invalid", 2) from error
    _closed(manifest, {"schema", "files"}, "artifact_manifest_invalid")
    payload = {key: value for key, value in files.items() if key != "artifact-manifest.v1.json"}
    expected_rows = ([{"path":path,"sha256":sha256_bytes(data)} for path,data in sorted(payload.items(),key=lambda item:item[0].encode())] + [{"path":"artifact-manifest.v1.json","sha256":"self-described-by-manifest-bytes"}]) if expected_schema == "m20.stage0-artifact-manifest.v1" else _manifest_rows(payload)
    if manifest["schema"] != expected_schema or manifest["files"] != expected_rows:
        raise PipelineError("artifact_manifest_mismatch", 2)
    return manifest, sha256_bytes(raw)


def _selection(value: dict) -> dict:
    fields = {"schema", "experiment_id", "eligible_cluster_ids", "hash_ordered_cluster_ids", "stage1_cluster_ids", "stage2a_cumulative_cluster_ids", "selection_sha256"}
    _closed(value, fields, "selection_contract_invalid")
    body = {key: value[key] for key in value if key != "selection_sha256"}
    lists = [value[name] for name in ("eligible_cluster_ids", "hash_ordered_cluster_ids", "stage1_cluster_ids", "stage2a_cumulative_cluster_ids")]
    if value["schema"] != "m20.stage0-selection.v1" or value["experiment_id"] != EXPERIMENT_ID or value["selection_sha256"] != hash_json(body):
        raise PipelineError("selection_hash_mismatch", 2)
    if any(not isinstance(rows, list) or not all(isinstance(item, str) and item for item in rows) or len(rows) != len(set(rows)) for rows in lists):
        raise PipelineError("selection_membership_invalid", 2)
    eligible, ordered, first, cumulative = lists
    if eligible != sorted(eligible, key=str.encode) or set(eligible) != set(ordered) or first != ordered[:10] or cumulative != ordered[:40] or len(first) != 10 or len(cumulative) != 40:
        raise PipelineError("selection_membership_invalid", 2)
    return value


def _stage0_root(selection_path: Path) -> tuple[Path, dict, str, dict]:
    if selection_path.name != "stage0-selection.v1.json" or not selection_path.is_absolute():
        raise PipelineError("stage0_selection_path_invalid", 2)
    root = _safe_root(selection_path.parent, "stage0_root_invalid")
    manifest, manifest_hash = _verify_manifest(root, "m20.stage0-artifact-manifest.v1")
    selection, selection_raw = _read(selection_path)
    _selection(selection)
    result, _ = _read(root / "stage0-result.v1.json")
    gates = result.get("gates") if isinstance(result, dict) else None
    if result.get("schema") != "m20.stage0-result.v1" or result.get("model_calls") != 0 or not isinstance(gates, list) or not gates or any(row.get("passed") is not True for row in gates if isinstance(row, dict)) or len(gates) != 7:
        raise PipelineError("stage0_result_invalid", 2)
    if not any(row.get("path") == selection_path.name and row.get("sha256") == sha256_bytes(selection_raw) for row in manifest["files"]):
        raise PipelineError("selection_not_manifest_bound", 2)
    return root, selection, manifest_hash, result


def _labeler(value: dict, selection: dict) -> dict:
    fields = {"schema", "experiment_id", "selection_sha256", "labeler_identity", "did_not_implement_slice", "labels"}
    _closed(value, fields, "control_labeler_contract_invalid")
    if value["schema"] != "m20.control_labeler_record.v1" or value["experiment_id"] != EXPERIMENT_ID or value["selection_sha256"] != selection["selection_sha256"] or value["did_not_implement_slice"] is not True or not isinstance(value["labeler_identity"], str) or not value["labeler_identity"]:
        raise PipelineError("control_labeler_contract_invalid", 2)
    labels = value["labels"]
    if not isinstance(labels, list) or len(labels) != len(selection["eligible_cluster_ids"]):
        raise PipelineError("control_labels_incomplete", 2)
    identifiers = []
    for row in labels:
        _closed(row, {"commit_cluster_id", "label", "source_citations"}, "control_label_invalid")
        if row["label"] not in _LABELS or not isinstance(row["commit_cluster_id"], str):
            raise PipelineError("control_label_invalid", 2)
        citations = row["source_citations"]
        if not isinstance(citations, list) or not citations:
            raise PipelineError("control_label_source_missing", 2)
        for citation in citations:
            _closed(citation, {"path", "start_line", "end_line"}, "control_label_source_invalid")
            if not isinstance(citation["path"], str) or not citation["path"] or isinstance(citation["start_line"], bool) or not isinstance(citation["start_line"], int) or not isinstance(citation["end_line"], int) or citation["start_line"] < 1 or citation["end_line"] < citation["start_line"]:
                raise PipelineError("control_label_source_invalid", 2)
        identifiers.append(row["commit_cluster_id"])
    if identifiers != selection["eligible_cluster_ids"]:
        raise PipelineError("control_labels_membership_invalid", 2)
    return value


def seal_controls(selection_path: str | Path, labeler_1_path: str | Path, labeler_2_path: str | Path, new_root: str | Path) -> dict:
    selection_input = Path(selection_path)
    first_input = Path(labeler_1_path)
    second_input = Path(labeler_2_path)
    if any(path.is_symlink() or not path.is_absolute() for path in (selection_input, first_input, second_input)):
        raise PipelineError("authenticated_path_invalid", 2)
    _, selection, stage0_manifest_hash, _ = _stage0_root(selection_input)
    first, first_raw = _read(first_input)
    second, second_raw = _read(second_input)
    _labeler(first, selection); _labeler(second, selection)
    if first["labeler_identity"] == second["labeler_identity"]:
        raise PipelineError("control_labeler_identity_duplicate", 2)
    root = Path(new_root)
    if root.exists() or root.is_symlink():
        raise PipelineError("output_root_exists", 2)
    root.mkdir(parents=False)
    (root / "labeler-1.json").write_bytes(first_raw)
    (root / "labeler-2.json").write_bytes(second_raw)
    rows = []
    for left, right in zip(first["labels"], second["labels"]):
        resolution = left["label"] if left["label"] == right["label"] and left["label"] != "unable" else "unresolved"
        rows.append({"commit_cluster_id": left["commit_cluster_id"], "labeler_1": left["label"], "labeler_2": right["label"], "resolution": resolution})
    combined = {"schema": "m20.control_labels.v1", "experiment_id": EXPERIMENT_ID, "stage0_selection_path": str(selection_input), "selection_sha256": selection["selection_sha256"], "stage0_artifact_manifest_sha256": stage0_manifest_hash, "labeler_1_sha256": sha256_bytes(first_raw), "labeler_2_sha256": sha256_bytes(second_raw), "labels": rows}
    (root / "control-labels.v1.json").write_bytes(canonical_bytes(combined))
    _write_manifest(root, "m20.control-artifact-manifest.v1")
    return combined


def _controls(path: Path, selection: dict) -> tuple[Path, dict, str]:
    if path.name != "control-labels.v1.json" or not path.is_absolute():
        raise PipelineError("control_manifest_path_invalid", 2)
    root = _safe_root(path.parent, "control_root_invalid")
    _verify_manifest(root, "m20.control-artifact-manifest.v1")
    value, raw = _read(path)
    _closed(value, {"schema", "experiment_id", "stage0_selection_path", "selection_sha256", "stage0_artifact_manifest_sha256", "labeler_1_sha256", "labeler_2_sha256", "labels"}, "control_manifest_invalid")
    _, bound_selection, bound_stage0_hash, _ = _stage0_root(Path(value["stage0_selection_path"]))
    first, first_raw = _read(root / "labeler-1.json"); second, second_raw = _read(root / "labeler-2.json")
    _labeler(first, selection); _labeler(second, selection)
    if value["schema"] != "m20.control_labels.v1" or value["experiment_id"] != EXPERIMENT_ID or value["selection_sha256"] != selection["selection_sha256"] or value["selection_sha256"] != bound_selection["selection_sha256"] or value["stage0_artifact_manifest_sha256"] != bound_stage0_hash or value["labeler_1_sha256"] != sha256_bytes(first_raw) or value["labeler_2_sha256"] != sha256_bytes(second_raw):
        raise PipelineError("control_manifest_mismatch", 2)
    expected = []
    for left, right in zip(first["labels"], second["labels"]):
        expected.append({"commit_cluster_id": left["commit_cluster_id"], "labeler_1": left["label"], "labeler_2": right["label"], "resolution": left["label"] if left["label"] == right["label"] and left["label"] != "unable" else "unresolved"})
    if value["labels"] != expected:
        raise PipelineError("control_manifest_mismatch", 2)
    return root, value, sha256_bytes(raw)


def _active_tuple() -> dict:
    benchmark = Path(__file__).resolve().parent.parent
    registration, _ = _read(benchmark / "preregistration.json")
    hashes = registration.get("arm_neutral_contracts", {}).get("freeze_hashes", {})
    keys = ("freeze_manifest_sha256", "evaluator_bundle_sha256", "evaluator_execution_sha256")
    if any(not isinstance(hashes.get(key), str) or not hashes[key] for key in keys):
        raise PipelineError("active_freeze_tuple_invalid", 3)
    return {key: hashes[key] for key in keys}


def _cluster_dir(root: Path, unit_id: str) -> Path:
    return root / "clusters" / unit_id.rsplit(":", 1)[-1]


def _unit_source(stage0_root: Path, unit_id: str) -> dict:
    directory = _cluster_dir(stage0_root, unit_id)
    builds = []
    for number in (1, 2):
        build_root = directory / f"build-{number}"
        value, _ = _read(build_root / "cluster-result.v1.json")
        relative = value.get("frozen_obligation_path")
        if not isinstance(relative, str) or Path(relative).is_absolute() or any(part in {"", ".", ".."} for part in Path(relative).parts):
            raise PipelineError("frozen_obligation_locator_invalid", 2)
        obligation_path = build_root / relative
        obligation, raw = _read(obligation_path)
        if sha256_bytes(raw) != value.get("frozen_obligation_sha256"):
            raise PipelineError("frozen_obligation_hash_mismatch", 2)
        builds.append((value, raw, obligation_path))
    if canonical_bytes(builds[0][0]) != canonical_bytes(builds[1][0]) or builds[0][1] != builds[1][1]:
        raise PipelineError("stage0_build_pair_mismatch", 2)
    value, raw, path = builds[0]
    required = ("repository_root", "base_commit_oid", "head_commit_oid", "selected_obligation_id", "frozen_obligation_sha256")
    if any(not isinstance(value.get(key), str) or not value[key] for key in required) or value.get("model_eligible") is not True:
        raise PipelineError("selected_unit_invalid", 2)
    if value["selected_obligation_id"] not in value.get("subject_retained_obligation_ids", []) or value["selected_obligation_id"] not in value.get("applicable_obligation_ids", []):
        raise PipelineError("selected_obligation_membership_invalid", 2)
    return {"unit_id": unit_id, "repository_root": value["repository_root"], "base_commit_oid": value["base_commit_oid"], "head_commit_oid": value["head_commit_oid"], "obligation_id": value["selected_obligation_id"], "frozen_obligation_path": str(path.resolve()), "frozen_obligation_sha256": sha256_bytes(raw)}


def _transport_identity() -> dict:
    output = {}
    for name, text in (("reviewer", REVIEWER_PATH), ("judge", JUDGE_PATH)):
        path = Path(text)
        try:
            status = path.lstat()
        except OSError as error:
            raise PipelineError("backend_adapter_unavailable", 2) from error
        if not stat.S_ISREG(status.st_mode) or path.is_symlink() or status.st_mode & 0o111 == 0:
            raise PipelineError("backend_adapter_unavailable", 2)
        output[name] = {"adapter_id": REVIEWER_ADAPTER if name == "reviewer" else JUDGE_ADAPTER, "path": text, "sha256": sha256_bytes(path.read_bytes())}
    return output


def _launch_preimage(unit: dict, selection_hash: str, stage: str, rank: int) -> dict:
    return {"schema": "m20.pipeline_launch.v2", "experiment_id": EXPERIMENT_ID, "unit_id": unit["unit_id"], "repository_root": unit["repository_root"], "base_commit_oid": unit["base_commit_oid"], "head_commit_oid": unit["head_commit_oid"], "frozen_obligation_path": unit["frozen_obligation_path"], "frozen_obligation_sha256": unit["frozen_obligation_sha256"], "selection_manifest_sha256": selection_hash, "selection_membership": {"stage": stage, "cumulative_rank": rank}, "context_policy_id": CONTEXT_POLICY_ID, "context_policy_sha256": CONTEXT_HASH}


def _manifest(stage: str, stage0_root: Path, stage0_manifest_hash: str, selection: dict, controls_path: Path, control_hash: str, units: list[dict], transport: dict, predecessor: dict | None) -> dict:
    start = 1 if stage == "stage1" else 11
    memberships = [{"unit_id": unit["unit_id"], "cumulative_rank": start + index} for index, unit in enumerate(units)]
    preimages = [{**_launch_preimage(unit, selection["selection_sha256"], stage, row["cumulative_rank"]), "stage_manifest_path": None} for unit, row in zip(units, memberships)]
    return {"schema": "m20.model_stage_manifest.v1", "experiment_id": EXPERIMENT_ID, "stage": stage, "active_freeze": _active_tuple(), "source_stage0_root": str(stage0_root), "stage0_artifact_manifest_sha256": stage0_manifest_hash, "selection_manifest_sha256": selection["selection_sha256"], "control_manifest_path": str(controls_path), "control_manifest_sha256": control_hash, "ordered_membership": memberships, "units": units, "packet_contract": {"schema": "arm-neutral.source-grounded-packet@3", "context_policy_id": CONTEXT_POLICY_ID, "context_policy_sha256": CONTEXT_HASH, "admitted_source_byte_ceiling": 65_536}, "budget_contract": {"reviewer_output_tokens": REVIEWER_OUTPUT_TOKENS, "reviewer_timeout_seconds": REVIEWER_TIMEOUT_SECONDS, "judge_timeout_seconds": JUDGE_TIMEOUT_SECONDS}, "public_seeds": {"arm_order": ARM_SEED, "judge_permutation": JUDGE_SEED}, "fixed_transports": transport, "launch_preimages": preimages, "predecessor": predecessor}


def _launch(preimage: dict, stage_manifest_path: Path, stage_manifest_hash: str) -> dict:
    return {**{key: value for key, value in preimage.items() if key != "stage_manifest_path"}, "stage_manifest_path": str(stage_manifest_path), "stage_manifest_sha256": stage_manifest_hash}


def _cell(run_root: Path, unit: dict) -> dict:
    verification = verify_run(run_root)
    if verification.get("ok") is not True:
        raise PipelineError("unit_run_invalid", 4)
    pair, _ = _read(run_root / "pair.json")
    primary, _ = _read(run_root / "primary.json")
    task_id = pair["task_id"]
    arm_a = stable_id("hidden-arm", {"task_id": task_id, "construction_kind": "baseline_diff"})
    arm_b = stable_id("hidden-arm", {"task_id": task_id, "construction_kind": "subject_windows"})
    values = {row["hidden_arm_id"]: int(row["completed"] is True) for row in primary["scores"]}
    if set(values) != {arm_a, arm_b}:
        raise PipelineError("primary_arm_mapping_invalid", 4)
    return {"unit_id": unit["unit_id"], "repository_root": unit["repository_root"], "A": values[arm_a], "B": values[arm_b], "run_seal_id": verification["run_id"]}


def _reduce(stage: str, cells: list[dict], controls: dict, calls: dict, duration_seconds: str) -> dict:
    n00 = sum(row["A"] == 0 and row["B"] == 0 for row in cells); b = sum(row["A"] == 0 and row["B"] == 1 for row in cells); c = sum(row["A"] == 1 and row["B"] == 0 for row in cells); n11 = sum(row["A"] == 1 and row["B"] == 1 for row in cells)
    repositories = []
    for repository in sorted({row["repository_root"] for row in cells}, key=str.encode):
        subset = [row for row in cells if row["repository_root"] == repository]
        repositories.append({"repository_root": repository, "n": len(subset), "b": sum(row["A"] == 0 and row["B"] == 1 for row in subset), "c": sum(row["A"] == 1 and row["B"] == 0 for row in subset)})
    leave_one_out = [{"excluded_repository_root": row["repository_root"], "n": len(cells) - row["n"], "b": b - row["b"], "c": c - row["c"]} for row in repositories]
    threshold = b >= (8 if stage == "stage1" else 18) and c <= (1 if stage == "stage1" else 7)
    unresolved = sum(row["resolution"] == "unresolved" for row in controls["labels"])
    reasons = [] if threshold else ["paired_threshold_not_met"]
    decision = ("advance" if threshold else "stop") if stage == "stage1" else ("success" if threshold else "failure")
    return {"schema": "m20.model_stage_result.v1", "stage": stage, "cells": cells, "n": len(cells), "n00": n00, "b": b, "c": c, "n11": n11, "repository_cells": repositories, "leave_one_repository_out": leave_one_out, "control_summary": {"eligible": len(controls["labels"]), "unresolved": unresolved}, "model_calls": calls, "budget": {"reviewer_output_tokens": REVIEWER_OUTPUT_TOKENS, "reviewer_timeout_seconds": REVIEWER_TIMEOUT_SECONDS, "judge_timeout_seconds": JUDGE_TIMEOUT_SECONDS, "authorized_model_seconds": STAGE1_MODEL_SECONDS if stage == "stage1" else STAGE2A_MODEL_SECONDS, "wall_envelope_seconds": STAGE1_WALL_SECONDS if stage == "stage1" else STAGE2A_WALL_SECONDS, "observed_active_seconds": duration_seconds}, "decision": decision, "reasons": reasons}


def _run_units(root: Path, manifest: dict, stage_manifest_path: Path, stage_manifest_hash: str, transport, start_rank: int, started: float, wall_seconds: int) -> tuple[list[dict], dict]:
    cells = []; reviewer_calls = judge_calls = 0
    for offset, (unit, preimage) in enumerate(zip(manifest["units"], manifest["launch_preimages"])):
        if time.monotonic() - started > wall_seconds:
            raise PipelineError("stage_wall_envelope_exceeded", 4)
        rank = start_rank + offset
        directory = root / "units" / f"{rank:04d}-{unit['unit_id'].rsplit(':', 1)[-1]}"
        result = RUN(_launch(preimage, stage_manifest_path, stage_manifest_hash), transport, directory)
        calls = result.get("model_call_count", 3 if result.get("pipeline_terminal_state") == "sealed" else 0)
        reviewer_calls += 2 if calls else 0; judge_calls += 1 if calls else 0
        cells.append(_cell(directory, unit) if calls else {"unit_id": unit["unit_id"], "repository_root": unit["repository_root"], "A": 0, "B": 0, "run_seal_id": result["run_seal_id"]})
    return cells, {"reviewer": reviewer_calls, "judge": judge_calls}


def stage1(selection_path: str | Path, new_root: str | Path, controls_path: str | Path, transport) -> dict:
    from .stage0_driver import _production_freeze_gate
    _production_freeze_gate()
    selection_input = Path(selection_path)
    controls_input = Path(controls_path)
    if any(path.is_symlink() or not path.is_absolute() for path in (selection_input, controls_input)):
        raise PipelineError("authenticated_path_invalid", 2)
    stage0_root, selection, stage0_manifest_hash, _ = _stage0_root(selection_input)
    _, controls, control_hash = _controls(controls_input, selection)
    identity = _transport_identity()
    if transport.descriptor() != {"reviewer": REVIEWER_ADAPTER, "judge": JUDGE_ADAPTER}:
        raise PipelineError("backend_adapter_mismatch", 2)
    units = [_unit_source(stage0_root, unit_id) for unit_id in selection["stage1_cluster_ids"]]
    root = Path(new_root)
    if root.exists() or root.is_symlink(): raise PipelineError("output_root_exists", 2)
    root.mkdir(parents=False); (root / "selection").mkdir(); (root / "controls").mkdir(); (root / "units").mkdir()
    shutil.copyfile(selection_input, root / "selection/stage0-selection.v1.json")
    shutil.copyfile(controls_input, root / "controls/control-labels.v1.json")
    manifest = _manifest("stage1", stage0_root, stage0_manifest_hash, selection, controls_input, control_hash, units, identity, None)
    manifest_path = (root / "stage-manifest.v1.json").resolve(); manifest_path.write_bytes(canonical_bytes(manifest)); manifest_hash = sha256_bytes(manifest_path.read_bytes())
    started = time.monotonic(); cells, calls = _run_units(root, manifest, manifest_path, manifest_hash, transport, 1, started, STAGE1_WALL_SECONDS)
    if calls != {"reviewer": 20, "judge": 10}: raise PipelineError("stage1_call_rectangle_invalid", 4)
    elapsed=time.monotonic()-started
    if elapsed > STAGE1_WALL_SECONDS: raise PipelineError("stage_wall_envelope_exceeded",4)
    result = _reduce("stage1", cells, controls, calls, f"{elapsed:.6f}")
    (root / "stage-result.v1.json").write_bytes(canonical_bytes(result)); _write_manifest(root, "m20.model-stage-artifact-manifest.v1")
    return result


def _verify_model_stage(root: Path, expected_stage: str | None = None) -> tuple[dict, dict, str]:
    _safe_root(root); _, artifact_hash = _verify_manifest(root, "m20.model-stage-artifact-manifest.v1")
    manifest, manifest_raw = _read(root / "stage-manifest.v1.json"); result, result_raw = _read(root / "stage-result.v1.json")
    _closed(manifest,{"schema","experiment_id","stage","active_freeze","source_stage0_root","stage0_artifact_manifest_sha256","selection_manifest_sha256","control_manifest_path","control_manifest_sha256","ordered_membership","units","packet_contract","budget_contract","public_seeds","fixed_transports","launch_preimages","predecessor"},"model_stage_manifest_invalid")
    if manifest.get("schema") != "m20.model_stage_manifest.v1" or result.get("schema") != "m20.model_stage_result.v1" or manifest.get("stage") != result.get("stage") or expected_stage is not None and manifest.get("stage") != expected_stage:
        raise PipelineError("model_stage_contract_invalid", 2)
    stage = manifest["stage"]; start = 1 if stage == "stage1" else 11
    expected_top={"stage-manifest.v1.json","stage-result.v1.json","artifact-manifest.v1.json","units"} | ({"selection","controls"} if stage=="stage1" else {"predecessor"})
    if {path.name for path in root.iterdir()} != expected_top: raise PipelineError("stage_closed_layout_invalid",2)
    stage0_root, selection, stage0_hash, _ = _stage0_root(Path(manifest["source_stage0_root"]) / "stage0-selection.v1.json")
    _, controls, control_hash = _controls(Path(manifest["control_manifest_path"]),selection)
    if manifest["active_freeze"] != _active_tuple() or manifest["stage0_artifact_manifest_sha256"] != stage0_hash or manifest["selection_manifest_sha256"] != selection["selection_sha256"] or manifest["control_manifest_sha256"] != control_hash or manifest["packet_contract"] != {"schema":"arm-neutral.source-grounded-packet@3","context_policy_id":CONTEXT_POLICY_ID,"context_policy_sha256":CONTEXT_HASH,"admitted_source_byte_ceiling":65_536} or manifest["budget_contract"] != {"reviewer_output_tokens":12_000,"reviewer_timeout_seconds":900,"judge_timeout_seconds":90} or manifest["public_seeds"] != {"arm_order":ARM_SEED,"judge_permutation":JUDGE_SEED} or manifest["fixed_transports"] != _transport_identity(): raise PipelineError("model_stage_tuple_mismatch",2)
    expected_ids=selection["stage1_cluster_ids"] if stage=="stage1" else selection["stage2a_cumulative_cluster_ids"][10:40]
    expected_membership=[{"unit_id":unit_id,"cumulative_rank":start+index} for index,unit_id in enumerate(expected_ids)]
    expected_unit_records=[_unit_source(stage0_root,unit_id) for unit_id in expected_ids]
    expected_preimages=[{**_launch_preimage(unit,selection["selection_sha256"],stage,row["cumulative_rank"]),"stage_manifest_path":None} for unit,row in zip(expected_unit_records,expected_membership)]
    if manifest["ordered_membership"] != expected_membership or manifest["units"] != expected_unit_records or manifest["launch_preimages"] != expected_preimages: raise PipelineError("stage_membership_mismatch",2)
    if stage=="stage1":
        if (root/"selection/stage0-selection.v1.json").read_bytes() != (stage0_root/"stage0-selection.v1.json").read_bytes() or (root/"controls/control-labels.v1.json").read_bytes() != Path(manifest["control_manifest_path"]).read_bytes() or manifest["predecessor"] is not None: raise PipelineError("stage_import_mismatch",2)
    expected_units = [f"units/{start + index:04d}-{unit['unit_id'].rsplit(':', 1)[-1]}" for index, unit in enumerate(manifest["units"])]
    actual_units = sorted(path.relative_to(root).as_posix() for path in (root / "units").iterdir() if path.is_dir())
    if actual_units != expected_units:
        raise PipelineError("stage_unit_layout_invalid", 2)
    cells = [_cell(root / relative, unit) for relative, unit in zip(expected_units, manifest["units"])]
    if stage == "stage2a":
        predecessor = root / "predecessor/stage1"
        _, predecessor_result, predecessor_hash = _verify_model_stage(predecessor, "stage1")
        expected_predecessor={"artifact_manifest_sha256":predecessor_hash,"stage_result_sha256":sha256_bytes((predecessor/"stage-result.v1.json").read_bytes())}
        if manifest["predecessor"] != expected_predecessor or predecessor_result["decision"] != "advance": raise PipelineError("stage_predecessor_mismatch",2)
        cells = predecessor_result["cells"] + cells
    controls, _ = _read(Path(manifest["control_manifest_path"]))
    expected = _reduce(stage, cells, controls, result["model_calls"], result["budget"]["observed_active_seconds"])
    if expected != result:
        raise PipelineError("stage_reduction_mismatch", 2)
    if stage == "stage1" and len(cells) != 10 or stage == "stage2a" and len(cells) != 40:
        raise PipelineError("stage_pair_count_invalid", 2)
    if result["model_calls"] != ({"reviewer":20,"judge":10} if stage=="stage1" else {"reviewer":80,"judge":40}): raise PipelineError("stage_call_rectangle_invalid",2)
    return manifest, result, artifact_hash


def stage2a(stage1_root: str | Path, new_root: str | Path, transport) -> dict:
    from .stage0_driver import _production_freeze_gate
    _production_freeze_gate()
    predecessor_root = Path(stage1_root)
    if predecessor_root.is_symlink() or not predecessor_root.is_absolute():
        raise PipelineError("stage_root_invalid", 2)
    predecessor_manifest, predecessor_result, predecessor_hash = _verify_model_stage(predecessor_root, "stage1")
    if predecessor_result["decision"] != "advance": raise PipelineError("stage1_not_advanced", 2)
    selection_path = Path(predecessor_manifest["source_stage0_root"]) / "stage0-selection.v1.json"
    stage0_root, selection, stage0_manifest_hash, _ = _stage0_root(selection_path)
    controls_path = Path(predecessor_manifest["control_manifest_path"]); _, controls, control_hash = _controls(controls_path, selection)
    identity = _transport_identity()
    if identity != predecessor_manifest["fixed_transports"] or transport.descriptor() != {"reviewer": REVIEWER_ADAPTER, "judge": JUDGE_ADAPTER}: raise PipelineError("stage2a_tuple_mismatch", 2)
    units = [_unit_source(stage0_root, unit_id) for unit_id in selection["stage2a_cumulative_cluster_ids"][10:40]]
    predecessor_active=float(predecessor_result["budget"]["observed_active_seconds"]); remaining_wall=STAGE2A_WALL_SECONDS-predecessor_active
    if remaining_wall <= 0: raise PipelineError("stage2a_cumulative_wall_exceeded",4)
    root = Path(new_root)
    if root.exists() or root.is_symlink(): raise PipelineError("output_root_exists", 2)
    root.mkdir(parents=False); (root / "predecessor").mkdir(); shutil.copytree(predecessor_root, root / "predecessor/stage1"); (root / "units").mkdir()
    predecessor = {"artifact_manifest_sha256": predecessor_hash, "stage_result_sha256": sha256_bytes((predecessor_root / "stage-result.v1.json").read_bytes())}
    manifest = _manifest("stage2a", stage0_root, stage0_manifest_hash, selection, controls_path, control_hash, units, identity, predecessor)
    manifest_path = (root / "stage-manifest.v1.json").resolve(); manifest_path.write_bytes(canonical_bytes(manifest)); manifest_hash = sha256_bytes(manifest_path.read_bytes())
    started = time.monotonic(); added, calls = _run_units(root, manifest, manifest_path, manifest_hash, transport, 11, started, remaining_wall)
    if calls != {"reviewer": 60, "judge": 30}: raise PipelineError("stage2a_call_rectangle_invalid", 4)
    total_calls = {"reviewer": predecessor_result["model_calls"]["reviewer"] + calls["reviewer"], "judge": predecessor_result["model_calls"]["judge"] + calls["judge"]}
    cumulative_active=predecessor_active+(time.monotonic()-started)
    if cumulative_active > STAGE2A_WALL_SECONDS: raise PipelineError("stage2a_cumulative_wall_exceeded",4)
    result = _reduce("stage2a", predecessor_result["cells"] + added, controls, total_calls, f"{cumulative_active:.6f}")
    (root / "stage-result.v1.json").write_bytes(canonical_bytes(result)); _write_manifest(root, "m20.model-stage-artifact-manifest.v1")
    return result


def verify_stage(root: str | Path) -> dict:
    path = Path(root)
    try:
        if (path / "stage0-selection.v1.json").is_file():
            _, selection, _, _ = _stage0_root(path / "stage0-selection.v1.json"); kind = "stage0"; identity = selection["selection_sha256"]
        elif (path / "control-labels.v1.json").is_file():
            value, raw = _read(path / "control-labels.v1.json"); _, selection, _, _ = _stage0_root(Path(value["stage0_selection_path"])); _controls(path / "control-labels.v1.json", selection); kind = "controls"; identity = sha256_bytes(raw)
        else:
            manifest, result, identity = _verify_model_stage(path); kind = manifest["stage"]
        return {"schema": "m20.verify-stage.v1", "stage": kind, "identity_sha256": identity, "ok": True, "failure_code": None}
    except Exception as error:
        return {"schema": "m20.verify-stage.v1", "stage": None, "identity_sha256": None, "ok": False, "failure_code": getattr(error, "code", "stage_artifact_invalid")}
