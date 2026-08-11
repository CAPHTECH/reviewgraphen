//! Source-bound `reviewgraphen.review.report.v4` projection.
//!
//! The v4 path intentionally keeps report records borrowed from the verified
//! v5 snapshot. Selection, closure, counts, `I5`, and `Rr4` are calculated
//! before a report row or output buffer is allocated.

use crate::{
    LogicalCharge, OwnershipError, ReportAccounting, ReportCounts, ReportError, ReportLimits,
    bounded_json_bytes, json_encoded_len, ownership_charge,
};
use reviewgraphen_core::{
    AuthorityTrustRootsV4, EventContractVersion, M5DoubleSubmitAssignmentsV4, StableId,
};
use reviewgraphen_store::{
    DerivedIndexV5, EventJournal, IndexArtifactRegistrationV5, IndexClaim,
    IndexClaimAssessmentV3AtV5, IndexDecisionV3AtV5, IndexEvidenceBindingV3AtV5,
    IndexEvidenceV3AtV5, IndexFindingV3AtV5, IndexSnapshotV5, IndexVerificationV3AtV5,
    JournalIdentity, M5ReportAuthorityInspectionV4, StoreRoot, V5CoverageAxis, V5SelectionItem,
    V5SelectionRequest, V5SelectionSummary, V5SelectionVisitError, V5SelectionVisitor,
    ValidatedIndexSnapshotV5,
};
use serde::{
    Deserialize, Serialize,
    de::{Deserializer, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq, SerializeStruct},
};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const SCHEMA_V4: &str = "reviewgraphen.review.report.v4";
const INDEX_V5: &str = "reviewgraphen.index_projection.v5";
const M5_REGISTRATION_ITEM_SCHEMA: &str = "reviewgraphen.artifact_registration.v4.report_item";
const M5_DESCRIPTOR_ITEM_SCHEMA: &str = "reviewgraphen.gluing_input_descriptor.v4.report_item";
const M5_REGISTRATION_ACTOR: &str = "engine:reviewgraphen.m5_gluing_input@1";

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("M5 bundle is incomplete after {registered_inputs} durable gluing-input registrations")]
pub struct M5BundleIncomplete {
    pub registered_inputs: u64,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("Report V4 semantic contract failed: {0}")]
pub struct ReportV4SemanticError(&'static str);

/// Checks Report V4 cross-field semantics which Draft 2020-12 cannot express:
/// positional context order, exact attempt/result linkage, nested body hashes,
/// and the one-loss-per-omitted-M5-array correspondence.
pub fn validate_v4_semantics(report: &serde_json::Value) -> Result<(), ReportV4SemanticError> {
    validate_v4_semantics_inner(report).map_err(ReportV4SemanticError)
}

/// Inclusive Report V4 bounds. The frozen V3 limits remain embedded
/// unchanged; M5 topology rows have their own exact cardinality caps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReportLimitsV4 {
    pub inherited: ReportLimits,
    pub artifact_registrations_v4: u64,
    pub gluing_input_descriptors: u64,
    pub context_covers: u64,
    pub sections: u64,
    pub gluing_attempts: u64,
    pub restrictions: u64,
    pub global_candidates: u64,
    pub gluing_obstructions: u64,
}

