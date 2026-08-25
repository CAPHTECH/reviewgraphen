"""Closed deterministic Stage-0 orchestration; scoring is deliberately absent."""
from __future__ import annotations

import hashlib
import math
import os
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterable

from .canonical import canonical_bytes, hash_json, sha256_bytes, stable_id

EXPERIMENT_ID = "m20-changed-public-callee-utility-v1"
COMMIT_SELECTION_SEED = "m20-commit-selection-v1"
EXPECTED_CLUSTERS = 300


class Stage0Error(ValueError):
    def __init__(self, code: str, exit_code: int = 4):
        self.code, self.exit_code = code, exit_code
        super().__init__(code)


@dataclass(frozen=True)
class Cluster:
    repository_id: str
    repository_root: str
    base_commit_oid: str
    head_commit_oid: str

    @property
    def commit_cluster_id(self) -> str:
        return commit_cluster_id(self.repository_id, self.base_commit_oid, self.head_commit_oid)


@dataclass(frozen=True)
class Stage0Selection:
    """Launch authorization type. It is intentionally not score-shaped."""
    value: dict

    def __iter__(self):
        # Makes the launch-only type fail closed at any list-of-score boundary.
        from .pipeline import PipelineError
        raise PipelineError("stage0_selection_not_score_input", 2)

    def __getitem__(self, _key):
        from .pipeline import PipelineError
        raise PipelineError("stage0_selection_not_score_input", 2)


ClusterPipeline = Callable[[Cluster, Path], dict]
CompletionHook = Callable[[Cluster], None]


def worker_ceiling(cpu_count: int | None = None) -> int:
    """Return the operational ceiling; it is never a canonical input."""
    available = os.cpu_count() if cpu_count is None else cpu_count
    if available is None or isinstance(available, bool) or available <= 2:
        return 1
    return max(1, min(16, available - 2))


def commit_cluster_id(repository_id: str, base_oid: str, head_oid: str) -> str:
    if not all(isinstance(value, str) and value for value in (repository_id, base_oid, head_oid)):
        raise Stage0Error("cluster_identity_invalid", 2)
    return stable_id("commit-cluster", {
        "cluster_contract": "m20.commit_cluster@1",
        "experiment_id": EXPERIMENT_ID,
        "repository_id": repository_id,
        "base_commit_oid": base_oid,
        "head_commit_oid": head_oid,
    })


def _hash_order(seed: str, identity: str) -> tuple[bytes, bytes]:
    return hashlib.sha256(seed.encode() + b"\0" + identity.encode()).digest(), identity.encode()


def enumerate_clusters(repositories: Iterable[dict], expected_count: int = EXPECTED_CLUSTERS) -> tuple[Cluster, ...]:
    clusters: list[Cluster] = []
    repository_ids: set[str] = set()
    for repository in repositories:
        if not isinstance(repository, dict) or set(repository) != {"repository_id", "repository_root", "commits"}:
            raise Stage0Error("corpus_repository_invalid", 2)
        repository_id, repository_root, commits = repository["repository_id"], repository["repository_root"], repository["commits"]
        if not isinstance(repository_id, str) or not repository_id or repository_id in repository_ids:
            raise Stage0Error("corpus_repository_duplicate", 2)
        if not isinstance(repository_root, str) or not repository_root or not isinstance(commits, list):
            raise Stage0Error("corpus_repository_invalid", 2)
        repository_ids.add(repository_id)
        for pair in commits:
            if not isinstance(pair, dict) or set(pair) != {"base_commit_oid", "head_commit_oid"}:
                raise Stage0Error("cluster_record_invalid", 2)
            clusters.append(Cluster(repository_id, repository_root, pair["base_commit_oid"], pair["head_commit_oid"]))
    identifiers = [cluster.commit_cluster_id for cluster in clusters]
    if len(identifiers) != len(set(identifiers)):
        raise Stage0Error("commit_cluster_duplicate", 2)
    if len(clusters) != expected_count:
        raise Stage0Error("commit_cluster_count_invalid", 2)
    return tuple(sorted(clusters, key=lambda item: item.commit_cluster_id.encode()))


def _ids(value: object, code: str) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(item, str) and item for item in value):
        raise Stage0Error(code)
    if value != sorted(value, key=lambda item: item.encode()) or len(value) != len(set(value)):
        raise Stage0Error(code)
    return value


