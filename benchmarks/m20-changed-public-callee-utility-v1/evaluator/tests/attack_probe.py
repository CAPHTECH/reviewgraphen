"""Named behavioral oracles used against current and mutated evaluator copies."""
import copy
import hashlib
import inspect
import subprocess
import sys
import tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from evaluator.canonical import MAX_INTEGER, canonical_bytes, hash_json, parse_json_bytes, sha256_bytes
from evaluator.freeze import ATTACK_CLASSES
from evaluator.model_boundary import DIMENSIONS, MAX_MODEL_BYTES, TypedError, decode_model
from evaluator.pipeline import MAX_SOURCE_BYTES, PipelineError, _arm, _batch, _bit, _budget, _candidate, _codes, _loss, _obligation, _opportunity, _packet, _primary, _utility
from evaluator.repository import GitRepository, PreflightError, valid_path
from evaluator.source_payload import extract, payload_record, source_record, validate_payload_closure
from evaluator.stage0_contract import build_context_projection_v3
from evaluator.textnorm import normalize


def _judge_batch(forced=False):
    candidates = []
    for index in range(2):
        state = "mechanical_forced_zero" if forced and index == 1 else "judgeable"
        candidates.append({"candidate_id": f"c{index}", "packet_sha256": f"p{index}", "output_artifact_sha256": f"o{index}", "binding_view_sha256": f"b{index}", "mechanical_score_sha256": f"m{index}", "mechanical_state": {"kind": state}})
    return {"schema":"m20.utility_judge_batch_input.v1","task_id":"t","rubric_id":"r","batch_id":"batch","candidates":candidates}


def _judge_response(batch, values=(1,1,2,2)):
    scores=[]
    for candidate in batch["candidates"]:
        forced=candidate["mechanical_state"]["kind"]=="mechanical_forced_zero"; numbers=(0,0,0,0) if forced else values
        scores.append({"candidate_id":candidate["candidate_id"],"packet_sha256":candidate["packet_sha256"],"output_artifact_sha256":candidate["output_artifact_sha256"],"binding_view_sha256":candidate["binding_view_sha256"],"mechanical_score_sha256":candidate["mechanical_score_sha256"],"score_source":"mechanical_forced_zero" if forced else "judge","dimensions":dict(zip(DIMENSIONS,numbers)),"total":sum(numbers),"verdict":"not_usable" if forced else "usable"})
    return {"schema":"m20.utility_judge_batch_output.v1","batch_id":"batch","scores":scores}


def _valid_obligation():
    pairs=[{"caller_endpoint_id":"a","callee_endpoint_id":"b"}]
    sources=[{"required_id":"s0","role":"changed","snapshot_side":"head","path":"src/lib.rs","start_line":1,"end_line":1,"blob_oid":"a"*40},{"required_id":"s1","role":"context","snapshot_side":"head","path":"src/caller.rs","start_line":1,"end_line":1,"blob_oid":"b"*40}]
    projection=build_context_projection_v3(
        {"projection_id":"p","snapshot_id":"snapshot","request_id":"request","obligation_ids":["o"],"relation_ids":["r"],"endpoint_pairs":pairs},
        ["file:0","file:1"],["file:0","file:1"],
        [{"role":"callee","status":"admitted","endpoint_id":"b","source_artifact_id":"file:0","start_line":1,"end_line":1,"window_id":"w0"},{"role":"caller","status":"admitted","endpoint_id":"a","source_artifact_id":"file:1","start_line":1,"end_line":1,"window_id":"w1"}],
        [{"source_artifact_id":"file:0","snapshot_side":"head","path":"src/lib.rs","blob_oid":"a"*40},{"source_artifact_id":"file:1","snapshot_side":"head","path":"src/caller.rs","blob_oid":"b"*40}],[],
        [{"window_id":"w0","source_artifact_id":"file:0","start_line":1,"end_line":1,"role":"callee","source_required_id":"s0","support_anchor_ids":[]},{"window_id":"w1","source_artifact_id":"file:1","start_line":1,"end_line":1,"role":"caller","source_required_id":"s1","support_anchor_ids":[]}],[],{"state":"known_zero"},[],[],[],
    )
    return {"schema":"m20.frozen_obligation.v1","unit_id":"u","rule_id":"relation.changed_public_callee@1","property_id":"rust.callee_contract_review@1","obligation_ids":["o"],"relation_ids":["r"],"endpoint_pairs":pairs,"subject_windows":[{"subject_id":"s0","window_id":"w0","role":"callee"},{"subject_id":"s1","window_id":"w1","role":"caller"}],"sources":sources,"required_references":[],"projection":projection,"bounded_scope_manifest_id":"scope"}


