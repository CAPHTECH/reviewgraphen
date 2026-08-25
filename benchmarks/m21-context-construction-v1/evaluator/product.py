"""Pinned ReviewGraphen CLI boundary used by the m21 evaluator.

The product audit is never accepted from a caller.  Each invocation creates a
private clone, writes the fixed request itself, runs the pinned binary, and
validates the resulting canonical artifact closure before returning it.
"""

from __future__ import annotations

import hashlib
import subprocess
import tempfile
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path

from .canonical import CanonicalError, canonical_bytes, hash_json, parse_json_bytes, sha256, write


REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
PRODUCT_CLI = REPOSITORY_ROOT / "target/debug/reviewgraphen"
PRODUCT_CLI_SHA256 = "sha256:20ee1c3228ad3297f41dbf464ae6c447b42fe79afe74eab435c91209c5667b00"
GIT = Path("/usr/bin/git")
PRODUCT_TIMEOUT_SECONDS = 1_800
_ENV = {
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_CONFIG_COUNT": "0",
    "LC_ALL": "C",
    "PATH": "/usr/bin:/bin",
}


class ProductError(ValueError):
    """Typed failure at the pinned product boundary."""

    def __init__(self, code: str, detail: str | None = None):
        self.record = {
            "schema": "m21.typed_failure.v1",
            "code": code,
            "detail": detail or code,
        }
        super().__init__(code)


@dataclass(frozen=True)
class ProductRun:
    audit: dict
    diagnostics: dict
    audit_sha256: str
    manifest_sha256: str
    diagnostics_sha256: str
    snapshot_id: str
    universe_id: str
    ingest_operations: int
    projection_operations: int
    ingest_elapsed_ns: int
    projection_elapsed_ns: int


