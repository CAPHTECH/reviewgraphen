//! Closed M6 report reduction.
//!
//! This module intentionally has no public journal/store entry point.  Its
//! input is minted only after the V5 source-bound authority inspection has
//! completed; wiring that inspection lives with the V6 store reader.  Keeping
//! this reduction independent makes the report/gate contract testable without
//! granting a detached JSON document authority over a run.

use crate::ReportError;
use reviewgraphen_core::{ContentHash, StableId, canonical_json};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const GATE_SCHEMA: &str = "reviewgraphen.incremental_gate.v5";
const GATE_POLICY: &str = "reviewgraphen.incremental_gate@1";

/// Exact event tuple used by the V5 report for every inherited and M6 durable
/// body.  The body remains the Core DTO; this wrapper never parses, reshapes,
/// or supplies an untyped JSON authority.
#[derive(Serialize)]
struct TypedReportEventTupleV5<'a, T: Serialize> {
    event_id: &'a StableId,
    event_sequence: u64,
    body_hash: ContentHash,
    body: &'a T,
}

impl<'a, T: Serialize> TypedReportEventTupleV5<'a, T> {
    fn from_witness(
        value: &'a T,
        witness: &'a reviewgraphen_core::V5ProjectionEventWitness,
    ) -> Self {
        Self {
            event_id: &witness.event_id,
            event_sequence: witness.sequence,
            body_hash: ContentHash::sha256(
                &canonical_json(value).expect("typed report body must be serializable"),
            ),
            body: value,
        }
    }
}

/// The M5 bundle is one atomic event carrying several strict durable DTOs.
/// It must retain the same witness for every emitted subrecord rather than
/// minting independent event tuples for sections, restrictions, or result.
#[derive(Serialize)]
struct TypedM5BundleItemV5<'a, T: Serialize> {
    event_id: &'a StableId,
    event_sequence: u64,
    body_hash: ContentHash,
    body: &'a T,
}

impl<'a, T: Serialize> TypedM5BundleItemV5<'a, T> {
    fn from_bundle(
        value: &'a T,
        witness: &'a reviewgraphen_core::V5ProjectionEventWitness,
    ) -> Result<Self, ReportError> {
        Ok(Self {
            event_id: &witness.event_id,
            event_sequence: witness.sequence,
            body_hash: ContentHash::sha256(&canonical_json(value)?),
            body: value,
        })
    }
}

/// Caller-controlled presentation metadata for a V5 report.  It deliberately
/// contains neither a source/target run locator nor any authority coordinate:
/// Store derives those only while holding the future dual-run report authority
/// callback.  This prevents a request from substituting a source report,
/// terminal proof, marker, or snapshot for the verified dual-run basis.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportRequestV5 {
    pub report_id: StableId,
    pub target_plan_id: StableId,
    pub selected_obligation_ids: BTreeSet<StableId>,
    pub tool_versions: BTreeMap<String, String>,
}

/// Semantic checks which are intentionally stricter than Draft 2020-12's
/// local object validation.  The source-bound generator invokes this after it
/// has reduced typed durable rows and before it returns canonical bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReportV5SemanticError(&'static str);

impl std::fmt::Display for ReportV5SemanticError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for ReportV5SemanticError {}

pub fn validate_v5_semantics(report: &Value) -> Result<(), ReportV5SemanticError> {
    let object = report
        .as_object()
        .ok_or(ReportV5SemanticError("report must be an object"))?;
    if object.get("schema").and_then(Value::as_str) != Some("reviewgraphen.review.report.v5") {
        return Err(ReportV5SemanticError("wrong V5 report schema"));
    }
    validate_v5_tuple_integrity(object)?;
    validate_v5_source_closure(
        object
            .get("scenario")
            .and_then(Value::as_object)
            .and_then(|scenario| scenario.get("incremental_source_closure"))
            .and_then(Value::as_object)
            .and_then(|tuple| tuple.get("body"))
            .and_then(Value::as_object)
            .ok_or(ReportV5SemanticError(
                "incremental source closure is missing",
            ))?,
    )?;
    let coverage = object
        .get("coverage")
        .and_then(Value::as_object)
        .ok_or(ReportV5SemanticError("coverage is missing"))?;
    let denominator = semantic_id_set(
        coverage,
        "denominator_obligation_ids",
        "coverage denominator is invalid",
    )?;
    if denominator.is_empty() {
        return Err(ReportV5SemanticError("coverage denominator is empty"));
    }
    let scenario = object
        .get("scenario")
        .and_then(Value::as_object)
        .ok_or(ReportV5SemanticError("scenario is missing"))?;
    let selected = semantic_id_set(
        scenario,
        "selected_obligation_ids",
        "scenario selection is invalid",
    )?;
    if denominator != selected {
        return Err(ReportV5SemanticError(
            "coverage denominator does not equal scenario selection",
        ));
    }
    if coverage.get("selected").and_then(Value::as_u64)
        != Some(u64::try_from(denominator.len()).unwrap_or(u64::MAX))
    {
        return Err(ReportV5SemanticError(
            "coverage selected count does not match denominator",
        ));
    }
    if coverage.get("universe_id") != scenario.get("universe_id") {
        return Err(ReportV5SemanticError(
            "coverage universe does not match scenario universe",
        ));
    }
    for (ids_key, count_key) in [
        ("visited_obligation_ids", "visited"),
        ("completed_obligation_ids", "completed"),
        ("evidence_supported_obligation_ids", "evidence_supported"),
        (
            "m5_dependent_successor_obligation_ids",
            "m5_dependent_successors",
        ),
        (
            "required_human_resolution_obligation_ids",
            "required_human_resolutions",
        ),
        (
            "structurally_preserved_obligation_ids",
            "structurally_preserved",
        ),
        ("native_verified_obligation_ids", "native_verified"),
        ("verified_obligation_ids", "verified"),
        ("fresh_verified_obligation_ids", "fresh_verified"),
        ("accepted_obligation_ids", "accepted"),
    ] {
        let values = semantic_id_set(coverage, ids_key, "coverage axis is invalid")?;
        if !values.is_subset(&denominator)
            || coverage.get(count_key).and_then(Value::as_u64)
                != Some(u64::try_from(values.len()).unwrap_or(u64::MAX))
        {
            return Err(ReportV5SemanticError("coverage axis/count mismatch"));
        }
    }
    let native = semantic_id_set(
        coverage,
        "native_verified_obligation_ids",
        "native coverage is invalid",
    )?;
    if native
        != semantic_id_set(
            coverage,
            "verified_obligation_ids",
            "verified coverage is invalid",
        )?
        || native
            != semantic_id_set(
                coverage,
                "fresh_verified_obligation_ids",
                "fresh coverage is invalid",
            )?
    {
        return Err(ReportV5SemanticError(
            "native, verified, and fresh coverage must be exact current-native equality",
        ));
    }
    let views = object
        .get("projection")
        .and_then(Value::as_object)
        .and_then(|projection| projection.get("views"))
        .and_then(Value::as_array)
        .ok_or(ReportV5SemanticError("projection views are missing"))?;
    if views.len() != 3
        || views
            .iter()
            .map(|view| view.get("kind").and_then(Value::as_str))
            .collect::<Vec<_>>()
            != vec![Some("human"), Some("ci"), Some("machine")]
    {
        return Err(ReportV5SemanticError("V5 inherited views are not closed"));
    }
    for view in views {
        let losses = view
            .get("information_loss")
            .and_then(Value::as_array)
            .ok_or(ReportV5SemanticError("view losses are missing"))?;
        let m6_losses = losses
            .iter()
            .filter(|loss| {
                loss.get("kind").and_then(Value::as_str) == Some("omitted_m6_incremental_records")
            })
            .count();
        if m6_losses > 16 {
            return Err(ReportV5SemanticError(
                "more than sixteen M6 losses in one view",
            ));
        }
        for loss in losses {
            validate_v5_loss(loss)?;
        }
    }
    let result = object
        .get("result")
        .and_then(Value::as_object)
        .ok_or(ReportV5SemanticError("result is missing"))?;
    for key in ["partial_rerun_actions", "gluing_rerun_actions"] {
        let actions = result
            .get(key)
            .and_then(Value::as_array)
            .ok_or(ReportV5SemanticError("action array is missing"))?;
        for action in actions {
            validate_v5_action(action)?;
        }
    }
    validate_v5_cardinality_obstructions(
        result
            .get("obstructions")
            .and_then(Value::as_array)
            .ok_or(ReportV5SemanticError("obstruction array is missing"))?,
    )?;
    validate_v5_gate(
        object
            .get("gate")
            .and_then(Value::as_object)
            .ok_or(ReportV5SemanticError("gate is missing"))?,
    )?;
    Ok(())
}

/// Re-check the content-addressed envelope fields of every serialized V5
/// event tuple. Schema validation only checks that these fields have the right
/// shapes; a detached consumer must not trust a retained hash after its body
/// has been edited.
fn validate_v5_tuple_integrity(
    report: &serde_json::Map<String, Value>,
) -> Result<(), ReportV5SemanticError> {
    let mut tuples = Vec::new();
    if let Some(scenario) = report.get("scenario").and_then(Value::as_object) {
        tuples.extend(scenario.values().filter(|row| {
            row.get("body").is_some()
                && row.get("event_id").is_some()
                && row.get("body_hash").is_some()
        }));
    }
    if let Some(result) = report.get("result").and_then(Value::as_object) {
        for value in result.values() {
            match value {
                Value::Array(rows) => tuples.extend(rows.iter().filter(|row| {
                    row.get("body").is_some()
                        && row.get("event_id").is_some()
                        && row.get("body_hash").is_some()
                })),
                Value::Object(row)
                    if row.get("body").is_some()
                        && row.get("event_id").is_some()
                        && row.get("body_hash").is_some() =>
                {
                    tuples.push(value)
                }
                _ => {}
            }
        }
    }
    for tuple in tuples {
        let tuple = tuple
            .as_object()
            .ok_or(ReportV5SemanticError("V5 tuple is not an object"))?;
        let body = tuple
            .get("body")
            .ok_or(ReportV5SemanticError("V5 tuple body is missing"))?;
        let body_schema = body.get("schema").and_then(Value::as_str);
        if !matches!(
            body_schema,
            Some("reviewgraphen.evidence.v3" | "reviewgraphen.human_decision.v3")
        ) {
            continue;
        }
        let event_id = tuple
            .get("event_id")
            .and_then(Value::as_str)
            .ok_or(ReportV5SemanticError("V5 tuple event ID is missing"))?;
        if !event_id.starts_with("event:") {
            return Err(ReportV5SemanticError(
                "V5 tuple event ID has wrong namespace",
            ));
        }
        if tuple
            .get("event_sequence")
            .and_then(Value::as_u64)
            .is_none_or(|sequence| sequence == 0)
        {
            return Err(ReportV5SemanticError("V5 tuple event sequence is invalid"));
        }
        let declared_hash = tuple
            .get("body_hash")
            .and_then(Value::as_str)
            .ok_or(ReportV5SemanticError("V5 tuple body hash is missing"))?;
        let recomputed = ContentHash::sha256(
            &canonical_json(body)
                .map_err(|_| ReportV5SemanticError("V5 tuple body is not canonicalizable"))?,
        )
        .to_string();
        if declared_hash != recomputed {
            return Err(ReportV5SemanticError(
                "V5 tuple body hash does not match body",
            ));
        }
    }
    Ok(())
}

fn validate_v5_cardinality_obstructions(
    obstructions: &[Value],
) -> Result<(), ReportV5SemanticError> {
    for obstruction in obstructions.iter().filter(|value| {
        value.get("kind").and_then(Value::as_str) == Some("m6_claim_cardinality_unsupported")
    }) {
        let source_ids = semantic_sorted_id_array(
            obstruction
                .get("source_ids")
                .ok_or(ReportV5SemanticError("cardinality source IDs are missing"))?,
            "cardinality source IDs are not sorted unique",
        )?;
        let claim_count = source_ids
            .iter()
            .filter(|id| id.starts_with("claim:"))
            .count();
        if claim_count == 1 || claim_count > 16 || source_ids.len() != 7 + claim_count {
            return Err(ReportV5SemanticError(
                "cardinality obstruction source accounting mismatch",
            ));
        }
        let fresh = source_ids
            .iter()
            .filter(|id| id.starts_with("partial-rerun-action-v5:"))
            .count();
        let events = source_ids
            .iter()
            .filter(|id| id.starts_with("event:"))
            .count();
        if !matches!((fresh, events), (1, 1) | (0, 2))
            || obstruction
                .get("blocks")
                .and_then(Value::as_array)
                .is_none_or(|blocks| {
                    blocks.len() != 1
                        || blocks[0]
                            .as_str()
                            .is_none_or(|id| !id.starts_with("obligation:"))
                })
            || !obstruction
                .get("message")
                .and_then(Value::as_str)
                .is_some_and(|message| {
                    message
                        == format!("M6 requires exactly one parsed claim; observed {claim_count}")
                })
        {
            return Err(ReportV5SemanticError(
                "cardinality obstruction is not exact",
            ));
        }
    }
    Ok(())
}

fn semantic_id_set(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
    error: &'static str,
) -> Result<BTreeSet<String>, ReportV5SemanticError> {
    let values = object
        .get(key)
        .and_then(Value::as_array)
        .ok_or(ReportV5SemanticError(error))?;
    let mut ids = BTreeSet::new();
    for value in values {
        let Some(id) = value.as_str() else {
            return Err(ReportV5SemanticError(error));
        };
        if !ids.insert(id.to_owned()) {
            return Err(ReportV5SemanticError(error));
        }
    }
    Ok(ids)
}

fn semantic_sorted_id_array<'a>(
    value: &'a Value,
    error: &'static str,
) -> Result<Vec<&'a str>, ReportV5SemanticError> {
    let values = value.as_array().ok_or(ReportV5SemanticError(error))?;
    let ids = values
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()
        .ok_or(ReportV5SemanticError(error))?;
    if ids.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(ReportV5SemanticError(error));
    }
    Ok(ids)
}

fn validate_v5_loss(loss: &Value) -> Result<(), ReportV5SemanticError> {
    if loss.get("kind").and_then(Value::as_str) != Some("omitted_m6_incremental_records") {
        // Frozen inherited V3/V4 loss contracts retain their own semantics.
        return Ok(());
    }
    let pointer = loss
        .get("recovery_ref")
        .and_then(Value::as_str)
        .ok_or(ReportV5SemanticError("M6 loss pointer is missing"))?;
    let permitted_pointer = [
        "reviewgraphen.review.report.v5#/result/artifact_registrations_v5",
        "reviewgraphen.review.report.v5#/result/program_mappings",
        "reviewgraphen.review.report.v5#/result/obligation_correspondence_entries",
        "reviewgraphen.review.report.v5#/result/historical_record_assessments",
        "reviewgraphen.review.report.v5#/result/gluing_freshness",
        "reviewgraphen.review.report.v5#/result/preservation_evidence",
        "reviewgraphen.review.report.v5#/result/preservation_verifications",
        "reviewgraphen.review.report.v5#/result/partial_rerun_actions",
        "reviewgraphen.review.report.v5#/result/gluing_rerun_actions",
        "reviewgraphen.review.report.v5#/scenario/incremental_source_closure",
        "reviewgraphen.review.report.v5#/scenario/change_morphism",
        "reviewgraphen.review.report.v5#/scenario/obligation_correspondence",
        "reviewgraphen.review.report.v5#/result/staleness_assessment",
        "reviewgraphen.review.report.v5#/result/partial_rerun_plan",
        "reviewgraphen.review.report.v5#/result/gluing_rerun_plan",
        "reviewgraphen.review.report.v5#/gate",
    ]
    .contains(&pointer);
    if !permitted_pointer
        || loss.get("reason").and_then(Value::as_str)
            != Some(&format!("view omits complete records from {pointer}"))
        || semantic_sorted_id_array(
            loss.get("source_ids")
                .ok_or(ReportV5SemanticError("M6 loss source IDs are missing"))?,
            "M6 loss source IDs are not sorted unique",
        )?
        .is_empty()
    {
        return Err(ReportV5SemanticError("M6 loss semantics mismatch"));
    }
    let properties = loss
        .get("affected_properties")
        .and_then(Value::as_array)
        .ok_or(ReportV5SemanticError("M6 loss properties are missing"))?;
    let properties = properties
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()
        .ok_or(ReportV5SemanticError("M6 loss properties are invalid"))?;
    if properties.is_empty() || properties.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(ReportV5SemanticError(
            "M6 loss properties are not sorted unique",
        ));
    }
    Ok(())
}

fn validate_v5_action(action: &Value) -> Result<(), ReportV5SemanticError> {
    let state = action
        .get("state")
        .and_then(Value::as_str)
        .ok_or(ReportV5SemanticError("action state is missing"))?;
    let witnesses = semantic_sorted_id_array(
        action
            .get("completion_witness_ids")
            .ok_or(ReportV5SemanticError("action witnesses are missing"))?,
        "action witnesses are not sorted unique",
    )?;
    let resolved = action
        .get("resolved_reviewer_output_records")
        .and_then(Value::as_array)
        .ok_or(ReportV5SemanticError("action resolved output is missing"))?;
    let action_kind = action
        .get("body")
        .and_then(Value::as_object)
        .and_then(|body| body.get("action"))
        .and_then(Value::as_str)
        .ok_or(ReportV5SemanticError("action kind is missing"))?;
    match (state, action_kind) {
        ("pending", _) if witnesses.is_empty() && resolved.is_empty() => Ok(()),
        ("complete", "rerun_verifier")
            if !witnesses.is_empty() && resolved_reviewer_outputs_are_exact(resolved) =>
        {
            Ok(())
        }
        ("complete", _) if !witnesses.is_empty() && resolved.is_empty() => Ok(()),
        _ => Err(ReportV5SemanticError("action completion state mismatch")),
    }
}

fn resolved_reviewer_outputs_are_exact(records: &[Value]) -> bool {
    records.len() == 2
        && records[0]
            .get("body")
            .and_then(Value::as_object)
            .and_then(|body| body.get("id"))
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with("execution:"))
        && records[1]
            .get("body")
            .and_then(Value::as_object)
            .and_then(|body| body.get("id"))
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with("claim:"))
}