impl Default for ReportLimitsV4 {
    fn default() -> Self {
        Self {
            inherited: ReportLimits::default(),
            artifact_registrations_v4: 2,
            gluing_input_descriptors: 2,
            context_covers: 1,
            sections: 2,
            gluing_attempts: 1,
            restrictions: 2,
            global_candidates: 1,
            gluing_obstructions: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct M5ReportCounts {
    artifact_registrations_v4: u64,
    gluing_input_descriptors: u64,
    context_covers: u64,
    sections: u64,
    gluing_attempts: u64,
    restrictions: u64,
    global_candidates: u64,
    gluing_obstructions: u64,
}

impl M5ReportCounts {
    fn rows(self) -> Result<u64, ReportError> {
        [
            self.artifact_registrations_v4,
            self.gluing_input_descriptors,
            self.context_covers,
            self.sections,
            self.gluing_attempts,
            self.restrictions,
            self.global_candidates,
            self.gluing_obstructions,
        ]
        .into_iter()
        .try_fold(0_u64, |sum, count| {
            sum.checked_add(count)
                .ok_or_else(|| incomplete("report_v4_rows", u64::MAX))
        })
    }

    fn omission_losses(self) -> u64 {
        [
            self.gluing_input_descriptors,
            self.context_covers,
            self.sections,
            self.gluing_attempts,
            self.restrictions,
            self.global_candidates,
            self.gluing_obstructions,
        ]
        .into_iter()
        .filter(|count| *count != 0)
        .count() as u64
    }
}

#[derive(Clone, Copy)]
struct ReportCountsV4 {
    inherited: ReportCounts,
    m5: M5ReportCounts,
}

impl ReportLimitsV4 {
    fn preflight(
        self,
        inherited: ReportCounts,
        m5: M5ReportCounts,
        journal_bytes: u64,
        index_bytes: u64,
        reserved: u64,
    ) -> Result<(), ReportError> {
        self.inherited
            .preflight(inherited, journal_bytes, index_bytes, reserved)?;
        for (operation, observed, limit) in [
            (
                "artifact_registrations_v4",
                m5.artifact_registrations_v4,
                self.artifact_registrations_v4,
            ),
            (
                "gluing_input_descriptors",
                m5.gluing_input_descriptors,
                self.gluing_input_descriptors,
            ),
            ("context_covers", m5.context_covers, self.context_covers),
            ("sections", m5.sections, self.sections),
            ("gluing_attempts", m5.gluing_attempts, self.gluing_attempts),
            ("restrictions", m5.restrictions, self.restrictions),
            (
                "global_candidates",
                m5.global_candidates,
                self.global_candidates,
            ),
            (
                "gluing_obstructions",
                m5.gluing_obstructions,
                self.gluing_obstructions,
            ),
        ] {
            if observed > limit {
                return Err(ReportError::Incomplete {
                    operation,
                    limit,
                    observed,
                });
            }
        }
        let rows = inherited
            .rows(self.inherited.rows)?
            .checked_add(m5.rows()?)
            .ok_or_else(|| incomplete("report_v4_rows", self.inherited.rows))?;
        if rows > self.inherited.rows {
            return Err(ReportError::Incomplete {
                operation: "report_v4_rows",
                limit: self.inherited.rows,
                observed: rows,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReportRequestV4 {
    pub report_id: StableId,
    pub repository_id: StableId,
    pub program_space_ref: StableId,
    pub plan_id: StableId,
    pub selected_obligation_ids: BTreeSet<StableId>,
    pub tool_versions: BTreeMap<String, String>,
}

pub fn generate_v4(
    root: &StoreRoot,
    identity: JournalIdentity,
    base_roots: AuthorityTrustRootsV4,
    assignments: M5DoubleSubmitAssignmentsV4,
    request: &ReportRequestV4,
) -> Result<crate::GeneratedReport, ReportError> {
    generate_v4_with_limits(
        root,
        identity,
        base_roots,
        assignments,
        request,
        ReportLimitsV4::default(),
    )
}

pub fn generate_v4_with_limits(
    root: &StoreRoot,
    identity: JournalIdentity,
    base_roots: AuthorityTrustRootsV4,
    assignments: M5DoubleSubmitAssignmentsV4,
    request: &ReportRequestV4,
    limits: ReportLimitsV4,
) -> Result<crate::GeneratedReport, ReportError> {
    if identity.version() != EventContractVersion::V4
        || request.selected_obligation_ids.is_empty()
        || request.tool_versions.is_empty()
        || request
            .tool_versions
            .iter()
            .any(|(k, v)| k.is_empty() || v.is_empty())
    {
        return Err(ReportError::Source(
            "v4 requires v4 input, selected obligations, and tool versions",
        ));
    }
    if request.repository_id != *base_roots.repository_id() {
        return Err(ReportError::Source("v4 authority repository closure"));
    }
    let journal = EventJournal::open(root, identity)?;
    let authority = match journal.inspect_m5_report_authority_v4(base_roots, assignments)? {
        M5ReportAuthorityInspectionV4::Complete(authority) => authority,
        M5ReportAuthorityInspectionV4::Incomplete { registered_inputs } => {
            return Err(M5BundleIncomplete { registered_inputs }.into());
        }
    };
    let index = DerivedIndexV5::open(root)?;
    // The index owns replay/CAS verification while observing the journal.
    let validated = authority.validated_snapshot_v5(&index, &journal)?;
    let snapshot = validated.snapshot();
    let reader = journal.reader()?;
    if snapshot.marker.confirmed_offset != reader.confirmed_offset()
        || snapshot.marker.tail_hash != *reader.tail_hash()
        || snapshot.marker.event_count != u64::try_from(reader.events().len()).unwrap_or(u64::MAX)
        || snapshot.marker.event_contract_version != EventContractVersion::V4.schema()
        || snapshot.marker.projection_contract_version != INDEX_V5
        || snapshot.marker.index_schema_version != 5
        || snapshot.marker.sqlite_user_version != 5
    {
        return Err(ReportError::Source(
            "v4 journal/index confirmed-tail mismatch",
        ));
    }
    // I5 is measured by streaming serialization of the complete snapshot,
    // never by materialising canonical_json(snapshot).
    let index_bytes = json_encoded_len(snapshot, "index_bytes", limits.inherited.working_bytes)?;
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

#[derive(Serialize)]
struct ArtifactRegistrationBodyV4<'a> {
    cas_hash: &'a reviewgraphen_core::ContentHash,
    id: &'a StableId,
    media_type: &'a str,
    run_id: &'a StableId,
    schema: &'a str,
    sensitivity: &'a str,
    size: u64,
    source: &'a serde_json::Value,
}

fn value_string<'a>(
    value: &'a serde_json::Value,
    field: &'static str,
) -> Result<&'a str, ReportError> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or(ReportError::Source("v4 M5 index item is incomplete"))
}

fn stable_value_id<'a>(
    value: &'a serde_json::Value,
    field: &'static str,
) -> Result<&'a str, ReportError> {
    let id = value_string(value, field)?;
    StableId::parse(id)?;
    Ok(id)
}

fn validate_complete_m5_snapshot(
    snapshot: &IndexSnapshotV5,
) -> Result<M5ReportCounts, ReportError> {
    let registered_inputs = u64_count(snapshot.artifact_registrations_v4.len())?;
    let bundle_empty = snapshot.context_covers.is_empty()
        && snapshot.sections.is_empty()
        && snapshot.restrictions.is_empty()
        && snapshot.gluing_attempts.is_empty()
        && snapshot.global_candidates.is_empty()
        && snapshot.gluing_obstructions.is_empty();
    if bundle_empty && registered_inputs <= 2 {
        return Err(M5BundleIncomplete { registered_inputs }.into());
    }
    if snapshot.artifact_registrations_v4.len() != 2
        || snapshot.gluing_input_descriptors.len() != 2
        || snapshot.context_covers.len() != 1
        || snapshot.gluing_attempts.len() != 1
        || snapshot.sections.len() > 2
        || snapshot.restrictions.len() > 2
        || snapshot.global_candidates.len() > 1
        || snapshot.gluing_obstructions.len() > 1
    {
        return Err(ReportError::Source("v4 M5 bundle cardinality mismatch"));
    }

    let mut repository_source_hash = None;
    for (position, (registration, descriptor)) in snapshot
        .artifact_registrations_v4
        .iter()
        .zip(&snapshot.gluing_input_descriptors)
        .enumerate()
    {
        let observed_repository_source_hash =
            value_string(&registration.source, "repository_source_hash")?;
        let parsed_repository_source_hash =
            reviewgraphen_core::ContentHash::parse(observed_repository_source_hash.to_owned())?;
        crate::validate_metadata_hash("repository_source_hash", &parsed_repository_source_hash)?;
        if let Some(expected) = repository_source_hash {
            if expected != observed_repository_source_hash {
                return Err(ReportError::Source(
                    "v4 M5 descriptor repository-source roots differ",
                ));
            }
        } else {
            repository_source_hash = Some(observed_repository_source_hash);
        }
        let expected_context = [
            reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
            reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID,
        ][position];
        if registration.event_actor != M5_REGISTRATION_ACTOR
            || registration.registration_id != descriptor.registration_id
            || registration.descriptor_id.as_str() != stable_value_id(&descriptor.descriptor, "id")?
            || value_string(&registration.source, "kind")? != "gluing_input"
            || value_string(&registration.source, "context_id")? != expected_context
            || value_string(&descriptor.descriptor, "context_id")? != expected_context
            || value_string(&registration.source, "descriptor_id")?
                != registration.descriptor_id.as_str()
            || registration.cas_hash != descriptor.descriptor_hash
            || registration.size != descriptor.descriptor_size
            || registration.body_hash
                != reviewgraphen_core::ContentHash::sha256(&reviewgraphen_core::canonical_json(
                    &ArtifactRegistrationBodyV4 {
                        schema: &registration.schema,
                        id: &registration.registration_id,
                        run_id: &registration.run_id,
                        cas_hash: &registration.cas_hash,
                        media_type: &registration.media_type,
                        size: registration.size,
                        sensitivity: &registration.sensitivity,
                        source: &registration.source,
                    },
                )?)
        {
            return Err(ReportError::Source(
                "v4 M5 descriptor/registration closure mismatch",
            ));
        }
        let descriptor_bytes = reviewgraphen_core::canonical_json(&descriptor.descriptor)?;
        if descriptor.body_hash != reviewgraphen_core::ContentHash::sha256(&descriptor_bytes)
            || descriptor.descriptor_hash
                != reviewgraphen_core::ContentHash::sha256(&descriptor_bytes)
            || descriptor.descriptor_size
                != u64::try_from(descriptor_bytes.len())
                    .map_err(|_| incomplete("m5_descriptor_bytes", u64::MAX))?
        {
            return Err(ReportError::Source("v4 M5 descriptor hash mismatch"));
        }
    }

    let bundle_sequence = snapshot.context_covers[0].event_sequence;
    let bundle_event_id = &snapshot.context_covers[0].event_id;
    let same_event = |sequence: u64, event_id: &StableId| {
        sequence == bundle_sequence && event_id == bundle_event_id
    };
    if snapshot
        .sections
        .iter()
        .any(|row| !same_event(row.event_sequence, &row.event_id))
        || snapshot
            .restrictions
            .iter()
            .any(|row| !same_event(row.event_sequence, &row.event_id))
        || snapshot
            .gluing_attempts
            .iter()
            .any(|row| !same_event(row.event_sequence, &row.event_id))
        || snapshot
            .global_candidates
            .iter()
            .any(|row| !same_event(row.event_sequence, &row.event_id))
        || snapshot
            .gluing_obstructions
            .iter()
            .any(|row| !same_event(row.event_sequence, &row.event_id))
    {
        return Err(ReportError::Source("v4 M5 bundle event tuple mismatch"));
    }

    if snapshot.sections.len() != snapshot.restrictions.len() {
        return Err(ReportError::Source(
            "v4 M5 section/restriction cardinality mismatch",
        ));
    }
    for (position, (section, restriction)) in snapshot
        .sections
        .iter()
        .zip(&snapshot.restrictions)
        .enumerate()
    {
        if snapshot.sections.len() == 2
            && value_string(&section.section, "context_id")?
                != [
                    reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
                    reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID,
                ][position]
        {
            return Err(ReportError::Source("v4 M5 section context order mismatch"));
        }
        if value_string(&restriction.restriction, "section_id")?
            != value_string(&section.section, "id")?
        {
            return Err(ReportError::Source(
                "v4 M5 restriction context order mismatch",
            ));
        }
    }

    validate_snapshot_attempt_result(snapshot)?;

    Ok(M5ReportCounts {
        artifact_registrations_v4: registered_inputs,
        gluing_input_descriptors: u64_count(snapshot.gluing_input_descriptors.len())?,
        context_covers: u64_count(snapshot.context_covers.len())?,
        sections: u64_count(snapshot.sections.len())?,
        gluing_attempts: u64_count(snapshot.gluing_attempts.len())?,
        restrictions: u64_count(snapshot.restrictions.len())?,
        global_candidates: u64_count(snapshot.global_candidates.len())?,
        gluing_obstructions: u64_count(snapshot.gluing_obstructions.len())?,
    })
}

fn validate_snapshot_attempt_result(snapshot: &IndexSnapshotV5) -> Result<(), ReportError> {
    let attempt = &snapshot.gluing_attempts[0].attempt;
    let attempt_id = value_string(attempt, "id")?;
    let result = value_string(attempt, "result")?;
    let candidate_ref = optional_value_string(attempt, "global_candidate_id")?;
    let obstruction_ref = optional_value_string(attempt, "obstruction_id")?;
    match result {
        "failed" | "unknown"
            if snapshot.global_candidates.is_empty() && snapshot.gluing_obstructions.len() == 1 =>
        {
            let obstruction = &snapshot.gluing_obstructions[0].obstruction;
            if candidate_ref.is_some()
                || obstruction_ref != Some(value_string(obstruction, "id")?)
                || value_string(obstruction, "attempt_id")? != attempt_id
            {
                return Err(ReportError::Source(
                    "v4 M5 obstructed-attempt linkage mismatch",
                ));
            }
        }
        "candidate" | "glued_with_qualification" | "glued"
            if snapshot.global_candidates.len() == 1 && snapshot.gluing_obstructions.is_empty() =>
        {
            let candidate = &snapshot.global_candidates[0].candidate;
            if obstruction_ref.is_some() || candidate_ref != Some(value_string(candidate, "id")?) {
                return Err(ReportError::Source(
                    "v4 M5 successful-attempt candidate linkage mismatch",
                ));
            }
        }
        "failed" | "candidate" | "glued_with_qualification" | "glued" | "unknown" => {
            return Err(ReportError::Source(
                "v4 M5 attempt result cardinality mismatch",
            ));
        }
        _ => return Err(ReportError::Source("v4 M5 attempt result is unknown")),
    }
    if snapshot
        .restrictions
        .iter()
        .any(|row| row.attempt_id.as_str() != attempt_id)
        || snapshot
            .global_candidates
            .iter()
            .any(|row| row.attempt_id.as_str() != attempt_id)
    {
        return Err(ReportError::Source("v4 M5 attempt index linkage mismatch"));
    }
    Ok(())
}

fn optional_value_string<'a>(
    value: &'a serde_json::Value,
    field: &'static str,
) -> Result<Option<&'a str>, ReportError> {
    match value.get(field) {
        Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value)),
        _ => Err(ReportError::Source("v4 M5 nullable ID is incomplete")),
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    journal: &EventJournal<'_>,
    validated: &ValidatedIndexSnapshotV5<'_, '_, '_>,
    request: &ReportRequestV4,
    limits: ReportLimitsV4,
    journal_bytes: u64,
    index_bytes: u64,
) -> Result<crate::GeneratedReport, ReportError> {
    let snapshot = validated.snapshot();
    // Refuse J + I5 before constructing any report-owned selection or row.
    limits.preflight(
        ReportCounts::default(),
        M5ReportCounts::default(),
        journal_bytes,
        index_bytes,
        0,
    )?;
    let universe = snapshot
        .universe
        .as_ref()
        .ok_or(ReportError::Source("v4 missing universe"))?;
    for (field, hash) in [
        ("rule_set_hash", &universe.rule_set_hash),
        ("extractor_set_hash", &universe.extractor_set_hash),
        ("genesis_hash", &snapshot.marker.genesis_hash),
        ("confirmed_tail_hash", &snapshot.marker.tail_hash),
        (
            "authority_policy_revision_hash",
            &snapshot.policy_revision_hash,
        ),
        (
            "authority_replay_basis_digest",
            &snapshot.authority_replay_basis_digest,
        ),
    ] {
        crate::validate_metadata_hash(field, hash)?;
    }
    if request
        .program_space_ref
        .as_str()
        .strip_prefix("program-space:")
        != Some(universe.snapshot_id.as_str())
    {
        return Err(ReportError::Source("v4 program-space closure"));
    }
    let selection = V5SelectionRequest {
        plan_id: &request.plan_id,
        selected_obligation_ids: &request.selected_obligation_ids,
        expected_confirmed_offset: snapshot.marker.confirmed_offset,
        expected_event_count: snapshot.marker.event_count,
        expected_tail_hash: &snapshot.marker.tail_hash,
    };
    let mut charge = ChargeVisitor::default();
    let first = validated
        .visit_selection(journal, selection, &mut charge)
        .map_err(|error| selection_charge_error(error, limits.inherited.working_bytes))?;
    let m5_counts = validate_complete_m5_snapshot(snapshot)?;
    let final_counts = report_counts(first, m5_counts);
    let selected_count = u64_count(request.selected_obligation_ids.len())?;
    let status = if first.counts.completed_ids == selected_count {
        Status::Completed
    } else {
        Status::Partial
    };
    let report_charge = assemble_report_charge(request, snapshot, universe, status, &charge)
        .map_err(|_| incomplete("report_v4_reservation", limits.inherited.working_bytes))?;
    let reserved = report_charge;
    // This is the only J + I5 + Rr4 construction gate. No report-owned ID
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
            V5SelectionRequest {
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
        return Err(ReportError::Source("v4 selection changed between passes"));
    }
    ids.canonicalize_and_verify(second)?;
    if (status == Status::Completed)
        != ids
            .completed
            .iter()
            .eq(request.selected_obligation_ids.iter())
        || charge.obstruction_kind_mask != ids.obstruction_kind_mask
    {
        return Err(ReportError::Source("v4 materialized selection mismatch"));
    }
    let shape = V4Report {
        schema: SCHEMA_V4,
        report_type: "review",
        report_version: 4,
        metadata: V4Metadata::new(request, snapshot, universe),
        scenario: V4Scenario::new(request, universe, &ids.registrations, snapshot),
        result: V4Result {
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
            gluing_attempts: GluingAttemptRows(&snapshot.gluing_attempts),
            gluing_input_descriptors: GluingInputDescriptorRows {
                descriptors: &snapshot.gluing_input_descriptors,
                registrations: &snapshot.artifact_registrations_v4,
            },
            gluing_obstructions: GluingObstructionRows(&snapshot.gluing_obstructions),
            global_candidates: GlobalCandidateRows(&snapshot.global_candidates),
            claim_assessments: AssessmentRows {
                rows: &snapshot.claim_assessments,
                ids: &ids.claim_assessments,
            },
            obstructions: ObstructionRows {
                rows: &snapshot.executions,
                ids: &ids.obstructions,
            },
            context_covers: ContextCoverRows(&snapshot.context_covers),
            restrictions: RestrictionRows(&snapshot.restrictions),
            sections: SectionRows(&snapshot.sections),
        },
        coverage: V4Coverage::new(
            universe,
            &ids.denominator,
            &request.selected_obligation_ids,
            &ids.visited,
            &ids.completed,
            &ids.evidence_supported,
            &ids.verified,
            &ids.accepted,
        ),
        projection: V4Projection::new(
            request,
            status,
            &ids.executions,
            &ids.claims,
            ids.obstruction_kind_mask,
            snapshot,
        ),
    };
    let realized = ownership_charge(&shape)
        .map_err(|_| incomplete("report_v4_reservation", limits.inherited.working_bytes))?;
    if realized != report_charge {
        return Err(ReportError::Source("v4 charge/materialization mismatch"));
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
    let output = json_encoded_len(
        &shape,
        "canonical_report_bytes",
        limits.inherited.canonical_bytes,
    )?;
    limits
        .inherited
        .check_serialization(journal_bytes, index_bytes, realized, largest, output)?;
    let canonical_bytes = bounded_json_bytes(&shape, output, limits.inherited)?;
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

impl V5SelectionVisitor for ChargeVisitor {
    type Error = OwnershipError;

    fn visit(&mut self, item: V5SelectionItem<'_>) -> Result<(), Self::Error> {
        match item {
            V5SelectionItem::ArtifactRegistration(row) => {
                self.registrations
                    .list_serialized(&Registration::from(row))?;
                self.registration_ids
                    .list_serialized(&row.registration_id)?;
            }
            V5SelectionItem::Execution(row) => {
                self.executions.list_serialized(&Execution(row))?;
                self.execution_ids.list_serialized(&row.execution_id)?;
            }
            V5SelectionItem::Claim(row) => {
                self.claims.list_serialized(&Claim(row))?;
                self.claim_ids.list_serialized(&row.claim_id)?;
            }
            V5SelectionItem::Evidence(row) => {
                self.evidence.list_serialized(&Evidence::from(row))?;
            }
            V5SelectionItem::EvidenceBinding(row) => {
                self.evidence_bindings
                    .list_serialized(&Binding::from(row))?;
            }
            V5SelectionItem::Verification(row) => {
                self.verifications
                    .list_serialized(&Verification::from(row))?;
            }
            V5SelectionItem::Decision(row) => {
                self.decisions.list_serialized(&Decision::from(row))?;
            }
            V5SelectionItem::Finding(row) => {
                self.findings.list_serialized(&Finding::from(row))?;
            }
            V5SelectionItem::ClaimAssessment(row) => {
                self.claim_assessments
                    .list_serialized(&Assessment::from(row))?;
            }
            V5SelectionItem::Obstruction(row) => {
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
            V5SelectionItem::CoverageId { axis, id } => match axis {
                V5CoverageAxis::Denominator => self.denominator.list_serialized(id)?,
                V5CoverageAxis::Visited => self.visited.list_serialized(id)?,
                V5CoverageAxis::Completed => self.completed.list_serialized(id)?,
                V5CoverageAxis::EvidenceSupported => {
                    self.evidence_supported.list_serialized(id)?;
                }
                V5CoverageAxis::Verified => self.verified.list_serialized(id)?,
                V5CoverageAxis::Accepted => self.accepted.list_serialized(id)?,
            },
        }
        Ok(())
    }
}

fn assemble_report_charge(
    request: &ReportRequestV4,
    snapshot: &IndexSnapshotV5,
    universe: &reviewgraphen_store::IndexUniverse,
    status: Status,
    selected: &ChargeVisitor,
) -> Result<u64, OwnershipError> {
    let mut metadata = LogicalCharge::new();
    metadata.serialized(&V4Metadata::new(request, snapshot, universe))?;

    let mut scenario = LogicalCharge::new();
    scenario.field_charge("artifact_registration_ids", selected.registration_ids)?;
    scenario.field_serialized(
        "context_cover_ids",
        &ContextCoverIds(&snapshot.context_covers),
    )?;
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
    result.field_serialized(
        "gluing_attempts",
        &GluingAttemptRows(&snapshot.gluing_attempts),
    )?;
    result.field_serialized(
        "gluing_input_descriptors",
        &GluingInputDescriptorRows {
            descriptors: &snapshot.gluing_input_descriptors,
            registrations: &snapshot.artifact_registrations_v4,
        },
    )?;
    result.field_serialized(
        "gluing_obstructions",
        &GluingObstructionRows(&snapshot.gluing_obstructions),
    )?;
    result.field_serialized(
        "global_candidates",
        &GlobalCandidateRows(&snapshot.global_candidates),
    )?;
    result.field_charge("obstructions", selected.obstructions)?;
    result.field_serialized(
        "context_covers",
        &ContextCoverRows(&snapshot.context_covers),
    )?;
    result.field_serialized("restrictions", &RestrictionRows(&snapshot.restrictions))?;
    result.field_serialized("sections", &SectionRows(&snapshot.sections))?;
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
    for kind in M5_ARRAY_KINDS {
        if kind.count(snapshot) != 0 {
            losses.list_serialized(&M5OmissionLoss { kind, snapshot })?;
        }
    }
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
    report.field_string("schema", SCHEMA_V4)?;
    Ok(report.bytes())
}

fn preflight_report_materialization<T>(
    limits: ReportLimitsV4,
    counts: ReportCountsV4,
    journal_bytes: u64,
    index_bytes: u64,
    reserved: u64,
    materialize: impl FnOnce() -> Result<T, ReportError>,
) -> Result<T, ReportError> {
    limits.preflight(
        counts.inherited,
        counts.m5,
        journal_bytes,
        index_bytes,
        reserved,
    )?;
    materialize()
}

fn report_counts(summary: V5SelectionSummary, m5: M5ReportCounts) -> ReportCountsV4 {
    ReportCountsV4 {
        inherited: ReportCounts {
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
            information_loss_records: 1 + m5.omission_losses(),
        },
        m5,
    }
}

fn selection_charge_error(error: V5SelectionVisitError<OwnershipError>, limit: u64) -> ReportError {
    match error {
        V5SelectionVisitError::Index(error) => error.into(),
        V5SelectionVisitError::Visitor(_) => incomplete("report_v4_charge", limit),
    }
}

fn selection_materialize_error(
    error: V5SelectionVisitError<std::convert::Infallible>,
) -> ReportError {
    match error {
        V5SelectionVisitError::Index(error) => error.into(),
        V5SelectionVisitError::Visitor(never) => match never {},
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
    fn with_summary(summary: V5SelectionSummary) -> Result<Self, ReportError> {
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

    fn canonicalize_and_verify(&mut self, summary: V5SelectionSummary) -> Result<(), ReportError> {
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

impl V5SelectionVisitor for SelectionIds {
    type Error = std::convert::Infallible;

    fn visit(&mut self, item: V5SelectionItem<'_>) -> Result<(), Self::Error> {
        match item {
            V5SelectionItem::ArtifactRegistration(row) => {
                self.registrations.push(row.registration_id.clone());
            }
            V5SelectionItem::Execution(row) => self.executions.push(row.execution_id.clone()),
            V5SelectionItem::Claim(row) => self.claims.push(row.claim_id.clone()),
            V5SelectionItem::Evidence(row) => self.evidence.push(row.evidence_id.clone()),
            V5SelectionItem::EvidenceBinding(row) => {
                self.evidence_bindings.push(row.binding_id.clone());
            }
            V5SelectionItem::Verification(row) => {
                self.verifications.push(row.verification_id.clone());
            }
            V5SelectionItem::Decision(row) => self.decisions.push(row.decision_id.clone()),
            V5SelectionItem::Finding(row) => self.findings.push(row.finding_id.clone()),
            V5SelectionItem::ClaimAssessment(row) => {
                self.claim_assessments.push(row.claim_id.clone());
            }
            V5SelectionItem::Obstruction(row) => {
                self.obstructions.push(row.execution_id.clone());
                if let Some((_, bit)) = obstruction_kind_and_bit(&row.outcome_kind) {
                    self.obstruction_kind_mask |= bit;
                }
            }
            V5SelectionItem::CoverageId { axis, id } => match axis {
                V5CoverageAxis::Denominator => self.denominator.push(id.clone()),
                V5CoverageAxis::Visited => self.visited.push(id.clone()),
                V5CoverageAxis::Completed => self.completed.push(id.clone()),
                V5CoverageAxis::EvidenceSupported => self.evidence_supported.push(id.clone()),
                V5CoverageAxis::Verified => self.verified.push(id.clone()),
                V5CoverageAxis::Accepted => self.accepted.push(id.clone()),
            },
        }
        Ok(())
    }
}

fn reserved_ids(count: u64) -> Result<Vec<StableId>, ReportError> {
    let capacity =
        usize::try_from(count).map_err(|_| incomplete("report_v4_materialization", u64::MAX))?;
    let mut ids = Vec::new();
    ids.try_reserve_exact(capacity)
        .map_err(|_| incomplete("report_v4_materialization", count))?;
    Ok(ids)
}

fn canonical_ids(ids: &mut Vec<StableId>, expected: u64) -> Result<(), ReportError> {
    ids.sort_unstable();
    ids.dedup();
    if u64_count(ids.len())? != expected {
        return Err(ReportError::Source("v4 selection cardinality mismatch"));
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
    u64::try_from(n).map_err(|_| incomplete("report_v4_count", u64::MAX))
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Completed,
    Partial,
}
#[derive(Serialize)]
struct V4Report<'a> {
    coverage: V4Coverage<'a>,
    metadata: V4Metadata<'a>,
    projection: V4Projection<'a>,
    report_type: &'static str,
    report_version: u8,
    result: V4Result<'a>,
    scenario: V4Scenario<'a>,
    schema: &'static str,
}
#[derive(Serialize)]
struct V4Metadata<'a> {
    authority_policy_revision_hash: &'a reviewgraphen_core::ContentHash,
    authority_replay_basis_digest: &'a reviewgraphen_core::ContentHash,
    confirmed_event_count: u64,
    confirmed_offset: u64,
    confirmed_tail_hash: &'a reviewgraphen_core::ContentHash,
    event_contract_version: &'static str,
    extractor_set_hash: &'a reviewgraphen_core::ContentHash,
    genesis_hash: &'a reviewgraphen_core::ContentHash,
    gluing_profile_descriptor_id: &'static str,
    index_projection_version: &'static str,
    policy_version: &'a str,
    profile_id: &'a str,
    report_id: &'a StableId,
    rule_set_hash: &'a reviewgraphen_core::ContentHash,
    run_id: &'a StableId,
    tool_versions: &'a BTreeMap<String, String>,
}
impl<'a> V4Metadata<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        request: &'a ReportRequestV4,
        snapshot: &'a IndexSnapshotV5,
        universe: &'a reviewgraphen_store::IndexUniverse,
    ) -> Self {
        Self {
            report_id: &request.report_id,
            run_id: &snapshot.marker.run_id,
            profile_id: &universe.profile_id,
            rule_set_hash: &universe.rule_set_hash,
            extractor_set_hash: &universe.extractor_set_hash,
            policy_version: &universe.policy_version,
            event_contract_version: EventContractVersion::V4.schema(),
            index_projection_version: INDEX_V5,
            genesis_hash: &snapshot.marker.genesis_hash,
            gluing_profile_descriptor_id: reviewgraphen_core::DOUBLE_SUBMIT_GLUING_DESCRIPTOR_ID,
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
struct V4Scenario<'a> {
    artifact_registration_ids: &'a [StableId],
    context_cover_ids: ContextCoverIds<'a>,
    plan_id: &'a StableId,
    program_space_ref: &'a StableId,
    repository_id: &'a StableId,
    selected_obligation_ids: &'a BTreeSet<StableId>,
    snapshot_id: &'a StableId,
    universe_id: &'a StableId,
}
impl<'a> V4Scenario<'a> {
    fn new(
        request: &'a ReportRequestV4,
        universe: &'a reviewgraphen_store::IndexUniverse,
        registration_ids: &'a [StableId],
        snapshot: &'a IndexSnapshotV5,
    ) -> Self {
        Self {
            repository_id: &request.repository_id,
            snapshot_id: &universe.snapshot_id,
            program_space_ref: &request.program_space_ref,
            universe_id: &universe.universe_id,
            plan_id: &request.plan_id,
            selected_obligation_ids: &request.selected_obligation_ids,
            artifact_registration_ids: registration_ids,
            context_cover_ids: ContextCoverIds(&snapshot.context_covers),
        }
    }
}

struct ContextCoverIds<'a>(&'a [reviewgraphen_store::ContextCoverV4IndexItem]);

impl Serialize for ContextCoverIds<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for row in self.0 {
            sequence.serialize_element(
                row.cover
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| serde::ser::Error::custom("context cover lacks id"))?,
            )?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
struct ArtifactRegistrationV4ReportItem<'a> {
    body_hash: &'a reviewgraphen_core::ContentHash,
    event_actor: &'a str,
    event_id: &'a StableId,
    event_sequence: u64,
    registration: ArtifactRegistrationBodyV4<'a>,
    schema: &'static str,
}

impl<'a> From<&'a reviewgraphen_store::ArtifactRegistrationV4IndexItem>
    for ArtifactRegistrationV4ReportItem<'a>
{
    fn from(row: &'a reviewgraphen_store::ArtifactRegistrationV4IndexItem) -> Self {
        Self {
            schema: M5_REGISTRATION_ITEM_SCHEMA,
            event_sequence: row.event_sequence,
            event_id: &row.event_id,
            event_actor: &row.event_actor,
            registration: ArtifactRegistrationBodyV4 {
                schema: &row.schema,
                id: &row.registration_id,
                run_id: &row.run_id,
                cas_hash: &row.cas_hash,
                media_type: &row.media_type,
                size: row.size,
                sensitivity: &row.sensitivity,
                source: &row.source,
            },
            body_hash: &row.body_hash,
        }
    }
}

#[derive(Serialize)]
struct GluingInputDescriptorV4ReportItem<'a> {
    descriptor: &'a serde_json::Value,
    descriptor_body_hash: &'a reviewgraphen_core::ContentHash,
    registration: ArtifactRegistrationV4ReportItem<'a>,
    registration_id: &'a StableId,
    schema: &'static str,
}

struct GluingInputDescriptorRows<'a> {
    descriptors: &'a [reviewgraphen_store::GluingInputDescriptorV4IndexItem],
    registrations: &'a [reviewgraphen_store::ArtifactRegistrationV4IndexItem],
}

impl Serialize for GluingInputDescriptorRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.descriptors.len() != self.registrations.len() {
            return Err(serde::ser::Error::custom(
                "descriptor/registration cardinality mismatch",
            ));
        }
        let mut sequence = serializer.serialize_seq(Some(self.descriptors.len()))?;
        for (descriptor, registration) in self.descriptors.iter().zip(self.registrations) {
            sequence.serialize_element(&GluingInputDescriptorV4ReportItem {
                schema: M5_DESCRIPTOR_ITEM_SCHEMA,
                descriptor: &descriptor.descriptor,
                descriptor_body_hash: &descriptor.body_hash,
                registration_id: &descriptor.registration_id,
                registration: ArtifactRegistrationV4ReportItem::from(registration),
            })?;
        }
        sequence.end()
    }
}

macro_rules! m5_report_rows {
    ($rows:ident, $item:ident, $store:ty, $field:ident, first) => {
        struct $rows<'a>(&'a [$store]);

        #[derive(Serialize)]
        struct $item<'a> {
            $field: &'a serde_json::Value,
            body_hash: &'a reviewgraphen_core::ContentHash,
            event_id: &'a StableId,
            event_sequence: u64,
        }

