"""Sole end-to-end constructor and scorer for one paired m20 unit."""; import hashlib; import unicodedata; from pathlib import Path; from .artifacts import ArtifactSink; from .canonical import canonical_bytes, hash_json, parse_json_bytes, sha256_bytes, stable_id; from .model_boundary import DIMENSIONS, DecodedModel, ModelResult, TypedError, decode_model; from .repository import GitRepository, PreflightError, production_rust, valid_path; from .source_payload import extract, payload_record, source_record, validate_payload_closure; from .stage0_contract import CONTEXT_POLICY_ID, CONTEXT_V2_HASH, CONTEXT_V3_HASH, PROFILE_HASH, Stage0ContractError, validate_context_projection_public, validate_occurrence_public; from .textnorm import normalize; MAX_SOURCE_BYTES = 65_536; CONTEXT_HASH = CONTEXT_V3_HASH; RULE_ID = "relation.changed_public_callee@1"; PROPERTY_ID = "rust.callee_contract_review@1"; PACKET_V2 = "arm-neutral.source-grounded-packet@2"; PACKET_V3 = "arm-neutral.source-grounded-packet@3"; _FIXTURE_EXECUTION_IDENTITY: str | None = None
class PipelineError(ValueError):
    def __init__(self, code: str, exit_code: int = 4): self.code, self.exit_code = code, exit_code; super().__init__(code)
def _data(name: str): return parse_json_bytes((Path(__file__).with_name("data") / name).read_bytes())
def _schema(name: str): return parse_json_bytes((Path(__file__).with_name("schemas") / name).read_bytes())
def _closed(value, fields, code="authenticated_input_invalid"):
    if not isinstance(value, dict) or set(value) != set(fields): raise PipelineError(code, 2)
    return value
def _unique(values, key=lambda x: x): return isinstance(values, list) and len(values) == len({key(item) for item in values})
def _bit(seed: str, identity: str) -> int: return hashlib.sha256(seed.encode() + b"\0" + identity.encode()).digest()[-1] & 1
def _launch(value: dict) -> dict:
    fields = {"schema", "experiment_id", "unit_id", "repository_root", "base_commit_oid", "head_commit_oid", "frozen_obligation_path", "frozen_obligation_sha256", "stage_manifest_path", "stage_manifest_sha256", "context_policy_id", "context_policy_sha256"}; _closed(value, fields)
    if value["schema"] != "m20.pipeline_launch.v1" or value["experiment_id"] != "m20-changed-public-callee-utility-v1": raise PipelineError("launch_schema_invalid", 2)
    if any(not isinstance(value[field], str) or not value[field] for field in fields - {"schema"}): raise PipelineError("launch_scalar_invalid", 2)
    if value["context_policy_id"] != CONTEXT_POLICY_ID or value["context_policy_sha256"] != CONTEXT_HASH: raise PipelineError("launch_context_policy_invalid", 2)
    return value
def _read_authenticated(path_text: str, expected_hash: str) -> tuple[dict, bytes]:
    path = Path(path_text)
    if path.is_symlink() or not path.is_absolute() or not path.is_file(): raise PipelineError("authenticated_path_invalid", 2)
    raw = path.read_bytes()
    if sha256_bytes(raw) != expected_hash: raise PipelineError("authenticated_hash_mismatch", 2)
    try: return parse_json_bytes(raw), raw
    except Exception as error: raise PipelineError("authenticated_json_invalid", 2) from error
def _stage(value: dict, launch: dict) -> dict:
    fields = {"schema", "experiment_id", "unit_id", "repository_root", "repository_allow_list", "base_commit_oid", "head_commit_oid", "frozen_obligation_sha256", "profile_id", "profile_sha256", "context_policy_id", "context_policy_sha256", "occurrence_closure", "public_seeds", "backend_adapters"}; _closed(value, fields)
    if value["schema"] != "m20.stage_manifest.v1" or value["profile_id"] != "rust.production.v1" or value["profile_sha256"] != PROFILE_HASH or value["context_policy_id"] != CONTEXT_POLICY_ID or value["context_policy_sha256"] != CONTEXT_HASH: raise PipelineError("stage_contract_invalid", 2)
    for field in ("experiment_id", "unit_id", "repository_root", "base_commit_oid", "head_commit_oid", "frozen_obligation_sha256", "context_policy_id", "context_policy_sha256"):
        if value[field] != launch[field]: raise PipelineError("stage_selector_mismatch", 2)
    _closed(value["public_seeds"], {"arm_order", "judge_permutation"}); _closed(value["backend_adapters"], {"reviewer", "judge"})
    if value["public_seeds"] != {"arm_order":"m20-arm-order-v1","judge_permutation":"m20-judge-permutation-v1"}: raise PipelineError("stage_seed_invalid",2)
    if not isinstance(value["repository_allow_list"], list) or not value["repository_allow_list"] or not all(isinstance(item,str) and item for item in value["repository_allow_list"]) or not _unique(value["repository_allow_list"]) or value["repository_root"] not in value["repository_allow_list"]: raise PipelineError("repository_allow_list_invalid", 2)
    try: validate_occurrence_public(value["occurrence_closure"])
    except Stage0ContractError as error: raise PipelineError(error.code, 2) from error
    return value
