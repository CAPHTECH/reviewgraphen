"""Atomic Stage-0 occurrence and context-v3 closure constructors."""
from __future__ import annotations

from typing import Any

from .canonical import MAX_INTEGER, canonical_bytes, hash_json, parse_json_bytes, sha256_bytes, stable_id


PROFILE_HASH = "sha256:4b6cca93794ab03b1576e17d2e395ec43f731d316685247a89363ae2e840dd96"
CONTEXT_V2_HASH = "sha256:7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26"
CONTEXT_V3_HASH = "sha256:932bfa18c5d286c63196366d6d2dc1aaf402f50baa1f1ab5f075b1007be55dd8"
CONTEXT_POLICY_ID = "context.subject_windows@3"
EXTRACTOR_ID = "reviewgraphen.ingest.rust-call-enumeration@2"
CONTEXT_V3_BYTES = b'{"accepted_file_denominator_bound":"request.ingest.max_files","anchors_per_file":1024,"assumptions":"empty","callees_depth":3,"callers_depth":2,"candidate_order":["subject_priority","distance","path_rank","artifact_id"],"canonical_envelope_bytes":786432,"contains_edges":1000000,"discovery_paths":20,"edge_kind_direction_order":["calls:forward","calls:reverse","contains:forward","contains:reverse","covers:forward","covers:reverse"],"excerpt_lines":400,"final_window_order":["source_artifact_id","start_line","end_line","window_id"],"included_files":64,"latent_cardinality":"known_zero_or_unknown_with_qualification_ids","loss_reason_precedence":["missing_location","missing_source","giant_line","per_window_lines","per_window_bytes","per_file_window_cap","total_window_cap","total_excerpt_bytes","overlap_unmergeable","path_cap","test_cap","not_reached","included_file_cap","artifact_bytes_cap","total_resolved_bytes_cap"],"materialized_source_denominator":"subject_file_ids_union_reached_file_ids","max_assumptions":64,"max_discovered_structural_ids":4096,"max_excerpt_bytes":262144,"max_materialized_source_candidates":4096,"max_resolved_artifact_bytes":1048576,"max_resolved_bytes":8388608,"max_string_bytes":16384,"max_subject_losses":2,"max_support_loss_summaries":15,"max_total_excerpt_bytes":1048576,"max_unknowns":64,"obligations_per_envelope":1,"policy_id":"context.subject_windows@3","related_tests":10,"relation_scan":1000000,"seed_fields":["source_ids","target_refs","context_ids"],"source_candidate_denominator":"all_accepted_file_ids_known_count_and_sorted_id_set_sha256","subject_endpoints":2,"subject_order":["callee","caller"],"support_anchor_denominator":"reached_range_bearing_exact_path_anchor_ids_known_count_and_sorted_id_set_sha256","support_loss_summary":"reason_known_count_and_sorted_anchor_id_set_sha256","unknown_reason_ids":["unresolved_invariant_scope","unresolved_relation_endpoint","unresolved_review_context_member","unresolved_seed_reference"],"window_candidate_order":["priority","role","source_artifact_id","start_line","end_line","owner_id"],"window_merge":"same_source_overlap_or_adjacent_if_union_within_per_window_bounds","windows_per_envelope":8,"windows_per_file":4}'

LEGAL_CALLS = {
    "direct": {
        "direct_non_path",
        "direct_empty_path",
        "direct_shadowed_binding",
        "direct_unresolved_scope",
        "direct_target_count_zero",
        "direct_target_count_multiple",
    },
    "method": {"method_dispatch_unresolved"},
    "macro_invocation": {"macro_expansion_unresolved"},
}
CALL_KINDS = {
    "direct": "relation_unresolved",
    "method": "dynamic_dispatch_unresolved",
    "macro_invocation": "macro_expansion_unresolved",
}
LOSS_REASONS = (
    "missing_location",
    "missing_source",
    "giant_line",
    "per_window_lines",
    "per_window_bytes",
    "per_file_window_cap",
    "total_window_cap",
    "total_excerpt_bytes",
    "overlap_unmergeable",
    "path_cap",
    "test_cap",
    "not_reached",
    "included_file_cap",
    "artifact_bytes_cap",
    "total_resolved_bytes_cap",
)


class Stage0ContractError(ValueError):
    """Typed deterministic refusal at the Stage-0 sealing boundary."""

    def __init__(self, code: str):
        self.code = code
        super().__init__(code)


def _closed(value: Any, fields: set[str], code: str) -> dict:
    if not isinstance(value, dict) or set(value) != fields:
        raise Stage0ContractError(code)
    return value