        impl Serialize for $rows<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
                for row in self.0 {
                    sequence.serialize_element(&$item {
                        event_sequence: row.event_sequence,
                        event_id: &row.event_id,
                        $field: &row.$field,
                        body_hash: &row.body_hash,
                    })?;
                }
                sequence.end()
            }
        }
    };
    ($rows:ident, $item:ident, $store:ty, $field:ident, after_body) => {
        struct $rows<'a>(&'a [$store]);

        #[derive(Serialize)]
        struct $item<'a> {
            body_hash: &'a reviewgraphen_core::ContentHash,
            $field: &'a serde_json::Value,
            event_id: &'a StableId,
            event_sequence: u64,
        }

        impl Serialize for $rows<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
                for row in self.0 {
                    sequence.serialize_element(&$item {
                        event_sequence: row.event_sequence,
                        event_id: &row.event_id,
                        $field: &row.$field,
                        body_hash: &row.body_hash,
                    })?;
                }
                sequence.end()
            }
        }
    };
    ($rows:ident, $item:ident, $store:ty, $field:ident, last) => {
        struct $rows<'a>(&'a [$store]);

        #[derive(Serialize)]
        struct $item<'a> {
            body_hash: &'a reviewgraphen_core::ContentHash,
            event_id: &'a StableId,
            event_sequence: u64,
            $field: &'a serde_json::Value,
        }

        impl Serialize for $rows<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
                for row in self.0 {
                    sequence.serialize_element(&$item {
                        event_sequence: row.event_sequence,
                        event_id: &row.event_id,
                        $field: &row.$field,
                        body_hash: &row.body_hash,
                    })?;
                }
                sequence.end()
            }
        }
    };
}