def _obligation(value: dict, unit_id: str) -> dict:
    fields = {"schema", "unit_id", "rule_id", "property_id", "obligation_ids", "relation_ids", "endpoint_pairs", "subject_windows", "sources", "required_references", "projection", "bounded_scope_manifest_id"}; _closed(value, fields)
    if value["schema"] != "m20.frozen_obligation.v1" or value["unit_id"] != unit_id or value["rule_id"] != RULE_ID or value["property_id"] != PROPERTY_ID: raise PipelineError("obligation_contract_invalid", 2)
    for field in ("obligation_ids", "relation_ids"):
        if not isinstance(value[field],list) or not value[field] or not all(isinstance(item,str) and item for item in value[field]) or not _unique(value[field]) or value[field] != sorted(value[field], key=lambda x: x.encode()): raise PipelineError("duplicate_required_id", 2)
    if not isinstance(value["bounded_scope_manifest_id"], str) or not value["bounded_scope_manifest_id"]: raise PipelineError("obligation_contract_invalid", 2)
    for collection, fields2, sort_key in (
        (value["endpoint_pairs"], {"caller_endpoint_id", "callee_endpoint_id"}, lambda x: (x["caller_endpoint_id"].encode(), x["callee_endpoint_id"].encode())),
        (value["subject_windows"], {"subject_id", "window_id", "role"}, lambda x: (x["subject_id"].encode(), x["window_id"].encode(), x["role"].encode())),
    ):
        if not isinstance(collection, list) or not collection: raise PipelineError("obligation_contract_invalid", 2)
        for item in collection:
            _closed(item, fields2, "obligation_contract_invalid")
            if not all(isinstance(item[field],str) and item[field] for field in fields2): raise PipelineError("obligation_contract_invalid",2)
        if collection != sorted(collection, key=sort_key) or not _unique(collection, lambda x: tuple(x[k] for k in sorted(fields2))): raise PipelineError("duplicate_required_id", 2)
    source_fields = {"required_id", "role", "snapshot_side", "path", "start_line", "end_line", "blob_oid"}
    if not isinstance(value["sources"], list) or not value["sources"]: raise PipelineError("obligation_contract_invalid", 2)
    for source in value["sources"]:
        _closed(source, source_fields, "obligation_contract_invalid")
        if not isinstance(source["required_id"], str) or not source["required_id"]: raise PipelineError("obligation_contract_invalid", 2)
        if source["role"] not in {"changed", "context", "support"} or source["snapshot_side"] not in {"base", "head"} or not valid_path(source["path"]) or not production_rust(source["path"]) or len(source["blob_oid"]) not in {40,64} or any(c not in "0123456789abcdef" for c in source["blob_oid"]): raise PipelineError("source_path_or_role_invalid", 2)
        if isinstance(source["start_line"], bool) or isinstance(source["end_line"], bool) or not isinstance(source["start_line"], int) or not isinstance(source["end_line"], int) or source["start_line"] < 1 or source["end_line"] < source["start_line"]: raise PipelineError("span_invalid", 2)
    if not _unique(value["sources"], lambda x: x["required_id"]) or not _unique(value["sources"], lambda x: (x["role"], x["snapshot_side"], x["path"], x["start_line"], x["end_line"], x["blob_oid"])): raise PipelineError("duplicate_required_id", 2)
    if not any(source["role"]=="changed" for source in value["sources"]): raise PipelineError("changed_source_missing",2)
    refs = value["required_references"]
    if not isinstance(refs, list) or not _unique(refs, lambda x: x.get("reference_id") if isinstance(x, dict) else None): raise PipelineError("duplicate_required_id", 2)
    for ref in refs:
        _closed(ref, {"reference_id", "snapshot_side", "path", "blob_oid"}, "obligation_contract_invalid")
        if not isinstance(ref["reference_id"], str) or not ref["reference_id"]: raise PipelineError("obligation_contract_invalid", 2)
        if ref["snapshot_side"] not in {"base", "head"} or not valid_path(ref["path"]) or len(ref["blob_oid"]) not in {40,64} or any(c not in "0123456789abcdef" for c in ref["blob_oid"]): raise PipelineError("source_path_invalid", 2)
    projection = value["projection"]
    try: validate_context_projection_public(projection)
    except Stage0ContractError as error: raise PipelineError(error.code, 2) from error
    if not isinstance(projection["projection_id"], str) or not projection["projection_id"] or not isinstance(projection["canonical_sha256"], str) or not isinstance(projection["source_required_ids"], list) or not all(isinstance(item, str) and item for item in projection["source_required_ids"]): raise PipelineError("obligation_contract_invalid", 2)
    if not _unique(projection["source_required_ids"]) or projection["source_required_ids"] != sorted(projection["source_required_ids"], key=lambda x: x.encode()) or set(projection["source_required_ids"]) != {x["required_id"] for x in value["sources"]}: raise PipelineError("projection_domain_invalid", 2)
    admitted_windows = projection["admitted_windows"]
    if set(projection["source_required_ids"]) != {window["source_required_id"] for window in admitted_windows}: raise PipelineError("projection_domain_invalid", 2)
    if {(item["role"], item["window_id"]) for item in value["subject_windows"]} != {(item["role"], item["window_id"]) for item in projection["subject_outcomes"] if item["status"] == "admitted"}: raise PipelineError("subject_projection_mismatch", 2)
    if any(item["role"] not in {"caller", "callee"} for item in value["subject_windows"]): raise PipelineError("obligation_contract_invalid", 2)
    return value
def _loss(reason: str, scope: str, task_id: str, registry_key: str, support_ids: list[str]) -> tuple[dict, dict]:
    registry_version = "m20-question-registry.v1"; question = stable_id("undecidable-question", {"registry_version": registry_version, "task_id": task_id, "registry_key": registry_key}); recovery = stable_id("recovery", {"task_id": task_id, "registry_key": registry_key, "support_ids": support_ids}); identity = {"reason": reason, "omitted_scope": scope, "recovery_reference": recovery, "undecidable_question_id": question, "support_ids": support_ids}; loss_id = stable_id("loss", identity)
    return ({"loss_id": loss_id, "reason": reason, "omitted_scope": scope, "recovery_reference": recovery, "primary_abstention_eligible": False, "undecidable_question_id": question},
            {"loss_id": loss_id, "task_id": task_id, "registry_key": registry_key, "qid": question, "support_ids": support_ids})


def _routine(task_id: str, hidden_arm_id: str, scope_id: str) -> dict: recovery = stable_id("recovery", {"task_id": task_id, "hidden_arm_id": hidden_arm_id, "bounded_scope_manifest_id": scope_id}); body = {"reason": "routine_scope_omission", "omitted_scope": "bounded_context", "recovery_reference": recovery, "undecidable_question_id": None}; return {"loss_id": stable_id("loss", body), **body, "primary_abstention_eligible": False}
def _arm(repository, trees, specs: list[dict], references: list[dict], projection: dict, task_id: str, hidden_arm_id: str, scope_id: str) -> dict:
    sources, payloads, unavailable = [], {}, []; seen_keys = set()
    for spec in specs:
        key = (spec["role"], spec["snapshot_side"], spec["path"], spec["start_line"], spec["end_line"], spec["blob_oid"])
        if key in seen_keys: raise PipelineError("duplicate_internal_source")
        seen_keys.add(key)
        try: raw = repository.blob_for(trees, spec["snapshot_side"], spec["path"], spec["blob_oid"]); result = extract(raw, spec["start_line"], spec["end_line"])
        except KeyError: result = {"status": "obstruction", "obstruction_kind": "blob_unavailable"}
        if result["status"] != "payload": unavailable.append(stable_id("source-obstruction", {"required_id": spec["required_id"], "kind": result["obstruction_kind"]})); continue
        source = source_record(spec["role"], spec["snapshot_side"], spec["path"], spec["start_line"], spec["end_line"], result); sources.append(source); payloads[result["payload_id"]] = payload_record(result)
    unresolved = []
    for ref in references:
        try: repository.blob_for(trees, ref["snapshot_side"], ref["path"], ref["blob_oid"])
        except KeyError: unresolved.append(stable_id("reference-obstruction", {"reference_id": ref["reference_id"], "path": ref["path"]}))
    projection_body = projection if "policy_id" in projection else {"projection_id": projection["projection_id"], "source_required_ids": projection["source_required_ids"]}; projection_bad = projection["canonical_sha256"] != hash_json({key:value for key,value in projection_body.items() if key != "canonical_sha256"}); support, hidden = [], []; registry = _data("question_registry.v1.json")["entries"]
    status = {"required_source_body": unavailable, "required_reference_target": unresolved,
              "required_projection_integrity": [stable_id("projection-obstruction", projection_body)] if projection_bad else []}
    for entry in registry:
        ids = sorted(status[entry["key"]], key=lambda x: x.encode())
        if ids: visible, private = _loss(entry["reason"], entry["scope"], task_id, entry["key"], ids); support.append(visible); hidden.append(private)
    return {"hidden_arm_id": hidden_arm_id, "sources": sorted(sources, key=lambda x: x["source_id"]), "payloads": sorted(payloads.values(), key=lambda x: x["payload_id"]), "task_losses": support, "hidden_loss_support": hidden, "routine_loss": _routine(task_id, hidden_arm_id, scope_id)}
