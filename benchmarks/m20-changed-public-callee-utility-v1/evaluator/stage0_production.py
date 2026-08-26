"""Production-only corpus and frozen-cluster-pipeline adapters."""
from __future__ import annotations

import hashlib
import re
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

from .canonical import canonical_bytes, hash_json, parse_json_bytes, sha256_bytes, stable_id
from .pipeline import packet_v3_account
from .repository import GitRepository, production_rust
from .stage0_contract import PROFILE_HASH, build_context_projection_v3
from .stage0_driver import Stage0Error, build_payload_hash

GIT = Path("/usr/bin/git")
ROOTS = (Path("/home/rizumita/github"),)
REVIEWGRAPHEN = Path("/home/rizumita/workspace/reviewgraphen")
SEED = "m20-repository-selection-v1"
PIPELINE = Path("/home/rizumita/workspace/reviewgraphen/target/release/reviewgraphen")
ENV = {"GIT_CONFIG_NOSYSTEM":"1", "GIT_CONFIG_GLOBAL":"/dev/null", "GIT_CONFIG_COUNT":"0", "LC_ALL":"C", "PATH":""}


def _git(root: Path, arguments: list[str]) -> list[str]:
    result = subprocess.run([str(GIT), *arguments], cwd=root, env=ENV, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, shell=False, timeout=30, check=False)
    if result.returncode or result.stderr:
        raise Stage0Error("corpus_git_failed", 2)
    try: return result.stdout.decode("utf-8", "strict").splitlines()
    except UnicodeDecodeError as error: raise Stage0Error("corpus_git_output_invalid", 2) from error


def _canonical_origin(root: Path) -> str:
    values = _git(root, ["config", "--get", "remote.origin.url"])
    if len(values) != 1: raise Stage0Error("repository_origin_invalid", 2)
    value = values[0]
    if value.startswith("git@github.com:"): value = "https://github.com/" + value[len("git@github.com:"):]
    if value.startswith("ssh://git@github.com/"): value = "https://github.com/" + value[len("ssh://git@github.com/"):]
    if value.endswith(".git"): value = value[:-4]
    if not value.startswith("https://github.com/") or value.count("/") != 4: raise Stage0Error("repository_origin_invalid", 2)
    return value[len("https://"):]


def resolve_corpus() -> list[dict]:
    candidates = [path for parent in ROOTS if parent.is_dir() for path in parent.iterdir() if path.is_dir() and not path.is_symlink()]
    candidates.append(REVIEWGRAPHEN)
    eligible = []
    for root in candidates:
        if not (root / ".git").is_dir(): continue
        repository_id = _canonical_origin(root)
        eligible.append((hashlib.sha256(SEED.encode() + b"\0" + repository_id.encode()).digest(), repository_id.encode(), root, repository_id))
    eligible.sort()
    if len(eligible) < 3: raise Stage0Error("corpus_repository_count_invalid", 2)
    output = []
    for _, _, root, repository_id in eligible[:3]:
        commits = _git(root, ["rev-list", "--first-parent", "--max-count=100", "HEAD"])
        if len(commits) != 100: raise Stage0Error("commit_cluster_count_invalid", 2)
        pairs = []
        for head in commits:
            parents = _git(root, ["rev-list", "--parents", "--max-count=1", head])
            fields = parents[0].split() if len(parents) == 1 else []
            if len(fields) < 2: raise Stage0Error("first_parent_missing", 2)
            pairs.append({"base_commit_oid": fields[1], "head_commit_oid": head})
        output.append({"repository_id":repository_id, "repository_root":str(root.resolve()), "commits":pairs})
    return output


def _stderr_text(value: bytes | str | None) -> str:
    if isinstance(value, bytes): return value.decode("utf-8", "replace").strip()
    return value.strip() if isinstance(value, str) else ""


def _record_pipeline_failure(diagnostic_build: Path, cluster_id: str, exit_code: int | None, stderr: str, reason: str, elapsed_seconds: float, stage0_code: str = "frozen_cluster_pipeline_failed") -> dict:
    elapsed = f"{elapsed_seconds:.6f}"
    failure = {"schema":"m20.stage0-cluster-pipeline-failure.v1", "commit_cluster_id":cluster_id, "product_exit_code":exit_code, "product_stderr":stderr, "typed_reason":reason, "elapsed_seconds":elapsed}
    with (diagnostic_build / "pipeline-failure.v1.json").open("xb") as sink: sink.write(canonical_bytes(failure))
    return {"schema":"m20.stage0-error-diagnostic.v1", "code":stage0_code, "commit_cluster_id":cluster_id, "product_exit_code":exit_code, "typed_reason":reason, "product_stderr":stderr, "elapsed_seconds":elapsed}