fn validate_v5_source_closure(
    closure: &serde_json::Map<String, Value>,
) -> Result<(), ReportV5SemanticError> {
    fn oid(value: Option<&Value>) -> Option<&str> {
        value.and_then(Value::as_str).filter(|value| {
            value.len() == 40
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    }
    fn tree(value: Option<&Value>) -> Option<&str> {
        value.and_then(Value::as_str).filter(|value| {
            value.len() == 44
                && value.starts_with("git:")
                && value[4..]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
    }
    let source_commit = oid(closure.get("source_resolved_target_commit_oid"))
        .ok_or(ReportV5SemanticError("source commit OID is invalid"))?;
    let target_base_commit = oid(closure.get("target_resolved_base_commit_oid"))
        .ok_or(ReportV5SemanticError("target base commit OID is invalid"))?;
    let target_commit = oid(closure.get("target_resolved_target_commit_oid"))
        .ok_or(ReportV5SemanticError("target commit OID is invalid"))?;
    let source_tree = tree(closure.get("source_target_tree_hash"))
        .ok_or(ReportV5SemanticError("source tree hash is invalid"))?;
    let target_base_tree = tree(closure.get("target_base_tree_hash"))
        .ok_or(ReportV5SemanticError("target base tree hash is invalid"))?;
    let _target_tree = tree(closure.get("target_target_tree_hash"))
        .ok_or(ReportV5SemanticError("target tree hash is invalid"))?;
    closure
        .get("source_event_count")
        .and_then(Value::as_u64)
        .filter(|count| *count > 0)
        .ok_or(ReportV5SemanticError("source event count is invalid"))?;
    closure
        .get("target_predecessor_event_count")
        .and_then(Value::as_u64)
        .filter(|count| *count > 0)
        .ok_or(ReportV5SemanticError(
            "target predecessor event count is invalid",
        ))?;
    if source_commit != target_base_commit
        || source_tree != target_base_tree
        || target_base_commit == target_commit
    {
        return Err(ReportV5SemanticError(
            "source/target Git closure is inconsistent",
        ));
    }
    Ok(())
}

fn validate_v5_gate(gate: &serde_json::Map<String, Value>) -> Result<(), ReportV5SemanticError> {
    let status = gate
        .get("status")
        .and_then(Value::as_str)
        .ok_or(ReportV5SemanticError("gate status is missing"))?;
    let blocking = semantic_sorted_id_array(
        gate.get("blocking_ids")
            .ok_or(ReportV5SemanticError("gate blockers are missing"))?,
        "gate blockers are not sorted unique",
    )?;
    let incomplete = semantic_sorted_id_array(
        gate.get("incomplete_ids")
            .ok_or(ReportV5SemanticError("gate incomplete IDs are missing"))?,
        "gate incomplete IDs are not sorted unique",
    )?;
    let reasons = gate
        .get("reasons")
        .and_then(Value::as_array)
        .ok_or(ReportV5SemanticError("gate reasons are missing"))?;
    let reason_set = reasons
        .iter()
        .map(Value::as_str)
        .collect::<Option<BTreeSet<_>>>()
        .ok_or(ReportV5SemanticError("gate reasons are invalid"))?;
    if reason_set.len() != reasons.len() {
        return Err(ReportV5SemanticError("gate reasons are duplicated"));
    }
    let _ = semantic_sorted_id_array(
        gate.get("source_ids")
            .ok_or(ReportV5SemanticError("gate source IDs are missing"))?,
        "gate source IDs are not sorted unique",
    )?;
    match status {
        "blocked" if !blocking.is_empty() && !reason_set.is_empty() => Ok(()),
        "incomplete" if blocking.is_empty() && !incomplete.is_empty() && !reason_set.is_empty() => {
            Ok(())
        }
        "pass" if blocking.is_empty() && incomplete.is_empty() && reason_set.is_empty() => Ok(()),
        _ => Err(ReportV5SemanticError("gate status/reason set mismatch")),
    }
}

impl ReportRequestV5 {
    fn validate(&self) -> Result<(), ReportV5ReductionError> {
        if self.report_id.kind() != "report"
            || self.target_plan_id.kind() != "plan"
            || self
                .selected_obligation_ids
                .iter()
                .any(|id| id.kind() != "obligation")
            || self.tool_versions.is_empty()
            || self.tool_versions.len() > 64
            || self.tool_versions.iter().any(|(name, version)| {
                name.is_empty() || version.is_empty() || name.len() > 256 || version.len() > 256
            })
        {
            return Err(ReportV5ReductionError::InvalidRequest);
        }
        Ok(())
    }
}

/// Generates a V5 report exclusively from a Store-minted source-bound
/// authority. There is intentionally no overload accepting JSON, an index
/// snapshot, a terminal proof, or source/target run coordinates. The concrete
/// Store owner is the seal: applications cannot construct it, clone it, or
/// recover it from a target-only terminal proof.
pub fn generate_v5(
    authority: reviewgraphen_store::V5TerminalReportAuthority<'_, '_, '_>,
    request: &ReportRequestV5,
) -> Result<crate::GeneratedReport, ReportError> {
    generate_v5_with_limits(authority, request, ReportLimitsV5::default())
}

/// Source-bound equivalent with explicit report limits for deterministic
/// boundary tests. Production callers should use [`generate_v5`].
pub fn generate_v5_with_limits(
    authority: reviewgraphen_store::V5TerminalReportAuthority<'_, '_, '_>,
    request: &ReportRequestV5,
    limits: ReportLimitsV5,
) -> Result<crate::GeneratedReport, ReportError> {
    // These terms were measured while Store held the same dual-session
    // authority that persisted the terminal marker.  Do not replace a missing
    // receipt with zero: that would make the working-set proof a detached
    // report-side assertion.
    let accounting = authority.report_accounting().map_err(|_| {
        ReportError::Source("V5 report requires a persisted dual-session accounting receipt")
    })?;
    let terminal_index = TerminalIndexReceiptCoordinatesV5::from_authority(&authority)?;
    authority
        .with_terminal_rows(|rows| {
            reduce_locked_terminal_rows_v5(rows, request, limits, accounting, &terminal_index)
                .map_err(|error| {
                    reviewgraphen_store::IncrementalSessionError::Authority(
                        report_v5_terminal_reduction_stage(&error),
                    )
                })
        })
        .map_err(|error| match error {
            reviewgraphen_store::IncrementalSessionError::Authority(stage) => {
                ReportError::Source(stage)
            }
            _ => ReportError::Source(
                "V5 report requires complete source-bound typed terminal authority",
            ),
        })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TerminalIndexReceiptCoordinatesV5 {
    run_id: StableId,
    confirmed_offset: u64,
    tail_hash: ContentHash,
    event_count: u64,
    snapshot_hash: ContentHash,
    terminal_proof_id: StableId,
    terminal_proof_hash: ContentHash,
}

impl TerminalIndexReceiptCoordinatesV5 {
    fn from_authority(
        authority: &reviewgraphen_store::V5TerminalReportAuthority<'_, '_, '_>,
    ) -> Result<Self, ReportError> {
        let receipt = authority.terminal_index_rebuild_receipt().map_err(|_| {
            ReportError::Source(
                "V5 report requires the source-bound terminal V6 index rebuild receipt",
            )
        })?;
        Ok(Self {
            run_id: receipt.rebuild.run_id.clone(),
            confirmed_offset: receipt.rebuild.confirmed_offset,
            tail_hash: receipt.rebuild.tail_hash.clone(),
            event_count: receipt.rebuild.event_count,
            snapshot_hash: receipt.rebuild.snapshot_hash.clone(),
            terminal_proof_id: receipt.terminal_proof_id.clone(),
            terminal_proof_hash: receipt.terminal_proof_hash.clone(),
        })
    }

    fn validate_locked_proof(
        &self,
        proof: &reviewgraphen_core::TerminalProofV5,
    ) -> Result<(), ReportError> {
        let proof_bytes = proof.canonical_bytes().map_err(|_| {
            ReportError::Source("locked terminal proof cannot be canonically encoded")
        })?;
        if self.confirmed_offset == 0
            || self.run_id != *proof.run_id()
            || self.tail_hash != *proof.tail_hash()
            || self.event_count != proof.event_count()
            || self.terminal_proof_id != *proof.id()
            || self.terminal_proof_hash != ContentHash::sha256(&proof_bytes)
        {
            return Err(ReportError::Source(
                "terminal V6 rebuild receipt differs from the locked terminal proof or journal",
            ));
        }
        Ok(())
    }
}

fn report_v5_terminal_reduction_stage(error: &ReportError) -> &'static str {
    match error {
        ReportError::Source(stage) => stage,
        ReportError::Incomplete { .. } => "typed V5 report reduction exceeded its declared bound",
        _ => "typed V5 report reduction refused the locked terminal rows",
    }
}

fn report_v5_reduction_error_stage(
    reducer: &'static str,
    error: &ReportV5ReductionError,
) -> &'static str {
    match (reducer, error) {
        ("typed V5 projection", ReportV5ReductionError::UnknownCoverageMember) => {
            "typed V5 projection contains a loss with an unknown obligation"
        }
        ("typed V5 projection", ReportV5ReductionError::InvalidGateSemantics) => {
            "typed V5 projection has inconsistent gate loss semantics"
        }
        ("typed V5 projection", ReportV5ReductionError::InvalidWitnessKind) => {
            "typed V5 projection contains a loss with an invalid source witness kind"
        }
        (_, ReportV5ReductionError::EmptyDenominator) => {
            "typed V5 reduction has an empty accepted obligation denominator"
        }
        (_, ReportV5ReductionError::UnknownCoverageMember) => {
            "typed V5 reduction names an obligation outside its accepted denominator"
        }
        (_, ReportV5ReductionError::InvalidCoverageAxis(axis)) => match *axis {
            "selected" => "typed V5 reduction has an invalid durable plan selection axis",
            "completed" => "typed V5 reduction has an invalid completed-obligation axis",
            "verified" | "fresh_verified" => {
                "typed V5 reduction has an invalid fresh verification axis"
            }
            "accepted" => "typed V5 reduction has an invalid human-accepted finding axis",
            _ => "typed V5 reduction has an invalid named coverage axis",
        },
        (_, ReportV5ReductionError::InvalidWitnessKind) => {
            "typed V5 reduction contains an invalid event witness kind"
        }
        (_, ReportV5ReductionError::InvalidGateSemantics) => {
            "typed V5 reduction has inconsistent incremental gate semantics"
        }
        (_, ReportV5ReductionError::TooManyRecords) => {
            "typed V5 reduction exceeded its bounded record cardinality"
        }
        (_, ReportV5ReductionError::InvalidRequest) => {
            "typed V5 reduction rejected the durable plan/request binding"
        }
        (_, ReportV5ReductionError::Core(_)) => {
            "typed V5 reduction refused a Core-owned projection invariant"
        }
    }
}

/// Inclusive V5-only report limits from ADR-0023 §14.  These are intentionally
/// separate from the frozen V2/V3/V4 limits: a V5 report retains the complete
/// inherited V4 shape *and* accounts for the incremental families below.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReportLimitsV5 {
    pub rows: u64,
    pub program_mappings: u64,
    pub correspondence_entries: u64,
    pub historical_assessments: u64,
    pub artifact_registrations_v5: u64,
    pub preservation_evidence: u64,
    pub preservation_verifications: u64,
    pub partial_rerun_actions: u64,
    pub gluing_rerun_actions: u64,
    pub cardinality_obstructions: u64,
    pub gluing_rerun_plans: u64,
    pub gluing_freshness: u64,
    pub information_loss_records: u64,
    pub views: u64,
    pub canonical_bytes: u64,
    pub working_bytes: u64,
}

impl Default for ReportLimitsV5 {
    fn default() -> Self {
        Self {
            rows: 262_144,
            program_mappings: 8_192,
            correspondence_entries: 4_096,
            historical_assessments: 8_192,
            artifact_registrations_v5: 4_096,
            preservation_evidence: 2_048,
            preservation_verifications: 2_048,
            partial_rerun_actions: 4_096,
            gluing_rerun_actions: 5,
            cardinality_obstructions: 2_048,
            gluing_rerun_plans: 1,
            gluing_freshness: 1,
            information_loss_records: 4_096,
            views: 3,
            canonical_bytes: 134_217_728,
            working_bytes: 536_870_912,
        }
    }
}

/// Counts which are new at the M6 report layer. `inherited_target_v4_rows`
/// is computed by the frozen V4 builder from the same target authority; it is
/// deliberately not reconstructed from report JSON.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReportCountsV5 {
    pub inherited_target_v4_rows: u64,
    pub artifact_registrations_v5: u64,
    pub program_mappings: u64,
    pub correspondence_entries: u64,
    pub historical_assessments: u64,
    pub gluing_freshness: u64,
    pub preservation_evidence: u64,
    pub preservation_verifications: u64,
    pub partial_rerun_actions: u64,
    pub cardinality_obstructions: u64,
    pub gluing_rerun_actions: u64,
    pub gluing_rerun_plan_present: u64,
    /// Exactly the 16 M6 locations times at most three inherited views, plus
    /// any frozen inherited loss rows. It is independently bounded here.
    pub information_loss_records: u64,
    pub views: u64,
}

impl ReportCountsV5 {
    fn checked_rows(self, limit: u64) -> Result<u64, ReportError> {
        [
            self.inherited_target_v4_rows,
            self.artifact_registrations_v5,
            self.program_mappings,
            self.correspondence_entries,
            self.historical_assessments,
            self.gluing_freshness,
            self.preservation_evidence,
            self.preservation_verifications,
            self.partial_rerun_actions,
            self.cardinality_obstructions,
            self.gluing_rerun_actions,
            self.gluing_rerun_plan_present,
            // §14's six fixed singleton report records: closure, morphism,
            // correspondence seal, staleness seal, partial plan, and gate.
            6,
        ]
        .into_iter()
        .try_fold(0_u64, |total, value| {
            total.checked_add(value).ok_or(ReportError::Incomplete {
                operation: "report_rows_v5",
                limit,
                observed: u64::MAX,
            })
        })
    }
}

/// Explicit dual-session measurements used by the §14 V5 peak formula.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReportAccountingV5 {
    pub source_journal_bytes: u64,
    pub target_journal_bytes: u64,
    pub source_index_bytes: u64,
    pub target_index_bytes: u64,
    pub source_index_owned_bytes: u64,
    pub target_index_owned_bytes: u64,
    pub source_cas_bytes: u64,
    pub target_cas_bytes: u64,
    pub source_event_line_bytes: u64,
    pub target_event_line_bytes: u64,
    pub mapping_reservation_bytes: u64,
    pub reserved_report_bytes: u64,
    pub realized_report_bytes: u64,
    pub largest_record_bytes: u64,
    pub canonical_report_bytes: u64,
}

impl ReportLimitsV5 {
    pub fn preflight(self, counts: ReportCountsV5) -> Result<(), ReportError> {
        for (operation, observed, limit) in [
            (
                "program_mappings",
                counts.program_mappings,
                self.program_mappings,
            ),
            (
                "obligation_correspondence_entries",
                counts.correspondence_entries,
                self.correspondence_entries,
            ),
            (
                "historical_record_assessments",
                counts.historical_assessments,
                self.historical_assessments,
            ),
            (
                "artifact_registrations_v5",
                counts.artifact_registrations_v5,
                self.artifact_registrations_v5,
            ),
            (
                "preservation_evidence",
                counts.preservation_evidence,
                self.preservation_evidence,
            ),
            (
                "preservation_verifications",
                counts.preservation_verifications,
                self.preservation_verifications,
            ),
            (
                "partial_rerun_actions",
                counts.partial_rerun_actions,
                self.partial_rerun_actions,
            ),
            (
                "gluing_rerun_actions",
                counts.gluing_rerun_actions,
                self.gluing_rerun_actions,
            ),
            (
                "m6_claim_cardinality_obstructions",
                counts.cardinality_obstructions,
                self.cardinality_obstructions,
            ),
            (
                "gluing_rerun_plan_present",
                counts.gluing_rerun_plan_present,
                self.gluing_rerun_plans,
            ),
            (
                "gluing_freshness",
                counts.gluing_freshness,
                self.gluing_freshness,
            ),
            (
                "information_loss_records_v5",
                counts.information_loss_records,
                self.information_loss_records,
            ),
            ("views_v5", counts.views, self.views),
        ] {
            if observed > limit {
                return Err(ReportError::Incomplete {
                    operation,
                    limit,
                    observed,
                });
            }
        }
        let observed = counts.checked_rows(self.rows)?;
        if observed > self.rows {
            return Err(ReportError::Incomplete {
                operation: "report_rows_v5",
                limit: self.rows,
                observed,
            });
        }
        Ok(())
    }

    pub fn check_accounting(self, accounting: ReportAccountingV5) -> Result<(), ReportError> {
        let base = [
            accounting.source_journal_bytes,
            accounting.target_journal_bytes,
            accounting.source_index_bytes,
            accounting.target_index_bytes,
            accounting.source_index_owned_bytes,
            accounting.target_index_owned_bytes,
            accounting.source_cas_bytes,
            accounting.target_cas_bytes,
            accounting.source_event_line_bytes,
            accounting.target_event_line_bytes,
            accounting.mapping_reservation_bytes,
        ]
        .into_iter()
        .try_fold(0_u64, |total, value| total.checked_add(value))
        .ok_or(ReportError::Incomplete {
            operation: "report_v5_working_bytes",
            limit: self.working_bytes,
            observed: u64::MAX,
        })?;
        let projection =
            base.checked_add(accounting.reserved_report_bytes)
                .ok_or(ReportError::Incomplete {
                    operation: "report_v5_working_bytes",
                    limit: self.working_bytes,
                    observed: u64::MAX,
                })?;
        let serialization = base
            .checked_add(accounting.realized_report_bytes)
            .and_then(|value| value.checked_add(accounting.largest_record_bytes))
            .and_then(|value| value.checked_add(accounting.canonical_report_bytes))
            .ok_or(ReportError::Incomplete {
                operation: "report_v5_working_bytes",
                limit: self.working_bytes,
                observed: u64::MAX,
            })?;
        if accounting.canonical_report_bytes > self.canonical_bytes {
            return Err(ReportError::Incomplete {
                operation: "report_v5_canonical_bytes",
                limit: self.canonical_bytes,
                observed: accounting.canonical_report_bytes,
            });
        }
        let observed = projection.max(serialization);
        if observed > self.working_bytes {
            return Err(ReportError::Incomplete {
                operation: "report_v5_working_bytes",
                limit: self.working_bytes,
                observed,
            });
        }
        Ok(())
    }
}

/// First stage of the Store→Report V5 reducer.  It deliberately accepts the
/// concrete Store callback rows rather than an index-record enum or JSON
/// value.  The source M4/M5 baseline and target terminal proof have already
/// been checked by Store while both locks are live; this layer checks that the
/// typed terminal collection still carries the required durable M6 seals.
///
/// The current Store callback supplies M6 DTOs but no Core-owned, complete
/// target inherited DTO collection. Refusing that shape is intentional: the
/// report cannot substitute source rows, `V5TypedProjectionRecord` metadata,
/// or a target-only proof for target facts. The next Store contract must pass
/// a strict replay-owned collection with every inherited decoded DTO and its
/// `V5ProjectionEventWitness`; this is the single adapter seam that will
/// reduce that collection to coverage, losses, gate, and the final document.
fn reduce_locked_terminal_rows_v5(
    rows: reviewgraphen_store::V5TerminalReportRows<'_>,
    request: &ReportRequestV5,
    limits: ReportLimitsV5,
    accounting: reviewgraphen_store::V5TerminalReportAccounting,
    terminal_index: &TerminalIndexReceiptCoordinatesV5,
) -> Result<crate::GeneratedReport, ReportError> {
    request
        .validate()
        .map_err(|_| ReportError::Source("invalid V5 report request"))?;
    terminal_index.validate_locked_proof(rows.terminal_proof)?;
    if rows.source_v4.gluing_input_descriptors.len() != 2
        || rows.source_v4.artifact_registrations_v4.len() != 2
        || rows.source_v4.gluing_attempts.len() != 1
    {
        return Err(ReportError::Source(
            "locked source does not contain the complete V4 M5 baseline",
        ));
    }
    let mut closure = 0_u8;
    let mut morphism = 0_u8;
    let mut correspondence = 0_u8;
    let mut staleness = 0_u8;
    let mut partial_plan = 0_u8;
    let mut terminal_marker = 0_u8;
    for row in rows.target_v5 {
        use reviewgraphen_core::V5TypedProjectionRecord as Row;
        match row {
            Row::IncrementalSourceBoundV5 { value, .. }
                if value.id() == rows.terminal_proof.source_closure_id() =>
            {
                closure += 1
            }
            Row::ChangeMorphismSealedV5 { .. } => morphism += 1,
            Row::ObligationCorrespondenceSealedV5 { .. } => correspondence += 1,
            Row::StalenessAssessmentSealedV5 { .. } => staleness += 1,
            Row::PartialRerunPlanSealedV5 { value, .. }
                if value.id() == rows.terminal_proof.partial_rerun_plan_id() =>
            {
                partial_plan += 1
            }
            Row::TerminalCompletedV5 { value, .. }
                if value.source_closure_id == *rows.terminal_proof.source_closure_id()
                    && value.partial_rerun_plan_id
                        == *rows.terminal_proof.partial_rerun_plan_id()
                    && value.target_plan_id == *rows.terminal_proof.target_plan_id() =>
            {
                terminal_marker += 1;
            }
            _ => {}
        }
    }
    if (
        closure,
        morphism,
        correspondence,
        staleness,
        partial_plan,
        terminal_marker,
    ) != (1, 1, 1, 1, 1, 1)
    {
        return Err(ReportError::Source(
            "locked target terminal has incomplete or duplicate M6 authority seals",
        ));
    }
    reduce_complete_inherited_target_rows_v5(rows, request, limits, accounting, terminal_index)
}

