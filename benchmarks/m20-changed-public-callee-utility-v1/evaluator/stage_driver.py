"""Authenticated closed-root drivers for m20 controls, Stage 1, and Stage 2A."""
from __future__ import annotations

import os
import base64
import hashlib
import re
import shutil
import stat
import subprocess
import tempfile
import time
from datetime import datetime
from pathlib import Path

from .artifacts import verify_run
from .canonical import canonical_bytes, hash_json, parse_json_bytes, sha256_bytes, stable_id
from .pipeline import CONTEXT_HASH, CONTEXT_POLICY_ID, PipelineError, RUN, _response_hmac_key
from .stage0_driver import (
    EXPECTED_CLUSTERS,
    Stage0Error,
    _hash_order,
    _validate_build,
    commit_cluster_id,
    reduce_gates,
)

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


def _git_pinned_registration(path: Path) -> tuple[dict, str]:
    try:
        raw=path.read_bytes(); value=parse_json_bytes(raw)
        top=subprocess.run(["/usr/bin/git","-C",str(path.parent),"rev-parse","--show-toplevel"],stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=30,check=False)
        root=Path(top.stdout.decode("utf-8","strict").strip()); relative=path.relative_to(root).as_posix()
        committed=subprocess.run(["/usr/bin/git","-C",str(root),"show","HEAD:"+relative],stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=30,check=False)
    except (OSError,ValueError,UnicodeError,subprocess.TimeoutExpired) as error:
        raise PipelineError("preregistration_invalid", 3) from error
    if top.returncode!=0 or committed.returncode!=0 or committed.stdout!=raw: raise PipelineError("preregistration_git_pin_mismatch",3)
    return value, sha256_bytes(raw)


def _registration() -> tuple[dict, str]:
    return _git_pinned_registration(Path(__file__).resolve().parent.parent/"preregistration.json")


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


def _generic_request_id(request: dict) -> str:
    observer=request["observer"]
    identity={"schema":request["schema"],"repository_identity":request["repository_identity"],"base_revision":request["base_revision"],"target_revision":request["target_revision"],"ingest":request["ingest"],"plan":request["plan"],"observer":observer,"verifier_descriptor_id":request["verifier_descriptor_id"],"context_policy_id":request["context_policy_id"]}
    return stable_id("request",{"schema":request["schema"],"request_sha256":sha256_bytes(canonical_bytes(identity))})


def _verify_generic_run(request: dict, run: dict, run_raw: bytes, artifact_root: Path, product_manifest: dict) -> None:
    request_fields={"schema","workspace_admission_root","repository_admission_root","repository_identity","base_revision","target_revision","ingest","plan","observer","verifier_descriptor_id","context_policy_id"}
    run_fields={"schema","run_id","request_id","legacy_ingestion","ingestion_report_v2","obligation_contract","plan","contexts","observations","provider_free_packet_bindings","coverage","verifier","authority"}
    _closed(request,request_fields,"stage0_product_request_schema_invalid")
    _closed(run,run_fields,"stage0_product_run_schema_invalid")
    if request["schema"]!="reviewgraphen.generic_review_request.v3" or run["schema"]!="reviewgraphen.generic_review_run.v3" or request["workspace_admission_root"]!="." or request["repository_admission_root"]!="." or request["observer"]!={"kind":"deterministic_abstain"} or request["context_policy_id"]!=CONTEXT_POLICY_ID: raise PipelineError("stage0_product_run_schema_invalid",2)
    request_id=_generic_request_id(request)
    legacy=run["legacy_ingestion"]; plan=run["plan"]
    _closed(legacy,{"repository_identity","program_space_id","snapshot_id","base_commit_oid","base_tree_hash","target_commit_oid","target_tree_hash"},"stage0_product_run_schema_invalid")
    _closed(plan,{"id","universe_id","waves","deferred_obligation_ids"},"stage0_product_run_schema_invalid")
    if run["request_id"]!=request_id or legacy["repository_identity"]!=request["repository_identity"] or legacy["base_commit_oid"]!=request["base_revision"] or legacy["target_commit_oid"]!=request["target_revision"] or legacy["program_space_id"]!=legacy["snapshot_id"] or run["run_id"]!=stable_id("run",{"kind":"generic-review-v3","plan_id":plan["id"]}): raise PipelineError("stage0_product_request_run_binding_invalid",2)
    if product_manifest["snapshot_id"]!=legacy["snapshot_id"] or product_manifest["universe_id"]!=plan["universe_id"]: raise PipelineError("stage0_product_request_run_binding_invalid",2)
    audit=[row for row in product_manifest["artifacts"] if row["path"]=="audit.run.v3.json" and row["role"]=="audit"]
    if len(audit)!=1 or (artifact_root/"audit.run.v3.json").read_bytes()!=run_raw: raise PipelineError("stage0_product_run_artifact_mismatch",2)