def _ingest_admission_reason(stderr: str, diagnostic: dict) -> str | None:
    """Recognize only bounded-ingest admission failures attested by product diagnostics."""
    if not stderr.startswith("generic review ingestion failed:") or not isinstance(diagnostic, dict) or diagnostic.get("schema") != "reviewgraphen.generic_review_diagnostics.v1":
        return None
    stages = diagnostic.get("stages")
    if not isinstance(stages, list) or not any(isinstance(row, dict) and row.get("stage") == "ingest" and row.get("status") == "failed" for row in stages):
        return None
    detail = stderr[len("generic review ingestion failed:"):].strip()
    if re.fullmatch(r"Git tree contains more than the configured [0-9]+ regular-file bound", detail):
        return "max_files"
    if re.fullmatch(r"snapshot source bytes total [0-9]+, above the 67108864 byte bound", detail):
        return "snapshot_bytes"
    if re.fullmatch(r"Git blob .+ is [0-9]+ bytes, above the 4194304 byte bound", detail):
        return "blob_bytes"
    return None


def _ingest_exclusion_or_fatal(exit_code: int, stderr: str, diagnostic: dict) -> str:
    reason = _ingest_admission_reason(stderr, diagnostic) if exit_code == 20 else None
    if reason is None:
        raise Stage0Error("frozen_cluster_pipeline_failed")
    return reason


def _product_invocation(request_path: Path) -> tuple[str, dict]:
    executable_sha = sha256_bytes(PIPELINE.read_bytes())
    invocation = {"product_executable_sha256":executable_sha,"argv":["review","--request","pipeline-request.v3.json","--artifacts","pipeline-artifacts","--diagnostics","generic-review-diagnostics.v1.json"],"cwd_scope":"isolated-repository-copy","request_sha256":sha256_bytes(request_path.read_bytes())}
    return executable_sha, invocation


def _excluded_build(cluster, reason: str, ignored_symlink_count: int) -> dict:
    value = {"schema":"m20.stage0-cluster-build.v1", "commit_cluster_id":cluster.commit_cluster_id, "repository_root":cluster.repository_root, "base_commit_oid":cluster.base_commit_oid, "head_commit_oid":cluster.head_commit_oid, "applicable_obligation_ids":[], "subject_retained_obligation_ids":[], "deferred_obligation_ids":[], "subject_remainders":[], "selected_obligation_id":"", "frozen_obligation_path":"", "frozen_obligation_sha256":"", "admitted_source_bytes":0, "whole_changed_production_files_bytes":0, "ignored_symlink_count":ignored_symlink_count, "model_eligible":False, "enumeration_honest":True, "ingest_exclusion":{"code":"stage0_ingest_admission_rejected","reason":reason}}
    value["deterministic_payload_sha256"] = build_payload_hash(value)
    return value


def _packet_v3_context_budget(repository, trees: tuple[dict, dict], context: dict) -> tuple[int, int, bool]:
    materialized = {item["artifact_id"]:item for item in context.get("materialized_sources", [])}
    callee = next((item for item in context.get("subject_outcomes", []) if item.get("role") == "callee" and item.get("state") == "admitted"), None)
    source = materialized.get(callee.get("source_artifact_id")) if isinstance(callee, dict) else None
    requested = callee.get("requested_range") if isinstance(callee, dict) else None
    if not isinstance(source, dict) or not isinstance(requested, dict): return 0, 0, False
    path = source.get("path"); head = trees[1].get(path) if isinstance(path, str) else None
    if head is None: return 0, 0, False
    callee_body = {"role":"changed", "snapshot_side":"head", "path":path, "start_line":requested.get("start_line"), "end_line":requested.get("end_line"), "blob_oid":head[1]}
    windows = []
    for window in context.get("windows", []):
        window_source = materialized.get(window.get("source_artifact_id")); window_path = window_source.get("path") if isinstance(window_source, dict) else None; entry = trees[1].get(window_path) if isinstance(window_path, str) else None; span = window.get("range")
        if entry is None or not isinstance(span, dict): return 0, 0, False
        body = {"role":"changed" if "callee" in window.get("roles", []) else "context", "snapshot_side":"head", "path":window_path, "start_line":span.get("start_line"), "end_line":span.get("end_line"), "blob_oid":entry[1]}
        windows.append({"required_id":stable_id("source-request", body), **body})
    callee_spec = {"required_id":stable_id("source-request", callee_body), **callee_body}
    try: _, _, counts, eligible = packet_v3_account(repository, trees, callee_spec, windows)
    except (KeyError, TypeError, ValueError): return 0, 0, False
    return counts[0], counts[1], eligible


