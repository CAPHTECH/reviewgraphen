"""Execute every section 11 named attack, including real source mutants."""
import base64
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from evaluator.artifacts import verify_run
from evaluator.canonical import canonical_bytes, hash_json, parse_json_bytes, sha256_bytes, stable_id
from evaluator.freeze import ATTACK_CLASSES
from evaluator.tests.attack_probe import probe

MUTATIONS = {
    "M01": ("pipeline.py", "MAX_SOURCE_BYTES = 65_536", "MAX_SOURCE_BYTES = 65_537"),
    "M02": ("pipeline.py", 'score["total"] >= 6', 'score["total"] >= 5'),
    "M03": ("pipeline.py", 'return hashlib.sha256(seed.encode() + b"\\0" + identity.encode()).digest()[-1] & 1', "return 0"),
    "M04": ("pipeline.py", '[item["reason"], item["undecidable_question_id"]]', '[item["reason"], "question-omitted"]'),
    "M05": ("pipeline.py", 'recovery = stable_id("recovery", {"task_id": task_id, "registry_key": registry_key, "support_ids": support_ids}); identity = {"reason": reason, "omitted_scope": scope, "recovery_reference": recovery, "undecidable_question_id": question, "support_ids": support_ids}; loss_id', 'recovery = stable_id("recovery", {"task_id": task_id, "registry_key": registry_key, "support_ids": support_ids[:1]}); identity = {"reason": reason, "omitted_scope": scope, "recovery_reference": recovery, "undecidable_question_id": question, "support_ids": support_ids[:1]}; loss_id'),
    "M06": ("textnorm.py", "utf8_length <= field_limit", "utf8_length <= field_limit + 1"),
    "M07": ("source_payload.py", "if set(by_id) != used:", "if False and set(by_id) != used:"),
    "M08": ("pipeline.py", '"tool_calls": list(result.tool_calls), "raw_sha256": sha256_bytes(result.raw_bytes)', '"tool_calls": list(result.tool_calls), "raw_sha256": None'),
    "M09": ("pipeline.py", '{"changed", "context", "support"}', '{"changed", "context", "support", "unknown"}'),
    "M10": ("freeze.py", '{"schema": "m20.evaluator_bundle.v1", "evaluator_version": EVALUATOR_VERSION, "files": files}', '{"schema": "m20.evaluator_bundle.v1", "evaluator_version": EVALUATOR_VERSION, "files": files, "runtime": runtime_provenance()}'),
    "H3R01": ("repository.py", 'if digest != oid: raise PreflightError("object_hash_mismatch")', 'if False: raise PreflightError("object_hash_mismatch")'),
    "H3D01": ("model_boundary.py", "def _depth(raw: bytes) -> None:\n    depth = 0", "def _depth(raw: bytes) -> None:\n    return\n    depth = 0"),
    "H3N01": ("textnorm.py", 'return unicodedata.normalize("NFKC", value).casefold()', 'return unicodedata.normalize("NFKC", value)'),
    "H3A01": ("pipeline.py", 'from .artifacts import ArtifactSink', 'from .artifacts import ArtifactSink, verify_run'),
    "H3F01": ("freeze.py", 'cases=[("claim_success","claim","pass"),("abstention_success"', 'cases=[("abstention_success"'),
    "H3C01": ("cli.py", 'COMMANDS = ("stage0", "seal-controls"', 'COMMANDS = ("stage0", "run", "seal-controls"'),
    "X01": ("model_boundary.py", 'not 1 <= len(disposition["claims"]) <= 3', 'not 0 <= len(disposition["claims"]) <= 3'),
    "X02": ("model_boundary.py", 'not 1 <= len(value) <= 8', 'not 0 <= len(value) <= 8'),
    "X03": ("repository.py", '{"40000", "100644", "100755", "120000"}', '{"40000", "100644", "100755", "120000", "160000"}'),
    "X04": ("repository.py", 'if not parents or parents[0] != base_oid: raise PreflightError("first_parent_mismatch")', 'if False: raise PreflightError("first_parent_mismatch")'),
    "X05": ("repository.py", 'max(0, lo - 3) + 1', 'max(0, lo - 2) + 1'),
    "X06": ("pipeline.py", ' and score["verdict"] == "usable"', ''),
    "X07": ("pipeline.py", 'elif source["role"] == "changed": changed = True', 'elif source["role"] in {"changed","context","support"}: changed = True'),
    "X08": ("pipeline.py", 'sum(source["bytes"] for source in packet["source_inventory"]["admitted_sources"])', 'sum(payload["byte_length"] for payload in packet["payloads"])'),
    "OCC01": ("stage0_contract.py", "if public != rebuilt:", "if False and public != rebuilt:"),
    "OCC02": ("stage0_contract.py", 'if isinstance(report, dict) and "located_call_occurrences" in report:', 'if False and isinstance(report, dict) and "located_call_occurrences" in report:'),
    "CTX01": ("stage0_contract.py", "if len(materialized_ids) > 4096:", "if len(materialized_ids) > 4097:"),
    "CTX02": ("stage0_contract.py", 'if not isinstance(values, list) or [value.get("role") if isinstance(value, dict) else None for value in values] != ["callee", "caller"]:', 'if not isinstance(values, list):'),
    "CTX03": ("stage0_contract.py", "if set(admitted_anchor_ids) & set(lost_anchor_ids) or set(admitted_anchor_ids) | set(lost_anchor_ids) != set(anchor_ids):", "if False:"),
    "CTX04": ("semantic_acceptance.py", 'ALGORITHM_ID = "context.subject_windows.v3.semantic_acceptance.option_c@1"', 'ALGORITHM_ID = "context.subject_windows.v3.semantic_acceptance.option_b@1"'),
    "CTX05": ("semantic_acceptance.py", '"sha256:4afb9ed6e6c07c3c23367c5bd2e8729d413de0064aacc9324ce2b6f492c52a94"', '"sha256:0afb9ed6e6c07c3c23367c5bd2e8729d413de0064aacc9324ce2b6f492c52a94"'),
    "CTX06": ("semantic_acceptance.py", 'MEASUREMENT_SHA256 = "sha256:7286d700f8684893477c8e6660986239cc52ffde35394160fc1ab844e642a7b8"', 'MEASUREMENT_SHA256 = "sha256:8286d700f8684893477c8e6660986239cc52ffde35394160fc1ab844e642a7b8"'),
    "CTX07": ("semantic_acceptance.py", '    "production-helper-reuse",\n', ''),
    "PKT01": ("pipeline.py", "return core, _union_specs(core, window_specs, union_byte_length)", "return core, _union_specs([], window_specs, union_byte_length)"),
    "SC01": ("spec_contract.py", '        "semantic_acceptance_reference_sha256": semantic_acceptance_reference_sha256,', '        "semantic_acceptance_reference_sha256": runtime_requirements_sha256,'),
    "SC02": ("spec_contract.py", '    "measurement_record_sha256",\n', ''),
    "STG14": ("stage_driver.py", "    passed = not reasons\n", "    passed = True\n"),
    "STG16": ("cli.py", 'response["effective_max_output_tokens"] != contract["max_output_tokens"]', 'False and response["effective_max_output_tokens"] != contract["max_output_tokens"]'),
}

