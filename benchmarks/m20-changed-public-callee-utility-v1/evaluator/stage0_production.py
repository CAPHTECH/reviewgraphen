"""Production-only corpus and frozen-cluster-pipeline adapters."""
from __future__ import annotations

import hashlib
import subprocess
from pathlib import Path

from .canonical import canonical_bytes, parse_json_bytes
from .repository import GitRepository, production_rust
from .stage0_contract import PROFILE_HASH
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


def run_frozen_cluster_pipeline(cluster, build_root: Path) -> dict:
    if not PIPELINE.is_file() or PIPELINE.is_symlink(): raise Stage0Error("frozen_cluster_pipeline_unavailable", 3)
    request = {"schema":"reviewgraphen.generic_review_request.v3", "workspace_admission_root":".", "repository_admission_root":".", "repository_identity":cluster.repository_id, "base_revision":cluster.base_commit_oid, "target_revision":cluster.head_commit_oid, "ingest":{"profile_id":"rust.production.v1", "profile_version":"1", "rule_set_hash":PROFILE_HASH, "max_files":4096, "max_file_bytes":4194304, "max_total_source_bytes":67108864}, "plan":{"max_waves":1024, "max_obligations_per_wave":1024}, "observer":{"kind":"deterministic_abstain"}, "verifier_descriptor_id":"workspace.cargo_test@1", "context_policy_id":"context.subject_windows@3"}
    request_path = build_root / "pipeline-request.v3.json"; request_path.write_bytes(canonical_bytes(request))
    artifact_root = build_root / "pipeline-artifacts"
    result = subprocess.run([str(PIPELINE), "review", "--request", str(request_path), "--artifacts", str(artifact_root)], cwd=Path(cluster.repository_root), env={"PATH":"", "LC_ALL":"C"}, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, shell=False, timeout=1800, check=False)
    if result.returncode or result.stderr: raise Stage0Error("frozen_cluster_pipeline_failed")
    try: run = parse_json_bytes(result.stdout)
    except ValueError as error: raise Stage0Error("frozen_cluster_pipeline_output_invalid") from error
    obligations = sorted(item["id"] for item in run.get("obligation_contract", []) if item.get("rule_id") == "relation.changed_public_callee@1" and item.get("applicability_status") == "applicable")
    contexts = {row["context"]["obligation_id"]:row["context"] for row in run.get("contexts", [])}
    retained, remainders, admitted = [], [], 0
    for obligation in obligations:
        context = contexts.get(obligation)
        outcomes = context.get("subject_outcomes", []) if isinstance(context, dict) else []
        if len(outcomes) == 2 and [item.get("role") for item in outcomes] == ["callee", "caller"] and all(item.get("state") == "admitted" for item in outcomes):
            retained.append(obligation); admitted += sum(window["excerpt_byte_length"] for window in context.get("windows", []))
        else:
            loss = next((item for item in outcomes if item.get("state") != "admitted"), None) or {}
            remainders.append({"obligation_id":obligation, "kind":"subject_unknown" if loss.get("state") == "unknown" else "subject_loss", "source_id":loss.get("source_artifact_id") or run["legacy_ingestion"]["snapshot_id"], "recovery_reference":loss.get("recovery_reference") or context.get("projection_hash", run["run_id"]) if isinstance(context, dict) else run["run_id"]})
    repository = GitRepository(cluster.repository_root, [cluster.repository_root]); trees = repository.snapshots(cluster.base_commit_oid, cluster.head_commit_oid)[:2]
    changed = {path for path in set(trees[0]) | set(trees[1]) if production_rust(path) and trees[0].get(path) != trees[1].get(path)}
    whole = sum(len(repository.object(entry[1], "blob")[1]) for path in changed for entry in (trees[0].get(path), trees[1].get(path)) if entry is not None)
    coverage = run.get("coverage", {}); honest = coverage.get("enumeration_capability_states", {}).get("direct_calls") == "partial" and coverage.get("global_call_coverage_claim") == "prohibited" and coverage.get("enumeration_obstruction_ids") == sorted(coverage.get("enumeration_obstruction_summary_ids", []) + coverage.get("enumeration_limitation_ids", []))
    value = {"schema":"m20.stage0-cluster-build.v1", "commit_cluster_id":cluster.commit_cluster_id, "applicable_obligation_ids":obligations, "subject_retained_obligation_ids":retained, "deferred_obligation_ids":sorted(set(coverage.get("deferred_obligation_ids", [])) & set(obligations)), "subject_remainders":sorted(remainders, key=lambda item:item["obligation_id"]), "admitted_source_bytes":admitted, "whole_changed_production_files_bytes":whole, "model_eligible":bool(obligations and retained and admitted <= 65536), "enumeration_honest":honest}
    value["deterministic_payload_sha256"] = build_payload_hash(value); return value
