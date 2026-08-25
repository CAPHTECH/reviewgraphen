"""Deterministic full-pipeline fixture support; never imported by production."""
import base64
import hashlib
import os
import shutil
import zlib
from contextlib import contextmanager
from pathlib import Path

from evaluator.canonical import canonical_bytes, hash_json, parse_json_bytes, sha256_bytes
from evaluator.model_boundary import DIMENSIONS, ModelResult
from evaluator.pipeline import CONTEXT_HASH, PROFILE_HASH
from evaluator.repository import GitRepository
from evaluator.stage0_contract import CONTEXT_POLICY_ID, build_context_projection_v3, build_occurrence_closure

FIXTURE_ROOT = Path("/tmp") / ("m20-evaluator-pipeline-fixture-v1" + os.environ.get("M20_SWEEP_WORKER", ""))
LOGICAL_ROOT = "/m20-fixture-v1"


def _object(git: Path, kind: str, body: bytes) -> str:
    framed = kind.encode() + b" " + str(len(body)).encode() + b"\0" + body
    oid = hashlib.sha1(framed).hexdigest()
    target = git / "objects" / oid[:2] / oid[2:]
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(zlib.compress(framed))
    return oid


def _tree(git: Path, entries: list[tuple[str, str, str]]) -> str:
    body = b"".join(mode.encode() + b" " + name.encode() + b"\0" + bytes.fromhex(oid) for mode, name, oid in sorted(entries, key=lambda x: x[1].encode()))
    return _object(git, "tree", body)


def repository(root: Path, source_bytes: int | None = None, subject_bytes: int | None = None) -> dict:
    git = root / ".git"
    (git / "objects").mkdir(parents=True)
    (git / "refs" / "heads").mkdir(parents=True)
    (git / "config").write_text("[core]\n\trepositoryformatversion = 0\n\tbare = false\n", encoding="utf-8", newline="\n")
    (git / "HEAD").write_text("ref: refs/heads/main\n", encoding="ascii", newline="\n")
    base_size = source_bytes // 2 if source_bytes is not None else None
    head_size = source_bytes - base_size if source_bytes is not None else None
    base_body = b"pub fn f() -> i32 { 1 }\n" if base_size is None else b"a" * (base_size - 1) + b"\n"
    head_body = b"pub fn f() -> i32 { 2 }\n" if head_size is None else b"b" * (head_size - 1) + b"\n"
    base_good = _object(git, "blob", base_body)
    head_good = _object(git, "blob", head_body)
    base_bad = _object(git, "blob", b"\xff\n")
    head_bad = _object(git, "blob", b"\xfe\n")
    subject = _object(git, "blob", b"s" * (subject_bytes - 1) + b"\n") if subject_bytes is not None else None
    base_entries = [("100644", "bad.rs", base_bad), ("100644", "lib.rs", base_good)]
    head_entries = [("100644", "bad.rs", head_bad), ("100644", "lib.rs", head_good)]
    if subject is not None:
        base_entries.append(("100644", "subject.rs", subject)); head_entries.append(("100644", "subject.rs", subject))
    base_src = _tree(git, base_entries)
    head_src = _tree(git, head_entries)
    base_tree = _tree(git, [("40000", "src", base_src)])
    head_tree = _tree(git, [("40000", "src", head_src)])
    base = _object(git, "commit", f"tree {base_tree}\nauthor A <a@a> 0 +0000\ncommitter A <a@a> 0 +0000\n\nbase\n".encode())
    head = _object(git, "commit", f"tree {head_tree}\nparent {base}\nauthor A <a@a> 1 +0000\ncommitter A <a@a> 1 +0000\n\nhead\n".encode())
    (git / "refs" / "heads" / "main").write_text(head + "\n", encoding="ascii", newline="\n")
    return {"base": base, "head": head, "base_good": base_good, "head_good": head_good, "head_bad": head_bad, "subject": subject}


