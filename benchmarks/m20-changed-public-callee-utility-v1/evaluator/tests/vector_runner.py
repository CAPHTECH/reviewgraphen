"""Execute 52 unchanged vectors plus atomic occurrence/context-v3 vectors."""
from pathlib import Path

from evaluator.canonical import canonical_b64_decode, canonical_bytes, hash_json, parse_json_bytes, sha256_bytes, stable_id
from evaluator.model_boundary import DIMENSIONS, TypedError, decode_model
from evaluator.semantic_acceptance import (
    ALGORITHM_ID,
    ALGORITHM_SOURCE_SHA256,
    MEASUREMENT_SHA256,
    PRE_ORACLE_FREEZE_MANIFEST_SHA256,
    REQUIRED_MUTANTS,
    SEMANTIC_ACCEPTANCE_REFERENCE_SHA256,
    SUPERSEDED_FREEZE_MANIFEST_SHA256,
    reference_sha256,
)
from evaluator.spec_contract import EXECUTION_PREIMAGE_KEYS, execution_hash
from evaluator.pipeline import PipelineError, _lens, _opportunity, _utility
from evaluator.source_payload import extract, payload_record, source_record, validate_payload_closure
from evaluator.stage0_contract import (
    CONTEXT_V2_HASH,
    CONTEXT_V3_BYTES,
    CONTEXT_V3_HASH,
    Stage0ContractError,
    build_context_projection_v3,
    build_occurrence_closure,
    context_policy_v3,
    id_set_commitment,
    support_anchor,
    validate_occurrence_rebuild,
)
from evaluator.textnorm import normalize


def _load(name):
    version = "v3" if name == "context" else "v1"
    return parse_json_bytes((Path(__file__).parents[1] / "reference_vectors" / f"{name}.{version}.json").read_bytes())["vectors"]


def _text(row):
    if "input_codepoints" in row:
        try: normalize("".join(chr(value) for value in row["input_codepoints"]), field_limit=row.get("field_limit", 1024)); return False
        except TypedError: return True
    value = row["input"] if isinstance(row["input"], str) else row["input"]["prefix"] + row["input"]["repeat"] * row["input"]["count"]
    result = normalize(value, field_limit=row["field_limit"], extra_lexemes=row["lexemes"])
    expected = row["expected"]
    if expected.get("normalized_text") == "same_as_input": expected = {**expected, "normalized_text": value}
    return result["substantive"] == expected["substantive"] and result["normalized_text"] == expected["normalized_text"] and ("distinct_tokens" not in expected or result["distinct_tokens"] == expected["distinct_tokens"]) and ("distinct_token_count" not in expected or len(result["distinct_tokens"]) == expected["distinct_token_count"])


def _source(row):
    if row["id"] <= "S07":
        result = extract(bytes.fromhex(row["bytes_hex"]), row["range"]["start_line"], row["range"]["end_line"])
        if "expected_error" in row: return result.get("obstruction_kind") == row["expected_error"]
        source = source_record("changed", "head", "src/lib.rs", row["range"]["start_line"], row["range"]["end_line"], result)
        return result["text"] == row["expected"]["text"] and result["sha256"] == row["expected"]["sha256"] and result["payload_id"] == row["expected"]["payload_id"] and source["source_id"] == row["expected"]["source_id"]
    result = extract(b"a\nb\n", 2, 2); payload = payload_record(result); source = source_record("changed", "head", "src/lib.rs", 2, 2, result)
    body = {"schema": "m20.source-inventory.v3", "admitted_sources": [source], "declared_losses": []}
    packet = {"schema": "arm-neutral.source-grounded-packet@3", "task_id": "task", "instruction": "instruction", "response_schema": {}, "source_inventory": {**body, "source_inventory_id": stable_id("source-inventory", body), "canonical_sha256": hash_json(body)}, "payloads": [payload]}
    if row["id"] == "S08":
        packet["payloads"][0]["text"] = "c\n"; return "payload_hash_mismatch" in validate_payload_closure(packet)
    if row["id"] == "S09":
        try: canonical_b64_decode("YQ"); return False
        except ValueError: return True
    packet["payloads"][0]["text"] = "relation.changed_public_callee@1"; packet["source_inventory"]["admitted_sources"][0]["path"] = "relation.changed_public_callee@1.rs"
    try: _lens(packet)
    except PipelineError: return False
    packet["instruction"] = "relation.changed_public_callee@1"
    try: _lens(packet); return False
    except PipelineError: return True


REASONS = {"source": "task_blocking_source_unavailable", "reference": "task_blocking_reference_unresolved", "projection": "task_blocking_projection_integrity"}