def _replay_product_execution(build_root: Path, repository_root: str, executable: Path, request_raw: bytes, run_raw: bytes, artifact_root: Path, timeout_seconds: int) -> None:
    """Re-execute a preregistered deterministic product request outside its saved root."""
    try:
        with tempfile.TemporaryDirectory(prefix="m20-stage0-product-replay-") as temporary:
            replay_root = Path(temporary) / "repository"
            cloned = subprocess.run(
                ["/usr/bin/git", "clone", "--shared", "--no-checkout", "--quiet", repository_root, str(replay_root)],
                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                env={"PATH":"", "LC_ALL":"C", "LANG":"C"}, shell=False,
                timeout=timeout_seconds, check=False,
            )
            if cloned.returncode != 0 or cloned.stdout or cloned.stderr:
                raise PipelineError("stage0_product_replay_repository_invalid", 2)
            (replay_root / "pipeline-request.v3.json").write_bytes(request_raw)
            replayed = subprocess.run(
                [str(executable), "review", "--request", "pipeline-request.v3.json", "--artifacts", "pipeline-artifacts", "--diagnostics", "generic-review-diagnostics.v1.json"],
                cwd=replay_root, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                env={"PATH":"", "LC_ALL":"C", "LANG":"C"}, shell=False,
                timeout=timeout_seconds, check=False,
            )
            replay_artifacts = replay_root / "pipeline-artifacts"
            if replayed.returncode != 0 or replayed.stderr or replayed.stdout != run_raw or not replay_artifacts.is_dir():
                raise PipelineError("stage0_product_replay_mismatch", 2)
            if _files(replay_artifacts) != _files(artifact_root):
                raise PipelineError("stage0_product_replay_mismatch", 2)
    except subprocess.TimeoutExpired as error:
        raise PipelineError("stage0_product_replay_timeout", 2) from error
    except OSError as error:
        raise PipelineError("stage0_product_replay_unavailable", 2) from error


def _verify_product_execution(build_root: Path, corpus_contract: dict, replay_repository_root: str | None = None) -> None:
    try:
        request, request_raw = _read(build_root / "pipeline-request.v3.json")
        run, run_raw = _read(build_root / "product-run.v1.json")
        execution, _ = _read(build_root / "product-execution.v1.json")
        artifact_root = _safe_root(build_root / "pipeline-artifacts", "stage0_product_artifacts_invalid")
        product_manifest, product_raw = _read(artifact_root / "artifact-manifest.v1.json")
    except PipelineError as error:
        raise PipelineError("stage0_product_execution_invalid", 2) from error
    _closed(execution, {"schema","product_executable_path","product_executable_sha256","invocation_sha256","request_sha256","run_sha256","artifact_manifest_sha256","request_id","run_id"}, "stage0_product_execution_invalid")
    _closed(product_manifest, {"schema","request_sha256","request_id","run_id","snapshot_id","universe_id","artifacts"}, "stage0_product_artifact_manifest_invalid")
    rows = product_manifest.get("artifacts")
    if not isinstance(rows, list): raise PipelineError("stage0_product_artifact_manifest_invalid", 2)
    expected_paths = []
    for row in rows:
        _closed(row,{"path","role","byte_length","sha256"},"stage0_product_artifact_manifest_invalid")
        relative=row.get("path")
        if not isinstance(relative,str) or Path(relative).is_absolute() or any(part in {"", ".", ".."} for part in Path(relative).parts): raise PipelineError("stage0_product_artifact_manifest_invalid",2)
        path=artifact_root/relative
        if path.is_symlink() or not path.is_file() or row.get("byte_length")!=path.stat().st_size or row.get("sha256")!=sha256_bytes(path.read_bytes()): raise PipelineError("stage0_product_artifact_hash_mismatch",2)
        expected_paths.append(relative)
    actual_paths=sorted(path.relative_to(artifact_root).as_posix() for path in artifact_root.rglob("*") if path.is_file() and not path.is_symlink() and path.name!="artifact-manifest.v1.json")
    if expected_paths != sorted(expected_paths,key=str.encode) or len(expected_paths)!=len(set(expected_paths)) or actual_paths!=expected_paths: raise PipelineError("stage0_product_artifact_manifest_invalid",2)
    request_id=run.get("request_id"); run_id=run.get("run_id")
    executable=Path(corpus_contract["product_cli_path"])
    try: executable_status=executable.lstat(); executable_sha=sha256_bytes(executable.read_bytes())
    except OSError as error: raise PipelineError("stage0_product_executable_unavailable",2) from error
    if executable.is_symlink() or not stat.S_ISREG(executable_status.st_mode) or executable_status.st_mode & 0o111 == 0 or executable_sha!=corpus_contract["product_cli_sha256"]: raise PipelineError("stage0_product_executable_identity_mismatch",2)
    invocation={"product_executable_sha256":executable_sha,"argv":["review","--request","pipeline-request.v3.json","--artifacts","pipeline-artifacts","--diagnostics","generic-review-diagnostics.v1.json"],"cwd_scope":"isolated-repository-copy","request_sha256":sha256_bytes(request_raw)}
    if execution != {"schema":"m20.stage0-product-execution.v2","product_executable_path":corpus_contract["product_cli_path"],"product_executable_sha256":executable_sha,"invocation_sha256":hash_json(invocation),"request_sha256":sha256_bytes(request_raw),"run_sha256":sha256_bytes(run_raw),"artifact_manifest_sha256":sha256_bytes(product_raw),"request_id":request_id,"run_id":run_id} or product_manifest["schema"]!="reviewgraphen.generic_review_artifact_manifest.v1" or product_manifest["request_sha256"]!=sha256_bytes(request_raw) or product_manifest["request_id"]!=request_id or product_manifest["run_id"]!=run_id or not all(isinstance(value,str) and value for value in (request_id,run_id)):
        raise PipelineError("stage0_product_execution_invalid",2)
    try:
        schema_check=subprocess.run([str(executable),"schema","validate","product-run.v1.json"],cwd=build_root,env={"PATH":"","LC_ALL":"C","LANG":"C"},stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,shell=False,timeout=30,check=False)
        schema_result=parse_json_bytes(schema_check.stdout)
    except (OSError,ValueError,subprocess.TimeoutExpired) as error: raise PipelineError("stage0_product_run_schema_invalid",2) from error
    if schema_check.returncode!=0 or schema_result!={"schema":"reviewgraphen.generic_review_run.v3","valid":True}: raise PipelineError("stage0_product_run_schema_invalid",2)
    _verify_generic_run(request,run,run_raw,artifact_root,product_manifest)
    if replay_repository_root is not None:
        _replay_product_execution(build_root, replay_repository_root, executable, request_raw, run_raw, artifact_root, corpus_contract["replay_verification"]["timeout_seconds"])