def _opportunity(arms: list[dict]) -> dict:
    signatures = []
    for arm in arms: signatures.append(sorted([[item["reason"], item["undecidable_question_id"]] for item in arm["task_losses"]], key=lambda x: (x[0].encode(), x[1].encode())))
    comparable = signatures[0] == signatures[1]
    for arm in arms:
        for loss in arm["task_losses"]: loss["primary_abstention_eligible"] = comparable
    return {"schema": "m20.pair-opportunity.v1", "comparable": comparable, "signatures": signatures,
            "eligible_question_ids": [sorted([loss["undecidable_question_id"] for loss in arm["task_losses"]], key=lambda x: x.encode()) if comparable else [] for arm in arms]}


def _lens(packet: dict) -> None:
    forbidden_keys = {"rule_id", "property_id", "obligation_id", "relation_id", "endpoint_id", "caller_id", "callee_id", "subject_id"}; forbidden = tuple(unicodedata.normalize("NFKC", value).casefold() for value in (RULE_ID, PROPERTY_ID, "callee contract", "changed public callee", "subject-first", "selected by candidate d", "candidate d obligation"))
    def walk(value, pointer="", exempt=False):
        if isinstance(value, dict):
            for key, item in value.items():
                if key in forbidden_keys: raise PipelineError("reviewer_lens_leak")
                child_exempt = key == "text" and pointer.startswith("/payloads/") or key == "path" and pointer.startswith("/source_inventory/admitted_sources/"); walk(item, pointer + "/" + key, child_exempt)
        elif isinstance(value, list):
            for index, item in enumerate(value): walk(item, pointer + "/" + str(index), exempt)
        elif isinstance(value, str) and not exempt:
            folded = unicodedata.normalize("NFKC", value).casefold()
            if any(term in folded for term in forbidden) or pointer.endswith("/role") and folded in {"caller", "callee", "endpoint", "subject"}: raise PipelineError("reviewer_lens_leak")
    walk(packet)
def _packet_with_schema(arm: dict, task_id: str, schema: str) -> dict:
    losses = sorted([arm["routine_loss"], *arm["task_losses"]], key=lambda x: x["loss_id"]); body = {"schema": "m20.source-inventory.v3", "admitted_sources": arm["sources"], "declared_losses": losses}; inventory = {"schema": body["schema"], "source_inventory_id": stable_id("source-inventory", body), "admitted_sources": arm["sources"], "declared_losses": losses, "canonical_sha256": hash_json(body)}; instruction = (Path(__file__).with_name("data") / "common_instruction.txt").read_bytes()
    if not instruction.endswith(b"\n") or instruction.endswith(b"\n\n"): raise PipelineError("instruction_invalid")
    packet = {"schema": schema, "task_id": task_id, "instruction": instruction[:-1].decode("utf-8"), "response_schema": _schema("reviewer_output.v1.json"), "source_inventory": inventory, "payloads": arm["payloads"]}
    _lens(packet)
    if validate_payload_closure(packet): raise PipelineError("source_payload_closure_invalid")
    return packet
def _packet_v2(arm: dict, task_id: str) -> dict: return _packet_with_schema(arm, task_id, PACKET_V2)
def _packet(arm: dict, task_id: str) -> dict: return _packet_with_schema(arm, task_id, PACKET_V3)


def _union_specs(core: list[dict], additions: list[dict]) -> list[dict]:
    by_key = {}
    for spec in [*core, *additions]:
        key = (spec["snapshot_side"], spec["path"], spec["start_line"], spec["end_line"])
        previous = by_key.get(key)
        if previous is None: by_key[key] = dict(spec)
        elif previous["blob_oid"] != spec["blob_oid"]: raise PipelineError("shared_core_source_conflict", 2)
    return sorted(by_key.values(), key=lambda x: (x["path"].encode(), x["snapshot_side"] != "base", x["start_line"], x["end_line"], x["role"].encode()))


def _shared_core_specs(repository, trees: tuple[dict, dict], obligation: dict) -> list[dict]:
    baseline = repository.baseline_specs(trees)
    callee = next((item for item in obligation["projection"]["subject_outcomes"] if item["role"] == "callee" and item["status"] == "admitted"), None)
    if callee is None: raise PipelineError("shared_core_callee_invalid", 2)
    materialized = next((item for item in obligation["projection"]["materialized_sources"] if item["source_artifact_id"] == callee["source_artifact_id"]), None)
    if materialized is None or materialized["snapshot_side"] != "head": raise PipelineError("shared_core_callee_invalid", 2)
    body = {"role":"changed", "snapshot_side":"head", "path":materialized["path"], "start_line":callee["start_line"], "end_line":callee["end_line"], "blob_oid":materialized["blob_oid"]}
    return _union_specs(baseline, [{"required_id":stable_id("source-request", body), **body}])
def _codes(codes) -> list[str]:
    order = _data("failure_codes.v1.json")["codes"]; unknown = set(codes) - set(order)
    if unknown: raise PipelineError("unknown_failure_code")
    return [code for code in order if code in set(codes)]
def _observations(observations: list[dict], packet: dict) -> list[str]:
    by_id = {source["source_id"]: source for source in packet["source_inventory"]["admitted_sources"]}; codes, changed = [], False
    for observation in observations:
        source = by_id.get(observation["source_id"])
        if source is None: codes.append("foreign_source_id")
        elif observation["start_line"] < source["range"]["start_line"] or observation["end_line"] > source["range"]["end_line"]: codes.append("observation_range_invalid")
        elif source["role"] == "changed": changed = True
    if not changed: codes.append("changed_source_observation_missing")
    return codes