/// The only adapter path allowed once Store supplies Core's replay-owned
/// inherited bodies.  This deliberately takes the strict projection enum,
/// never `Value`, a SQLite row, or a raw payload.  Keeping the selection here
/// makes an absent V4 gluing bundle a source-bound refusal instead of silently
/// producing a report whose inherited `result` section is incomplete.
fn reduce_complete_inherited_target_rows_v5(
    rows: reviewgraphen_store::V5TerminalReportRows<'_>,
    request: &ReportRequestV5,
    limits: ReportLimitsV5,
    store_accounting: reviewgraphen_store::V5TerminalReportAccounting,
    terminal_index: &TerminalIndexReceiptCoordinatesV5,
) -> Result<crate::GeneratedReport, ReportError> {
    use reviewgraphen_core::V5InheritedReportProjection as Row;
    let mut families = InheritedTargetFamiliesV5::default();
    let mut inherited = TypedInheritedResultV5::default();
    for row in rows.target_inherited {
        match row {
            Row::RunGenesisManifest { .. } => families.genesis += 1,
            Row::ArtifactRegistered {
                witness,
                value,
                source_ids,
            } => {
                families.artifact_registrations += 1;
                record_completion_sources_v5(&mut inherited, source_ids, witness);
                inherited
                    .artifact_registrations
                    .push(TypedReportEventTupleV5::from_witness(value, witness));
            }
            Row::ContextEnvelopeProjected {
                witness,
                value,
                source_ids,
            } => {
                families.context_envelopes += 1;
                record_completion_sources_v5(&mut inherited, source_ids, witness);
                inherited
                    .context_envelopes
                    .push(TypedReportEventTupleV5::from_witness(value, witness));
            }
            Row::ObligationTransition {
                witness,
                obligation_id,
                next,
                ..
            } => {
                families.transitions += 1;
                inherited.transitions.push(TypedObligationTransitionV5 {
                    witness,
                    obligation_id,
                    next,
                });
            }
            Row::ReviewExecutionRecorded {
                witness,
                execution,
                claims,
                source_ids,
            } => {
                families.executions += 1;
                record_completion_sources_v5(&mut inherited, source_ids, witness);
                inherited.executions.push(TypedExecutionWithClaimsV5 {
                    witness,
                    execution,
                    claims,
                    source_ids,
                });
            }
            Row::EvidenceRecordedV3 {
                witness,
                value,
                source_ids,
            } => {
                families.evidence += 1;
                record_completion_sources_v5(&mut inherited, source_ids, witness);
                inherited
                    .evidence
                    .push(TypedReportEventTupleV5::from_witness(value, witness));
            }
            Row::EvidenceBoundV3 {
                witness,
                value,
                source_ids,
            } => {
                families.bindings += 1;
                record_completion_sources_v5(&mut inherited, source_ids, witness);
                inherited
                    .bindings
                    .push(TypedReportEventTupleV5::from_witness(value, witness));
            }
            Row::VerificationRecordedV3 {
                witness,
                value,
                source_ids,
            } => {
                families.verifications += 1;
                record_completion_sources_v5(&mut inherited, source_ids, witness);
                inherited
                    .verifications
                    .push(TypedReportEventTupleV5::from_witness(value, witness));
            }
            Row::DecisionRecordedV3 {
                witness,
                value,
                source_ids,
            } => {
                families.decisions += 1;
                record_completion_sources_v5(&mut inherited, source_ids, witness);
                inherited
                    .decisions
                    .push(TypedReportEventTupleV5::from_witness(value, witness));
            }
            Row::FindingRecordedV3 {
                witness,
                value,
                source_ids,
            } => {
                families.findings += 1;
                record_completion_sources_v5(&mut inherited, source_ids, witness);
                inherited
                    .findings
                    .push(TypedReportEventTupleV5::from_witness(value, witness));
            }
            Row::ArtifactRegisteredV4 {
                witness,
                value,
                source_ids,
            } => {
                families.v4_registrations += 1;
                record_completion_sources_v5(&mut inherited, source_ids, witness);
                inherited
                    .v4_registrations
                    .push(TypedReportEventTupleV5::from_witness(value, witness));
            }
            Row::GluingInputDescriptorV4 {
                witness,
                value,
                source_ids,
            } => {
                record_completion_sources_v5(&mut inherited, source_ids, witness);
                inherited
                    .gluing_descriptors
                    .push(TypedReportEventTupleV5::from_witness(value, witness));
            }
            Row::ReviewPlanRecorded { value, .. }
                if value.id() == rows.terminal_proof.target_plan_id() =>
            {
                families.target_plan += 1;
                inherited.target_plan = Some(value);
            }
            Row::ReviewPlanRecorded { .. } => {
                return Err(ReportError::Source(
                    "target inherited DTO projection contains a non-terminal-plan review plan",
                ));
            }
            Row::SnapshotSourcesRecorded { .. } => families.snapshot_sources += 1,
            Row::GluingBundleRecordedV4 {
                witness,
                value,
                source_ids,
            } => {
                families.m5_bundle += 1;
                record_completion_sources_v5(&mut inherited, source_ids, witness);
                inherited.m5_bundle = Some(TypedM5BundleV5::from_bundle(value, witness)?);
            }
        }
    }
    if families.genesis != 1 || families.target_plan != 1 || families.snapshot_sources != 1 {
        return Err(ReportError::Source(
            "locked target inherited DTO projection is incomplete or ambiguous",
        ));
    }
    let terminal_requires_gluing = rows
        .target_v5
        .iter()
        .find_map(|row| match row {
            reviewgraphen_core::V5TypedProjectionRecord::TerminalCompletedV5 { value, .. } => {
                Some(value.gluing_required)
            }
            _ => None,
        })
        .ok_or(ReportError::Source(
            "terminal marker is absent from target projection",
        ))?;
    if terminal_requires_gluing && families.m5_bundle != 1 {
        return Err(ReportError::Source(
            "locked target inherited DTO projection lacks the required M5 bundle",
        ));
    }
    let mut m6 = TerminalM6ReportRowsV5::default();
    for row in rows.target_v5 {
        use reviewgraphen_core::V5TypedProjectionRecord as Row;
        match row {
            Row::IncrementalSourceBoundV5 { witness, value, .. } => {
                m6.closure = Some((witness, value));
            }
            Row::ChangeMorphismSealedV5 { witness, value, .. } => {
                m6.morphism = Some((witness, value));
            }
            Row::ObligationCorrespondenceSealedV5 { witness, value, .. } => {
                m6.correspondence = Some((witness, value));
            }
            Row::StalenessAssessmentSealedV5 { witness, value, .. } => {
                m6.staleness = Some((witness, value));
            }
            Row::PartialRerunPlanSealedV5 { witness, value, .. } => {
                m6.partial_plan = Some((witness, value));
            }
            Row::ProgramMappingRecordedV5 { witness, value, .. } => {
                m6.mappings.push((witness, value))
            }
            Row::ObligationCorrespondenceEntryRecordedV5 { witness, value, .. } => {
                m6.correspondence_entries.push((witness, value))
            }
            Row::HistoricalRecordAssessedV5 { witness, value, .. } => {
                m6.historical.push((witness, value))
            }
            Row::GluingFreshnessRecordedV5 { witness, value, .. } => {
                m6.gluing_freshness.push((witness, value))
            }
            Row::ArtifactRegisteredV5 { witness, value, .. } => {
                m6.artifact_registrations.push((witness, value))
            }
            Row::PreservationVerifiedV5 {
                witness,
                evidence,
                verification,
                ..
            } => {
                m6.preservation_evidence.push((witness, evidence));
                m6.preservation_verifications.push((witness, verification));
            }
            Row::PartialRerunActionRecordedV5 { witness, value, .. } => {
                m6.partial_actions.push((witness, value));
            }
            Row::GluingRerunActionRecordedV5 { witness, value, .. } => {
                m6.gluing_actions.push((witness, value));
            }
            Row::GluingRerunPlanSealedV5 { witness, value, .. } => {
                m6.gluing_plan = Some((witness, value));
            }
            _ => {}
        }
    }
    let (closure_witness, closure) = m6
        .closure
        .ok_or(ReportError::Source("target closure DTO missing"))?;
    let (morphism_witness, morphism) = m6
        .morphism
        .ok_or(ReportError::Source("target morphism DTO missing"))?;
    let (correspondence_witness, correspondence) = m6
        .correspondence
        .ok_or(ReportError::Source("target correspondence DTO missing"))?;
    let (staleness_witness, staleness) = m6
        .staleness
        .ok_or(ReportError::Source("target staleness DTO missing"))?;
    let (partial_witness, partial_plan) = m6
        .partial_plan
        .ok_or(ReportError::Source("target partial plan DTO missing"))?;
    if request.target_plan_id != *rows.terminal_proof.target_plan_id() {
        return Err(ReportError::Source(
            "V5 request target plan is not the terminal target plan",
        ));
    }
    let target_plan = inherited.target_plan.ok_or(ReportError::Source(
        "locked target inherited DTO projection lacks the durable terminal plan",
    ))?;
    let durable_selected = target_plan
        .waves()
        .iter()
        .flat_map(|wave| wave.obligation_ids().iter().cloned())
        .collect::<BTreeSet<_>>();
    require_exact_plan_selection_v5(&request.selected_obligation_ids, &durable_selected).map_err(
        |_| {
            ReportError::Source(
                "V5 request selected obligations must exactly equal the durable target plan waves",
            )
        },
    )?;
    let closure_projection = closure.report_projection();
    let denominator = rows.target_facts.universe.obligation_ids().clone();
    if !durable_selected.is_subset(&denominator) {
        return Err(ReportError::Source(
            "V5 selected obligation is outside target denominator",
        ));
    }
    let program_space_ref = StableId::parse(format!(
        "program-space:{}",
        rows.target_facts.program_space.snapshot_id()
    ))?;
    let current_m5_ids = inherited
        .v4_registrations
        .iter()
        .map(|row| row.body.id().clone())
        .chain(inherited.m5_bundle.iter().flat_map(|bundle| {
            std::iter::once(bundle.cover.body.id().clone())
                .chain(bundle.sections.iter().map(|row| row.body.id().clone()))
                .chain(bundle.restrictions.iter().map(|row| row.body.id().clone()))
                .chain(std::iter::once(bundle.attempt.body.id().clone()))
                .chain(bundle.candidate.iter().map(|row| row.body.id().clone()))
                .chain(bundle.obstruction.iter().map(|row| row.body.id().clone()))
        }))
        .collect::<BTreeSet<_>>();
    let live_gate = derive_live_gate_axes_v5(
        &inherited,
        &m6,
        &denominator,
        &durable_selected,
        partial_plan,
    )?;
    let gate = reduce_incremental_gate_v5(&GateInputV5 {
        source_closure_id: closure.id().clone(),
        change_morphism_id: morphism.id().clone(),
        obligation_correspondence_id: correspondence.id().clone(),
        staleness_assessment_id: staleness.id().clone(),
        partial_rerun_plan_id: partial_plan.id().clone(),
        gluing_rerun_plan_id: m6.gluing_plan.map(|(_, value)| value.id().clone()),
        native_passes: live_gate.native_passes.clone(),
        current_findings: live_gate.current_findings.clone(),
        current_m5_ids,
        extraction: ExtractionWitnessesV5 {
            snapshot_id: rows.target_facts.program_space.snapshot_id().clone(),
            program_space_id: program_space_ref.clone(),
            universe_id: rows.target_facts.universe.id().clone(),
            plan_id: request.target_plan_id.clone(),
            limitation_ids: rows.target_facts.universe.limitation_ids().clone(),
        },
        required_fresh_obligation_ids: live_gate.required_fresh_obligation_ids,
        fresh_verified_obligation_ids: live_gate.fresh_verified_obligation_ids,
        target_gluing_assignment_conflict_ids: live_gate.target_gluing_assignment_conflict_ids,
        pending_rerun_action_ids: live_gate.pending_rerun_action_ids,
        unresolved_mapping_ids: live_gate.unresolved_mapping_ids,
        unsupported_impact_ids: live_gate.unsupported_impact_ids,
        target_gluing_missing_ids: live_gate.target_gluing_missing_ids,
        target_gluing_incomplete_ids: live_gate.target_gluing_incomplete_ids,
        gluing_plan_missing_scope_id: live_gate.gluing_plan_missing_scope_id,
        human_resolution_missing_ids: live_gate.human_resolution_missing_ids,
        cardinality_obstructions: live_gate.cardinality_obstructions.clone(),
        extraction_incomplete_ids: rows.target_facts.universe.limitation_ids().clone(),
    })
    .map_err(|error| {
        ReportError::Source(report_v5_reduction_error_stage("typed V5 gate", &error))
    })?;
    let completed = inherited
        .transitions
        .iter()
        .filter_map(|transition| {
            (*transition.next == reviewgraphen_core::ObligationLifecycle::Completed)
                .then_some(transition.obligation_id.clone())
        })
        .filter(|id| durable_selected.contains(id))
        .collect::<BTreeSet<_>>();
    // A lifecycle transition is necessary but not sufficient for the public
    // terminal status. Every sealed action must have its own later typed
    // completion receipt; this keeps abstention and malformed reviewer paths
    // partial even though the terminal marker is durable.
    let all_actions_completed = m6
        .partial_actions
        .iter()
        .all(|(_, action)| action_completion_v5(action, &inherited).is_some());
    let status = if completed == durable_selected && all_actions_completed {
        "completed"
    } else {
        "partial"
    };
    let artifact_registration_ids = m6
        .artifact_registrations
        .iter()
        .map(|(_, value)| value.id().clone())
        .collect::<Vec<_>>();
    let preservation_registration_ids = m6
        .artifact_registrations
        .iter()
        .filter(|(_, value)| {
            matches!(
                value.source().role(),
                reviewgraphen_core::PreservationArtifactRoleV5::Output
            )
        })
        .map(|(_, value)| value.id().clone())
        .collect::<Vec<_>>();
    let context_cover_ids = inherited
        .m5_bundle
        .iter()
        .map(|bundle| bundle.cover.body.id().clone())
        .collect::<Vec<_>>();
    let native_coverage = live_gate
        .native_passes
        .iter()
        .map(|value| value.obligation_id.clone())
        .filter(|id| durable_selected.contains(id))
        .collect::<BTreeSet<_>>();
    let accepted_coverage = live_gate
        .current_findings
        .iter()
        .filter(|finding| finding.outcome == CurrentFindingOutcomeV5::AcceptedIssue)
        .map(|finding| finding.native.obligation_id.clone())
        .filter(|id| durable_selected.contains(id))
        .collect::<BTreeSet<_>>();
    let required_human_coverage = m6
        .partial_actions
        .iter()
        .filter(|(_, action)| {
            action.action() == reviewgraphen_core::PartialRerunActionKindV5::RerunHumanDecision
        })
        .flat_map(|(_, action)| action.subject_ids().iter().cloned())
        .filter(|id| durable_selected.contains(id))
        .collect::<BTreeSet<_>>();
    let coverage = reduce_coverage_v5(&CoverageInputV5 {
        universe_id: rows.target_facts.universe.id().clone(),
        denominator: denominator.clone(),
        selected: durable_selected.clone(),
        visited: inherited
            .transitions
            .iter()
            .map(|transition| transition.obligation_id.clone())
            .filter(|id| durable_selected.contains(id))
            .collect(),
        completed,
        evidence_supported: native_coverage.clone(),
        m5_dependent_successors: q_target_m5_dependents_v5(
            &rows.target_facts.obligations,
            &durable_selected,
            partial_plan.target_gluing_required(),
        ),
        required_human_resolutions: required_human_coverage,
        structurally_preserved: m6
            .preservation_verifications
            .iter()
            .map(|(_, verification)| verification.target_obligation_id().clone())
            .filter(|id| durable_selected.contains(id))
            .collect(),
        native_verified: native_coverage.clone(),
        verified: native_coverage.clone(),
        fresh_verified: native_coverage,
        accepted: accepted_coverage,
    })
    .map_err(|error| {
        ReportError::Source(report_v5_reduction_error_stage("typed V5 coverage", &error))
    })?;
    let accepted_properties = accepted_obligation_properties_v5(
        &rows.source_v4.obligations,
        &rows.target_facts.obligations,
    )
    .map_err(|error| {
        ReportError::Source(report_v5_reduction_error_stage(
            "typed V5 accepted obligation properties",
            &error,
        ))
    })?;
    let document = TerminalReportDocumentV5 {
        schema: "reviewgraphen.review.report.v5",
        report_type: "review",
        report_version: 5,
        metadata: terminal_metadata_v5(
            rows.terminal_proof,
            request,
            &rows.target_facts.program_space,
            &closure_projection,
            terminal_index,
        ),
        scenario: TerminalScenarioV5 {
            repository_id: rows.target_facts.program_space.repository_id(),
            snapshot_id: rows.target_facts.program_space.snapshot_id(),
            program_space_ref: &program_space_ref,
            universe_id: rows.target_facts.universe.id(),
            plan_id: &request.target_plan_id,
            selected_obligation_ids: &durable_selected,
            artifact_registration_ids,
            preservation_registration_ids,
            context_cover_ids,
            closure: TypedReportEventTupleV5::from_witness(closure, closure_witness),
            morphism: TypedReportEventTupleV5::from_witness(morphism, morphism_witness),
            correspondence: TypedReportEventTupleV5::from_witness(
                correspondence,
                correspondence_witness,
            ),
        },
        result: terminal_result_v5(
            status,
            &inherited,
            &m6,
            &live_gate.cardinality_obstructions,
            closure.id(),
            partial_plan.id(),
            staleness,
            staleness_witness,
            partial_plan,
            partial_witness,
        ),
        coverage,
        projection: TerminalProjectionV5::new(
            status,
            closure.id(),
            &inherited,
            &m6,
            TerminalProjectionObligationAuthorityV5 {
                target_obligations: &rows.target_facts.obligations,
                accepted_properties: &accepted_properties,
            },
            &denominator,
            &gate,
        )
        .map_err(|error| {
            ReportError::Source(report_v5_reduction_error_stage(
                "typed V5 projection",
                &error,
            ))
        })?,
        gate,
    };
    let bytes = canonical_json(&document)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ReportError::Json)?;
    validate_v5_semantics(&value)
        .map_err(|_| ReportError::Source("typed terminal V5 report violates semantic contract"))?;
    let canonical_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let inherited_target_v4_rows = families.artifact_registrations
        + families.context_envelopes
        + families.executions
        + families.evidence
        + families.bindings
        + families.verifications
        + families.decisions
        + families.findings
        + families.v4_registrations
        + u64::from(families.m5_bundle);
    let loss_locations = [
        false, // proof-locked source closure is referenced by ID in every view.
        m6.artifact_registrations.is_empty(),
        m6.mappings.is_empty(),
        m6.correspondence_entries.is_empty(),
        m6.historical.is_empty(),
        m6.gluing_freshness.is_empty(),
        m6.preservation_evidence.is_empty(),
        m6.preservation_verifications.is_empty(),
        m6.partial_actions.is_empty(),
        m6.gluing_actions.is_empty(),
        false, // morphism is a required terminal seal and is not embedded in a view.
        false, // correspondence is a required terminal seal and is not embedded in a view.
        false, // staleness is a required terminal seal and is not embedded in a view.
        false, // partial plan is a required terminal seal and is not embedded in a view.
        m6.gluing_plan.is_none(),
    ]
    .into_iter()
    .filter(|empty| !empty)
    .count();
    let information_loss_records =
        u64::try_from(loss_locations.saturating_mul(3)).unwrap_or(u64::MAX);
    limits.preflight(ReportCountsV5 {
        inherited_target_v4_rows,
        artifact_registrations_v5: u64::try_from(m6.artifact_registrations.len())
            .unwrap_or(u64::MAX),
        program_mappings: u64::try_from(m6.mappings.len()).unwrap_or(u64::MAX),
        correspondence_entries: u64::try_from(m6.correspondence_entries.len()).unwrap_or(u64::MAX),
        historical_assessments: u64::try_from(m6.historical.len()).unwrap_or(u64::MAX),
        gluing_freshness: u64::try_from(m6.gluing_freshness.len()).unwrap_or(u64::MAX),
        preservation_evidence: u64::try_from(m6.preservation_evidence.len()).unwrap_or(u64::MAX),
        preservation_verifications: u64::try_from(m6.preservation_verifications.len())
            .unwrap_or(u64::MAX),
        partial_rerun_actions: u64::try_from(m6.partial_actions.len()).unwrap_or(u64::MAX),
        cardinality_obstructions: u64::try_from(live_gate.cardinality_obstructions.len())
            .unwrap_or(u64::MAX),
        gluing_rerun_actions: u64::try_from(m6.gluing_actions.len()).unwrap_or(u64::MAX),
        gluing_rerun_plan_present: u64::from(m6.gluing_plan.is_some()),
        information_loss_records,
        views: 3,
    })?;
    let report_accounting = ReportAccountingV5 {
        source_journal_bytes: store_accounting.source_journal_bytes,
        target_journal_bytes: store_accounting.target_journal_bytes,
        source_index_bytes: store_accounting.source_index_bytes,
        target_index_bytes: store_accounting.target_index_bytes,
        source_index_owned_bytes: store_accounting.source_index_owned_bytes,
        target_index_owned_bytes: store_accounting.target_index_owned_bytes,
        source_cas_bytes: store_accounting.source_cas_bytes,
        target_cas_bytes: store_accounting.target_cas_bytes,
        source_event_line_bytes: store_accounting.source_event_line_bytes,
        target_event_line_bytes: store_accounting.target_event_line_bytes,
        mapping_reservation_bytes: store_accounting.mapping_reservation_bytes,
        // `canonical_json` retains exactly one generated document buffer.
        // Its realized length and allocation capacity are the actual report
        // terms for this live authority, not test-only placeholders.
        reserved_report_bytes: u64::try_from(bytes.capacity()).unwrap_or(u64::MAX),
        realized_report_bytes: canonical_bytes,
        largest_record_bytes: canonical_bytes,
        canonical_report_bytes: canonical_bytes,
    };
    limits.check_accounting(report_accounting)?;
    Ok(crate::GeneratedReport {
        canonical_bytes: bytes,
        accounting: crate::ReportAccounting {
            canonical_report_bytes: canonical_bytes,
            ..crate::ReportAccounting::default()
        },
    })
}

#[derive(Default)]
struct TypedInheritedResultV5<'a> {
    artifact_registrations:
        Vec<TypedReportEventTupleV5<'a, reviewgraphen_core::ArtifactRegisteredV3>>,
    context_envelopes: Vec<TypedReportEventTupleV5<'a, reviewgraphen_core::ReviewContextEnvelope>>,
    target_plan: Option<&'a reviewgraphen_core::ReviewPlan>,
    transitions: Vec<TypedObligationTransitionV5<'a>>,
    executions: Vec<TypedExecutionWithClaimsV5<'a>>,
    evidence: Vec<TypedReportEventTupleV5<'a, reviewgraphen_core::EvidenceV3>>,
    bindings: Vec<TypedReportEventTupleV5<'a, reviewgraphen_core::EvidenceBindingV3>>,
    verifications: Vec<TypedReportEventTupleV5<'a, reviewgraphen_core::VerificationV3>>,
    decisions: Vec<TypedReportEventTupleV5<'a, reviewgraphen_core::DecisionV3>>,
    findings: Vec<TypedReportEventTupleV5<'a, reviewgraphen_core::FindingV3>>,
    v4_registrations: Vec<TypedReportEventTupleV5<'a, reviewgraphen_core::ArtifactRegistrationV4>>,
    gluing_descriptors:
        Vec<TypedReportEventTupleV5<'a, reviewgraphen_core::GluingInputDescriptorV4>>,
    m5_bundle: Option<TypedM5BundleV5<'a>>,
    completion_witness_by_source: BTreeMap<StableId, StableId>,
}

struct TypedObligationTransitionV5<'a> {
    witness: &'a reviewgraphen_core::V5ProjectionEventWitness,
    obligation_id: &'a StableId,
    next: &'a reviewgraphen_core::ObligationLifecycle,
}

/// `Q_target` is not a caller selection.  It is the M5-dependent part of the
/// durable target plan: selected payment obligations when the sealed partial
/// plan requires target gluing.
fn q_target_m5_dependents_v5(
    obligations: &[reviewgraphen_core::Obligation],
    selected: &BTreeSet<StableId>,
    target_gluing_required: bool,
) -> BTreeSet<StableId> {
    if !target_gluing_required {
        return BTreeSet::new();
    }
    obligations
        .iter()
        .filter(|obligation| {
            selected.contains(obligation.id()) && obligation.property_id() == "payment.at_most_once"
        })
        .map(|obligation| obligation.id().clone())
        .collect()
}