def _verify_custodian_anchor(root: Path, manifest_hash: str, corpus_contract: dict, freeze_hash: str, preregistration_sha256: str) -> None:
    path_text=corpus_contract.get("custodian_anchor_path")
    if not isinstance(path_text,str) or not Path(path_text).is_absolute():
        raise PipelineError("stage0_custodian_anchor_unsealed",2)
    path=Path(path_text)
    try: status=path.lstat(); anchor,raw=_read(path,"stage0_custodian_anchor_invalid")
    except (OSError,PipelineError) as error: raise PipelineError("stage0_custodian_anchor_missing",2) from error
    if path.is_symlink() or not stat.S_ISREG(status.st_mode) or stat.S_IMODE(status.st_mode)!=0o600:
        raise PipelineError("stage0_custodian_anchor_mismatch",2)
    if not isinstance(anchor,dict) or "custodian_ed25519_signature_base64" not in anchor: raise PipelineError("stage0_custodian_signature_missing",2)
    _closed(anchor,{"schema","experiment_id","stage","stage0_root","stage0_artifact_manifest_sha256","signed_at_utc","preregistration_freeze_sha256","preregistration_sha256","custodian_ed25519_signature_base64"},"stage0_custodian_anchor_invalid")
    signature_text=anchor.get("custodian_ed25519_signature_base64")
    if not isinstance(signature_text,str) or not signature_text: raise PipelineError("stage0_custodian_signature_missing",2)
    public_text=corpus_contract.get("custodian_ed25519_public_key_base64"); public_hash=corpus_contract.get("custodian_ed25519_public_key_sha256")
    if not isinstance(public_text,str) or not public_text or not isinstance(public_hash,str): raise PipelineError("stage0_custodian_public_key_unsealed",2)
    try:
        public_bytes=base64.b64decode(public_text,validate=True); signature=base64.b64decode(signature_text,validate=True)
    except (ValueError,TypeError) as error: raise PipelineError("stage0_custodian_signature_invalid",2) from error
    if len(public_bytes)!=32 or len(signature)!=64 or sha256_bytes(public_bytes)!=public_hash: raise PipelineError("stage0_custodian_public_key_mismatch",2)
    body={key:anchor[key] for key in anchor if key!="custodian_ed25519_signature_base64"}
    try:
        from cryptography.exceptions import InvalidSignature
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    except ImportError as error: raise PipelineError("stage0_custodian_verifier_unavailable",3) from error
    try: Ed25519PublicKey.from_public_bytes(public_bytes).verify(signature,canonical_bytes(body))
    except InvalidSignature as error: raise PipelineError("stage0_custodian_signature_invalid",2) from error
    except (ValueError,TypeError) as error: raise PipelineError("stage0_custodian_public_key_mismatch",2) from error
    expected={"schema":"m20.custodian-stage0-anchor.v2","experiment_id":EXPERIMENT_ID,"stage":"stage0","stage0_root":str(root),"stage0_artifact_manifest_sha256":manifest_hash,"signed_at_utc":anchor.get("signed_at_utc"),"preregistration_freeze_sha256":freeze_hash,"preregistration_sha256":preregistration_sha256}
    try: datetime.strptime(anchor.get("signed_at_utc",""),"%Y-%m-%dT%H:%M:%SZ")
    except (ValueError,TypeError) as error: raise PipelineError("stage0_custodian_anchor_mismatch",2) from error
    if body!=expected or not isinstance(anchor.get("signed_at_utc"),str) or re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z",anchor["signed_at_utc"]) is None or not isinstance(freeze_hash,str) or len(freeze_hash)!=71 or not freeze_hash.startswith("sha256:"):
        raise PipelineError("stage0_custodian_anchor_mismatch",2)