m5_report_rows!(
    ContextCoverRows,
    ContextCoverReportItem,
    reviewgraphen_store::ContextCoverV4IndexItem,
    cover,
    after_body
);
m5_report_rows!(
    SectionRows,
    SectionReportItem,
    reviewgraphen_store::SectionV4IndexItem,
    section,
    last
);
m5_report_rows!(
    RestrictionRows,
    RestrictionReportItem,
    reviewgraphen_store::RestrictionV4IndexItem,
    restriction,
    last
);
m5_report_rows!(
    GluingAttemptRows,
    GluingAttemptReportItem,
    reviewgraphen_store::GluingAttemptV4IndexItem,
    attempt,
    first
);
m5_report_rows!(
    GlobalCandidateRows,
    GlobalCandidateReportItem,
    reviewgraphen_store::GlobalCandidateV4IndexItem,
    candidate,
    after_body
);
m5_report_rows!(
    GluingObstructionRows,
    GluingObstructionReportItem,
    reviewgraphen_store::GluingObstructionV4IndexItem,
    obstruction,
    last
);

#[derive(Serialize)]
struct V4Result<'a> {
    artifact_registrations: RegistrationRows<'a>,
    claim_assessments: AssessmentRows<'a>,
    claims: ClaimRows<'a>,
    context_covers: ContextCoverRows<'a>,
    decisions: DecisionRows<'a>,
    evidence: EvidenceRows<'a>,
    evidence_bindings: BindingRows<'a>,
    executions: ExecutionRows<'a>,
    findings: FindingRows<'a>,
    global_candidates: GlobalCandidateRows<'a>,
    gluing_attempts: GluingAttemptRows<'a>,
    gluing_input_descriptors: GluingInputDescriptorRows<'a>,
    gluing_obstructions: GluingObstructionRows<'a>,
    obstructions: ObstructionRows<'a>,
    restrictions: RestrictionRows<'a>,
    sections: SectionRows<'a>,
    status: Status,
    verifications: VerificationRows<'a>,
}
#[derive(Serialize)]
struct V4Coverage<'a> {
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
impl<'a> V4Coverage<'a> {
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
struct V4Projection<'a> {
    views: One<V4View<'a>>,
}
#[derive(Serialize)]
struct V4View<'a> {
    information_loss: V4Losses<'a>,
    kind: &'static str,
    payload: V4Payload<'a>,
    source_ids: &'a BTreeSet<StableId>,
}
#[derive(Serialize)]
struct V4Loss<'a> {
    affected_properties: [&'static str; 1],
    kind: &'static str,
    meaningful: bool,
    reason: &'static str,
    recoverable: bool,
    recovery_ref: &'a StableId,
    source_ids: &'a BTreeSet<StableId>,
}

