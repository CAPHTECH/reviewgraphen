"""A/B request construction with product work charged inside the harness."""

from __future__ import annotations

import json
import hashlib
import hmac
import re
import secrets
import time
import urllib.request
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path

from .budget import LIMITS, enforce
from .canonical import canonical_bytes, sha256, stable_id
from .packet import build
from .product import ProductError, base_tree_workspace


MODEL = "Qwen3.8-27B-MLX-4bit"
TOOLS = [
    {"type":"function","function":{"name":"list_paths","description":"List base-tree paths under a prefix.","parameters":{"type":"object","properties":{"prefix":{"type":"string"}},"required":["prefix"],"additionalProperties":False}}},
    {"type":"function","function":{"name":"search_text_or_identifier","description":"Search bounded base source.","parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":False}}},
    {"type":"function","function":{"name":"read_range","description":"Read a bounded base-tree range.","parameters":{"type":"object","properties":{"path":{"type":"string"},"start":{"type":"integer"},"end":{"type":"integer"}},"required":["path","start","end"],"additionalProperties":False}}},
]
TOKENIZER = {"kind":"deterministic_approximation","algorithm":"utf8_bytes_ceiling_div_4.v1","frozen":True}
PUBLIC_TASK_KEYS = {
    "task_id", "snapshot_id", "task_kind", "title", "subject_symbol_id",
    "subject_path", "subject_start_line", "subject_end_line", "failing_identifier",
}
FORBIDDEN_PACKET_KEYS = {
    "fix_oid", "oracle", "oracle_id", "history", "realized_fix_commit_oid",
    "arm", "arm_label", "expected_paths", "evaluator_rationale", "other_arm_output",
}
class HarnessError(ValueError):
    def __init__(self, code: str):
        self.record = {"schema":"m21.typed_failure.v1", "code":code}
        super().__init__(code)


@dataclass(frozen=True)
class TreatmentSource:
    """Evaluator-only base input.  It has no field for a fix or history."""

    repository: Path
    repository_id: str
    base_oid: str

    def __post_init__(self):
        if not isinstance(self.repository, Path) or not self.repository_id or not self.base_oid:
            raise HarnessError("treatment_source_invalid")


def _validate_measurement_values(values: dict) -> None:
    if any(type(value) is not int or value < 0 for value in values.values()):
        raise HarnessError("harness_measurement_invalid")
    if values["packet_construction_ns"] > values["elapsed_ns"]:
        raise HarnessError("harness_measurement_invalid")
    if values["model_elapsed_ns"] > values["elapsed_ns"]:
        raise HarnessError("harness_measurement_invalid")
    if values["product_ingest_elapsed_ns"] + values["product_projection_elapsed_ns"] > values["packet_construction_ns"]:
        raise HarnessError("harness_measurement_invalid")
    enforce("input_tokens", values["input_tokens"])
    enforce("output_tokens", values["output_tokens"])
    enforce("tool_calls", values["tool_calls"])
    if values["elapsed_ns"] > LIMITS["wall_seconds"] * 1_000_000_000:
        raise HarnessError("harness_measurement_wall_exceeded")


@contextmanager
def arm_workspace(source: TreatmentSource, arm: str):
    """Give either arm the identical, future-object-free base checkout."""
    if arm not in {"A", "B"}:
        raise HarnessError("arm_invalid")
    try:
        with base_tree_workspace(source.repository, source.base_oid) as workspace:
            yield workspace
    except ProductError as error:
        raise HarnessError(error.record["code"]) from error


def token_count(data: bytes) -> int:
    return (len(data) + 3) // 4


def _public_task(task: dict) -> dict:
    if not isinstance(task, dict) or not {"task_id", "snapshot_id", "task_kind"} <= set(task):
        raise HarnessError("task_invalid")
    if set(task) - PUBLIC_TASK_KEYS:
        raise HarnessError("task_contains_evaluator_state")
    title = task.get("title", "")
    if not isinstance(title, str) or re.search(r"\b[0-9a-fA-F]{7,64}\b", title):
        raise HarnessError("task_brief_leakage_detected")
    return dict(task)