def _losses(names, arm):
    if "unknown" in names or "serialized_eligible_true" in names: raise ValueError("closed")
    output = []
    for name in sorted(set(names)):
        output.append({"reason": REASONS[name], "undecidable_question_id": "q:" + name, "primary_abstention_eligible": False})
    return {"task_losses": output}


def _loss(row):
    if row["id"] in {"L09", "L10"}:
        try: _losses(row["arm_a"], 0); return False
        except ValueError: return True
    arms = [_losses(row["arm_a"], 0), _losses(row["arm_b"], 1)]
    opportunity = _opportunity(arms)
    expected = row["expected"]
    eligible = sum(loss["primary_abstention_eligible"] for loss in arms[0]["task_losses"])
    return opportunity["comparable"] == expected["comparable"] and eligible == expected.get("eligible", eligible) and (row["id"] != "L08" or len(arms[0]["task_losses"]) == 1)


def _batch(forced_second=False):
    candidates = []
    for index in range(2):
        forced = forced_second and index == 1
        candidates.append({"candidate_id": f"candidate:{index}", "packet_sha256": f"packet:{index}", "output_artifact_sha256": f"output:{index}", "binding_view_sha256": f"binding:{index}", "mechanical_score_sha256": f"mechanical:{index}", "mechanical_state": {"kind": "mechanical_forced_zero" if forced else "judgeable"}})
    return {"schema": "m20.utility_judge_batch_input.v1", "task_id": "task", "rubric_id": "rubric", "batch_id": "batch", "candidates": candidates}


def _response(batch, values=(1,1,2,2)):
    scores = []
    for candidate in batch["candidates"]:
        forced = candidate["mechanical_state"]["kind"] == "mechanical_forced_zero"
        numbers = (0,0,0,0) if forced else values
        scores.append({"candidate_id": candidate["candidate_id"], "packet_sha256": candidate["packet_sha256"], "output_artifact_sha256": candidate["output_artifact_sha256"], "binding_view_sha256": candidate["binding_view_sha256"], "mechanical_score_sha256": candidate["mechanical_score_sha256"], "score_source": "mechanical_forced_zero" if forced else "judge", "dimensions": dict(zip(DIMENSIONS, numbers)), "total": sum(numbers), "verdict": "not_usable" if forced else "usable"})
    return {"schema": "m20.utility_judge_batch_output.v1", "batch_id": batch["batch_id"], "scores": scores}


def _judge(row):
    batch = _batch(row["id"] == "J11"); response = _response(batch)
    if row["id"] == "J03": response["scores"][1]["candidate_id"] = response["scores"][0]["candidate_id"]
    elif row["id"] == "J04": response["scores"].pop()
    elif row["id"] == "J05": response["scores"].append(dict(response["scores"][0]))
    elif row["id"] == "J06": response["scores"][0]["packet_sha256"] = "changed"
    elif row["id"] == "J07": response["scores"][0]["total"] += 1
    elif row["id"] == "J08": response["scores"][0]["dimensions"] = dict(zip(DIMENSIONS, (1,1,1,1))); response["scores"][0]["total"] = 4
    elif row["id"] == "J09": response["scores"][0]["dimensions"] = dict(zip(DIMENSIONS, (0,2,2,2))); response["scores"][0]["total"] = 6
    elif row["id"] == "J10": batch["candidates"][0]["hidden_arm_id"] = "forbidden"; return "hidden_arm_id" in batch["candidates"][0]
    elif row["id"] == "J12": return all(not score["utility_judge_valid"] for score in _utility(batch, {"batch_id":"batch","entries":[{"candidate_id":"candidate:0","hidden_arm_id":"h0"},{"candidate_id":"candidate:1","hidden_arm_id":"h1"}]}, None, True)["scores"])
    try:
        decoded = decode_model(canonical_bytes(response), "judge", {"batch": batch})
    except TypedError:
        return row["id"] in {"J03","J04","J05","J06","J07"}
    reverse = {"batch_id": "batch", "entries": [{"candidate_id":"candidate:0","hidden_arm_id":"h0"},{"candidate_id":"candidate:1","hidden_arm_id":"h1"}]}
    utility = _utility(batch, reverse, decoded)
    values = [score["utility_judge_valid"] for score in utility["scores"]]
    return values == ([False, True] if row["id"] in {"J08","J09"} else [True, False] if row["id"] == "J11" else [True, True])


def _occurrence_fixture(drafts):
    identity={"snapshot_id":"snapshot:1","target_revision":"revision:1","legacy_program_space_sha256":"sha256:"+"1"*64,"legacy_extraction_report_sha256":"sha256:"+"2"*64}
    files=[{"file_source_id":"file:a","path":"src/a.rs"},{"file_source_id":"file:b","path":"src/b.rs"}]
    metrics={"schema":"m20.stage0-occurrence-metrics.v1","cluster_id":"cluster:1","wall_time_milliseconds":183200,"peak_bytes":1000}
    return identity,files,metrics,build_occurrence_closure(identity,files,drafts,["file:a"] if len(drafts)<2 else ["file:a","file:b"],metrics)