#[derive(Clone, Copy)]
enum M5ArrayKind {
    GluingInputDescriptors,
    ContextCovers,
    Sections,
    GluingAttempts,
    Restrictions,
    GlobalCandidates,
    GluingObstructions,
}

const M5_ARRAY_KINDS: [M5ArrayKind; 7] = [
    M5ArrayKind::GluingInputDescriptors,
    M5ArrayKind::ContextCovers,
    M5ArrayKind::Sections,
    M5ArrayKind::GluingAttempts,
    M5ArrayKind::Restrictions,
    M5ArrayKind::GlobalCandidates,
    M5ArrayKind::GluingObstructions,
];

impl M5ArrayKind {
    const fn array_name(self) -> &'static str {
        match self {
            Self::GluingInputDescriptors => "gluing_input_descriptors",
            Self::ContextCovers => "context_covers",
            Self::Sections => "sections",
            Self::GluingAttempts => "gluing_attempts",
            Self::Restrictions => "restrictions",
            Self::GlobalCandidates => "global_candidates",
            Self::GluingObstructions => "gluing_obstructions",
        }
    }

    const fn reason(self) -> &'static str {
        match self {
            Self::GluingInputDescriptors => {
                "view omits complete records from gluing_input_descriptors"
            }
            Self::ContextCovers => "view omits complete records from context_covers",
            Self::Sections => "view omits complete records from sections",
            Self::GluingAttempts => "view omits complete records from gluing_attempts",
            Self::Restrictions => "view omits complete records from restrictions",
            Self::GlobalCandidates => "view omits complete records from global_candidates",
            Self::GluingObstructions => "view omits complete records from gluing_obstructions",
        }
    }

    const fn recovery_ref(self) -> &'static str {
        match self {
            Self::GluingInputDescriptors => {
                "reviewgraphen.review.report.v4#/result/gluing_input_descriptors"
            }
            Self::ContextCovers => "reviewgraphen.review.report.v4#/result/context_covers",
            Self::Sections => "reviewgraphen.review.report.v4#/result/sections",
            Self::GluingAttempts => "reviewgraphen.review.report.v4#/result/gluing_attempts",
            Self::Restrictions => "reviewgraphen.review.report.v4#/result/restrictions",
            Self::GlobalCandidates => "reviewgraphen.review.report.v4#/result/global_candidates",
            Self::GluingObstructions => {
                "reviewgraphen.review.report.v4#/result/gluing_obstructions"
            }
        }
    }

    fn count(self, snapshot: &IndexSnapshotV5) -> usize {
        match self {
            Self::GluingInputDescriptors => snapshot.gluing_input_descriptors.len(),
            Self::ContextCovers => snapshot.context_covers.len(),
            Self::Sections => snapshot.sections.len(),
            Self::GluingAttempts => snapshot.gluing_attempts.len(),
            Self::Restrictions => snapshot.restrictions.len(),
            Self::GlobalCandidates => snapshot.global_candidates.len(),
            Self::GluingObstructions => snapshot.gluing_obstructions.len(),
        }
    }
}

