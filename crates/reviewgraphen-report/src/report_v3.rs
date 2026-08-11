//! Source-bound `reviewgraphen.review.report.v3` projection.
//!
//! The v3 path intentionally keeps report records borrowed from the verified
//! v4 snapshot.  Selection, closure, counts, `I4`, and `Rr3` are calculated
//! before a report row or output buffer is allocated.

use crate::{
    LogicalCharge, OwnershipError, ReportAccounting, ReportCounts, ReportError, ReportLimits,
    bounded_json_bytes, json_encoded_len, ownership_charge,
};
use reviewgraphen_core::{AuthorityTrustRootsV3, EventContractVersion, StableId};
use reviewgraphen_store::{
    DerivedIndexV4, EventJournal, IndexArtifactRegistrationV4, IndexClaim, IndexClaimAssessmentV3,
    IndexDecisionV3, IndexEvidenceBindingV3, IndexEvidenceV3, IndexFindingV3, IndexSnapshotV4,
    IndexVerificationV3, JournalIdentity, StoreRoot, V4CoverageAxis, V4SelectionItem,
    V4SelectionRequest, V4SelectionSummary, V4SelectionVisitError, V4SelectionVisitor,
    ValidatedIndexSnapshotV4,
};
use serde::{
    Deserialize, Serialize,
    de::{Deserializer, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq, SerializeStruct},
};
use std::collections::{BTreeMap, BTreeSet};

const SCHEMA_V3: &str = "reviewgraphen.review.report.v3";
const INDEX_V4: &str = "reviewgraphen.index_projection.v4";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportRequestV3 {
    pub report_id: StableId,
    pub repository_id: StableId,
    pub program_space_ref: StableId,
    pub plan_id: StableId,
    pub selected_obligation_ids: BTreeSet<StableId>,
    pub tool_versions: BTreeMap<String, String>,
}

pub fn generate_v3(
    root: &StoreRoot,
    identity: JournalIdentity,
    roots: &AuthorityTrustRootsV3,
    request: &ReportRequestV3,
) -> Result<crate::GeneratedReport, ReportError> {
    generate_v3_with_limits(root, identity, roots, request, ReportLimits::default())
}

pub fn generate_v3_with_limits(
    root: &StoreRoot,
    identity: JournalIdentity,
    roots: &AuthorityTrustRootsV3,
    request: &ReportRequestV3,
    limits: ReportLimits,
) -> Result<crate::GeneratedReport, ReportError> {
    if identity.version() != EventContractVersion::V3
        || request.selected_obligation_ids.is_empty()
        || request.tool_versions.is_empty()
        || request
            .tool_versions
            .iter()
            .any(|(k, v)| k.is_empty() || v.is_empty())
    {
        return Err(ReportError::Source(
            "v3 requires v3 input, selected obligations, and tool versions",
        ));
    }
    if request.repository_id != *roots.repository_id() {
        return Err(ReportError::Source("v3 authority repository closure"));
    }
    let journal = EventJournal::open(root, identity)?;
    let index = DerivedIndexV4::open(root)?;
    // The index owns replay/CAS verification while observing the journal.
    let validated = index.validated_snapshot_current_v4(&journal, roots)?;
    let snapshot = validated.snapshot();
    let reader = journal.reader()?;
    if snapshot.marker.confirmed_offset != reader.confirmed_offset()
        || snapshot.marker.tail_hash != *reader.tail_hash()
        || snapshot.marker.event_count != u64::try_from(reader.events().len()).unwrap_or(u64::MAX)
        || snapshot.marker.event_contract_version != EventContractVersion::V3.schema()
        || snapshot.marker.projection_contract_version != INDEX_V4
        || snapshot.marker.index_schema_version != 4
        || snapshot.marker.sqlite_user_version != 4
    {
        return Err(ReportError::Source(
            "v3 journal/index confirmed-tail mismatch",
        ));
    }
    // I4 is measured by streaming serialization of the complete snapshot,
    // never by materialising canonical_json(snapshot).
    let index_bytes = json_encoded_len(snapshot, "index_bytes", limits.working_bytes)?;
    let journal_bytes = reader.confirmed_offset();
    drop(reader);
    build(
        &journal,
        &validated,
        request,
        limits,
        journal_bytes,
        index_bytes,
    )
}

#[allow(clippy::too_many_arguments)]
fn build(
    journal: &EventJournal<'_>,
    validated: &ValidatedIndexSnapshotV4<'_, '_, '_>,
    request: &ReportRequestV3,
    limits: ReportLimits,
    journal_bytes: u64,
    index_bytes: u64,
) -> Result<crate::GeneratedReport, ReportError> {
    let snapshot = validated.snapshot();
    // Refuse J + I4 before constructing any report-owned selection or row.
    limits.preflight(ReportCounts::default(), journal_bytes, index_bytes, 0)?;
    let universe = snapshot
        .universe
        .as_ref()
        .ok_or(ReportError::Source("v3 missing universe"))?;
    if request
        .program_space_ref
        .as_str()
        .strip_prefix("program-space:")
        != Some(universe.snapshot_id.as_str())
    {
        return Err(ReportError::Source("v3 program-space closure"));
    }
    let selection = V4SelectionRequest {
        plan_id: &request.plan_id,
        selected_obligation_ids: &request.selected_obligation_ids,
        expected_confirmed_offset: snapshot.marker.confirmed_offset,
        expected_event_count: snapshot.marker.event_count,
        expected_tail_hash: &snapshot.marker.tail_hash,
    };
    let mut charge = ChargeVisitor::default();
    let first = validated
        .visit_selection(journal, selection, &mut charge)
        .map_err(|error| selection_charge_error(error, limits.working_bytes))?;
    let final_counts = report_counts(first);
    let selected_count = u64_count(request.selected_obligation_ids.len())?;
    let status = if first.counts.completed_ids == selected_count {
        Status::Completed
    } else {
        Status::Partial
    };
    let report_charge = assemble_report_charge(request, snapshot, universe, status, &charge)
        .map_err(|_| incomplete("report_v3_reservation", limits.working_bytes))?;
    let reserved = report_charge;
    // This is the only J + I4 + Rr3 construction gate. No report-owned ID
    // vector or selected row exists before it succeeds.
    let mut ids = preflight_report_materialization(
        limits,
        final_counts,
        journal_bytes,
        index_bytes,
        reserved,
        || SelectionIds::with_summary(first),
    )?;
    let second = validated
        .visit_selection(
            journal,
            V4SelectionRequest {
                plan_id: &request.plan_id,
                selected_obligation_ids: &request.selected_obligation_ids,
                expected_confirmed_offset: snapshot.marker.confirmed_offset,
                expected_event_count: snapshot.marker.event_count,
                expected_tail_hash: &snapshot.marker.tail_hash,
            },
            &mut ids,
        )
        .map_err(selection_materialize_error)?;
    if first != second {
        return Err(ReportError::Source("v3 selection changed between passes"));
    }
    ids.canonicalize_and_verify(second)?;
    if (status == Status::Completed)
        != ids
            .completed
            .iter()
            .eq(request.selected_obligation_ids.iter())
        || charge.obstruction_kind_mask != ids.obstruction_kind_mask
    {
        return Err(ReportError::Source("v3 materialized selection mismatch"));
    }
    let shape = V3Report {
        schema: SCHEMA_V3,
        report_type: "review",
        report_version: 3,
        metadata: V3Metadata::new(request, snapshot, universe),
        scenario: V3Scenario::new(request, universe, &ids.registrations),
        result: V3Result {
            status,
            artifact_registrations: RegistrationRows {
                rows: &snapshot.artifact_registrations,
                ids: &ids.registrations,
            },
            executions: ExecutionRows {
                rows: &snapshot.executions,
                ids: &ids.executions,
            },
            claims: ClaimRows {
                rows: &snapshot.claims,
                ids: &ids.claims,
            },
            evidence: EvidenceRows {
                rows: &snapshot.evidence,
                ids: &ids.evidence,
            },
            evidence_bindings: BindingRows {
                rows: &snapshot.evidence_bindings,
                ids: &ids.evidence_bindings,
            },
            verifications: VerificationRows {
                rows: &snapshot.verifications,
                ids: &ids.verifications,
            },
            decisions: DecisionRows {
                rows: &snapshot.decisions,
                ids: &ids.decisions,
            },
            findings: FindingRows {
                rows: &snapshot.findings,
                ids: &ids.findings,
            },
            claim_assessments: AssessmentRows {
                rows: &snapshot.claim_assessments,
                ids: &ids.claim_assessments,
            },
            obstructions: ObstructionRows {
                rows: &snapshot.executions,
                ids: &ids.obstructions,
            },
        },
        coverage: V3Coverage::new(
            universe,
            &ids.denominator,
            &request.selected_obligation_ids,
            &ids.visited,
            &ids.completed,
            &ids.evidence_supported,
            &ids.verified,
            &ids.accepted,
        ),
        projection: V3Projection::new(
            request,
            status,
            &ids.executions,
            &ids.claims,
            ids.obstruction_kind_mask,
        ),
    };
    let realized = ownership_charge(&shape)
        .map_err(|_| incomplete("report_v3_reservation", limits.working_bytes))?;
    if realized != report_charge {
        return Err(ReportError::Source("v3 charge/materialization mismatch"));
    }
    let largest = largest_record(
        &shape,
        snapshot,
        &ids.registrations,
        &ids.executions,
        &ids.claims,
        &ids.evidence,
        &ids.evidence_bindings,
        &ids.verifications,
        &ids.decisions,
        &ids.findings,
        &ids.claim_assessments,
    )?;
    let output = json_encoded_len(&shape, "canonical_report_bytes", limits.canonical_bytes)?;
    limits.check_serialization(journal_bytes, index_bytes, realized, largest, output)?;
    let canonical_bytes = bounded_json_bytes(&shape, output, limits)?;
    Ok(crate::GeneratedReport {
        canonical_bytes,
        accounting: ReportAccounting {
            journal_bytes,
            index_bytes,
            reserved_report_bytes: reserved,
            realized_report_bytes: realized,
            largest_record_bytes: largest,
            canonical_report_bytes: output,
        },
    })
}