def _mechanical(packet: dict, arm: dict, opportunity: dict, result: ModelResult, decoded: DecodedModel | None, decode_codes: list[str], unit_id: str, binding_hash: str) -> dict:
    codes, traces = list(decode_codes), []
    if result.timeout: codes.append("timeout")
    if result.process_exit: codes.append("process_exit")
    if not result.raw_bytes: codes.append("empty_response")
    if result.client_truncation: codes.append("client_truncation")
    if result.provider_truncation: codes.append("provider_truncation")
    if result.tool_calls: codes.append("tool_violation")
    if decoded is not None:
        disposition = decoded.value["disposition"]
        if disposition["kind"] == "claim":
            for claim in disposition["claims"]:
                codes.extend(_observations(claim["observations"], packet))
                if claim["conclusion"] == "inconclusive": codes.append("inconclusive_disposition")
                fields = [(claim["summary"], 1024), *[(claim["mechanism"][name], 512) for name in ("trigger", "observed_behavior", "consequence")]]
                for text, limit in fields:
                    try:
                        trace = normalize(text, packet, decoded.value, limit); traces.append(hash_json(trace))
                        if not trace["substantive"]: codes.append("non_substantive_text")
                    except TypedError: codes.append("invalid_text_codepoint")
        else:
            abstention = disposition["abstention"]; codes.extend(_observations(abstention["observations"], packet)); losses = {loss["loss_id"]: loss for loss in packet["source_inventory"]["declared_losses"]}; cited = [losses.get(identifier) for identifier in abstention["basis_loss_ids"]]
            if any(loss is None for loss in cited): codes.append("foreign_loss_id")
            elif any(loss["reason"] == "routine_scope_omission" for loss in cited): codes.append("routine_loss_only" if len(cited) == 1 else "mixed_loss_class")
            elif not opportunity["comparable"]: codes.append("asymmetric_abstention_opportunity")
            elif any(not loss["primary_abstention_eligible"] or loss["reason"] != abstention["reason"] for loss in cited): codes.append("loss_question_mismatch")
            for text in (abstention["blocked_question"], abstention["needed_evidence"]):
                try:
                    trace = normalize(text, packet, decoded.value, 512); traces.append(hash_json(trace))
                    if not trace["substantive"]: codes.append("non_substantive_text")
                except TypedError: codes.append("invalid_text_codepoint")
    codes = _codes(codes); process = not any(code in {"timeout", "process_exit", "empty_response", "malformed_json", "schema_invalid", "task_id_mismatch", "inventory_hash_mismatch"} for code in codes); closure = not any(code in {"packet_hash_mismatch", "inventory_hash_mismatch", "payload_missing", "payload_hash_mismatch", "payload_length_mismatch", "source_payload_closure_invalid", "reviewer_lens_leak", "hidden_binding_mismatch", "pair_opportunity_mismatch", "foreign_source_id", "observation_range_invalid", "changed_source_observation_missing", "foreign_loss_id", "routine_loss_only", "mixed_loss_class", "asymmetric_abstention_opportunity", "loss_question_mismatch"} for code in codes); useful = not any(code in {"foreign_source_id", "observation_range_invalid", "changed_source_observation_missing", "foreign_loss_id", "routine_loss_only", "mixed_loss_class", "asymmetric_abstention_opportunity", "loss_question_mismatch", "inconclusive_disposition", "invalid_text_codepoint", "non_substantive_text"} for code in codes); policy = not any(code in {"policy_violation", "tool_violation", "client_truncation", "provider_truncation"} for code in codes); return {"schema": "m20.mechanical_score.v2", "task_id": packet["task_id"], "unit_id": unit_id, "hidden_arm_id": arm["hidden_arm_id"], "packet_sha256": hash_json(packet), "raw_sha256": sha256_bytes(result.raw_bytes), "parsed_sha256": decoded.parsed_sha256 if decoded else None, "binding_view_sha256": binding_hash, "pair_opportunity_sha256": hash_json(opportunity), "process_and_schema_valid": process, "closure_valid": closure, "mechanical_usefulness_valid": useful, "policy_valid": policy, "hashes_retained": True, "failure_codes": codes, "normalization_trace_ids": sorted(set(traces))}
def _binding(obligation: dict, arm: dict, task_id: str) -> dict: body = {"schema": "m20.judge-binding-view.v1", "task_id": task_id, "rule_id": obligation["rule_id"], "property_id": obligation["property_id"], "obligation_ids": obligation["obligation_ids"], "relation_ids": obligation["relation_ids"], "endpoint_pairs": obligation["endpoint_pairs"], "subject_windows": obligation["subject_windows"], "context_policy_id": CONTEXT_POLICY_ID, "context_policy_sha256": CONTEXT_HASH, "context_projection_sha256": obligation["projection"]["canonical_sha256"], "hidden_loss_support": sorted(arm["hidden_loss_support"], key=lambda x: x["loss_id"])}; return {**body, "binding_view_sha256": hash_json(body)}
def _candidate(packet: dict, view: dict, mechanical: dict, decoded: DecodedModel | None, arm_id: str) -> tuple[dict, dict]: judgeable = all(mechanical[field] for field in ("process_and_schema_valid", "closure_valid", "mechanical_usefulness_valid", "policy_valid", "hashes_retained")) and mechanical["failure_codes"] == [] and decoded is not None; state = {"kind": "judgeable", "parsed_output": decoded.value} if judgeable else {"kind": "mechanical_forced_zero", "parsed_output": None, "failure_codes": mechanical["failure_codes"]}; hashes = {"packet_sha256": hash_json(packet), "output_artifact_sha256": mechanical["raw_sha256"], "binding_view_sha256": view["binding_view_sha256"], "mechanical_score_sha256": hash_json(mechanical)}; candidate_id = stable_id("judge-candidate", {"task_id": packet["task_id"], **hashes, "mechanical_state": state["kind"]}); return ({"candidate_id": candidate_id, **hashes, "packet": packet, "binding_view": view, "mechanical_state": state}, {"candidate_id": candidate_id, "hidden_arm_id": arm_id})
def _batch(candidates: list[tuple[dict, dict]], task_id: str, seed: str) -> tuple[dict, dict]:
    ordered = sorted(candidates, key=lambda x: x[1]["hidden_arm_id"])
    if _bit(seed, task_id): ordered.reverse()
    public, reverse = [x[0] for x in ordered], [x[1] for x in ordered]; rubric = _data("utility_rubric.v1.json"); preimage = {"task_id": task_id, "rubric_id": rubric["schema"], "candidate_ids_in_order": [x["candidate_id"] for x in public], "packet_hashes_in_order": [x["packet_sha256"] for x in public], "output_artifact_hashes_in_order": [x["output_artifact_sha256"] for x in public], "binding_view_hashes_in_order": [x["binding_view_sha256"] for x in public], "mechanical_score_hashes_in_order": [x["mechanical_score_sha256"] for x in public]}; batch_id = stable_id("judge-batch", preimage); return ({"schema": "m20.utility_judge_batch_input.v1", "task_id": task_id, "rubric_id": rubric["schema"], "batch_id": batch_id, "candidates": public}, {"batch_id": batch_id, "entries": reverse})
def _utility(batch: dict, reverse: dict, decoded: DecodedModel | None, unavailable=False) -> dict:
    passed = {}
    if decoded is not None:
        for candidate, score in zip(batch["candidates"], decoded.value["scores"]): dimensions = [score["dimensions"][name] for name in DIMENSIONS]; passed[candidate["candidate_id"]] = candidate["mechanical_state"]["kind"] == "judgeable" and min(dimensions) >= 1 and score["total"] >= 6 and score["verdict"] == "usable"
    scores = []
    for entry in reverse["entries"]: valid = passed.get(entry["candidate_id"], False); code = [] if valid else ["judge_batch_unavailable" if unavailable else "judge_batch_invalid" if decoded is None else "utility_threshold_failed"]; scores.append({**entry, "utility_judge_valid": valid, "failure_codes": code})
    return {"schema": "m20.utility-result.v1", "batch_id": batch["batch_id"], "scores": scores}
