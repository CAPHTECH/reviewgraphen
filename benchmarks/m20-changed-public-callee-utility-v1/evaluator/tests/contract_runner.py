"""Derive executable cases from the independent specification transcription."""
import copy
import hashlib
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from evaluator.canonical import MAX_INTEGER, canonical_bytes, hash_json
from evaluator.model_boundary import DIMENSIONS, TypedError, decode_model
from evaluator.pipeline import MAX_SOURCE_BYTES, PipelineError, _budget, _observations, _obligation, _packet, _primary, _utility
from evaluator.repository import GitRepository, PreflightError
from evaluator.source_payload import extract, payload_record, source_record
from evaluator.textnorm import normalize
from .attack_probe import _judge_batch, _judge_response, _reviewer_response, _valid_obligation
from .spec_contracts import SPEC_CONTRACTS, derived_cases


def _accept_reviewer(value):
    try: decode_model(canonical_bytes(value),"reviewer",{"task_id":"t","source_inventory_id":"i"}); return True
    except TypedError:return False


def _arm_with_bytes(sizes):
    sources=[]; payloads={}
    for index,size in enumerate(sizes):
        result=extract(bytes([97+index])*size,1,1); source=source_record("changed","head",f"src/{index}.rs",1,1,result); sources.append(source); payloads[result["payload_id"]]=payload_record(result)
    return {"hidden_arm_id":"h","sources":sources,"payloads":sorted(payloads.values(),key=lambda x:x["payload_id"]),"task_losses":[],"routine_loss":{"loss_id":"l","reason":"routine_scope_omission","omitted_scope":"bounded_context","recovery_reference":"r","primary_abstention_eligible":False,"undecidable_question_id":None}}


def _numeric(row,value):
    surface=row["surface"]
    if surface in {"claims","observations","summary","mechanism"}: return _accept_reviewer(_reviewer_response(**({"claims":value} if surface=="claims" else {"observations":value} if surface=="observations" else {"summary_length":value} if surface=="summary" else {"mechanism_length":value})))
    if surface=="basis_losses":
        output={"schema":"arm-neutral.source-grounded-disposition@1","task_id":"t","source_inventory_id":"i","disposition":{"kind":"abstention","claims":[],"abstention":{"reason":"task_blocking_source_unavailable","basis_loss_ids":[f"l{i}" for i in range(max(0,value))],"observations":[{"source_id":"s","start_line":1,"end_line":1}],"blocked_question":"x","needed_evidence":"y"}}}; return _accept_reviewer(output)
    if surface=="line":
        output=_reviewer_response(); output["disposition"]["claims"][0]["observations"][0]["start_line"]=value; return _accept_reviewer(output)
    if surface=="model_bytes":
        base=canonical_bytes(_reviewer_response()); raw=base+b" "*max(0,value-len(base))
        try:decode_model(raw,"reviewer",{"task_id":"t","source_inventory_id":"i"});return True
        except TypedError:return False
    if surface=="model_depth":
        try:decode_model(b"["*value+b"]"*value,"reviewer",{"task_id":"t","source_inventory_id":"i"})
        except TypedError as error:return not any(item.detail_id=="depth" for item in error.errors)
    if surface in {"dimension","judge_total","score_records"}:
        batch=_judge_batch(); response=_judge_response(batch)
        if surface=="dimension": response["scores"][0]["dimensions"][DIMENSIONS[0]]=value; response["scores"][0]["total"]=sum(response["scores"][0]["dimensions"].values())
        elif surface=="judge_total":
            remaining=max(0,value); numbers=[]
            for _ in DIMENSIONS: numbers.append(min(2,remaining)); remaining-=numbers[-1]
            response["scores"][0]["dimensions"]=dict(zip(DIMENSIONS,numbers)); response["scores"][0]["total"]=value
        else: response["scores"]=(response["scores"]+[copy.deepcopy(response["scores"][0])])[:max(0,value)]
        try:decode_model(canonical_bytes(response),"judge",{"batch":batch});return True
        except TypedError:return False
    if surface=="source_bytes":
        try:return _budget("u",[_packet(_arm_with_bytes([value]),"t")])["pair_model_eligible"]
        except (PipelineError,ValueError):return False
    if surface=="diff_context": return _diff_context()==value
    if surface in {"text_claim","text_prose"}:
        limit=1024 if surface=="text_claim" else 512; return normalize("a b c "+"x"*max(0,value-6),field_limit=limit)["substantive"]
    if surface=="distinct_tokens": return normalize(" ".join(chr(97+i) for i in range(max(0,value)))+" "*24)["substantive"]
    raise AssertionError(surface)


def _tree_mode(mode):
    child_body=b"" if mode=="40000" else b"x"; child_kind="tree" if mode=="40000" else "blob"; child=_oid(child_kind,child_body); body=mode.encode()+b" x\0"+bytes.fromhex(child); tree=_oid("tree",body); repo=object.__new__(GitRepository); repo.object_format="sha1"; repo.oid_bytes=20; repo._cache={tree:("tree",body),child:(child_kind,child_body)}
    try:repo.tree(tree);return True
    except PreflightError:return False


def _oid(kind,body): return hashlib.sha1(kind.encode()+b" "+str(len(body)).encode()+b"\0"+body).hexdigest()