#[derive(Default)]
struct ChargeVisitor {
    registrations: LogicalCharge,
    executions: LogicalCharge,
    claims: LogicalCharge,
    evidence: LogicalCharge,
    evidence_bindings: LogicalCharge,
    verifications: LogicalCharge,
    decisions: LogicalCharge,
    findings: LogicalCharge,
    claim_assessments: LogicalCharge,
    obstructions: LogicalCharge,
    registration_ids: LogicalCharge,
    execution_ids: LogicalCharge,
    claim_ids: LogicalCharge,
    denominator: LogicalCharge,
    visited: LogicalCharge,
    completed: LogicalCharge,
    evidence_supported: LogicalCharge,
    verified: LogicalCharge,
    accepted: LogicalCharge,
    obstruction_kind_mask: u8,
}

impl V4SelectionVisitor for ChargeVisitor {
    type Error = OwnershipError;

    fn visit(&mut self, item: V4SelectionItem<'_>) -> Result<(), Self::Error> {
        match item {
            V4SelectionItem::ArtifactRegistration(row) => {
                self.registrations
                    .list_serialized(&Registration::from(row))?;
                self.registration_ids
                    .list_serialized(&row.registration_id)?;
            }
            V4SelectionItem::Execution(row) => {
                self.executions.list_serialized(&Execution(row))?;
                self.execution_ids.list_serialized(&row.execution_id)?;
            }
            V4SelectionItem::Claim(row) => {
                self.claims.list_serialized(&Claim(row))?;
                self.claim_ids.list_serialized(&row.claim_id)?;
            }
            V4SelectionItem::Evidence(row) => {
                self.evidence.list_serialized(&Evidence::from(row))?;
            }
            V4SelectionItem::EvidenceBinding(row) => {
                self.evidence_bindings
                    .list_serialized(&Binding::from(row))?;
            }
            V4SelectionItem::Verification(row) => {
                self.verifications
                    .list_serialized(&Verification::from(row))?;
            }
            V4SelectionItem::Decision(row) => {
                self.decisions.list_serialized(&Decision::from(row))?;
            }
            V4SelectionItem::Finding(row) => {
                self.findings.list_serialized(&Finding::from(row))?;
            }
            V4SelectionItem::ClaimAssessment(row) => {
                self.claim_assessments
                    .list_serialized(&Assessment::from(row))?;
            }
            V4SelectionItem::Obstruction(row) => {
                let (kind, bit) = obstruction_kind_and_bit(&row.outcome_kind)
                    .ok_or_else(|| OwnershipError::Message("unknown reviewer outcome".into()))?;
                self.obstruction_kind_mask |= bit;
                self.obstructions.list_serialized(&Obstruction {
                    blocks: JsonStrings(&row.obligation_ids_canonical_json),
                    kind,
                    message: "The selected obligation remains in progress after this reviewer outcome.",
                    source_ids: [&row.execution_id],
                })?;
            }
            V4SelectionItem::CoverageId { axis, id } => match axis {
                V4CoverageAxis::Denominator => self.denominator.list_serialized(id)?,
                V4CoverageAxis::Visited => self.visited.list_serialized(id)?,
                V4CoverageAxis::Completed => self.completed.list_serialized(id)?,
                V4CoverageAxis::EvidenceSupported => {
                    self.evidence_supported.list_serialized(id)?;
                }
                V4CoverageAxis::Verified => self.verified.list_serialized(id)?,
                V4CoverageAxis::Accepted => self.accepted.list_serialized(id)?,
            },
        }
        Ok(())
    }
}

fn assemble_report_charge(
    request: &ReportRequestV3,
    snapshot: &IndexSnapshotV4,
    universe: &reviewgraphen_store::IndexUniverse,
    status: Status,
    selected: &ChargeVisitor,
) -> Result<u64, OwnershipError> {
    let mut metadata = LogicalCharge::new();
    metadata.serialized(&V3Metadata::new(request, snapshot, universe))?;

    let mut scenario = LogicalCharge::new();
    scenario.field_charge("artifact_registration_ids", selected.registration_ids)?;
    scenario.field_serialized("plan_id", &request.plan_id)?;
    scenario.field_serialized("program_space_ref", &request.program_space_ref)?;
    scenario.field_serialized("repository_id", &request.repository_id)?;
    scenario.field_serialized("selected_obligation_ids", &request.selected_obligation_ids)?;
    scenario.field_serialized("snapshot_id", &universe.snapshot_id)?;
    scenario.field_serialized("universe_id", &universe.universe_id)?;

    let mut result = LogicalCharge::new();
    result.field_charge("artifact_registrations", selected.registrations)?;
    result.field_charge("claim_assessments", selected.claim_assessments)?;
    result.field_charge("claims", selected.claims)?;
    result.field_charge("decisions", selected.decisions)?;
    result.field_charge("evidence", selected.evidence)?;
    result.field_charge("evidence_bindings", selected.evidence_bindings)?;
    result.field_charge("executions", selected.executions)?;
    result.field_charge("findings", selected.findings)?;
    result.field_charge("obstructions", selected.obstructions)?;
    result.field_serialized("status", &status)?;
    result.field_charge("verifications", selected.verifications)?;

    let mut coverage = LogicalCharge::new();
    coverage.field_number("accepted")?;
    coverage.field_charge("accepted_obligation_ids", selected.accepted)?;
    coverage.field_number("completed")?;
    coverage.field_charge("completed_obligation_ids", selected.completed)?;
    coverage.field_charge("denominator_obligation_ids", selected.denominator)?;
    coverage.field_number("evidence_supported")?;
    coverage.field_charge(
        "evidence_supported_obligation_ids",
        selected.evidence_supported,
    )?;
    coverage.field_number("fresh_verified")?;
    coverage.field_charge("fresh_verified_obligation_ids", selected.verified)?;
    coverage.field_number("selected")?;
    coverage.field_serialized("universe_id", &universe.universe_id)?;
    coverage.field_number("verified")?;
    coverage.field_charge("verified_obligation_ids", selected.verified)?;
    coverage.field_number("visited")?;
    coverage.field_charge("visited_obligation_ids", selected.visited)?;

    let mut kinds = LogicalCharge::new();
    for (bit, kind) in OBSTRUCTION_KINDS {
        if selected.obstruction_kind_mask & bit != 0 {
            kinds.list_serialized(&kind)?;
        }
    }
    let mut payload = LogicalCharge::new();
    payload.field_charge("claim_ids", selected.claim_ids)?;
    payload.field_charge("execution_ids", selected.execution_ids)?;
    payload.field_charge("obstruction_kinds", kinds)?;
    payload.field_serialized("status", &status)?;
    let mut loss = LogicalCharge::new();
    loss.field_serialized("affected_properties", &["review.authority_trace"])?;
    loss.field_string("kind", "authority_detail_omitted")?;
    loss.field_boolean("meaningful")?;
    loss.field_string(
        "reason",
        "The machine projection omits authority detail retained by this source-bound report.",
    )?;
    loss.field_boolean("recoverable")?;
    loss.field_serialized("recovery_ref", &request.report_id)?;
    loss.field_serialized("source_ids", &request.selected_obligation_ids)?;
    let mut losses = LogicalCharge::new();
    losses.list_charge(loss)?;
    let mut view = LogicalCharge::new();
    view.field_charge("information_loss", losses)?;
    view.field_string("kind", "machine")?;
    view.field_charge("payload", payload)?;
    view.field_serialized("source_ids", &request.selected_obligation_ids)?;
    let mut views = LogicalCharge::new();
    views.list_charge(view)?;
    let mut projection = LogicalCharge::new();
    projection.field_charge("views", views)?;

    let mut report = LogicalCharge::new();
    report.field_charge("coverage", coverage)?;
    report.field_charge("metadata", metadata)?;
    report.field_charge("projection", projection)?;
    report.field_string("report_type", "review")?;
    report.field_number("report_version")?;
    report.field_charge("result", result)?;
    report.field_charge("scenario", scenario)?;
    report.field_string("schema", SCHEMA_V3)?;
    Ok(report.bytes())
}

fn preflight_report_materialization<T>(
    limits: ReportLimits,
    counts: ReportCounts,
    journal_bytes: u64,
    index_bytes: u64,
    reserved: u64,
    materialize: impl FnOnce() -> Result<T, ReportError>,
) -> Result<T, ReportError> {
    limits
        .preflight(counts, journal_bytes, index_bytes, reserved)
        .map_err(ReportError::from)?;
    materialize()
}

fn report_counts(summary: V4SelectionSummary) -> ReportCounts {
    ReportCounts {
        registrations: summary.counts.artifact_registrations,
        executions: summary.counts.executions,
        claims: summary.counts.claims,
        evidence: summary.counts.evidence,
        evidence_bindings: summary.counts.evidence_bindings,
        verifications: summary.counts.verifications,
        decisions: summary.counts.decisions,
        findings: summary.counts.findings,
        claim_assessments: summary.counts.claim_assessments,
        obstructions: summary.counts.obstructions,
        views: 1,
        information_loss_records: 1,
    }
}

fn selection_charge_error(error: V4SelectionVisitError<OwnershipError>, limit: u64) -> ReportError {
    match error {
        V4SelectionVisitError::Index(error) => error.into(),
        V4SelectionVisitError::Visitor(_) => incomplete("report_v3_charge", limit),
    }
}

fn selection_materialize_error(
    error: V4SelectionVisitError<std::convert::Infallible>,
) -> ReportError {
    match error {
        V4SelectionVisitError::Index(error) => error.into(),
        V4SelectionVisitError::Visitor(never) => match never {},
    }
}

struct SelectionIds {
    registrations: Vec<StableId>,
    executions: Vec<StableId>,
    claims: Vec<StableId>,
    evidence: Vec<StableId>,
    evidence_bindings: Vec<StableId>,
    verifications: Vec<StableId>,
    decisions: Vec<StableId>,
    findings: Vec<StableId>,
    claim_assessments: Vec<StableId>,
    obstructions: Vec<StableId>,
    denominator: Vec<StableId>,
    visited: Vec<StableId>,
    completed: Vec<StableId>,
    evidence_supported: Vec<StableId>,
    verified: Vec<StableId>,
    accepted: Vec<StableId>,
    obstruction_kind_mask: u8,
}