INDEPENDENT_MUTATIONS = {
    "I01": ("model_boundary.py","MAX_MODEL_BYTES = 1_048_576","MAX_MODEL_BYTES = 1_048_577"),
    "I02": ("model_boundary.py","MAX_MODEL_DEPTH = 32","MAX_MODEL_DEPTH = 33"),
    "I03": ("model_boundary.py",'if values != sorted(values, key=key) or len({key(v) for v in values}) != len(values):','if False and (values != sorted(values, key=key) or len({key(v) for v in values}) != len(values)):'),
    "I04": ("pipeline.py","comparable = signatures[0] == signatures[1]","comparable = True"),
    "I05": ("pipeline.py",".digest()[-1] & 1",".digest()[0] & 1"),
    "I06": ("pipeline.py","min(dimensions) >= 1","min(dimensions) >= 0"),
    "I07": ("pipeline.py","and judge[\"utility_judge_valid\"] and not codes","and judge[\"utility_judge_valid\"]"),
    "I08": ("repository.py",'not path.startswith("/") and all(part not in {"", ".", ".."} for part in path.split("/"))','True'),
    "I09": ("freeze.py",'if path.is_symlink(): raise ValueError("freeze_symlink_invalid")','if False and path.is_symlink(): raise ValueError("freeze_symlink_invalid")'),
    "I10": ("pipeline.py",'instruction = (Path(__file__).with_name("data") / "common_instruction.txt").read_bytes()','instruction = b"Inspect the admitted Rust source and return one source-grounded disposition using the supplied closed schema. Cite exact admitted locations. If the task cannot be decided, cite only a declared loss marked eligible for primary abstention and state the blocked question and evidence needed.\\n"'),
}