def _enum(row,value):
    surface=row["surface"]
    if surface=="tree_mode": return _tree_mode(value)
    if surface in {"source_role","snapshot_side"}:
        obligation=_valid_obligation(); source=obligation["sources"][0]; source["role" if surface=="source_role" else "snapshot_side"]=value
        if surface=="source_role" and value!="changed": obligation["sources"][1]["role"]="changed"
        try:_obligation(obligation,"u");return True
        except PipelineError:return False
    if surface in {"conclusion","disposition_kind","abstention_reason"}:
        output=_reviewer_response()
        if surface=="conclusion":output["disposition"]["claims"][0]["conclusion"]=value
        elif surface=="disposition_kind":
            if value=="abstention": output={"schema":"arm-neutral.source-grounded-disposition@1","task_id":"t","source_inventory_id":"i","disposition":{"kind":"abstention","claims":[],"abstention":{"reason":"task_blocking_source_unavailable","basis_loss_ids":["l"],"observations":[{"source_id":"s","start_line":1,"end_line":1}],"blocked_question":"x","needed_evidence":"y"}}}
            elif value!="claim":output["disposition"]["kind"]=value
        else:
            output={"schema":"arm-neutral.source-grounded-disposition@1","task_id":"t","source_inventory_id":"i","disposition":{"kind":"abstention","claims":[],"abstention":{"reason":value,"basis_loss_ids":["l"],"observations":[{"source_id":"s","start_line":1,"end_line":1}],"blocked_question":"x","needed_evidence":"y"}}}
        return _accept_reviewer(output)
    batch=_judge_batch(forced=value=="mechanical_forced_zero"); response=_judge_response(batch)
    if surface=="verdict":response["scores"][0]["verdict"]=value
    else:response["scores"][1 if value=="mechanical_forced_zero" else 0]["score_source"]=value
    try:decode_model(canonical_bytes(response),"judge",{"batch":batch});return True
    except TypedError:return False


def _diff_context():
    before="".join(f"a{i}\n" for i in range(10)).encode(); after=before.replace(b"a4\n",b"changed\n"); boid,aoid=_oid("blob",before),_oid("blob",after); repo=object.__new__(GitRepository); repo.object_format="sha1"; repo.oid_bytes=20; repo._cache={boid:("blob",before),aoid:("blob",after)}; specs=repo.baseline_specs(({"src/lib.rs":("100644",boid)},{"src/lib.rs":("100644",aoid)})); return 4-specs[0]["start_line"]+1


def _conjunction(row,dropped):
    surface=row["surface"]
    if surface=="first_parent":
        class Repo(GitRepository):
            def commit(self,oid): return (oid,[] if dropped=="has_parent" else ["other"] if dropped=="first_parent_is_base" else ["base"])
            def tree(self,oid): return {}
        repo=object.__new__(Repo)
        try:repo.snapshots("base","head");return True
        except PreflightError:return False
    if surface=="utility":
        batch=_judge_batch(); numbers=[1,1,2,2]
        if dropped=="judgeable":batch["candidates"][0]["mechanical_state"]={"kind":"mechanical_forced_zero"}
        if dropped in DIMENSIONS:numbers[DIMENSIONS.index(dropped)]=0
        if dropped=="total_at_least_6":numbers=[1,1,1,1]
        response=_judge_response(batch,numbers)
        if dropped=="verdict_usable":response["scores"][0]["verdict"]="not_usable"
        decoded=decode_model(canonical_bytes(response),"judge",{"batch":batch}); reverse={"batch_id":"batch","entries":[{"candidate_id":"c0","hidden_arm_id":"h0"},{"candidate_id":"c1","hidden_arm_id":"h1"}]}; return _utility(batch,reverse,decoded)["scores"][0]["utility_judge_valid"]
    if surface=="changed_observation":
        source={"source_id":"s","role":"context" if dropped=="role_changed" else "changed","range":{"start_line":1,"end_line":2}}; packet={"source_inventory":{"admitted_sources":[source]}}; observation={"source_id":"foreign" if dropped=="known_source" else "s","start_line":1,"end_line":3 if dropped=="range_admitted" else 2}; return not _observations([observation],packet)
    if surface=="primary":
        mechanical={"schema":"m20.mechanical_score.v2","task_id":"t","unit_id":"u","hidden_arm_id":"h","packet_sha256":"p","raw_sha256":"r","parsed_sha256":"x","binding_view_sha256":"b","pair_opportunity_sha256":"o","process_and_schema_valid":True,"closure_valid":True,"mechanical_usefulness_valid":True,"policy_valid":True,"hashes_retained":True,"failure_codes":[],"normalization_trace_ids":[]}; utility={"batch_id":"batch","scores":[{"hidden_arm_id":"h","candidate_id":"c","utility_judge_valid":True,"failure_codes":[]}]}
        if dropped in mechanical:mechanical[dropped]=False
        elif dropped=="utility_judge_valid":utility["scores"][0]["utility_judge_valid"]=False;utility["scores"][0]["failure_codes"]=["utility_threshold_failed"]
        elif dropped=="failure_codes_empty":mechanical["failure_codes"]=["schema_invalid"]
        return _primary([mechanical],utility)["scores"][0]["completed"]
    if surface=="source_ceiling":
        arm=_arm_with_bytes([40000]); arm["sources"].append(dict(arm["sources"][0])) if dropped=="sum_every_admitted_source" else None
        if dropped=="at_most_65536":arm=_arm_with_bytes([65537])
        try:return _budget("u",[_packet(arm,"t")])["pair_model_eligible"]
        except PipelineError:return False
    raise AssertionError(surface)


def execute_case(case):
    kind,row,value,expected=case; observed=_numeric(row,value) if kind=="numeric" else _enum(row,value) if kind=="enum" else _conjunction(row,value); return observed==expected


def x_probe(identifier):
    surfaces={"X01":{"claims"},"X02":{"observations"},"X03":{"tree_mode"},"X04":{"first_parent"},"X05":{"diff_context"},"X06":{"utility"},"X07":{"changed_observation"},"X08":{"source_ceiling"}}[identifier]
    selected=[case for case in derived_cases() if case[1]["surface"] in surfaces]
    return bool(selected) and all(execute_case(case) for case in selected)