impl SelectionIds {
    fn with_summary(summary: V4SelectionSummary) -> Result<Self, ReportError> {
        let counts = summary.counts;
        Ok(Self {
            registrations: reserved_ids(counts.artifact_registrations)?,
            executions: reserved_ids(counts.executions)?,
            claims: reserved_ids(counts.claims)?,
            evidence: reserved_ids(counts.evidence)?,
            evidence_bindings: reserved_ids(counts.evidence_bindings)?,
            verifications: reserved_ids(counts.verifications)?,
            decisions: reserved_ids(counts.decisions)?,
            findings: reserved_ids(counts.findings)?,
            claim_assessments: reserved_ids(counts.claim_assessments)?,
            obstructions: reserved_ids(counts.obstructions)?,
            denominator: reserved_ids(counts.denominator_ids)?,
            visited: reserved_ids(counts.visited_ids)?,
            completed: reserved_ids(counts.completed_ids)?,
            evidence_supported: reserved_ids(counts.evidence_supported_ids)?,
            verified: reserved_ids(counts.verified_ids)?,
            accepted: reserved_ids(counts.accepted_ids)?,
            obstruction_kind_mask: 0,
        })
    }

    fn canonicalize_and_verify(&mut self, summary: V4SelectionSummary) -> Result<(), ReportError> {
        let counts = summary.counts;
        canonical_ids(&mut self.registrations, counts.artifact_registrations)?;
        canonical_ids(&mut self.executions, counts.executions)?;
        canonical_ids(&mut self.claims, counts.claims)?;
        canonical_ids(&mut self.evidence, counts.evidence)?;
        canonical_ids(&mut self.evidence_bindings, counts.evidence_bindings)?;
        canonical_ids(&mut self.verifications, counts.verifications)?;
        canonical_ids(&mut self.decisions, counts.decisions)?;
        canonical_ids(&mut self.findings, counts.findings)?;
        canonical_ids(&mut self.claim_assessments, counts.claim_assessments)?;
        canonical_ids(&mut self.obstructions, counts.obstructions)?;
        canonical_ids(&mut self.denominator, counts.denominator_ids)?;
        canonical_ids(&mut self.visited, counts.visited_ids)?;
        canonical_ids(&mut self.completed, counts.completed_ids)?;
        canonical_ids(&mut self.evidence_supported, counts.evidence_supported_ids)?;
        canonical_ids(&mut self.verified, counts.verified_ids)?;
        canonical_ids(&mut self.accepted, counts.accepted_ids)
    }
}

impl V4SelectionVisitor for SelectionIds {
    type Error = std::convert::Infallible;

    fn visit(&mut self, item: V4SelectionItem<'_>) -> Result<(), Self::Error> {
        match item {
            V4SelectionItem::ArtifactRegistration(row) => {
                self.registrations.push(row.registration_id.clone());
            }
            V4SelectionItem::Execution(row) => self.executions.push(row.execution_id.clone()),
            V4SelectionItem::Claim(row) => self.claims.push(row.claim_id.clone()),
            V4SelectionItem::Evidence(row) => self.evidence.push(row.evidence_id.clone()),
            V4SelectionItem::EvidenceBinding(row) => {
                self.evidence_bindings.push(row.binding_id.clone());
            }
            V4SelectionItem::Verification(row) => {
                self.verifications.push(row.verification_id.clone());
            }
            V4SelectionItem::Decision(row) => self.decisions.push(row.decision_id.clone()),
            V4SelectionItem::Finding(row) => self.findings.push(row.finding_id.clone()),
            V4SelectionItem::ClaimAssessment(row) => {
                self.claim_assessments.push(row.claim_id.clone());
            }
            V4SelectionItem::Obstruction(row) => {
                self.obstructions.push(row.execution_id.clone());
                if let Some((_, bit)) = obstruction_kind_and_bit(&row.outcome_kind) {
                    self.obstruction_kind_mask |= bit;
                }
            }
            V4SelectionItem::CoverageId { axis, id } => match axis {
                V4CoverageAxis::Denominator => self.denominator.push(id.clone()),
                V4CoverageAxis::Visited => self.visited.push(id.clone()),
                V4CoverageAxis::Completed => self.completed.push(id.clone()),
                V4CoverageAxis::EvidenceSupported => self.evidence_supported.push(id.clone()),
                V4CoverageAxis::Verified => self.verified.push(id.clone()),
                V4CoverageAxis::Accepted => self.accepted.push(id.clone()),
            },
        }
        Ok(())
    }
}

fn reserved_ids(count: u64) -> Result<Vec<StableId>, ReportError> {
    let capacity =
        usize::try_from(count).map_err(|_| incomplete("report_v3_materialization", u64::MAX))?;
    let mut ids = Vec::new();
    ids.try_reserve_exact(capacity)
        .map_err(|_| incomplete("report_v3_materialization", count))?;
    Ok(ids)
}

fn canonical_ids(ids: &mut Vec<StableId>, expected: u64) -> Result<(), ReportError> {
    ids.sort_unstable();
    ids.dedup();
    if u64_count(ids.len())? != expected {
        return Err(ReportError::Source("v3 selection cardinality mismatch"));
    }
    Ok(())
}

fn contains_id(ids: &[StableId], id: &StableId) -> bool {
    ids.binary_search(id).is_ok()
}

fn incomplete(operation: &'static str, limit: u64) -> ReportError {
    ReportError::Incomplete {
        operation,
        limit,
        observed: u64::MAX,
    }
}
fn u64_count(n: usize) -> Result<u64, ReportError> {
    u64::try_from(n).map_err(|_| incomplete("report_v3_count", u64::MAX))
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Completed,
    Partial,
}
#[derive(Serialize)]
struct V3Report<'a> {
    coverage: V3Coverage<'a>,
    metadata: V3Metadata<'a>,
    projection: V3Projection<'a>,
    report_type: &'static str,
    report_version: u8,
    result: V3Result<'a>,
    scenario: V3Scenario<'a>,
    schema: &'static str,
}
#[derive(Serialize)]
struct V3Metadata<'a> {
    authority_policy_revision_hash: &'a reviewgraphen_core::ContentHash,
    authority_replay_basis_digest: &'a reviewgraphen_core::ContentHash,
    confirmed_event_count: u64,
    confirmed_offset: u64,
    confirmed_tail_hash: &'a reviewgraphen_core::ContentHash,
    event_contract_version: &'static str,
    extractor_set_hash: &'a reviewgraphen_core::ContentHash,
    genesis_hash: &'a reviewgraphen_core::ContentHash,
    index_projection_version: &'static str,
    policy_version: &'a str,
    profile_id: &'a str,
    report_id: &'a StableId,
    rule_set_hash: &'a reviewgraphen_core::ContentHash,
    run_id: &'a StableId,
    tool_versions: &'a BTreeMap<String, String>,
}
impl<'a> V3Metadata<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        request: &'a ReportRequestV3,
        snapshot: &'a IndexSnapshotV4,
        universe: &'a reviewgraphen_store::IndexUniverse,
    ) -> Self {
        Self {
            report_id: &request.report_id,
            run_id: &snapshot.marker.run_id,
            profile_id: &universe.profile_id,
            rule_set_hash: &universe.rule_set_hash,
            extractor_set_hash: &universe.extractor_set_hash,
            policy_version: &universe.policy_version,
            event_contract_version: EventContractVersion::V3.schema(),
            index_projection_version: INDEX_V4,
            genesis_hash: &snapshot.marker.genesis_hash,
            confirmed_offset: snapshot.marker.confirmed_offset,
            confirmed_tail_hash: &snapshot.marker.tail_hash,
            confirmed_event_count: snapshot.marker.event_count,
            tool_versions: &request.tool_versions,
            authority_policy_revision_hash: &snapshot.policy_revision_hash,
            authority_replay_basis_digest: &snapshot.authority_replay_basis_digest,
        }
    }
}
#[derive(Serialize)]
struct V3Scenario<'a> {
    artifact_registration_ids: &'a [StableId],
    plan_id: &'a StableId,
    program_space_ref: &'a StableId,
    repository_id: &'a StableId,
    selected_obligation_ids: &'a BTreeSet<StableId>,
    snapshot_id: &'a StableId,
    universe_id: &'a StableId,
}
impl<'a> V3Scenario<'a> {
    fn new(
        request: &'a ReportRequestV3,
        universe: &'a reviewgraphen_store::IndexUniverse,
        registration_ids: &'a [StableId],
    ) -> Self {
        Self {
            repository_id: &request.repository_id,
            snapshot_id: &universe.snapshot_id,
            program_space_ref: &request.program_space_ref,
            universe_id: &universe.universe_id,
            plan_id: &request.plan_id,
            selected_obligation_ids: &request.selected_obligation_ids,
            artifact_registration_ids: registration_ids,
        }
    }
}
#[derive(Serialize)]
struct V3Result<'a> {
    artifact_registrations: RegistrationRows<'a>,
    claim_assessments: AssessmentRows<'a>,
    claims: ClaimRows<'a>,
    decisions: DecisionRows<'a>,
    evidence: EvidenceRows<'a>,
    evidence_bindings: BindingRows<'a>,
    executions: ExecutionRows<'a>,
    findings: FindingRows<'a>,
    obstructions: ObstructionRows<'a>,
    status: Status,
    verifications: VerificationRows<'a>,
}
#[derive(Serialize)]
struct V3Coverage<'a> {
    accepted: u64,
    accepted_obligation_ids: &'a [StableId],
    completed: u64,
    completed_obligation_ids: &'a [StableId],
    denominator_obligation_ids: &'a [StableId],
    evidence_supported: u64,
    evidence_supported_obligation_ids: &'a [StableId],
    fresh_verified: u64,
    fresh_verified_obligation_ids: &'a [StableId],
    selected: u64,
    universe_id: &'a StableId,
    verified: u64,
    verified_obligation_ids: &'a [StableId],
    visited: u64,
    visited_obligation_ids: &'a [StableId],
}
impl<'a> V3Coverage<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        universe: &'a reviewgraphen_store::IndexUniverse,
        denominator: &'a [StableId],
        selected: &'a BTreeSet<StableId>,
        visited: &'a [StableId],
        completed: &'a [StableId],
        evidence_supported: &'a [StableId],
        verified: &'a [StableId],
        accepted: &'a [StableId],
    ) -> Self {
        Self {
            universe_id: &universe.universe_id,
            denominator_obligation_ids: denominator,
            visited_obligation_ids: visited,
            completed_obligation_ids: completed,
            evidence_supported_obligation_ids: evidence_supported,
            verified_obligation_ids: verified,
            fresh_verified_obligation_ids: verified,
            accepted_obligation_ids: accepted,
            selected: selected.len() as u64,
            visited: visited.len() as u64,
            completed: completed.len() as u64,
            evidence_supported: evidence_supported.len() as u64,
            verified: verified.len() as u64,
            fresh_verified: verified.len() as u64,
            accepted: accepted.len() as u64,
        }
    }
}
#[derive(Serialize)]
struct V3Projection<'a> {
    views: One<V3View<'a>>,
}
#[derive(Serialize)]
struct V3View<'a> {
    information_loss: One<V3Loss<'a>>,
    kind: &'static str,
    payload: V3Payload<'a>,
    source_ids: &'a BTreeSet<StableId>,
}
#[derive(Serialize)]
struct V3Loss<'a> {
    affected_properties: [&'static str; 1],
    kind: &'static str,
    meaningful: bool,
    reason: &'static str,
    recoverable: bool,
    recovery_ref: &'a StableId,
    source_ids: &'a BTreeSet<StableId>,
}
#[derive(Serialize)]
struct V3Payload<'a> {
    claim_ids: &'a [StableId],
    execution_ids: &'a [StableId],
    obstruction_kinds: ObstructionKinds,
    status: Status,
}
struct One<T>(T);
impl<T: Serialize> Serialize for One<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(1))?;
        seq.serialize_element(&self.0)?;
        seq.end()
    }
}
impl<'a> V3Projection<'a> {
    fn new(
        request: &'a ReportRequestV3,
        status: Status,
        execution_ids: &'a [StableId],
        claim_ids: &'a [StableId],
        obstruction_kind_mask: u8,
    ) -> Self {
        Self {
            views: One(V3View {
                kind: "machine",
                source_ids: &request.selected_obligation_ids,
                information_loss: One(V3Loss {
                    kind: "authority_detail_omitted",
                    reason: "The machine projection omits authority detail retained by this source-bound report.",
                    source_ids: &request.selected_obligation_ids,
                    affected_properties: ["review.authority_trace"],
                    meaningful: true,
                    recoverable: true,
                    recovery_ref: &request.report_id,
                }),
                payload: V3Payload {
                    status,
                    execution_ids,
                    claim_ids,
                    obstruction_kinds: ObstructionKinds(obstruction_kind_mask),
                },
            }),
        }
    }
}