fn validate_v4_semantics_inner(report: &serde_json::Value) -> Result<(), &'static str> {
    if report.get("schema").and_then(serde_json::Value::as_str) != Some(SCHEMA_V4) {
        return Err("schema discriminator mismatch");
    }
    let result = report
        .get("result")
        .and_then(serde_json::Value::as_object)
        .ok_or("result is missing")?;
    let metadata = report.get("metadata").ok_or("metadata is missing")?;
    let scenario = report.get("scenario").ok_or("scenario is missing")?;
    let descriptors = semantic_array(result, "gluing_input_descriptors")?;
    let covers = semantic_array(result, "context_covers")?;
    let sections = semantic_array(result, "sections")?;
    let attempts = semantic_array(result, "gluing_attempts")?;
    let restrictions = semantic_array(result, "restrictions")?;
    let candidates = semantic_array(result, "global_candidates")?;
    let obstructions = semantic_array(result, "gluing_obstructions")?;
    if descriptors.len() != 2 || covers.len() != 1 || attempts.len() != 1 {
        return Err("M5 report cardinality mismatch");
    }

    let expected_contexts = [
        reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
        reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID,
    ];
    for (position, item) in descriptors.iter().enumerate() {
        let descriptor = semantic_nested(item, "descriptor")?;
        if semantic_string(descriptor, "context_id")? != expected_contexts[position] {
            return Err("descriptor context order mismatch");
        }
        validate_descriptor_report_item(item, metadata, scenario)?;
    }
    if descriptor_repository_source_hash(&descriptors[0])?
        != descriptor_repository_source_hash(&descriptors[1])?
    {
        return Err("descriptor repository-source roots differ");
    }
    for (position, item) in sections.iter().enumerate() {
        let section = semantic_nested(item, "section")?;
        if sections.len() == 2
            && semantic_string(section, "context_id")? != expected_contexts[position]
        {
            return Err("section context order mismatch");
        }
        validate_nested_body_hash(item, "section", "body_hash")?;
    }
    for item in covers {
        validate_nested_body_hash(item, "cover", "body_hash")?;
    }
    for item in attempts {
        validate_nested_body_hash(item, "attempt", "body_hash")?;
    }
    for item in restrictions {
        validate_nested_body_hash(item, "restriction", "body_hash")?;
    }
    for item in candidates {
        validate_nested_body_hash(item, "candidate", "body_hash")?;
    }
    for item in obstructions {
        validate_nested_body_hash(item, "obstruction", "body_hash")?;
    }

    let bundle_sequence = semantic_u64(&covers[0], "event_sequence")?;
    let bundle_event_id = semantic_string(&covers[0], "event_id")?;
    for rows in [sections, attempts, restrictions, candidates, obstructions] {
        for row in rows {
            if semantic_u64(row, "event_sequence")? != bundle_sequence
                || semantic_string(row, "event_id")? != bundle_event_id
            {
                return Err("bundle event tuple mismatch");
            }
        }
    }

    let cover = semantic_nested(&covers[0], "cover")?;
    let cover_id = semantic_string(cover, "id")?;
    let attempt = semantic_nested(&attempts[0], "attempt")?;
    let attempt_id = semantic_string(attempt, "id")?;
    if semantic_string(attempt, "cover_id")? != cover_id {
        return Err("attempt cover linkage mismatch");
    }
    let descriptor_ids = descriptors
        .iter()
        .map(|item| semantic_nested(item, "descriptor").and_then(|row| semantic_string(row, "id")))
        .collect::<Result<Vec<_>, _>>()?;
    let section_ids = sections
        .iter()
        .map(|item| semantic_nested(item, "section").and_then(|row| semantic_string(row, "id")))
        .collect::<Result<Vec<_>, _>>()?;
    let restriction_ids = restrictions
        .iter()
        .map(|item| semantic_nested(item, "restriction").and_then(|row| semantic_string(row, "id")))
        .collect::<Result<Vec<_>, _>>()?;
    require_same_string_set(attempt, "input_descriptor_ids", &descriptor_ids)?;
    require_same_string_set(attempt, "section_ids", &section_ids)?;
    require_same_string_set(attempt, "restriction_ids", &restriction_ids)?;

    for (position, item) in sections.iter().enumerate() {
        let section = semantic_nested(item, "section")?;
        if semantic_string(section, "cover_id")? != cover_id {
            return Err("section cover linkage mismatch");
        }
        let context = semantic_string(section, "context_id")?;
        let descriptor_position = expected_contexts
            .iter()
            .position(|expected| *expected == context)
            .ok_or("section context is outside the profile")?;
        if semantic_string(section, "input_descriptor_id")? != descriptor_ids[descriptor_position] {
            return Err("section descriptor linkage mismatch");
        }
        if let Some(restriction_item) = restrictions.get(position) {
            let restriction = semantic_nested(restriction_item, "restriction")?;
            if semantic_string(restriction, "section_id")? != section_ids[position] {
                return Err("restriction context order or section linkage mismatch");
            }
        }
    }
    if restrictions.len() != sections.len() {
        return Err("section/restriction cardinality mismatch");
    }

    validate_attempt_outcome(
        attempt,
        attempt_id,
        cover_id,
        &section_ids,
        &restriction_ids,
        candidates,
        obstructions,
    )?;
    validate_m5_loss_correspondence(report, result)?;
    Ok(())
}

fn validate_attempt_outcome(
    attempt: &serde_json::Value,
    attempt_id: &str,
    cover_id: &str,
    section_ids: &[&str],
    restriction_ids: &[&str],
    candidates: &[serde_json::Value],
    obstructions: &[serde_json::Value],
) -> Result<(), &'static str> {
    let result = semantic_string(attempt, "result")?;
    let candidate_ref = semantic_optional_string(attempt, "global_candidate_id")?;
    let obstruction_ref = semantic_optional_string(attempt, "obstruction_id")?;
    match result {
        "failed" | "unknown" if candidates.is_empty() && obstructions.len() == 1 => {
            let obstruction = semantic_nested(&obstructions[0], "obstruction")?;
            if candidate_ref.is_some()
                || obstruction_ref != Some(semantic_string(obstruction, "id")?)
                || semantic_string(obstruction, "attempt_id")? != attempt_id
            {
                return Err("obstructed attempt linkage mismatch");
            }
            require_same_string_set(obstruction, "section_ids", section_ids)?;
        }
        "candidate" | "glued_with_qualification" | "glued"
            if candidates.len() == 1 && obstructions.is_empty() =>
        {
            let candidate = semantic_nested(&candidates[0], "candidate")?;
            if obstruction_ref.is_some()
                || candidate_ref != Some(semantic_string(candidate, "id")?)
                || semantic_string(candidate, "cover_id")? != cover_id
            {
                return Err("successful attempt candidate linkage mismatch");
            }
            require_same_string_set(candidate, "required_section_ids", section_ids)?;
            require_same_string_set(candidate, "restriction_ids", restriction_ids)?;
        }
        "failed" | "candidate" | "glued_with_qualification" | "glued" | "unknown" => {
            return Err("attempt result cardinality mismatch");
        }
        _ => return Err("attempt result is outside the closed contract"),
    }
    Ok(())
}

fn validate_descriptor_report_item(
    item: &serde_json::Value,
    metadata: &serde_json::Value,
    scenario: &serde_json::Value,
) -> Result<(), &'static str> {
    let descriptor = semantic_nested(item, "descriptor")?;
    let registration_item = semantic_nested(item, "registration")?;
    let registration = semantic_nested(registration_item, "registration")?;
    validate_nested_body_hash(registration_item, "registration", "body_hash")?;
    let descriptor_bytes = reviewgraphen_core::canonical_json(descriptor)
        .map_err(|_| "descriptor canonical encoding failed")?;
    let descriptor_hash = reviewgraphen_core::ContentHash::sha256(&descriptor_bytes).to_string();
    let descriptor_size =
        u64::try_from(descriptor_bytes.len()).map_err(|_| "descriptor size overflow")?;
    let source = registration
        .get("source")
        .ok_or("descriptor registration source is missing")?;
    if semantic_string(item, "descriptor_body_hash")? != descriptor_hash
        || semantic_string(registration, "cas_hash")? != descriptor_hash
        || semantic_u64(registration, "size")? != descriptor_size
        || semantic_string(item, "registration_id")? != semantic_string(registration, "id")?
        || semantic_string(source, "kind")? != "gluing_input"
        || semantic_string(source, "descriptor_id")? != semantic_string(descriptor, "id")?
        || semantic_string(source, "context_id")? != semantic_string(descriptor, "context_id")?
        || semantic_string(source, "descriptor_hash")? != descriptor_hash
        || semantic_u64(source, "descriptor_size")? != descriptor_size
        || semantic_string(source, "descriptor_media_type")?
            != semantic_string(registration, "media_type")?
        || semantic_string(source, "descriptor_sensitivity")?
            != semantic_string(registration, "sensitivity")?
        || semantic_string(registration, "run_id")? != semantic_string(descriptor, "run_id")?
        || semantic_string(source, "run_id")? != semantic_string(descriptor, "run_id")?
        || semantic_string(descriptor, "run_id")? != semantic_string(metadata, "run_id")?
        || semantic_string(source, "snapshot_id")? != semantic_string(descriptor, "snapshot_id")?
        || semantic_string(descriptor, "snapshot_id")? != semantic_string(scenario, "snapshot_id")?
        || semantic_string(source, "universe_id")? != semantic_string(descriptor, "universe_id")?
        || semantic_string(descriptor, "universe_id")? != semantic_string(scenario, "universe_id")?
        || semantic_string(source, "plan_id")? != semantic_string(descriptor, "plan_id")?
        || semantic_string(descriptor, "plan_id")? != semantic_string(scenario, "plan_id")?
        || semantic_string(source, "profile_descriptor_id")?
            != semantic_string(descriptor, "profile_descriptor_id")?
        || semantic_string(descriptor, "profile_descriptor_id")?
            != semantic_string(metadata, "gluing_profile_descriptor_id")?
        || semantic_string(source, "genesis_hash")? != semantic_string(metadata, "genesis_hash")?
        || semantic_string(source, "policy_revision_hash")?
            != semantic_string(metadata, "authority_policy_revision_hash")?
        || semantic_string(source, "repository_id")? != semantic_string(scenario, "repository_id")?
    {
        return Err("descriptor registration/hash closure mismatch");
    }
    Ok(())
}

