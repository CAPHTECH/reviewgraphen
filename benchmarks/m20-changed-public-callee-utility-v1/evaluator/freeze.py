"""Portable G6 bundle identity and complete behavioral freeze gate."""; import ast; import platform; import shutil; import stat; import sys; import unicodedata; from pathlib import Path; from .canonical import canonical_bytes, hash_json, parse_json_bytes, sha256_bytes; from .semantic_acceptance import MEASUREMENT_SHA256, reference_sha256, verify_workspace_binding; from .spec_contract import FREEZE_MANIFEST_KEYS, execution_hash, verify_spec_contract; EVALUATOR_VERSION = "m20-evaluator.pipeline.v1"; ACTIVE_FREEZE_SUPERSEDES_SHA256 = "sha256:f3af4c7b6ec1e3bec422d25daaa51e9252e01d37537282cfc73de948fa0adcb8"; RUNTIME_REQUIREMENTS = {"schema": "m20.evaluator_runtime_requirements.v1", "implementation": "cpython", "python_version": [3, 13, 5], "unicodedata_version": "15.1.0", "stdlib_only": True}; ALLOWED = {".py", ".json", ".txt", ".md"}; SEED = "m20-evaluator-fixtures-v1"; VERSION = "m20-pipeline-fixture-generator.v1"
ATTACK_CLASSES = {
    "M01":"REQUIRED", "M02":"REQUIRED", "M03":"REQUIRED", "M04":"REQUIRED", "M05":"REQUIRED", "M06":"REQUIRED", "M07":"REQUIRED+AUDIT", "M08":"REQUIRED+AUDIT", "M09":"REQUIRED", "M10":"REQUIRED", "P01":"UNREPRESENTABLE+AUDIT", "P02":"UNREPRESENTABLE+AUDIT", "P03":"UNREPRESENTABLE+AUDIT", "P04":"UNREPRESENTABLE", "P05":"UNREPRESENTABLE+AUDIT", "P06":"REQUIRED", "P07":"REQUIRED", "P08":"UNREPRESENTABLE", "N01":"UNREPRESENTABLE", "N02":"REQUIRED", "N03":"UNREPRESENTABLE", "N04":"UNREPRESENTABLE", "N05":"REQUIRED", "N06":"REQUIRED", "N07":"UNREPRESENTABLE", "N08":"AUDIT", "N09":"AUDIT", "N10":"AUDIT", "N11":"UNREPRESENTABLE+AUDIT", "N12":"UNREPRESENTABLE+AUDIT", "N13":"REQUIRED", "N14":"REQUIRED", "N15":"REQUIRED", "N16":"REQUIRED", "N17":"UNREPRESENTABLE+REQUIRED", "N18":"REQUIRED", "N19":"REQUIRED", "N20":"REQUIRED", "N21":"REQUIRED", "N22":"REQUIRED", "N23":"REQUIRED", "N24":"REQUIRED", "N25":"REQUIRED", "H3R01":"REQUIRED", "H3R02":"HOSTILE", "H3D01":"REQUIRED", "H3D02":"HOSTILE", "H3N01":"REQUIRED", "H3A01":"REQUIRED", "H3F01":"REQUIRED", "H3C01":"REQUIRED", "X01":"REQUIRED", "X02":"REQUIRED", "X03":"REQUIRED", "X04":"REQUIRED", "X05":"REQUIRED", "X06":"REQUIRED", "X07":"REQUIRED", "X08":"REQUIRED", "OCC01":"REQUIRED", "OCC02":"REQUIRED", "CTX01":"REQUIRED", "CTX02":"REQUIRED", "CTX03":"REQUIRED", "CTX04":"REQUIRED", "CTX05":"REQUIRED", "CTX06":"REQUIRED", "CTX07":"REQUIRED", "SC01":"REQUIRED", "SC02":"REQUIRED", "H4F3":"HOSTILE", "H4F4":"HOSTILE"}


def _vectors() -> dict:
    vector_root=Path(__file__).with_name("reference_vectors"); versions={"text":"v1","source":"v1","loss":"v1","judge":"v1","occurrence":"v1","context":"v3"}
    return {name:parse_json_bytes((vector_root/f"{name}.{version}.json").read_bytes())["vectors"] for name,version in versions.items()}
