"""Fixed-universe AST mutation execution with baseline and calibration controls."""
import ast
import copy
import math
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
import unicodedata
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from evaluator.canonical import canonical_bytes, hash_json, sha256_bytes, stable_id
from evaluator.pipeline import PipelineError
from .mutation_triage import classify_mutant
from .score_surface_oracle import score_atoms_bytes


MODULES = {
    "canonical.py": ("scoring-relevant", "canonical bytes, hashes, IDs, and hostile JSON decoding bind every score"),
    "source_payload.py": ("scoring-relevant", "source extraction and payload closure determine admitted evidence"),
    "textnorm.py": ("scoring-relevant", "normalization and substantive-text checks determine mechanical validity"),
    "repository.py": ("scoring-relevant", "authenticated Git objects and baseline construction determine packets"),
    "model_boundary.py": ("scoring-relevant", "closed reviewer and judge decoding determines arm eligibility and utility"),
    "pipeline.py": ("scoring-relevant", "pair construction, policy, judging, and primary scoring are measurement semantics"),
    "stage0_contract.py": ("scoring-relevant", "occurrence summaries and context-v3 commitments determine admitted treatment inputs"),
    "artifacts.py": ("non-scoring", "one-way persistence and offline audit do not construct recorded scores"),
    "freeze.py": ("non-scoring", "bundle identity and release gating do not construct a unit score"),
    "cli.py": ("non-scoring", "fixed command routing and display do not construct a unit score"),
}
OPERATORS = (
    "comparison_replacement", "integer_minus_one", "integer_plus_one", "integer_zero",
    "and_or_swap", "condition_negation", "condition_clause_deletion", "boolean_flip",
    "if_condition_false", "if_condition_true", "raise_deletion", "membership_inversion",
    "membership_set_relaxation",
)
COMPARES = (ast.Lt, ast.LtE, ast.Gt, ast.GtE, ast.Eq, ast.NotEq)
ORACLE_NAMES = ("vectors", "unit", "attacks", "fixtures")
ORACLE_CHILD_MARKER = ".m20-mutation-oracle-child.v1"
CALIBRATIONS = (
    ("E01", "canonical.py", (("decoded = base64.b64decode", "decoded_bytes = base64.b64decode"), ("base64.b64encode(decoded)", "base64.b64encode(decoded_bytes)"), ("return decoded", "return decoded_bytes"))),
    ("E02", "source_payload.py", (("text = excerpt.decode(\"utf-8\", \"strict\")\n    digest = sha256_bytes(excerpt)", "digest = sha256_bytes(excerpt)\n    text = excerpt.decode(\"utf-8\", \"strict\")"),)),
    ("E03", "textnorm.py", (("distinct = sorted(set(tokens))", "unique_tokens = sorted(set(tokens))"), ("\"distinct_tokens\": distinct", "\"distinct_tokens\": unique_tokens"), ("len(distinct) >= 3", "len(unique_tokens) >= 3"))),
    ("E04", "canonical.py", (("out = {}\n        for key, value in pairs:", "decoded_object = {}\n        for key, value in pairs:"), ("if key in out:", "if key in decoded_object:"), ("out[key] = value", "decoded_object[key] = value"), ("return out", "return decoded_object"))),
    ("E05", "model_boundary.py", (("raw_hash = sha256_bytes(raw_bytes)", "observed_raw_hash = sha256_bytes(raw_bytes)"), ("DecodedModel(typed, raw_hash,", "DecodedModel(typed, observed_raw_hash,"))),
    ("E06", "canonical.py", (("def hash_json(value: Any) -> str:\n    return sha256_bytes", "def hash_json(value: Any) -> str:\n    if False:\n        raise AssertionError(\"unreachable calibration\")\n    return sha256_bytes"),)),
)
LIMITATIONS = (
    {"id":"L01","status":"addressed","statement":"baseline control uses the identical isolated oracle environment"},
    {"id":"L02","status":"addressed","statement":"resume is unsupported; every accepted sweep is one provenance-bound clean run"},
    {"id":"L03","status":"addressed","statement":"timeouts are isolated-retried and separated from assertion kills"},
    {"id":"L04","status":"addressed","statement":"mutation selection or hash failures abort without entering the score"},
    {"id":"L05","status":"remaining","statement":"the score covers the declared 13 operators, not every possible AST transformation"},
    {"id":"L06","status":"remaining","statement":"the score covers seven scoring modules; integrity/release/CLI/data surfaces use separate gates"},
    {"id":"L07","status":"remaining","statement":"full-run vector cases still share deterministic transport semantics; asymmetric byte overflow is now direct"},
    {"id":"L08","status":"remaining","statement":"the score measures test sensitivity only, not specification or research validity or token/information/cost equality"},
)