def _run(command: list[str], cwd: Path, *, timeout: int = 60) -> bytes:
    try:
        result = subprocess.run(
            command,
            cwd=cwd,
            env=_ENV,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            shell=False,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ProductError("product_process_failed", type(error).__name__) from error
    if result.returncode:
        detail = result.stderr.decode("utf-8", "replace")[:500]
        raise ProductError("product_process_failed", detail)
    return result.stdout


def _git(repo: Path, *args: str) -> str:
    return _run([str(GIT), "-C", str(repo), *args], repo).decode("utf-8", "strict")


def resolve_commit(repo: Path, value: str) -> str:
    if not isinstance(value, str) or len(value) not in {40, 64}:
        raise ProductError("git_commit_oid_invalid")
    if any(character not in "0123456789abcdef" for character in value):
        raise ProductError("git_commit_oid_invalid")
    resolved = _git(repo, "rev-parse", "--verify", value + "^{commit}").strip()
    if resolved != value:
        raise ProductError("git_commit_oid_invalid")
    return resolved


def commit_parent(repo: Path, oid: str) -> str:
    parents = _git(repo, "show", "-s", "--format=%P", oid).split()
    if len(parents) != 1:
        raise ProductError("git_single_parent_required")
    return resolve_commit(repo, parents[0])


def _validate_base_tree_workspace(repo: Path, base_oid: str, tree_oid: str) -> None:
    """Prove the exported repository contains exactly one shallow base closure."""
    if (repo / ".git/objects/info/alternates").exists():
        raise ProductError("base_workspace_not_isolated", "alternate object store")
    if _git(repo, "rev-parse", "--verify", "HEAD^{commit}").strip() != base_oid:
        raise ProductError("base_workspace_identity_mismatch")
    if _git(repo, "rev-parse", "--verify", "HEAD^{tree}").strip() != tree_oid:
        raise ProductError("base_workspace_identity_mismatch")
    history = _git(repo, "rev-list", "--all", "--parents").splitlines()
    if history != [base_oid]:
        raise ProductError("base_workspace_history_present")
    reachable = {
        line.split(" ", 1)[0]
        for line in _git(repo, "rev-list", "--objects", base_oid).splitlines()
        if line
    }
    physical = {
        line.split(" ", 1)[0]
        for line in _git(repo, "cat-file", "--batch-check", "--batch-all-objects").splitlines()
        if line
    }
    if physical != reachable:
        raise ProductError("base_workspace_extra_objects")
    if _git(repo, "status", "--porcelain", "--untracked-files=all"):
        raise ProductError("base_workspace_tree_dirty")


@contextmanager
def base_tree_workspace(repository: str | Path, base_oid: str):
    """Yield an isolated shallow checkout whose only object closure is B."""
    source = Path(repository)
    if source.is_symlink() or not source.is_dir():
        raise ProductError("repository_not_allowed")
    source = source.resolve(strict=True)
    base = resolve_commit(source, base_oid)
    tree = _git(source, "rev-parse", "--verify", base + "^{tree}").strip()
    with tempfile.TemporaryDirectory(prefix="m21-base-tree-") as temporary:
        root = Path(temporary)
        detached = root / "repository"
        detached.mkdir()
        _run([str(GIT), "init", "-q", str(detached)], root)
        _run(
            [
                str(GIT),
                "-C",
                str(detached),
                "fetch",
                "-q",
                "--depth=1",
                "--no-tags",
                source.as_uri(),
                base,
            ],
            root,
            timeout=120,
        )
        _run([str(GIT), "-C", str(detached), "checkout", "-q", "--detach", base], root)
        # FETCH_HEAD embeds the source URL and is not needed after checkout.
        try:
            (detached / ".git/FETCH_HEAD").unlink()
        except FileNotFoundError:
            pass
        _validate_base_tree_workspace(detached, base, tree)
        yield detached


def _product_id(kind: str, body: dict) -> str:
    return f"{kind}:{hash_json(body)}"


def expected_snapshot_id(repository_id: str, target_oid: str, target_tree_oid: str) -> str:
    repository = _product_id("repository", {"identity": repository_id})
    return _product_id(
        "snapshot",
        {
            "repository": repository,
            "target_revision": target_oid,
            "tree_hash": "git:" + target_tree_oid,
        },
    )


def _request(repository_id: str, base_oid: str, target_oid: str) -> dict:
    return {
        "schema": "reviewgraphen.generic_review_request.v3",
        "repository_identity": repository_id,
        "workspace_admission_root": ".",
        "repository_admission_root": ".",
        "base_revision": base_oid,
        "target_revision": target_oid,
        "context_policy_id": "context.subject_windows@3",
        "ingest": {
            "profile_id": "rust.production.v1",
            "profile_version": "1",
            "rule_set_hash": "sha256:8f6bfbfb2dbf2f0eaf916b152ddba1e422db8b6de95f931c0c78c9ee4d050b47",
            "max_files": 20_000,
            "max_file_bytes": 4_194_304,
            "max_total_source_bytes": 67_108_864,
        },
        "plan": {"max_waves": 8, "max_obligations_per_wave": 2_048},
        "observer": {"kind": "deterministic_abstain"},
        "verifier_descriptor_id": None,
    }


def _verify_product_cli() -> None:
    try:
        stat = PRODUCT_CLI.lstat()
        digest = "sha256:" + hashlib.sha256(PRODUCT_CLI.read_bytes()).hexdigest()
    except OSError as error:
        raise ProductError("product_cli_identity_mismatch", "missing pinned CLI") from error
    if PRODUCT_CLI.is_symlink() or not PRODUCT_CLI.is_file() or not stat.st_mode & 0o111:
        raise ProductError("product_cli_identity_mismatch", "pinned CLI is not an executable regular file")
    if PRODUCT_CLI.resolve() != PRODUCT_CLI or digest != PRODUCT_CLI_SHA256:
        raise ProductError("product_cli_identity_mismatch", digest)


def _manifest_entry(manifest: dict, path: str) -> dict:
    rows = manifest.get("artifacts")
    if not isinstance(rows, list):
        raise ProductError("product_manifest_invalid")
    matches = [row for row in rows if isinstance(row, dict) and row.get("path") == path]
    if len(matches) != 1:
        raise ProductError("product_manifest_invalid")
    return matches[0]


def _validate_product_output(
    repo: Path,
    repository_id: str,
    base_oid: str,
    target_oid: str,
    request_bytes: bytes,
    audit_bytes: bytes,
    manifest_bytes: bytes,
    diagnostics_bytes: bytes,
) -> ProductRun:
    """Validate bytes produced inside the private product artifact root."""
    try:
        audit = parse_json_bytes(audit_bytes)
        manifest = parse_json_bytes(manifest_bytes)
        diagnostics = parse_json_bytes(diagnostics_bytes)
    except CanonicalError as error:
        raise ProductError("product_artifact_invalid", str(error)) from error
    if any(
        canonical_bytes(value) != data
        for value, data in ((audit, audit_bytes), (manifest, manifest_bytes), (diagnostics, diagnostics_bytes))
    ):
        raise ProductError("product_artifact_not_canonical")
    if audit.get("schema") != "reviewgraphen.generic_review_run.v3":
        raise ProductError("product_audit_schema_invalid")
    if manifest.get("schema") != "reviewgraphen.generic_review_artifact_manifest.v1":
        raise ProductError("product_manifest_invalid")
    if diagnostics.get("schema") != "reviewgraphen.generic_review_diagnostics.v1":
        raise ProductError("product_diagnostics_invalid")
    audit_hash = sha256(audit_bytes)
    audit_entry = _manifest_entry(manifest, "audit.run.v3.json")
    if audit_entry.get("sha256") != audit_hash or audit_entry.get("byte_length") != len(audit_bytes):
        raise ProductError("product_audit_hash_mismatch")
    if manifest.get("request_sha256") != sha256(request_bytes):
        raise ProductError("product_request_hash_mismatch")

    base_tree = _git(repo, "rev-parse", "--verify", base_oid + "^{tree}").strip()
    target_tree = _git(repo, "rev-parse", "--verify", target_oid + "^{tree}").strip()
    snapshot = expected_snapshot_id(repository_id, target_oid, target_tree)
    legacy = audit.get("legacy_ingestion")
    if not isinstance(legacy, dict) or legacy != {
        "repository_identity": repository_id,
        "program_space_id": snapshot,
        "snapshot_id": snapshot,
        "base_commit_oid": base_oid,
        "base_tree_hash": "git:" + base_tree,
        "target_commit_oid": target_oid,
        "target_tree_hash": "git:" + target_tree,
    }:
        raise ProductError("product_audit_revision_mismatch")

    plan = audit.get("plan")
    universe = plan.get("universe_id") if isinstance(plan, dict) else None
    if not isinstance(universe, str) or not universe.startswith("universe:sha256:"):
        raise ProductError("product_universe_id_invalid")
    if any(
        context.get("context", {}).get("snapshot_id") != snapshot
        for context in audit.get("contexts", [])
        if isinstance(context, dict)
    ):
        raise ProductError("product_snapshot_closure_mismatch")
    if manifest.get("snapshot_id") != snapshot or manifest.get("universe_id") != universe:
        raise ProductError("product_manifest_identity_mismatch")
    if manifest.get("request_id") != audit.get("request_id") or manifest.get("run_id") != audit.get("run_id"):
        raise ProductError("product_manifest_identity_mismatch")
    if (
        diagnostics.get("request_id") != audit.get("request_id")
        or diagnostics.get("run_id") != audit.get("run_id")
        or diagnostics.get("terminal_code") != "0"
    ):
        raise ProductError("product_diagnostics_invalid")
    stages = diagnostics.get("stages")
    expected_stages = ["ingest", "synthesize", "context", "observer", "report", "artifact_write"]
    if (
        not isinstance(stages, list)
        or [row.get("stage") for row in stages if isinstance(row, dict)] != expected_stages
        or any(row.get("status") != "completed" for row in stages if isinstance(row, dict))
        or any(type(row.get("elapsed_microseconds")) is not int or row["elapsed_microseconds"] < 0 for row in stages if isinstance(row, dict))
    ):
        raise ProductError("product_diagnostics_invalid")
    ingest_operations = sum(row["stage"] == "ingest" and row["status"] == "completed" for row in stages)
    projection_operations = sum(row["stage"] == "context" and row["status"] == "completed" for row in stages)
    ingest_elapsed_ns = next(row["elapsed_microseconds"] for row in stages if row["stage"] == "ingest") * 1_000
    projection_elapsed_ns = next(row["elapsed_microseconds"] for row in stages if row["stage"] == "context") * 1_000

    return ProductRun(
        audit=audit,
        diagnostics=diagnostics,
        audit_sha256=audit_hash,
        manifest_sha256=sha256(manifest_bytes),
        diagnostics_sha256=sha256(diagnostics_bytes),
        snapshot_id=snapshot,
        universe_id=universe,
        ingest_operations=ingest_operations,
        projection_operations=projection_operations,
        ingest_elapsed_ns=ingest_elapsed_ns,
        projection_elapsed_ns=projection_elapsed_ns,
    )


def run_product_review(repository: str | Path, repository_id: str, base_oid: str, target_oid: str) -> ProductRun:
    """Run the pinned CLI; no CLI path, audit, or artifact root is caller-controlled."""
    _verify_product_cli()
    source = Path(repository)
    if source.is_symlink() or not source.is_dir():
        raise ProductError("repository_not_allowed")
    source = source.resolve(strict=True)
    base = resolve_commit(source, base_oid)
    target = resolve_commit(source, target_oid)
    request = _request(repository_id, base, target)
    request_bytes = canonical_bytes(request)

    with tempfile.TemporaryDirectory(prefix="m21-product-") as temporary:
        root = Path(temporary)
        detached = root / "repository"
        _run(
            [str(GIT), "clone", "-q", "--shared", "--no-checkout", str(source), str(detached)],
            root,
            timeout=120,
        )
        request_path = detached / "m21-request.json"
        write(request_path, request)
        artifacts = detached / "m21-product-output"
        diagnostics_path = detached / "m21-product-diagnostics.json"
        _run(
            [
                str(PRODUCT_CLI),
                "review",
                "--request",
                request_path.name,
                "--artifacts",
                artifacts.name,
                "--diagnostics",
                diagnostics_path.name,
            ],
            detached,
            timeout=PRODUCT_TIMEOUT_SECONDS,
        )
        audit_path = artifacts / "audit.run.v3.json"
        manifest_path = artifacts / "artifact-manifest.v1.json"
        try:
            audit_bytes = audit_path.read_bytes()
            manifest_bytes = manifest_path.read_bytes()
            diagnostics_bytes = diagnostics_path.read_bytes()
        except OSError as error:
            raise ProductError("product_artifact_missing") from error

        # The fixed CLI's own semantic validator is an additional closure
        # check; it is not a substitute for the independent Git/OID checks.
        _run(
            [str(PRODUCT_CLI), "schema", "validate", str(audit_path)],
            detached,
            timeout=120,
        )
        _run(
            [str(PRODUCT_CLI), "schema", "validate", str(diagnostics_path)],
            detached,
            timeout=120,
        )
        return _validate_product_output(
            source,
            repository_id,
            base,
            target,
            request_bytes,
            audit_bytes,
            manifest_bytes,
            diagnostics_bytes,
        )
