"""Pinned ReviewGraphen CLI boundary used by the m21 evaluator.

The product audit is never accepted from a caller.  Each invocation creates a
private clone, writes the fixed request itself, runs the pinned binary, and
validates the resulting canonical artifact closure before returning it.
"""

from __future__ import annotations

import hashlib
import shutil
import subprocess
import tempfile
import time
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path

from .canonical import CanonicalError, canonical_bytes, hash_json, parse_json_bytes, sha256, write


REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
PRODUCT_CLI = REPOSITORY_ROOT / "target/debug/reviewgraphen"
PRODUCT_CLI_SHA256 = "sha256:20ee1c3228ad3297f41dbf464ae6c447b42fe79afe74eab435c91209c5667b00"
PREREGISTRATION_PATH = Path(__file__).resolve().parents[1] / "preregistration.json"
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


@dataclass(frozen=True)
class ContextProductRun:
    packet: dict
    packet_bytes: bytes
    request_sha256: str
    packet_artifact_sha256: str
    manifest_sha256: str
    execution_elapsed_ns: int


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


def _context_identity_mismatch(field: str, observed: str) -> ProductError:
    return ProductError(f"context_product_identity_mismatch:{field}", observed)


def _load_context_product_pin() -> dict:
    """Read the sole expected identity authority from canonical preregistration."""
    try:
        source = PREREGISTRATION_PATH.read_bytes()
        preregistration = parse_json_bytes(source)
    except (OSError, CanonicalError) as error:
        raise ProductError("context_product_preregistration_invalid", type(error).__name__) from error
    encoded = canonical_bytes(preregistration)
    if source not in {encoded, encoded + b"\n"}:
        raise ProductError("context_product_preregistration_invalid", "not canonical")
    try:
        pin = preregistration["product_pins"]["context_projection"]
    except (KeyError, TypeError) as error:
        raise ProductError("context_product_preregistration_invalid", "pin missing") from error
    required = {
        "binary_path", "binary_sha256", "build_command", "cargo", "commit",
        "context_policy_hash", "context_policy_id", "extractor_version", "frozen",
        "profile_id", "profile_version", "rule_set_hash", "rule_set_material",
        "rustc", "tree", "worktree",
    }
    if (
        not isinstance(pin, dict)
        or set(pin) != required
        or any(type(pin[key]) is not str for key in required - {"frozen"})
        or type(pin["frozen"]) is not bool
    ):
        raise ProductError("context_product_preregistration_invalid", "pin shape")
    return pin