#[derive(Clone, Copy)]
struct ArtifactSource<'a>(&'a reviewgraphen_core::ArtifactSourceV3);

impl Serialize for ArtifactSource<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use reviewgraphen_core::ArtifactSourceV3;
        match self.0 {
            ArtifactSourceV3::RunGenesis { run_id } => {
                let mut out = serializer.serialize_struct("ArtifactSourceV3", 2)?;
                out.serialize_field("kind", "run_genesis")?;
                out.serialize_field("run_id", run_id)?;
                out.end()
            }
            ArtifactSourceV3::SnapshotIngest {
                adapter_id,
                run_id,
                snapshot_id,
            } => {
                let mut out = serializer.serialize_struct("ArtifactSourceV3", 4)?;
                out.serialize_field("adapter_id", adapter_id)?;
                out.serialize_field("kind", "snapshot_ingest")?;
                out.serialize_field("run_id", run_id)?;
                out.serialize_field("snapshot_id", snapshot_id)?;
                out.end()
            }
            ArtifactSourceV3::ReviewerExecution {
                execution_id,
                reviewer_id,
                run_id,
            } => {
                let mut out = serializer.serialize_struct("ArtifactSourceV3", 4)?;
                out.serialize_field("execution_id", execution_id)?;
                out.serialize_field("kind", "reviewer_execution")?;
                out.serialize_field("reviewer_id", reviewer_id)?;
                out.serialize_field("run_id", run_id)?;
                out.end()
            }
            ArtifactSourceV3::VerifierArtifact {
                claim_id,
                descriptor_id,
                procedure_version,
                role,
                run_id,
            } => {
                let mut out = serializer.serialize_struct("ArtifactSourceV3", 6)?;
                out.serialize_field("claim_id", claim_id)?;
                out.serialize_field("descriptor_id", descriptor_id)?;
                out.serialize_field("kind", "verifier_artifact")?;
                out.serialize_field("procedure_version", procedure_version)?;
                out.serialize_field("role", role)?;
                out.serialize_field("run_id", run_id)?;
                out.end()
            }
            ArtifactSourceV3::ExternalHarnessWitness {
                claim_body_hash,
                claim_id,
                descriptor_id,
                genesis_hash,
                harness_id,
                harness_revision,
                harness_source_hash,
                policy_revision_hash,
                procedure_version,
                property_id,
                repository_id,
                repository_source_hash,
                run_id,
                snapshot_id,
                test_artifact_id,
                universe_id,
            } => {
                let mut out = serializer.serialize_struct("ArtifactSourceV3", 17)?;
                out.serialize_field("claim_body_hash", claim_body_hash)?;
                out.serialize_field("claim_id", claim_id)?;
                out.serialize_field("descriptor_id", descriptor_id)?;
                out.serialize_field("genesis_hash", genesis_hash)?;
                out.serialize_field("harness_id", harness_id)?;
                out.serialize_field("harness_revision", harness_revision)?;
                out.serialize_field("harness_source_hash", harness_source_hash)?;
                out.serialize_field("kind", "external_harness_witness")?;
                out.serialize_field("policy_revision_hash", policy_revision_hash)?;
                out.serialize_field("procedure_version", procedure_version)?;
                out.serialize_field("property_id", property_id)?;
                out.serialize_field("repository_id", repository_id)?;
                out.serialize_field("repository_source_hash", repository_source_hash)?;
                out.serialize_field("run_id", run_id)?;
                out.serialize_field("snapshot_id", snapshot_id)?;
                out.serialize_field("test_artifact_id", test_artifact_id)?;
                out.serialize_field("universe_id", universe_id)?;
                out.end()
            }
        }
    }
}

struct RegistrationRows<'a> {
    rows: &'a [IndexArtifactRegistrationV4],
    ids: &'a [StableId],
}
impl Serialize for RegistrationRows<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut out = s.serialize_seq(Some(self.ids.len()))?;
        for row in self
            .rows
            .iter()
            .filter(|r| contains_id(self.ids, &r.registration_id))
        {
            out.serialize_element(&Registration::from(row))?;
        }
        out.end()
    }
}
#[derive(Serialize)]
struct Registration<'a> {
    body_hash: &'a reviewgraphen_core::ContentHash,
    cas_hash: &'a reviewgraphen_core::ContentHash,
    event_id: &'a StableId,
    event_sequence: u64,
    media_type: &'a str,
    registration_id: &'a StableId,
    run_id: &'a StableId,
    sensitivity: &'a str,
    size: u64,
    source: ArtifactSource<'a>,
}
impl<'a> From<&'a IndexArtifactRegistrationV4> for Registration<'a> {
    fn from(r: &'a IndexArtifactRegistrationV4) -> Self {
        Self {
            event_sequence: r.event_sequence,
            event_id: &r.event_id,
            registration_id: &r.registration_id,
            run_id: &r.run_id,
            cas_hash: &r.cas_hash,
            media_type: &r.media_type,
            size: r.size,
            sensitivity: &r.sensitivity,
            source: ArtifactSource(&r.source),
            body_hash: &r.body_hash,
        }
    }
}

struct ExecutionRows<'a> {
    rows: &'a [reviewgraphen_store::IndexExecution],
    ids: &'a [StableId],
}
impl Serialize for ExecutionRows<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut out = s.serialize_seq(Some(self.ids.len()))?;
        for r in self
            .rows
            .iter()
            .filter(|r| contains_id(self.ids, &r.execution_id))
        {
            out.serialize_element(&Execution(r))?;
        }
        out.end()
    }
}
struct Execution<'a>(&'a reviewgraphen_store::IndexExecution);
impl Serialize for Execution<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let r = self.0;
        let outcome: Outcome<'_> =
            serde_json::from_str(&r.outcome_canonical_json).map_err(serde::ser::Error::custom)?;
        ExecutionFields {
            event_sequence: r.event_sequence,
            event_id: &r.event_id,
            id: &r.execution_id,
            plan_id: &r.plan_id,
            wave_id: &r.wave_id,
            obligation_ids: JsonStrings(&r.obligation_ids_canonical_json),
            envelope_id: &r.envelope_id,
            snapshot_id: &r.snapshot_id,
            reviewer_kind: &r.reviewer_kind,
            reviewer_id: &r.reviewer_id,
            provider: r.provider.as_deref(),
            model: r.model.as_deref(),
            model_revision: r.model_revision.as_deref(),
            system_prompt_version: &r.system_prompt_version,
            prompt_template_version: &r.prompt_template_version,
            inference_settings: JsonMap(&r.inference_settings_canonical_json),
            tool_policy_version: &r.tool_policy_version,
            tool_calls: JsonStrings(&r.tool_calls_canonical_json),
            attempt: r.attempt,
            raw_artifact_registration_id: &r.raw_registration_id,
            raw_artifact_hash: &r.raw_hash,
            parsed_claim_ids: JsonStrings(&r.parsed_claim_ids_canonical_json),
            outcome,
            identity_body_hash: &r.identity_body_hash,
            body_hash: &r.body_hash,
        }
        .serialize(s)
    }
}
#[derive(Serialize)]
struct ExecutionFields<'a> {
    attempt: u32,
    body_hash: &'a reviewgraphen_core::ContentHash,
    envelope_id: &'a StableId,
    event_id: &'a StableId,
    event_sequence: u64,
    id: &'a StableId,
    identity_body_hash: &'a reviewgraphen_core::ContentHash,
    inference_settings: JsonMap<'a>,
    model: Option<&'a str>,
    model_revision: Option<&'a str>,
    obligation_ids: JsonStrings<'a>,
    outcome: Outcome<'a>,
    parsed_claim_ids: JsonStrings<'a>,
    plan_id: &'a StableId,
    prompt_template_version: &'a str,
    provider: Option<&'a str>,
    raw_artifact_hash: &'a reviewgraphen_core::ContentHash,
    raw_artifact_registration_id: &'a StableId,
    reviewer_id: &'a str,
    reviewer_kind: &'a str,
    snapshot_id: &'a StableId,
    system_prompt_version: &'a str,
    tool_calls: JsonStrings<'a>,
    tool_policy_version: &'a str,
    wave_id: &'a StableId,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Outcome<'a> {
    Structured,
    Abstained {
        #[serde(borrow)]
        reason: &'a str,
        #[serde(borrow)]
        detail: &'a str,
    },
    Malformed {
        #[serde(borrow)]
        reason: &'a str,
        #[serde(borrow)]
        diagnostic: &'a str,
    },
    ProviderFailure {
        retryable: bool,
        #[serde(borrow)]
        diagnostic: &'a str,
    },
}