def _stage0_root(selection_path: Path, reexecute_all: bool = False) -> tuple[Path, dict, str, dict]:
    if not isinstance(reexecute_all,bool): raise PipelineError("stage0_reexecution_mode_invalid",2)
    if selection_path.name != "stage0-selection.v1.json" or not selection_path.is_absolute():
        raise PipelineError("stage0_selection_path_invalid", 2)
    root = _safe_root(selection_path.parent, "stage0_root_invalid")
    manifest, manifest_hash = _verify_manifest(root, "m20.stage0-artifact-manifest.v1")
    if {path.name for path in root.iterdir()} != {
        "corpus-manifest.v1.json",
        "clusters",
        "stage0-result.v1.json",
        "stage0-selection.v1.json",
        "artifact-manifest.v1.json",
    }:
        raise PipelineError("stage0_closed_layout_invalid", 2)
    corpus, _ = _read(root / "corpus-manifest.v1.json")
    _closed(corpus, {"schema", "experiment_id", "cluster_ids", "clusters"}, "stage0_corpus_invalid")
    clusters = corpus["clusters"]
    if (
        corpus["schema"] != "m20.corpus-manifest.v1"
        or corpus["experiment_id"] != EXPERIMENT_ID
        or not isinstance(clusters, list)
        or len(clusters) != EXPECTED_CLUSTERS
        or not isinstance(corpus["cluster_ids"], list)
        or len(corpus["cluster_ids"]) != EXPECTED_CLUSTERS
    ):
        raise PipelineError("stage0_cluster_count_invalid", 2)
    registration,preregistration_sha256=_registration(); corpus_contract = registration.get("corpus_identity", {}); freeze_hash=registration.get("arm_neutral_contracts",{}).get("freeze_hashes",{}).get("freeze_manifest_sha256")
    _closed(corpus_contract,{"schema","repositories","cluster_count","cluster_manifest_sha256","product_cli_path","product_cli_sha256","replay_verification","custodian_anchor_path","custodian_ed25519_public_key_base64","custodian_ed25519_public_key_sha256"},"corpus_identity_contract_invalid")
    replay_contract=corpus_contract.get("replay_verification")
    _closed(replay_contract,{"schema","seed","sampled_build_count","timeout_seconds"},"corpus_identity_contract_invalid")
    pinned_repositories=corpus_contract.get("repositories")
    if corpus_contract.get("schema")!="m20.corpus-identity.v1" or corpus_contract.get("cluster_count")!=EXPECTED_CLUSTERS or not isinstance(pinned_repositories,list) or len(pinned_repositories)!=3 or replay_contract.get("schema")!="m20.stage0-product-replay-sample.v1" or replay_contract.get("seed")!="m20-stage0-product-replay-v1" or replay_contract.get("sampled_build_count")!=12 or replay_contract.get("timeout_seconds")!=1800:
        raise PipelineError("corpus_identity_contract_invalid",2)
    for row in pinned_repositories:
        _closed(row,{"url","repository_id","pinned_commit"},"corpus_identity_contract_invalid")
        if row.get("url") != "https://" + str(row.get("repository_id")) or not isinstance(row.get("pinned_commit"),str) or not row["pinned_commit"]: raise PipelineError("corpus_identity_contract_invalid",2)
    _verify_custodian_anchor(root,manifest_hash,corpus_contract,freeze_hash,preregistration_sha256)
    expected_ids = []
    repository_ids = set()
    repository_roots = set()
    first_builds = []
    all_build_keys=[(cluster["commit_cluster_id"],number) for cluster in clusters for number in (1,2) if isinstance(cluster,dict) and isinstance(cluster.get("commit_cluster_id"),str)]
    replay_keys=set(all_build_keys) if reexecute_all else set(sorted(all_build_keys,key=lambda item: hashlib.sha256((replay_contract["seed"]+"\0"+item[0]+"\0"+str(item[1])).encode()).digest())[:replay_contract["sampled_build_count"]])
    if len(all_build_keys)!=EXPECTED_CLUSTERS*2 or len(replay_keys)!=(EXPECTED_CLUSTERS*2 if reexecute_all else replay_contract["sampled_build_count"]): raise PipelineError("corpus_identity_contract_invalid",2)
    for cluster in clusters:
        _closed(cluster, {"commit_cluster_id", "repository_id", "repository_root", "base_commit_oid", "head_commit_oid"}, "stage0_cluster_invalid")
        try:
            identity = commit_cluster_id(cluster["repository_id"], cluster["base_commit_oid"], cluster["head_commit_oid"])
        except Stage0Error as error:
            raise PipelineError("stage0_cluster_invalid", 2) from error
        if identity != cluster["commit_cluster_id"] or not isinstance(cluster["repository_root"], str) or not cluster["repository_root"]:
            raise PipelineError("stage0_cluster_invalid", 2)
        expected_ids.append(identity)
        repository_ids.add(cluster["repository_id"])
        repository_roots.add(cluster["repository_root"])
        directory = root / "clusters" / identity.rsplit(":", 1)[-1]
        if directory.is_symlink() or not directory.is_dir() or {path.name for path in directory.iterdir()} != {"build-1", "build-2"}:
            raise PipelineError("stage0_build_layout_invalid", 2)
        builds = []
        byte_trees = []
        for number in (1, 2):
            build_root = directory / f"build-{number}"
            _verify_product_execution(build_root, corpus_contract, cluster["repository_root"] if (identity,number) in replay_keys else None)
            value, _ = _read(build_root / "cluster-result.v1.json")
            try:
                _validate_build(value, identity)
            except Stage0Error as error:
                raise PipelineError("stage0_build_invalid", 2) from error
            if any(value[field] != cluster[field] for field in ("repository_root", "base_commit_oid", "head_commit_oid")):
                raise PipelineError("stage0_build_cluster_mismatch", 2)
            if value["model_eligible"]:
                obligation = build_root / value["frozen_obligation_path"]
                if obligation.is_symlink() or not obligation.is_file() or sha256_bytes(obligation.read_bytes()) != value["frozen_obligation_sha256"]:
                    raise PipelineError("frozen_obligation_artifact_invalid", 2)
            builds.append(value)
            byte_trees.append({path.relative_to(build_root).as_posix(): path.read_bytes() for path in build_root.rglob("*") if path.is_file() and not path.is_symlink()})
        if canonical_bytes(builds[0]) != canonical_bytes(builds[1]) or byte_trees[0] != byte_trees[1]:
            raise PipelineError("stage0_build_pair_mismatch", 2)
        first_builds.append(builds[0])
    if len(repository_ids) != 3 or len(repository_roots) != 3 or expected_ids != sorted(expected_ids, key=str.encode) or corpus["cluster_ids"] != expected_ids or len(set(expected_ids)) != EXPECTED_CLUSTERS:
        raise PipelineError("stage0_corpus_membership_invalid", 2)
    cluster_identity_rows=[{"commit_cluster_id":cluster["commit_cluster_id"],"repository_id":cluster["repository_id"],"base_commit_oid":cluster["base_commit_oid"],"head_commit_oid":cluster["head_commit_oid"]} for cluster in clusters]
    identity_manifest={"schema":"m20.corpus-cluster-manifest.v1","clusters":cluster_identity_rows}
    pinned_ids={row["repository_id"]:row["pinned_commit"] for row in pinned_repositories}
    observed_heads={repository_id:{cluster["head_commit_oid"] for cluster in clusters if cluster["repository_id"]==repository_id} for repository_id in repository_ids}
    if hash_json(identity_manifest)!=corpus_contract["cluster_manifest_sha256"] or set(pinned_ids)!=repository_ids or any(pinned not in observed_heads[repository_id] for repository_id,pinned in pinned_ids.items()):
        raise PipelineError("stage0_corpus_identity_mismatch",2)
    cluster_directories = sorted(path.name for path in (root / "clusters").iterdir() if path.is_dir() and not path.is_symlink())
    if cluster_directories != sorted(identity.rsplit(":", 1)[-1] for identity in expected_ids):
        raise PipelineError("stage0_corpus_membership_invalid", 2)
    selection, selection_raw = _read(selection_path)
    _selection(selection)
    result, _ = _read(root / "stage0-result.v1.json")
    recomputed_gates = reduce_gates(first_builds, EXPECTED_CLUSTERS)
    expected_result = {"schema":"m20.stage0-result.v1", "experiment_id":EXPERIMENT_ID, "cluster_count":EXPECTED_CLUSTERS, "model_calls":0, "gates":recomputed_gates}
    if result != expected_result or any(row["passed"] is not True for row in recomputed_gates):
        raise PipelineError("stage0_result_invalid", 2)
    eligible = sorted((item["commit_cluster_id"] for item in first_builds if item["model_eligible"]), key=lambda identity: _hash_order("m20-commit-selection-v1", identity))
    expected_selection = {"schema":"m20.stage0-selection.v1", "experiment_id":EXPERIMENT_ID, "eligible_cluster_ids":sorted(eligible), "hash_ordered_cluster_ids":eligible, "stage1_cluster_ids":eligible[:10], "stage2a_cumulative_cluster_ids":eligible[:40]}
    expected_selection["selection_sha256"] = hash_json(expected_selection)
    if selection != expected_selection:
        raise PipelineError("selection_membership_invalid", 2)
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
    registration = parse_json_bytes((Path(__file__).resolve().parent.parent / "preregistration.json").read_bytes())
    gate = registration.get("backend_gate", {})
    listing = gate.get("pinned_listing_sha256")
    health = gate.get("pinned_health_sha256")
    executable_pins = {"reviewer":gate.get("reviewer_transport_sha256"), "judge":gate.get("judge_transport_sha256")}
    response_key_path=gate.get("response_hmac_key_path"); response_key_sha=gate.get("response_hmac_key_sha256")
    if gate.get("reviewer_transport_path") != REVIEWER_PATH or gate.get("judge_transport_path") != JUDGE_PATH or response_key_path!="/run/secrets/m20-evaluator-response-hmac-key" or not isinstance(response_key_sha,str) or len(response_key_sha)!=71 or not response_key_sha.startswith("sha256:") or not all(isinstance(value, str) and len(value) == 64 for value in (listing, health)) or any(not isinstance(value,str) or len(value)!=71 or not value.startswith("sha256:") for value in executable_pins.values()):
        raise PipelineError("backend_pin_contract_invalid", 3)
    if sha256_bytes(_response_hmac_key())!=response_key_sha: raise PipelineError("backend_response_hmac_key_stage_mismatch",3)
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
        if output[name]["sha256"] != executable_pins[name]:
            raise PipelineError("backend_executable_identity_mismatch", 3)
    output["reviewer"]["pinned_listing_sha256"] = listing
    output["reviewer"]["pinned_health_sha256"] = health
    output["reviewer"]["response_hmac_key_path"] = response_key_path
    output["reviewer"]["response_hmac_key_sha256"] = response_key_sha
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
    hidden_by_slot = {row["slot"]: row["hidden_arm_id"] for row in pair["slot_map"]}
    backend_integrity = True
    leakage_integrity = True
    judge_positive = {arm_a: False, arm_b: False}
    completed = {row["hidden_arm_id"]: row["completed"] is True for row in primary["scores"]}
    for slot in (0, 1):
        execution, _ = _read(run_root / f"slots/{slot}/execution.json")
        mechanical, _ = _read(run_root / f"slots/{slot}/mechanical.json")
        backend_integrity &= execution["process_exit"] == 0 and execution["timeout"] is False and execution["tool_calls"] == [] and execution["adapter_id"] == REVIEWER_ADAPTER
        leakage_integrity &= "reviewer_lens_leak" not in mechanical["failure_codes"]
        parsed_path = run_root / f"slots/{slot}/parsed.json"
        if parsed_path.is_file():
            parsed, _ = _read(parsed_path)
            disposition = parsed.get("disposition", {})
            claims = disposition.get("claims", []) if disposition.get("kind") == "claim" else []
            judge_positive[hidden_by_slot[slot]] = completed[hidden_by_slot[slot]] and any(claim.get("conclusion") == "issue_present" for claim in claims if isinstance(claim, dict))
    judge_execution, _ = _read(run_root / "judge/execution.json")
    utility, _ = _read(run_root / "judge/utility.json")
    backend_integrity &= judge_execution["process_exit"] == 0 and judge_execution["timeout"] is False and judge_execution["tool_calls"] == [] and judge_execution["adapter_id"] == JUDGE_ADAPTER
    judge_complete = judge_execution["parsed_present"] is True and len(utility.get("scores", [])) == 2
    return {
        "unit_id": unit["unit_id"],
        "repository_root": unit["repository_root"],
        "A": values[arm_a],
        "B": values[arm_b],
        "run_seal_id": verification["run_id"],
        "backend_integrity": backend_integrity,
        "leakage_integrity": leakage_integrity,
        "judge_complete": judge_complete,
        "judge_positive_A": judge_positive[arm_a],
        "judge_positive_B": judge_positive[arm_b],
    }


