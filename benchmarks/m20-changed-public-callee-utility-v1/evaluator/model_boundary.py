"""The only semantic decoder for untrusted reviewer and judge bytes."""; from dataclasses import dataclass; from typing import Any, Iterable; from .canonical import MAX_INTEGER, hash_json, parse_json_bytes, sha256_bytes; MAX_MODEL_BYTES = 1_048_576; MAX_MODEL_DEPTH = 32; DIMENSIONS = ("source_specificity", "hidden_task_relevance", "mechanism_or_blocker_specificity", "audit_actionability")
@dataclass(frozen=True)
class ValidationError:
    code: str; json_pointer: str; detail_id: str
    def as_dict(self) -> dict: return {"code": self.code, "json_pointer": self.json_pointer, "detail_id": self.detail_id}
class TypedError(ValueError):
    def __init__(self, errors: Iterable[ValidationError]): self.errors = tuple(sorted(errors, key=lambda e: (e.json_pointer.encode(), e.code.encode(), e.detail_id.encode()))); super().__init__("typed_validation_failed")
@dataclass(frozen=True)
class DecodedModel: value: dict; raw_sha256: str; parsed_sha256: str
@dataclass(frozen=True)
class ModelResult: raw_bytes: bytes; process_exit: int = 0; timeout: bool = False; client_truncation: bool = False; provider_truncation: bool = False; usage: tuple[tuple[str, int], ...] = (); tool_calls: tuple[str, ...] = ()
def reject(code: str, pointer: str = "", detail: str = "invalid") -> None: raise TypedError((ValidationError(code, pointer, detail),))
def _closed(value: Any, fields: set[str], pointer: str) -> dict:
    if not isinstance(value, dict) or set(value) != fields: reject("schema_invalid", pointer, "closed_object")
    return value
def _string(value: Any, pointer: str, minimum: int = 1, maximum: int | None = None) -> str:
    if not isinstance(value, str) or len(value) < minimum or maximum is not None and len(value) > maximum: reject("schema_invalid", pointer, "string")
    if "\x00" in value or any(0xD800 <= ord(c) <= 0xDFFF for c in value): reject("invalid_text_codepoint", pointer, "string")
    return value
def _integer(value: Any, pointer: str, minimum: int = 0, maximum: int = MAX_INTEGER) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum: reject("schema_invalid", pointer, "integer")
    return value
def _sorted_unique(values: list, key, pointer: str) -> None:
    if values != sorted(values, key=key) or len({key(v) for v in values}) != len(values): reject("schema_invalid", pointer, "sorted_unique")
def _observations(value: Any, pointer: str) -> None:
    if not isinstance(value, list) or not 1 <= len(value) <= 8: reject("schema_invalid", pointer, "observations")
    for index, observation in enumerate(value):
        p = f"{pointer}/{index}"; _closed(observation, {"source_id", "start_line", "end_line"}, p); _string(observation["source_id"], p + "/source_id"); _integer(observation["start_line"], p + "/start_line", 1); _integer(observation["end_line"], p + "/end_line", 1)
        if observation["end_line"] < observation["start_line"]: reject("schema_invalid", p, "range")
    _sorted_unique(value, lambda x: (x["source_id"].encode(), x["start_line"], x["end_line"]), pointer)
def _reviewer(value: Any, task_id: str, inventory_id: str) -> dict:
    top = _closed(value, {"schema", "task_id", "source_inventory_id", "disposition"}, "")
    if top["schema"] != "arm-neutral.source-grounded-disposition@1": reject("schema_invalid", "/schema", "reviewer")
    _string(top["task_id"], "/task_id"); _string(top["source_inventory_id"], "/source_inventory_id")
    if top["task_id"] != task_id: reject("task_id_mismatch", "/task_id", "expected")
    if top["source_inventory_id"] != inventory_id: reject("inventory_hash_mismatch", "/source_inventory_id", "expected")
    disposition = _closed(top["disposition"], {"kind", "claims", "abstention"}, "/disposition")
    if disposition["kind"] == "claim":
        if disposition["abstention"] is not None or not isinstance(disposition["claims"], list) or not 1 <= len(disposition["claims"]) <= 3: reject("schema_invalid", "/disposition", "claim_union")
        for index, claim in enumerate(disposition["claims"]):
            p = f"/disposition/claims/{index}"; _closed(claim, {"conclusion", "summary", "observations", "mechanism"}, p)
            if claim["conclusion"] not in {"issue_present", "issue_absent", "inconclusive"}: reject("schema_invalid", p + "/conclusion", "enum")
            _string(claim["summary"], p + "/summary", 1, 1024); _observations(claim["observations"], p + "/observations"); mechanism = _closed(claim["mechanism"], {"trigger", "observed_behavior", "consequence"}, p + "/mechanism")
            for field in ("trigger", "observed_behavior", "consequence"): _string(mechanism[field], p + "/mechanism/" + field, 1, 512)
    elif disposition["kind"] == "abstention":
        if disposition["claims"] != [] or not isinstance(disposition["abstention"], dict): reject("schema_invalid", "/disposition", "abstention_union")
        abstention = _closed(disposition["abstention"], {"reason", "basis_loss_ids", "observations", "blocked_question", "needed_evidence"}, "/disposition/abstention")
        if abstention["reason"] not in {"task_blocking_source_unavailable", "task_blocking_reference_unresolved", "task_blocking_projection_integrity"}: reject("schema_invalid", "/disposition/abstention/reason", "enum")
        losses = abstention["basis_loss_ids"]
        if not isinstance(losses, list) or not 1 <= len(losses) <= 3: reject("schema_invalid", "/disposition/abstention/basis_loss_ids", "array")
        for loss in losses: _string(loss, "/disposition/abstention/basis_loss_ids")
        _sorted_unique(losses, lambda x: x.encode(), "/disposition/abstention/basis_loss_ids"); _observations(abstention["observations"], "/disposition/abstention/observations"); _string(abstention["blocked_question"], "/disposition/abstention/blocked_question", 1, 512); _string(abstention["needed_evidence"], "/disposition/abstention/needed_evidence", 1, 512)
    else: reject("schema_invalid", "/disposition/kind", "union")
    return top