def _assert_packet_blind(packet: dict) -> None:
    def visit(value):
        if isinstance(value, dict):
            if set(value) & FORBIDDEN_PACKET_KEYS:
                raise HarnessError("packet_leakage_detected")
            for item in value.values(): visit(item)
        elif isinstance(value, list):
            for item in value: visit(item)
        elif isinstance(value, str):
            if "/.git/" in value or value.startswith(".git/") or re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", value):
                raise HarnessError("packet_leakage_detected")
    visit(packet)


def request(task: dict, packet: dict | None = None) -> dict:
    public = _public_task(task)
    content = "Return only one closed m21.context_set.v1 JSON object. Task: " + canonical_bytes(public).decode()
    if packet is not None:
        _assert_packet_blind(packet)
        content += "\nSealed subject-first packet: " + canonical_bytes(packet).decode()
    return {"model":MODEL,"messages":[{"role":"user","content":content}],"tools":TOOLS,"temperature":0,"max_tokens":24000,"reasoning_effort":"low","stream":False}


def _packet_facts(task: dict, audit: dict) -> list[dict]:
    subject_id = task.get("subject_symbol_id")
    if not isinstance(subject_id, str):
        return []
    facts = {}
    for envelope in audit.get("contexts", []):
        context = envelope.get("context", {})
        outcomes = [row for row in context.get("subject_outcomes", []) if row.get("state") == "admitted"]
        if subject_id not in {row.get("endpoint_id") for row in outcomes}:
            continue
        paths = {row.get("artifact_id"):row.get("path") for row in context.get("materialized_sources", [])}
        for row in outcomes:
            symbol_id = row.get("endpoint_id")
            span = row.get("requested_range", {})
            path = paths.get(row.get("source_artifact_id"))
            if not isinstance(path, str) or not isinstance(symbol_id, str):
                raise HarnessError("product_projection_invalid")
            relation = "subject" if symbol_id == subject_id else row.get("role", "reference")
            fact_id = stable_id("packet-fact", context.get("context_id", ""), symbol_id, relation)
            facts[fact_id] = {
                "fact_id":fact_id,
                "distance":0 if relation == "subject" else 1,
                "path":path,
                "start_line":span.get("start_line"),
                "end_line":span.get("end_line"),
                "symbol_id":symbol_id,
                "relation":relation,
                "binding":"resolved",
                "source_id":row.get("source_artifact_id"),
            }
    return sorted(facts.values(), key=lambda row: row["fact_id"])


def _build_treatment_packet(task: dict, source: TreatmentSource) -> tuple[dict, int, int, int, int]:
    # The former parent(base)->base D projection is intentionally unavailable:
    # it requires history and is not a task-subject projection.  The product
    # task-subject entry will be wired here after the separate design ruling.
    raise HarnessError("task_subject_projection_pending")