def _replace(raw: bytes, node: ast.AST, replacement: str) -> bytes:
    lines = raw.splitlines(keepends=True)
    start = sum(len(line) for line in lines[:node.lineno - 1]) + node.col_offset
    end = sum(len(line) for line in lines[:node.end_lineno - 1]) + node.end_col_offset
    return raw[:start] + replacement.encode("utf-8") + raw[end:]


def _location(node: ast.AST) -> dict:
    return {"line":node.lineno, "column":node.col_offset, "end_line":node.end_lineno, "end_column":node.end_col_offset}


def _variants(node: ast.AST):
    if isinstance(node, ast.Compare):
        for index, operator in enumerate(node.ops):
            if type(operator) in COMPARES:
                for replacement in COMPARES:
                    if replacement is type(operator):
                        continue
                    changed = copy.deepcopy(node); changed.ops[index] = replacement()
                    yield "comparison_replacement", ast.unparse(changed)
            elif isinstance(operator, (ast.In, ast.NotIn)):
                changed = copy.deepcopy(node); changed.ops[index] = ast.NotIn() if isinstance(operator, ast.In) else ast.In()
                yield "membership_inversion", ast.unparse(changed)
            if isinstance(operator, ast.In) and isinstance(node.comparators[index], (ast.Set, ast.List, ast.Tuple)):
                changed = copy.deepcopy(node); changed.comparators[index].elts.append(ast.Constant("__m20_mutant_enum_extension__"))
                yield "membership_set_relaxation", ast.unparse(changed)
    if isinstance(node, ast.Constant) and isinstance(node.value, int) and not isinstance(node.value, bool):
        for operator, value in (("integer_minus_one", node.value - 1), ("integer_plus_one", node.value + 1), ("integer_zero", 0)):
            if value != node.value:
                yield operator, repr(value)
    if isinstance(node, ast.BoolOp):
        changed = copy.deepcopy(node); changed.op = ast.Or() if isinstance(node.op, ast.And) else ast.And()
        yield "and_or_swap", ast.unparse(changed)
        for index in range(len(node.values)):
            remaining = [copy.deepcopy(value) for offset, value in enumerate(node.values) if offset != index]
            if remaining:
                replacement = remaining[0] if len(remaining) == 1 else ast.BoolOp(op=copy.deepcopy(node.op), values=remaining)
                yield "condition_clause_deletion", ast.unparse(replacement)
    if isinstance(node, ast.Constant) and isinstance(node.value, bool):
        yield "boolean_flip", repr(not node.value)
    if isinstance(node, (ast.If, ast.While, ast.IfExp)):
        yield "condition_negation", "not (" + ast.unparse(node.test) + ")"
    if isinstance(node, ast.If):
        yield "if_condition_true", "True"
        yield "if_condition_false", "False"
    if isinstance(node, ast.Raise):
        yield "raise_deletion", "pass"