/// Resolve loss properties only from immutable obligation rows which are
/// already covered by the dual-prefix report authority.  M6 correspondence
/// records legitimately name both source and target obligations; using only
/// the target facts would make a complete source-bound correspondence row
/// unprojectable merely because its source obligation is not in the target
/// universe.
fn accepted_obligation_properties_v5(
    source: &[reviewgraphen_store::IndexObligation],
    target: &[reviewgraphen_core::Obligation],
) -> Result<AcceptedObligationPropertiesV5, ReportV5ReductionError> {
    let mut by_obligation = BTreeMap::new();
    for (id, property) in target
        .iter()
        .map(|row| (row.id(), row.property_id()))
        .chain(
            source
                .iter()
                .map(|row| (&row.obligation_id, row.property_id.as_str())),
        )
    {
        if property.is_empty() {
            return Err(ReportV5ReductionError::InvalidGateSemantics);
        }
        match by_obligation.insert(id.clone(), property.to_owned()) {
            Some(previous) if previous != property => {
                return Err(ReportV5ReductionError::InvalidGateSemantics);
            }
            _ => {}
        }
    }
    Ok(AcceptedObligationPropertiesV5 { by_obligation })
}

fn require_exact_plan_selection_v5(
    caller: &BTreeSet<StableId>,
    durable_plan: &BTreeSet<StableId>,
) -> Result<(), ReportV5ReductionError> {
    if durable_plan.is_empty() || caller != durable_plan {
        Err(ReportV5ReductionError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn record_completion_sources_v5(
    inherited: &mut TypedInheritedResultV5<'_>,
    source_ids: &[StableId],
    witness: &reviewgraphen_core::V5ProjectionEventWitness,
) {
    for source_id in source_ids {
        inherited
            .completion_witness_by_source
            .entry(source_id.clone())
            .or_insert_with(|| witness.event_id.clone());
    }
}

#[derive(Default)]
struct TerminalM6ReportRowsV5<'a> {
    closure: Option<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::IncrementalSourceClosureV5,
    )>,
    morphism: Option<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::ChangeMorphismV5,
    )>,
    correspondence: Option<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::ObligationCorrespondenceV5,
    )>,
    staleness: Option<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::StalenessAssessmentV5,
    )>,
    partial_plan: Option<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::PartialRerunPlanV5,
    )>,
    mappings: Vec<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::ProgramMappingV5,
    )>,
    correspondence_entries: Vec<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::ObligationCorrespondenceEntryV5,
    )>,
    historical: Vec<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::HistoricalRecordAssessmentV5,
    )>,
    gluing_freshness: Vec<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::GluingFreshnessV5,
    )>,
    artifact_registrations: Vec<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::ArtifactRegistrationV5,
    )>,
    preservation_evidence: Vec<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::PreservationEvidenceV5,
    )>,
    preservation_verifications: Vec<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::PreservationVerificationV5,
    )>,
    partial_actions: Vec<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::PartialRerunActionV5,
    )>,
    gluing_actions: Vec<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::GluingRerunActionV5,
    )>,
    gluing_plan: Option<(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::GluingRerunPlanSealV5,
    )>,
}

struct TupleRows<'a, T: Serialize>(&'a [(&'a reviewgraphen_core::V5ProjectionEventWitness, &'a T)]);
impl<T: Serialize> Serialize for TupleRows<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for (witness, value) in self.0 {
            sequence.serialize_element(&TypedReportEventTupleV5::from_witness(*value, witness))?;
        }
        sequence.end()
    }
}

struct InheritedTupleRows<'a, T: Serialize>(&'a [TypedReportEventTupleV5<'a, T>]);
impl<T: Serialize> Serialize for InheritedTupleRows<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for item in self.0 {
            sequence.serialize_element(item)?;
        }
        sequence.end()
    }
}

struct InheritedExecutionRows<'a>(&'a [TypedExecutionWithClaimsV5<'a>]);
impl Serialize for InheritedExecutionRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            #[derive(Serialize)]
            struct Row<'a> {
                event_sequence: u64,
                event_id: &'a StableId,
                body_hash: ContentHash,
                body: ExecutionBody<'a>,
            }
            #[derive(Serialize)]
            struct ExecutionBody<'a> {
                #[serde(flatten)]
                value: &'a reviewgraphen_core::ExecutionRecord,
                identity_body_hash: ContentHash,
                body_hash: ContentHash,
            }
            let row = Row {
                event_sequence: value.witness.sequence,
                event_id: &value.witness.event_id,
                body_hash: value.witness.body_hash.clone(),
                body: ExecutionBody {
                    value: value.execution,
                    identity_body_hash: value
                        .execution
                        .identity_body_hash()
                        .map_err(serde::ser::Error::custom)?,
                    body_hash: value
                        .execution
                        .body_hash()
                        .map_err(serde::ser::Error::custom)?,
                },
            };
            sequence.serialize_element(&row)?;
        }
        sequence.end()
    }
}

struct InheritedClaimRows<'a>(&'a [TypedExecutionWithClaimsV5<'a>]);
impl Serialize for InheritedClaimRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let count = self.0.iter().map(|value| value.claims.len()).sum();
        let mut sequence = serializer.serialize_seq(Some(count))?;
        for execution in self.0 {
            for claim in execution.claims {
                #[derive(Serialize)]
                struct Row<'a> {
                    event_sequence: u64,
                    event_id: &'a StableId,
                    body_hash: ContentHash,
                    body: ClaimBody<'a>,
                }
                #[derive(Serialize)]
                struct ClaimBody<'a> {
                    #[serde(flatten)]
                    value: &'a reviewgraphen_core::ExecutionClaimV2,
                    identity_body_hash: ContentHash,
                    body_hash: ContentHash,
                }
                let row = Row {
                    event_sequence: execution.witness.sequence,
                    event_id: &execution.witness.event_id,
                    body_hash: execution.witness.body_hash.clone(),
                    body: ClaimBody {
                        value: claim,
                        identity_body_hash: claim
                            .identity_body_hash()
                            .map_err(serde::ser::Error::custom)?,
                        body_hash: claim.body_hash().map_err(serde::ser::Error::custom)?,
                    },
                };
                sequence.serialize_element(&row)?;
            }
        }
        sequence.end()
    }
}

struct M5BundleRows<'a, T: Serialize>(&'a [TypedM5BundleItemV5<'a, T>]);
impl<T: Serialize> Serialize for M5BundleRows<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for row in self.0 {
            sequence.serialize_element(row)?;
        }
        sequence.end()
    }
}

struct M5BundleOne<'a, T: Serialize>(Option<&'a TypedM5BundleItemV5<'a, T>>);
impl<T: Serialize> Serialize for M5BundleOne<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(usize::from(self.0.is_some())))?;
        if let Some(row) = self.0 {
            sequence.serialize_element(row)?;
        }
        sequence.end()
    }
}

struct DescriptorRows<'a>(
    &'a [TypedReportEventTupleV5<'a, reviewgraphen_core::GluingInputDescriptorV4>],
);
impl Serialize for DescriptorRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for row in self.0 {
            sequence.serialize_element(row)?;
        }
        sequence.end()
    }
}

/// Actions are durable scheduling records.  V5 does not contain a separate
/// mutable completion payload; completion is established by the later typed
/// execution/verification/decision rows.  Until that relation is reduced
/// here, retain the action's authoritative durable state as pending rather
/// than inventing completion witnesses from presentation data.
struct PartialActionRows<'a> {
    actions: &'a [(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::PartialRerunActionV5,
    )],
    inherited: &'a TypedInheritedResultV5<'a>,
}

struct GluingActionRows<'a> {
    actions: &'a [(
        &'a reviewgraphen_core::V5ProjectionEventWitness,
        &'a reviewgraphen_core::GluingRerunActionV5,
    )],
    seal_witness: Option<&'a reviewgraphen_core::V5ProjectionEventWitness>,
    seal: Option<&'a reviewgraphen_core::GluingRerunPlanSealV5>,
}
impl Serialize for GluingActionRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        #[derive(Serialize)]
        struct Row<'a> {
            event_id: &'a StableId,
            event_sequence: u64,
            body_hash: &'a ContentHash,
            body: &'a reviewgraphen_core::GluingRerunActionV5,
            state: &'static str,
            completion_witness_ids: Vec<StableId>,
            resolved_reviewer_output_records: Vec<Value>,
        }
        let mut sequence = serializer.serialize_seq(Some(self.actions.len()))?;
        for (witness, value) in self.actions {
            let completion_witness_ids = self
                .seal_witness
                .zip(self.seal)
                .filter(|(_, seal)| seal.source_ids().contains(value.id()))
                .map(|(witness, _)| vec![witness.event_id.clone()])
                .unwrap_or_default();
            sequence.serialize_element(&Row {
                event_id: &witness.event_id,
                event_sequence: witness.sequence,
                body_hash: &witness.body_hash,
                body: value,
                state: if completion_witness_ids.is_empty() {
                    "pending"
                } else {
                    "complete"
                },
                completion_witness_ids,
                resolved_reviewer_output_records: Vec::new(),
            })?;
        }
        sequence.end()
    }
}
impl Serialize for PartialActionRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        #[derive(Serialize)]
        struct Row<'a> {
            event_id: &'a StableId,
            event_sequence: u64,
            body_hash: &'a ContentHash,
            body: &'a reviewgraphen_core::PartialRerunActionV5,
            state: &'static str,
            completion_witness_ids: Vec<StableId>,
            resolved_reviewer_output_records: ResolvedReviewerOutputs<'a>,
        }
        let mut sequence = serializer.serialize_seq(Some(self.actions.len()))?;
        for (witness, value) in self.actions {
            let completion = action_completion_v5(value, self.inherited);
            let complete = completion.is_some();
            sequence.serialize_element(&Row {
                event_id: &witness.event_id,
                event_sequence: witness.sequence,
                body_hash: &witness.body_hash,
                body: value,
                state: if complete { "complete" } else { "pending" },
                completion_witness_ids: completion.into_iter().collect(),
                resolved_reviewer_output_records: ResolvedReviewerOutputs {
                    action: value,
                    inherited: self.inherited,
                    include: complete,
                },
            })?;
        }
        sequence.end()
    }
}

fn action_completion_v5(
    action: &reviewgraphen_core::PartialRerunActionV5,
    inherited: &TypedInheritedResultV5<'_>,
) -> Option<StableId> {
    // The only completion receipt is a later Core-admitted durable row that
    // names this exact scheduled action in its typed source set.  This works
    // for both scheduled-action and existing-target-record prerequisites and
    // deliberately does not infer completion from the terminal marker.
    inherited
        .completion_witness_by_source
        .get(action.id())
        .cloned()
}

struct ResolvedReviewerOutputs<'a> {
    action: &'a reviewgraphen_core::PartialRerunActionV5,
    inherited: &'a TypedInheritedResultV5<'a>,
    include: bool,
}
impl Serialize for ResolvedReviewerOutputs<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use reviewgraphen_core::{ActionPrerequisiteV5, PartialRerunActionKindV5};
        use serde::ser::SerializeSeq;
        if !self.include || self.action.action() != PartialRerunActionKindV5::RerunVerifier {
            return serializer.serialize_seq(Some(0))?.end();
        }
        let records = self
            .action
            .prerequisites()
            .iter()
            .filter_map(|row| match row {
                ActionPrerequisiteV5::ExistingTargetRecord { record_id, .. } => Some(record_id),
                ActionPrerequisiteV5::ScheduledAction { .. } => None,
            })
            .collect::<Vec<_>>();
        let execution_id = records.iter().find(|id| id.kind() == "execution").copied();
        let claim_id = records.iter().find(|id| id.kind() == "claim").copied();
        let Some(execution) = execution_id.and_then(|id| {
            self.inherited
                .executions
                .iter()
                .find(|row| row.execution.id() == id)
        }) else {
            return Err(serde::ser::Error::custom(
                "completed verifier action lacks execution output",
            ));
        };
        let Some(claim) =
            claim_id.and_then(|id| execution.claims.iter().find(|row| row.id() == id))
        else {
            return Err(serde::ser::Error::custom(
                "completed verifier action lacks claim output",
            ));
        };
        #[derive(Serialize)]
        struct ExecutionTuple<'a> {
            event_id: &'a StableId,
            event_sequence: u64,
            body_hash: &'a ContentHash,
            body: ExecutionOutput<'a>,
        }
        #[derive(Serialize)]
        struct ExecutionOutput<'a> {
            #[serde(flatten)]
            value: &'a reviewgraphen_core::ExecutionRecord,
            identity_body_hash: ContentHash,
            body_hash: ContentHash,
        }
        #[derive(Serialize)]
        struct ClaimTuple<'a> {
            event_id: &'a StableId,
            event_sequence: u64,
            body_hash: &'a ContentHash,
            body: ClaimOutput<'a>,
        }
        #[derive(Serialize)]
        struct ClaimOutput<'a> {
            #[serde(flatten)]
            value: &'a reviewgraphen_core::ExecutionClaimV2,
            identity_body_hash: ContentHash,
            body_hash: ContentHash,
        }
        let mut sequence = serializer.serialize_seq(Some(2))?;
        sequence.serialize_element(&ExecutionTuple {
            event_id: &execution.witness.event_id,
            event_sequence: execution.witness.sequence,
            body_hash: &execution.witness.body_hash,
            body: ExecutionOutput {
                value: execution.execution,
                identity_body_hash: execution
                    .execution
                    .identity_body_hash()
                    .map_err(serde::ser::Error::custom)?,
                body_hash: execution
                    .execution
                    .body_hash()
                    .map_err(serde::ser::Error::custom)?,
            },
        })?;
        sequence.serialize_element(&ClaimTuple {
            event_id: &execution.witness.event_id,
            event_sequence: execution.witness.sequence,
            body_hash: &execution.witness.body_hash,
            body: ClaimOutput {
                value: claim,
                identity_body_hash: claim
                    .identity_body_hash()
                    .map_err(serde::ser::Error::custom)?,
                body_hash: claim.body_hash().map_err(serde::ser::Error::custom)?,
            },
        })?;
        sequence.end()
    }
}

#[derive(Serialize)]
struct TerminalMetadataV5<'a> {
    report_id: &'a StableId,
    run_id: &'a StableId,
    profile_id: String,
    rule_set_hash: &'a ContentHash,
    extractor_set_hash: &'a ContentHash,
    policy_version: &'a str,
    event_contract_version: &'static str,
    index_projection_version: &'static str,
    genesis_hash: &'a ContentHash,
    confirmed_offset: u64,
    confirmed_tail_hash: &'a ContentHash,
    confirmed_event_count: u64,
    tool_versions: &'a BTreeMap<String, String>,
    authority_policy_revision_hash: &'a ContentHash,
    authority_replay_basis_digest: &'a ContentHash,
    gluing_profile_descriptor_id: &'static str,
    incremental_policy_descriptor_id: &'static str,
    source_run_id: &'a StableId,
    source_genesis_hash: &'a ContentHash,
    source_confirmed_offset: u64,
    source_tail_hash: &'a ContentHash,
    source_event_count: u64,
    source_index_snapshot_hash: &'a ContentHash,
    source_authority_policy_revision_hash: &'a ContentHash,
    source_authority_replay_basis_digest: &'a ContentHash,
    target_predecessor_offset: u64,
    target_predecessor_tail_hash: &'a ContentHash,
    target_predecessor_event_count: u64,
    target_predecessor_index_snapshot_hash: &'a ContentHash,
    target_pre_incremental_authority_replay_basis_digest: &'a ContentHash,
    target_index_snapshot_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct TerminalScenarioV5<'a> {
    repository_id: &'a StableId,
    snapshot_id: &'a StableId,
    program_space_ref: &'a StableId,
    universe_id: &'a StableId,
    plan_id: &'a StableId,
    selected_obligation_ids: &'a BTreeSet<StableId>,
    artifact_registration_ids: Vec<StableId>,
    preservation_registration_ids: Vec<StableId>,
    context_cover_ids: Vec<StableId>,
    #[serde(rename = "incremental_source_closure")]
    closure: TypedReportEventTupleV5<'a, reviewgraphen_core::IncrementalSourceClosureV5>,
    #[serde(rename = "change_morphism")]
    morphism: TypedReportEventTupleV5<'a, reviewgraphen_core::ChangeMorphismV5>,
    #[serde(rename = "obligation_correspondence")]
    correspondence: TypedReportEventTupleV5<'a, reviewgraphen_core::ObligationCorrespondenceV5>,
}

#[derive(Serialize)]
struct TerminalResultV5<'a> {
    status: &'static str,
    artifact_registrations: InheritedTupleRows<'a, reviewgraphen_core::ArtifactRegisteredV3>,
    artifact_registrations_v5: TupleRows<'a, reviewgraphen_core::ArtifactRegistrationV5>,
    executions: InheritedExecutionRows<'a>,
    claims: InheritedClaimRows<'a>,
    evidence: InheritedTupleRows<'a, reviewgraphen_core::EvidenceV3>,
    evidence_bindings: InheritedTupleRows<'a, reviewgraphen_core::EvidenceBindingV3>,
    verifications: InheritedTupleRows<'a, reviewgraphen_core::VerificationV3>,
    decisions: InheritedTupleRows<'a, reviewgraphen_core::DecisionV3>,
    findings: InheritedTupleRows<'a, reviewgraphen_core::FindingV3>,
    obstructions: ObstructionRowsV5<'a>,
    gluing_input_descriptors: DescriptorRows<'a>,
    context_covers: M5BundleOne<'a, reviewgraphen_core::ContextCoverV4>,
    sections: M5BundleRows<'a, reviewgraphen_core::SectionV4>,
    gluing_attempts: M5BundleOne<'a, reviewgraphen_core::GluingAttemptV4>,
    restrictions: M5BundleRows<'a, reviewgraphen_core::RestrictionV4>,
    global_candidates: M5BundleOne<'a, reviewgraphen_core::GlobalCandidateV4>,
    gluing_obstructions: M5BundleOne<'a, reviewgraphen_core::GluingObstructionV4>,
    program_mappings: TupleRows<'a, reviewgraphen_core::ProgramMappingV5>,
    obligation_correspondence_entries:
        TupleRows<'a, reviewgraphen_core::ObligationCorrespondenceEntryV5>,
    historical_record_assessments: TupleRows<'a, reviewgraphen_core::HistoricalRecordAssessmentV5>,
    gluing_freshness: TupleRows<'a, reviewgraphen_core::GluingFreshnessV5>,
    staleness_assessment:
        Option<TypedReportEventTupleV5<'a, reviewgraphen_core::StalenessAssessmentV5>>,
    preservation_evidence: TupleRows<'a, reviewgraphen_core::PreservationEvidenceV5>,
    preservation_verifications: TupleRows<'a, reviewgraphen_core::PreservationVerificationV5>,
    partial_rerun_actions: PartialActionRows<'a>,
    partial_rerun_plan: Option<TypedReportEventTupleV5<'a, reviewgraphen_core::PartialRerunPlanV5>>,
    gluing_rerun_actions: GluingActionRows<'a>,
    gluing_rerun_plan:
        Option<TypedReportEventTupleV5<'a, reviewgraphen_core::GluingRerunPlanSealV5>>,
}

/// Reviewer outcomes that did not yield structured claims are durable
/// executions and therefore need no invented event or report-local ID.  The
/// execution is the exact source and its accepted obligation set is exactly
/// what remains blocked.
struct ObstructionRowsV5<'a> {
    executions: &'a [TypedExecutionWithClaimsV5<'a>],
    cardinality: &'a [CardinalityObstructionWitnessV5],
    source_closure_id: &'a StableId,
    partial_plan_id: &'a StableId,
}
impl Serialize for ObstructionRowsV5<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let inherited_count = self
            .executions
            .iter()
            .filter(|row| obstruction_kind_v5(row.execution.outcome()).is_some())
            .count();
        let count = inherited_count
            .checked_add(self.cardinality.len())
            .ok_or_else(|| serde::ser::Error::custom("M6 obstruction count overflow"))?;
        let mut sequence = serializer.serialize_seq(Some(count))?;
        for row in self
            .executions
            .iter()
            .filter(|row| obstruction_kind_v5(row.execution.outcome()).is_some())
        {
            #[derive(Serialize)]
            struct Obstruction<'a> {
                kind: &'static str,
                message: &'static str,
                source_ids: [&'a StableId; 1],
                blocks: Vec<StableId>,
            }
            sequence.serialize_element(&Obstruction {
                kind: obstruction_kind_v5(row.execution.outcome())
                    .expect("filter retains only obstruction outcomes"),
                message: "The selected obligation remains in progress after this reviewer outcome.",
                source_ids: [row.execution.id()],
                blocks: row.execution.obligation_ids().iter().cloned().collect(),
            })?;
        }
        let mut cardinality = self.cardinality.iter().collect::<Vec<_>>();
        cardinality.sort_by(|left, right| {
            left.subject_obligation_id
                .cmp(&right.subject_obligation_id)
                .then(left.execution_id.cmp(&right.execution_id))
        });
        for row in cardinality {
            let source_ids = row
                .exact_source_ids(self.source_closure_id, self.partial_plan_id)
                .map_err(serde::ser::Error::custom)?;
            let observed = u64::try_from(row.observed_claim_ids.len())
                .map_err(|_| serde::ser::Error::custom("M6 claim count overflow"))?;
            #[derive(Serialize)]
            struct Obstruction {
                kind: &'static str,
                message: String,
                source_ids: Vec<StableId>,
                blocks: Vec<StableId>,
            }
            sequence.serialize_element(&Obstruction {
                kind: "m6_claim_cardinality_unsupported",
                message: format!("M6 requires exactly one parsed claim; observed {observed}"),
                source_ids: ids(&source_ids),
                blocks: vec![row.subject_obligation_id.clone()],
            })?;
        }
        sequence.end()
    }
}