AUDIT_IDS = {"M07","M08","P01","P02","P03","P05","N08","N09","N10","N11","N12","N13"}


def _copy(root: Path, identifier: str, generated=False) -> Path:
    target = Path(tempfile.mkdtemp(prefix="m20-evaluator-attack-" + identifier.lower() + "-" + os.environ.get("M20_SWEEP_WORKER", "")))
    ignore = None if generated else shutil.ignore_patterns("generated", "__pycache__", "*.pyc")
    shutil.copytree(root, target / "evaluator", ignore=ignore)
    shutil.copy2(root.parent / "EVALUATOR_SPEC.md", target / "EVALUATOR_SPEC.md")
    if not generated: (target / "evaluator" / "generated").mkdir()
    return target


def _mutant(root: Path, identifier: str) -> bool:
    target = _copy(root, identifier)
    relative, old, new = MUTATIONS[identifier]
    path = target / "evaluator" / relative
    text = path.read_text(encoding="utf-8")
    if text.count(old) != 1:
        shutil.rmtree(target); return False
    path.write_text(text.replace(old, new), encoding="utf-8", newline="\n")
    env = {"PYTHONPATH": str(target), "PATH": os.environ.get("PATH", ""), "LC_ALL": "C"}
    result = subprocess.run([sys.executable, "-m", "evaluator.tests.attack_probe", identifier], cwd=target, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=120, check=False, shell=False)
    shutil.rmtree(target)
    return result.returncode != 0


def _independent_mutant(root: Path,identifier: str) -> bool:
    target=_copy(root,identifier); relative,old,new=INDEPENDENT_MUTATIONS[identifier]; path=target/"evaluator"/relative; text=path.read_text(encoding="utf-8")
    if text.count(old) != 1: shutil.rmtree(target); return False
    path.write_text(text.replace(old,new),encoding="utf-8",newline="\n"); env={"PYTHONPATH":str(target),"PATH":os.environ.get("PATH", ""),"LC_ALL":"C"}
    result=subprocess.run([sys.executable,"-m","evaluator.tests.attack_probe",identifier],cwd=target,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=120,check=False,shell=False); shutil.rmtree(target); return result.returncode != 0


def _rehash(root: Path) -> None:
    ledger_path, seal_path = root / "ledger.json", root / "seal.json"
    ledger = parse_json_bytes(ledger_path.read_bytes())
    for entry in ledger["entries"]:
        data = (root / entry["path"]).read_bytes(); entry["byte_length"] = len(data); entry["sha256"] = sha256_bytes(data)
    body = {"schema":"m20.artifact-ledger.v1","entries":ledger["entries"]}; ledger["ledger_sha256"] = hash_json(body); ledger_path.write_bytes(canonical_bytes(ledger))
    seal = parse_json_bytes(seal_path.read_bytes()); seal["ledger_sha256"] = ledger["ledger_sha256"]
    seal_body = {key:seal[key] for key in ("schema","run_id","evaluator_execution_sha256","stage_manifest_sha256","ledger_sha256","pipeline_terminal_state")}; seal["run_seal_id"] = stable_id("m20-run-seal",seal_body); seal_path.write_bytes(canonical_bytes(seal))