_GATE_REASONS = {
    "stage0": "stage0_gates_failed",
    "primary_rectangle": "paired_threshold_not_met",
    "sensitivity_tables": "sensitivity_tables_incomplete",
    "backend_integrity": "backend_integrity_failed",
    "leakage_integrity": "leakage_integrity_failed",
    "judge_completeness": "judge_batch_incomplete",
    "control_adequacy": "insufficient_control_observations",
    "safety": "safety_gate_failed",
}


def _decision(stage: str, gates: dict) -> tuple[str, list[str]]:
    if set(gates) != set(_GATE_REASONS) or not all(isinstance(value, bool) for value in gates.values()):
        raise PipelineError("stage_gate_contract_invalid", 4)
    reasons = [_GATE_REASONS[name] for name, passed in gates.items() if not passed]
    passed = not reasons
    return (("advance" if passed else "stop") if stage == "stage1" else ("success" if passed else "failure"), reasons)


def _reduce(stage: str, cells: list[dict], controls: dict, calls: dict, duration_seconds: str, stage0_verification: dict) -> dict:
    n00 = sum(row["A"] == 0 and row["B"] == 0 for row in cells); b = sum(row["A"] == 0 and row["B"] == 1 for row in cells); c = sum(row["A"] == 1 and row["B"] == 0 for row in cells); n11 = sum(row["A"] == 1 and row["B"] == 1 for row in cells)
    repositories = []
    for repository in sorted({row["repository_root"] for row in cells}, key=str.encode):
        subset = [row for row in cells if row["repository_root"] == repository]
        repositories.append({"repository_root": repository, "n": len(subset), "A0":sum(row["A"]==0 for row in subset), "A1":sum(row["A"]==1 for row in subset), "B0":sum(row["B"]==0 for row in subset), "B1":sum(row["B"]==1 for row in subset), "n00":sum(row["A"]==0 and row["B"]==0 for row in subset), "b": sum(row["A"] == 0 and row["B"] == 1 for row in subset), "c": sum(row["A"] == 1 and row["B"] == 0 for row in subset), "n11":sum(row["A"]==1 and row["B"]==1 for row in subset)})
    leave_one_out = [{"excluded_repository_root": row["repository_root"], "n": len(cells) - row["n"], "b": b - row["b"], "c": c - row["c"]} for row in repositories]
    rectangle = b >= (8 if stage == "stage1" else 18) and c <= (1 if stage == "stage1" else 7)
    labels = {row["commit_cluster_id"]: row["resolution"] for row in controls["labels"]}
    sampled = [labels.get(row["unit_id"], "unresolved") for row in cells]
    controls_ids = {row["unit_id"] for row in cells if labels.get(row["unit_id"]) == "clean_refactor_control"}
    minimum_controls = 2 if stage == "stage1" else 8
    safety_a = sum(row["judge_positive_A"] for row in cells if row["unit_id"] in controls_ids)
    safety_b = sum(row["judge_positive_B"] for row in cells if row["unit_id"] in controls_ids)
    expected_n=10 if stage=="stage1" else 40
    cell_shape=all(isinstance(row.get("A"),int) and not isinstance(row.get("A"),bool) and row["A"] in (0,1) and isinstance(row.get("B"),int) and not isinstance(row.get("B"),bool) and row["B"] in (0,1) and isinstance(row.get("repository_root"),str) and row["repository_root"] for row in cells)
    sensitivity_complete = len(cells)==expected_n and len({row.get("unit_id") for row in cells})==expected_n and cell_shape and len(repositories)==3 and len(leave_one_out)==3 and all(row["n"]>0 and row["A0"]+row["A1"]==row["n"] and row["B0"]+row["B1"]==row["n"] and row["n00"]+row["b"]+row["c"]+row["n11"]==row["n"] for row in repositories) and sum(row["n"] for row in repositories)==expected_n
    stage0_gate_ids=["prevalence","subject_retention","bounded_context","determinism","enumeration_honesty","fan_out","deferred_fraction"]
    stage0_passed=isinstance(stage0_verification,dict) and stage0_verification.get("schema")=="m20.stage0-result.v1" and stage0_verification.get("experiment_id")==EXPERIMENT_ID and stage0_verification.get("cluster_count")==EXPECTED_CLUSTERS and stage0_verification.get("model_calls")==0 and stage0_verification.get("gates")==[{"id":identifier,"passed":True} for identifier in stage0_gate_ids]
    gates = {
        "stage0": stage0_passed,
        "primary_rectangle": rectangle,
        "sensitivity_tables": sensitivity_complete,
        "backend_integrity": all(row["backend_integrity"] for row in cells),
        "leakage_integrity": all(row["leakage_integrity"] for row in cells),
        "judge_completeness": all(row["judge_complete"] for row in cells),
        "control_adequacy": len(controls_ids) >= minimum_controls,
        "safety": safety_b <= safety_a + 1,
    }
    decision, reasons = _decision(stage, gates)
    return {"schema": "m20.model_stage_result.v1", "stage": stage, "stage0_verification_sha256":hash_json(stage0_verification), "cells": cells, "n": len(cells), "n00": n00, "b": b, "c": c, "n11": n11, "repository_cells": repositories, "leave_one_repository_out": leave_one_out, "gates": gates, "control_summary": {"sampled": len(sampled), "agreed_clean": len(controls_ids), "minimum_required": minimum_controls, "unresolved": sum(value == "unresolved" for value in sampled)}, "safety_summary": {"agreed_controls": len(controls_ids), "judge_positive_A": safety_a, "judge_positive_B": safety_b}, "model_calls": calls, "budget": {"reviewer_output_tokens": REVIEWER_OUTPUT_TOKENS, "reviewer_timeout_seconds": REVIEWER_TIMEOUT_SECONDS, "judge_timeout_seconds": JUDGE_TIMEOUT_SECONDS, "authorized_model_seconds": STAGE1_MODEL_SECONDS if stage == "stage1" else STAGE2A_MODEL_SECONDS, "wall_envelope_seconds": STAGE1_WALL_SECONDS if stage == "stage1" else STAGE2A_WALL_SECONDS, "observed_active_seconds": duration_seconds}, "decision": decision, "reasons": reasons}


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
        cells.append(_cell(directory, unit) if calls else {"unit_id": unit["unit_id"], "repository_root": unit["repository_root"], "A": 0, "B": 0, "run_seal_id": result["run_seal_id"], "backend_integrity": True, "leakage_integrity": True, "judge_complete": False, "judge_positive_A": False, "judge_positive_B": False})
    return cells, {"reviewer": reviewer_calls, "judge": judge_calls}