fn obstruction_kind_v5(value: &reviewgraphen_core::ExecutionOutcome) -> Option<&'static str> {
    match value {
        reviewgraphen_core::ExecutionOutcome::Structured => None,
        reviewgraphen_core::ExecutionOutcome::Abstained { .. } => Some("reviewer_abstained"),
        reviewgraphen_core::ExecutionOutcome::Malformed { .. } => Some("reviewer_malformed"),
        reviewgraphen_core::ExecutionOutcome::ProviderFailure { .. } => {
            Some("reviewer_provider_failure")
        }
    }
}

#[derive(Serialize)]
struct TerminalViewPayloadV5 {
    status: &'static str,
    execution_ids: Vec<StableId>,
    claim_ids: Vec<StableId>,
    obstruction_kinds: Vec<String>,
}
#[derive(Serialize)]
struct TerminalViewV5 {
    kind: &'static str,
    source_ids: Vec<StableId>,
    information_loss: Vec<M6InformationLossV5>,
    payload: TerminalViewPayloadV5,
}
#[derive(Serialize)]
struct TerminalProjectionV5 {
    views: Vec<TerminalViewV5>,
}

/// The terminal projection's two immutable obligation views have distinct
/// scopes: Program closure remains target-only while loss property resolution
/// includes both locked source and target obligation rows.
struct TerminalProjectionObligationAuthorityV5<'a> {
    target_obligations: &'a [reviewgraphen_core::Obligation],
    accepted_properties: &'a AcceptedObligationPropertiesV5,
}

impl TerminalProjectionV5 {
    fn new(
        status: &'static str,
        closure_id: &StableId,
        inherited: &TypedInheritedResultV5<'_>,
        m6: &TerminalM6ReportRowsV5<'_>,
        obligation_authority: TerminalProjectionObligationAuthorityV5<'_>,
        denominator: &BTreeSet<StableId>,
        gate: &IncrementalGateV5,
    ) -> Result<Self, ReportV5ReductionError> {
        let obligations = obligation_authority.target_obligations;
        let target_obligations = denominator.clone();
        let properties_for_program_ids = |ids: &BTreeSet<StableId>| {
            obligations
                .iter()
                .filter(|obligation| {
                    obligation
                        .target_refs()
                        .iter()
                        .chain(obligation.source_ids())
                        .any(|id| ids.contains(id))
                })
                .map(|obligation| obligation.id().clone())
                .collect::<BTreeSet<_>>()
        };
        let historical_closure = |historical: &reviewgraphen_core::HistoricalRecordAssessmentV5| {
            std::iter::once(historical.source_record_id())
                .chain(historical.successor_record_ids().iter())
                .filter(|id| id.kind() == "obligation")
                .cloned()
                .collect::<BTreeSet<_>>()
        };
        let historical_properties =
            m6.historical
                .iter()
                .fold(BTreeSet::new(), |mut out, (_, value)| {
                    out.extend(historical_closure(value));
                    out
                });
        let historical_m5_properties = m6.historical.iter().any(|(_, value)| {
            matches!(
                value.source_record_kind(),
                reviewgraphen_core::HistoricalRecordKindV5::GluingInputDescriptor
                    | reviewgraphen_core::HistoricalRecordKindV5::ContextCover
                    | reviewgraphen_core::HistoricalRecordKindV5::Section
                    | reviewgraphen_core::HistoricalRecordKindV5::Restriction
                    | reviewgraphen_core::HistoricalRecordKindV5::GluingAttempt
                    | reviewgraphen_core::HistoricalRecordKindV5::GlobalCandidate
                    | reviewgraphen_core::HistoricalRecordKindV5::GluingObstruction
            )
        });
        let partial_properties =
            m6.partial_actions
                .iter()
                .fold(BTreeSet::new(), |mut out, (_, value)| {
                    out.extend(value.subject_ids().iter().cloned());
                    out
                });
        let preservation_properties =
            m6.preservation_verifications
                .iter()
                .fold(BTreeSet::new(), |mut out, (_, value)| {
                    out.insert(value.target_obligation_id().clone());
                    out
                });
        let historical_forced_properties = if historical_m5_properties {
            BTreeSet::from(["payment.at_most_once".to_owned()])
        } else {
            BTreeSet::new()
        };
        let gate_has_m5_subject = gate
            .blocking_ids
            .iter()
            .chain(gate.incomplete_ids.iter())
            .any(|id| {
                matches!(
                    id.kind(),
                    "gluing-input-descriptor-v4"
                        | "registration-v4"
                        | "context-cover-v4"
                        | "section-v4"
                        | "restriction-v4"
                        | "gluing-attempt-v4"
                        | "global-candidate-v4"
                        | "gluing-obstruction-v4"
                )
            });
        let gate_forced_properties = if gate_has_m5_subject {
            BTreeSet::from(["payment.at_most_once".to_owned()])
        } else {
            BTreeSet::new()
        };
        let omitted = vec![
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::SourceClosure,
                ids: BTreeSet::from([closure_id.clone()]),
                affected_obligation_ids: target_obligations.clone(),
                forced_properties: BTreeSet::new(),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::ArtifactRegistrationsV5,
                ids: m6
                    .artifact_registrations
                    .iter()
                    .map(|(_, v)| v.id().clone())
                    .collect(),
                affected_obligation_ids: m6
                    .artifact_registrations
                    .iter()
                    .map(|(_, v)| v.source().target_obligation_id().clone())
                    .collect(),
                forced_properties: BTreeSet::new(),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::ProgramMappings,
                ids: m6.mappings.iter().map(|(_, v)| v.id().clone()).collect(),
                affected_obligation_ids: m6.mappings.iter().fold(
                    BTreeSet::new(),
                    |mut out, (_, value)| {
                        let mut ids = value.from_ids().clone();
                        ids.extend(value.to_ids().iter().cloned());
                        out.extend(properties_for_program_ids(&ids));
                        out
                    },
                ),
                forced_properties: BTreeSet::new(),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::ObligationCorrespondenceEntries,
                ids: m6
                    .correspondence_entries
                    .iter()
                    .map(|(_, v)| v.id().clone())
                    .collect(),
                affected_obligation_ids: m6.correspondence_entries.iter().fold(
                    BTreeSet::new(),
                    |mut out, (_, value)| {
                        out.extend(value.from_obligation_ids().iter().cloned());
                        out.extend(value.to_obligation_ids().iter().cloned());
                        out
                    },
                ),
                forced_properties: BTreeSet::new(),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::HistoricalRecordAssessments,
                ids: m6.historical.iter().map(|(_, v)| v.id().clone()).collect(),
                affected_obligation_ids: historical_properties.clone(),
                forced_properties: historical_forced_properties,
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::GluingFreshness,
                ids: m6
                    .gluing_freshness
                    .iter()
                    .map(|(_, v)| v.id().clone())
                    .collect(),
                affected_obligation_ids: BTreeSet::new(),
                forced_properties: BTreeSet::from(["payment.at_most_once".to_owned()]),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::PartialRerunActions,
                ids: m6
                    .partial_actions
                    .iter()
                    .map(|(_, v)| v.id().clone())
                    .collect(),
                affected_obligation_ids: partial_properties.clone(),
                forced_properties: BTreeSet::new(),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::GluingRerunActions,
                ids: m6
                    .gluing_actions
                    .iter()
                    .map(|(_, v)| v.id().clone())
                    .collect(),
                affected_obligation_ids: BTreeSet::new(),
                forced_properties: BTreeSet::from(["payment.at_most_once".to_owned()]),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::PreservationEvidence,
                ids: m6
                    .preservation_evidence
                    .iter()
                    .map(|(_, v)| v.id().clone())
                    .collect(),
                affected_obligation_ids: m6
                    .preservation_evidence
                    .iter()
                    .map(|(_, v)| v.target_obligation_id().clone())
                    .collect(),
                forced_properties: BTreeSet::new(),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::PreservationVerifications,
                ids: m6
                    .preservation_verifications
                    .iter()
                    .map(|(_, v)| v.id().clone())
                    .collect(),
                affected_obligation_ids: preservation_properties.clone(),
                forced_properties: BTreeSet::new(),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::ChangeMorphism,
                ids: m6.morphism.iter().map(|(_, v)| v.id().clone()).collect(),
                affected_obligation_ids: m6.mappings.iter().fold(
                    BTreeSet::new(),
                    |mut out, (_, value)| {
                        let mut ids = value.from_ids().clone();
                        ids.extend(value.to_ids().iter().cloned());
                        out.extend(properties_for_program_ids(&ids));
                        out
                    },
                ),
                forced_properties: BTreeSet::new(),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::ObligationCorrespondence,
                ids: m6
                    .correspondence
                    .iter()
                    .map(|(_, v)| v.id().clone())
                    .collect(),
                affected_obligation_ids: m6.correspondence_entries.iter().fold(
                    BTreeSet::new(),
                    |mut out, (_, value)| {
                        out.extend(value.from_obligation_ids().iter().cloned());
                        out.extend(value.to_obligation_ids().iter().cloned());
                        out
                    },
                ),
                forced_properties: BTreeSet::new(),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::StalenessAssessment,
                ids: m6.staleness.iter().map(|(_, v)| v.id().clone()).collect(),
                affected_obligation_ids: historical_properties,
                forced_properties: BTreeSet::new(),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::PartialRerunPlan,
                ids: m6
                    .partial_plan
                    .iter()
                    .map(|(_, v)| v.id().clone())
                    .collect(),
                affected_obligation_ids: partial_properties
                    .into_iter()
                    .chain(preservation_properties)
                    .collect(),
                forced_properties: BTreeSet::new(),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::GluingRerunPlan,
                ids: m6.gluing_plan.iter().map(|(_, v)| v.id().clone()).collect(),
                affected_obligation_ids: BTreeSet::new(),
                forced_properties: BTreeSet::from(["payment.at_most_once".to_owned()]),
            },
            OmittedM6RecordsV5 {
                location: M6LossLocationV5::Gate,
                ids: BTreeSet::from([gate.id.clone()]),
                affected_obligation_ids: gate
                    .required_fresh_obligation_ids
                    .iter()
                    .chain(
                        gate.incomplete_ids
                            .iter()
                            .filter(|id| id.kind() == "obligation"),
                    )
                    .cloned()
                    .collect(),
                forced_properties: gate_forced_properties,
            },
        ];
        let losses = reduce_m6_losses_v5(&omitted, obligation_authority.accepted_properties)?;
        let execution_ids = inherited
            .executions
            .iter()
            .map(|row| row.execution.id().clone())
            .collect::<Vec<_>>();
        let claim_ids = inherited
            .executions
            .iter()
            .flat_map(|row| row.claims.iter().map(|claim| claim.id().clone()))
            .collect::<Vec<_>>();
        let mut source_ids = BTreeSet::from([closure_id.clone()]);
        source_ids.extend(execution_ids.iter().cloned());
        source_ids.extend(claim_ids.iter().cloned());
        for loss in &losses {
            source_ids.extend(loss.source_ids.iter().cloned());
        }
        let obstruction_kinds = inherited
            .executions
            .iter()
            .filter_map(|row| obstruction_kind_v5(row.execution.outcome()))
            .map(str::to_owned)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let payload = || TerminalViewPayloadV5 {
            status,
            execution_ids: execution_ids.clone(),
            claim_ids: claim_ids.clone(),
            obstruction_kinds: obstruction_kinds.clone(),
        };
        Ok(Self {
            views: vec![
                TerminalViewV5 {
                    kind: "human",
                    source_ids: ids(&source_ids),
                    information_loss: losses.clone(),
                    payload: payload(),
                },
                TerminalViewV5 {
                    kind: "ci",
                    source_ids: ids(&source_ids),
                    information_loss: losses.clone(),
                    payload: payload(),
                },
                TerminalViewV5 {
                    kind: "machine",
                    source_ids: ids(&source_ids),
                    information_loss: losses,
                    payload: payload(),
                },
            ],
        })
    }
}
#[derive(Serialize)]
struct TerminalReportDocumentV5<'a> {
    schema: &'static str,
    report_type: &'static str,
    report_version: u64,
    metadata: TerminalMetadataV5<'a>,
    scenario: TerminalScenarioV5<'a>,
    result: TerminalResultV5<'a>,
    coverage: CoverageV5,
    projection: TerminalProjectionV5,
    gate: IncrementalGateV5,
}

fn terminal_metadata_v5<'a>(
    proof: &'a reviewgraphen_core::TerminalProofV5,
    request: &'a ReportRequestV5,
    program: &'a reviewgraphen_core::ProgramSpace,
    source: &'a reviewgraphen_core::IncrementalSourceClosureReportProjectionV5,
    target_index: &'a TerminalIndexReceiptCoordinatesV5,
) -> TerminalMetadataV5<'a> {
    TerminalMetadataV5 {
        report_id: &request.report_id,
        run_id: proof.run_id(),
        profile_id: program.profile_key(),
        rule_set_hash: program.rule_set_hash(),
        extractor_set_hash: program.extractor_set_hash(),
        policy_version: program.policy_version(),
        event_contract_version: "reviewgraphen.review_event.v5",
        index_projection_version: "reviewgraphen.index_projection.v6",
        genesis_hash: proof.genesis_hash(),
        confirmed_offset: target_index.confirmed_offset,
        confirmed_tail_hash: proof.tail_hash(),
        confirmed_event_count: proof.event_count(),
        tool_versions: &request.tool_versions,
        authority_policy_revision_hash: proof.policy_revision_hash(),
        authority_replay_basis_digest: proof.authority_replay_basis_digest(),
        gluing_profile_descriptor_id: "reviewgraphen.double_submit_gluing@1",
        incremental_policy_descriptor_id: GATE_POLICY,
        source_run_id: &source.source_run_id,
        source_genesis_hash: &source.source_genesis_hash,
        source_confirmed_offset: source.source_confirmed_offset,
        source_tail_hash: &source.source_tail_hash,
        source_event_count: source.source_event_count,
        source_index_snapshot_hash: &source.source_index_snapshot_hash,
        source_authority_policy_revision_hash: &source.source_authority_policy_revision_hash,
        source_authority_replay_basis_digest: &source.source_authority_replay_basis_digest,
        target_predecessor_offset: source.target_predecessor_offset,
        target_predecessor_tail_hash: &source.target_predecessor_tail_hash,
        target_predecessor_event_count: source.target_predecessor_event_count,
        target_predecessor_index_snapshot_hash: &source.target_predecessor_index_snapshot_hash,
        target_pre_incremental_authority_replay_basis_digest: &source
            .target_pre_incremental_authority_replay_basis_digest,
        target_index_snapshot_hash: &target_index.snapshot_hash,
    }
}

// This mirrors the closed V5 result object families; keeping the durable
// witness values explicit prevents an untyped intermediate aggregate.
#[allow(clippy::too_many_arguments)]
fn terminal_result_v5<'a>(
    status: &'static str,
    inherited: &'a TypedInheritedResultV5<'a>,
    m6: &'a TerminalM6ReportRowsV5<'a>,
    cardinality_obstructions: &'a [CardinalityObstructionWitnessV5],
    source_closure_id: &'a StableId,
    partial_plan_id: &'a StableId,
    staleness: &'a reviewgraphen_core::StalenessAssessmentV5,
    staleness_witness: &'a reviewgraphen_core::V5ProjectionEventWitness,
    partial: &'a reviewgraphen_core::PartialRerunPlanV5,
    partial_witness: &'a reviewgraphen_core::V5ProjectionEventWitness,
) -> TerminalResultV5<'a> {
    let bundle = inherited.m5_bundle.as_ref();
    TerminalResultV5 {
        status,
        artifact_registrations: InheritedTupleRows(&inherited.artifact_registrations),
        artifact_registrations_v5: TupleRows(&m6.artifact_registrations),
        executions: InheritedExecutionRows(&inherited.executions),
        claims: InheritedClaimRows(&inherited.executions),
        evidence: InheritedTupleRows(&inherited.evidence),
        evidence_bindings: InheritedTupleRows(&inherited.bindings),
        verifications: InheritedTupleRows(&inherited.verifications),
        decisions: InheritedTupleRows(&inherited.decisions),
        findings: InheritedTupleRows(&inherited.findings),
        obstructions: ObstructionRowsV5 {
            executions: &inherited.executions,
            cardinality: cardinality_obstructions,
            source_closure_id,
            partial_plan_id,
        },
        gluing_input_descriptors: DescriptorRows(&inherited.gluing_descriptors),
        context_covers: M5BundleOne(bundle.map(|value| &value.cover)),
        sections: M5BundleRows(bundle.map_or(&[], |value| value.sections.as_slice())),
        gluing_attempts: M5BundleOne(bundle.map(|value| &value.attempt)),
        restrictions: M5BundleRows(bundle.map_or(&[], |value| value.restrictions.as_slice())),
        global_candidates: M5BundleOne(bundle.and_then(|value| value.candidate.as_ref())),
        gluing_obstructions: M5BundleOne(bundle.and_then(|value| value.obstruction.as_ref())),
        program_mappings: TupleRows(&m6.mappings),
        obligation_correspondence_entries: TupleRows(&m6.correspondence_entries),
        historical_record_assessments: TupleRows(&m6.historical),
        gluing_freshness: TupleRows(&m6.gluing_freshness),
        staleness_assessment: Some(TypedReportEventTupleV5::from_witness(
            staleness,
            staleness_witness,
        )),
        preservation_evidence: TupleRows(&m6.preservation_evidence),
        preservation_verifications: TupleRows(&m6.preservation_verifications),
        partial_rerun_actions: PartialActionRows {
            actions: &m6.partial_actions,
            inherited,
        },
        partial_rerun_plan: Some(TypedReportEventTupleV5::from_witness(
            partial,
            partial_witness,
        )),
        gluing_rerun_actions: GluingActionRows {
            actions: &m6.gluing_actions,
            seal_witness: m6.gluing_plan.map(|(witness, _)| witness),
            seal: m6.gluing_plan.map(|(_, value)| value),
        },
        gluing_rerun_plan: m6
            .gluing_plan
            .map(|(witness, value)| TypedReportEventTupleV5::from_witness(value, witness)),
    }
}

struct TypedExecutionWithClaimsV5<'a> {
    witness: &'a reviewgraphen_core::V5ProjectionEventWitness,
    execution: &'a reviewgraphen_core::ExecutionRecord,
    claims: &'a [reviewgraphen_core::ExecutionClaimV2],
    source_ids: &'a [StableId],
}

struct TypedM5BundleV5<'a> {
    cover: TypedM5BundleItemV5<'a, reviewgraphen_core::ContextCoverV4>,
    sections: Vec<TypedM5BundleItemV5<'a, reviewgraphen_core::SectionV4>>,
    restrictions: Vec<TypedM5BundleItemV5<'a, reviewgraphen_core::RestrictionV4>>,
    attempt: TypedM5BundleItemV5<'a, reviewgraphen_core::GluingAttemptV4>,
    candidate: Option<TypedM5BundleItemV5<'a, reviewgraphen_core::GlobalCandidateV4>>,
    obstruction: Option<TypedM5BundleItemV5<'a, reviewgraphen_core::GluingObstructionV4>>,
}

impl<'a> TypedM5BundleV5<'a> {
    fn from_bundle(
        value: &'a reviewgraphen_core::GluingBundleV4,
        witness: &'a reviewgraphen_core::V5ProjectionEventWitness,
    ) -> Result<Self, ReportError> {
        Ok(Self {
            cover: TypedM5BundleItemV5::from_bundle(value.cover(), witness)?,
            sections: value
                .sections()
                .iter()
                .map(|row| TypedM5BundleItemV5::from_bundle(row, witness))
                .collect::<Result<_, _>>()?,
            restrictions: value
                .restrictions()
                .iter()
                .map(|row| TypedM5BundleItemV5::from_bundle(row, witness))
                .collect::<Result<_, _>>()?,
            attempt: TypedM5BundleItemV5::from_bundle(value.attempt(), witness)?,
            candidate: value
                .global_candidate()
                .map(|row| TypedM5BundleItemV5::from_bundle(row, witness))
                .transpose()?,
            obstruction: value
                .obstruction()
                .map(|row| TypedM5BundleItemV5::from_bundle(row, witness))
                .transpose()?,
        })
    }
}

/// Exhaustive classification of the Core-owned historical report projection.
/// It makes the eventual serialiser consume every inherited family explicitly;
/// no record can disappear through a generic catch-all map.
#[derive(Default)]
struct InheritedTargetFamiliesV5 {
    genesis: u8,
    artifact_registrations: u64,
    snapshot_sources: u8,
    target_plan: u8,
    context_envelopes: u64,
    transitions: u64,
    executions: u64,
    evidence: u64,
    bindings: u64,
    verifications: u64,
    decisions: u64,
    findings: u64,
    v4_registrations: u64,
    m5_bundle: u8,
}