impl Serialize for Outcome<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Structured => {
                let mut out = serializer.serialize_struct("Outcome", 1)?;
                out.serialize_field("kind", "structured")?;
                out.end()
            }
            Self::Abstained { reason, detail } => {
                let mut out = serializer.serialize_struct("Outcome", 3)?;
                out.serialize_field("detail", detail)?;
                out.serialize_field("kind", "abstained")?;
                out.serialize_field("reason", reason)?;
                out.end()
            }
            Self::Malformed { reason, diagnostic } => {
                let mut out = serializer.serialize_struct("Outcome", 3)?;
                out.serialize_field("diagnostic", diagnostic)?;
                out.serialize_field("kind", "malformed")?;
                out.serialize_field("reason", reason)?;
                out.end()
            }
            Self::ProviderFailure {
                retryable,
                diagnostic,
            } => {
                let mut out = serializer.serialize_struct("Outcome", 3)?;
                out.serialize_field("diagnostic", diagnostic)?;
                out.serialize_field("kind", "provider_failure")?;
                out.serialize_field("retryable", retryable)?;
                out.end()
            }
        }
    }
}

struct ClaimRows<'a> {
    rows: &'a [IndexClaim],
    ids: &'a [StableId],
}
impl Serialize for ClaimRows<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut out = s.serialize_seq(Some(self.ids.len()))?;
        for r in self
            .rows
            .iter()
            .filter(|r| contains_id(self.ids, &r.claim_id))
        {
            out.serialize_element(&Claim(r))?;
        }
        out.end()
    }
}
struct Claim<'a>(&'a IndexClaim);
impl Serialize for Claim<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let r = self.0;
        let confidence: Option<f64> = serde_json::from_str(&r.candidate_confidence_canonical_json)
            .map_err(serde::ser::Error::custom)?;
        ClaimFields {
            event_sequence: r.event_sequence,
            event_id: &r.event_id,
            id: &r.claim_id,
            execution_id: &r.execution_id,
            obligation_ids: JsonStrings(&r.obligation_ids_canonical_json),
            property_id: &r.property_id,
            target_refs: JsonStrings(&r.target_refs_canonical_json),
            polarity: &r.polarity,
            disposition: &r.disposition,
            summary: &r.summary,
            source_ids: JsonStrings(&r.source_ids_canonical_json),
            assumptions: JsonStrings(&r.assumptions_canonical_json),
            requested_evidence: JsonStrings(&r.requested_evidence_canonical_json),
            candidate_confidence: confidence,
            author_kind: &r.author_kind,
            review_status: &r.review_status,
            identity_body_hash: &r.identity_body_hash,
            body_hash: &r.body_hash,
        }
        .serialize(s)
    }
}
#[derive(Serialize)]
struct ClaimFields<'a> {
    assumptions: JsonStrings<'a>,
    author_kind: &'a str,
    body_hash: &'a reviewgraphen_core::ContentHash,
    candidate_confidence: Option<f64>,
    disposition: &'a str,
    event_id: &'a StableId,
    event_sequence: u64,
    execution_id: &'a StableId,
    id: &'a StableId,
    identity_body_hash: &'a reviewgraphen_core::ContentHash,
    obligation_ids: JsonStrings<'a>,
    polarity: &'a str,
    property_id: &'a str,
    requested_evidence: JsonStrings<'a>,
    review_status: &'a str,
    source_ids: JsonStrings<'a>,
    summary: &'a str,
    target_refs: JsonStrings<'a>,
}

macro_rules! selected_rows {
    ($name:ident, $row:ty, $id:ident, $field:ident, $item:ident) => {
        struct $name<'a> {
            rows: &'a [$row],
            ids: &'a [StableId],
        }
        impl Serialize for $name<'_> {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut out = s.serialize_seq(Some(self.ids.len()))?;
                for row in self.rows.iter().filter(|r| contains_id(self.ids, &r.$id)) {
                    out.serialize_element(&<$item>::from(row))?;
                }
                out.end()
            }
        }
    };
}
selected_rows!(
    EvidenceRows,
    IndexEvidenceV3,
    evidence_id,
    evidence_id,
    Evidence
);
#[derive(Serialize)]
struct Evidence<'a> {
    body_hash: &'a reviewgraphen_core::ContentHash,
    descriptor_id: &'a str,
    event_id: &'a StableId,
    event_sequence: u64,
    id: &'a StableId,
    input_registration_id: &'a StableId,
    kind: &'a str,
    observation: &'a str,
    output_registration_id: &'a StableId,
    procedure_version: &'a str,
    schema: &'a str,
    snapshot_id: &'a StableId,
    subject_ids: JsonStrings<'a>,
}
impl<'a> From<&'a IndexEvidenceV3> for Evidence<'a> {
    fn from(r: &'a IndexEvidenceV3) -> Self {
        Self {
            event_sequence: r.event_sequence,
            event_id: &r.event_id,
            id: &r.evidence_id,
            schema: &r.schema,
            kind: &r.kind,
            snapshot_id: &r.snapshot_id,
            subject_ids: JsonStrings(&r.subject_ids_canonical_json),
            descriptor_id: &r.descriptor_id,
            procedure_version: &r.procedure_version,
            input_registration_id: &r.input_registration_id,
            output_registration_id: &r.output_registration_id,
            observation: &r.observation,
            body_hash: &r.body_hash,
        }
    }
}
selected_rows!(
    BindingRows,
    IndexEvidenceBindingV3,
    binding_id,
    binding_id,
    Binding
);
#[derive(Serialize)]
struct Binding<'a> {
    body_hash: &'a reviewgraphen_core::ContentHash,
    claim_id: &'a StableId,
    event_id: &'a StableId,
    event_sequence: u64,
    evidence_id: &'a StableId,
    id: &'a StableId,
    property_id: &'a str,
    relation: &'a str,
    schema: &'a str,
}
impl<'a> From<&'a IndexEvidenceBindingV3> for Binding<'a> {
    fn from(r: &'a IndexEvidenceBindingV3) -> Self {
        Self {
            event_sequence: r.event_sequence,
            event_id: &r.event_id,
            id: &r.binding_id,
            schema: &r.schema,
            claim_id: &r.claim_id,
            evidence_id: &r.evidence_id,
            relation: &r.relation,
            property_id: &r.property_id,
            body_hash: &r.body_hash,
        }
    }
}
selected_rows!(
    VerificationRows,
    IndexVerificationV3,
    verification_id,
    verification_id,
    Verification
);
#[derive(Serialize)]
struct Verification<'a> {
    body_hash: &'a reviewgraphen_core::ContentHash,
    claim_id: &'a StableId,
    descriptor_id: &'a str,
    event_id: &'a StableId,
    event_sequence: u64,
    evidence_ids: JsonStrings<'a>,
    id: &'a StableId,
    input_registration_id: &'a StableId,
    limitations: JsonStrings<'a>,
    outcome: &'a str,
    output_registration_id: &'a StableId,
    procedure_version: &'a str,
    schema: &'a str,
}
impl<'a> From<&'a IndexVerificationV3> for Verification<'a> {
    fn from(r: &'a IndexVerificationV3) -> Self {
        Self {
            event_sequence: r.event_sequence,
            event_id: &r.event_id,
            id: &r.verification_id,
            schema: &r.schema,
            claim_id: &r.claim_id,
            descriptor_id: &r.descriptor_id,
            procedure_version: &r.procedure_version,
            input_registration_id: &r.input_registration_id,
            output_registration_id: &r.output_registration_id,
            evidence_ids: JsonStrings(&r.evidence_ids_canonical_json),
            outcome: &r.outcome,
            limitations: JsonStrings(&r.limitations_canonical_json),
            body_hash: &r.body_hash,
        }
    }
}
selected_rows!(
    DecisionRows,
    IndexDecisionV3,
    decision_id,
    decision_id,
    Decision
);
#[derive(Serialize)]
struct Decision<'a> {
    actor: &'a str,
    authority_id: &'a str,
    body_hash: &'a reviewgraphen_core::ContentHash,
    claim_id: &'a StableId,
    event_id: &'a StableId,
    event_sequence: u64,
    expires_at: Option<&'a str>,
    id: &'a StableId,
    issued_at: &'a str,
    outcome: &'a str,
    policy_revision_hash: &'a reviewgraphen_core::ContentHash,
    property_id: &'a str,
    rationale: &'a str,
    run_id: &'a StableId,
    schema: &'a str,
    snapshot_id: &'a StableId,
    source_ids: JsonStrings<'a>,
    universe_id: &'a StableId,
}
impl<'a> From<&'a IndexDecisionV3> for Decision<'a> {
    fn from(r: &'a IndexDecisionV3) -> Self {
        Self {
            event_sequence: r.event_sequence,
            event_id: &r.event_id,
            id: &r.decision_id,
            schema: &r.schema,
            policy_revision_hash: &r.policy_revision_hash,
            run_id: &r.run_id,
            universe_id: &r.universe_id,
            claim_id: &r.claim_id,
            property_id: &r.property_id,
            outcome: &r.outcome,
            actor: &r.actor,
            authority_id: &r.authority_id,
            snapshot_id: &r.snapshot_id,
            source_ids: JsonStrings(&r.source_ids_canonical_json),
            rationale: &r.rationale,
            issued_at: &r.issued_at,
            expires_at: r.expires_at.as_deref(),
            body_hash: &r.body_hash,
        }
    }
}
selected_rows!(FindingRows, IndexFindingV3, finding_id, finding_id, Finding);
#[derive(Serialize)]
struct Finding<'a> {
    body_hash: &'a reviewgraphen_core::ContentHash,
    claim_id: &'a StableId,
    decision_id: Option<&'a StableId>,
    event_id: &'a StableId,
    event_sequence: u64,
    evidence_ids: JsonStrings<'a>,
    id: &'a StableId,
    projection_descriptor_id: &'a str,
    schema: &'a str,
    status: &'a str,
    supersedes_finding_id: Option<&'a StableId>,
    verification_ids: JsonStrings<'a>,
}
impl<'a> From<&'a IndexFindingV3> for Finding<'a> {
    fn from(r: &'a IndexFindingV3) -> Self {
        Self {
            event_sequence: r.event_sequence,
            event_id: &r.event_id,
            id: &r.finding_id,
            schema: &r.schema,
            projection_descriptor_id: &r.projection_descriptor_id,
            claim_id: &r.claim_id,
            status: &r.status,
            evidence_ids: JsonStrings(&r.evidence_ids_canonical_json),
            verification_ids: JsonStrings(&r.verification_ids_canonical_json),
            decision_id: r.decision_id.as_ref(),
            supersedes_finding_id: r.supersedes_finding_id.as_ref(),
            body_hash: &r.body_hash,
        }
    }
}
struct AssessmentRows<'a> {
    rows: &'a [IndexClaimAssessmentV3],
    ids: &'a [StableId],
}
impl Serialize for AssessmentRows<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut out = s.serialize_seq(Some(self.ids.len()))?;
        for r in self
            .rows
            .iter()
            .filter(|r| contains_id(self.ids, &r.claim_id))
        {
            out.serialize_element(&Assessment::from(r))?;
        }
        out.end()
    }
}
#[derive(Serialize)]
struct Assessment<'a> {
    active_decision_id: Option<&'a StableId>,
    binding_ids: JsonStrings<'a>,
    claim_id: &'a StableId,
    confirmed_event_sequence: u64,
    current_finding_id: Option<&'a StableId>,
    decision_conflict: bool,
    decision_ids: JsonStrings<'a>,
    disposition: &'a str,
    evidence_ids: JsonStrings<'a>,
    finding_ids: JsonStrings<'a>,
    review_status: &'a str,
    verification_ids: JsonStrings<'a>,
}
impl<'a> From<&'a IndexClaimAssessmentV3> for Assessment<'a> {
    fn from(r: &'a IndexClaimAssessmentV3) -> Self {
        Self {
            claim_id: &r.claim_id,
            disposition: &r.disposition,
            review_status: &r.review_status,
            binding_ids: JsonStrings(&r.binding_ids_canonical_json),
            evidence_ids: JsonStrings(&r.evidence_ids_canonical_json),
            verification_ids: JsonStrings(&r.verification_ids_canonical_json),
            decision_ids: JsonStrings(&r.decision_ids_canonical_json),
            finding_ids: JsonStrings(&r.finding_ids_canonical_json),
            active_decision_id: r.active_decision_id.as_ref(),
            current_finding_id: r.current_finding_id.as_ref(),
            decision_conflict: r.decision_conflict,
            confirmed_event_sequence: r.confirmed_event_sequence,
        }
    }
}