def _judge(value: Any, batch: dict) -> dict:
    top = _closed(value, {"schema", "batch_id", "scores"}, "")
    if top["schema"] != "m20.utility_judge_batch_output.v1" or top["batch_id"] != batch["batch_id"]: reject("judge_candidate_mismatch", "/batch_id", "expected")
    scores = top["scores"]
    if not isinstance(scores, list) or len(scores) != 2: reject("judge_batch_invalid", "/scores", "arity")
    candidates = batch["candidates"]
    if [score.get("candidate_id") if isinstance(score, dict) else None for score in scores] != [candidate["candidate_id"] for candidate in candidates]: reject("judge_candidate_mismatch", "/scores", "order")
    for index, (score, candidate) in enumerate(zip(scores, candidates)):
        p = f"/scores/{index}"; _closed(score, {"candidate_id", "packet_sha256", "output_artifact_sha256", "binding_view_sha256", "mechanical_score_sha256", "score_source", "dimensions", "total", "verdict"}, p)
        for field in ("candidate_id", "packet_sha256", "output_artifact_sha256", "binding_view_sha256", "mechanical_score_sha256"):
            if score[field] != candidate[field]: reject("judge_candidate_mismatch", p + "/" + field, "echo")
        dimensions = _closed(score["dimensions"], set(DIMENSIONS), p + "/dimensions"); values = [_integer(dimensions[name], p + "/dimensions/" + name, 0, 2) for name in DIMENSIONS]; total = _integer(score["total"], p + "/total", 0, 8)
        if total != sum(values) or score["verdict"] not in {"usable", "not_usable"}: reject("judge_batch_invalid", p, "score")
        forced = candidate["mechanical_state"]["kind"] == "mechanical_forced_zero"
        if forced:
            if score["score_source"] != "mechanical_forced_zero" or total != 0 or score["verdict"] != "not_usable": reject("judge_batch_invalid", p, "forced_zero")
        elif score["score_source"] != "judge": reject("judge_batch_invalid", p + "/score_source", "judgeable")
    return top
def _depth(raw: bytes) -> None:
    depth = 0; in_string = escaped = False
    for byte in raw:
        if in_string:
            if escaped: escaped = False
            elif byte == 0x5C: escaped = True
            elif byte == 0x22: in_string = False
        elif byte == 0x22: in_string = True
        elif byte in (0x7B, 0x5B):
            depth += 1
            if depth > MAX_MODEL_DEPTH: reject("schema_invalid", "", "depth")
        elif byte in (0x7D, 0x5D):
            depth -= 1
            if depth < 0: reject("malformed_json", "", "framing")
def decode_model(raw_bytes: bytes, expected_kind: str, expected: dict) -> DecodedModel:
    raw_hash = sha256_bytes(raw_bytes) if isinstance(raw_bytes, bytes) else "sha256:" + "0" * 64
    try:
        if not isinstance(raw_bytes, bytes): reject("schema_invalid", "", "raw_bytes")
        if len(raw_bytes) > MAX_MODEL_BYTES: reject("schema_invalid", "", "byte_limit")
        _depth(raw_bytes); value = parse_json_bytes(raw_bytes)
        if expected_kind == "reviewer": typed = _reviewer(value, expected["task_id"], expected["source_inventory_id"])
        elif expected_kind == "judge": typed = _judge(value, expected["batch"])
        else: reject("schema_invalid", "", "expected_kind")
        return DecodedModel(typed, raw_hash, hash_json(typed))
    except TypedError: raise
    except Exception as error: code = "malformed_json" if error.__class__.__name__ in {"CanonicalError", "JSONDecodeError", "UnicodeDecodeError"} else "schema_invalid"; raise TypedError((ValidationError(code, "", "model_decode"),)) from error