def _primary(mechanicals: list[dict], utility: dict) -> dict:
    by_arm = {score["hidden_arm_id"]: score for score in utility["scores"]}; output = []
    for mechanical in sorted(mechanicals, key=lambda x: x["hidden_arm_id"]): judge = by_arm[mechanical["hidden_arm_id"]]; codes = _codes([*mechanical["failure_codes"], *judge["failure_codes"]]); booleans = [mechanical[field] for field in ("process_and_schema_valid", "closure_valid", "mechanical_usefulness_valid", "policy_valid", "hashes_retained")]; completed = all(booleans) and judge["utility_judge_valid"] and not codes; output.append({"schema": "m20.primary_score.v3", "task_id": mechanical["task_id"], "unit_id": mechanical["unit_id"], "hidden_arm_id": mechanical["hidden_arm_id"], "candidate_id": judge["candidate_id"], "judge_batch_id": utility["batch_id"], "mechanical_score_sha256": hash_json(mechanical), "packet_sha256": mechanical["packet_sha256"], "raw_sha256": mechanical["raw_sha256"], "parsed_sha256": mechanical["parsed_sha256"], "binding_view_sha256": mechanical["binding_view_sha256"], **{field: mechanical[field] for field in ("process_and_schema_valid", "closure_valid", "mechanical_usefulness_valid", "policy_valid", "hashes_retained")}, "utility_judge_valid": judge["utility_judge_valid"], "completed": completed, "failure_codes": codes})
    return {"schema": "m20.primary-pair.v1", "scores": output}
def _result(value) -> ModelResult:
    valid_usage=isinstance(value,ModelResult) and all(isinstance(item,tuple) and len(item)==2 and isinstance(item[0],str) and not isinstance(item[1],bool) and isinstance(item[1],int) and item[1]>=0 for item in value.usage)
    valid_flags=isinstance(value,ModelResult) and all(isinstance(item,bool) for item in (value.timeout,value.client_truncation,value.provider_truncation))
    if not isinstance(value, ModelResult) or not isinstance(value.raw_bytes, bytes) or isinstance(value.process_exit, bool) or not isinstance(value.process_exit, int) or not valid_flags or not valid_usage or not all(isinstance(item,str) for item in value.tool_calls): raise PipelineError("model_transport_result_invalid")
    return value
def _decode(result: ModelResult, kind: str, expected: dict) -> tuple[DecodedModel | None, list[str]]:
    if result.timeout or result.process_exit or not result.raw_bytes: return None, []
    try: return decode_model(result.raw_bytes, kind, expected), []
    except TypedError as error: return None, [item.code if item.code in _data("failure_codes.v1.json")["codes"] else "schema_invalid" for item in error.errors]
def _token_observation(result: ModelResult) -> dict:
    names = [name for name, _ in result.usage]; allowed = {"input_tokens", "output_tokens", "cache_tokens"}
    valid = bool(names) and len(names) == len(set(names)) and set(names) <= allowed
    if valid:
        counts = dict(result.usage)
        return {"authority":"observation_only","source":"backend_report","tokenizer":None,"input_tokens":counts.get("input_tokens"),"output_tokens":counts.get("output_tokens"),"cache_tokens":counts.get("cache_tokens"),"counting_scope":None,"unavailable_reason":None}
    reason = "backend_usage_absent" if not names else "backend_usage_invalid"
    return {"authority":"observation_only","source":"unavailable","tokenizer":None,"input_tokens":None,"output_tokens":None,"cache_tokens":None,"counting_scope":None,"unavailable_reason":reason}
def _execution(result: ModelResult, decoded: DecodedModel | None, adapter: str) -> dict: return {"schema": "m20.model-execution.v1", "adapter_id": adapter, "process_exit": result.process_exit, "timeout": result.timeout, "client_truncation": result.client_truncation, "provider_truncation": result.provider_truncation, "token_observation": _token_observation(result), "tool_calls": list(result.tool_calls), "raw_sha256": sha256_bytes(result.raw_bytes), "parsed_sha256": decoded.parsed_sha256 if decoded else None, "parsed_present": decoded is not None}
def _budget(unit_id: str, packets: list[dict], admitted_source_bytes: list[int] | None = None) -> dict:
    counts = admitted_source_bytes if admitted_source_bytes is not None else [sum(source["bytes"] for source in packet["source_inventory"]["admitted_sources"]) for packet in packets]
    arms = [{"slot":slot,"packet_sha256":hash_json(packet),"admitted_source_bytes":counts[slot],"within_ceiling":counts[slot] <= MAX_SOURCE_BYTES} for slot,packet in enumerate(packets)]
    body = {"schema":"m20.input_budget_audit.v1","unit_id":unit_id,"ceiling_kind":"admitted_source_utf8_bytes","ceiling_bytes":MAX_SOURCE_BYTES,"arms":arms,"pair_model_eligible":all(arm["within_ceiling"] for arm in arms)}
    return {**body,"budget_audit_id":stable_id("input-budget-audit",body)}