def dry_run(task: dict, treatment: TreatmentSource | None = None, *, arm: str = "A"):
    started = time.monotonic_ns()
    packet = None
    ingest_operations = projection_operations = 0
    ingest_elapsed_ns = projection_elapsed_ns = 0
    if arm not in {"A", "B"}:
        raise HarnessError("arm_invalid")
    if treatment is not None:
        with arm_workspace(treatment, arm):
            if arm == "B":
                packet, ingest_operations, projection_operations, ingest_elapsed_ns, projection_elapsed_ns = _build_treatment_packet(task, treatment)
    elif arm == "B":
        raise HarnessError("base_workspace_required")
    packet_ns = time.monotonic_ns() - started if treatment is not None else 0
    body = canonical_bytes(request(task, packet))
    tokens = token_count(body)
    elapsed_ns = time.monotonic_ns() - started
    values = {
        "input_tokens":tokens,
        "output_tokens":0,
        "tool_calls":0,
        "model_elapsed_ns":0,
        "packet_construction_ns":packet_ns,
        "elapsed_ns":elapsed_ns,
        "product_ingest_operations":ingest_operations,
        "product_projection_operations":projection_operations,
        "product_ingest_elapsed_ns":ingest_elapsed_ns,
        "product_projection_elapsed_ns":projection_elapsed_ns,
    }
    _validate_measurement_values(values)

    # A distinct type, constructor capability, and HMAC key are created for
    # every harness execution.  Neither the type nor key exists at module scope.
    constructor_capability = object()
    authentication_key = secrets.token_bytes(32)
    payload = canonical_bytes({"schema":"m21.harness_measurement.v1", **values})
    authentication_tag = hmac.new(authentication_key, payload, hashlib.sha256).digest()

    class RunMeasurement:
        __slots__ = ("_values", "_tag")

        def __new__(cls, capability, *_args):
            if capability is not constructor_capability:
                raise HarnessError("harness_measurement_private")
            return object.__new__(cls)

        def __init__(self, capability, sealed_values, tag):
            object.__setattr__(self, "_values", sealed_values)
            object.__setattr__(self, "_tag", tag)

        def __getattr__(self, name):
            if name in values:
                return self._verified_values()[name]
            raise AttributeError(name)

        def __setattr__(self, name, value):
            raise AttributeError("harness measurement is immutable")

        def _verified_values(self):
            sealed = object.__getattribute__(self, "_values")
            tag = object.__getattribute__(self, "_tag")
            candidate = canonical_bytes({"schema":"m21.harness_measurement.v1", **sealed})
            if not hmac.compare_digest(tag, hmac.new(authentication_key, candidate, hashlib.sha256).digest()):
                raise HarnessError("harness_measurement_authentication_failed")
            _validate_measurement_values(sealed)
            return dict(sealed)

        def record(self):
            return {"schema":"m21.harness_measurement.v1", **self._verified_values(), "tokenizer":dict(TOKENIZER)}

    class RunResult:
        __slots__ = ("body", "request_bytes", "request_sha256", "packet", "measurement")

        def __new__(cls, capability, *_args):
            if capability is not constructor_capability:
                raise HarnessError("harness_result_private")
            return object.__new__(cls)

        def __init__(self, capability, result_body, result_packet, result_measurement):
            object.__setattr__(self, "body", result_body)
            object.__setattr__(self, "request_bytes", len(body))
            object.__setattr__(self, "request_sha256", sha256(body))
            object.__setattr__(self, "packet", result_packet)
            object.__setattr__(self, "measurement", result_measurement)

        def __setattr__(self, name, value):
            raise AttributeError("harness result is immutable")

        def record(self):
            return {
                "schema":"m21.http_dry_run.v1",
                "model":MODEL,
                "request_bytes":self.request_bytes,
                "request_sha256":self.request_sha256,
                "http_sent":False,
                "body":self.body,
                "packet_id":None if self.packet is None else self.packet["packet_id"],
                "measurement":self.measurement.record(),
            }

        def score(self, context, oracle_lines, subject_lines):
            if type(self.measurement) is not RunMeasurement:
                raise HarnessError("harness_measurement_type_invalid")
            from .scoring import _score_values
            return _score_values(context, oracle_lines, subject_lines, self.measurement._verified_values())

    measurement = RunMeasurement(constructor_capability, dict(values), authentication_tag)
    return RunResult(constructor_capability, json.loads(body), packet, measurement)


def preflight(endpoint):
    with urllib.request.urlopen(endpoint.rstrip("/") + "/v1/models", timeout=30) as response:
        listing = json.loads(response.read())
    if MODEL not in [row.get("id") for row in listing.get("data", [])]:
        raise RuntimeError("model_identity_missing")


def arm_c(task, packet):
    _public_task(task)
    _assert_packet_blind(packet)
    return {"task_id":task["task_id"],"snapshot_id":task["snapshot_id"],"context_items":packet["context_items"],"coverage_claim":packet["coverage_claim"],"declared_losses":packet["declared_losses"]}