struct ObstructionRows<'a> {
    rows: &'a [reviewgraphen_store::IndexExecution],
    ids: &'a [StableId],
}
impl Serialize for ObstructionRows<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let count = self
            .rows
            .iter()
            .filter(|r| contains_id(self.ids, &r.execution_id))
            .count();
        let mut out = s.serialize_seq(Some(count))?;
        for r in self
            .rows
            .iter()
            .filter(|r| contains_id(self.ids, &r.execution_id))
        {
            out.serialize_element(&Obstruction {
                kind: obstruction_kind(&r.outcome_kind)
                    .ok_or_else(|| serde::ser::Error::custom("unknown reviewer outcome"))?,
                message: "The selected obligation remains in progress after this reviewer outcome.",
                source_ids: [&r.execution_id],
                blocks: JsonStrings(&r.obligation_ids_canonical_json),
            })?;
        }
        out.end()
    }
}
#[derive(Serialize)]
struct Obstruction<'a> {
    blocks: JsonStrings<'a>,
    kind: &'static str,
    message: &'static str,
    source_ids: [&'a StableId; 1],
}
fn obstruction_kind(value: &str) -> Option<&'static str> {
    obstruction_kind_and_bit(value).map(|(kind, _)| kind)
}
const OBSTRUCTION_KINDS: [(u8, &str); 3] = [
    (1, "reviewer_abstained"),
    (2, "reviewer_malformed"),
    (4, "reviewer_provider_failure"),
];
fn obstruction_kind_and_bit(value: &str) -> Option<(&'static str, u8)> {
    match value {
        "abstained" => Some((OBSTRUCTION_KINDS[0].1, OBSTRUCTION_KINDS[0].0)),
        "malformed" => Some((OBSTRUCTION_KINDS[1].1, OBSTRUCTION_KINDS[1].0)),
        "provider_failure" => Some((OBSTRUCTION_KINDS[2].1, OBSTRUCTION_KINDS[2].0)),
        _ => None,
    }
}
struct ObstructionKinds(u8);
impl Serialize for ObstructionKinds {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let n = OBSTRUCTION_KINDS
            .iter()
            .filter(|(bit, _)| self.0 & bit != 0)
            .count();
        let mut out = s.serialize_seq(Some(n))?;
        for (bit, kind) in OBSTRUCTION_KINDS {
            if self.0 & bit != 0 {
                out.serialize_element(kind)?;
            }
        }
        out.end()
    }
}