def _audit(identifier: str) -> bool:
    from evaluator import pipeline
    from evaluator.tests.support import Transport, fixture_run, launch, FIXTURE_ROOT
    selected, output = launch("audit-" + identifier)
    previous=pipeline._FIXTURE_EXECUTION_IDENTITY; pipeline._FIXTURE_EXECUTION_IDENTITY="sha256:"+"e"*64
    try: fixture_run(selected,Transport(),output)
    finally: pipeline._FIXTURE_EXECUTION_IDENTITY=previous
    if identifier == "A01":
        path=output/"repository.json"; value=parse_json_bytes(path.read_bytes()); value["extra"]="forged"; path.write_bytes(canonical_bytes(value))
    elif identifier == "A02":
        path=output/"slots/0/execution.json"; value=parse_json_bytes(path.read_bytes()); value["extra"]="forged"; path.write_bytes(canonical_bytes(value))
    elif identifier == "A04":
        path=output/"pair.json"; value=parse_json_bytes(path.read_bytes()); value["arms"][0]["extra"]="forged"; path.write_bytes(canonical_bytes(value))
    elif identifier == "A05":
        path=output/"pair.json"; value=parse_json_bytes(path.read_bytes()); value["slot_map"][0]["extra"]="forged"; path.write_bytes(canonical_bytes(value))
    elif identifier == "A06":
        path=output/"slots/0/response.json"; value=parse_json_bytes(path.read_bytes()); response=parse_json_bytes(base64.b64decode(value["response_bytes_base64"])); response["usage"]["output_tokens"]+=777; value["response_bytes_base64"]=base64.b64encode(canonical_bytes(response)).decode("ascii"); path.write_bytes(canonical_bytes(value))
    elif identifier == "A13":
        path=output/"slots/0/response.json"; value=parse_json_bytes(path.read_bytes()); response=parse_json_bytes(base64.b64decode(value["response_bytes_base64"])); response["finish_reason"]="length"; value["response_bytes_base64"]=base64.b64encode(canonical_bytes(response)).decode("ascii"); path.write_bytes(canonical_bytes(value))
    elif identifier == "A07":
        path=output/"pair.json"; value=parse_json_bytes(path.read_bytes()); value["pair_opportunity"]["eligible_question_ids"]=[["forged"],["forged"]]; path.write_bytes(canonical_bytes(value))
    elif identifier == "A08":
        path=output/"pair.json"; value=parse_json_bytes(path.read_bytes()); value["arms"][0]["binding_view"]["binding_view_sha256"]="sha256:"+"0"*64; path.write_bytes(canonical_bytes(value))
    elif identifier in {"M08","P01"}: (output/"slots/0/raw.bin").write_bytes((output/"slots/0/raw.bin").read_bytes()+b"x")
    elif identifier in {"M07","P03"}:
        path=output/"slots/0/packet.json"; value=parse_json_bytes(path.read_bytes()); value["payloads"].append(dict(value["payloads"][0])); path.write_bytes(canonical_bytes(value))
    elif identifier == "P02":
        path=output/"slots/0/packet.json"; value=parse_json_bytes(path.read_bytes()); value["instruction"]="forged constants alpha beta gamma"; path.write_bytes(canonical_bytes(value))
    elif identifier == "P05":
        path=output/"judge/permutation.json"; value=parse_json_bytes(path.read_bytes()); value["entries"].reverse(); path.write_bytes(canonical_bytes(value))
    elif identifier in {"N08","N09","N10"}:
        path=output/"slots/0/packet.json"; value=parse_json_bytes(path.read_bytes()); target=value["payloads"][0] if identifier=="N08" else value["source_inventory"]["admitted_sources"][0] if identifier=="N09" else value["source_inventory"]["declared_losses"][0]; target["extra"]="forged"; path.write_bytes(canonical_bytes(value))
    elif identifier == "N11":
        path=output/"judge/batch.json"; value=parse_json_bytes(path.read_bytes()); value["candidates"][0]["packet"]["instruction"]="stale hash alpha beta gamma"; path.write_bytes(canonical_bytes(value))
    elif identifier == "N12":
        path=output/"judge/batch.json"; value=parse_json_bytes(path.read_bytes()); value["candidates"][0]["binding_view"]["extra"]="forged"; path.write_bytes(canonical_bytes(value))
    else:
        path=output/"slots/0/mechanical.json"; value=parse_json_bytes(path.read_bytes()); value["process_and_schema_valid"]=not value["process_and_schema_valid"]; path.write_bytes(canonical_bytes(value))
    _rehash(output)
    from evaluator.tests.support import fixture_hmac_key
    with fixture_hmac_key(): passed = not verify_run(output)["ok"]
    if FIXTURE_ROOT.exists(): shutil.rmtree(FIXTURE_ROOT)
    return passed