def _text(value: Any, code: str) -> str:
    if not isinstance(value, str) or not value:
        raise Stage0ContractError(code)
    return value


def _count(value: Any, code: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= MAX_INTEGER:
        raise Stage0ContractError(code)
    return value


def _ids(values: Any, code: str, allow_empty: bool = True) -> list[str]:
    if not isinstance(values, list) or (not allow_empty and not values):
        raise Stage0ContractError(code)
    if not all(isinstance(value, str) and value for value in values):
        raise Stage0ContractError(code)
    if values != sorted(values, key=lambda value: value.encode("utf-8")) or len(values) != len(set(values)):
        raise Stage0ContractError(code)
    return values


def id_set_commitment(values: list[str]) -> dict:
    ordered = _ids(values, "id_set_invalid")
    return {
        "cardinality": "known",
        "observed_count": len(ordered),
        "sorted_id_set_sha256": hash_json(ordered),
    }


def _validate_commitment(value: Any, code: str) -> dict:
    item = _closed(value, {"cardinality", "observed_count", "sorted_id_set_sha256"}, code)
    if item["cardinality"] != "known":
        raise Stage0ContractError(code)
    _count(item["observed_count"], code)
    _text(item["sorted_id_set_sha256"], code)
    return item


def context_policy_v3() -> dict:
    if sha256_bytes(CONTEXT_V3_BYTES) != CONTEXT_V3_HASH:
        raise Stage0ContractError("context_policy_hash_invalid")
    value = parse_json_bytes(CONTEXT_V3_BYTES)
    if canonical_bytes(value) != CONTEXT_V3_BYTES or value["policy_id"] != CONTEXT_POLICY_ID:
        raise Stage0ContractError("context_policy_bytes_invalid")
    return value


def _occurrence(draft: Any, snapshot_id: str, accepted_by_path: dict[str, str]) -> dict:
    fields = {"path", "span", "source_ids", "call_kind", "reason"}
    item = _closed(draft, fields, "occurrence_draft_invalid")
    path = _text(item["path"], "occurrence_path_invalid")
    if path not in accepted_by_path:
        raise Stage0ContractError("occurrence_file_not_accepted")
    call_kind = item["call_kind"]
    reason = item["reason"]
    if call_kind not in LEGAL_CALLS or reason not in LEGAL_CALLS[call_kind]:
        raise Stage0ContractError("occurrence_taxonomy_invalid")
    source_ids = _ids(item["source_ids"], "occurrence_sources_invalid", allow_empty=False)
    if accepted_by_path[path] not in source_ids:
        raise Stage0ContractError("occurrence_file_source_missing")
    span = _closed(item["span"], {"start_line", "start_column", "end_line", "end_column"}, "occurrence_span_invalid")
    for name in span:
        if _count(span[name], "occurrence_span_invalid") < 1:
            raise Stage0ContractError("occurrence_span_invalid")
    if (span["end_line"], span["end_column"]) < (span["start_line"], span["start_column"]):
        raise Stage0ContractError("occurrence_span_invalid")
    description = (
        "direct_calls unresolved occurrence: call_kind="
        f"{call_kind}; reason={reason}; path={path}; span="
        f"{span['start_line']}:{span['start_column']}-{span['end_line']}:{span['end_column']}"
    )
    body = {
        "schema": "reviewgraphen.ingestion_obstruction.v2",
        "snapshot_id": snapshot_id,
        "projection_extractor_id": EXTRACTOR_ID,
        "file_source_id": accepted_by_path[path],
        "path": path,
        "span": span,
        "source_ids": source_ids,
        "call_kind": call_kind,
        "reason": reason,
        "kind": CALL_KINDS[call_kind],
        "severity": "medium",
        "related_capabilities": ["direct_calls"],
    }
    return {**body, "id": stable_id("ingestion-obstruction", body), "description": description}


def _global_limitation(snapshot_id: str, source_ids: list[str]) -> dict:
    body = {
        "schema": "reviewgraphen.ingestion_global_limitation.v2",
        "snapshot_id": snapshot_id,
        "projection_extractor_id": EXTRACTOR_ID,
        "kind": "global_direct_calls_limitation",
        "severity": "info",
        "source_ids": _ids(source_ids, "global_limitation_sources_invalid", allow_empty=False),
        "related_capabilities": ["direct_calls"],
        "latent_occurrence_count": "unknown",
    }
    return {
        **body,
        "id": stable_id("ingestion-limitation", body),
        "description": "direct_calls enumeration is incomplete; macro latent occurrence count is unknown",
    }


def build_occurrence_closure(
    identity: dict,
    accepted_files: list[dict],
    occurrence_drafts: list[dict],
    global_source_ids: list[str],
    metrics: dict,
) -> dict:
    identity = _closed(
        identity,
        {"snapshot_id", "target_revision", "legacy_program_space_sha256", "legacy_extraction_report_sha256"},
        "occurrence_identity_invalid",
    )
    for value in identity.values():
        _text(value, "occurrence_identity_invalid")
    if not isinstance(accepted_files, list):
        raise Stage0ContractError("accepted_files_invalid")
    accepted_by_path: dict[str, str] = {}
    for raw in accepted_files:
        item = _closed(raw, {"file_source_id", "path"}, "accepted_file_invalid")
        path = _text(item["path"], "accepted_file_invalid")
        source_id = _text(item["file_source_id"], "accepted_file_invalid")
        if path in accepted_by_path or source_id in accepted_by_path.values():
            raise Stage0ContractError("accepted_file_duplicate")
        accepted_by_path[path] = source_id
    occurrences = [_occurrence(item, identity["snapshot_id"], accepted_by_path) for item in occurrence_drafts]
    occurrence_ids = sorted((item["id"] for item in occurrences), key=lambda value: value.encode("utf-8"))
    if len(occurrence_ids) != len(set(occurrence_ids)):
        raise Stage0ContractError("occurrence_id_duplicate")
    by_file: dict[str, list[dict]] = {}
    for item in occurrences:
        by_file.setdefault(item["file_source_id"], []).append(item)
    summaries = []
    for file_source_id, file_occurrences in by_file.items():
        path = file_occurrences[0]["path"]
        bucket_groups: dict[tuple[str, str], list[str]] = {}
        for item in file_occurrences:
            bucket_groups.setdefault((item["call_kind"], item["reason"]), []).append(item["id"])
        if len(bucket_groups) > 8:
            raise Stage0ContractError("occurrence_bucket_limit")
        buckets = []
        for (call_kind, reason), identifiers in sorted(bucket_groups.items(), key=lambda pair: (pair[0][0].encode(), pair[0][1].encode())):
            ordered = sorted(identifiers, key=lambda value: value.encode("utf-8"))
            buckets.append({
                "call_kind": call_kind,
                "reason": reason,
                "observed_occurrence_count": len(ordered),
                "occurrence_id_set_sha256": hash_json(ordered),
            })
        file_ids = sorted((item["id"] for item in file_occurrences), key=lambda value: value.encode("utf-8"))
        body = {
            "schema": "reviewgraphen.ingestion_obstruction_summary.v1",
            "kind": "call_enumeration_summary",
            "severity": "medium",
            "snapshot_id": identity["snapshot_id"],
            "projection_extractor_id": EXTRACTOR_ID,
            "file_source_id": file_source_id,
            "path": path,
            "related_capabilities": ["direct_calls"],
            "observed_occurrence_count": len(file_ids),
            "occurrence_id_set_sha256": hash_json(file_ids),
            "buckets": buckets,
            "detail_retention": "spans_and_owner_sources_omitted_rebuildable",
        }
        summaries.append({**body, "id": stable_id("ingestion-obstruction-summary", body)})
    summaries.sort(key=lambda item: item["id"].encode("utf-8"))
    limitation = _global_limitation(identity["snapshot_id"], global_source_ids)
    report_body = {
        "schema": "reviewgraphen.ingestion_report.v2",
        **identity,
        "projection_extractor_id": EXTRACTOR_ID,
        "source_occurrence_summaries": summaries,
        "observed_occurrence_count": len(occurrence_ids),
        "occurrence_id_set_sha256": hash_json(occurrence_ids),
        "global_direct_calls_limitation": limitation,
    }
    report_preimage = {
        **{key: value for key, value in report_body.items() if key != "source_occurrence_summaries"},
        "source_occurrence_summary_ids": [item["id"] for item in summaries],
    }
    report = {**report_body, "report_id": stable_id("ingestion-report", report_preimage)}
    summary_ids = [item["id"] for item in summaries]
    obstruction_ids = sorted([*summary_ids, limitation["id"]], key=lambda value: value.encode("utf-8"))
    coverage = {
        "enumeration_obstruction_summary_ids": summary_ids,
        "enumeration_limitation_ids": [limitation["id"]],
        "enumeration_obstruction_ids": obstruction_ids,
        "observed_unresolved_call_occurrence_count": len(occurrence_ids),
        "occurrence_id_set_sha256": hash_json(occurrence_ids),
    }
    metric_seed = _closed(metrics, {"schema", "cluster_id", "wall_time_milliseconds", "peak_bytes"}, "occurrence_metrics_invalid")
    if metric_seed["schema"] != "m20.stage0-occurrence-metrics.v1":
        raise Stage0ContractError("occurrence_metrics_invalid")
    _text(metric_seed["cluster_id"], "occurrence_metrics_invalid")
    _count(metric_seed["wall_time_milliseconds"], "occurrence_metrics_invalid")
    _count(metric_seed["peak_bytes"], "occurrence_metrics_invalid")
    checked_metrics = {
        **metric_seed,
        "report_bytes": len(canonical_bytes(report)),
        "summary_rows": len(summaries),
        "observed_occurrence_count": len(occurrence_ids),
    }
    return {"schema": "m20.stage0-occurrence-closure.v1", "ingestion_report_v2": report, "rule_coverage": coverage, "metrics": checked_metrics}


def validate_occurrence_metrics(value: Any, summary_rows: int, occurrence_count: int, report_bytes: int) -> dict:
    item = _closed(
        value,
        {"schema", "cluster_id", "wall_time_milliseconds", "peak_bytes", "report_bytes", "summary_rows", "observed_occurrence_count"},
        "occurrence_metrics_invalid",
    )
    if item["schema"] != "m20.stage0-occurrence-metrics.v1":
        raise Stage0ContractError("occurrence_metrics_invalid")
    _text(item["cluster_id"], "occurrence_metrics_invalid")
    for field in ("wall_time_milliseconds", "peak_bytes", "report_bytes", "summary_rows", "observed_occurrence_count"):
        _count(item[field], "occurrence_metrics_invalid")
    if item["summary_rows"] != summary_rows or item["observed_occurrence_count"] != occurrence_count or item["report_bytes"] != report_bytes:
        raise Stage0ContractError("occurrence_metrics_mismatch")
    return item


def validate_occurrence_rebuild(
    identity: dict,
    accepted_files: list[dict],
    occurrence_drafts: list[dict],
    global_source_ids: list[str],
    public: Any,
) -> dict:
    if not isinstance(public, dict):
        raise Stage0ContractError("occurrence_closure_invalid")
    report = public.get("ingestion_report_v2")
    if isinstance(report, dict) and "located_call_occurrences" in report:
        raise Stage0ContractError("pre_amendment_occurrence_shape")
    metrics = public.get("metrics")
    if not isinstance(metrics, dict):
        raise Stage0ContractError("occurrence_metrics_invalid")
    metric_seed = {key: metrics[key] for key in ("schema", "cluster_id", "wall_time_milliseconds", "peak_bytes") if key in metrics}
    rebuilt = build_occurrence_closure(identity, accepted_files, occurrence_drafts, global_source_ids, metric_seed)
    if public != rebuilt:
        raise Stage0ContractError("occurrence_closure_mismatch")
    return rebuilt


def validate_occurrence_public(public: Any) -> dict:
    value = _closed(public, {"schema", "ingestion_report_v2", "rule_coverage", "metrics"}, "occurrence_closure_invalid")
    if value["schema"] != "m20.stage0-occurrence-closure.v1":
        raise Stage0ContractError("occurrence_closure_invalid")
    report = _closed(
        value["ingestion_report_v2"],
        {
            "schema", "report_id", "snapshot_id", "target_revision", "legacy_program_space_sha256",
            "legacy_extraction_report_sha256", "projection_extractor_id", "source_occurrence_summaries",
            "observed_occurrence_count", "occurrence_id_set_sha256", "global_direct_calls_limitation",
        },
        "ingestion_report_invalid",
    )
    if "located_call_occurrences" in report or report["schema"] != "reviewgraphen.ingestion_report.v2" or report["projection_extractor_id"] != EXTRACTOR_ID:
        raise Stage0ContractError("pre_amendment_occurrence_shape")
    summaries = report["source_occurrence_summaries"]
    if not isinstance(summaries, list) or len(summaries) > 20_000:
        raise Stage0ContractError("occurrence_summary_invalid")
    summary_ids = []
    file_ids = set()
    total = 0
    for summary in summaries:
        body = _closed(
            summary,
            {"schema", "id", "kind", "severity", "snapshot_id", "projection_extractor_id", "file_source_id", "path", "related_capabilities", "observed_occurrence_count", "occurrence_id_set_sha256", "buckets", "detail_retention"},
            "occurrence_summary_invalid",
        )
        if body["schema"] != "reviewgraphen.ingestion_obstruction_summary.v1" or body["kind"] != "call_enumeration_summary" or body["severity"] != "medium" or body["snapshot_id"] != report["snapshot_id"] or body["projection_extractor_id"] != EXTRACTOR_ID or body["related_capabilities"] != ["direct_calls"] or body["detail_retention"] != "spans_and_owner_sources_omitted_rebuildable":
            raise Stage0ContractError("occurrence_summary_invalid")
        if body["file_source_id"] in file_ids:
            raise Stage0ContractError("occurrence_summary_duplicate_file")
        file_ids.add(body["file_source_id"])
        count = _count(body["observed_occurrence_count"], "occurrence_summary_invalid")
        if count < 1 or not isinstance(body["buckets"], list) or not 1 <= len(body["buckets"]) <= 8:
            raise Stage0ContractError("occurrence_bucket_invalid")
        bucket_keys = []
        bucket_total = 0
        for bucket in body["buckets"]:
            item = _closed(bucket, {"call_kind", "reason", "observed_occurrence_count", "occurrence_id_set_sha256"}, "occurrence_bucket_invalid")
            if item["call_kind"] not in LEGAL_CALLS or item["reason"] not in LEGAL_CALLS[item["call_kind"]]:
                raise Stage0ContractError("occurrence_bucket_invalid")
            amount = _count(item["observed_occurrence_count"], "occurrence_bucket_invalid")
            if amount < 1:
                raise Stage0ContractError("occurrence_bucket_invalid")
            bucket_total += amount
            bucket_keys.append((item["call_kind"], item["reason"]))
        if bucket_keys != sorted(bucket_keys, key=lambda pair: (pair[0].encode(), pair[1].encode())) or len(bucket_keys) != len(set(bucket_keys)) or bucket_total != count:
            raise Stage0ContractError("occurrence_bucket_invalid")
        expected_id = stable_id("ingestion-obstruction-summary", {key: item for key, item in body.items() if key != "id"})
        if body["id"] != expected_id:
            raise Stage0ContractError("occurrence_summary_id_invalid")
        summary_ids.append(body["id"])
        total += count
    if summary_ids != sorted(summary_ids, key=lambda value: value.encode()) or len(summary_ids) != len(set(summary_ids)):
        raise Stage0ContractError("occurrence_summary_order_invalid")
    limitation = _closed(
        report["global_direct_calls_limitation"],
        {"schema", "id", "snapshot_id", "projection_extractor_id", "kind", "severity", "source_ids", "related_capabilities", "latent_occurrence_count", "description"},
        "global_limitation_invalid",
    )
    if limitation["schema"] != "reviewgraphen.ingestion_global_limitation.v2" or limitation["snapshot_id"] != report["snapshot_id"] or limitation["projection_extractor_id"] != EXTRACTOR_ID or limitation["kind"] != "global_direct_calls_limitation" or limitation["severity"] != "info" or limitation["related_capabilities"] != ["direct_calls"] or limitation["latent_occurrence_count"] != "unknown":
        raise Stage0ContractError("global_limitation_invalid")
    _ids(limitation["source_ids"], "global_limitation_invalid", allow_empty=False)
    limitation_body = {key: item for key, item in limitation.items() if key not in {"id", "description"}}
    if limitation["id"] != stable_id("ingestion-limitation", limitation_body) or limitation["description"] != "direct_calls enumeration is incomplete; macro latent occurrence count is unknown":
        raise Stage0ContractError("global_limitation_invalid")
    if _count(report["observed_occurrence_count"], "ingestion_report_invalid") != total:
        raise Stage0ContractError("occurrence_report_count_invalid")
    report_preimage = {
        **{key: item for key, item in report.items() if key not in {"report_id", "source_occurrence_summaries"}},
        "source_occurrence_summary_ids": summary_ids,
    }
    if report["report_id"] != stable_id("ingestion-report", report_preimage):
        raise Stage0ContractError("occurrence_report_id_invalid")
    coverage = _closed(
        value["rule_coverage"],
        {"enumeration_obstruction_summary_ids", "enumeration_limitation_ids", "enumeration_obstruction_ids", "observed_unresolved_call_occurrence_count", "occurrence_id_set_sha256"},
        "occurrence_coverage_invalid",
    )
    obstruction_ids = sorted([*summary_ids, limitation["id"]], key=lambda item: item.encode())
    if coverage != {
        "enumeration_obstruction_summary_ids": summary_ids,
        "enumeration_limitation_ids": [limitation["id"]],
        "enumeration_obstruction_ids": obstruction_ids,
        "observed_unresolved_call_occurrence_count": report["observed_occurrence_count"],
        "occurrence_id_set_sha256": report["occurrence_id_set_sha256"],
    }:
        raise Stage0ContractError("occurrence_coverage_invalid")
    validate_occurrence_metrics(value["metrics"], len(summaries), total, len(canonical_bytes(report)))
    return value


def support_anchor(anchor: Any) -> dict:
    item = _closed(anchor, {"snapshot_id", "source_artifact_id", "owner_artifact_id", "start_line", "end_line"}, "support_anchor_invalid")
    for field in ("snapshot_id", "source_artifact_id", "owner_artifact_id"):
        _text(item[field], "support_anchor_invalid")
    start = _count(item["start_line"], "support_anchor_invalid")
    end = _count(item["end_line"], "support_anchor_invalid")
    if start < 1 or end < start:
        raise Stage0ContractError("support_anchor_invalid")
    identity = {"anchor_contract": "context.support_anchor@1", "end_line": end, "owner_artifact_id": item["owner_artifact_id"], "snapshot_id": item["snapshot_id"], "source_artifact_id": item["source_artifact_id"], "start_line": start}
    return {**item, "anchor_id": stable_id("context-support-anchor", identity)}


def _latent(value: Any) -> dict:
    if not isinstance(value, dict) or value.get("state") not in {"known_zero", "unknown"}:
        raise Stage0ContractError("latent_cardinality_invalid")
    if value["state"] == "known_zero":
        return _closed(value, {"state"}, "latent_cardinality_invalid")
    item = _closed(value, {"state", "capability_states", "qualification_ids"}, "latent_cardinality_invalid")
    states = item["capability_states"]
    if not isinstance(states, dict) or not states or not all(isinstance(key, str) and key and state in {"partial", "unknown"} for key, state in states.items()):
        raise Stage0ContractError("latent_cardinality_invalid")
    _ids(item["qualification_ids"], "latent_cardinality_invalid", allow_empty=False)
    return item


def _subject_outcomes(values: Any) -> list[dict]:
    if not isinstance(values, list) or [value.get("role") if isinstance(value, dict) else None for value in values] != ["callee", "caller"]:
        raise Stage0ContractError("subject_outcome_order_invalid")
    output = []
    for value in values:
        if value.get("status") == "admitted":
            item = _closed(value, {"role", "status", "endpoint_id", "source_artifact_id", "start_line", "end_line", "window_id"}, "subject_outcome_invalid")
            for field in ("role", "endpoint_id", "source_artifact_id", "window_id"):
                _text(item[field], "subject_outcome_invalid")
            if _count(item["start_line"], "subject_outcome_invalid") < 1 or _count(item["end_line"], "subject_outcome_invalid") < item["start_line"]:
                raise Stage0ContractError("subject_outcome_invalid")
        elif value.get("status") == "lost":
            item = _closed(value, {"role", "status", "endpoint_id", "subject_loss"}, "subject_outcome_invalid")
            loss = _closed(item["subject_loss"], {"severity", "reason", "recovery_reference", "source_ids"}, "subject_outcome_invalid")
            if loss["severity"] != "high":
                raise Stage0ContractError("subject_outcome_invalid")
            _text(loss["reason"], "subject_outcome_invalid")
            _text(loss["recovery_reference"], "subject_outcome_invalid")
            _ids(loss["source_ids"], "subject_outcome_invalid", allow_empty=False)
        else:
            raise Stage0ContractError("subject_outcome_invalid")
        output.append(item)
    return output


def build_context_projection_v3(
    identity: dict,
    accepted_file_ids: list[str],
    reached_file_ids: list[str],
    subject_outcomes: list[dict],
    materialized_sources: list[dict],
    anchors: list[dict],
    admitted_windows: list[dict],
    support_loss_partitions: list[dict],
    latent_cardinality: dict,
    unknown_ids: list[str],
    remaining_loss_ids: list[str],
    remaining_source_ids: list[str],
) -> dict:
    context_policy_v3()
    identity = _closed(identity, {"projection_id", "snapshot_id", "request_id", "obligation_ids", "relation_ids", "endpoint_pairs"}, "context_identity_invalid")
    for field in ("projection_id", "snapshot_id", "request_id"):
        _text(identity[field], "context_identity_invalid")
    accepted = _ids(accepted_file_ids, "accepted_file_denominator_invalid")
    reached = _ids(reached_file_ids, "reached_file_denominator_invalid")
    if not set(reached) <= set(accepted):
        raise Stage0ContractError("reached_file_not_accepted")
    subjects = _subject_outcomes(subject_outcomes)
    subject_files = [item["source_artifact_id"] for item in subjects if item["status"] == "admitted"]
    if any(value not in accepted for value in subject_files):
        raise Stage0ContractError("subject_file_not_accepted")
    materialized_ids = sorted(set(subject_files) | set(reached), key=lambda value: value.encode())
    if len(materialized_ids) > 4096:
        raise Stage0ContractError("materialized_source_overflow")
    if not isinstance(materialized_sources, list):
        raise Stage0ContractError("materialized_sources_invalid")
    source_fields = {"source_artifact_id", "snapshot_side", "path", "blob_oid"}
    source_rows = []
    for raw in materialized_sources:
        item = _closed(raw, source_fields, "materialized_sources_invalid")
        for field in source_fields:
            _text(item[field], "materialized_sources_invalid")
        source_rows.append(item)
    source_rows.sort(key=lambda item: item["source_artifact_id"].encode())
    if [item["source_artifact_id"] for item in source_rows] != materialized_ids:
        raise Stage0ContractError("materialized_sources_invalid")
    anchor_rows = [support_anchor(item) for item in anchors]
    anchor_rows.sort(key=lambda item: item["anchor_id"].encode())
    anchor_ids = [item["anchor_id"] for item in anchor_rows]
    if len(anchor_ids) != len(set(anchor_ids)) or any(item["source_artifact_id"] not in reached for item in anchor_rows):
        raise Stage0ContractError("support_anchor_invalid")
    windows = []
    window_fields = {"window_id", "source_artifact_id", "start_line", "end_line", "role", "source_required_id", "support_anchor_ids"}
    for raw in admitted_windows:
        item = _closed(raw, window_fields, "admitted_window_invalid")
        for field in ("window_id", "source_artifact_id", "role", "source_required_id"):
            _text(item[field], "admitted_window_invalid")
        if item["source_artifact_id"] not in materialized_ids or _count(item["start_line"], "admitted_window_invalid") < 1 or _count(item["end_line"], "admitted_window_invalid") < item["start_line"]:
            raise Stage0ContractError("admitted_window_invalid")
        _ids(item["support_anchor_ids"], "admitted_window_invalid")
        windows.append(item)
    windows.sort(key=lambda item: (item["source_artifact_id"].encode(), item["start_line"], item["end_line"], item["window_id"].encode()))
    if len(windows) > 8 or len({item["window_id"] for item in windows}) != len(windows):
        raise Stage0ContractError("admitted_window_invalid")
    for subject in subjects:
        if subject["status"] == "admitted" and not any(window["window_id"] == subject["window_id"] and window["source_artifact_id"] == subject["source_artifact_id"] for window in windows):
            raise Stage0ContractError("subject_window_missing")
    admitted_anchor_ids = []
    for window in windows:
        admitted_anchor_ids.extend(window["support_anchor_ids"])
    if len(admitted_anchor_ids) != len(set(admitted_anchor_ids)) or not set(admitted_anchor_ids) <= set(anchor_ids):
        raise Stage0ContractError("support_anchor_partition_invalid")
    if not isinstance(support_loss_partitions, list) or len(support_loss_partitions) > 15:
        raise Stage0ContractError("support_loss_summary_invalid")
    summaries = []
    lost_anchor_ids = []
    reason_indexes = []
    for raw in support_loss_partitions:
        item = _closed(raw, {"reason", "anchor_ids"}, "support_loss_summary_invalid")
        if item["reason"] not in LOSS_REASONS:
            raise Stage0ContractError("support_loss_summary_invalid")
        identifiers = _ids(item["anchor_ids"], "support_loss_summary_invalid", allow_empty=False)
        lost_anchor_ids.extend(identifiers)
        reason_indexes.append(LOSS_REASONS.index(item["reason"]))
        summaries.append({"reason": item["reason"], **id_set_commitment(identifiers)})
    if reason_indexes != sorted(reason_indexes) or len(reason_indexes) != len(set(reason_indexes)) or len(lost_anchor_ids) != len(set(lost_anchor_ids)):
        raise Stage0ContractError("support_loss_summary_invalid")
    if set(admitted_anchor_ids) & set(lost_anchor_ids) or set(admitted_anchor_ids) | set(lost_anchor_ids) != set(anchor_ids):
        raise Stage0ContractError("support_anchor_partition_invalid")
    latent = _latent(latent_cardinality)
    unknowns = _ids(unknown_ids, "context_unknowns_invalid")
    if len(unknowns) > 64:
        raise Stage0ContractError("context_unknown_overflow")
    obligations = _ids(identity["obligation_ids"], "context_identity_invalid", allow_empty=False)
    relations = _ids(identity["relation_ids"], "context_identity_invalid", allow_empty=False)
    if not isinstance(identity["endpoint_pairs"], list) or not identity["endpoint_pairs"]:
        raise Stage0ContractError("context_identity_invalid")
    for pair in identity["endpoint_pairs"]:
        _closed(pair, {"caller_endpoint_id", "callee_endpoint_id"}, "context_identity_invalid")
    body = {
        "policy_id": CONTEXT_POLICY_ID,
        "policy_sha256": CONTEXT_V3_HASH,
        **identity,
        "obligation_ids": obligations,
        "relation_ids": relations,
        "accepted_file_denominator": id_set_commitment(accepted),
        "reached_file_denominator": id_set_commitment(reached),
        "materialized_source_denominator": id_set_commitment(materialized_ids),
        "support_anchor_denominator": id_set_commitment(anchor_ids),
        "latent_cardinality": latent,
        "subject_outcomes": subjects,
        "materialized_sources": source_rows,
        "admitted_windows": windows,
        "support_loss_summaries": summaries,
        "unknown_ids": unknowns,
        "remaining_loss_ids": _ids(remaining_loss_ids, "context_loss_ids_invalid"),
        "remaining_source_ids": _ids(remaining_source_ids, "context_source_ids_invalid"),
        "source_required_ids": sorted((item["source_required_id"] for item in windows), key=lambda value: value.encode()),
    }
    if len(body["source_required_ids"]) != len(set(body["source_required_ids"])):
        raise Stage0ContractError("source_required_ids_invalid")
    return {**body, "canonical_sha256": hash_json(body)}


def validate_context_projection_public(value: Any) -> dict:
    fields = {
        "policy_id", "policy_sha256", "projection_id", "snapshot_id", "request_id", "obligation_ids", "relation_ids", "endpoint_pairs",
        "accepted_file_denominator", "reached_file_denominator", "materialized_source_denominator", "support_anchor_denominator", "latent_cardinality",
        "subject_outcomes", "materialized_sources", "admitted_windows", "support_loss_summaries", "unknown_ids", "remaining_loss_ids", "remaining_source_ids",
        "source_required_ids", "canonical_sha256",
    }
    item = _closed(value, fields, "context_projection_invalid")
    context_policy_v3()
    if item["policy_id"] != CONTEXT_POLICY_ID or item["policy_sha256"] != CONTEXT_V3_HASH:
        raise Stage0ContractError("context_policy_version_invalid")
    for field in ("accepted_file_denominator", "reached_file_denominator", "materialized_source_denominator", "support_anchor_denominator"):
        _validate_commitment(item[field], "context_commitment_invalid")
    if item["materialized_source_denominator"]["observed_count"] > 4096:
        raise Stage0ContractError("materialized_source_overflow")
    _subject_outcomes(item["subject_outcomes"])
    _latent(item["latent_cardinality"])
    _ids(item["unknown_ids"], "context_unknowns_invalid")
    _ids(item["remaining_loss_ids"], "context_loss_ids_invalid")
    _ids(item["remaining_source_ids"], "context_source_ids_invalid")
    _ids(item["source_required_ids"], "source_required_ids_invalid")
    if len(item["unknown_ids"]) > 64 or len(item["support_loss_summaries"]) > 15:
        raise Stage0ContractError("context_bound_invalid")
    for summary in item["support_loss_summaries"]:
        row = _closed(summary, {"reason", "cardinality", "observed_count", "sorted_id_set_sha256"}, "support_loss_summary_invalid")
        if row["reason"] not in LOSS_REASONS:
            raise Stage0ContractError("support_loss_summary_invalid")
        _validate_commitment({key: row[key] for key in ("cardinality", "observed_count", "sorted_id_set_sha256")}, "support_loss_summary_invalid")
    body = {key: value for key, value in item.items() if key != "canonical_sha256"}
    if item["canonical_sha256"] != hash_json(body):
        raise Stage0ContractError("context_projection_hash_invalid")
    return item


def validate_context_rebuild(expected: Any, *args: Any, **kwargs: Any) -> dict:
    rebuilt = build_context_projection_v3(*args, **kwargs)
    if expected != rebuilt:
        raise Stage0ContractError("context_projection_rebuild_mismatch")
    return rebuilt