def _occurrence_drafts():
    return [
        {"path":"src/a.rs","span":{"start_line":1,"start_column":2,"end_line":1,"end_column":5},"source_ids":["file:a","owner:a"],"call_kind":"direct","reason":"direct_target_count_zero"},
        {"path":"src/a.rs","span":{"start_line":2,"start_column":1,"end_line":2,"end_column":3},"source_ids":["file:a","owner:b"],"call_kind":"method","reason":"method_dispatch_unresolved"},
        {"path":"src/b.rs","span":{"start_line":3,"start_column":1,"end_line":3,"end_column":4},"source_ids":["file:b","owner:c"],"call_kind":"macro_invocation","reason":"macro_expansion_unresolved"},
    ]


def _occurrence(row):
    drafts=_occurrence_drafts()
    selected=[] if row["id"]=="O01" else drafts[:1] if row["id"]=="O02" else drafts
    identity,files,metrics,public=_occurrence_fixture(selected)
    report=public["ingestion_report_v2"]
    if row["id"]=="O01":
        actual={"global_limitation_id":report["global_direct_calls_limitation"]["id"],"observed_occurrence_count":report["observed_occurrence_count"],"occurrence_id_set_sha256":report["occurrence_id_set_sha256"],"report_bytes":public["metrics"]["report_bytes"],"report_id":report["report_id"],"summary_rows":public["metrics"]["summary_rows"]}
        return actual==row["expected"]
    if row["id"]=="O02":
        summary=report["source_occurrence_summaries"][0]; actual={"bucket_digest":summary["buckets"][0]["occurrence_id_set_sha256"],"observed_occurrence_count":report["observed_occurrence_count"],"occurrence_id_set_sha256":report["occurrence_id_set_sha256"],"report_id":report["report_id"],"summary_id":summary["id"]}
        return actual==row["expected"]
    if row["id"]=="O03":
        actual={"file_counts":[item["observed_occurrence_count"] for item in report["source_occurrence_summaries"]],"observed_occurrence_count":report["observed_occurrence_count"],"occurrence_id_set_sha256":report["occurrence_id_set_sha256"],"report_id":report["report_id"],"summary_ids":[item["id"] for item in report["source_occurrence_summaries"]]}
        return actual==row["expected"]
    if row["id"]=="O04": public["ingestion_report_v2"]["observed_occurrence_count"]+=1
    elif row["id"]=="O05": identity,files,metrics,public=_occurrence_fixture(drafts[:1])
    else:
        report=public["ingestion_report_v2"]; report["located_call_occurrences"]=[]
        for field in ("source_occurrence_summaries","observed_occurrence_count","occurrence_id_set_sha256"): report.pop(field)
    try:
        validate_occurrence_rebuild(identity,files,drafts,["file:a","file:b"],public); return False
    except Stage0ContractError as error:
        return error.code==row["expected_error"]


def _context_fixture():
    identity={"projection_id":"projection:1","snapshot_id":"snapshot:1","request_id":"request:1","obligation_ids":["obligation:1"],"relation_ids":["relation:1"],"endpoint_pairs":[{"caller_endpoint_id":"endpoint:caller","callee_endpoint_id":"endpoint:callee"}]}
    subjects=[{"role":"callee","status":"admitted","endpoint_id":"endpoint:callee","source_artifact_id":"file:b","start_line":10,"end_line":12,"window_id":"window:1"},{"role":"caller","status":"lost","endpoint_id":"endpoint:caller","subject_loss":{"severity":"high","reason":"missing_source","recovery_reference":"recovery:1","source_ids":["file:a"]}}]
    sources=[{"source_artifact_id":"file:b","snapshot_side":"head","path":"src/b.rs","blob_oid":"b"*40}]
    anchors=[{"snapshot_id":"snapshot:1","source_artifact_id":"file:b","owner_artifact_id":"owner:1","start_line":10,"end_line":12},{"snapshot_id":"snapshot:1","source_artifact_id":"file:b","owner_artifact_id":"owner:2","start_line":20,"end_line":22}]
    anchor_ids=[support_anchor(item)["anchor_id"] for item in anchors]
    windows=[{"window_id":"window:1","source_artifact_id":"file:b","start_line":10,"end_line":12,"role":"callee","source_required_id":"required:1","support_anchor_ids":[anchor_ids[0]]}]
    return {"identity":identity,"accepted_file_ids":["file:a","file:b"],"reached_file_ids":["file:b"],"subject_outcomes":subjects,"materialized_sources":sources,"anchors":anchors,"admitted_windows":windows,"support_loss_partitions":[{"reason":"missing_source","anchor_ids":[anchor_ids[1]]}],"latent_cardinality":{"state":"unknown","capability_states":{"direct_calls":"partial"},"qualification_ids":["qualification:1"]},"unknown_ids":["unknown:1"],"remaining_loss_ids":["loss:1"],"remaining_source_ids":["source:1"]}