def build_payload_hash(value: dict) -> str:
    return hash_json({key: value[key] for key in sorted(value) if key != "deterministic_payload_sha256"})


def _validate_build(value: dict, cluster_id: str) -> dict:
    fields = {"schema", "commit_cluster_id", "applicable_obligation_ids", "subject_retained_obligation_ids", "deferred_obligation_ids", "subject_remainders", "admitted_source_bytes", "whole_changed_production_files_bytes", "model_eligible", "enumeration_honest", "deterministic_payload_sha256"}
    if not isinstance(value, dict) or set(value) != fields or value["schema"] != "m20.stage0-cluster-build.v1" or value["commit_cluster_id"] != cluster_id:
        raise Stage0Error("cluster_build_invalid")
    applicable = _ids(value["applicable_obligation_ids"], "applicable_set_invalid")
    retained = _ids(value["subject_retained_obligation_ids"], "subject_set_invalid")
    deferred = _ids(value["deferred_obligation_ids"], "deferred_set_invalid")
    if not set(retained) <= set(applicable) or not set(deferred) <= set(applicable):
        raise Stage0Error("cluster_set_not_subset")
    remainder = value["subject_remainders"]
    if not isinstance(remainder, list) or {item for item in applicable if item not in retained} != {item.get("obligation_id") for item in remainder if isinstance(item, dict)}:
        raise Stage0Error("subject_remainder_incomplete")
    for item in remainder:
        if not isinstance(item, dict) or set(item) != {"obligation_id", "kind", "source_id", "recovery_reference"} or item["kind"] not in {"subject_loss", "subject_unknown"} or not all(isinstance(item[key], str) and item[key] for key in ("obligation_id", "source_id", "recovery_reference")):
            raise Stage0Error("subject_remainder_invalid")
    for field in ("admitted_source_bytes", "whole_changed_production_files_bytes"):
        if isinstance(value[field], bool) or not isinstance(value[field], int) or value[field] < 0:
            raise Stage0Error("cluster_bytes_invalid")
    if not isinstance(value["model_eligible"], bool) or not isinstance(value["enumeration_honest"], bool):
        raise Stage0Error("cluster_flag_invalid")
    if value["deterministic_payload_sha256"] != build_payload_hash(value):
        raise Stage0Error("cluster_payload_hash_invalid")
    return value


def _median(values: list[int]) -> tuple[int, int]:
    ordered = sorted(values); middle = len(ordered) // 2
    return (ordered[middle], 1) if len(ordered) % 2 else (ordered[middle - 1] + ordered[middle], 2)


def reduce_gates(builds: list[dict], expected_count: int = EXPECTED_CLUSTERS) -> list[dict]:
    applicable = {item["commit_cluster_id"]: item["applicable_obligation_ids"] for item in builds}
    a = {identifier for values in applicable.values() for identifier in values}
    s = {identifier for item in builds for identifier in item["subject_retained_obligation_ids"]}
    d = {identifier for item in builds for identifier in item["deferred_obligation_ids"]}
    admitted = [item["admitted_source_bytes"] for item in builds for _ in item["applicable_obligation_ids"]]
    whole = [item["whole_changed_production_files_bytes"] for item in builds for _ in item["applicable_obligation_ids"]]
    bounded = False
    if admitted:
        am_num, am_den = _median(admitted); wm_num, wm_den = _median(whole)
        bounded = 2 * am_num * wm_den <= wm_num * am_den and sorted(admitted)[math.ceil(0.90 * len(admitted)) - 1] <= 65_536
    fanout = sorted((len(values), cluster_id) for cluster_id, values in applicable.items())
    prevalence = (sum(bool(values) for values in applicable.values()) >= 45 and len(a) >= 60) if expected_count == EXPECTED_CLUSTERS else (len(applicable) == expected_count and all(applicable.values()))
    fanout_ok = fanout[284][0] <= 50 if expected_count == EXPECTED_CLUSTERS else fanout[math.ceil(0.95 * expected_count) - 1][0] <= 50
    checks = (
        ("prevalence", prevalence),
        ("subject_retention", 20 * len(s) >= 19 * len(a)),
        ("bounded_context", bounded),
        ("determinism", True),
        ("enumeration_honesty", all(item["enumeration_honest"] for item in builds)),
        ("fan_out", len(fanout) == expected_count and fanout_ok),
        ("deferred_fraction", 20 * len(d) <= len(a)),
    )
    return [{"id": name, "passed": passed} for name, passed in checks]