def stage1(selection_path: str | Path, new_root: str | Path, controls_path: str | Path, transport) -> dict:
    from .stage0_driver import _production_freeze_gate
    _production_freeze_gate()
    selection_input = Path(selection_path)
    controls_input = Path(controls_path)
    if any(path.is_symlink() or not path.is_absolute() for path in (selection_input, controls_input)):
        raise PipelineError("authenticated_path_invalid", 2)
    stage0_root, selection, stage0_manifest_hash, stage0_verification = _stage0_root(selection_input)
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
    result = _reduce("stage1", cells, controls, calls, f"{elapsed:.6f}",stage0_verification)
    (root / "stage-result.v1.json").write_bytes(canonical_bytes(result)); _write_manifest(root, "m20.model-stage-artifact-manifest.v1")
    return result


def _verify_model_stage(root: Path, expected_stage: str | None = None, reexecute_all: bool = False) -> tuple[dict, dict, str]:
    _safe_root(root); _, artifact_hash = _verify_manifest(root, "m20.model-stage-artifact-manifest.v1")
    manifest, manifest_raw = _read(root / "stage-manifest.v1.json"); result, result_raw = _read(root / "stage-result.v1.json")
    _closed(manifest,{"schema","experiment_id","stage","active_freeze","source_stage0_root","stage0_artifact_manifest_sha256","selection_manifest_sha256","control_manifest_path","control_manifest_sha256","ordered_membership","units","packet_contract","budget_contract","public_seeds","fixed_transports","launch_preimages","predecessor"},"model_stage_manifest_invalid")
    if manifest.get("schema") != "m20.model_stage_manifest.v1" or result.get("schema") != "m20.model_stage_result.v1" or manifest.get("stage") != result.get("stage") or expected_stage is not None and manifest.get("stage") != expected_stage:
        raise PipelineError("model_stage_contract_invalid", 2)
    stage = manifest["stage"]; start = 1 if stage == "stage1" else 11
    expected_top={"stage-manifest.v1.json","stage-result.v1.json","artifact-manifest.v1.json","units"} | ({"selection","controls"} if stage=="stage1" else {"predecessor"})
    if {path.name for path in root.iterdir()} != expected_top: raise PipelineError("stage_closed_layout_invalid",2)
    stage0_root, selection, stage0_hash, stage0_verification = _stage0_root(Path(manifest["source_stage0_root"]) / "stage0-selection.v1.json",reexecute_all)
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
    expected = _reduce(stage, cells, controls, result["model_calls"], result["budget"]["observed_active_seconds"],stage0_verification)
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
    stage0_root, selection, stage0_manifest_hash, stage0_verification = _stage0_root(selection_path)
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
    result = _reduce("stage2a", predecessor_result["cells"] + added, controls, total_calls, f"{cumulative_active:.6f}",stage0_verification)
    (root / "stage-result.v1.json").write_bytes(canonical_bytes(result)); _write_manifest(root, "m20.model-stage-artifact-manifest.v1")
    return result


def verify_stage(root: str | Path, reexecute_all: bool = False) -> dict:
    path = Path(root)
    try:
        if (path / "stage0-selection.v1.json").is_file():
            _, selection, _, _ = _stage0_root(path / "stage0-selection.v1.json",reexecute_all); kind = "stage0"; identity = selection["selection_sha256"]
        elif (path / "control-labels.v1.json").is_file():
            value, raw = _read(path / "control-labels.v1.json"); _, selection, _, _ = _stage0_root(Path(value["stage0_selection_path"]),reexecute_all); _controls(path / "control-labels.v1.json", selection); kind = "controls"; identity = sha256_bytes(raw)
        else:
            manifest, result, identity = _verify_model_stage(path,reexecute_all=reexecute_all); kind = manifest["stage"]
        return {"schema": "m20.verify-stage.v1", "stage": kind, "identity_sha256": identity, "ok": True, "failure_code": None}
    except Exception as error:
        return {"schema": "m20.verify-stage.v1", "stage": None, "identity_sha256": None, "ok": False, "failure_code": getattr(error, "code", "stage_artifact_invalid")}