def launch(case: str = "claim", source_bytes: int | None = None, subject_bytes: int | None = None) -> tuple[dict, Path]:
    if FIXTURE_ROOT.exists():
        shutil.rmtree(FIXTURE_ROOT)
    FIXTURE_ROOT.mkdir()
    repo = FIXTURE_ROOT / "repository"
    repo.mkdir()
    objects = repository(repo, source_bytes, subject_bytes)
    unit = "fixture-" + case
    sources = [
        {"required_id": "required:bad", "role": "changed", "snapshot_side": "head", "path": "src/bad.rs", "start_line": 1, "end_line": 1, "blob_oid": objects["head_bad"]},
        {"required_id": "required:good", "role": "changed", "snapshot_side": "head", "path": "src/lib.rs", "start_line": 1, "end_line": 1, "blob_oid": objects["head_good"]},
    ]
    if subject_bytes is not None:
        sources[1] = {"required_id": "required:subject", "role": "changed", "snapshot_side": "head", "path": "src/subject.rs", "start_line": 1, "end_line": 1, "blob_oid": objects["subject"]}
    elif source_bytes is not None:
        sources.append({"required_id": "required:base-good", "role": "context", "snapshot_side": "base", "path": "src/lib.rs", "start_line": 1, "end_line": 1, "blob_oid": objects["base_good"]})
    endpoint_pairs = [{"caller_endpoint_id": "endpoint:caller", "callee_endpoint_id": "endpoint:callee"}]
    subject_windows = [{"subject_id": "subject:callee", "window_id": "window:callee", "role": "callee"}, {"subject_id": "subject:caller", "window_id": "window:caller", "role": "caller"}]
    file_ids = [f"file:{index:04d}" for index in range(len(sources))]
    projection_windows = [
        {"window_id":"window:callee","source_artifact_id":file_ids[0],"start_line":1,"end_line":1,"role":"callee","source_required_id":sources[0]["required_id"],"support_anchor_ids":[]},
        {"window_id":"window:caller","source_artifact_id":file_ids[1],"start_line":1,"end_line":1,"role":"caller","source_required_id":sources[1]["required_id"],"support_anchor_ids":[]},
    ]
    projection_windows.extend({"window_id":f"window:support:{index}","source_artifact_id":file_ids[index],"start_line":1,"end_line":1,"role":"support","source_required_id":sources[index]["required_id"],"support_anchor_ids":[]} for index in range(2,len(sources)))
    projection = build_context_projection_v3(
        {"projection_id":"projection:fixture","snapshot_id":"snapshot:fixture","request_id":"request:fixture","obligation_ids":["obligation:fixture"],"relation_ids":["relation:fixture"],"endpoint_pairs":endpoint_pairs},
        file_ids,
        file_ids,
        [
            {"role":"callee","status":"admitted","endpoint_id":"endpoint:callee","source_artifact_id":file_ids[0],"start_line":1,"end_line":1,"window_id":"window:callee"},
            {"role":"caller","status":"admitted","endpoint_id":"endpoint:caller","source_artifact_id":file_ids[1],"start_line":1,"end_line":1,"window_id":"window:caller"},
        ],
        [{"source_artifact_id":file_id,"snapshot_side":source["snapshot_side"],"path":source["path"],"blob_oid":source["blob_oid"]} for file_id,source in zip(file_ids,sources)],
        [],
        projection_windows,
        [],
        {"state":"unknown","capability_states":{"direct_calls":"partial"},"qualification_ids":["qualification:fixture"]},
        [],
        [],
        [],
    )
    obligation = {"schema": "m20.frozen_obligation.v1", "unit_id": unit, "rule_id": "relation.changed_public_callee@1", "property_id": "rust.callee_contract_review@1", "obligation_ids": ["obligation:fixture"], "relation_ids": ["relation:fixture"], "endpoint_pairs": endpoint_pairs, "subject_windows": subject_windows, "sources": sources, "required_references": [], "projection": projection, "bounded_scope_manifest_id": "scope:fixture"}
    obligation_path = FIXTURE_ROOT / "obligation.json"
    obligation_path.write_bytes(canonical_bytes(obligation))
    occurrence_closure = build_occurrence_closure(
        {"snapshot_id":"snapshot:fixture","target_revision":objects["head"],"legacy_program_space_sha256":"sha256:"+"1"*64,"legacy_extraction_report_sha256":"sha256:"+"2"*64},
        [{"file_source_id":file_ids[0],"path":sources[0]["path"]}],
        [],
        [file_ids[0]],
        {"schema":"m20.stage0-occurrence-metrics.v1","cluster_id":unit,"wall_time_milliseconds":0,"peak_bytes":0},
    )
    stage = {"schema": "m20.stage_manifest.v1", "experiment_id": "m20-changed-public-callee-utility-v1", "unit_id": unit, "repository_root": LOGICAL_ROOT + "/repository", "repository_allow_list": [LOGICAL_ROOT + "/repository"], "base_commit_oid": objects["base"], "head_commit_oid": objects["head"], "frozen_obligation_sha256": sha256_bytes(obligation_path.read_bytes()), "profile_id": "rust.production.v1", "profile_sha256": PROFILE_HASH, "context_policy_id": CONTEXT_POLICY_ID, "context_policy_sha256": CONTEXT_HASH, "occurrence_closure": occurrence_closure, "public_seeds": {"arm_order": "m20-arm-order-v1", "judge_permutation": "m20-judge-permutation-v1"}, "backend_adapters": {"reviewer": "fixture-reviewer.v1", "judge": "fixture-judge.v1"}}
    stage_path = FIXTURE_ROOT / "stage.json"
    stage_path.write_bytes(canonical_bytes(stage))
    value = {"schema": "m20.pipeline_launch.v1", "experiment_id": stage["experiment_id"], "unit_id": unit, "repository_root": stage["repository_root"], "base_commit_oid": objects["base"], "head_commit_oid": objects["head"], "frozen_obligation_path": LOGICAL_ROOT + "/obligation.json", "frozen_obligation_sha256": stage["frozen_obligation_sha256"], "stage_manifest_path": LOGICAL_ROOT + "/stage.json", "stage_manifest_sha256": sha256_bytes(stage_path.read_bytes()), "context_policy_id": CONTEXT_POLICY_ID, "context_policy_sha256": CONTEXT_HASH}
    return value, FIXTURE_ROOT / "run"