def _observed_tool_version(tool: str) -> str:
    executable = shutil.which(tool)
    if executable is None:
        raise _context_identity_mismatch(tool, "not found")
    try:
        result = subprocess.run(
            [executable, "-V"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            shell=False,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise _context_identity_mismatch(tool, type(error).__name__) from error
    if result.returncode:
        raise _context_identity_mismatch(tool, f"exit {result.returncode}")
    return result.stdout.decode("utf-8", "strict").strip()


def _schema_const(schema: dict, *path: str) -> str:
    value = schema
    try:
        for key in path:
            value = value[key]
    except (KeyError, TypeError) as error:
        raise ProductError("context_product_identity_surface_invalid", ".".join(path)) from error
    if not isinstance(value, str):
        raise ProductError("context_product_identity_surface_invalid", ".".join(path))
    return value


def _verify_context_product_cli() -> dict:
    """Bind context execution to every preregistered source/tool/extractor pin."""
    pin = _load_context_product_pin()
    root = Path(pin["worktree"])
    cli = Path(pin["binary_path"])
    if cli != root / "target/release/reviewgraphen":
        raise _context_identity_mismatch("binary_path", str(cli))
    try:
        stat = cli.lstat()
        digest = "sha256:" + hashlib.sha256(cli.read_bytes()).hexdigest()
    except OSError as error:
        raise _context_identity_mismatch("binary_sha256", "missing") from error
    if (
        cli.is_symlink()
        or not cli.is_file()
        or not stat.st_mode & 0o111
        or cli.resolve() != cli
    ):
        raise _context_identity_mismatch("binary_path", str(cli))
    if digest != pin["binary_sha256"]:
        raise _context_identity_mismatch("binary_sha256", digest)
    try:
        commit = _git(root, "rev-parse", "HEAD").strip()
        tree = _git(root, "rev-parse", "HEAD^{tree}").strip()
    except ProductError as error:
        raise _context_identity_mismatch("worktree", error.record["detail"]) from error
    for field, observed in (("commit", commit), ("tree", tree)):
        if observed != pin[field]:
            raise _context_identity_mismatch(field, observed)
    for field in ("rustc", "cargo"):
        observed = _observed_tool_version(field)
        if observed != pin[field]:
            raise _context_identity_mismatch(field, observed)
    try:
        request_schema = parse_json_bytes(
            _run(
                [str(cli), "schema", "print", "reviewgraphen.context_request.v1"],
                root,
                timeout=120,
            )
        )
        packet_schema = parse_json_bytes(
            _run(
                [str(cli), "schema", "print", "reviewgraphen.context_packet.v1"],
                root,
                timeout=120,
            )
        )
    except CanonicalError as error:
        raise ProductError("context_product_identity_surface_invalid", str(error)) from error
    observed_extractor = {
        "profile_id": _schema_const(
            request_schema, "properties", "ingest", "properties", "profile_id", "const"
        ),
        "profile_version": _schema_const(
            request_schema, "properties", "ingest", "properties", "profile_version", "const"
        ),
        "extractor_version": _schema_const(
            request_schema, "properties", "ingest", "properties", "extractor_version", "const"
        ),
        "context_policy_id": _schema_const(
            request_schema, "properties", "context_policy_id", "const"
        ),
        "context_policy_hash": _schema_const(
            packet_schema, "properties", "context_policy_hash", "const"
        ),
    }
    for field, observed in observed_extractor.items():
        if observed != pin[field]:
            raise _context_identity_mismatch(field, observed)
    observed_rule_set_hash = sha256(pin["rule_set_material"].encode("utf-8"))
    if observed_rule_set_hash != pin["rule_set_hash"]:
        raise _context_identity_mismatch("rule_set_hash", observed_rule_set_hash)
    return pin


def _context_request_from_verified_pin(
    repository_id: str,
    revision: str,
    subject_symbol_ids: list[str],
    verified_pin: dict,
) -> dict:
    """Construct the only admitted request; no task prose or hint is accepted."""
    if (
        not isinstance(repository_id, str)
        or not repository_id
        or not isinstance(revision, str)
        or len(revision) != 40
        or any(character not in "0123456789abcdef" for character in revision)
        or not isinstance(subject_symbol_ids, list)
        or not subject_symbol_ids
        or len(subject_symbol_ids) > 64
        or any(not isinstance(symbol_id, str) or ":" not in symbol_id for symbol_id in subject_symbol_ids)
        or subject_symbol_ids != sorted(set(subject_symbol_ids))
    ):
        raise ProductError("context_request_invalid")
    return {
        "schema": "reviewgraphen.context_request.v1",
        "repository_identity": repository_id,
        "revision": revision,
        "ingest": {
            "profile_id": verified_pin["profile_id"],
            "profile_version": verified_pin["profile_version"],
            "rule_set_hash": verified_pin["rule_set_hash"],
            "extractor_version": verified_pin["extractor_version"],
            "max_files": 20_000,
            "max_file_bytes": 16_777_216,
            "max_total_source_bytes": 17_179_869_184,
        },
        "subject": {"state": "resolved", "symbol_ids": subject_symbol_ids},
        "context_policy_id": verified_pin["context_policy_id"],
    }


def _context_request(repository_id: str, revision: str, subject_symbol_ids: list[str]) -> dict:
    """Build a request only after independently observing the preregistered identity."""
    return _context_request_from_verified_pin(
        repository_id,
        revision,
        subject_symbol_ids,
        _verify_context_product_cli(),
    )


def _context_packet_identity(packet: dict) -> dict:
    return {
        key: packet[key]
        for key in (
            "context",
            "context_policy_hash",
            "context_policy_id",
            "extractor_set_hash",
            "extractor_version",
            "profile_id",
            "profile_version",
            "repository_identity",
            "request_id",
            "revision",
            "rule_set_hash",
            "schema",
            "snapshot_id",
            "tree_hash",
        )
    }


def _verify_context_packet_extractor_identity(packet: dict, verified_pin: dict) -> None:
    """Reject product echo drift before any packet-derived measurement is exposed."""
    for field in ("profile_id", "profile_version", "rule_set_hash", "extractor_version"):
        observed = packet.get(field)
        if observed != verified_pin[field]:
            raise _context_identity_mismatch(field, str(observed))


def _validate_context_product_output(
    repo: Path,
    request: dict,
    request_bytes: bytes,
    packet_bytes: bytes,
    artifact_packet_bytes: bytes,
    manifest_bytes: bytes,
    execution_elapsed_ns: int,
    verified_pin: dict,
) -> ContextProductRun:
    try:
        packet = parse_json_bytes(packet_bytes)
        artifact_packet = parse_json_bytes(artifact_packet_bytes)
        manifest = parse_json_bytes(manifest_bytes)
    except CanonicalError as error:
        raise ProductError("context_product_artifact_invalid", str(error)) from error
    if (
        canonical_bytes(packet) != packet_bytes
        or canonical_bytes(artifact_packet) != artifact_packet_bytes
        or canonical_bytes(manifest) != manifest_bytes
    ):
        raise ProductError("context_product_artifact_not_canonical")
    if packet_bytes != artifact_packet_bytes or packet != artifact_packet:
        raise ProductError("context_product_stdout_artifact_mismatch")
    required = {
        "schema", "packet_id", "packet_sha256", "request_id", "repository_identity",
        "revision", "snapshot_id", "tree_hash", "profile_id", "profile_version",
        "rule_set_hash", "extractor_version", "extractor_set_hash", "context_policy_id",
        "context_policy_hash", "context",
    }
    if not isinstance(packet, dict) or set(packet) != required:
        raise ProductError("context_product_packet_invalid")
    _verify_context_packet_extractor_identity(packet, verified_pin)
    revision = resolve_commit(repo, request["revision"])
    tree = _git(repo, "rev-parse", "--verify", revision + "^{tree}").strip()
    snapshot = expected_snapshot_id(request["repository_identity"], revision, tree)
    expected_request_id = _product_id(
        "request",
        {"request_sha256": sha256(request_bytes), "schema": request["schema"]},
    )
    identity_hash = sha256(canonical_bytes(_context_packet_identity(packet)))
    context = packet.get("context")
    if (
        packet["schema"] != "reviewgraphen.context_packet.v1"
        or packet["packet_sha256"] != identity_hash
        or packet["packet_id"] != "context-packet:" + identity_hash
        or packet["request_id"] != expected_request_id
        or packet["repository_identity"] != request["repository_identity"]
        or packet["revision"] != revision
        or packet["snapshot_id"] != snapshot
        or packet["tree_hash"] != "git:" + tree
        or packet["profile_id"] != request["ingest"]["profile_id"]
        or packet["profile_version"] != request["ingest"]["profile_version"]
        or packet["rule_set_hash"] != request["ingest"]["rule_set_hash"]
        or packet["extractor_version"] != request["ingest"]["extractor_version"]
        or packet["context_policy_id"] != verified_pin["context_policy_id"]
        or packet["context_policy_hash"] != verified_pin["context_policy_hash"]
        or not isinstance(context, dict)
        or context.get("request_id") != expected_request_id
        or context.get("snapshot_id") != snapshot
        or context.get("subject_binding") != request["subject"]
    ):
        raise ProductError("context_product_identity_closure_invalid")
    accounting_keys = {
        "accepted_file_denominator", "reached_file_denominator",
        "materialized_source_denominator", "support_anchor_denominator",
        "latent_cardinality", "declared_losses", "support_loss_summaries", "unknowns",
    }
    if not accounting_keys <= set(context):
        raise ProductError("context_product_accounting_missing")
    artifact_hash = sha256(artifact_packet_bytes)
    expected_manifest = {
        "schema": "reviewgraphen.context_artifact_manifest.v1",
        "request_sha256": sha256(request_bytes),
        "request_id": expected_request_id,
        "packet_id": packet["packet_id"],
        "snapshot_id": snapshot,
        "artifacts": [{
            "path": "context_packet.v1.json",
            "role": "context_packet",
            "byte_length": len(artifact_packet_bytes),
            "sha256": artifact_hash,
        }],
    }
    if manifest != expected_manifest:
        raise ProductError("context_product_manifest_invalid")
    if type(execution_elapsed_ns) is not int or execution_elapsed_ns <= 0:
        raise ProductError("context_product_measurement_invalid")
    return ContextProductRun(
        packet=packet,
        packet_bytes=packet_bytes,
        request_sha256=sha256(request_bytes),
        packet_artifact_sha256=artifact_hash,
        manifest_sha256=sha256(manifest_bytes),
        execution_elapsed_ns=execution_elapsed_ns,
    )


def run_context_product(
    repository: str | Path,
    repository_id: str,
    revision: str,
    subject_symbol_ids: list[str],
) -> ContextProductRun:
    """Execute the pinned request->packet path; callers cannot inject a CLI or hint."""
    verified_pin = _verify_context_product_cli()
    context_product_cli = Path(verified_pin["binary_path"])
    repo = Path(repository)
    if repo.is_symlink() or not repo.is_dir():
        raise ProductError("repository_not_allowed")
    repo = repo.resolve(strict=True)
    resolved = resolve_commit(repo, revision)
    history = _git(repo, "rev-list", "--all", "--parents").splitlines()
    if history != [resolved]:
        raise ProductError("base_workspace_history_present")
    request = _context_request_from_verified_pin(
        repository_id,
        resolved,
        subject_symbol_ids,
        verified_pin,
    )
    request_bytes = canonical_bytes(request)
    with tempfile.TemporaryDirectory(prefix="m21-context-command-", dir=repo) as temporary:
        root = Path(temporary)
        request_path = root / "context-request.v1.json"
        write(request_path, request)
        artifacts = root / "artifacts"
        _run(
            [str(context_product_cli), "schema", "validate", str(request_path)],
            repo,
            timeout=120,
        )
        started = time.monotonic_ns()
        stdout = _run(
            [
                str(context_product_cli),
                "context",
                "--request",
                str(request_path.relative_to(repo)),
                "--artifacts",
                str(artifacts.relative_to(repo)),
            ],
            repo,
            timeout=PRODUCT_TIMEOUT_SECONDS,
        )
        execution_elapsed_ns = time.monotonic_ns() - started
        packet_path = artifacts / "context_packet.v1.json"
        manifest_path = artifacts / "context-artifact-manifest.v1.json"
        try:
            artifact_packet_bytes = packet_path.read_bytes()
            manifest_bytes = manifest_path.read_bytes()
        except OSError as error:
            raise ProductError("context_product_artifact_missing") from error
        _run(
            [str(context_product_cli), "schema", "validate", str(packet_path)],
            repo,
            timeout=120,
        )
        return _validate_context_product_output(
            repo,
            request,
            request_bytes,
            stdout,
            artifact_packet_bytes,
            manifest_bytes,
            execution_elapsed_ns,
            verified_pin,
        )


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