def RUN(frozen_launch: dict, model_transport, new_output_root: str | Path) -> dict:
    launch = _launch(frozen_launch); stage, _ = _read_authenticated(launch["stage_manifest_path"], launch["stage_manifest_sha256"]); obligation, _ = _read_authenticated(launch["frozen_obligation_path"], launch["frozen_obligation_sha256"]); stage, obligation = _stage(stage, launch), _obligation(obligation, launch["unit_id"]); descriptor = model_transport.descriptor() if hasattr(model_transport, "descriptor") else None
    if descriptor != stage["backend_adapters"]: raise PipelineError("backend_adapter_mismatch", 2)
    repository = GitRepository(launch["repository_root"], stage["repository_allow_list"]); trees = repository.snapshots(launch["base_commit_oid"], launch["head_commit_oid"]); core_specs = _shared_core_specs(repository, trees[:2], obligation)
    if not core_specs: raise PipelineError("baseline_empty", 2)
    treatment_specs = _union_specs(core_specs, obligation["sources"])
    task_id = stable_id("review-task", {"unit_id": launch["unit_id"], "obligation_ids": obligation["obligation_ids"], "rule_id": RULE_ID, "property_id": PROPERTY_ID}); arm_ids = [stable_id("hidden-arm", {"task_id": task_id, "construction_kind": kind}) for kind in ("baseline_diff", "subject_windows")]; core_projection = {"projection_id": stable_id("baseline-projection", {"unit_id": launch["unit_id"]}), "source_required_ids": sorted([x["required_id"] for x in core_specs], key=lambda x: x.encode())}; core_projection["canonical_sha256"] = hash_json(core_projection)
    arms = [_arm(repository, trees[:2], core_specs, [], core_projection, task_id, arm_ids[0], stable_id("scope", {"task_id": task_id, "arm": "baseline"})),
            _arm(repository, trees[:2], treatment_specs, obligation["required_references"], obligation["projection"], task_id, arm_ids[1], obligation["bounded_scope_manifest_id"])]
    opportunity = _opportunity(arms); packets = [_packet(arm, task_id) for arm in arms]; admitted_by_arm = {arm["hidden_arm_id"]:sum(source["bytes"] for source in arm["sources"]) for arm in arms}; slot_arms = arms if _bit(stage["public_seeds"]["arm_order"], launch["unit_id"]) == 0 else list(reversed(arms)); arm_packet = {arm["hidden_arm_id"]: packet for arm, packet in zip(arms, packets)}; slot_packets = [arm_packet[arm["hidden_arm_id"]] for arm in slot_arms]; budget = _budget(launch["unit_id"], slot_packets, [admitted_by_arm[arm["hidden_arm_id"]] for arm in slot_arms]); from .freeze import execution_identity, runtime_compatible
    if not runtime_compatible(): raise PipelineError("runtime_incompatible", 3)
    execution_sha = _FIXTURE_EXECUTION_IDENTITY or execution_identity(Path(__file__).resolve().parent)
    if _FIXTURE_EXECUTION_IDENTITY is None:
        registration=parse_json_bytes(Path(__file__).resolve().parent.parent.joinpath("preregistration.json").read_bytes()); expected=registration.get("arm_neutral_contracts",{}).get("freeze_hashes",{}).get("evaluator_execution_sha256")
        if expected != execution_sha: raise PipelineError("evaluator_freeze_mismatch",3)
    output_root = Path(new_output_root)
    if output_root.exists() or output_root.is_symlink(): raise PipelineError("output_root_exists", 2)
    sink = ArtifactSink(output_root); run_id = stable_id("m20-run", {"unit_id": launch["unit_id"], "task_id": task_id, "stage_manifest_sha256": launch["stage_manifest_sha256"]}); views = {arm["hidden_arm_id"]: _binding(obligation, arm, task_id) for arm in arms}; pair = {"schema": "m20.run_manifest.v1", "run_id": run_id, "task_id": task_id, "unit_id": launch["unit_id"], "context_policy_id": CONTEXT_POLICY_ID, "context_policy_sha256": CONTEXT_HASH, "context_projection_sha256": obligation["projection"]["canonical_sha256"], "slot_map": [{"slot": index, "hidden_arm_id": arm["hidden_arm_id"]} for index, arm in enumerate(slot_arms)], "pair_opportunity": opportunity, "arms": [{"hidden_arm_id": arm["hidden_arm_id"], "binding_view": views[arm["hidden_arm_id"]]} for arm in sorted(arms, key=lambda x: x["hidden_arm_id"])]}; sink.json("launch.json", launch, "launch"); sink.json("repository.json", {**repository.provenance(), "base_commit_oid": launch["base_commit_oid"], "head_commit_oid": launch["head_commit_oid"], "base_tree_oid": trees[2], "head_tree_oid": trees[3]}, "repository"); sink.json("obligation.json", obligation, "obligation"); sink.json("pair.json", pair, "run_manifest"); sink.json("budget.json",budget,"input_budget_audit")
    for slot,packet in enumerate(slot_packets): sink.json(f"slots/{slot}/packet.json",packet,"packet")
    if not budget["pair_model_eligible"]:
        seal=sink.finalize(execution_sha,launch["stage_manifest_sha256"],run_id,"model_ineligible")
        return {"schema":"m20.pipeline_result.v1","unit_id":launch["unit_id"],"pipeline_terminal_state":"model_ineligible","model_ineligible_reason":"admitted_source_byte_ceiling_exceeded","arm_results":{"A":None,"B":None},"model_call_count":0,"run_seal_id":seal["run_seal_id"]}
    mechanicals, decoded_by_arm = [], {}
    for slot, arm in enumerate(slot_arms):
        packet, view = arm_packet[arm["hidden_arm_id"]], views[arm["hidden_arm_id"]]; request = canonical_bytes(packet); sink.bytes(f"slots/{slot}/request.json", request, "reviewer_request"); result = _result(model_transport.review(request, slot, 900)); decoded, decode_codes = _decode(result, "reviewer", {"task_id": task_id, "source_inventory_id": packet["source_inventory"]["source_inventory_id"]}); mechanical = _mechanical(packet, arm, opportunity, result, decoded, decode_codes, launch["unit_id"], view["binding_view_sha256"]); sink.bytes(f"slots/{slot}/raw.bin", result.raw_bytes, "reviewer_raw"); sink.json(f"slots/{slot}/execution.json", _execution(result, decoded, stage["backend_adapters"]["reviewer"]), "reviewer_execution")
        if decoded: sink.json(f"slots/{slot}/parsed.json", decoded.value, "reviewer_parsed")
        sink.json(f"slots/{slot}/mechanical.json", mechanical, "mechanical"); mechanicals.append(mechanical); decoded_by_arm[arm["hidden_arm_id"]] = decoded
    mech_by_arm = {item["hidden_arm_id"]: item for item in mechanicals}; candidates = [_candidate(arm_packet[arm["hidden_arm_id"]], views[arm["hidden_arm_id"]], mech_by_arm[arm["hidden_arm_id"]], decoded_by_arm[arm["hidden_arm_id"]], arm["hidden_arm_id"]) for arm in arms]; batch, reverse = _batch(candidates, task_id, stage["public_seeds"]["judge_permutation"]); sink.json("judge/permutation.json", reverse, "permutation"); sink.json("judge/request.json", batch, "judge_request"); rubric_instruction = _data("utility_rubric.v1.json")["instruction"].encode("utf-8"); judge_result = _result(model_transport.judge(canonical_bytes(batch), rubric_instruction, 90)); judge_decoded, _ = _decode(judge_result, "judge", {"batch": batch}); utility = _utility(batch, reverse, judge_decoded, judge_result.timeout or judge_result.process_exit != 0 or not judge_result.raw_bytes); sink.bytes("judge/raw.bin", judge_result.raw_bytes, "judge_raw"); sink.json("judge/execution.json", _execution(judge_result, judge_decoded, stage["backend_adapters"]["judge"]), "judge_execution")
    if judge_decoded: sink.json("judge/parsed.json", judge_decoded.value, "judge_parsed")
    sink.json("judge/utility.json", utility, "utility"); primary = _primary(mechanicals, utility); sink.json("primary.json", primary, "primary"); seal = sink.finalize(execution_sha, launch["stage_manifest_sha256"], run_id, "sealed"); return {"schema": "m20.run-result.v1", "run_id": run_id, "pipeline_terminal_state": "sealed", "primary": primary, "seal": seal}
def _packet_audit(packet: dict) -> None:
    _closed(packet, {"schema", "task_id", "instruction", "response_schema", "source_inventory", "payloads"}, "packet_invalid"); instruction = (Path(__file__).with_name("data") / "common_instruction.txt").read_bytes()[:-1].decode("utf-8")
    if packet["schema"] != PACKET_V3 or packet["instruction"] != instruction or packet["response_schema"] != _schema("reviewer_output.v1.json"): raise PipelineError("packet_constants_invalid")
    inventory = _closed(packet["source_inventory"], {"schema", "source_inventory_id", "admitted_sources", "declared_losses", "canonical_sha256"}, "inventory_invalid")
    for source in inventory["admitted_sources"]: _closed(source, {"source_id", "role", "snapshot_side", "path", "range", "payload_id", "bytes", "sha256"}, "source_invalid")
    for loss in inventory["declared_losses"]: _closed(loss, {"loss_id", "reason", "omitted_scope", "recovery_reference", "primary_abstention_eligible", "undecidable_question_id"}, "loss_invalid")
    for payload in packet["payloads"]: _closed(payload, {"payload_id", "encoding", "media_type", "byte_length", "sha256", "text"}, "payload_invalid")
    if inventory["admitted_sources"] != sorted(inventory["admitted_sources"],key=lambda x:x["source_id"]) or not _unique(inventory["admitted_sources"],lambda x:x["source_id"]) or inventory["declared_losses"] != sorted(inventory["declared_losses"],key=lambda x:x["loss_id"]) or not _unique(inventory["declared_losses"],lambda x:x["loss_id"]) or packet["payloads"] != sorted(packet["payloads"],key=lambda x:x["payload_id"]) or not _unique(packet["payloads"],lambda x:x["payload_id"]): raise PipelineError("packet_order_or_duplicate_invalid")
    body = {"schema": "m20.source-inventory.v3", "admitted_sources": inventory["admitted_sources"], "declared_losses": inventory["declared_losses"]}
    if inventory["source_inventory_id"] != stable_id("source-inventory", body) or inventory["canonical_sha256"] != hash_json(body) or validate_payload_closure(packet): raise PipelineError("packet_audit_invalid")
    _lens(packet)