fn descriptor_repository_source_hash(item: &serde_json::Value) -> Result<&str, &'static str> {
    let registration_item = semantic_nested(item, "registration")?;
    let registration = semantic_nested(registration_item, "registration")?;
    let source = registration
        .get("source")
        .ok_or("descriptor registration source is missing")?;
    semantic_full_sha256(source, "repository_source_hash")
}

fn validate_nested_body_hash(
    item: &serde_json::Value,
    nested: &'static str,
    hash_field: &'static str,
) -> Result<(), &'static str> {
    let nested = item.get(nested).ok_or("nested report record is missing")?;
    let bytes = reviewgraphen_core::canonical_json(nested)
        .map_err(|_| "nested report record canonical encoding failed")?;
    if semantic_string(item, hash_field)?
        != reviewgraphen_core::ContentHash::sha256(&bytes).to_string()
    {
        return Err("nested report record body hash mismatch");
    }
    Ok(())
}

fn validate_m5_loss_correspondence(
    report: &serde_json::Value,
    result: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), &'static str> {
    let views = report
        .pointer("/projection/views")
        .and_then(serde_json::Value::as_array)
        .ok_or("projection views array is missing")?;
    for view in views {
        let losses = view
            .get("information_loss")
            .and_then(serde_json::Value::as_array)
            .ok_or("projection loss array is missing")?;
        for kind in M5_ARRAY_KINDS {
            let rows = semantic_array(result, kind.array_name())?;
            let matching = losses
                .iter()
                .filter(|loss| {
                    loss.get("recovery_ref").and_then(serde_json::Value::as_str)
                        == Some(kind.recovery_ref())
                })
                .collect::<Vec<_>>();
            if rows.is_empty() {
                if !matching.is_empty() {
                    return Err("empty M5 array has an omission loss");
                }
                continue;
            }
            if matching.len() != 1 {
                return Err("nonempty M5 array lacks exactly one omission loss");
            }
            let loss = matching[0];
            if semantic_string(loss, "kind")? != "omitted_m5_gluing_records"
                || semantic_string(loss, "reason")? != kind.reason()
                || loss.get("affected_properties")
                    != Some(&serde_json::json!([
                        reviewgraphen_core::DOUBLE_SUBMIT_PROPERTY_ID
                    ]))
                || loss.get("meaningful").and_then(serde_json::Value::as_bool) != Some(true)
                || loss.get("recoverable").and_then(serde_json::Value::as_bool) != Some(true)
            {
                return Err("M5 omission loss semantics mismatch");
            }
            let expected = m5_result_ids(kind, rows)?;
            if semantic_string_array(loss, "source_ids")? != expected {
                return Err("M5 omission loss source IDs mismatch");
            }
        }
    }
    Ok(())
}

fn m5_result_ids(kind: M5ArrayKind, rows: &[serde_json::Value]) -> Result<Vec<&str>, &'static str> {
    let nested = match kind {
        M5ArrayKind::GluingInputDescriptors => "descriptor",
        M5ArrayKind::ContextCovers => "cover",
        M5ArrayKind::Sections => "section",
        M5ArrayKind::GluingAttempts => "attempt",
        M5ArrayKind::Restrictions => "restriction",
        M5ArrayKind::GlobalCandidates => "candidate",
        M5ArrayKind::GluingObstructions => "obstruction",
    };
    let mut ids = Vec::with_capacity(if matches!(kind, M5ArrayKind::GluingInputDescriptors) {
        rows.len() * 2
    } else {
        rows.len()
    });
    for row in rows {
        ids.push(semantic_string(semantic_nested(row, nested)?, "id")?);
        if matches!(kind, M5ArrayKind::GluingInputDescriptors) {
            ids.push(semantic_string(row, "registration_id")?);
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

fn semantic_array<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &'static str,
) -> Result<&'a [serde_json::Value], &'static str> {
    object
        .get(field)
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .ok_or("required report array is missing")
}

fn semantic_nested<'a>(
    value: &'a serde_json::Value,
    field: &'static str,
) -> Result<&'a serde_json::Value, &'static str> {
    value.get(field).ok_or("nested report record is missing")
}

fn semantic_string<'a>(
    value: &'a serde_json::Value,
    field: &'static str,
) -> Result<&'a str, &'static str> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or("required report string is missing")
}

fn semantic_full_sha256<'a>(
    value: &'a serde_json::Value,
    field: &'static str,
) -> Result<&'a str, &'static str> {
    let hash = semantic_string(value, field)?;
    let Some(hex) = hash.strip_prefix("sha256:") else {
        return Err("required full SHA-256 is missing");
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err("required full SHA-256 is invalid");
    }
    Ok(hash)
}

fn semantic_optional_string<'a>(
    value: &'a serde_json::Value,
    field: &'static str,
) -> Result<Option<&'a str>, &'static str> {
    match value.get(field) {
        Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value)),
        _ => Err("required nullable report string is missing"),
    }
}

fn semantic_u64(value: &serde_json::Value, field: &'static str) -> Result<u64, &'static str> {
    value
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or("required report integer is missing")
}

fn semantic_string_array<'a>(
    value: &'a serde_json::Value,
    field: &'static str,
) -> Result<Vec<&'a str>, &'static str> {
    value
        .get(field)
        .and_then(serde_json::Value::as_array)
        .ok_or("required report ID array is missing")?
        .iter()
        .map(|value| value.as_str().ok_or("report ID array is not strings"))
        .collect()
}

fn require_same_string_set(
    value: &serde_json::Value,
    field: &'static str,
    expected: &[&str],
) -> Result<(), &'static str> {
    let mut observed = semantic_string_array(value, field)?;
    let mut expected = expected.to_vec();
    observed.sort_unstable();
    expected.sort_unstable();
    if observed != expected {
        return Err("report ID set linkage mismatch");
    }
    Ok(())
}

struct V4Losses<'a> {
    authority: V4Loss<'a>,
    snapshot: &'a IndexSnapshotV5,
}

impl Serialize for V4Losses<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let m5_count = M5_ARRAY_KINDS
            .iter()
            .filter(|kind| kind.count(self.snapshot) != 0)
            .count();
        let mut sequence = serializer.serialize_seq(Some(1 + m5_count))?;
        sequence.serialize_element(&self.authority)?;
        for kind in M5_ARRAY_KINDS {
            if kind.count(self.snapshot) != 0 {
                sequence.serialize_element(&M5OmissionLoss {
                    kind,
                    snapshot: self.snapshot,
                })?;
            }
        }
        sequence.end()
    }
}

struct M5OmissionLoss<'a> {
    kind: M5ArrayKind,
    snapshot: &'a IndexSnapshotV5,
}

#[derive(Serialize)]
struct M5OmissionLossWire<'a> {
    affected_properties: [&'static str; 1],
    kind: &'static str,
    meaningful: bool,
    reason: &'static str,
    recoverable: bool,
    recovery_ref: &'static str,
    source_ids: M5LossSourceIds<'a>,
}

impl Serialize for M5OmissionLoss<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        M5OmissionLossWire {
            affected_properties: [reviewgraphen_core::DOUBLE_SUBMIT_PROPERTY_ID],
            kind: "omitted_m5_gluing_records",
            meaningful: true,
            reason: self.kind.reason(),
            recoverable: true,
            recovery_ref: self.kind.recovery_ref(),
            source_ids: M5LossSourceIds {
                kind: self.kind,
                snapshot: self.snapshot,
            },
        }
        .serialize(serializer)
    }
}

struct M5LossSourceIds<'a> {
    kind: M5ArrayKind,
    snapshot: &'a IndexSnapshotV5,
}

impl Serialize for M5LossSourceIds<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut ids = [None; 4];
        let mut len = 0_usize;
        match self.kind {
            M5ArrayKind::GluingInputDescriptors => {
                for row in &self.snapshot.gluing_input_descriptors {
                    push_loss_id(
                        &mut ids,
                        &mut len,
                        row.descriptor
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .ok_or_else(|| serde::ser::Error::custom("descriptor lacks id"))?,
                    );
                    push_loss_id(&mut ids, &mut len, row.registration_id.as_str());
                }
            }
            M5ArrayKind::ContextCovers => {
                for row in &self.snapshot.context_covers {
                    push_loss_id(
                        &mut ids,
                        &mut len,
                        value_id_for_serialize(&row.cover, "id")?,
                    );
                }
            }
            M5ArrayKind::Sections => {
                for row in &self.snapshot.sections {
                    push_loss_id(
                        &mut ids,
                        &mut len,
                        value_id_for_serialize(&row.section, "id")?,
                    );
                }
            }
            M5ArrayKind::GluingAttempts => {
                for row in &self.snapshot.gluing_attempts {
                    push_loss_id(
                        &mut ids,
                        &mut len,
                        value_id_for_serialize(&row.attempt, "id")?,
                    );
                }
            }
            M5ArrayKind::Restrictions => {
                for row in &self.snapshot.restrictions {
                    push_loss_id(
                        &mut ids,
                        &mut len,
                        value_id_for_serialize(&row.restriction, "id")?,
                    );
                }
            }
            M5ArrayKind::GlobalCandidates => {
                for row in &self.snapshot.global_candidates {
                    push_loss_id(
                        &mut ids,
                        &mut len,
                        value_id_for_serialize(&row.candidate, "id")?,
                    );
                }
            }
            M5ArrayKind::GluingObstructions => {
                for row in &self.snapshot.gluing_obstructions {
                    push_loss_id(
                        &mut ids,
                        &mut len,
                        value_id_for_serialize(&row.obstruction, "id")?,
                    );
                }
            }
        }
        ids[..len].sort_unstable();
        let mut sequence = serializer.serialize_seq(Some(len))?;
        for id in ids[..len].iter().flatten() {
            sequence.serialize_element(id)?;
        }
        sequence.end()
    }
}