def _write(path: Path, value: dict) -> None:
    try:
        with path.open("xb") as handle: handle.write(canonical_bytes(value))
    except OSError as error:
        raise Stage0Error("artifact_write_failed") from error


def _run_cluster(cluster: Cluster, cluster_root: Path, cluster_pipeline: ClusterPipeline, completion_hook: CompletionHook | None) -> tuple[str, dict]:
    directory = cluster_root / cluster.commit_cluster_id.rsplit(":", 1)[1]
    try:
        directory.mkdir()
        pair = []
        for number in (1, 2):
            build_root = directory / f"build-{number}"
            build_root.mkdir()
            value = _validate_build(cluster_pipeline(cluster, build_root), cluster.commit_cluster_id)
            _write(build_root / "cluster-result.v1.json", value)
            pair.append(value)
    except OSError as error:
        raise Stage0Error("artifact_write_failed") from error
    if canonical_bytes(pair[0]) != canonical_bytes(pair[1]):
        raise Stage0Error("cluster_build_nondeterministic")
    if completion_hook is not None:
        completion_hook(cluster)
    return cluster.commit_cluster_id, pair[0]


def _ordered_terminal_builds(completed: dict[str, dict], expected_ids: set[str], expected_count: int) -> list[dict]:
    if set(completed) != expected_ids or len(completed) != expected_count:
        raise Stage0Error("terminal_cluster_set_invalid")
    return [completed[cluster_id] for cluster_id in sorted(completed, key=str.encode)]


def run_stage0(output_root: str | Path, repositories: Iterable[dict], cluster_pipeline: ClusterPipeline, expected_count: int = EXPECTED_CLUSTERS, *, jobs: int | None = None, _completion_hook: CompletionHook | None = None, _executor_factory=ThreadPoolExecutor) -> dict:
    root = Path(output_root)
    try: root.mkdir(parents=False, exist_ok=False)
    except FileExistsError as error: raise Stage0Error("output_exists", 2) from error
    except OSError as error: raise Stage0Error("output_create_failed", 2) from error
    clusters = enumerate_clusters(repositories, expected_count)
    corpus = {"schema": "m20.corpus-manifest.v1", "experiment_id": EXPERIMENT_ID, "cluster_ids": [item.commit_cluster_id for item in clusters], "clusters": [{"commit_cluster_id": item.commit_cluster_id, "repository_id": item.repository_id, "repository_root": item.repository_root, "base_commit_oid": item.base_commit_oid, "head_commit_oid": item.head_commit_oid} for item in clusters]}
    _write(root / "corpus-manifest.v1.json", corpus); cluster_root = root / "clusters"; cluster_root.mkdir()
    ceiling = worker_ceiling()
    width = ceiling if jobs is None else jobs
    if isinstance(width, bool) or not isinstance(width, int) or width < 1 or width > ceiling:
        raise Stage0Error("stage0_jobs_invalid", 2)
    completed: dict[str, dict] = {}
    with _executor_factory(max_workers=width, thread_name_prefix="m20-stage0") as executor:
        futures = [executor.submit(_run_cluster, cluster, cluster_root, cluster_pipeline, _completion_hook) for cluster in clusters]
        for future in as_completed(futures):
            cluster_id, build = future.result()
            if cluster_id in completed:
                raise Stage0Error("commit_cluster_duplicate")
            completed[cluster_id] = build
    expected_ids = {cluster.commit_cluster_id for cluster in clusters}
    first_builds = _ordered_terminal_builds(completed, expected_ids, expected_count)
    gates = reduce_gates(first_builds, expected_count)
    eligible = sorted((item["commit_cluster_id"] for item in first_builds if item["model_eligible"]), key=lambda identity: _hash_order(COMMIT_SELECTION_SEED, identity))
    result = {"schema": "m20.stage0-result.v1", "experiment_id": EXPERIMENT_ID, "cluster_count": len(clusters), "model_calls": 0, "gates": gates}
    _write(root / "stage0-result.v1.json", result)
    if not all(gate["passed"] for gate in gates):
        paths = sorted(path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_file())
        records = [{"path": path, "sha256": sha256_bytes((root / path).read_bytes())} for path in paths]
        records.append({"path":"artifact-manifest.v1.json", "sha256":"self-described-by-manifest-bytes"})
        _write(root / "artifact-manifest.v1.json", {"schema": "m20.stage0-artifact-manifest.v1", "files": records})
        raise Stage0Error("stage0_gate_failed", 2)
    selection = {"schema": "m20.stage0-selection.v1", "experiment_id": EXPERIMENT_ID, "eligible_cluster_ids": sorted(eligible), "hash_ordered_cluster_ids": eligible, "stage1_cluster_ids": eligible[:10], "stage2a_cumulative_cluster_ids": eligible[:40]}
    selection["selection_sha256"] = hash_json(selection)
    _write(root / "stage0-selection.v1.json", selection)
    paths = sorted(path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_file())
    records = [{"path": path, "sha256": sha256_bytes((root / path).read_bytes())} for path in paths]
    records.append({"path":"artifact-manifest.v1.json", "sha256":"self-described-by-manifest-bytes"})
    _write(root / "artifact-manifest.v1.json", {"schema": "m20.stage0-artifact-manifest.v1", "files": records})
    return {"schema": "m20.stage0-driver-result.v1", "selection": Stage0Selection(selection), "output_root": str(root)}