def generate_mutants(root: Path) -> list[dict]:
    rows = []
    for relative, (category, _) in MODULES.items():
        if category != "scoring-relevant":
            continue
        raw = (root / relative).read_bytes(); tree = ast.parse(raw, filename=relative); seen = set(); baseline_score_atoms = score_atoms_bytes(raw, relative)
        for node in ast.walk(tree):
            target = node.test if isinstance(node, (ast.If, ast.While, ast.IfExp)) else node
            for operator, replacement in _variants(node):
                mutated = _replace(raw, target, replacement); digest = sha256_bytes(mutated)
                if digest in seen:
                    continue
                try:
                    compile(mutated, relative, "exec")
                except SyntaxError:
                    continue
                seen.add(digest); identity = {"module":relative, "operator":operator, "location":_location(target), "replacement_sha256":sha256_bytes(replacement.encode())}
                rows.append({"mutant_id":stable_id("m20-ast-mutant", identity), **identity, "source_sha256":sha256_bytes(raw), "mutated_source_sha256":digest, "replacement":replacement, "score_surface_changed":score_atoms_bytes(mutated, relative) != baseline_score_atoms})
    return sorted(rows, key=lambda row: row["mutant_id"].encode())


def _commands() -> dict[str, list[str]]:
    return {
        "vectors":[sys.executable,"-m","evaluator","verify-reference-vectors"],
        "unit":[sys.executable,"-m","unittest","discover","-f","-s","evaluator/tests","-t",".","-p","test_*.py"],
        "attacks":[sys.executable,"-m","evaluator","run-attacks"],
        "fixtures":[sys.executable,"-m","evaluator","generate-fixtures","--check"],
    }


def _oracle_run(work: Path, worker: str, timeouts: dict[str,int], source_workspace: Path, stop_on_detection: bool = False) -> dict:
    (work/ORACLE_CHILD_MARKER).write_bytes(canonical_bytes({"schema":"m20.mutation-oracle-child.v1"}))
    exits, elapsed = {}, {}
    for name, command in _commands().items():
        # Physical fixture state is oracle-local.  Its logical identity, and
        # therefore every canonical byte, is deliberately independent of it.
        env = {"PYTHONPATH":str(work), "PATH":os.environ.get("PATH", ""), "LC_ALL":"C", "PYTHONDONTWRITEBYTECODE":"1", "M20_SWEEP_WORKER":worker+"-"+work.name+"-"+name, "M20_TEST_SOURCE_WORKSPACE":str(source_workspace)}
        started = time.monotonic_ns()
        try:
            result = subprocess.run(command, cwd=work, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=timeouts[name], check=False, shell=False)
            exits[name] = result.returncode
        except subprocess.TimeoutExpired:
            exits[name] = 124
        elapsed[name] = (time.monotonic_ns() - started) // 1_000_000
        if stop_on_detection and exits[name] != 0:
            break
    return {"exit_codes":exits, "elapsed_ms":elapsed}