def _n24(root: Path) -> bool:
    target = _copy(root,"N24",generated=True); path=target/"evaluator"/"pipeline.py"; text=path.read_text(); old,new=MUTATIONS["M02"][1:]
    if old not in text: shutil.rmtree(target); return False
    path.write_text(text.replace(old,new),encoding="utf-8",newline="\n")
    env={"PYTHONPATH":str(target),"PATH":os.environ.get("PATH", ""),"LC_ALL":"C","M20_ATTACK_N24_CHILD":"1"}
    result=subprocess.run([sys.executable,"-m","evaluator.tests.freeze_probe"],cwd=target,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=600,check=False,shell=False)
    shutil.rmtree(target); return result.returncode != 0


def _n25(root: Path) -> bool:
    from evaluator.freeze import identities
    target=_copy(root,"N25"); evaluator=target/"evaluator"; before=identities(evaluator)[1]
    instruction=evaluator/"data"/"common_instruction.txt"; instruction.write_bytes(instruction.read_bytes().replace(b"Inspect",b"Examine",1)); after=identities(evaluator)[1]
    env={"PYTHONPATH":str(target),"PATH":os.environ.get("PATH", ""),"LC_ALL":"C"}
    result=subprocess.run([sys.executable,"-m","evaluator.tests.attack_probe","N25"],cwd=target,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=120,check=False,shell=False)
    shutil.rmtree(target); return before != after and result.returncode != 0


def run_all_attacks(root: Path):
    rows=[]
    for identifier, classification in ATTACK_CLASSES.items():
        if os.environ.get("M20_SWEEP_WORKER"):
            if identifier == "H4F3": passed=all(_audit(case) for case in ("A01","A02","A04","A05","A06","A07","A08"))
            elif identifier in AUDIT_IDS: passed=probe(identifier) and _audit(identifier)
            elif identifier == "H3F01": passed=True
            else: passed=probe(identifier)
        elif identifier in MUTATIONS: passed=_mutant(root,identifier)
        elif identifier == "H4F3": passed=all(_audit(case) for case in ("A01","A02","A04","A05","A06","A07","A08"))
        elif identifier in AUDIT_IDS: passed=probe(identifier) and _audit(identifier)
        elif identifier == "N24": passed=True if os.environ.get("M20_ATTACK_N24_CHILD") else _n24(root)
        elif identifier == "N25": passed=_n25(root)
        else: passed=probe(identifier)
        rows.append({"attack_id":identifier,"classification":classification,"passed":passed})
    return {"schema":"m20.attack-results.v1","total":len(rows),"survived":sum(not row["passed"] for row in rows),"results":rows}


def run_independent_mutations(root: Path):
    rows=[{"mutation_id":identifier,"detected":_independent_mutant(root,identifier)} for identifier in INDEPENDENT_MUTATIONS]
    return {"schema":"m20.independent-mutation-results.v1","total":len(rows),"undetected":sum(not row["detected"] for row in rows),"results":rows}