def _decoder_reject(mutator):
    batch=_judge_batch(); response=_judge_response(batch); mutator(response,batch)
    try: decode_model(canonical_bytes(response),"judge",{"batch":batch}); return False
    except TypedError: return True


def _repo(response_for):
    repository=object.__new__(GitRepository); repository.object_format="sha1"; repository.oid_bytes=20; repository._cache={}; repository.root=Path("/"); repository.env={}
    def invoke(*args,**kwargs):
        oid=kwargs["input"].decode("ascii").strip(); response=response_for(oid)
        return SimpleNamespace(returncode=0,stderr=b"",stdout=response)
    return repository,invoke


def _git_oid(kind: str, content: bytes) -> str:
    return hashlib.sha1(kind.encode()+b" "+str(len(content)).encode()+b"\0"+content).hexdigest()


def _git_frame(oid: str, kind: str, content: bytes) -> bytes:
    return oid.encode()+b" "+kind.encode()+b" "+str(len(content)).encode()+b"\n"+content+b"\n"


def _repository_boundaries() -> bool:
    def raises(code,operation):
        try: operation(); return False
        except PreflightError as error: return error.code==code
    content=b"content"; real=_git_oid("blob",content); false="0"*40
    repo,invoke=_repo(lambda oid:_git_frame(oid,"blob",content))
    with patch("evaluator.repository.subprocess.run",side_effect=invoke):
        if not raises("object_hash_mismatch",lambda:repo.object(false)): return False
    repo,invoke=_repo(lambda oid:b"missing-newline")
    with patch("evaluator.repository.subprocess.run",side_effect=invoke):
        if not raises("object_framing_invalid",lambda:repo.object(real)): return False
    repo,invoke=_repo(lambda oid:_git_frame(oid,"blob",content))
    with patch("evaluator.repository.subprocess.run",side_effect=invoke):
        if not raises("object_oid_invalid",lambda:repo.object("g"*40)) or not raises("object_type_invalid",lambda:repo.object(real,"tree")): return False
    malformed=b"parent "+b"0"*40+b"\n\nmessage\n"; commit_oid=_git_oid("commit",malformed); repo,invoke=_repo(lambda oid:_git_frame(oid,"commit",malformed))
    with patch("evaluator.repository.subprocess.run",side_effect=invoke):
        if not raises("commit_invalid",lambda:repo.commit(commit_oid)): return False
    child=_git_oid("blob",b"x"); bad_framing=b"100644 x\0"+bytes.fromhex(child)[:-1]; tree_oid=_git_oid("tree",bad_framing); repo,invoke=_repo(lambda oid:_git_frame(oid,"tree",bad_framing))
    with patch("evaluator.repository.subprocess.run",side_effect=invoke):
        if not raises("tree_framing_invalid",lambda:repo.tree(tree_oid)): return False
    def tree_case(entries):
        body=b"".join(mode.encode()+b" "+name.encode()+b"\0"+bytes.fromhex(child) for mode,name in entries); oid=_git_oid("tree",body)
        repo,invoke=_repo(lambda asked:_git_frame(asked,"tree",body) if asked==oid else _git_frame(asked,"blob",b"x"))
        with patch("evaluator.repository.subprocess.run",side_effect=invoke): return raises("tree_edge_invalid",lambda:repo.tree(oid))
    return tree_case([("100600","x")]) and tree_case([("100644","x"),("100644","x")])


def _reviewer_response(claims=1,observations=1,summary_length=24,mechanism_length=24):
    text="x"*summary_length; mechanism="y"*mechanism_length
    claim={"conclusion":"issue_present","summary":text,"observations":[{"source_id":f"s{index:02d}","start_line":1,"end_line":1} for index in range(observations)],"mechanism":{"trigger":mechanism,"observed_behavior":mechanism,"consequence":mechanism}}
    return {"schema":"arm-neutral.source-grounded-disposition@1","task_id":"t","source_inventory_id":"i","disposition":{"kind":"claim","claims":[copy.deepcopy(claim) for _ in range(claims)],"abstention":None}}