def _token_observation_audit(value) -> None:
    value=_closed(value,{"authority","source","tokenizer","input_tokens","output_tokens","cache_tokens","counting_scope","unavailable_reason"},"token_observation_invalid")
    if value["authority"]!="observation_only" or value["source"] not in {"backend_report","unavailable"}: raise PipelineError("token_observation_invalid")
    tokenizer=value["tokenizer"]
    if tokenizer is not None:
        _closed(tokenizer,{"name","revision","vocabulary_sha256","config_sha256"},"token_observation_invalid")
        if not all(isinstance(tokenizer[field],str) and tokenizer[field] for field in tokenizer): raise PipelineError("token_observation_invalid")
    counts=[value[field] for field in ("input_tokens","output_tokens","cache_tokens")]
    if any(item is not None and (isinstance(item,bool) or not isinstance(item,int) or item<0) for item in counts): raise PipelineError("token_observation_invalid")
    if value["counting_scope"] is not None and (not isinstance(value["counting_scope"],str) or not value["counting_scope"]): raise PipelineError("token_observation_invalid")
    if value["source"]=="unavailable":
        if tokenizer is not None or any(item is not None for item in counts) or value["counting_scope"] is not None or value["unavailable_reason"] not in {"backend_usage_absent","backend_usage_invalid"}: raise PipelineError("token_observation_invalid")
    elif value["unavailable_reason"] is not None or tokenizer is None and all(item is None for item in counts) and value["counting_scope"] is None: raise PipelineError("token_observation_invalid")