def _full_runs(vectors: dict) -> list[dict]:
    from . import pipeline; from .tests.support import FIXTURE_ROOT, Transport, artifact_set, fixture_run, launch; cases=[("claim_success","claim","pass"),("abstention_success","abstention","pass"),("routine_only_zero","routine","pass"),("inconclusive_zero","inconclusive","pass"),("identifier_echo_zero","echo","pass"),("reviewer_decode_failure","malformed","pass"),("reviewer_empty_failure","empty","pass"),("judge_whole_batch_failure","claim","malformed")]; cases.extend((f"vector_{row['id']}","claim","pass") for name in ("text","source","loss","judge") for row in vectors[name]); output=[]; previous=pipeline._FIXTURE_EXECUTION_IDENTITY; pipeline._FIXTURE_EXECUTION_IDENTITY="sha256:"+"f"*64
    try:
        for name,reviewer,judge in cases: selected,run_root=launch(name); result=fixture_run(selected,Transport(reviewer,judge),run_root); output.append({"case":name,"launch":selected,"transport":{"reviewer":reviewer,"judge":judge},"expected_terminal_result":result,"artifacts":artifact_set(run_root)})
        for name,size in (("budget_exact_65536",65536),("budget_plus_one_65537",65537)):
            subject_size=65536 if size==65536 else 65537; selected,run_root=launch(name,source_bytes=65536,subject_bytes=subject_size); result=fixture_run(selected,Transport(),run_root); output.append({"case":name,"launch":selected,"transport":{"reviewer":"claim","judge":"pass","baseline_admitted_source_bytes":65536,"subject_admitted_source_bytes":subject_size},"expected_terminal_result":result,"artifacts":artifact_set(run_root)})
    finally:
        pipeline._FIXTURE_EXECUTION_IDENTITY=previous
        if FIXTURE_ROOT.exists(): shutil.rmtree(FIXTURE_ROOT)
    return output
def generated_values() -> dict: vectors=_vectors(); reference={"schema":"m20.reference-vectors.v1","seed":SEED,"generator_version":VERSION,**vectors}; fixtures={"schema":"m20.generated-fixtures.v1","seed":SEED,"generator_version":VERSION,"runs":_full_runs(vectors)}; attacks={"schema":"m20.attacks.v1","seed":SEED,"attacks":[{"attack_id":identifier,"classification":classification,"operation":"semantic_mutation_or_hostile_boundary_"+identifier.lower(),"oracle":"section_11_named_oracle_"+identifier.lower()} for identifier,classification in ATTACK_CLASSES.items()]}; partial={"reference_vectors.generated.json":reference,"fixtures.generated.json":fixtures,"attacks.generated.json":attacks}; inventory={"schema":"m20.generated-inventory.v1","files":{name:{"byte_length":len(canonical_bytes(value)),"sha256":sha256_bytes(canonical_bytes(value))} for name,value in partial.items()}}; return {**partial,"inventory.generated.json":inventory}
def generated_bytes() -> dict[str,bytes]: return {name:canonical_bytes(value) for name,value in generated_values().items()}


def write_generated(root: Path) -> None:
    root.mkdir(parents=True,exist_ok=True)
    for name,data in generated_bytes().items(): (root/name).write_bytes(data)


def check_generated(root: Path) -> bool: return all((root/name).is_file() and (root/name).read_bytes()==data for name,data in generated_bytes().items())


def runtime_compatible() -> bool: return sys.implementation.name == RUNTIME_REQUIREMENTS["implementation"] and list(sys.version_info[:3]) == RUNTIME_REQUIREMENTS["python_version"] and unicodedata.unidata_version == RUNTIME_REQUIREMENTS["unicodedata_version"]
def runtime_provenance() -> dict: executable = Path(sys.executable); return {"python_version": list(sys.version_info[:3]), "executable_sha256": sha256_bytes(executable.read_bytes()), "implementation": sys.implementation.name, "unicodedata_version": unicodedata.unidata_version, "platform": platform.platform(), "stdlib_only": True}
def _text(data: bytes) -> bool:
    try: data.decode("utf-8", "strict"); return not data.startswith(b"\xef\xbb\xbf") and b"\r" not in data and data.endswith(b"\n") and not data.endswith(b"\n\n")
    except UnicodeDecodeError: return False