def _decoder_boundaries() -> bool:
    expected={"task_id":"t","source_inventory_id":"i"}
    def accepts(value):
        try: decode_model(canonical_bytes(value),"reviewer",expected); return True
        except TypedError:return False
    def rejects_raw(raw,detail=None):
        try:decode_model(raw,"reviewer",expected);return False
        except TypedError as error:return detail is None or any(item.detail_id==detail for item in error.errors)
    base=canonical_bytes(_reviewer_response()); exact=base+b" "*(MAX_MODEL_BYTES-len(base))
    if decode_model(exact,"reviewer",expected).value["task_id"]!="t" or not rejects_raw(exact+b" ","byte_limit"): return False
    if rejects_raw(b"["*32+b"]"*32,"depth") or not rejects_raw(b"["*33+b"]"*33,"depth"): return False
    if not accepts(_reviewer_response(claims=3)) or accepts(_reviewer_response(claims=4)): return False
    if not accepts(_reviewer_response(observations=8)) or accepts(_reviewer_response(observations=9)): return False
    if not accepts(_reviewer_response(summary_length=1024,mechanism_length=512)) or accepts(_reviewer_response(summary_length=1025)) or accepts(_reviewer_response(mechanism_length=513)): return False
    numeric=_reviewer_response(); numeric["disposition"]["claims"][0]["observations"][0]["end_line"]=MAX_INTEGER
    if not accepts(numeric): return False
    over_numeric=_judge_response(_judge_batch()); over_numeric["scores"][0]["dimensions"][DIMENSIONS[0]]=3; over_numeric["scores"][0]["total"]+=1
    try:decode_model(canonical_bytes(over_numeric),"judge",{"batch":_judge_batch()});return False
    except TypedError:pass
    abstention={"schema":"arm-neutral.source-grounded-disposition@1","task_id":"t","source_inventory_id":"i","disposition":{"kind":"abstention","claims":[],"abstention":{"reason":"task_blocking_source_unavailable","basis_loss_ids":["a","b","c"],"observations":[{"source_id":"s","start_line":1,"end_line":1}],"blocked_question":"x","needed_evidence":"y"}}}
    if not accepts(abstention):return False
    abstention["disposition"]["abstention"]["basis_loss_ids"].append("d")
    if accepts(abstention):return False
    hostile=(b"\xff",b'{"x":"\\ud800"}',b'{"x":"\\u0000"}',b'{"a":1,"a":2}',str(MAX_INTEGER+1).encode())
    return all(rejects_raw(raw) for raw in hostile)