def _production_freeze_gate(benchmark_root: Path | None = None) -> None:
    from .canonical import parse_json_bytes
    from .freeze import verify_manifest
    benchmark_root = Path(__file__).resolve().parent.parent if benchmark_root is None else benchmark_root
    evaluator_root = benchmark_root / "evaluator"
    try:
        preregistration = parse_json_bytes((benchmark_root / "preregistration.json").read_bytes())
        contracts = preregistration["arm_neutral_contracts"]
        freeze_hashes = contracts["freeze_hashes"]
        freeze_history = contracts["freeze_history"]
        active_keys = ("freeze_manifest_sha256", "evaluator_bundle_sha256", "evaluator_execution_sha256", "generated_fixture_inventory_sha256", "reference_vector_set_sha256", "mutation_manifest_sha256", "semantic_acceptance_reference_sha256", "measurement_record_sha256")
        if any(not isinstance(freeze_hashes.get(key), str) or not freeze_hashes[key] for key in active_keys):
            raise ValueError("active_freeze_hash_missing")
        manifest_path = freeze_hashes["freeze_manifest_path"]
        active_manifest_hash = freeze_hashes["freeze_manifest_sha256"]
        if not isinstance(manifest_path, str) or not manifest_path or Path(manifest_path).is_absolute() or any(part in {"", ".", ".."} for part in Path(manifest_path).parts):
            raise Stage0Error("stage0_freeze_manifest_path_invalid", 3)
        manifest_raw = (benchmark_root / manifest_path).read_bytes()
        manifest = parse_json_bytes(manifest_raw)
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise Stage0Error("stage0_freeze_input_invalid", 3) from error
    if sha256_bytes(manifest_raw) != active_manifest_hash:
        raise Stage0Error("stage0_active_freeze_hash_mismatch", 3)
    manifest_slots = {key: manifest.get(key) for key in active_keys if key != "freeze_manifest_sha256"}
    if any(manifest_slots[key] != freeze_hashes[key] for key in manifest_slots):
        raise Stage0Error("stage0_active_manifest_slot_mismatch", 3)
    if not isinstance(freeze_history, list) or not freeze_history or manifest.get("supersedes_freeze_manifest_sha256") != freeze_history[-1].get("freeze_manifest_sha256"):
        raise Stage0Error("stage0_freeze_supersedes_mismatch", 3)
    verification = verify_manifest(evaluator_root, benchmark_root / "EVALUATOR_SPEC.md", manifest)
    required = ("bundle_valid", "design_spec_valid", "execution_contract_valid", "runtime_compatible", "checks_valid", "ok")
    if any(verification.get(key) is not True for key in required):
        raise Stage0Error("stage0_freeze_verification_failed", 3)


def production_stage0(output_root: str | Path, *, jobs: int | None = None) -> dict:
    """Production hook is closed: corpus resolution/pipeline are evaluator-owned."""
    root = Path(output_root)
    if root.exists() or root.is_symlink(): raise Stage0Error("output_exists", 2)
    _production_freeze_gate()
    from .stage0_production import resolve_corpus, run_frozen_cluster_pipeline
    return run_stage0(output_root, resolve_corpus(), run_frozen_cluster_pipeline, jobs=jobs)