def _frozen_obligation(cluster, run: dict, context: dict, repository, trees: tuple[dict, dict]) -> dict:
    obligation_id = context["obligation_id"]
    contract = next(item for item in run["obligation_contract"] if item.get("id") == obligation_id)
    subjects = context["subject_outcomes"]
    by_artifact = {item["artifact_id"]: item for item in context["materialized_sources"]}
    endpoint_pairs = [{"caller_endpoint_id": next(item["endpoint_id"] for item in subjects if item["role"] == "caller"), "callee_endpoint_id": next(item["endpoint_id"] for item in subjects if item["role"] == "callee")}]
    sources = []; windows = []; projection_subjects = []; materialized = {}
    for subject in subjects:
        source = by_artifact[subject["source_artifact_id"]]; path = source["path"]; entry = trees[1].get(path)
        if entry is None: raise Stage0Error("frozen_obligation_source_missing")
        span = subject["requested_range"]
        body = {"role":"changed" if subject["role"] == "callee" else "context", "snapshot_side":"head", "path":path, "start_line":span["start_line"], "end_line":span["end_line"], "blob_oid":entry[1]}
        required_id = stable_id("source-request", body)
        sources.append({"required_id":required_id, **body})
        windows.append({"window_id":subject["window_id"], "source_artifact_id":subject["source_artifact_id"], "start_line":span["start_line"], "end_line":span["end_line"], "role":subject["role"], "source_required_id":required_id, "support_anchor_ids":[]})
        projection_subjects.append({"role":subject["role"], "status":"admitted", "endpoint_id":subject["endpoint_id"], "source_artifact_id":subject["source_artifact_id"], "start_line":span["start_line"], "end_line":span["end_line"], "window_id":subject["window_id"]})
        materialized[subject["source_artifact_id"]] = {"source_artifact_id":subject["source_artifact_id"], "snapshot_side":"head", "path":path, "blob_oid":entry[1]}
    file_ids = sorted(materialized, key=str.encode)
    unknown_ids = sorted((stable_id("context-unknown", item) for item in context.get("unknowns", [])), key=str.encode)
    projection = build_context_projection_v3(
        {"projection_id":context["context_id"], "snapshot_id":context["snapshot_id"], "request_id":run["request_id"], "obligation_ids":[obligation_id], "relation_ids":contract["target_refs"], "endpoint_pairs":endpoint_pairs},
        file_ids, file_ids, projection_subjects, list(materialized.values()), [], windows, [],
        {"state":context["latent_cardinality"]["state"], "capability_states":{key:value for key,value in context["latent_cardinality"]["capability_states"].items() if value in {"partial","unknown"}}, "qualification_ids":context["latent_cardinality"]["qualification_ids"]},
        unknown_ids, [], [],
    )
    subject_windows = sorted(({"subject_id":item["endpoint_id"], "window_id":item["window_id"], "role":item["role"]} for item in subjects), key=lambda item:(item["subject_id"].encode(),item["window_id"].encode(),item["role"].encode()))
    return {"schema":"m20.frozen_obligation.v1", "unit_id":cluster.commit_cluster_id, "rule_id":"relation.changed_public_callee@1", "property_id":"rust.callee_contract_review@1", "obligation_ids":[obligation_id], "relation_ids":contract["target_refs"], "endpoint_pairs":endpoint_pairs, "subject_windows":subject_windows, "sources":sorted(sources,key=lambda item:(item["path"].encode(),item["snapshot_side"]!="base",item["start_line"],item["end_line"],item["role"].encode())), "required_references":[], "projection":projection, "bounded_scope_manifest_id":context["context_id"]}