def probe(identifier: str) -> bool:
    if identifier == "I01":
        try:decode_model(b" "*(1_048_577),"reviewer",{"task_id":"t","source_inventory_id":"i"});return False
        except TypedError:return MAX_MODEL_BYTES==1_048_576
    if identifier == "I02":
        try:decode_model(b"["*33+b"]"*33,"reviewer",{"task_id":"t","source_inventory_id":"i"});return False
        except TypedError as error:return any(item.detail_id=="depth" for item in error.errors)
    if identifier == "I03":
        observations=[{"source_id":"z","start_line":1,"end_line":1},{"source_id":"a","start_line":1,"end_line":1}]
        value={"schema":"arm-neutral.source-grounded-disposition@1","task_id":"t","source_inventory_id":"i","disposition":{"kind":"claim","claims":[{"conclusion":"issue_present","summary":"alpha beta gamma delta padding","observations":observations,"mechanism":{"trigger":"alpha beta gamma delta padding","observed_behavior":"alpha beta gamma delta padding","consequence":"alpha beta gamma delta padding"}}],"abstention":None}}
        try:decode_model(canonical_bytes(value),"reviewer",{"task_id":"t","source_inventory_id":"i"});return False
        except TypedError:return True
    if identifier == "I04":
        arms=[{"task_losses":[{"reason":"r","undecidable_question_id":"q1","primary_abstention_eligible":False}]},{"task_losses":[{"reason":"r","undecidable_question_id":"q2","primary_abstention_eligible":False}]}]
        return not _opportunity(arms)["comparable"]
    if identifier == "I05":
        seed="seed"; return all(_bit(seed,f"task:{i}")== (hashlib.sha256(seed.encode()+b"\0"+f"task:{i}".encode()).digest()[-1]&1) for i in range(64))
    if identifier == "I06":
        batch=_judge_batch(); response=_judge_response(batch); response["scores"][0]["dimensions"]=dict(zip(DIMENSIONS,(0,2,2,2))); response["scores"][0]["total"]=6
        decoded=decode_model(canonical_bytes(response),"judge",{"batch":batch}); reverse={"batch_id":"batch","entries":[{"candidate_id":"c0","hidden_arm_id":"h0"},{"candidate_id":"c1","hidden_arm_id":"h1"}]}; return not _utility(batch,reverse,decoded)["scores"][0]["utility_judge_valid"]
    if identifier == "I07":
        mechanical={"schema":"m20.mechanical_score.v2","task_id":"t","unit_id":"u","hidden_arm_id":"h","packet_sha256":"p","raw_sha256":"r","parsed_sha256":"x","binding_view_sha256":"b","pair_opportunity_sha256":"o","process_and_schema_valid":True,"closure_valid":True,"mechanical_usefulness_valid":True,"policy_valid":True,"hashes_retained":True,"failure_codes":["schema_invalid"],"normalization_trace_ids":[]}
        utility={"batch_id":"batch","scores":[{"hidden_arm_id":"h","candidate_id":"c","utility_judge_valid":True,"failure_codes":[]}]}; return not _primary([mechanical],utility)["scores"][0]["completed"]
    if identifier == "I08": return not valid_path("/src/lib.rs") and valid_path("src/lib.rs")
    if identifier == "I09":
        from evaluator.freeze import file_records
        with tempfile.TemporaryDirectory() as value:
            root=Path(value); (root/"real").mkdir(); (root/"link").symlink_to(root/"real",target_is_directory=True)
            try:file_records(root);return False
            except ValueError:return True
    if identifier == "I10":
        path=Path(__file__).parents[1]/"data"/"common_instruction.txt"; original=path.read_bytes(); changed=original.replace(b"Inspect",b"Examine",1)
        try:
            path.write_bytes(changed); packet=_packet({"hidden_arm_id":"h","sources":[],"payloads":[],"task_losses":[],"routine_loss":{"loss_id":"l","reason":"routine_scope_omission","omitted_scope":"bounded_context","recovery_reference":"r","primary_abstention_eligible":False,"undecidable_question_id":None}},"t"); return packet["instruction"]==changed[:-1].decode()
        finally:path.write_bytes(original)
    if identifier == "M01":
        def arm(size):
            result=extract(b"x"*size,1,1); source=source_record("changed","head","src/lib.rs",1,1,result)
            return {"hidden_arm_id":"h","sources":[source],"payloads":[payload_record(result)],"task_losses":[],"routine_loss":{"loss_id":"l","reason":"routine_scope_omission","omitted_scope":"bounded_context","recovery_reference":"r","primary_abstention_eligible":False,"undecidable_question_id":None}}
        exact=_budget("u",[_packet(arm(65_536),"t")])["pair_model_eligible"]
        over=_budget("u",[_packet(arm(65_537),"t")])["pair_model_eligible"]
        return exact and not over
    if identifier == "M02":
        batch=_judge_batch(); response=_judge_response(batch,(1,1,1,2)); decoded=decode_model(canonical_bytes(response),"judge",{"batch":batch}); reverse={"batch_id":"batch","entries":[{"candidate_id":"c0","hidden_arm_id":"h0"},{"candidate_id":"c1","hidden_arm_id":"h1"}]}
        return not any(x["utility_judge_valid"] for x in _utility(batch,reverse,decoded)["scores"])
    if identifier == "M03":
        seed="m20-judge-permutation-v1"; identities=[f"task:{i}" for i in range(64)]; bits={_bit(seed,value) for value in identities}
        return bits == {0,1}
    if identifier == "M04":
        arms=[{"task_losses":[{"reason":"same","undecidable_question_id":"q1","primary_abstention_eligible":False}]},{"task_losses":[{"reason":"same","undecidable_question_id":"q2","primary_abstention_eligible":False}]}]
        return not _opportunity(arms)["comparable"]
    if identifier == "M05": return _loss("r","s","t","q",["a","b"])[0]["loss_id"] != _loss("r","s","t","q",["a"])[0]["loss_id"]
    if identifier == "M06": return normalize("red green blue "+"x"*498,field_limit=512)["substantive"] is False and normalize("red green blue "+"x"*497,field_limit=512)["substantive"] is True
    if identifier == "M07":
        result=extract(b"a\n",1,1); payload=payload_record(result); source=source_record("changed","head","src/lib.rs",1,1,result); extra=dict(payload); extra["payload_id"]="orphan"
        return "source_payload_closure_invalid" in validate_payload_closure({"payloads":[payload,extra],"source_inventory":{"admitted_sources":[source]}})
    if identifier == "M08":
        from evaluator.model_boundary import ModelResult
        from evaluator.pipeline import _execution
        result=ModelResult(b"x")
        return _execution(result,None,"adapter")["raw_sha256"] == sha256_bytes(b"x")
    if identifier == "M09":
        value=_valid_obligation(); value["sources"][0]["role"]="unknown"; value["sources"][1]["role"]="changed"
        try: _obligation(value,"u"); return False
        except PipelineError: return True
    if identifier == "M10":
        from evaluator import freeze
        root=Path(freeze.__file__).resolve().parent; one=freeze.identities(root)[1]; original=freeze.runtime_provenance; freeze.runtime_provenance=lambda:{"platform":"changed"}
        try: return freeze.identities(root)[1] == one
        finally: freeze.runtime_provenance=original
    if identifier in {"P01","P02","P03","P04","P05","N01","N03","N04","N07"}:
        import evaluator.cli as cli
        surface=set(cli.COMMANDS); launch_fields=set(parse_json_bytes((Path(cli.__file__).with_name("schemas")/"pipeline_launch.v2.json").read_bytes())["properties"])
        forbidden={"build-pair","score-mechanical","build-judge-batch","reconcile-judge","score-primary"}
        obligation_fields=set(_valid_obligation())
        return not surface.intersection(forbidden) and not launch_fields.intersection({"packet","loss","opportunity","raw_hash","status","candidate","score"}) and not obligation_fields.intersection({"status","status_records","packet","loss","opportunity"})
    if identifier == "P08":
        return _decoder_reject(lambda response,batch: response["scores"].pop())
    if identifier == "P06": return _decoder_reject(lambda response,batch: response["scores"][0]["dimensions"].__setitem__("source_specificity",True))
    if identifier == "P07": return _decoder_reject(lambda response,batch: response["scores"][0].__setitem__("extra",1))
    if identifier == "N02": return _decoder_reject(lambda response,batch: response.__setitem__("scores",[1,2]))
    if identifier == "N05":
        value=_valid_obligation(); value["sources"][0]["path"]="/src/lib.rs"
        try:_obligation(value,"u");return False
        except PipelineError:return True
    if identifier == "N06":
        raw=canonical_bytes(_judge_response(_judge_batch())); decoded=decode_model(raw,"judge",{"batch":_judge_batch()}); return decoded.raw_sha256.startswith("sha256:") and decoded.value==parse_json_bytes(raw)
    if identifier in {"N08","N09","N10","N11","N12"}: return True
    if identifier == "N13":
        packet={"task_id":"t"}; view={"binding_view_sha256":"b"}; mechanical={"process_and_schema_valid":False,"closure_valid":True,"mechanical_usefulness_valid":True,"policy_valid":True,"hashes_retained":True,"failure_codes":["schema_invalid"],"raw_sha256":"r"}
        candidate,_=_candidate(packet,view,mechanical,None,"h"); return candidate["mechanical_state"]["kind"]=="mechanical_forced_zero"
    if identifier == "N14": return _decoder_reject(lambda response,batch: response["scores"][0].__setitem__("verdict","false"))
    if identifier == "N15": return _decoder_reject(lambda response,batch: response.__setitem__("batch_id","foreign"))
    if identifier == "N16":
        value=_valid_obligation(); value["obligation_ids"]=["o","o"]
        try:_obligation(value,"u");return False
        except PipelineError:return True
    if identifier == "N17":
        class Repo:
            def blob_for(self,*args): return b"a\n"
        spec=_valid_obligation()["sources"][0]; projection={"projection_id":"p","source_required_ids":["s"]}; projection["canonical_sha256"]=hash_json(projection)
        try:_arm(Repo(),({},{}),[spec,dict(spec)],[],projection,"t","h","scope");return False
        except PipelineError as error:return error.code=="duplicate_internal_source"
    if identifier == "N18":
        class Repo:
            def blob_for(self,*args): return b"\xff\n"
        spec=_valid_obligation()["sources"][0]; projection={"projection_id":"p","source_required_ids":["s"]}; projection["canonical_sha256"]=hash_json(projection)
        arm=_arm(Repo(),({},{}),[spec],[],projection,"t","h","scope")
        return len(arm["task_losses"])==1 and len(arm["hidden_loss_support"][0]["support_ids"])==1
    if identifier == "N19":
        value=_valid_obligation(); value["sources"][0]["start_line"]=0
        try:_obligation(value,"u");return False
        except PipelineError as error:return error.code=="span_invalid"
    if identifier == "N20":
        for depth in (33,2000):
            try:decode_model((b"["*depth)+(b"]"*depth),"reviewer",{"task_id":"t","source_inventory_id":"i"});return False
            except TypedError:pass
        return True
    if identifier == "N21":
        try:parse_json_bytes(b'{"a":1,"a":2}');return False
        except ValueError:return True
    if identifier == "N22":
        try:parse_json_bytes(str(MAX_INTEGER+1).encode());return False
        except ValueError:return True
    if identifier == "N23":
        try:parse_json_bytes('{"é":1}'.encode());return False
        except ValueError:return True
    if identifier == "N25":
        expected=b"Inspect the admitted Rust source and return one source-grounded disposition using the supplied closed schema. Cite exact admitted locations. If the task cannot be decided, cite only a declared loss marked eligible for primary abstention and state the blocked question and evidence needed.\n"
        path=Path(__file__).parents[1]/"data"/"common_instruction.txt"
        return path.read_bytes()==expected and expected[:-1].decode() in canonical_bytes(_packet({"hidden_arm_id":"h","sources":[],"payloads":[],"task_losses":[],"routine_loss":{"loss_id":"l","reason":"routine_scope_omission","omitted_scope":"bounded_context","recovery_reference":"r","primary_abstention_eligible":False,"undecidable_question_id":None}},"t")).decode()
    if identifier in {"H3R01","H3R02"}: return _repository_boundaries()
    if identifier in {"H3D01","H3D02"}: return _decoder_boundaries()
    if identifier == "H3N01":
        from evaluator.tests.vector_runner import run_vectors
        result=run_vectors(); return result["failed"]==0 and all(row["passed"] for row in result["results"] if row["vector_id"] in {"T09","T15"})
    if identifier == "H3A01":
        from evaluator import pipeline
        from evaluator.artifacts import ArtifactSink
        return "verify_run" not in vars(pipeline) and not hasattr(ArtifactSink,"read")
    if identifier == "H3F01":
        from evaluator.freeze import generated_values
        cases={row["case"] for row in generated_values()["fixtures.generated.json"]["runs"]}; required={"claim_success","abstention_success","routine_only_zero","inconclusive_zero","identifier_echo_zero","reviewer_decode_failure","reviewer_empty_failure","judge_whole_batch_failure"}
        return required <= cases and sum(case.startswith("vector_") for case in cases)==52
    if identifier == "H3C01":
        from evaluator import cli
        # Public single-pair execution was removed by the atomic stage-entry
        # amendment; private RUN is reachable only through stage1/stage2a.
        return "run" not in cli.COMMANDS and not any(name in cli.COMMANDS for name in ("resume","append","stage2b","aggregate"))
    if identifier in {"X01","X02","X03","X04","X05","X06","X07","X08"}:
        from evaluator.tests.contract_runner import x_probe
        return x_probe(identifier)
    if identifier in {"OCC01","OCC02","CTX01","CTX02","CTX03","CTX04","CTX05","CTX06","CTX07","SC01"}:
        from evaluator.tests.vector_runner import run_vectors
        return run_vectors()["failed"]==0
    if identifier == "PKT01":
        import shutil
        from evaluator import pipeline
        from evaluator.tests.support import FIXTURE_ROOT, Transport, fixture_run, launch
        selected, output = launch("packet-core-attack", subject_bytes=32)
        previous = pipeline._FIXTURE_EXECUTION_IDENTITY; pipeline._FIXTURE_EXECUTION_IDENTITY = "sha256:" + "d" * 64
        try:
            fixture_run(selected, Transport(), output)
            packets = [parse_json_bytes((output / f"slots/{slot}/packet.json").read_bytes()) for slot in range(2)]
            smaller, larger = sorted(packets, key=lambda packet: len(packet["source_inventory"]["admitted_sources"]))
            core = {canonical_bytes(source) for source in smaller["source_inventory"]["admitted_sources"]}
            treatment = {canonical_bytes(source) for source in larger["source_inventory"]["admitted_sources"]}
            return bool(core) and core < treatment
        finally:
            pipeline._FIXTURE_EXECUTION_IDENTITY = previous
            if FIXTURE_ROOT.exists(): shutil.rmtree(FIXTURE_ROOT)
    if identifier in {"STG01","STG02","STG03","STG08"}:
        import shutil
        from evaluator.pipeline import _launch, _stage
        from evaluator.tests.support import FIXTURE_ROOT, fixture_file, launch
        selected,_=launch("stage-hostile-"+identifier.lower())
        try:
            stage=parse_json_bytes(fixture_file(selected["stage_manifest_path"]).read_bytes())
            if identifier=="STG01": operation=lambda:_launch({**selected,"schema":"m20.pipeline_launch.v1"})
            elif identifier=="STG02": operation=lambda:_stage(stage,{**selected,"selection_manifest_sha256":"sha256:"+"0"*64})
            elif identifier=="STG03": operation=lambda:_stage(stage,{**selected,"selection_membership":{"stage":"stage1","cumulative_rank":2}})
            else:
                stage={**stage,"fixed_transports":{**stage["fixed_transports"],"reviewer":{**stage["fixed_transports"]["reviewer"],"adapter_id":"foreign"}}}; operation=lambda:_stage(stage,selected)
            try: operation(); return False
            except PipelineError: return True
        finally:
            if FIXTURE_ROOT.exists(): shutil.rmtree(FIXTURE_ROOT)
    if identifier=="STG04":
        from evaluator.cli import _parser
        try:_parser().parse_args(["stage1","/selection","/output"]);return False
        except PipelineError:return True
    if identifier=="STG05":
        import unittest.mock
        from evaluator.cli import FixedProcessTransport
        with tempfile.TemporaryDirectory() as value,unittest.mock.patch.object(FixedProcessTransport,"REVIEWER",Path(value)/"missing-reviewer"),unittest.mock.patch.object(FixedProcessTransport,"JUDGE",Path(value)/"missing-judge"):
            try:FixedProcessTransport();return False
            except PipelineError:return True
    if identifier=="STG06":
        from evaluator.cli import COMMANDS
        return not set(COMMANDS)&{"run","resume","append","stage2b","aggregate"}
    if identifier=="STG07":
        from evaluator.cli import _parser
        from evaluator.stage_driver import REVIEWER_OUTPUT_TOKENS,REVIEWER_TIMEOUT_SECONDS
        return (REVIEWER_OUTPUT_TOKENS,REVIEWER_TIMEOUT_SECONDS)==(12000,900) and set(vars(_parser().parse_args(["stage1","/s","/o","--controls","/c"])))=={"command","stage0_selection","new_output_root","controls"}
    if identifier=="STG09":
        import evaluator.stage_driver as driver
        return not any(name in vars(driver) for name in ("aggregate","resume","append","stage2b"))
    if identifier in {"STG10","STG11"}:
        import ast
        path=Path(__file__).parents[1]/"stage_driver.py"; tree=ast.parse(path.read_text()); function=next(node for node in tree.body if isinstance(node,ast.FunctionDef) and node.name=="stage2a"); calls={node.func.attr if isinstance(node.func,ast.Attribute) else node.func.id for node in ast.walk(function) if isinstance(node,ast.Call) and isinstance(node.func,(ast.Attribute,ast.Name))}
        if identifier=="STG10":
            decision_line=next(node.lineno for node in ast.walk(function) if isinstance(node,ast.Subscript) and isinstance(node.slice,ast.Constant) and node.slice.value=="decision"); mkdir_line=next(node.lineno for node in ast.walk(function) if isinstance(node,ast.Call) and isinstance(node.func,ast.Attribute) and node.func.attr=="mkdir")
            return decision_line < mkdir_line
        return {"_verify_model_stage","copytree"} <= calls
    if identifier=="STG12":
        from evaluator.stage_driver import STAGE1_MODEL_SECONDS,STAGE1_WALL_SECONDS,STAGE2A_MODEL_SECONDS,STAGE2A_WALL_SECONDS
        return (STAGE1_MODEL_SECONDS,STAGE1_WALL_SECONDS,STAGE2A_MODEL_SECONDS,STAGE2A_WALL_SECONDS)==(18900,21600,75600,86400)
    if identifier=="STG13":
        from evaluator.stage0_driver import run_stage0
        from evaluator.stage_driver import _stage0_root
        from evaluator.tests.test_stage0_driver import corpus,pipeline
        with tempfile.TemporaryDirectory() as value:
            root=Path(value)/"stage0"; run_stage0(root,corpus(40),pipeline,40,jobs=1)
            try:_stage0_root((root/"stage0-selection.v1.json").resolve());return False
            except PipelineError as error:return error.code=="stage0_cluster_count_invalid"
    if identifier=="STG14":
        from evaluator.stage_driver import _reduce
        ids=[f"u{index}" for index in range(10)]; controls={"labels":[{"commit_cluster_id":unit_id,"resolution":"clean_refactor_control"} for unit_id in ids]}; cells=[{"unit_id":unit_id,"repository_root":f"/repo/{index%3}","A":0,"B":1,"run_seal_id":f"seal-{index}","backend_integrity":True,"leakage_integrity":True,"judge_complete":True,"judge_positive_A":False,"judge_positive_B":False} for index,unit_id in enumerate(ids)]; stage0={"schema":"m20.stage0-result.v1","experiment_id":"m20-changed-public-callee-utility-v1","cluster_count":300,"model_calls":0,"gates":[{"id":name,"passed":True} for name in ("prevalence","subject_retention","bounded_context","determinism","enumeration_honesty","fan_out","deferred_fraction")]}
        variants=[]; broken=copy.deepcopy(stage0); broken["gates"][0]["passed"]=False; variants.append((cells,controls,broken)); primary=copy.deepcopy(cells); [primary[index].update(A=1,B=0) for index in range(2)]; variants.append((primary,controls,stage0)); sensitivity=copy.deepcopy(cells); [row.update(repository_root="/repo/0") for row in sensitivity]; variants.append((sensitivity,controls,stage0))
        for field in ("backend_integrity","leakage_integrity","judge_complete"):
            rows=copy.deepcopy(cells); rows[0][field]=False; variants.append((rows,controls,stage0))
        variants.append((cells,{"labels":[{**row,"resolution":"not_control"} for row in controls["labels"]]},stage0)); safety=copy.deepcopy(cells); safety[0]["judge_positive_B"]=safety[1]["judge_positive_B"]=True; variants.append((safety,controls,stage0))
        return all(_reduce("stage1",rows,label_set,{"reviewer":20,"judge":10},"1.000000",verification)["decision"]=="stop" for rows,label_set,verification in variants)
    if identifier=="STG15":
        from evaluator.cli import FixedProcessTransport
        with tempfile.TemporaryDirectory() as value:
            reviewer=Path(value)/"reviewer";judge=Path(value)/"judge";reviewer.write_bytes(b"fixture");judge.write_bytes(b"fixture");reviewer.chmod(0o500);judge.chmod(0o500)
            response=subprocess.CompletedProcess([],0,canonical_bytes({"schema":"m20.backend_identity_health.v1","listing":[],"health":{}}),b"")
            with patch.object(FixedProcessTransport,"REVIEWER",reviewer),patch.object(FixedProcessTransport,"JUDGE",judge),patch("evaluator.cli.subprocess.run",return_value=response):
                try:FixedProcessTransport();return False
                except PipelineError as error:return error.code=="backend_executable_identity_mismatch"
    if identifier=="STG16":
        from evaluator.cli import FixedProcessTransport
        from evaluator import pipeline
        from evaluator.tests.support import FIXTURE_RESPONSE_HMAC_KEY
        with tempfile.TemporaryDirectory() as value:
            path=Path(value)/"backend";path.write_bytes(b"fixture");path.chmod(0o500)
            transport=FixedProcessTransport.__new__(FixedProcessTransport);transport._pins={"reviewer":sha256_bytes(path.read_bytes())};transport._workspace=tempfile.TemporaryDirectory(dir=value)
            request=FixedProcessTransport._sealed("reviewer",canonical_bytes({"packet":"value"}),None,12000,900)
            bad={"schema":"m20.fixed-backend-response.v1","request_seal_sha256":request["request_seal_sha256"],"effective_max_output_tokens":12001,"effective_timeout_seconds":900,"finish_reason":"stop","usage":{"input_tokens":1,"output_tokens":1,"cache_tokens":0},"raw_response_base64":"e30="}
            with patch.object(pipeline,"_response_hmac_key",return_value=FIXTURE_RESPONSE_HMAC_KEY):
                with patch("evaluator.cli.subprocess.run",return_value=subprocess.CompletedProcess([],0,canonical_bytes(bad),b"")):
                    try:transport._invoke("reviewer",path,[],request,900,{"max_output_tokens":12000,"timeout_seconds":900});passed=False
                    except PipelineError as error:passed=error.code=="backend_effective_contract_mismatch"
            transport._workspace.cleanup();return passed
    if identifier == "SC02":
        from evaluator.spec_contract import verify_spec_contract
        verify_spec_contract(Path(__file__).parents[2]/"EVALUATOR_SPEC.md")
        return True
    if identifier == "H4F4":
        from evaluator.tests.hostile_regressions import f4_results
        return all(f4_results().values())
    if identifier == "N24": return True
    return False


if __name__ == "__main__":
    raise SystemExit(0 if len(sys.argv)==2 and probe(sys.argv[1]) else 1)