def _imports(root: Path, paths: list[Path]) -> None:
    local = {path.stem for path in paths if path.parent == root and path.suffix == ".py"} | {path.name for path in root.iterdir() if path.is_dir() and not path.is_symlink()}; stdlib = set(sys.stdlib_module_names)
    for path in paths:
        if path.suffix != ".py": continue
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        dynamic_aliases={"eval","exec","__import__"}; process_aliases=set(); os_aliases=set()
        relative=path.relative_to(root).as_posix(); executable_declared=relative in {"cli.py","repository.py","stage0_production.py"} or relative.startswith("tests/")
        for node in ast.walk(tree):
            if isinstance(node,ast.Import):
                for alias in node.names:
                    if alias.name=="subprocess":process_aliases.add(alias.asname or alias.name)
                    if alias.name=="os":os_aliases.add(alias.asname or alias.name)
                    if alias.name=="builtins": dynamic_aliases.add(alias.asname or alias.name)
            elif isinstance(node,ast.ImportFrom) and node.module=="builtins":
                dynamic_aliases.update(alias.asname or alias.name for alias in node.names if alias.name in {"eval","exec","__import__"})
            elif isinstance(node,ast.ImportFrom) and node.module=="subprocess": process_aliases.update(alias.asname or alias.name for alias in node.names if alias.name in {"run","Popen","call","check_call","check_output"})
        for node in ast.walk(tree):
            if isinstance(node, (ast.Import, ast.ImportFrom)):
                if isinstance(node, ast.ImportFrom) and node.level:
                    if node.module and node.module.split(".")[0] not in local and path.parent == root: raise ValueError("freeze_import_invalid")
                    continue
                names = [alias.name.split(".")[0] for alias in node.names] if isinstance(node, ast.Import) else [node.module.split(".")[0]] if node.module else []
                if any(name not in stdlib and name != "evaluator" for name in names): raise ValueError("freeze_import_invalid")
            if isinstance(node, (ast.Import,ast.ImportFrom)) and any(alias.name.split(".")[0]=="importlib" for alias in node.names): raise ValueError("freeze_dynamic_code_invalid")
            if isinstance(node,ast.Subscript) and isinstance(node.value,ast.Name) and node.value.id=="__builtins__" and isinstance(node.slice,ast.Constant) and node.slice.value in {"eval","exec","__import__"}: raise ValueError("freeze_dynamic_code_invalid")
            if isinstance(node, ast.Call):
                if isinstance(node.func,ast.Name) and node.func.id in dynamic_aliases: raise ValueError("freeze_dynamic_code_invalid")
                if isinstance(node.func,ast.Attribute) and node.func.attr in {"eval","exec","__import__","import_module"}: raise ValueError("freeze_dynamic_code_invalid")
                if isinstance(node.func,ast.Name) and node.func.id=="getattr" and len(node.args)>=2 and isinstance(node.args[1],ast.Constant) and node.args[1].value in {"eval","exec","__import__","import_module"}: raise ValueError("freeze_dynamic_code_invalid")
                process_call=isinstance(node.func,ast.Name) and node.func.id in process_aliases or isinstance(node.func,ast.Attribute) and isinstance(node.func.value,ast.Name) and node.func.value.id in process_aliases and node.func.attr in {"run","Popen","call","check_call","check_output"}
                os_call=isinstance(node.func,ast.Attribute) and isinstance(node.func.value,ast.Name) and node.func.value.id in os_aliases and node.func.attr in {"system","popen","spawnl","spawnlp","spawnv","spawnvp"}
                if os_call or process_call and not executable_declared: raise ValueError("freeze_executable_lookup_invalid")
def file_records(root: Path) -> list[dict]:
    paths = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink(): raise ValueError("freeze_symlink_invalid")
        if path.is_dir(): continue
        if "__pycache__" in path.parts or path.suffix in {".pyc", ".tmp"} or path.name.startswith(".coverage"): continue
        if path.suffix not in ALLOWED or stat.S_IMODE(path.stat().st_mode) & 0o111: raise ValueError("freeze_file_invalid")
        relative = path.relative_to(root).as_posix()
        if relative != unicodedata.normalize("NFC", relative) or relative.startswith("/") or any(part in {"", ".", ".."} for part in relative.split("/")): raise ValueError("freeze_path_invalid")
        data = path.read_bytes()
        if path.suffix in {".py", ".md", ".txt"} and not _text(data): raise ValueError("freeze_text_invalid")
        if path.suffix == ".json" and canonical_bytes(parse_json_bytes(data)) != data: raise ValueError("freeze_json_invalid")
        paths.append(path)
    _imports(root, paths); return [{"path": path.relative_to(root).as_posix(), "kind": path.suffix[1:], "byte_length": path.stat().st_size, "sha256": sha256_bytes(path.read_bytes())} for path in paths]