#[derive(Clone, Copy)]
struct JsonStrings<'a>(&'a str);
impl Serialize for JsonStrings<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut out = s.serialize_seq(None)?;
        let mut de = serde_json::Deserializer::from_str(self.0);
        de.deserialize_seq(ForwardStrings(&mut out))
            .map_err(serde::ser::Error::custom)?;
        de.end().map_err(serde::ser::Error::custom)?;
        out.end()
    }
}
struct ForwardStrings<'a, S>(&'a mut S);
impl<'de, S: SerializeSeq> Visitor<'de> for ForwardStrings<'_, S> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JSON string array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        while let Some(v) = a.next_element::<&'de str>()? {
            self.0
                .serialize_element(v)
                .map_err(|e| serde::de::Error::custom(e.to_string()))?;
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
struct JsonMap<'a>(&'a str);
impl Serialize for JsonMap<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut out = s.serialize_map(None)?;
        let mut de = serde_json::Deserializer::from_str(self.0);
        de.deserialize_map(ForwardMap(&mut out))
            .map_err(serde::ser::Error::custom)?;
        de.end().map_err(serde::ser::Error::custom)?;
        out.end()
    }
}
struct ForwardMap<'a, S>(&'a mut S);
impl<'de, S: SerializeMap> Visitor<'de> for ForwardMap<'_, S> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JSON string map")
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        while let Some((k, v)) = a.next_entry::<&'de str, &'de str>()? {
            self.0
                .serialize_entry(k, v)
                .map_err(|e| serde::de::Error::custom(e.to_string()))?;
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn largest_record(
    shape: &V3Report<'_>,
    snapshot: &IndexSnapshotV4,
    registrations: &[StableId],
    executions: &[StableId],
    claims: &[StableId],
    evidence: &[StableId],
    bindings: &[StableId],
    verifications: &[StableId],
    decisions: &[StableId],
    findings: &[StableId],
    assessments: &[StableId],
) -> Result<u64, ReportError> {
    let mut max = 0;
    macro_rules! rows {
        ($it:expr) => {
            for v in $it {
                max = max.max(json_encoded_len(&v, "largest_record_bytes", u64::MAX)?);
            }
        };
    }
    rows!(
        snapshot
            .artifact_registrations
            .iter()
            .filter(|r| contains_id(registrations, &r.registration_id))
            .map(Registration::from)
    );
    rows!(
        snapshot
            .executions
            .iter()
            .filter(|r| contains_id(executions, &r.execution_id))
            .map(Execution)
    );
    rows!(
        snapshot
            .claims
            .iter()
            .filter(|r| contains_id(claims, &r.claim_id))
            .map(Claim)
    );
    rows!(
        snapshot
            .evidence
            .iter()
            .filter(|r| contains_id(evidence, &r.evidence_id))
            .map(Evidence::from)
    );
    rows!(
        snapshot
            .evidence_bindings
            .iter()
            .filter(|r| contains_id(bindings, &r.binding_id))
            .map(Binding::from)
    );
    rows!(
        snapshot
            .verifications
            .iter()
            .filter(|r| contains_id(verifications, &r.verification_id))
            .map(Verification::from)
    );
    rows!(
        snapshot
            .decisions
            .iter()
            .filter(|r| contains_id(decisions, &r.decision_id))
            .map(Decision::from)
    );
    rows!(
        snapshot
            .findings
            .iter()
            .filter(|r| contains_id(findings, &r.finding_id))
            .map(Finding::from)
    );
    rows!(
        snapshot
            .claim_assessments
            .iter()
            .filter(|r| contains_id(assessments, &r.claim_id))
            .map(Assessment::from)
    );
    rows!(std::iter::once(&shape.projection.views.0));
    for r in snapshot
        .executions
        .iter()
        .filter(|r| contains_id(executions, &r.execution_id) && r.outcome_kind != "structured")
    {
        max = max.max(json_encoded_len(
            &Obstruction {
                kind: obstruction_kind(&r.outcome_kind).ok_or(ReportError::Json)?,
                message: "The selected obligation remains in progress after this reviewer outcome.",
                source_ids: [&r.execution_id],
                blocks: JsonStrings(&r.obligation_ids_canonical_json),
            },
            "largest_record_bytes",
            u64::MAX,
        )?);
    }
    Ok(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reviewgraphen_core::{
        ArtifactSourceV3, ContentHash, VerifierArtifactRoleV3, canonical_json,
    };
    use reviewgraphen_store::{
        IndexArtifactRegistrationV4, IndexClaim, IndexClaimAssessmentV3, IndexDecisionV3,
        IndexEvidenceBindingV3, IndexEvidenceV3, IndexExecution, IndexFindingV3, IndexMarkerV4,
        IndexUniverse, IndexVerificationV3,
    };

    fn id(value: &str) -> StableId {
        StableId::parse(value).unwrap()
    }

    fn assert_canonical<T: Serialize>(value: &T) {
        let streamed = serde_json::to_vec(value).unwrap();
        let decoded: serde_json::Value = serde_json::from_slice(&streamed).unwrap();
        assert_eq!(streamed, canonical_json(&decoded).unwrap());
    }

    #[test]
    fn outcomes_and_artifact_sources_stream_lexicographic_objects() {
        for outcome in [
            Outcome::Structured,
            Outcome::Abstained {
                reason: "insufficient_context",
                detail: "detail",
            },
            Outcome::Malformed {
                reason: "schema_violation",
                diagnostic: "diagnostic",
            },
            Outcome::ProviderFailure {
                retryable: true,
                diagnostic: "diagnostic",
            },
        ] {
            assert_canonical(&outcome);
        }

        let hash = ContentHash::sha256(b"canonical-source");
        let run = id("run:canonical");
        let sources = [
            ArtifactSourceV3::RunGenesis {
                run_id: run.clone(),
            },
            ArtifactSourceV3::SnapshotIngest {
                adapter_id: "adapter@1".into(),
                run_id: run.clone(),
                snapshot_id: id("snapshot:canonical"),
            },
            ArtifactSourceV3::ReviewerExecution {
                execution_id: id("execution:canonical"),
                reviewer_id: "reviewer@1".into(),
                run_id: run.clone(),
            },
            ArtifactSourceV3::VerifierArtifact {
                claim_id: id("claim:canonical"),
                descriptor_id: "descriptor@1".into(),
                procedure_version: "procedure@1".into(),
                role: VerifierArtifactRoleV3::Output,
                run_id: run.clone(),
            },
            ArtifactSourceV3::ExternalHarnessWitness {
                claim_body_hash: hash.clone(),
                claim_id: id("claim:canonical"),
                descriptor_id: "descriptor@1".into(),
                genesis_hash: hash.clone(),
                harness_id: "harness@1".into(),
                harness_revision: "revision@1".into(),
                harness_source_hash: hash.clone(),
                policy_revision_hash: hash.clone(),
                procedure_version: "procedure@1".into(),
                property_id: "payment.at_most_once".into(),
                repository_id: id("repository:canonical"),
                repository_source_hash: hash,
                run_id: run,
                snapshot_id: id("snapshot:canonical"),
                test_artifact_id: id("test:canonical"),
                universe_id: id("universe:canonical"),
            },
        ];
        for source in &sources {
            assert_canonical(&ArtifactSource(source));
        }
    }

    #[test]
    fn representative_nested_rows_stream_lexicographic_objects() {
        let hash = ContentHash::sha256(b"canonical-row");
        let execution = IndexExecution {
            event_sequence: 1,
            event_id: id("event:canonical"),
            execution_id: id("execution:canonical"),
            plan_id: id("plan:canonical"),
            wave_id: id("wave:canonical"),
            snapshot_id: id("snapshot:canonical"),
            envelope_id: id("envelope:canonical"),
            obligation_ids_canonical_json: r#"["obligation:canonical"]"#.into(),
            reviewer_kind: "fake".into(),
            reviewer_id: "reviewgraphen.fake_reviewer@1".into(),
            provider: None,
            model: None,
            model_revision: None,
            system_prompt_version: "reviewgraphen.system.no_tools@1".into(),
            prompt_template_version: "fixture@1".into(),
            inference_settings_canonical_json: "{}".into(),
            tool_policy_version: "reviewgraphen.tool_policy.none@1".into(),
            tool_calls_canonical_json: "[]".into(),
            attempt: 1,
            raw_registration_id: id("registration:canonical"),
            raw_hash: hash.clone(),
            parsed_claim_ids_canonical_json: r#"["claim:canonical"]"#.into(),
            outcome_kind: "abstained".into(),
            outcome_canonical_json:
                r#"{"detail":"detail","kind":"abstained","reason":"insufficient_context"}"#.into(),
            identity_body_hash: hash.clone(),
            body_hash: hash.clone(),
        };
        assert_canonical(&Execution(&execution));
        assert_canonical(&Obstruction {
            blocks: JsonStrings(&execution.obligation_ids_canonical_json),
            kind: "reviewer_abstained",
            message: "message",
            source_ids: [&execution.execution_id],
        });

        let finding = IndexFindingV3 {
            event_sequence: 2,
            event_id: id("event:finding"),
            finding_id: id("finding:canonical"),
            schema: "reviewgraphen.finding.v3".into(),
            projection_descriptor_id: "reviewgraphen.finding_projection@1".into(),
            claim_id: id("claim:canonical"),
            status: "accepted".into(),
            evidence_ids_canonical_json: r#"["evidence:canonical"]"#.into(),
            verification_ids_canonical_json: r#"["verification:canonical"]"#.into(),
            decision_id: Some(id("decision:canonical")),
            supersedes_finding_id: Some(id("finding:prior")),
            body_hash: hash,
        };
        assert_canonical(&Finding::from(&finding));

        let selected = BTreeSet::from([id("obligation:canonical")]);
        let view = V3View {
            information_loss: One(V3Loss {
                affected_properties: ["review.authority_trace"],
                kind: "authority_detail_omitted",
                meaningful: true,
                reason: "reason",
                recoverable: true,
                recovery_ref: &id("report:canonical"),
                source_ids: &selected,
            }),
            kind: "machine",
            payload: V3Payload {
                claim_ids: &[],
                execution_ids: &[],
                obstruction_kinds: ObstructionKinds(1),
                status: Status::Partial,
            },
            source_ids: &selected,
        };
        assert_canonical(&view);
    }

    #[test]
    fn every_v3_row_and_envelope_shape_streams_canonical_keys() {
        let hash = ContentHash::sha256(b"all-row-shapes");
        let event_id = id("event:all-rows");
        let claim_id = id("claim:all-rows");
        let execution_id = id("execution:all-rows");
        let registration_id = id("registration:all-rows");
        let evidence_id = id("evidence:all-rows");
        let verification_id = id("verification:all-rows");
        let decision_id = id("decision:all-rows");
        let finding_id = id("finding:all-rows");
        let obligation_id = id("obligation:all-rows");
        let obligation_json = r#"["obligation:all-rows"]"#;

        let registration = IndexArtifactRegistrationV4 {
            event_sequence: 1,
            event_id: event_id.clone(),
            registration_id: registration_id.clone(),
            run_id: id("run:all-rows"),
            cas_hash: hash.clone(),
            media_type: "application/json".into(),
            size: 1,
            sensitivity: "canonical_state".into(),
            source_kind: "run_genesis".into(),
            source_canonical_json: r#"{"kind":"run_genesis","run_id":"run:all-rows"}"#.into(),
            source: ArtifactSourceV3::RunGenesis {
                run_id: id("run:all-rows"),
            },
            body_hash: hash.clone(),
        };
        assert_canonical(&Registration::from(&registration));

        let claim = IndexClaim {
            event_sequence: 2,
            event_id: event_id.clone(),
            claim_id: claim_id.clone(),
            execution_id: execution_id.clone(),
            obligation_ids_canonical_json: obligation_json.into(),
            property_id: "payment.at_most_once".into(),
            target_refs_canonical_json: r#"["function:charge"]"#.into(),
            polarity: "issue_present".into(),
            disposition: "proposed".into(),
            summary: "summary".into(),
            source_ids_canonical_json: r#"["file:payment"]"#.into(),
            assumptions_canonical_json: "[]".into(),
            requested_evidence_canonical_json: "[]".into(),
            candidate_confidence_canonical_json: "1.0".into(),
            author_kind: "ai".into(),
            review_status: "unreviewed".into(),
            identity_body_hash: hash.clone(),
            body_hash: hash.clone(),
        };
        assert_canonical(&Claim(&claim));

        let evidence = IndexEvidenceV3 {
            event_sequence: 3,
            event_id: event_id.clone(),
            evidence_id: evidence_id.clone(),
            schema: "reviewgraphen.evidence.v3".into(),
            kind: "test_witness".into(),
            snapshot_id: id("snapshot:all-rows"),
            subject_ids_canonical_json: r#"["test:double-submit"]"#.into(),
            descriptor_id: "reviewgraphen.fixture_test_verifier@1".into(),
            procedure_version: "reviewgraphen.fixture_test.duplicate_submit@1".into(),
            input_registration_id: registration_id.clone(),
            output_registration_id: registration_id.clone(),
            observation: "witnessed".into(),
            body_hash: hash.clone(),
        };
        assert_canonical(&Evidence::from(&evidence));

        let binding = IndexEvidenceBindingV3 {
            event_sequence: 4,
            event_id: event_id.clone(),
            binding_id: id("binding:all-rows"),
            schema: "reviewgraphen.evidence_binding.v3".into(),
            claim_id: claim_id.clone(),
            evidence_id: evidence_id.clone(),
            relation: "reproduces".into(),
            property_id: "payment.at_most_once".into(),
            body_hash: hash.clone(),
        };
        assert_canonical(&Binding::from(&binding));

        let verification = IndexVerificationV3 {
            event_sequence: 5,
            event_id: event_id.clone(),
            verification_id: verification_id.clone(),
            schema: "reviewgraphen.verification.v3".into(),
            claim_id: claim_id.clone(),
            descriptor_id: "reviewgraphen.fixture_test_verifier@1".into(),
            procedure_version: "reviewgraphen.fixture_test.duplicate_submit@1".into(),
            input_registration_id: registration_id.clone(),
            output_registration_id: registration_id.clone(),
            evidence_ids_canonical_json: r#"["evidence:all-rows"]"#.into(),
            outcome: "passed".into(),
            limitations_canonical_json: r#"["bounded fixture"]"#.into(),
            body_hash: hash.clone(),
        };
        assert_canonical(&Verification::from(&verification));

        let decision = IndexDecisionV3 {
            event_sequence: 6,
            event_id: event_id.clone(),
            decision_id: decision_id.clone(),
            schema: "reviewgraphen.human_decision.v3".into(),
            policy_revision_hash: hash.clone(),
            run_id: id("run:all-rows"),
            universe_id: id("universe:all-rows"),
            claim_id: claim_id.clone(),
            property_id: "payment.at_most_once".into(),
            outcome: "accept".into(),
            actor: "human:reviewer".into(),
            authority_id: "board".into(),
            snapshot_id: id("snapshot:all-rows"),
            source_ids_canonical_json: r#"["claim:all-rows"]"#.into(),
            rationale: "accepted".into(),
            issued_at: "2026-08-10T00:00:00Z".into(),
            expires_at: None,
            body_hash: hash.clone(),
        };
        assert_canonical(&Decision::from(&decision));

        let finding = IndexFindingV3 {
            event_sequence: 7,
            event_id,
            finding_id: finding_id.clone(),
            schema: "reviewgraphen.finding.v3".into(),
            projection_descriptor_id: "reviewgraphen.finding_projection@1".into(),
            claim_id: claim_id.clone(),
            status: "accepted".into(),
            evidence_ids_canonical_json: r#"["evidence:all-rows"]"#.into(),
            verification_ids_canonical_json: r#"["verification:all-rows"]"#.into(),
            decision_id: Some(decision_id.clone()),
            supersedes_finding_id: None,
            body_hash: hash.clone(),
        };
        assert_canonical(&Finding::from(&finding));

        let assessment = IndexClaimAssessmentV3 {
            claim_id: claim_id.clone(),
            disposition: "accepted".into(),
            review_status: "accepted".into(),
            binding_ids_canonical_json: r#"["binding:all-rows"]"#.into(),
            evidence_ids_canonical_json: r#"["evidence:all-rows"]"#.into(),
            verification_ids_canonical_json: r#"["verification:all-rows"]"#.into(),
            decision_ids_canonical_json: r#"["decision:all-rows"]"#.into(),
            finding_ids_canonical_json: r#"["finding:all-rows"]"#.into(),
            active_decision_id: Some(decision_id),
            current_finding_id: Some(finding_id),
            decision_conflict: false,
            confirmed_event_sequence: 7,
        };
        assert_canonical(&Assessment::from(&assessment));

        let selected = BTreeSet::from([obligation_id.clone()]);
        let selected_ids = vec![obligation_id];
        let empty: Vec<StableId> = Vec::new();
        let report_id = id("report:all-rows");
        let report = V3Report {
            coverage: V3Coverage {
                accepted: 0,
                accepted_obligation_ids: &empty,
                completed: 0,
                completed_obligation_ids: &empty,
                denominator_obligation_ids: &selected_ids,
                evidence_supported: 0,
                evidence_supported_obligation_ids: &empty,
                fresh_verified: 0,
                fresh_verified_obligation_ids: &empty,
                selected: 1,
                universe_id: &id("universe:all-rows"),
                verified: 0,
                verified_obligation_ids: &empty,
                visited: 0,
                visited_obligation_ids: &empty,
            },
            metadata: V3Metadata {
                authority_policy_revision_hash: &hash,
                authority_replay_basis_digest: &hash,
                confirmed_event_count: 7,
                confirmed_offset: 7,
                confirmed_tail_hash: &hash,
                event_contract_version: "reviewgraphen.review_event.v3",
                extractor_set_hash: &hash,
                genesis_hash: &hash,
                index_projection_version: "reviewgraphen.index_projection.v4",
                policy_version: "policy@1",
                profile_id: "profile@1",
                report_id: &report_id,
                rule_set_hash: &hash,
                run_id: &id("run:all-rows"),
                tool_versions: &BTreeMap::from([("tool".into(), "1".into())]),
            },
            projection: V3Projection {
                views: One(V3View {
                    information_loss: One(V3Loss {
                        affected_properties: ["review.authority_trace"],
                        kind: "authority_detail_omitted",
                        meaningful: true,
                        reason: "reason",
                        recoverable: true,
                        recovery_ref: &report_id,
                        source_ids: &selected,
                    }),
                    kind: "machine",
                    payload: V3Payload {
                        claim_ids: &empty,
                        execution_ids: &empty,
                        obstruction_kinds: ObstructionKinds(0),
                        status: Status::Partial,
                    },
                    source_ids: &selected,
                }),
            },
            report_type: "review",
            report_version: 3,
            result: V3Result {
                artifact_registrations: RegistrationRows {
                    rows: &[],
                    ids: &empty,
                },
                claim_assessments: AssessmentRows {
                    rows: &[],
                    ids: &empty,
                },
                claims: ClaimRows {
                    rows: &[],
                    ids: &empty,
                },
                decisions: DecisionRows {
                    rows: &[],
                    ids: &empty,
                },
                evidence: EvidenceRows {
                    rows: &[],
                    ids: &empty,
                },
                evidence_bindings: BindingRows {
                    rows: &[],
                    ids: &empty,
                },
                executions: ExecutionRows {
                    rows: &[],
                    ids: &empty,
                },
                findings: FindingRows {
                    rows: &[],
                    ids: &empty,
                },
                obstructions: ObstructionRows {
                    rows: &[],
                    ids: &empty,
                },
                status: Status::Partial,
                verifications: VerificationRows {
                    rows: &[],
                    ids: &empty,
                },
            },
            scenario: V3Scenario {
                artifact_registration_ids: &empty,
                plan_id: &id("plan:all-rows"),
                program_space_ref: &id("program-space:snapshot:all-rows"),
                repository_id: &id("repository:all-rows"),
                selected_obligation_ids: &selected,
                snapshot_id: &id("snapshot:all-rows"),
                universe_id: &id("universe:all-rows"),
            },
            schema: SCHEMA_V3,
        };
        assert_canonical(&report);
    }

    #[test]
    fn pass_one_charge_assembly_matches_the_materialized_report_shape() {
        let digest = ContentHash::sha256(b"v3-charge");
        let universe = IndexUniverse {
            universe_id: id("universe:charge"),
            snapshot_id: id("snapshot:charge"),
            profile_id: "profile@1".into(),
            rule_set_hash: digest.clone(),
            extractor_set_hash: digest.clone(),
            policy_version: "policy@1".into(),
            rule_pack_version: "rules@1".into(),
            body_hash: digest.clone(),
        };
        let snapshot = IndexSnapshotV4 {
            marker: IndexMarkerV4 {
                index_schema_version: 4,
                sqlite_user_version: 4,
                projection_contract_version: INDEX_V4.into(),
                event_contract_version: EventContractVersion::V3.schema().into(),
                projection_mode: "authority_replay".into(),
                run_id: id("run:charge"),
                genesis_hash: digest.clone(),
                confirmed_offset: 1,
                tail_hash: digest.clone(),
                event_count: 1,
                policy_revision_hash: digest.clone(),
                authority_replay_basis_digest: digest.clone(),
            },
            events: Vec::new(),
            shadows: Vec::new(),
            projected_findings: Vec::new(),
            program_objects: Vec::new(),
            program_relations: Vec::new(),
            universe: Some(universe.clone()),
            obligations: Vec::new(),
            obligation_lifecycle: Vec::new(),
            executions: Vec::new(),
            claims: Vec::new(),
            artifact_registrations: Vec::new(),
            snapshot_sources: Vec::new(),
            context_envelopes: Vec::new(),
            review_plans: Vec::new(),
            evidence: Vec::new(),
            evidence_bindings: Vec::new(),
            verifications: Vec::new(),
            decisions: Vec::new(),
            findings: Vec::new(),
            claim_assessments: Vec::new(),
            policy_revision_hash: digest.clone(),
            authority_replay_basis_digest: digest,
        };
        let selected_id = id("obligation:charge");
        let request = ReportRequestV3 {
            report_id: id("report:charge"),
            repository_id: id("repository:charge"),
            program_space_ref: id("program-space:snapshot:charge"),
            plan_id: id("plan:charge"),
            selected_obligation_ids: BTreeSet::from([selected_id.clone()]),
            tool_versions: BTreeMap::from([("tool".into(), "1".into())]),
        };
        let mut pass_one = ChargeVisitor::default();
        pass_one.denominator.list_serialized(&selected_id).unwrap();
        let charged =
            assemble_report_charge(&request, &snapshot, &universe, Status::Partial, &pass_one)
                .unwrap();
        let denominator = vec![selected_id];
        let empty: Vec<StableId> = Vec::new();
        let shape = V3Report {
            coverage: V3Coverage::new(
                &universe,
                &denominator,
                &request.selected_obligation_ids,
                &empty,
                &empty,
                &empty,
                &empty,
                &empty,
            ),
            metadata: V3Metadata::new(&request, &snapshot, &universe),
            projection: V3Projection::new(&request, Status::Partial, &empty, &empty, 0),
            report_type: "review",
            report_version: 3,
            result: V3Result {
                artifact_registrations: RegistrationRows {
                    rows: &snapshot.artifact_registrations,
                    ids: &empty,
                },
                claim_assessments: AssessmentRows {
                    rows: &snapshot.claim_assessments,
                    ids: &empty,
                },
                claims: ClaimRows {
                    rows: &snapshot.claims,
                    ids: &empty,
                },
                decisions: DecisionRows {
                    rows: &snapshot.decisions,
                    ids: &empty,
                },
                evidence: EvidenceRows {
                    rows: &snapshot.evidence,
                    ids: &empty,
                },
                evidence_bindings: BindingRows {
                    rows: &snapshot.evidence_bindings,
                    ids: &empty,
                },
                executions: ExecutionRows {
                    rows: &snapshot.executions,
                    ids: &empty,
                },
                findings: FindingRows {
                    rows: &snapshot.findings,
                    ids: &empty,
                },
                obstructions: ObstructionRows {
                    rows: &snapshot.executions,
                    ids: &empty,
                },
                status: Status::Partial,
                verifications: VerificationRows {
                    rows: &snapshot.verifications,
                    ids: &empty,
                },
            },
            scenario: V3Scenario::new(&request, &universe, &empty),
            schema: SCHEMA_V3,
        };
        assert_eq!(charged, ownership_charge(&shape).unwrap());
    }

    #[test]
    fn projection_peak_refuses_plus_one_before_pass_two_materialization() {
        let materializations = std::cell::Cell::new(0_u8);
        let counts = ReportCounts {
            views: 1,
            information_loss_records: 1,
            ..ReportCounts::default()
        };
        let exact = ReportLimits {
            working_bytes: 100,
            ..ReportLimits::default()
        };
        preflight_report_materialization(exact, counts, 20, 30, 50, || {
            materializations.set(materializations.get() + 1);
            Ok(())
        })
        .unwrap();
        assert_eq!(materializations.get(), 1);
        materializations.set(0);
        let refused = preflight_report_materialization(exact, counts, 20, 30, 51, || {
            materializations.set(materializations.get() + 1);
            Ok(())
        })
        .unwrap_err();
        assert!(matches!(
            refused,
            ReportError::Incomplete { observed: 101, .. }
        ));
        assert_eq!(materializations.get(), 0);
    }
}