def fixture_file(logical_path: str) -> Path:
    """Resolve a stable fixture identity to its isolated physical test file."""
    prefix = LOGICAL_ROOT + "/"
    if not logical_path.startswith(prefix):
        raise ValueError("fixture_logical_path_invalid")
    relative = logical_path[len(prefix):]
    if not relative or relative.startswith("/") or ".." in Path(relative).parts:
        raise ValueError("fixture_logical_path_invalid")
    return FIXTURE_ROOT / relative


@contextmanager
def fixture_adapters():
    from evaluator import pipeline
    actual_repo = GitRepository(str(FIXTURE_ROOT / "repository"), [str(FIXTURE_ROOT / "repository")])
    class RepositoryAdapter:
        def __getattr__(self, name):
            return getattr(actual_repo, name)
        def provenance(self):
            return {**actual_repo.provenance(), "repository_root": LOGICAL_ROOT + "/repository"}
    original_repository, original_read = pipeline.GitRepository, pipeline._read_authenticated
    def authenticated(path_text, expected_hash):
        try:
            path = fixture_file(path_text)
        except ValueError:
            return original_read(path_text, expected_hash)
        raw = path.read_bytes()
        if sha256_bytes(raw) != expected_hash:
            raise pipeline.PipelineError("authenticated_hash_mismatch", 2)
        return parse_json_bytes(raw), raw
    pipeline.GitRepository = lambda root, allow_list: RepositoryAdapter()
    pipeline._read_authenticated = authenticated
    try:
        yield
    finally:
        pipeline.GitRepository, pipeline._read_authenticated = original_repository, original_read


