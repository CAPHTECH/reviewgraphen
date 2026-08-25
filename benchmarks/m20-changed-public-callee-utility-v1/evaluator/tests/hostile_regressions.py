"""Independent hostile inputs reported by the H4 quality review."""
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from evaluator.canonical import canonical_bytes, parse_json_bytes
from evaluator.freeze import file_records
from evaluator.pipeline import PipelineError, _obligation, _stage
from evaluator.repository import GitRepository, PreflightError
from .support import FIXTURE_ROOT, fixture_file, launch


def _oid(kind,content):return hashlib.sha1(kind.encode()+b" "+str(len(content)).encode()+b"\0"+content).hexdigest()
def _frame(object_id,kind,content,size=None):return object_id.encode()+b" "+kind.encode()+b" "+(str(len(content)) if size is None else size).encode()+b"\n"+content+b"\n"
def _repo(response):
    repository=object.__new__(GitRepository);repository.object_format="sha1";repository.oid_bytes=20;repository._cache={};repository.root=Path("/");repository.env={}
    return repository,patch("evaluator.repository.subprocess.run",return_value=SimpleNamespace(returncode=0,stderr=b"",stdout=response))
def _typed(code,operation,error_type):
    try:operation();return False
    except error_type as error:return getattr(error,"code",str(error))==code


def f4_results():
    results={}; content=b"x"; object_id=_oid("blob",content)
    for name,size in (("batch_size_leading_zero","01"),("batch_size_plus","+1")):
        repo,mock=_repo(_frame(object_id,"blob",content,size))
        with mock:results[name]=_typed("object_framing_invalid",lambda:repo.object(object_id),PreflightError)
    repo,mock=_repo(b"\xff"*40+b" blob 1\nx\n")
    with mock:results["batch_non_ascii_oid"]=_typed("object_framing_invalid",lambda:repo.object(object_id),PreflightError)
    tree="0"*40; commit=(f"tree {tree}\ntree {tree}\n\nmessage\n").encode(); commit_id=_oid("commit",commit); repo,mock=_repo(_frame(commit_id,"commit",commit))
    with mock:results["duplicate_commit_tree"]=_typed("commit_invalid",lambda:repo.commit(commit_id),PreflightError)
    with tempfile.TemporaryDirectory() as value:
        root=Path(value); (root/"qualified.py").write_text('import builtins\nbuiltins.eval("1")\n',encoding="utf-8",newline="\n")
        results["qualified_eval"]=_typed("freeze_dynamic_code_invalid",lambda:file_records(root),ValueError)
    with tempfile.TemporaryDirectory() as value:
        root=Path(value); (root/"dynamic.py").write_text('import importlib\nimportlib.import_module("os")\n',encoding="utf-8",newline="\n")
        results["importlib_dynamic"]=_typed("freeze_dynamic_code_invalid",lambda:file_records(root),ValueError)
    selected,_=launch("hostile-inputs")
    try:
        stage=parse_json_bytes(fixture_file(selected["stage_manifest_path"]).read_bytes()); bad=dict(stage); bad["selection_manifest_sha256"]="sha256:"+"0"*64; results["selection_binding_tamper"]=_typed("stage_contract_invalid",lambda:_stage(bad,selected),PipelineError)
        obligation=parse_json_bytes(fixture_file(selected["frozen_obligation_path"]).read_bytes()); obligation["obligation_ids"]=[1]; results["integer_obligation_id"]=_typed("duplicate_required_id",lambda:_obligation(obligation,selected["unit_id"]),PipelineError)
    finally:
        if FIXTURE_ROOT.exists():shutil.rmtree(FIXTURE_ROOT)
    env={"PYTHONPATH":str(Path(__file__).parents[2]),"PATH":os.environ.get("PATH",""),"LC_ALL":"C"}; process=subprocess.run([sys.executable,"-m","evaluator","run"],cwd=Path(__file__).parents[2],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=False,timeout=30,shell=False)
    try:record=parse_json_bytes(process.stdout);results["cli_missing_argv"]=process.returncode==2 and not process.stderr and record=={"schema":"m20.cli-error.v1","code":"cli_arguments_invalid"}
    except ValueError:results["cli_missing_argv"]=False
    return results