def audit_records(records: dict[str, dict], raws: dict[str, bytes], seal: dict) -> None:
    required = {"launch.json", "repository.json", "obligation.json", "pair.json", "budget.json", "slots/0/packet.json", "slots/0/request.json", "slots/0/execution.json", "slots/0/mechanical.json", "slots/1/packet.json", "slots/1/request.json", "slots/1/execution.json", "slots/1/mechanical.json", "judge/permutation.json", "judge/request.json", "judge/execution.json", "judge/utility.json", "primary.json"}
    ineligible = {"launch.json", "repository.json", "obligation.json", "pair.json", "budget.json", "slots/0/packet.json", "slots/1/packet.json"}
    optional={"slots/0/parsed.json","slots/1/parsed.json","judge/parsed.json"}
    if seal["pipeline_terminal_state"]=="model_ineligible":
        if set(records)!=ineligible or raws: raise PipelineError("artifact_set_invalid")
    elif seal["pipeline_terminal_state"]=="sealed":
        if not required.issubset(records) or set(records)-required-optional or set(raws) != {"slots/0/raw.bin", "slots/1/raw.bin", "judge/raw.bin"}: raise PipelineError("artifact_set_invalid")
    else: raise PipelineError("terminal_state_invalid")
    launch=_launch(records["launch.json"])
    repository=_closed(records["repository.json"],{"schema","repository_root","object_format","git_executable","git_executable_sha256","base_commit_oid","head_commit_oid","base_tree_oid","head_tree_oid"},"repository_invalid")
    if repository["schema"]!="m20.repository-provenance.v1" or repository["repository_root"]!=launch["repository_root"] or repository["base_commit_oid"]!=launch["base_commit_oid"] or repository["head_commit_oid"]!=launch["head_commit_oid"]: raise PipelineError("repository_invalid")
    if sha256_bytes(canonical_bytes(records["obligation.json"])) != launch["frozen_obligation_sha256"] or seal["stage_manifest_sha256"] != launch["stage_manifest_sha256"]: raise PipelineError("authenticated_artifact_mismatch")
    obligation=_obligation(records["obligation.json"],launch["unit_id"]); pair = records["pair.json"]; _closed(pair, {"schema", "run_id", "task_id", "unit_id", "context_policy_id", "context_policy_sha256", "context_projection_sha256", "slot_map", "pair_opportunity", "arms"}, "pair_invalid"); task_id=stable_id("review-task",{"unit_id":launch["unit_id"],"obligation_ids":obligation["obligation_ids"],"rule_id":RULE_ID,"property_id":PROPERTY_ID}); run_id=stable_id("m20-run",{"unit_id":launch["unit_id"],"task_id":task_id,"stage_manifest_sha256":launch["stage_manifest_sha256"]})
    for item in pair["slot_map"]: _closed(item,{"slot","hidden_arm_id"},"slot_map_invalid")
    pair_views={}
    for item in pair["arms"]:
        _closed(item,{"hidden_arm_id","binding_view"},"pair_arm_invalid"); view=_closed(item["binding_view"],{"schema","task_id","rule_id","property_id","obligation_ids","relation_ids","endpoint_pairs","subject_windows","context_policy_id","context_policy_sha256","context_projection_sha256","hidden_loss_support","binding_view_sha256"},"binding_view_invalid"); body={key:view[key] for key in view if key!="binding_view_sha256"}
        if view["binding_view_sha256"]!=hash_json(body) or item["hidden_arm_id"] in pair_views: raise PipelineError("binding_view_invalid")
        pair_views[item["hidden_arm_id"]]=view
    if pair["run_id"] != seal["run_id"] or pair["run_id"] != run_id or pair["task_id"] != task_id or pair["unit_id"]!=launch["unit_id"] or pair["context_policy_id"] != CONTEXT_POLICY_ID or pair["context_policy_sha256"] != CONTEXT_HASH or pair["context_projection_sha256"] != obligation["projection"]["canonical_sha256"] or [item["slot"] for item in pair["slot_map"]] != [0, 1] or len({item["hidden_arm_id"] for item in pair["slot_map"]}) != 2 or set(pair_views)!={item["hidden_arm_id"] for item in pair["slot_map"]}: raise PipelineError("pair_invalid")
    packets=[records[f"slots/{slot}/packet.json"] for slot in range(2)]
    for packet in packets:_packet_audit(packet)
    if records["budget.json"] != _budget(launch["unit_id"],packets): raise PipelineError("budget_audit_invalid")
    if seal["pipeline_terminal_state"]=="model_ineligible":
        if records["budget.json"]["pair_model_eligible"]: raise PipelineError("terminal_state_invalid")
        return
    if not records["budget.json"]["pair_model_eligible"]: raise PipelineError("terminal_state_invalid")
    mechanicals = []
    for slot in range(2):
        packet, execution, mechanical = records[f"slots/{slot}/packet.json"], records[f"slots/{slot}/execution.json"], records[f"slots/{slot}/mechanical.json"]; _closed(execution,{"schema","adapter_id","process_exit","timeout","client_truncation","provider_truncation","token_observation","tool_calls","raw_sha256","parsed_sha256","parsed_present"},"execution_invalid"); _token_observation_audit(execution["token_observation"]); _closed(mechanical,{"schema","task_id","unit_id","hidden_arm_id","packet_sha256","raw_sha256","parsed_sha256","binding_view_sha256","pair_opportunity_sha256","process_and_schema_valid","closure_valid","mechanical_usefulness_valid","policy_valid","hashes_retained","failure_codes","normalization_trace_ids"},"mechanical_invalid")
        if records[f"slots/{slot}/request.json"] != packet: raise PipelineError("request_packet_mismatch")
        raw = raws[f"slots/{slot}/raw.bin"]
        if execution["raw_sha256"] != sha256_bytes(raw) or mechanical["raw_sha256"] != sha256_bytes(raw) or mechanical["packet_sha256"] != hash_json(packet) or mechanical["hidden_arm_id"]!=pair["slot_map"][slot]["hidden_arm_id"] or mechanical["binding_view_sha256"]!=pair_views[mechanical["hidden_arm_id"]]["binding_view_sha256"]: raise PipelineError("raw_hash_mismatch")
        parsed_path = f"slots/{slot}/parsed.json"; decoded = None
        if execution["parsed_present"]:
            if parsed_path not in records: raise PipelineError("parsed_missing")
            decoded = decode_model(raw, "reviewer", {"task_id": packet["task_id"], "source_inventory_id": packet["source_inventory"]["source_inventory_id"]})
            if decoded.value != records[parsed_path] or decoded.parsed_sha256 != execution["parsed_sha256"] or decoded.parsed_sha256 != mechanical["parsed_sha256"]: raise PipelineError("parsed_hash_mismatch")
        elif parsed_path in records: raise PipelineError("parsed_unexpected")
        mechanicals.append(mechanical)
    signatures=[]
    for slot in range(2): losses=records[f"slots/{slot}/packet.json"]["source_inventory"]["declared_losses"]; signatures.append(sorted([[loss["reason"],loss["undecidable_question_id"]] for loss in losses if loss["reason"]!="routine_scope_omission"],key=lambda x:(x[0].encode(),x[1].encode())))
    opportunity=_closed(pair["pair_opportunity"],{"schema","comparable","signatures","eligible_question_ids"},"pair_opportunity_invalid"); comparable=opportunity["signatures"][0]==opportunity["signatures"][1]; eligible=[[item[1] for item in signature] for signature in opportunity["signatures"]] if comparable else [[],[]]
    if opportunity["schema"]!="m20.pair-opportunity.v1" or opportunity["comparable"] != comparable or sorted(opportunity["signatures"]) != sorted(signatures) or opportunity["eligible_question_ids"]!=eligible: raise PipelineError("pair_opportunity_invalid")
    batch, reverse = records["judge/request.json"], records["judge/permutation.json"]; _closed(batch,{"schema","task_id","rubric_id","batch_id","candidates"},"judge_batch_invalid"); _closed(reverse,{"batch_id","entries"},"permutation_invalid")
    for entry in reverse["entries"]: _closed(entry,{"candidate_id","hidden_arm_id"},"permutation_invalid")
    if reverse["batch_id"] != batch["batch_id"] or [entry["candidate_id"] for entry in reverse["entries"]] != [candidate["candidate_id"] for candidate in batch["candidates"]] or {entry["hidden_arm_id"] for entry in reverse["entries"]}!=set(pair_views): raise PipelineError("permutation_invalid")
    mech_hashes = {hash_json(item): item for item in mechanicals}
    for candidate,reverse_entry in zip(batch["candidates"],reverse["entries"]):
        _closed(candidate,{"candidate_id","packet_sha256","output_artifact_sha256","binding_view_sha256","mechanical_score_sha256","packet","binding_view","mechanical_state"},"candidate_invalid"); _packet_audit(candidate["packet"]); view = candidate["binding_view"]; _closed(view,{"schema","task_id","rule_id","property_id","obligation_ids","relation_ids","endpoint_pairs","subject_windows","context_policy_id","context_policy_sha256","context_projection_sha256","hidden_loss_support","binding_view_sha256"},"binding_view_invalid"); body = {key: view[key] for key in view if key != "binding_view_sha256"}
        if candidate["packet_sha256"] != hash_json(candidate["packet"]) or candidate["binding_view_sha256"] != hash_json(body) or view["binding_view_sha256"] != hash_json(body) or view!=pair_views[reverse_entry["hidden_arm_id"]] or candidate["mechanical_score_sha256"] not in mech_hashes: raise PipelineError("candidate_hash_invalid")
        mechanical = mech_hashes[candidate["mechanical_score_sha256"]]; should_judge = all(mechanical[field] for field in ("process_and_schema_valid", "closure_valid", "mechanical_usefulness_valid", "policy_valid", "hashes_retained")) and mechanical["failure_codes"] == [] and mechanical["parsed_sha256"] is not None
        if (candidate["mechanical_state"]["kind"] == "judgeable") != should_judge: raise PipelineError("mechanical_state_invalid")
        preimage = {"task_id": batch["task_id"], "packet_sha256": candidate["packet_sha256"], "output_artifact_sha256": candidate["output_artifact_sha256"], "binding_view_sha256": candidate["binding_view_sha256"], "mechanical_score_sha256": candidate["mechanical_score_sha256"], "mechanical_state": candidate["mechanical_state"]["kind"]}
        if candidate["candidate_id"] != stable_id("judge-candidate", preimage): raise PipelineError("candidate_id_invalid")
    judge_execution, judge_raw = records["judge/execution.json"], raws["judge/raw.bin"]
    _closed(judge_execution,{"schema","adapter_id","process_exit","timeout","client_truncation","provider_truncation","token_observation","tool_calls","raw_sha256","parsed_sha256","parsed_present"},"execution_invalid"); _token_observation_audit(judge_execution["token_observation"])
    if judge_execution["raw_sha256"] != sha256_bytes(judge_raw): raise PipelineError("judge_raw_hash_invalid")
    decoded = None
    if judge_execution["parsed_present"]:
        decoded = decode_model(judge_raw, "judge", {"batch": batch})
        if records.get("judge/parsed.json") != decoded.value or judge_execution["parsed_sha256"] != decoded.parsed_sha256: raise PipelineError("judge_parsed_invalid")
    expected_utility = _utility(batch, reverse, decoded, judge_execution["timeout"] or judge_execution["process_exit"] != 0 or not judge_raw)
    if records["judge/utility.json"] != expected_utility or records["primary.json"] != _primary(mechanicals, expected_utility): raise PipelineError("score_replay_invalid")