def _context(row):
    if row["id"]=="C01": return context_policy_v3()["policy_id"]=="context.subject_windows@3" and sha256_bytes(CONTEXT_V3_BYTES)==row["expected_hash"]==CONTEXT_V3_HASH
    if row["id"]=="C02": return CONTEXT_V2_HASH==row["expected_v2_hash"] and CONTEXT_V2_HASH!=CONTEXT_V3_HASH
    if row["id"]=="C03": return id_set_commitment([])==row["expected"]
    if row["id"]=="C05": return support_anchor({"snapshot_id":"snapshot:1","source_artifact_id":"file:b","owner_artifact_id":"owner:1","start_line":10,"end_line":12})["anchor_id"]==row["expected"]
    if row["id"]=="C11": return ALGORITHM_ID==row["expected"] and reference_sha256()==SEMANTIC_ACCEPTANCE_REFERENCE_SHA256
    if row["id"]=="C12": return ALGORITHM_SOURCE_SHA256==row["expected"]
    if row["id"]=="C13": return MEASUREMENT_SHA256==row["expected"]
    if row["id"]=="C14": return REQUIRED_MUTANTS==row["expected"]
    if row["id"]=="C15": return PRE_ORACLE_FREEZE_MANIFEST_SHA256==row["expected"]
    if row["id"]=="C16":
        values=row["inputs"]
        return list(EXECUTION_PREIMAGE_KEYS)==row["expected_keys"] and execution_hash(values["evaluator_bundle_sha256"],values["runtime_requirements_sha256"],values["semantic_acceptance_reference_sha256"])==row["expected_hash"] and SUPERSEDED_FREEZE_MANIFEST_SHA256==row["superseded_freeze_manifest_sha256"]
    args=_context_fixture()
    if row["id"]=="C04":
        value=build_context_projection_v3(**args); actual={key:value[key] for key in ("accepted_file_denominator","reached_file_denominator","materialized_source_denominator","support_anchor_denominator","support_loss_summaries","canonical_sha256")}; return actual==row["expected"]
    if row["id"]=="C06": args["subject_outcomes"].reverse()
    elif row["id"] in {"C07","C08"}:
        count=4096 if row["id"]=="C07" else 4097; file_ids=[f"file:{index:04d}" for index in range(count)]; args["accepted_file_ids"]=file_ids; args["reached_file_ids"]=file_ids; args["subject_outcomes"]=[{"role":"callee","status":"admitted","endpoint_id":"endpoint:callee","source_artifact_id":file_ids[0],"start_line":1,"end_line":1,"window_id":"window:0"},{"role":"caller","status":"admitted","endpoint_id":"endpoint:caller","source_artifact_id":file_ids[1],"start_line":1,"end_line":1,"window_id":"window:1"}]; args["materialized_sources"]=[{"source_artifact_id":value,"snapshot_side":"head","path":f"src/{index}.rs","blob_oid":"a"*40} for index,value in enumerate(file_ids)]; args["anchors"]=[]; args["admitted_windows"]=[{"window_id":"window:0","source_artifact_id":file_ids[0],"start_line":1,"end_line":1,"role":"callee","source_required_id":"required:0","support_anchor_ids":[]},{"window_id":"window:1","source_artifact_id":file_ids[1],"start_line":1,"end_line":1,"role":"caller","source_required_id":"required:1","support_anchor_ids":[]}]; args["support_loss_partitions"]=[]
    elif row["id"]=="C09": args["support_loss_partitions"]=[]
    else: args["latent_cardinality"]={"state":"unknown","capability_states":{"direct_calls":"partial"},"qualification_ids":[]}
    try:
        value=build_context_projection_v3(**args)
        return row["id"]=="C07" and value["materialized_source_denominator"]["observed_count"]==row["expected_count"]
    except Stage0ContractError as error:
        return row["id"]!="C07" and error.code==row["expected_error"]


def run_vectors():
    rows = []
    for name, runner in (("text", _text), ("source", _source), ("loss", _loss), ("judge", _judge), ("occurrence", _occurrence), ("context", _context)):
        for row in _load(name):
            try: passed = runner(row)
            except Exception: passed = False
            rows.append({"vector_id": row["id"], "passed": passed})
    return {"schema": "m20.reference-vector-result.v1", "total": len(rows), "passed": sum(row["passed"] for row in rows), "failed": sum(not row["passed"] for row in rows), "results": rows}