def identities(root: Path) -> tuple[list[dict], str, str, str]: files = file_records(root); bundle = hash_json({"schema": "m20.evaluator_bundle.v1", "evaluator_version": EVALUATOR_VERSION, "files": files}); requirements = hash_json(RUNTIME_REQUIREMENTS); execution = execution_hash(bundle, requirements, reference_sha256()); return files, bundle, requirements, execution
def execution_identity(root: Path) -> str: return identities(root)[3]
def acceptance_gate(root: Path) -> tuple[str, str, str]:
    if not runtime_compatible(): raise ValueError("runtime_incompatible")
    reference_sha256()
    from .tests.attack_oracles import run_all_attacks; from .tests.vector_runner import run_vectors
    if not check_generated(root / "generated"): raise ValueError("generated_fixture_mismatch")
    vector = run_vectors(); attacks = run_all_attacks(root)
    if vector["failed"] or attacks["survived"]: raise ValueError("acceptance_gate_failed")
    generated = root / "generated"; return (sha256_bytes((generated / "inventory.generated.json").read_bytes()), sha256_bytes((generated / "reference_vectors.generated.json").read_bytes()), sha256_bytes((generated / "attacks.generated.json").read_bytes()))
def freeze_manifest(root: Path, design_spec: Path) -> dict:
    verify_spec_contract(design_spec); verify_workspace_binding(design_spec.parents[2]); fixture_hash, vector_hash, attack_hash = acceptance_gate(root); files, bundle, requirement_hash, execution = identities(root); design_raw = design_spec.read_bytes()
    if not _text(design_raw): raise ValueError("design_spec_invalid")
    manifest = {"schema": "m20.evaluator_freeze.v1", "evaluator_version": EVALUATOR_VERSION, "design_spec_sha256": sha256_bytes(design_raw), "files": files, "evaluator_bundle_sha256": bundle, "runtime_requirements": RUNTIME_REQUIREMENTS, "runtime_requirements_sha256": requirement_hash, "semantic_acceptance_reference_sha256": reference_sha256(), "measurement_record_sha256": MEASUREMENT_SHA256, "supersedes_freeze_manifest_sha256": ACTIVE_FREEZE_SUPERSEDES_SHA256, "evaluator_execution_sha256": execution, "runtime_provenance": runtime_provenance(), "generated_fixture_inventory_sha256": fixture_hash, "reference_vector_set_sha256": vector_hash, "mutation_manifest_sha256": attack_hash}
    if set(manifest) != set(FREEZE_MANIFEST_KEYS): raise ValueError("implementation_manifest_keys_mismatch")
    return manifest
def verify_manifest(root: Path, design_spec: Path, manifest: dict) -> dict:
    try:
        verify_spec_contract(design_spec); required = set(FREEZE_MANIFEST_KEYS)
        if not isinstance(manifest, dict) or set(manifest) != required: raise ValueError("manifest_closed")
        if manifest["schema"] != "m20.evaluator_freeze.v1" or manifest["evaluator_version"] != EVALUATOR_VERSION: raise ValueError("manifest_identity_invalid")
        files, bundle, requirement_hash, execution = identities(root); bundle_valid = manifest["files"] == files and manifest["evaluator_bundle_sha256"] == bundle; design_valid = manifest["design_spec_sha256"] == sha256_bytes(design_spec.read_bytes()); contract_valid = manifest["runtime_requirements"] == RUNTIME_REQUIREMENTS and manifest["runtime_requirements_sha256"] == requirement_hash and manifest["semantic_acceptance_reference_sha256"] == reference_sha256() and manifest["measurement_record_sha256"] == MEASUREMENT_SHA256 and manifest["supersedes_freeze_manifest_sha256"] == ACTIVE_FREEZE_SUPERSEDES_SHA256 and manifest["evaluator_execution_sha256"] == execution; compatible = runtime_compatible(); checks = None
        if compatible:
            try: verify_workspace_binding(design_spec.parents[2]); observed = acceptance_gate(root); checks = list(observed) == [manifest["generated_fixture_inventory_sha256"], manifest["reference_vector_set_sha256"], manifest["mutation_manifest_sha256"]]
            except ValueError: checks = False
        ok = bundle_valid and design_valid and contract_valid and compatible and checks is True; return {"schema": "m20.verify-frozen.v1", "bundle_valid": bundle_valid, "design_spec_valid": design_valid, "execution_contract_valid": contract_valid, "runtime_compatible": compatible, "checks_valid": checks, "ok": ok}
    except Exception: return {"schema": "m20.verify-frozen.v1", "bundle_valid": False, "design_spec_valid": False, "execution_contract_valid": False, "runtime_compatible": runtime_compatible(), "checks_valid": False if runtime_compatible() else None, "ok": False}