def fixture_run(selected, transport, output):
    from evaluator import pipeline
    with fixture_adapters():
        return pipeline.RUN(selected, transport, output)


class Transport:
    def __init__(self, reviewer="claim", judge="pass"):
        self.reviewer_mode, self.judge_mode = reviewer, judge

    def descriptor(self):
        return {"reviewer": "fixture-reviewer.v1", "judge": "fixture-judge.v1"}

    def review(self, request: bytes, slot: int, timeout: int):
        packet = parse_json_bytes(request)
        if self.reviewer_mode == "empty": return ModelResult(b"")
        if self.reviewer_mode == "malformed": return ModelResult(b"{")
        source = packet["source_inventory"]["admitted_sources"][0]
        observation = {"source_id": source["source_id"], "start_line": source["range"]["start_line"], "end_line": source["range"]["end_line"]}
        if self.reviewer_mode in {"abstention", "routine"}:
            loss = next(item for item in packet["source_inventory"]["declared_losses"] if item["reason"] == ("routine_scope_omission" if self.reviewer_mode == "routine" else "task_blocking_source_unavailable"))
            disposition = {"kind": "abstention", "claims": [], "abstention": {"reason": "task_blocking_source_unavailable", "basis_loss_ids": [loss["loss_id"]], "observations": [observation], "blocked_question": "Required source bytes remain unavailable for exact review", "needed_evidence": "Provide exact source bytes and rerun deterministic extraction"}}
        else:
            summary = source["source_id"] if self.reviewer_mode == "echo" else "Changed behavior affects callers through a concrete return value"
            conclusion = "inconclusive" if self.reviewer_mode == "inconclusive" else "issue_present"
            disposition = {"kind": "claim", "claims": [{"conclusion": conclusion, "summary": summary, "observations": [observation], "mechanism": {"trigger": "Calling the changed function selects the modified branch", "observed_behavior": "The returned integer changes for the same direct invocation", "consequence": "Existing callers can observe a different contract result"}}], "abstention": None}
        return ModelResult(canonical_bytes({"schema": "arm-neutral.source-grounded-disposition@1", "task_id": packet["task_id"], "source_inventory_id": packet["source_inventory"]["source_inventory_id"], "disposition": disposition}))

    def judge(self, request: bytes, instruction: bytes, timeout: int):
        if self.judge_mode == "malformed": return ModelResult(b"{")
        batch = parse_json_bytes(request)
        scores = []
        for candidate in batch["candidates"]:
            forced = candidate["mechanical_state"]["kind"] == "mechanical_forced_zero"
            dimensions = {name: 0 if forced else (2 if name in {"mechanism_or_blocker_specificity", "audit_actionability"} else 1) for name in DIMENSIONS}
            scores.append({"candidate_id": candidate["candidate_id"], "packet_sha256": candidate["packet_sha256"], "output_artifact_sha256": candidate["output_artifact_sha256"], "binding_view_sha256": candidate["binding_view_sha256"], "mechanical_score_sha256": candidate["mechanical_score_sha256"], "score_source": "mechanical_forced_zero" if forced else "judge", "dimensions": dimensions, "total": sum(dimensions.values()), "verdict": "not_usable" if forced else "usable"})
        return ModelResult(canonical_bytes({"schema": "m20.utility_judge_batch_output.v1", "batch_id": batch["batch_id"], "scores": scores}))


def artifact_set(root: Path) -> list[dict]:
    return [{"path": path.relative_to(root).as_posix(), "bytes_base64": base64.b64encode(path.read_bytes()).decode(), "byte_length": path.stat().st_size, "sha256": sha256_bytes(path.read_bytes())} for path in sorted(root.rglob("*")) if path.is_file()]