fn push_loss_id<'a>(ids: &mut [Option<&'a str>; 4], len: &mut usize, id: &'a str) {
    if *len < ids.len() {
        ids[*len] = Some(id);
        *len += 1;
    }
}

fn value_id_for_serialize<'a, E: serde::ser::Error>(
    value: &'a serde_json::Value,
    field: &'static str,
) -> Result<&'a str, E> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| E::custom("M5 record lacks id"))
}
#[derive(Serialize)]
struct V4Payload<'a> {
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
impl<'a> V4Projection<'a> {
    fn new(
        request: &'a ReportRequestV4,
        status: Status,
        execution_ids: &'a [StableId],
        claim_ids: &'a [StableId],
        obstruction_kind_mask: u8,
        snapshot: &'a IndexSnapshotV5,
    ) -> Self {
        Self {
            views: One(V4View {
                kind: "machine",
                source_ids: &request.selected_obligation_ids,
                information_loss: V4Losses {
                    authority: V4Loss {
                        kind: "authority_detail_omitted",
                        reason: "The machine projection omits authority detail retained by this source-bound report.",
                        source_ids: &request.selected_obligation_ids,
                        affected_properties: ["review.authority_trace"],
                        meaningful: true,
                        recoverable: true,
                        recovery_ref: &request.report_id,
                    },
                    snapshot,
                },
                payload: V4Payload {
                    status,
                    execution_ids,
                    claim_ids,
                    obstruction_kinds: ObstructionKinds(obstruction_kind_mask),
                },
            }),
        }
    }
}

struct RegistrationRows<'a> {
    rows: &'a [IndexArtifactRegistrationV5],
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
    source: &'a serde_json::Value,
}
impl<'a> From<&'a IndexArtifactRegistrationV5> for Registration<'a> {
    fn from(r: &'a IndexArtifactRegistrationV5) -> Self {
        Self {
            event_sequence: r.event_sequence,
            event_id: &r.event_id,
            registration_id: &r.registration_id,
            run_id: &r.run_id,
            cas_hash: &r.cas_hash,
            media_type: &r.media_type,
            size: r.size,
            sensitivity: &r.sensitivity,
            source: &r.source,
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
    IndexEvidenceV3AtV5,
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
impl<'a> From<&'a IndexEvidenceV3AtV5> for Evidence<'a> {
    fn from(r: &'a IndexEvidenceV3AtV5) -> Self {
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
    IndexEvidenceBindingV3AtV5,
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
impl<'a> From<&'a IndexEvidenceBindingV3AtV5> for Binding<'a> {
    fn from(r: &'a IndexEvidenceBindingV3AtV5) -> Self {
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
    IndexVerificationV3AtV5,
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
impl<'a> From<&'a IndexVerificationV3AtV5> for Verification<'a> {
    fn from(r: &'a IndexVerificationV3AtV5) -> Self {
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
    IndexDecisionV3AtV5,
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
impl<'a> From<&'a IndexDecisionV3AtV5> for Decision<'a> {
    fn from(r: &'a IndexDecisionV3AtV5) -> Self {
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
selected_rows!(
    FindingRows,
    IndexFindingV3AtV5,
    finding_id,
    finding_id,
    Finding
);
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
impl<'a> From<&'a IndexFindingV3AtV5> for Finding<'a> {
    fn from(r: &'a IndexFindingV3AtV5) -> Self {
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
    rows: &'a [IndexClaimAssessmentV3AtV5],
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
impl<'a> From<&'a IndexClaimAssessmentV3AtV5> for Assessment<'a> {
    fn from(r: &'a IndexClaimAssessmentV3AtV5) -> Self {
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
    shape: &V4Report<'_>,
    snapshot: &IndexSnapshotV5,
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
    rows!(
        snapshot
            .gluing_input_descriptors
            .iter()
            .zip(&snapshot.artifact_registrations_v4)
            .map(
                |(descriptor, registration)| GluingInputDescriptorV4ReportItem {
                    schema: M5_DESCRIPTOR_ITEM_SCHEMA,
                    descriptor: &descriptor.descriptor,
                    descriptor_body_hash: &descriptor.body_hash,
                    registration_id: &descriptor.registration_id,
                    registration: ArtifactRegistrationV4ReportItem::from(registration),
                }
            )
    );
    rows!(
        snapshot
            .context_covers
            .iter()
            .map(|row| ContextCoverReportItem {
                event_sequence: row.event_sequence,
                event_id: &row.event_id,
                cover: &row.cover,
                body_hash: &row.body_hash,
            })
    );
    rows!(snapshot.sections.iter().map(|row| SectionReportItem {
        event_sequence: row.event_sequence,
        event_id: &row.event_id,
        section: &row.section,
        body_hash: &row.body_hash,
    }));
    rows!(
        snapshot
            .restrictions
            .iter()
            .map(|row| RestrictionReportItem {
                event_sequence: row.event_sequence,
                event_id: &row.event_id,
                restriction: &row.restriction,
                body_hash: &row.body_hash,
            })
    );
    rows!(
        snapshot
            .gluing_attempts
            .iter()
            .map(|row| GluingAttemptReportItem {
                event_sequence: row.event_sequence,
                event_id: &row.event_id,
                attempt: &row.attempt,
                body_hash: &row.body_hash,
            })
    );
    rows!(
        snapshot
            .global_candidates
            .iter()
            .map(|row| GlobalCandidateReportItem {
                event_sequence: row.event_sequence,
                event_id: &row.event_id,
                candidate: &row.candidate,
                body_hash: &row.body_hash,
            })
    );
    rows!(
        snapshot
            .gluing_obstructions
            .iter()
            .map(|row| GluingObstructionReportItem {
                event_sequence: row.event_sequence,
                event_id: &row.event_id,
                obstruction: &row.obstruction,
                body_hash: &row.body_hash,
            })
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

    #[test]
    fn global_candidate_has_an_independent_bound_and_all_seven_losses_are_counted() {
        let counts = M5ReportCounts {
            artifact_registrations_v4: 2,
            gluing_input_descriptors: 2,
            context_covers: 1,
            sections: 2,
            gluing_attempts: 1,
            restrictions: 2,
            global_candidates: 1,
            gluing_obstructions: 1,
        };
        assert_eq!(counts.omission_losses(), 7);
        ReportLimitsV4::default()
            .preflight(ReportCounts::default(), counts, 0, 0, 0)
            .unwrap();
        assert!(matches!(
            ReportLimitsV4 {
                global_candidates: 0,
                ..ReportLimitsV4::default()
            }
            .preflight(ReportCounts::default(), counts, 0, 0, 0),
            Err(ReportError::Incomplete {
                operation: "global_candidates",
                limit: 0,
                observed: 1,
            })
        ));
    }

    #[test]
    fn total_v4_rows_accept_exact_and_one_more_and_refuse_one_less_or_overflow() {
        let counts = M5ReportCounts {
            artifact_registrations_v4: 2,
            gluing_input_descriptors: 2,
            context_covers: 1,
            sections: 2,
            gluing_attempts: 1,
            restrictions: 2,
            global_candidates: 1,
            gluing_obstructions: 0,
        };
        let total = counts.rows().unwrap();
        for limit in [total, total + 1] {
            ReportLimitsV4 {
                inherited: ReportLimits {
                    rows: limit,
                    ..ReportLimits::default()
                },
                ..ReportLimitsV4::default()
            }
            .preflight(ReportCounts::default(), counts, 0, 0, 0)
            .unwrap();
        }
        assert!(matches!(
            ReportLimitsV4 {
                inherited: ReportLimits {
                    rows: total - 1,
                    ..ReportLimits::default()
                },
                ..ReportLimitsV4::default()
            }
            .preflight(ReportCounts::default(), counts, 0, 0, 0),
            Err(ReportError::Incomplete {
                operation: "report_v4_rows",
                limit,
                observed,
            }) if limit == total - 1 && observed == total
        ));
        assert!(matches!(
            M5ReportCounts {
                artifact_registrations_v4: u64::MAX,
                gluing_input_descriptors: 1,
                ..M5ReportCounts::default()
            }
            .rows(),
            Err(ReportError::Incomplete {
                operation: "report_v4_rows",
                observed: u64::MAX,
                ..
            })
        ));
    }
}
