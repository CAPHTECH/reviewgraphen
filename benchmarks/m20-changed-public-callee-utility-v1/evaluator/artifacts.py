"""One-way production sink and a separate hostile-artifact verifier."""; import os; from pathlib import Path; from .canonical import canonical_bytes, hash_json, parse_json_bytes, sha256_bytes, stable_id
class ArtifactError(ValueError): pass
class ArtifactSink:
    """Create-only sink.  Deliberately has no read operation."""
    def __init__(self, root: Path): self.root = root; root.mkdir(parents=False, exist_ok=False); self.entries: list[dict] = []
    def bytes(self, relative: str, data: bytes, kind: str) -> None:
        if not relative or relative.startswith("/") or any(part in {"", ".", ".."} for part in relative.split("/")): raise ArtifactError("artifact_path_invalid")
        target = self.root.joinpath(*relative.split("/")); target.parent.mkdir(parents=True, exist_ok=True); descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        try:
            with os.fdopen(descriptor, "wb") as handle: handle.write(data); handle.flush(); os.fsync(handle.fileno())
        except Exception:
            try: os.close(descriptor)
            except OSError: pass
            raise
        self.entries.append({"path": relative, "kind": kind, "byte_length": len(data), "sha256": sha256_bytes(data)})
    def json(self, relative: str, value: dict, kind: str) -> None: self.bytes(relative, canonical_bytes(value), kind)
    def finalize(self, execution_sha256: str, stage_sha256: str, run_id: str, terminal: str) -> dict: body = {"schema": "m20.artifact-ledger.v1", "entries": sorted(self.entries, key=lambda x: x["path"].encode())}; ledger = {**body, "ledger_sha256": hash_json(body)}; self.json("ledger.json", ledger, "ledger"); seal_body = {"schema": "m20.run-seal.v1", "run_id": run_id, "evaluator_execution_sha256": execution_sha256, "stage_manifest_sha256": stage_sha256, "ledger_sha256": ledger["ledger_sha256"], "pipeline_terminal_state": terminal}; seal = {**seal_body, "run_seal_id": stable_id("m20-run-seal", seal_body)}; self.json("seal.json", seal, "seal"); return seal
def _closed(value, fields, label):
    if not isinstance(value, dict) or set(value) != set(fields): raise ArtifactError(label + "_closed")
    return value
def verify_run(root: Path) -> dict:
    """Read hostile artifacts, independently close bytes, then replay semantics."""
    try:
        if root.is_symlink() or not root.is_dir(): raise ArtifactError("run_root_invalid")
        files = {}
        for path in root.rglob("*"):
            if path.is_symlink() or not path.is_file():
                if path.is_dir() and not path.is_symlink(): continue
                raise ArtifactError("artifact_type_invalid")
            relative = path.relative_to(root).as_posix(); files[relative] = path.read_bytes()
        if "ledger.json" not in files or "seal.json" not in files: raise ArtifactError("artifact_missing")
        ledger = parse_json_bytes(files["ledger.json"]); _closed(ledger, {"schema", "entries", "ledger_sha256"}, "ledger"); body = {"schema": "m20.artifact-ledger.v1", "entries": ledger["entries"]}
        if canonical_bytes(ledger) != files["ledger.json"]: raise ArtifactError("ledger_canonical_invalid")
        if ledger["schema"] != body["schema"] or ledger["ledger_sha256"] != hash_json(body) or not isinstance(ledger["entries"], list): raise ArtifactError("ledger_hash_invalid")
        expected_paths = [entry.get("path") for entry in ledger["entries"]]
        if expected_paths != sorted(expected_paths, key=lambda x: x.encode()) or len(set(expected_paths)) != len(expected_paths) or set(files) != set(expected_paths) | {"ledger.json", "seal.json"}: raise ArtifactError("ledger_paths_invalid")
        for entry in ledger["entries"]:
            _closed(entry, {"path", "kind", "byte_length", "sha256"}, "entry"); data = files[entry["path"]]
            if entry["byte_length"] != len(data) or entry["sha256"] != sha256_bytes(data): raise ArtifactError("ledger_entry_invalid")
        seal = parse_json_bytes(files["seal.json"]); _closed(seal, {"schema", "run_id", "evaluator_execution_sha256", "stage_manifest_sha256", "ledger_sha256", "pipeline_terminal_state", "run_seal_id"}, "seal"); seal_body = {key: seal[key] for key in ("schema", "run_id", "evaluator_execution_sha256", "stage_manifest_sha256", "ledger_sha256", "pipeline_terminal_state")}
        if canonical_bytes(seal) != files["seal.json"]: raise ArtifactError("seal_canonical_invalid")
        if seal["schema"] != "m20.run-seal.v1" or seal["ledger_sha256"] != ledger["ledger_sha256"] or seal["run_seal_id"] != stable_id("m20-run-seal", seal_body): raise ArtifactError("seal_invalid")
        json_records = {path: parse_json_bytes(data) for path, data in files.items() if path.endswith(".json") and path not in {"ledger.json", "seal.json"}}
        if any(canonical_bytes(value) != files[path] for path,value in json_records.items()): raise ArtifactError("artifact_json_canonical_invalid")
        from .pipeline import audit_records; audit_records(json_records, {path: data for path, data in files.items() if path.endswith("raw.bin")}, seal); return {"schema": "m20.verify-run.v1", "run_id": seal["run_id"], "ok": True, "failure_code": None, "token_observation_recomputable": False}
    except Exception as error: return {"schema": "m20.verify-run.v1", "run_id": None, "ok": False, "failure_code": getattr(error, "code", str(error) if isinstance(error, ArtifactError) else "artifact_invalid"), "token_observation_recomputable": False}