def _isolated_copy(root: Path) -> tuple[Path,Path]:
    work = Path(tempfile.mkdtemp(prefix="m20-mutation-sweep-", dir="/tmp")); evaluator = work / "evaluator"
    shutil.copytree(root, evaluator, ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
    shutil.copy2(root.parent / "EVALUATOR_SPEC.md", work / "EVALUATOR_SPEC.md")
    shutil.copy2(root.parent / "preregistration.json", work / "preregistration.json")
    return work, evaluator


def _apply_mutant(evaluator: Path, mutant: dict) -> None:
    path = evaluator / mutant["module"]; raw = path.read_bytes(); tree = ast.parse(raw, filename=mutant["module"]); selected = None
    for node in ast.walk(tree):
        target = node.test if isinstance(node, (ast.If, ast.While, ast.IfExp)) else node
        if not hasattr(target, "lineno") or _location(target) != mutant["location"]:
            continue
        for operator, replacement in _variants(node):
            if operator == mutant["operator"] and replacement == mutant["replacement"]:
                selected = _replace(raw, target, replacement); break
        if selected is not None:
            break
    if selected is None:
        raise PipelineError("mutation_sweep_selection_failed",3)
    if sha256_bytes(selected) != mutant["mutated_source_sha256"]:
        raise PipelineError("mutation_sweep_hash_mismatch",3)
    path.write_bytes(selected)


def _apply_calibration(evaluator: Path, calibration) -> None:
    identifier, relative, replacements = calibration; path = evaluator / relative; text = path.read_text(encoding="utf-8")
    for old, new in replacements:
        if text.count(old) != 1:
            raise PipelineError("mutation_sweep_calibration_apply_failed",3)
        text = text.replace(old, new)
    path.write_text(text, encoding="utf-8", newline="\n")


def _run_copy(root: Path, worker: str, timeouts: dict[str,int], mutant: dict | None = None, calibration=None) -> dict:
    work, evaluator = _isolated_copy(root)
    try:
        if mutant is not None:
            _apply_mutant(evaluator, mutant)
        if calibration is not None:
            _apply_calibration(evaluator, calibration)
        return _oracle_run(work, worker, timeouts, root.parents[2], stop_on_detection=mutant is not None)
    finally:
        shutil.rmtree(work, ignore_errors=True)


def _provenance(root: Path, jobs: int, baseline_timeouts: dict[str,int]) -> dict:
    included = []
    for path in sorted(root.rglob("*")):
        if not path.is_file() or "__pycache__" in path.parts or path.suffix == ".pyc":
            continue
        relative = path.relative_to(root).as_posix()
        included.append({"path":relative,"sha256":sha256_bytes(path.read_bytes()),"byte_length":path.stat().st_size})
    page_size = os.sysconf("SC_PAGE_SIZE") if "SC_PAGE_SIZE" in os.sysconf_names else None
    pages = os.sysconf("SC_PHYS_PAGES") if "SC_PHYS_PAGES" in os.sysconf_names else None
    runtime = {"implementation":sys.implementation.name,"python_version":list(sys.version_info[:3]),"unicodedata_version":unicodedata.unidata_version,"platform":platform.platform(),"cpu_count":os.cpu_count(),"physical_memory_bytes":page_size*pages if page_size is not None and pages is not None else None}
    body = {"schema":"m20.mutation-sweep-provenance.v1","runner_version":"m20-ast-sweep.v2","oracle_files":included,"oracle_bundle_sha256":hash_json(included),"normative_spec_sha256":sha256_bytes((root.parent/"EVALUATOR_SPEC.md").read_bytes()),"jobs":jobs,"baseline_timeout_seconds":baseline_timeouts,"runtime":runtime}
    return {**body,"run_identity":stable_id("m20-mutation-sweep-run",body)}


def _classify(initial: dict, retry: dict | None) -> tuple[str,str | None,dict]:
    first_timeout = any(code == 124 for code in initial["exit_codes"].values())
    if not first_timeout:
        return ("killed_assertion" if any(initial["exit_codes"].values()) else "survived", None, initial)
    assert retry is not None
    if any(code == 124 for code in retry["exit_codes"].values()):
        return "killed_timeout_reproduced", "reproduced_isolated", retry
    if any(retry["exit_codes"].values()):
        return "killed_assertion", "resource_timeout_then_assertion", retry
    return "survived", "resource_timeout_flaky", retry


def sweep(root: Path, jobs: int = 1) -> dict:
    generous = {name:300 for name in ORACLE_NAMES}
    baseline = _run_copy(root,"-baseline-control",generous)
    if any(baseline["exit_codes"].values()):
        raise PipelineError("mutation_sweep_baseline_failed",3)
    normal_timeouts = {name:max(30,math.ceil(baseline["elapsed_ms"][name]/1000)*4) for name in ORACLE_NAMES}
    retry_timeouts = {name:max(90,math.ceil(baseline["elapsed_ms"][name]/1000)*8) for name in ORACLE_NAMES}
    provenance = _provenance(root,jobs,generous)
    calibration_rows=[]
    for calibration in CALIBRATIONS:
        observed=_run_copy(root,"-calibration-"+calibration[0].lower(),normal_timeouts,calibration=calibration)
        survived=not any(observed["exit_codes"].values())
        calibration_rows.append({"calibration_id":calibration[0],"description":"meaning-preserving source transformation","survived":survived,"oracle":observed})
    if not all(row["survived"] for row in calibration_rows):
        raise PipelineError("mutation_sweep_equivalent_calibration_failed",3)
    mutants = generate_mutants(root)
    with ThreadPoolExecutor(max_workers=jobs) as executor:
        initial = list(executor.map(lambda mutant:_run_copy(root,"-"+mutant["mutant_id"].split(":")[-1][:16],normal_timeouts,mutant=mutant),mutants))
    rows=[]
    for mutant, first in zip(mutants,initial):
        retry=None
        if any(code==124 for code in first["exit_codes"].values()):
            retry=_run_copy(root,"-retry-"+mutant["mutant_id"].split(":")[-1][:16],retry_timeouts,mutant=mutant)
        outcome,timeout_class,decisive=_classify(first,retry)
        classification,reason=classify_mutant(mutant);triage={"classification":classification,"reason":reason}
        rows.append({**mutant,"outcome":outcome,"timeout_class":timeout_class,"oracle":{"initial":first,"isolated_retry":retry,"decisive_exit_codes":decisive["exit_codes"]},"triage":triage})
    killed_assertion=sum(row["outcome"]=="killed_assertion" for row in rows);killed_timeout=sum(row["outcome"]=="killed_timeout_reproduced" for row in rows);survived=sum(row["outcome"]=="survived" for row in rows)
    classes=("SCORE_AFFECTING","NON_SCORE","EQUIVALENT","UNDETERMINED");triage_counts={name:sum(row["outcome"]=="survived" and row["triage"]["classification"]==name for row in rows) for name in classes};universe_counts={name:sum(row["triage"]["classification"]==name for row in rows) for name in classes};eligible={"SCORE_AFFECTING","UNDETERMINED"};denominator=sum(row["triage"]["classification"] in eligible for row in rows);numerator=sum(row["triage"]["classification"] in eligible and row["outcome"] in {"killed_assertion","killed_timeout_reproduced"} for row in rows)
    modules=[{"path":path,"category":category,"rationale":rationale,"source_sha256":sha256_bytes((root/path).read_bytes())} for path,(category,rationale) in MODULES.items()]
    result={"schema":"m20.ast-mutation-sweep.v3","claim_scope":"score-impact test sensitivity for the declared six modules and thirteen operators; NON_SCORE and EQUIVALENT are excluded","provenance":provenance,"baseline_control":baseline,"normal_timeout_seconds":normal_timeouts,"isolated_retry_timeout_seconds":retry_timeouts,"equivalent_calibrations":calibration_rows,"operators":list(OPERATORS),"modules":modules,"limitations":list(LIMITATIONS),"mutants":rows,"summary":{"total":len(rows),"killed":killed_assertion+killed_timeout,"killed_by_assertion":killed_assertion,"killed_by_reproduced_timeout":killed_timeout,"initial_timeout":sum(any(code==124 for code in row["oracle"]["initial"]["exit_codes"].values()) for row in rows),"resource_timeout_flaky":sum(row["timeout_class"]=="resource_timeout_flaky" for row in rows),"survived":survived,"triage_survived":triage_counts,"triage_universe":universe_counts,"score_affecting_survived":triage_counts["SCORE_AFFECTING"],"undetermined_survived":triage_counts["UNDETERMINED"],"mutation_score":{"numerator":numerator,"denominator":denominator,"basis_points":((numerator)*10000//denominator if denominator else 10000)}}}
    return result


def write_sweep(root: Path, output: Path, jobs: int = 1) -> dict:
    if output.exists() or output.is_symlink():
        raise PipelineError("output_exists",2)
    result=sweep(root,jobs)
    with output.open("xb") as handle:
        handle.write(canonical_bytes(result))
    return result