// Do not add a generic event wrapper here. ADR-0023 §14 requires each report
// family to carry its decoded, closed durable DTO. `V5IndexProjectionRecord`
// is a Store classification record and is intentionally not a report input.
// `serde_json::Value` is used only after this reducer has produced its final
// typed document, for schema validation/canonical serialization.

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum IncrementalGateStatusV5 {
    Blocked,
    Incomplete,
    Pass,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum IncrementalGateReasonV5 {
    CurrentAcceptedIssue,
    TargetGluingAssignmentConflict,
    FreshVerificationMissing,
    RerunPending,
    MappingUnresolved,
    UnsupportedImpactPolicy,
    TargetGluingMissing,
    TargetGluingIncomplete,
    GluingPlanMissing,
    HumanResolutionMissing,
    M6ClaimCardinalityUnsupported,
    ExtractionIncomplete,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum M6LossLocationV5 {
    ArtifactRegistrationsV5,
    ProgramMappings,
    ObligationCorrespondenceEntries,
    HistoricalRecordAssessments,
    GluingFreshness,
    PreservationEvidence,
    PreservationVerifications,
    PartialRerunActions,
    GluingRerunActions,
    SourceClosure,
    ChangeMorphism,
    ObligationCorrespondence,
    StalenessAssessment,
    PartialRerunPlan,
    GluingRerunPlan,
    Gate,
}

impl M6LossLocationV5 {
    fn recovery_ref(self) -> &'static str {
        match self {
            Self::ArtifactRegistrationsV5 => {
                "reviewgraphen.review.report.v5#/result/artifact_registrations_v5"
            }
            Self::ProgramMappings => "reviewgraphen.review.report.v5#/result/program_mappings",
            Self::ObligationCorrespondenceEntries => {
                "reviewgraphen.review.report.v5#/result/obligation_correspondence_entries"
            }
            Self::HistoricalRecordAssessments => {
                "reviewgraphen.review.report.v5#/result/historical_record_assessments"
            }
            Self::GluingFreshness => "reviewgraphen.review.report.v5#/result/gluing_freshness",
            Self::PreservationEvidence => {
                "reviewgraphen.review.report.v5#/result/preservation_evidence"
            }
            Self::PreservationVerifications => {
                "reviewgraphen.review.report.v5#/result/preservation_verifications"
            }
            Self::PartialRerunActions => {
                "reviewgraphen.review.report.v5#/result/partial_rerun_actions"
            }
            Self::GluingRerunActions => {
                "reviewgraphen.review.report.v5#/result/gluing_rerun_actions"
            }
            Self::SourceClosure => {
                "reviewgraphen.review.report.v5#/scenario/incremental_source_closure"
            }
            Self::ChangeMorphism => "reviewgraphen.review.report.v5#/scenario/change_morphism",
            Self::ObligationCorrespondence => {
                "reviewgraphen.review.report.v5#/scenario/obligation_correspondence"
            }
            Self::StalenessAssessment => {
                "reviewgraphen.review.report.v5#/result/staleness_assessment"
            }
            Self::PartialRerunPlan => "reviewgraphen.review.report.v5#/result/partial_rerun_plan",
            Self::GluingRerunPlan => "reviewgraphen.review.report.v5#/result/gluing_rerun_plan",
            Self::Gate => "reviewgraphen.review.report.v5#/gate",
        }
    }

    fn reason(self) -> String {
        format!("view omits complete records from {}", self.recovery_ref())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct M6InformationLossV5 {
    pub kind: &'static str,
    pub reason: String,
    pub source_ids: Vec<StableId>,
    pub affected_properties: Vec<String>,
    pub meaningful: bool,
    pub recoverable: bool,
    pub recovery_ref: &'static str,
}

/// Gate/coverage axes reduced from the same typed terminal rows as the
/// document.  This deliberately has no caller-populated fields: a terminal
/// report cannot pass merely because an adapter omitted a non-empty family.
#[derive(Default)]
struct LiveGateAxesV5 {
    native_passes: Vec<NativePassWitnessV5>,
    current_findings: Vec<CurrentFindingWitnessV5>,
    required_fresh_obligation_ids: BTreeSet<StableId>,
    fresh_verified_obligation_ids: BTreeSet<StableId>,
    target_gluing_assignment_conflict_ids: BTreeSet<StableId>,
    pending_rerun_action_ids: BTreeSet<StableId>,
    unresolved_mapping_ids: BTreeSet<StableId>,
    unsupported_impact_ids: BTreeSet<StableId>,
    target_gluing_missing_ids: BTreeSet<StableId>,
    target_gluing_incomplete_ids: BTreeSet<StableId>,
    gluing_plan_missing_scope_id: Option<StableId>,
    human_resolution_missing_ids: BTreeSet<StableId>,
    cardinality_obstructions: Vec<CardinalityObstructionWitnessV5>,
}

fn derive_live_gate_axes_v5(
    inherited: &TypedInheritedResultV5<'_>,
    m6: &TerminalM6ReportRowsV5<'_>,
    denominator: &BTreeSet<StableId>,
    selected: &BTreeSet<StableId>,
    partial_plan: &reviewgraphen_core::PartialRerunPlanV5,
) -> Result<LiveGateAxesV5, ReportError> {
    use reviewgraphen_core::{
        ExecutionOutcome, FindingStatusV3, GluingObstructionKindV4, HistoricalAssessmentStatusV5,
        MappingStatusV5, PartialRerunActionKindV5, StaleReasonV5, VerificationOutcomeV3,
    };

    let mut result = LiveGateAxesV5::default();
    let mut candidates = BTreeMap::<StableId, Vec<NativePassWitnessV5>>::new();
    for execution in &inherited.executions {
        if !matches!(execution.execution.outcome(), ExecutionOutcome::Structured) {
            continue;
        }
        for claim in execution.claims {
            let Some(obligation_id) = claim.obligation_ids().iter().next() else {
                continue;
            };
            if claim.obligation_ids().len() != 1 || !denominator.contains(obligation_id) {
                continue;
            }
            let verifications = inherited
                .verifications
                .iter()
                .filter(|row| {
                    row.body.claim_id() == claim.id()
                        && row.body.outcome() == VerificationOutcomeV3::Passed
                        && row.body.evidence_ids().len() == 1
                })
                .collect::<Vec<_>>();
            for verification in verifications {
                let evidence_id = &verification.body.evidence_ids()[0];
                let evidence = inherited
                    .evidence
                    .iter()
                    .filter(|row| row.body.id() == evidence_id)
                    .collect::<Vec<_>>();
                let bindings = inherited
                    .bindings
                    .iter()
                    .filter(|row| {
                        row.body.claim_id() == claim.id() && row.body.evidence_id() == evidence_id
                    })
                    .collect::<Vec<_>>();
                if evidence.len() != 1 || bindings.len() != 1 {
                    continue;
                }
                let evidence = evidence[0].body;
                let binding = bindings[0].body;
                let registration_ids = BTreeSet::from([
                    execution.execution.raw_artifact_registration_id().clone(),
                    evidence.input_registration_id().clone(),
                    evidence.output_registration_id().clone(),
                    verification.body.input_registration_id().clone(),
                    verification.body.output_registration_id().clone(),
                ]);
                candidates
                    .entry(obligation_id.clone())
                    .or_default()
                    .push(NativePassWitnessV5 {
                        obligation_id: obligation_id.clone(),
                        context_envelope_id: execution.execution.envelope_id().clone(),
                        execution_id: execution.execution.id().clone(),
                        claim_id: claim.id().clone(),
                        evidence_id: evidence.id().clone(),
                        binding_id: binding.id().clone(),
                        verification_id: verification.body.id().clone(),
                        registration_ids,
                    });
            }
        }
    }
    // Multiple native closures for one obligation are never chosen by event
    // order. A current accepted/rejected finding is, however, an explicit
    // human-selected claim. It may disambiguate the matching fresh native
    // closure, provided the selection itself is unique and decision-bound.
    let superseded = inherited
        .findings
        .iter()
        .filter_map(|row| row.body.supersedes_finding_id().cloned())
        .collect::<BTreeSet<_>>();
    let selected_current_claim_ids = inherited
        .findings
        .iter()
        .filter(|row| !superseded.contains(row.body.id()))
        .filter(|finding| {
            let Some(decision_id) = finding.body.decision_id() else {
                return false;
            };
            inherited.decisions.iter().any(|decision| {
                decision.body.id() == decision_id
                    && decision.body.claim_id() == finding.body.claim_id()
                    && matches!(
                        (finding.body.status(), decision.body.outcome()),
                        (
                            FindingStatusV3::Accepted,
                            reviewgraphen_core::DecisionOutcomeV3::Accept
                        ) | (
                            FindingStatusV3::Rejected,
                            reviewgraphen_core::DecisionOutcomeV3::Reject
                        )
                    )
            })
        })
        .map(|row| row.body.claim_id().clone())
        .collect::<BTreeSet<_>>();
    result.native_passes = candidates
        .into_values()
        .filter_map(|mut rows| {
            if rows.len() == 1 {
                return rows.pop();
            }
            let mut selected = rows
                .into_iter()
                .filter(|row| selected_current_claim_ids.contains(&row.claim_id));
            let candidate = selected.next()?;
            selected.next().is_none().then_some(candidate)
        })
        .collect();
    result
        .native_passes
        .sort_by(|left, right| left.obligation_id.cmp(&right.obligation_id));
    result.fresh_verified_obligation_ids = result
        .native_passes
        .iter()
        .map(|row| row.obligation_id.clone())
        .collect();

    let native_by_claim = result
        .native_passes
        .iter()
        .map(|row| (row.claim_id.clone(), row.clone()))
        .collect::<BTreeMap<_, _>>();
    for finding in inherited
        .findings
        .iter()
        .filter(|row| !superseded.contains(row.body.id()))
    {
        let outcome = match finding.body.status() {
            FindingStatusV3::Accepted => CurrentFindingOutcomeV5::AcceptedIssue,
            FindingStatusV3::Rejected => CurrentFindingOutcomeV5::Rejected,
            FindingStatusV3::UnverifiedCandidate | FindingStatusV3::VerifiedCandidate => continue,
        };
        let Some(decision_id) = finding.body.decision_id() else {
            continue;
        };
        let Some(decision) = inherited.decisions.iter().find(|row| {
            row.body.id() == decision_id && row.body.claim_id() == finding.body.claim_id()
        }) else {
            continue;
        };
        let decision_matches = matches!(
            (outcome, decision.body.outcome()),
            (
                CurrentFindingOutcomeV5::AcceptedIssue,
                reviewgraphen_core::DecisionOutcomeV3::Accept
            ) | (
                CurrentFindingOutcomeV5::Rejected,
                reviewgraphen_core::DecisionOutcomeV3::Reject
            )
        );
        if decision_matches && let Some(native) = native_by_claim.get(finding.body.claim_id()) {
            result.current_findings.push(CurrentFindingWitnessV5 {
                decision_id: decision_id.clone(),
                finding_id: finding.body.id().clone(),
                outcome,
                native: native.clone(),
            });
        }
    }

    for (_, action) in &m6.partial_actions {
        let subject_ids = action
            .subject_ids()
            .iter()
            .filter(|id| denominator.contains(*id))
            .cloned()
            .collect::<BTreeSet<_>>();
        match action.action() {
            PartialRerunActionKindV5::RerunVerifier => {
                result.required_fresh_obligation_ids.extend(subject_ids);
            }
            PartialRerunActionKindV5::RerunHumanDecision
                if action_completion_v5(action, inherited).is_none() =>
            {
                result.human_resolution_missing_ids.extend(subject_ids);
            }
            _ => {}
        }
        if action_completion_v5(action, inherited).is_none() {
            result.pending_rerun_action_ids.insert(action.id().clone());
        }
    }
    for (_, mapping) in &m6.mappings {
        if mapping.status() == MappingStatusV5::Unresolved {
            result.unresolved_mapping_ids.insert(mapping.id().clone());
        }
    }
    for (_, historical) in &m6.historical {
        if historical.status() != HistoricalAssessmentStatusV5::StructurallyPreserved
            && historical
                .reasons()
                .contains(&StaleReasonV5::UnsupportedImpactPolicy)
        {
            result
                .unsupported_impact_ids
                .insert(historical.id().clone());
        }
    }
    if let Some(bundle) = &inherited.m5_bundle
        && let Some(obstruction) = &bundle.obstruction
    {
        match obstruction.body.kind() {
            GluingObstructionKindV4::AssignmentConflict => {
                result
                    .target_gluing_assignment_conflict_ids
                    .insert(obstruction.body.id().clone());
            }
            GluingObstructionKindV4::RequiredSectionMissing
            | GluingObstructionKindV4::RequiredOverlapMissing => {
                // M5's obstruction is invariant-scoped, while gate missing
                // members are target obligations. The only authoritative
                // target set at this seam is the selected denominator.
                result
                    .target_gluing_missing_ids
                    .extend(selected.iter().cloned());
            }
            GluingObstructionKindV4::SectionUnknown => {
                result
                    .target_gluing_incomplete_ids
                    .insert(obstruction.body.id().clone());
            }
        }
    }
    // GluingFreshnessV5 describes the source attempt's historical status. It
    // is audit material only: fresh target M5 rows above are the sole current
    // gluing gate inputs.
    if partial_plan.target_gluing_required() && m6.gluing_plan.is_none() {
        // The scope ID is intentionally not reconstructible from JSON or a
        // target proof. Until Core exposes the sealed typed scope, refusing is
        // safer than inventing a `gluing-rerun-scope-v5` identifier.
        return Err(ReportError::Source(
            "target requires gluing but authority lacks a sealed typed gluing rerun plan",
        ));
    }
    for execution in &inherited.executions {
        if !matches!(execution.execution.outcome(), ExecutionOutcome::Structured)
            || execution.execution.plan_id() != partial_plan.target_plan_id()
            || execution.claims.len() == 1
        {
            continue;
        }
        let Some(subject_obligation_id) = execution.execution.obligation_ids().iter().next() else {
            continue;
        };
        if execution.execution.obligation_ids().len() != 1
            || !denominator.contains(subject_obligation_id)
        {
            continue;
        }
        let reviewer_actions = m6
            .partial_actions
            .iter()
            .filter(|(_, action)| {
                action.action() == PartialRerunActionKindV5::RerunReviewer
                    && action.subject_ids().contains(subject_obligation_id)
            })
            .collect::<Vec<_>>();
        let completed_transitions = inherited
            .transitions
            .iter()
            .filter(|transition| {
                transition.obligation_id == subject_obligation_id
                    && *transition.next == reviewgraphen_core::ObligationLifecycle::Completed
                    && transition.witness.sequence > execution.witness.sequence
            })
            .collect::<Vec<_>>();
        let provenance = match completed_transitions.as_slice() {
            [transition] => {
                if !reviewer_actions.is_empty() {
                    return Err(ReportError::Source(
                        "reused cardinality-obstructed execution has a reviewer action",
                    ));
                }
                CardinalityProvenanceV5::Reused {
                    completed_event_id: transition.witness.event_id.clone(),
                }
            }
            [] => {
                let [(_, action)] = reviewer_actions.as_slice() else {
                    return Err(ReportError::Source(
                        "fresh cardinality-obstructed execution has no unique typed reviewer action",
                    ));
                };
                CardinalityProvenanceV5::Fresh {
                    reviewer_action_id: action.id().clone(),
                }
            }
            _ => {
                return Err(ReportError::Source(
                    "cardinality-obstructed execution has ambiguous completed transitions",
                ));
            }
        };
        result
            .cardinality_obstructions
            .push(CardinalityObstructionWitnessV5 {
                subject_obligation_id: subject_obligation_id.clone(),
                envelope_id: execution.execution.envelope_id().clone(),
                raw_registration_id: execution.execution.raw_artifact_registration_id().clone(),
                execution_id: execution.execution.id().clone(),
                execution_event_id: execution.witness.event_id.clone(),
                observed_claim_ids: execution
                    .claims
                    .iter()
                    .map(|claim| claim.id().clone())
                    .collect(),
                provenance,
            });
    }
    Ok(result)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CoverageInputV5 {
    pub universe_id: StableId,
    pub denominator: BTreeSet<StableId>,
    pub selected: BTreeSet<StableId>,
    pub visited: BTreeSet<StableId>,
    pub completed: BTreeSet<StableId>,
    pub evidence_supported: BTreeSet<StableId>,
    pub m5_dependent_successors: BTreeSet<StableId>,
    pub required_human_resolutions: BTreeSet<StableId>,
    pub structurally_preserved: BTreeSet<StableId>,
    pub native_verified: BTreeSet<StableId>,
    pub verified: BTreeSet<StableId>,
    pub fresh_verified: BTreeSet<StableId>,
    pub accepted: BTreeSet<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct CoverageV5 {
    pub universe_id: StableId,
    pub denominator_obligation_ids: Vec<StableId>,
    pub visited_obligation_ids: Vec<StableId>,
    pub completed_obligation_ids: Vec<StableId>,
    pub evidence_supported_obligation_ids: Vec<StableId>,
    pub m5_dependent_successor_obligation_ids: Vec<StableId>,
    pub required_human_resolution_obligation_ids: Vec<StableId>,
    pub structurally_preserved_obligation_ids: Vec<StableId>,
    pub native_verified_obligation_ids: Vec<StableId>,
    pub verified_obligation_ids: Vec<StableId>,
    pub fresh_verified_obligation_ids: Vec<StableId>,
    pub accepted_obligation_ids: Vec<StableId>,
    pub selected: u64,
    pub visited: u64,
    pub completed: u64,
    pub evidence_supported: u64,
    pub m5_dependent_successors: u64,
    pub required_human_resolutions: u64,
    pub structurally_preserved: u64,
    pub native_verified: u64,
    pub verified: u64,
    pub fresh_verified: u64,
    pub accepted: u64,
}

/// One exact native target M4 closure.  This is deliberately structured: a
/// caller cannot add arbitrary IDs to the gate provenance set and label them
/// as "native" witnesses.
#[derive(Clone, Debug, Eq, PartialEq)]
struct NativePassWitnessV5 {
    pub obligation_id: StableId,
    pub context_envelope_id: StableId,
    pub execution_id: StableId,
    pub claim_id: StableId,
    pub evidence_id: StableId,
    pub binding_id: StableId,
    pub verification_id: StableId,
    pub registration_ids: BTreeSet<StableId>,
}

/// A current target finding and its complete native closure.  Only an issue
/// finding may block; only a rejected finding may satisfy a required human
/// resolution.  The two states are not interchangeable.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CurrentFindingWitnessV5 {
    pub decision_id: StableId,
    pub finding_id: StableId,
    pub outcome: CurrentFindingOutcomeV5,
    pub native: NativePassWitnessV5,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CurrentFindingOutcomeV5 {
    AcceptedIssue,
    Rejected,
}

/// Fixed extraction roots.  These IDs are not optional provenance labels: a
/// passing gate must retain all four accepted target extraction roots.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ExtractionWitnessesV5 {
    pub snapshot_id: StableId,
    pub program_space_id: StableId,
    pub universe_id: StableId,
    pub plan_id: StableId,
    pub limitation_ids: BTreeSet<StableId>,
}

/// Typed, reduction-owned gate input.  All IDs enter a named semantic slot;
/// no `validated_*_witness_ids` escape hatch exists.
#[derive(Clone, Debug, Eq, PartialEq)]
struct GateInputV5 {
    pub source_closure_id: StableId,
    pub change_morphism_id: StableId,
    pub obligation_correspondence_id: StableId,
    pub staleness_assessment_id: StableId,
    pub partial_rerun_plan_id: StableId,
    pub gluing_rerun_plan_id: Option<StableId>,
    pub native_passes: Vec<NativePassWitnessV5>,
    pub current_findings: Vec<CurrentFindingWitnessV5>,
    /// Current, target M5 records only (descriptor/registration/cover/
    /// section/restriction/attempt/candidate/obstruction).  The Store reader
    /// must preselect these closed current rows.
    pub current_m5_ids: BTreeSet<StableId>,
    pub extraction: ExtractionWitnessesV5,
    pub required_fresh_obligation_ids: BTreeSet<StableId>,
    pub fresh_verified_obligation_ids: BTreeSet<StableId>,
    pub target_gluing_assignment_conflict_ids: BTreeSet<StableId>,
    pub pending_rerun_action_ids: BTreeSet<StableId>,
    pub unresolved_mapping_ids: BTreeSet<StableId>,
    pub unsupported_impact_ids: BTreeSet<StableId>,
    pub target_gluing_missing_ids: BTreeSet<StableId>,
    pub target_gluing_incomplete_ids: BTreeSet<StableId>,
    /// Once D2 is selection-ready but its second plan is unsealed, ADR-0023
    /// requires the derived scope ID itself.  An action ID would incorrectly
    /// imply that the post-D2 plan was already sealed and schedulable.
    pub gluing_plan_missing_scope_id: Option<StableId>,
    pub human_resolution_missing_ids: BTreeSet<StableId>,
    /// Each witness is produced from one typed M6 execution reduction; both
    /// its source closure and subject obligation enter the gate.  Keeping the
    /// subject separate prevents an execution-only incomplete ID from hiding
    /// the obligation which cannot progress.
    pub cardinality_obstructions: Vec<CardinalityObstructionWitnessV5>,
    pub extraction_incomplete_ids: BTreeSet<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CardinalityObstructionWitnessV5 {
    pub subject_obligation_id: StableId,
    pub envelope_id: StableId,
    pub raw_registration_id: StableId,
    pub execution_id: StableId,
    pub execution_event_id: StableId,
    pub observed_claim_ids: BTreeSet<StableId>,
    pub provenance: CardinalityProvenanceV5,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CardinalityProvenanceV5 {
    Fresh { reviewer_action_id: StableId },
    Reused { completed_event_id: StableId },
}

impl CardinalityObstructionWitnessV5 {
    fn checked_source_accounting(&self) -> Result<u64, ReportV5ReductionError> {
        let observed = u64::try_from(self.observed_claim_ids.len())
            .map_err(|_| ReportV5ReductionError::TooManyRecords)?;
        7_u64
            .checked_add(observed)
            .ok_or(ReportV5ReductionError::TooManyRecords)
    }

    fn exact_source_ids(
        &self,
        closure_id: &StableId,
        partial_plan_id: &StableId,
    ) -> Result<BTreeSet<StableId>, ReportV5ReductionError> {
        for (id, kind) in [
            (&self.subject_obligation_id, "obligation"),
            (&self.envelope_id, "context-envelope"),
            (&self.raw_registration_id, "registration"),
            (&self.execution_id, "execution"),
            (&self.execution_event_id, "event"),
        ] {
            require_kind(id, kind)?;
        }
        if self.observed_claim_ids.len() == 1 || self.observed_claim_ids.len() > 16 {
            return Err(ReportV5ReductionError::InvalidGateSemantics);
        }
        for id in &self.observed_claim_ids {
            require_kind(id, "claim")?;
        }
        let mut source_ids = BTreeSet::from([
            closure_id.clone(),
            partial_plan_id.clone(),
            self.envelope_id.clone(),
            self.raw_registration_id.clone(),
            self.execution_id.clone(),
            self.execution_event_id.clone(),
        ]);
        match &self.provenance {
            CardinalityProvenanceV5::Fresh { reviewer_action_id } => {
                require_kind(reviewer_action_id, "partial-rerun-action-v5")?;
                source_ids.insert(reviewer_action_id.clone());
            }
            CardinalityProvenanceV5::Reused { completed_event_id } => {
                require_kind(completed_event_id, "event")?;
                if completed_event_id == &self.execution_event_id {
                    return Err(ReportV5ReductionError::InvalidGateSemantics);
                }
                source_ids.insert(completed_event_id.clone());
            }
        }
        source_ids.extend(self.observed_claim_ids.iter().cloned());
        let expected = usize::try_from(self.checked_source_accounting()?)
            .map_err(|_| ReportV5ReductionError::TooManyRecords)?;
        if source_ids.len() != expected {
            return Err(ReportV5ReductionError::InvalidGateSemantics);
        }
        Ok(source_ids)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct IncrementalGateV5 {
    pub schema: &'static str,
    pub id: StableId,
    pub policy_descriptor_id: &'static str,
    pub status: IncrementalGateStatusV5,
    pub required_fresh_obligation_ids: Vec<StableId>,
    pub blocking_ids: Vec<StableId>,
    pub incomplete_ids: Vec<StableId>,
    pub reasons: Vec<IncrementalGateReasonV5>,
    pub source_ids: Vec<StableId>,
    pub body_hash: ContentHash,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OmittedM6RecordsV5 {
    pub location: M6LossLocationV5,
    pub ids: BTreeSet<StableId>,
    /// The precise obligation closure for the omitted record location.
    pub affected_obligation_ids: BTreeSet<StableId>,
    /// Fixed ADR properties (currently only target M5's payment invariant).
    pub forced_properties: BTreeSet<String>,
}

/// Authority-owned source and target obligation properties, keyed by immutable
/// obligation ID. This is the sole source for variable M6 loss properties:
/// neither report callers nor projection labels can manufacture a property
/// string.
#[derive(Clone, Debug, Eq, PartialEq)]
struct AcceptedObligationPropertiesV5 {
    by_obligation: BTreeMap<StableId, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ReportV5ReductionError {
    InvalidRequest,
    EmptyDenominator,
    UnknownCoverageMember,
    InvalidCoverageAxis(&'static str),
    InvalidWitnessKind,
    InvalidGateSemantics,
    TooManyRecords,
    Core(String),
}

impl std::fmt::Display for ReportV5ReductionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "report v5 reduction error: {self:?}")
    }
}

impl std::error::Error for ReportV5ReductionError {}

fn ids(values: &BTreeSet<StableId>) -> Vec<StableId> {
    values.iter().cloned().collect()
}

fn count(values: &BTreeSet<StableId>) -> u64 {
    u64::try_from(values.len()).unwrap_or(u64::MAX)
}

fn require_kind(id: &StableId, kind: &str) -> Result<(), ReportV5ReductionError> {
    if id.kind() == kind {
        Ok(())
    } else {
        Err(ReportV5ReductionError::InvalidWitnessKind)
    }
}

fn validate_native_witness(value: &NativePassWitnessV5) -> Result<(), ReportV5ReductionError> {
    for (id, kind) in [
        (&value.obligation_id, "obligation"),
        (&value.context_envelope_id, "context-envelope"),
        (&value.execution_id, "execution"),
        (&value.claim_id, "claim"),
        (&value.evidence_id, "evidence"),
        (&value.binding_id, "binding"),
        (&value.verification_id, "verification"),
    ] {
        require_kind(id, kind)?;
    }
    if value.registration_ids.is_empty() {
        return Err(ReportV5ReductionError::InvalidGateSemantics);
    }
    for id in &value.registration_ids {
        require_kind(id, "registration")?;
    }
    Ok(())
}

fn native_ids(value: &NativePassWitnessV5) -> BTreeSet<StableId> {
    BTreeSet::from([
        value.context_envelope_id.clone(),
        value.execution_id.clone(),
        value.claim_id.clone(),
        value.evidence_id.clone(),
        value.binding_id.clone(),
        value.verification_id.clone(),
    ])
    .union(&value.registration_ids)
    .cloned()
    .collect()
}

fn subset(
    name: &'static str,
    candidate: &BTreeSet<StableId>,
    denominator: &BTreeSet<StableId>,
) -> Result<(), ReportV5ReductionError> {
    if !candidate.is_subset(denominator) {
        return Err(if name == "coverage" {
            ReportV5ReductionError::UnknownCoverageMember
        } else {
            ReportV5ReductionError::InvalidCoverageAxis(name)
        });
    }
    Ok(())
}

fn reduce_coverage_v5(input: &CoverageInputV5) -> Result<CoverageV5, ReportV5ReductionError> {
    if input.denominator.is_empty() {
        return Err(ReportV5ReductionError::EmptyDenominator);
    }
    require_kind(&input.universe_id, "universe")?;
    for obligation_id in &input.denominator {
        require_kind(obligation_id, "obligation")?;
    }
    for (name, values) in [
        ("coverage", &input.selected),
        ("coverage", &input.visited),
        ("coverage", &input.completed),
        ("coverage", &input.evidence_supported),
        ("coverage", &input.m5_dependent_successors),
        ("coverage", &input.required_human_resolutions),
        ("coverage", &input.structurally_preserved),
        ("coverage", &input.native_verified),
        ("coverage", &input.verified),
        ("coverage", &input.fresh_verified),
        ("coverage", &input.accepted),
    ] {
        subset(name, values, &input.denominator)?;
    }
    // M6's native axis is deliberately narrow: it is exactly the current
    // target M4 `Passed` set.  V5 does not get to relabel that set as either
    // verified or fresh_verified.  The other coverage axes remain independent.
    if input.native_verified != input.verified {
        return Err(ReportV5ReductionError::InvalidCoverageAxis(
            "native_verified != verified",
        ));
    }
    if input.verified != input.fresh_verified {
        return Err(ReportV5ReductionError::InvalidCoverageAxis(
            "verified != fresh_verified",
        ));
    }
    Ok(CoverageV5 {
        universe_id: input.universe_id.clone(),
        denominator_obligation_ids: ids(&input.denominator),
        visited_obligation_ids: ids(&input.visited),
        completed_obligation_ids: ids(&input.completed),
        evidence_supported_obligation_ids: ids(&input.evidence_supported),
        m5_dependent_successor_obligation_ids: ids(&input.m5_dependent_successors),
        required_human_resolution_obligation_ids: ids(&input.required_human_resolutions),
        structurally_preserved_obligation_ids: ids(&input.structurally_preserved),
        native_verified_obligation_ids: ids(&input.native_verified),
        verified_obligation_ids: ids(&input.verified),
        fresh_verified_obligation_ids: ids(&input.fresh_verified),
        accepted_obligation_ids: ids(&input.accepted),
        selected: count(&input.selected),
        visited: count(&input.visited),
        completed: count(&input.completed),
        evidence_supported: count(&input.evidence_supported),
        m5_dependent_successors: count(&input.m5_dependent_successors),
        required_human_resolutions: count(&input.required_human_resolutions),
        structurally_preserved: count(&input.structurally_preserved),
        native_verified: count(&input.native_verified),
        verified: count(&input.verified),
        fresh_verified: count(&input.fresh_verified),
        accepted: count(&input.accepted),
    })
}

fn reduce_incremental_gate_v5(
    input: &GateInputV5,
) -> Result<IncrementalGateV5, ReportV5ReductionError> {
    const MAX_RECORDS: usize = 2_048;
    if input.native_passes.len() > MAX_RECORDS
        || input.current_findings.len() > MAX_RECORDS
        || input.current_m5_ids.len() > 8_192
        || input.cardinality_obstructions.len() > MAX_RECORDS
    {
        return Err(ReportV5ReductionError::TooManyRecords);
    }
    for (id, kind) in [
        (&input.source_closure_id, "incremental-source-closure-v5"),
        (&input.change_morphism_id, "change-morphism-v5"),
        (
            &input.obligation_correspondence_id,
            "obligation-correspondence-v5",
        ),
        (&input.staleness_assessment_id, "staleness-assessment-v5"),
        (&input.partial_rerun_plan_id, "partial-rerun-plan-v5"),
        (&input.extraction.snapshot_id, "snapshot"),
        (&input.extraction.program_space_id, "program-space"),
        (&input.extraction.universe_id, "universe"),
        (&input.extraction.plan_id, "plan"),
    ] {
        require_kind(id, kind)?;
    }
    if let Some(id) = &input.gluing_rerun_plan_id {
        require_kind(id, "gluing-rerun-plan-v5")?;
    }
    for native in &input.native_passes {
        validate_native_witness(native)?;
    }
    let mut native_by_obligation = BTreeMap::new();
    for native in &input.native_passes {
        if native_by_obligation
            .insert(native.obligation_id.clone(), native)
            .is_some()
        {
            // A second current native closure for one obligation is not a
            // harmless duplicate. It would let a report choose a closure by
            // presentation order rather than the frozen current reduction.
            return Err(ReportV5ReductionError::InvalidGateSemantics);
        }
    }
    let mut finding_outcomes = BTreeMap::new();
    for finding in &input.current_findings {
        require_kind(&finding.decision_id, "decision")?;
        require_kind(&finding.finding_id, "finding")?;
        validate_native_witness(&finding.native)?;
        let Some(native) = native_by_obligation.get(&finding.native.obligation_id) else {
            return Err(ReportV5ReductionError::InvalidGateSemantics);
        };
        if *native != &finding.native
            || finding_outcomes
                .insert(finding.finding_id.clone(), finding.outcome)
                .is_some()
        {
            return Err(ReportV5ReductionError::InvalidGateSemantics);
        }
    }
    for id in &input.current_m5_ids {
        if !matches!(
            id.kind(),
            "gluing-input-descriptor-v4"
                | "registration-v4"
                | "context-cover-v4"
                | "section-v4"
                | "restriction-v4"
                | "gluing-attempt-v4"
                | "global-candidate-v4"
                | "gluing-obstruction-v4"
        ) {
            return Err(ReportV5ReductionError::InvalidWitnessKind);
        }
    }
    for values in [
        &input.required_fresh_obligation_ids,
        &input.fresh_verified_obligation_ids,
    ] {
        for id in values {
            require_kind(id, "obligation")?;
        }
    }
    for id in &input.pending_rerun_action_ids {
        if !matches!(
            id.kind(),
            "partial-rerun-action-v5" | "gluing-rerun-action-v5"
        ) {
            return Err(ReportV5ReductionError::InvalidWitnessKind);
        }
    }
    for id in &input.unresolved_mapping_ids {
        require_kind(id, "program-mapping-v5")?;
    }
    for id in &input.unsupported_impact_ids {
        require_kind(id, "historical-record-assessment-v5")?;
    }
    for id in &input.target_gluing_assignment_conflict_ids {
        require_kind(id, "gluing-obstruction-v4")?;
    }
    for id in &input.target_gluing_missing_ids {
        require_kind(id, "obligation")?;
    }
    for id in &input.target_gluing_incomplete_ids {
        if !matches!(id.kind(), "gluing-attempt-v4" | "gluing-obstruction-v4") {
            return Err(ReportV5ReductionError::InvalidWitnessKind);
        }
    }
    if let Some(id) = &input.gluing_plan_missing_scope_id {
        require_kind(id, "gluing-rerun-scope-v5")?;
    }
    for id in &input.human_resolution_missing_ids {
        require_kind(id, "obligation")?;
    }
    for id in &input.extraction.limitation_ids {
        require_kind(id, "limitation")?;
    }

    let native_verified = input
        .native_passes
        .iter()
        .map(|value| value.obligation_id.clone())
        .collect::<BTreeSet<_>>();
    if native_verified != input.fresh_verified_obligation_ids {
        return Err(ReportV5ReductionError::InvalidGateSemantics);
    }
    let fresh_missing = input
        .required_fresh_obligation_ids
        .difference(&native_verified)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut cardinality_execution_ids = BTreeSet::new();
    let mut cardinality_sources = BTreeSet::new();
    let mut cardinality_subject_ids = BTreeSet::new();
    let mut cardinality_source_reservation = 0_u64;
    for obstruction in &input.cardinality_obstructions {
        cardinality_source_reservation = cardinality_source_reservation
            .checked_add(obstruction.checked_source_accounting()?)
            .ok_or(ReportV5ReductionError::TooManyRecords)?;
        let sources =
            obstruction.exact_source_ids(&input.source_closure_id, &input.partial_rerun_plan_id)?;
        cardinality_execution_ids.insert(obstruction.execution_id.clone());
        cardinality_subject_ids.insert(obstruction.subject_obligation_id.clone());
        cardinality_sources.extend(sources);
    }
    // This intentionally retains the pre-deduplicated ADR ownership charge:
    // a shared source ID never reduces the required `7 + observed` reservation.
    if cardinality_source_reservation > 47_104 {
        return Err(ReportV5ReductionError::TooManyRecords);
    }
    let _cardinality_source_reservation = cardinality_source_reservation;
    let accepted_issue_ids = input
        .current_findings
        .iter()
        .filter(|value| value.outcome == CurrentFindingOutcomeV5::AcceptedIssue)
        .map(|value| value.finding_id.clone())
        .collect::<BTreeSet<_>>();
    let mut blocking = accepted_issue_ids.clone();
    blocking.extend(input.target_gluing_assignment_conflict_ids.iter().cloned());
    let mut incomplete = fresh_missing.clone();
    for values in [
        &input.pending_rerun_action_ids,
        &input.unresolved_mapping_ids,
        &input.unsupported_impact_ids,
        &input.target_gluing_missing_ids,
        &input.target_gluing_incomplete_ids,
        &input.human_resolution_missing_ids,
        &input.extraction_incomplete_ids,
    ] {
        incomplete.extend(values.iter().cloned());
    }
    if let Some(scope_id) = &input.gluing_plan_missing_scope_id {
        incomplete.insert(scope_id.clone());
    }
    incomplete.extend(cardinality_execution_ids);
    incomplete.extend(cardinality_subject_ids);
    let status = if !blocking.is_empty() {
        IncrementalGateStatusV5::Blocked
    } else if !incomplete.is_empty() {
        IncrementalGateStatusV5::Incomplete
    } else {
        IncrementalGateStatusV5::Pass
    };
    let mut reasons = BTreeSet::new();
    for (present, reason) in [
        (
            !accepted_issue_ids.is_empty(),
            IncrementalGateReasonV5::CurrentAcceptedIssue,
        ),
        (
            !input.target_gluing_assignment_conflict_ids.is_empty(),
            IncrementalGateReasonV5::TargetGluingAssignmentConflict,
        ),
        (
            !fresh_missing.is_empty(),
            IncrementalGateReasonV5::FreshVerificationMissing,
        ),
        (
            !input.pending_rerun_action_ids.is_empty(),
            IncrementalGateReasonV5::RerunPending,
        ),
        (
            !input.unresolved_mapping_ids.is_empty(),
            IncrementalGateReasonV5::MappingUnresolved,
        ),
        (
            !input.unsupported_impact_ids.is_empty(),
            IncrementalGateReasonV5::UnsupportedImpactPolicy,
        ),
        (
            !input.target_gluing_missing_ids.is_empty(),
            IncrementalGateReasonV5::TargetGluingMissing,
        ),
        (
            !input.target_gluing_incomplete_ids.is_empty(),
            IncrementalGateReasonV5::TargetGluingIncomplete,
        ),
        (
            input.gluing_plan_missing_scope_id.is_some(),
            IncrementalGateReasonV5::GluingPlanMissing,
        ),
        (
            !input.human_resolution_missing_ids.is_empty(),
            IncrementalGateReasonV5::HumanResolutionMissing,
        ),
        (
            !input.cardinality_obstructions.is_empty(),
            IncrementalGateReasonV5::M6ClaimCardinalityUnsupported,
        ),
        (
            !input.extraction_incomplete_ids.is_empty(),
            IncrementalGateReasonV5::ExtractionIncomplete,
        ),
    ] {
        if present {
            reasons.insert(reason);
        }
    }
    let mut source_ids = BTreeSet::from([
        input.source_closure_id.clone(),
        input.change_morphism_id.clone(),
        input.obligation_correspondence_id.clone(),
        input.staleness_assessment_id.clone(),
        input.partial_rerun_plan_id.clone(),
        input.extraction.snapshot_id.clone(),
        input.extraction.program_space_id.clone(),
        input.extraction.universe_id.clone(),
        input.extraction.plan_id.clone(),
    ]);
    for native in &input.native_passes {
        source_ids.extend(native_ids(native));
    }
    for finding in &input.current_findings {
        source_ids.extend([finding.decision_id.clone(), finding.finding_id.clone()]);
        source_ids.extend(native_ids(&finding.native));
    }
    source_ids.extend(input.current_m5_ids.iter().cloned());
    source_ids.extend(input.extraction.limitation_ids.iter().cloned());
    source_ids.extend(cardinality_sources);
    if let Some(plan_id) = &input.gluing_rerun_plan_id {
        source_ids.insert(plan_id.clone());
    }
    source_ids.extend(blocking.iter().cloned());
    source_ids.extend(incomplete.iter().cloned());
    #[derive(Serialize)]
    struct Identity<'a> {
        policy_descriptor_id: &'static str,
        status: IncrementalGateStatusV5,
        required_fresh_obligation_ids: &'a Vec<StableId>,
        blocking_ids: &'a Vec<StableId>,
        incomplete_ids: &'a Vec<StableId>,
        reasons: &'a Vec<IncrementalGateReasonV5>,
        source_ids: &'a Vec<StableId>,
    }
    let required = ids(&input.required_fresh_obligation_ids);
    let blocking_ids = ids(&blocking);
    let incomplete_ids = ids(&incomplete);
    let reasons = reasons.into_iter().collect::<Vec<_>>();
    let source_ids = ids(&source_ids);
    let identity = Identity {
        policy_descriptor_id: GATE_POLICY,
        status,
        required_fresh_obligation_ids: &required,
        blocking_ids: &blocking_ids,
        incomplete_ids: &incomplete_ids,
        reasons: &reasons,
        source_ids: &source_ids,
    };
    let Value::Object(object) =
        serde_json::to_value(&identity).map_err(|e| ReportV5ReductionError::Core(e.to_string()))?
    else {
        return Err(ReportV5ReductionError::Core(
            "gate identity is not an object".to_owned(),
        ));
    };
    let bindings = object.into_iter().collect::<BTreeMap<_, _>>();
    let id = StableId::derived("incremental-gate-v5", &bindings)
        .map_err(|e| ReportV5ReductionError::Core(e.to_string()))?;
    #[derive(Serialize)]
    struct Body<'a> {
        schema: &'static str,
        id: &'a StableId,
        #[serde(flatten)]
        identity: &'a Identity<'a>,
    }
    let body_hash = ContentHash::sha256(
        &canonical_json(&Body {
            schema: GATE_SCHEMA,
            id: &id,
            identity: &identity,
        })
        .map_err(|e| ReportV5ReductionError::Core(e.to_string()))?,
    );
    Ok(IncrementalGateV5 {
        schema: GATE_SCHEMA,
        id,
        policy_descriptor_id: GATE_POLICY,
        status,
        required_fresh_obligation_ids: required,
        blocking_ids,
        incomplete_ids,
        reasons,
        source_ids,
        body_hash,
    })
}

fn reduce_m6_losses_v5(
    omitted: &[OmittedM6RecordsV5],
    accepted_properties: &AcceptedObligationPropertiesV5,
) -> Result<Vec<M6InformationLossV5>, ReportV5ReductionError> {
    let mut locations = BTreeSet::new();
    let mut out = Vec::new();
    for value in omitted {
        if value.ids.is_empty() {
            continue;
        }
        if !locations.insert(value.location) {
            return Err(ReportV5ReductionError::Core(
                "duplicate M6 omission location".to_owned(),
            ));
        }
        if value.ids.len() > 8_192 || value.affected_obligation_ids.len() > 2_048 {
            return Err(ReportV5ReductionError::TooManyRecords);
        }
        for id in &value.affected_obligation_ids {
            require_kind(id, "obligation")?;
        }
        let mut properties = value.forced_properties.clone();
        properties.extend(
            value
                .affected_obligation_ids
                .iter()
                .map(|id| {
                    accepted_properties
                        .by_obligation
                        .get(id)
                        .cloned()
                        .filter(|property| !property.is_empty())
                        .ok_or(ReportV5ReductionError::InvalidGateSemantics)
                })
                .collect::<Result<BTreeSet<_>, _>>()?,
        );
        let affected_properties = if properties.is_empty() {
            vec!["reviewgraphen.capability_gap".to_owned()]
        } else {
            properties.into_iter().collect()
        };
        out.push(M6InformationLossV5 {
            kind: "omitted_m6_incremental_records",
            reason: value.location.reason(),
            source_ids: ids(&value.ids),
            affected_properties,
            meaningful: true,
            recoverable: true,
            recovery_ref: value.location.recovery_ref(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(value: &str) -> StableId {
        StableId::parse(value).unwrap()
    }

    fn coverage() -> CoverageInputV5 {
        let o = id("obligation:test");
        CoverageInputV5 {
            universe_id: id("universe:test"),
            denominator: BTreeSet::from([o.clone()]),
            selected: BTreeSet::from([o.clone()]),
            visited: BTreeSet::from([o.clone()]),
            completed: BTreeSet::from([o.clone()]),
            evidence_supported: BTreeSet::from([o.clone()]),
            m5_dependent_successors: BTreeSet::new(),
            required_human_resolutions: BTreeSet::new(),
            structurally_preserved: BTreeSet::new(),
            native_verified: BTreeSet::from([o.clone()]),
            verified: BTreeSet::from([o.clone()]),
            fresh_verified: BTreeSet::from([o]),
            accepted: BTreeSet::new(),
        }
    }
    fn gate() -> GateInputV5 {
        let closure = id("incremental-source-closure-v5:test");
        let morphism = id("change-morphism-v5:test");
        let correspondence = id("obligation-correspondence-v5:test");
        let staleness = id("staleness-assessment-v5:test");
        let partial = id("partial-rerun-plan-v5:test");
        let native = NativePassWitnessV5 {
            obligation_id: id("obligation:test"),
            context_envelope_id: id("context-envelope:test"),
            execution_id: id("execution:test"),
            claim_id: id("claim:test"),
            evidence_id: id("evidence:test"),
            binding_id: id("binding:test"),
            verification_id: id("verification:test"),
            registration_ids: BTreeSet::from([id("registration:test")]),
        };
        GateInputV5 {
            source_closure_id: closure,
            change_morphism_id: morphism,
            obligation_correspondence_id: correspondence,
            staleness_assessment_id: staleness,
            partial_rerun_plan_id: partial,
            gluing_rerun_plan_id: None,
            native_passes: vec![native],
            current_findings: Vec::new(),
            current_m5_ids: BTreeSet::new(),
            extraction: ExtractionWitnessesV5 {
                snapshot_id: id("snapshot:test"),
                program_space_id: id("program-space:test"),
                universe_id: id("universe:test"),
                plan_id: id("plan:test"),
                limitation_ids: BTreeSet::new(),
            },
            required_fresh_obligation_ids: BTreeSet::from([id("obligation:test")]),
            fresh_verified_obligation_ids: BTreeSet::from([id("obligation:test")]),
            target_gluing_assignment_conflict_ids: BTreeSet::new(),
            pending_rerun_action_ids: BTreeSet::new(),
            unresolved_mapping_ids: BTreeSet::new(),
            unsupported_impact_ids: BTreeSet::new(),
            target_gluing_missing_ids: BTreeSet::new(),
            target_gluing_incomplete_ids: BTreeSet::new(),
            gluing_plan_missing_scope_id: None,
            human_resolution_missing_ids: BTreeSet::new(),
            cardinality_obstructions: Vec::new(),
            extraction_incomplete_ids: BTreeSet::new(),
        }
    }
    #[test]
    fn axes_are_distinct_and_deterministic() {
        let a = reduce_coverage_v5(&coverage()).unwrap();
        let b = reduce_coverage_v5(&coverage()).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.accepted, 0);
        assert_eq!(a.fresh_verified, 1);
    }

    #[test]
    fn typed_event_tuple_never_invents_event_metadata() {
        #[derive(Serialize)]
        struct Body {
            schema: &'static str,
            id: StableId,
        }
        let witness = reviewgraphen_core::V5ProjectionEventWitness {
            sequence: 7,
            event_id: id("event:sealed"),
            event_hash: ContentHash::parse(
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            )
            .unwrap(),
            payload_hash: ContentHash::parse(
                "sha256:1111111111111111111111111111111111111111111111111111111111111111",
            )
            .unwrap(),
            body_hash: ContentHash::parse(
                "sha256:2222222222222222222222222222222222222222222222222222222222222222",
            )
            .unwrap(),
            actor: "engine:test".to_owned(),
            logical_time: 8,
        };
        let body = Body {
            schema: "test.v1",
            id: id("record:test"),
        };
        let value =
            serde_json::to_value(TypedReportEventTupleV5::from_witness(&body, &witness)).unwrap();
        assert_eq!(value["event_id"], "event:sealed");
        assert_eq!(value["event_sequence"], 7);
        assert_eq!(
            value["body_hash"],
            ContentHash::sha256(&canonical_json(&body).unwrap()).to_string()
        );
        assert_eq!(value["body"]["id"], "record:test");
        assert!(value.get("actor").is_none());
    }
    #[test]
    fn gate_prioritises_current_target_issue() {
        let mut input = gate();
        input.current_findings.push(CurrentFindingWitnessV5 {
            decision_id: id("decision:test"),
            finding_id: id("finding:test"),
            outcome: CurrentFindingOutcomeV5::AcceptedIssue,
            native: input.native_passes[0].clone(),
        });
        input
            .pending_rerun_action_ids
            .insert(id("partial-rerun-action-v5:test"));
        let result = reduce_incremental_gate_v5(&input).unwrap();
        assert_eq!(result.status, IncrementalGateStatusV5::Blocked);
        assert_eq!(result.blocking_ids, vec![id("finding:test")]);
        assert!(
            result
                .incomplete_ids
                .contains(&id("partial-rerun-action-v5:test"))
        );
    }
    #[test]
    fn pass_gate_keeps_the_complete_base_witness_set() {
        let input = gate();
        let output = reduce_incremental_gate_v5(&input).unwrap();
        assert_eq!(output.status, IncrementalGateStatusV5::Pass);
        assert!(output.source_ids.contains(&input.source_closure_id));
        assert!(
            output
                .source_ids
                .contains(&input.native_passes[0].verification_id)
        );
        assert!(output.source_ids.contains(&input.extraction.snapshot_id));
    }
    #[test]
    fn wrong_kind_witness_is_refused() {
        let mut input = gate();
        input.native_passes[0].verification_id = id("claim:not-a-verification");
        assert_eq!(
            reduce_incremental_gate_v5(&input),
            Err(ReportV5ReductionError::InvalidWitnessKind)
        );
    }
    #[test]
    fn gate_refuses_duplicate_or_detached_current_native_witnesses() {
        let mut duplicate = gate();
        duplicate
            .native_passes
            .push(duplicate.native_passes[0].clone());
        assert_eq!(
            reduce_incremental_gate_v5(&duplicate),
            Err(ReportV5ReductionError::InvalidGateSemantics)
        );

        let mut detached = gate();
        let mut different = detached.native_passes[0].clone();
        different.execution_id = id("execution:other");
        detached.current_findings.push(CurrentFindingWitnessV5 {
            decision_id: id("decision:other"),
            finding_id: id("finding:other"),
            outcome: CurrentFindingOutcomeV5::Rejected,
            native: different,
        });
        assert_eq!(
            reduce_incremental_gate_v5(&detached),
            Err(ReportV5ReductionError::InvalidGateSemantics)
        );
    }
    #[test]
    fn hostile_coverage_axis_and_loss_are_refused() {
        let mut input = coverage();
        input.verified.insert(id("obligation:outside-denominator"));
        assert_eq!(
            reduce_coverage_v5(&input),
            Err(ReportV5ReductionError::UnknownCoverageMember)
        );
        let loss = OmittedM6RecordsV5 {
            location: M6LossLocationV5::Gate,
            ids: BTreeSet::from([id("incremental-gate-v5:test")]),
            affected_obligation_ids: BTreeSet::from([id("not-obligation:test")]),
            forced_properties: BTreeSet::new(),
        };
        assert_eq!(
            reduce_m6_losses_v5(
                &[loss],
                &AcceptedObligationPropertiesV5 {
                    by_obligation: BTreeMap::new(),
                }
            ),
            Err(ReportV5ReductionError::InvalidWitnessKind)
        );
    }
    #[test]
    fn losses_are_one_per_location_with_closed_pointer() {
        let loss = OmittedM6RecordsV5 {
            location: M6LossLocationV5::PartialRerunPlan,
            ids: BTreeSet::from([id("partial-rerun-plan-v5:test")]),
            affected_obligation_ids: BTreeSet::new(),
            forced_properties: BTreeSet::new(),
        };
        let output = reduce_m6_losses_v5(
            &[loss],
            &AcceptedObligationPropertiesV5 {
                by_obligation: BTreeMap::new(),
            },
        )
        .unwrap();
        assert_eq!(
            output[0].recovery_ref,
            "reviewgraphen.review.report.v5#/result/partial_rerun_plan"
        );
    }

    #[test]
    fn losses_use_accepted_obligation_properties_not_obligation_or_coverage_labels() {
        let loss = OmittedM6RecordsV5 {
            location: M6LossLocationV5::SourceClosure,
            ids: BTreeSet::from([id("incremental-source-closure-v5:test")]),
            affected_obligation_ids: BTreeSet::from([id("obligation:test")]),
            forced_properties: BTreeSet::new(),
        };
        let output = reduce_m6_losses_v5(
            std::slice::from_ref(&loss),
            &AcceptedObligationPropertiesV5 {
                by_obligation: BTreeMap::from([(
                    id("obligation:test"),
                    "payment.at_most_once".to_owned(),
                )]),
            },
        )
        .unwrap();
        assert_eq!(output[0].affected_properties, vec!["payment.at_most_once"]);
        assert_eq!(
            reduce_m6_losses_v5(
                &[loss],
                &AcceptedObligationPropertiesV5 {
                    by_obligation: BTreeMap::new(),
                },
            ),
            Err(ReportV5ReductionError::InvalidGateSemantics)
        );
    }

    #[test]
    fn gate_loss_keeps_its_m5_property_and_resolves_source_obligations() {
        // An incremental gate may refer to a target M5 record while the
        // corresponding M6 row spans a source obligation.  Both properties
        // must come from authority-owned obligation rows, never a report
        // label or an inferred target-only mapping.
        let source_obligation = reviewgraphen_store::IndexObligation {
            obligation_id: id("obligation:source"),
            target_kind: "node".to_owned(),
            target_ids_canonical_json: "[]".to_owned(),
            property_id: "source.property".to_owned(),
            lifecycle: "completed".to_owned(),
            body_hash: ContentHash::sha256(b"source-obligation"),
        };
        let accepted = accepted_obligation_properties_v5(&[source_obligation], &[]).unwrap();
        let gate = OmittedM6RecordsV5 {
            location: M6LossLocationV5::Gate,
            ids: BTreeSet::from([id("incremental-gate-v5:test")]),
            affected_obligation_ids: BTreeSet::from([id("obligation:source")]),
            forced_properties: BTreeSet::from(["payment.at_most_once".to_owned()]),
        };
        let output = reduce_m6_losses_v5(&[gate], &accepted).unwrap();
        assert_eq!(
            output[0].affected_properties,
            vec!["payment.at_most_once", "source.property"]
        );
    }

    #[test]
    fn cardinality_obstruction_makes_both_execution_and_subject_incomplete() {
        let mut input = gate();
        input
            .cardinality_obstructions
            .push(CardinalityObstructionWitnessV5 {
                subject_obligation_id: id("obligation:test"),
                envelope_id: id("context-envelope:test"),
                raw_registration_id: id("registration:test"),
                execution_id: id("execution:test"),
                execution_event_id: id("event:execution"),
                observed_claim_ids: BTreeSet::new(),
                provenance: CardinalityProvenanceV5::Fresh {
                    reviewer_action_id: id("partial-rerun-action-v5:test"),
                },
            });
        let result = reduce_incremental_gate_v5(&input).unwrap();
        assert_eq!(result.status, IncrementalGateStatusV5::Incomplete);
        assert!(result.incomplete_ids.contains(&id("execution:test")));
        assert!(result.incomplete_ids.contains(&id("obligation:test")));
    }

    #[test]
    fn cardinality_provenance_is_closed_and_does_not_accept_swapped_event_roles() {
        let mut input = gate();
        input
            .cardinality_obstructions
            .push(CardinalityObstructionWitnessV5 {
                subject_obligation_id: id("obligation:test"),
                envelope_id: id("context-envelope:test"),
                raw_registration_id: id("registration:test"),
                execution_id: id("execution:test"),
                execution_event_id: id("event:execution"),
                observed_claim_ids: BTreeSet::from([id("claim:first"), id("claim:second")]),
                provenance: CardinalityProvenanceV5::Reused {
                    completed_event_id: id("event:execution"),
                },
            });
        assert_eq!(
            reduce_incremental_gate_v5(&input),
            Err(ReportV5ReductionError::InvalidGateSemantics)
        );
    }

    fn cardinality(
        observed_claim_ids: BTreeSet<StableId>,
        provenance: CardinalityProvenanceV5,
    ) -> CardinalityObstructionWitnessV5 {
        CardinalityObstructionWitnessV5 {
            subject_obligation_id: id("obligation:test"),
            envelope_id: id("context-envelope:test"),
            raw_registration_id: id("registration:test"),
            execution_id: id("execution:test"),
            execution_event_id: id("event:execution"),
            observed_claim_ids,
            provenance,
        }
    }

    #[test]
    fn cardinality_obstructions_cover_fresh_zero_two_and_reused_two_exactly() {
        let closure = id("incremental-source-closure-v5:test");
        let partial = id("partial-rerun-plan-v5:test");
        let fresh_zero = cardinality(
            BTreeSet::new(),
            CardinalityProvenanceV5::Fresh {
                reviewer_action_id: id("partial-rerun-action-v5:test"),
            },
        );
        let fresh_two = cardinality(
            BTreeSet::from([id("claim:first"), id("claim:second")]),
            CardinalityProvenanceV5::Fresh {
                reviewer_action_id: id("partial-rerun-action-v5:test"),
            },
        );
        let reused_two = cardinality(
            BTreeSet::from([id("claim:first"), id("claim:second")]),
            CardinalityProvenanceV5::Reused {
                completed_event_id: id("event:completed"),
            },
        );
        for obstruction in [&fresh_zero, &fresh_two, &reused_two] {
            assert_eq!(
                obstruction
                    .exact_source_ids(&closure, &partial)
                    .unwrap()
                    .len(),
                usize::try_from(obstruction.checked_source_accounting().unwrap()).unwrap()
            );
        }
        assert_eq!(fresh_zero.checked_source_accounting().unwrap(), 7);
        assert_eq!(fresh_two.checked_source_accounting().unwrap(), 9);
        assert_eq!(reused_two.checked_source_accounting().unwrap(), 9);

        let mut input = gate();
        input.cardinality_obstructions = vec![fresh_zero, fresh_two, reused_two];
        let output = reduce_incremental_gate_v5(&input).unwrap();
        assert_eq!(output.status, IncrementalGateStatusV5::Incomplete);
        assert!(
            output
                .reasons
                .contains(&IncrementalGateReasonV5::M6ClaimCardinalityUnsupported)
        );
    }

    #[test]
    fn cardinality_obstruction_rows_preserve_zero_claims_and_canonical_message() {
        let obstruction = cardinality(
            BTreeSet::new(),
            CardinalityProvenanceV5::Fresh {
                reviewer_action_id: id("partial-rerun-action-v5:test"),
            },
        );
        let closure = id("incremental-source-closure-v5:test");
        let partial = id("partial-rerun-plan-v5:test");
        let value = serde_json::to_value(ObstructionRowsV5 {
            executions: &[],
            cardinality: &[obstruction],
            source_closure_id: &closure,
            partial_plan_id: &partial,
        })
        .unwrap();
        assert_eq!(value[0]["kind"], "m6_claim_cardinality_unsupported");
        assert_eq!(
            value[0]["message"],
            "M6 requires exactly one parsed claim; observed 0"
        );
        assert_eq!(value[0]["source_ids"].as_array().unwrap().len(), 7);
    }

    #[test]
    fn caller_selection_must_match_every_durable_plan_wave_member() {
        let plan = BTreeSet::from([id("obligation:first"), id("obligation:second")]);
        assert!(require_exact_plan_selection_v5(&plan, &plan).is_ok());
        assert_eq!(
            require_exact_plan_selection_v5(&BTreeSet::new(), &plan),
            Err(ReportV5ReductionError::InvalidRequest)
        );
        assert_eq!(
            require_exact_plan_selection_v5(&BTreeSet::from([id("obligation:first")]), &plan),
            Err(ReportV5ReductionError::InvalidRequest)
        );
    }

    #[test]
    fn preservation_and_native_axes_remain_distinct() {
        let mut input = coverage();
        input.structurally_preserved.insert(id("obligation:test"));
        input.native_verified.clear();
        input.verified.clear();
        input.fresh_verified.clear();
        let output = reduce_coverage_v5(&input).unwrap();
        assert_eq!(output.structurally_preserved, 1);
        assert_eq!(output.fresh_verified, 0);
    }

    #[test]
    fn gluing_plan_missing_requires_the_derived_planning_scope() {
        let mut input = gate();
        input.gluing_plan_missing_scope_id = Some(id("gluing-rerun-scope-v5:test"));
        let result = reduce_incremental_gate_v5(&input).unwrap();
        assert_eq!(result.status, IncrementalGateStatusV5::Incomplete);
        assert!(
            result
                .incomplete_ids
                .contains(&id("gluing-rerun-scope-v5:test"))
        );
        assert!(
            result
                .reasons
                .contains(&IncrementalGateReasonV5::GluingPlanMissing)
        );
    }

    #[test]
    fn request_cannot_carry_dual_run_authority_coordinates() {
        let valid = ReportRequestV5 {
            report_id: id("report:v5"),
            target_plan_id: id("plan:test"),
            selected_obligation_ids: BTreeSet::from([id("obligation:test")]),
            tool_versions: BTreeMap::from([(
                "reviewgraphen.runtime".to_owned(),
                "0.1.0".to_owned(),
            )]),
        };
        assert!(valid.validate().is_ok());
        let invalid = ReportRequestV5 {
            report_id: id("report:v5"),
            target_plan_id: id("plan:test"),
            selected_obligation_ids: BTreeSet::new(),
            tool_versions: BTreeMap::new(),
        };
        assert_eq!(
            invalid.validate(),
            Err(ReportV5ReductionError::InvalidRequest)
        );
    }

    #[test]
    fn v5_limits_accept_exact_and_refuse_each_plus_one() {
        let limits = ReportLimitsV5::default();
        let exact = ReportCountsV5 {
            inherited_target_v4_rows: limits.rows - 6,
            program_mappings: limits.program_mappings,
            correspondence_entries: limits.correspondence_entries,
            historical_assessments: limits.historical_assessments,
            artifact_registrations_v5: limits.artifact_registrations_v5,
            preservation_evidence: limits.preservation_evidence,
            preservation_verifications: limits.preservation_verifications,
            partial_rerun_actions: limits.partial_rerun_actions,
            cardinality_obstructions: limits.cardinality_obstructions,
            gluing_rerun_actions: limits.gluing_rerun_actions,
            gluing_rerun_plan_present: limits.gluing_rerun_plans,
            gluing_freshness: limits.gluing_freshness,
            information_loss_records: limits.information_loss_records,
            views: limits.views,
        };
        // The complete sum, not merely each individual family, is bounded.
        assert!(limits.preflight(exact).is_err());
        let exact_family = ReportCountsV5 {
            inherited_target_v4_rows: 0,
            ..exact
        };
        assert!(limits.preflight(exact_family).is_ok());
        let too_many = ReportCountsV5 {
            gluing_rerun_actions: limits.gluing_rerun_actions + 1,
            ..exact_family
        };
        assert!(matches!(
            limits.preflight(too_many),
            Err(ReportError::Incomplete {
                operation: "gluing_rerun_actions",
                limit,
                observed,
            }) if limit == limits.gluing_rerun_actions
                && observed == limits.gluing_rerun_actions + 1
        ));
    }

    #[test]
    fn v5_accounting_uses_the_dual_session_peaks_and_refuses_plus_one() {
        let limits = ReportLimitsV5 {
            canonical_bytes: 10,
            working_bytes: 100,
            ..ReportLimitsV5::default()
        };
        let exact = ReportAccountingV5 {
            reserved_report_bytes: 100,
            canonical_report_bytes: 10,
            ..ReportAccountingV5::default()
        };
        assert!(limits.check_accounting(exact).is_ok());
        assert!(matches!(
            limits.check_accounting(ReportAccountingV5 {
                canonical_report_bytes: 11,
                ..exact
            }),
            Err(ReportError::Incomplete {
                operation: "report_v5_canonical_bytes",
                limit: 10,
                observed: 11,
            })
        ));
        assert!(matches!(
            limits.check_accounting(ReportAccountingV5 {
                reserved_report_bytes: 101,
                ..exact
            }),
            Err(ReportError::Incomplete {
                operation: "report_v5_working_bytes",
                limit: 100,
                observed: 101,
            })
        ));
    }
}