def run_frozen_cluster_pipeline(cluster, build_root: Path, *, _pipeline_timeout: float = 1800, _max_files: int = 20000) -> dict:
    if not PIPELINE.is_file() or PIPELINE.is_symlink(): raise Stage0Error("frozen_cluster_pipeline_unavailable", 3)
    repository = GitRepository(cluster.repository_root, [cluster.repository_root]); trees = repository.snapshots(cluster.base_commit_oid, cluster.head_commit_oid)[:2]
    ignored_symlink_count = sum(tree.ignored_symlink_count for tree in trees)
    # ADR 0038 §§5.4.1/11 and preregistration arm_neutral_contracts.ingest_admission:
    # this is the v3 admission/resource ceiling, never a C/A/S/D denominator.
    request = {"schema":"reviewgraphen.generic_review_request.v3", "workspace_admission_root":".", "repository_admission_root":".", "repository_identity":cluster.repository_id, "base_revision":cluster.base_commit_oid, "target_revision":cluster.head_commit_oid, "ingest":{"profile_id":"rust.production.v1", "profile_version":"1", "rule_set_hash":PROFILE_HASH, "max_files":_max_files, "max_file_bytes":4194304, "max_total_source_bytes":67108864}, "plan":{"max_waves":1024, "max_obligations_per_wave":1024}, "observer":{"kind":"deterministic_abstain"}, "verifier_descriptor_id":"workspace.cargo_test@1", "context_policy_id":"context.subject_windows@3"}
    request_path = build_root / "pipeline-request.v3.json"; request_path.write_bytes(canonical_bytes(request))
    artifact_root = build_root / "pipeline-artifacts"
    stage_root = build_root.parents[2].resolve()
    diagnostic_root = Path(tempfile.gettempdir()) / "m20-stage0-diagnostics" / hashlib.sha256(str(stage_root).encode()).hexdigest()
    diagnostic_build = diagnostic_root / "clusters" / cluster.commit_cluster_id.rsplit(":", 1)[1] / build_root.name
    execution_root = diagnostic_build / "repository"
    try:
        diagnostic_build.mkdir(parents=True, exist_ok=False)
        clone = subprocess.run([str(GIT), "clone", "--shared", "--no-checkout", "--quiet", cluster.repository_root, str(execution_root)], cwd=diagnostic_build, env=ENV, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, shell=False, timeout=300, check=False)
        if clone.returncode or clone.stderr:
            raise Stage0Error("frozen_cluster_repository_copy_failed")
        started = time.monotonic()
        try:
            result = subprocess.run([str(PIPELINE), "review", "--request", str(request_path.resolve()), "--artifacts", "pipeline-artifacts", "--diagnostics", "generic-review-diagnostics.v1.json"], cwd=execution_root, env={"PATH":"/usr/bin", "LC_ALL":"C"}, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, shell=False, timeout=_pipeline_timeout, check=False)
        except subprocess.TimeoutExpired as error:
            diagnostic = _record_pipeline_failure(diagnostic_build, cluster.commit_cluster_id, None, _stderr_text(error.stderr), "timeout", time.monotonic() - started)
            raise Stage0Error("frozen_cluster_pipeline_failed", diagnostic=diagnostic) from error
        product_diagnostic = execution_root / "generic-review-diagnostics.v1.json"
        saved_product_diagnostic = diagnostic_build / product_diagnostic.name
        if product_diagnostic.is_file(): product_diagnostic.replace(saved_product_diagnostic)
        if result.returncode or result.stderr:
            stderr = _stderr_text(result.stderr)
            try: product_diagnostic_value = parse_json_bytes(saved_product_diagnostic.read_bytes())
            except (OSError, ValueError): product_diagnostic_value = {}
            try: exclusion_reason = _ingest_exclusion_or_fatal(result.returncode,stderr,product_diagnostic_value)
            except Stage0Error as error:
                diagnostic = _record_pipeline_failure(diagnostic_build, cluster.commit_cluster_id, result.returncode, stderr, "product_cli_error", time.monotonic() - started)
                raise Stage0Error("frozen_cluster_pipeline_failed", diagnostic=diagnostic) from error
            else:
                _record_pipeline_failure(diagnostic_build, cluster.commit_cluster_id, result.returncode, stderr, exclusion_reason, time.monotonic() - started, "stage0_ingest_admission_rejected")
                executable_sha, invocation = _product_invocation(request_path)
                execution = {"schema":"m20.stage0-product-ingest-exclusion.v1", "product_executable_path":str(PIPELINE), "product_executable_sha256":executable_sha, "invocation_sha256":hash_json(invocation), "request_sha256":sha256_bytes(request_path.read_bytes()), "product_exit_code":result.returncode, "product_stderr":stderr, "diagnostic_stage":"ingest", "diagnostic_status":"failed", "typed_code":"stage0_ingest_admission_rejected", "typed_reason":exclusion_reason}
                (build_root / "product-execution.v1.json").write_bytes(canonical_bytes(execution))
                return _excluded_build(cluster, exclusion_reason, ignored_symlink_count)
        (execution_root / "pipeline-artifacts").replace(artifact_root)
    finally:
        if execution_root.exists(): shutil.rmtree(execution_root)
    try: run = parse_json_bytes(result.stdout)
    except ValueError as error: raise Stage0Error("frozen_cluster_pipeline_output_invalid") from error
    run_raw = canonical_bytes(run)
    (build_root / "product-run.v1.json").write_bytes(run_raw)
    product_manifest = artifact_root / "artifact-manifest.v1.json"
    if not product_manifest.is_file(): raise Stage0Error("frozen_cluster_artifact_manifest_missing")
    executable_sha, invocation = _product_invocation(request_path)
    execution = {
        "schema":"m20.stage0-product-execution.v2",
        "product_executable_path":str(PIPELINE),
        "product_executable_sha256":executable_sha,
        "invocation_sha256":hash_json(invocation),
        "request_sha256":sha256_bytes(request_path.read_bytes()),
        "run_sha256":sha256_bytes(run_raw),
        "artifact_manifest_sha256":sha256_bytes(product_manifest.read_bytes()),
        "request_id":run.get("request_id"),
        "run_id":run.get("run_id"),
    }
    (build_root / "product-execution.v1.json").write_bytes(canonical_bytes(execution))
    obligations = sorted(item["id"] for item in run.get("obligation_contract", []) if item.get("rule_id") == "relation.changed_public_callee@1" and item.get("applicability_status") == "applicable")
    contexts = {row["context"]["obligation_id"]:row["context"] for row in run.get("contexts", [])}
    retained, remainders = [], []
    for obligation in obligations:
        context = contexts.get(obligation)
        outcomes = context.get("subject_outcomes", []) if isinstance(context, dict) else []
        if len(outcomes) == 2 and [item.get("role") for item in outcomes] == ["callee", "caller"] and all(item.get("state") == "admitted" for item in outcomes):
            retained.append(obligation)
        else:
            loss = next((item for item in outcomes if item.get("state") != "admitted"), None) or {}
            remainders.append({"obligation_id":obligation, "kind":"subject_unknown" if loss.get("state") == "unknown" else "subject_loss", "source_id":loss.get("source_artifact_id") or run["legacy_ingestion"]["snapshot_id"], "recovery_reference":loss.get("recovery_reference") or context.get("projection_hash", run["run_id"]) if isinstance(context, dict) else run["run_id"]})
    packet_budgets = [_packet_v3_context_budget(repository, trees, contexts[obligation]) for obligation in retained]
    admitted = sum(treatment_bytes for _, treatment_bytes, _ in packet_budgets)
    changed = {path for path in set(trees[0]) | set(trees[1]) if production_rust(path) and trees[0].get(path) != trees[1].get(path)}
    whole = sum(len(repository.object(entry[1], "blob")[1]) for path in changed for entry in (trees[0].get(path), trees[1].get(path)) if entry is not None)
    coverage = run.get("coverage", {}); honest = coverage.get("enumeration_capability_states", {}).get("direct_calls") == "partial" and coverage.get("global_call_coverage_claim") == "prohibited" and coverage.get("enumeration_obstruction_ids") == sorted(coverage.get("enumeration_obstruction_summary_ids", []) + coverage.get("enumeration_limitation_ids", []))
    eligible = bool(obligations and retained and len(packet_budgets) == len(retained) and all(item[2] for item in packet_budgets))
    selected = retained[0] if eligible else ""; obligation_relative = "frozen-obligation.v1.json" if eligible else ""; obligation_hash = ""
    if eligible:
        frozen = _frozen_obligation(cluster, run, contexts[selected], repository, trees)
        frozen_raw = canonical_bytes(frozen); (build_root / obligation_relative).write_bytes(frozen_raw); obligation_hash = sha256_bytes(frozen_raw)
    value = {"schema":"m20.stage0-cluster-build.v1", "commit_cluster_id":cluster.commit_cluster_id, "repository_root":cluster.repository_root, "base_commit_oid":cluster.base_commit_oid, "head_commit_oid":cluster.head_commit_oid, "applicable_obligation_ids":obligations, "subject_retained_obligation_ids":retained, "deferred_obligation_ids":sorted(set(coverage.get("deferred_obligation_ids", [])) & set(obligations)), "subject_remainders":sorted(remainders, key=lambda item:item["obligation_id"]), "selected_obligation_id":selected, "frozen_obligation_path":obligation_relative, "frozen_obligation_sha256":obligation_hash, "admitted_source_bytes":admitted, "whole_changed_production_files_bytes":whole, "ignored_symlink_count":ignored_symlink_count, "model_eligible":eligible, "enumeration_honest":honest, "ingest_exclusion":None}
    value["deterministic_payload_sha256"] = build_payload_hash(value); return value
