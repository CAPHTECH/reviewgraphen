//! Pure M5 context-cover and gluing contracts.
//!
//! These records do not persist events and do not create or mutate program
//! facts, claims, evidence, verification, decisions, findings, acceptance, or
//! coverage. A [`GlobalCandidateV4`] is only a mechanically compatible local
//! assignment projection; it is never a global safety result or sign-off.
#![allow(dead_code)] // Activated by the separately reviewed event-v4 integration unit.

use crate::{
    ClaimAssessmentV3, ContentHash, DomainError, EvidenceRelationV3, ExecutionClaimV2, Obligation,
    ReviewAggregate, StableId, VerificationOutcomeV3,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const DOUBLE_SUBMIT_GLUING_DESCRIPTOR_ID: &str = "reviewgraphen.double_submit_gluing@1";
pub const DOUBLE_SUBMIT_PROFILE_ID: &str = "double-submit-payment@1";
pub const DOUBLE_SUBMIT_PROPERTY_ID: &str = "payment.at_most_once";
pub const DOUBLE_SUBMIT_INVARIANT_ID: &str = "invariant:payment-at-most-once";
pub const DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID: &str = "context:payment";
pub const DOUBLE_SUBMIT_UI_CONTEXT_ID: &str = "context:ui-event";
pub const DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID: &str = "function:checkout-submit";
pub const DOUBLE_SUBMIT_ASSIGNMENT_KEY: &str = "caller_duplicate_protection";

pub const MAX_M5_DESCRIPTOR_QUALIFICATION_IDS: usize = 32;
pub const MAX_M5_REQUIRED_CONTEXTS: usize = 2;
pub const MAX_M5_SELECTED_OBLIGATIONS: usize = 2_048;
pub const MAX_M5_CONTEXT_MEMBER_IDS: usize = 4_096;
pub const MAX_M5_OVERLAP_IDS: usize = 4_096;
pub const MAX_M5_COVER_DOMAIN_IDS: usize = 16_384;
pub const MAX_M5_COVER_SOURCE_IDS: usize = 16_387;
pub const MAX_M5_SECTIONS: usize = 2;
pub const MAX_M5_CLAIM_SOURCE_IDS: usize = 128;
pub const MAX_M5_SECTION_TRACE_IDS: usize = 64;
pub const MAX_M5_SECTION_SOURCE_IDS: usize = 455;
pub const MAX_M5_RESTRICTION_SOURCE_IDS: usize = 4_424;
pub const MAX_M5_TRACE_IDS: usize = 128;
pub const MAX_M5_ATTEMPT_SOURCE_IDS: usize = 4_944;
pub const MAX_M5_DESCRIPTOR_CANONICAL_BYTES: usize = 65_536;
pub const MAX_M5_BUNDLE_CANONICAL_BYTES: usize = 1_048_576;
pub const MAX_M5_STABLE_ID_BYTES: usize = 256;
/// `cover.source_ids` plus, for each of at most two selected Sections, the
/// obligation/claim IDs, claim sources, and five exact M4 trace sets, plus the
/// two descriptor/registration pairs retained by a legal 0/1/2 prefix.
pub const MAX_M5_PROFILE_SOURCE_IDS: usize = MAX_M5_COVER_SOURCE_IDS
    + MAX_M5_SECTIONS * (2 + MAX_M5_CLAIM_SOURCE_IDS + 5 * MAX_M5_SECTION_TRACE_IDS)
    + 2 * MAX_M5_REQUIRED_CONTEXTS;
pub const MAX_M5_PROFILE_SOURCE_RETAINED_BYTES: usize =
    MAX_M5_PROFILE_SOURCE_IDS * (std::mem::size_of::<StableId>() + MAX_M5_STABLE_ID_BYTES);

pub type M5Result<T> = std::result::Result<T, M5Error>;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum M5Error {
    #[error("snapshot mismatch: expected {expected}, got {actual}")]
    SnapshotMismatch {
        expected: StableId,
        actual: StableId,
    },
    #[error("{field} mismatch: expected {expected}, got {actual}")]
    BindingMismatch {
        field: &'static str,
        expected: String,
        actual: String,
    },
    #[error("duplicate value in {field}")]
    Duplicate { field: &'static str },
    #[error("non-canonical context order in {field}")]
    ContextOrder { field: &'static str },
    #[error("{field} must not be empty")]
    Empty { field: &'static str },
    #[error("{operation} exceeds limit {limit} (observed {observed})")]
    Incomplete {
        operation: &'static str,
        limit: usize,
        observed: usize,
    },
    #[error("invalid M5 record: {0}")]
    Validation(String),
    #[error("canonical M5 serialization failed: {0}")]
    Canonical(String),
}

impl From<DomainError> for M5Error {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::Incomplete {
                operation,
                limit,
                observed,
            } => Self::Incomplete {
                operation,
                limit,
                observed,
            },
            DomainError::CanonicalJson(reason) => Self::Canonical(reason),
            other => Self::Validation(other.to_string()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentValueV4 {
    Satisfied,
    Required,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentCompatibilityV4 {
    Compatible,
    Conflict,
    Unknown,
}

impl AssignmentValueV4 {
    #[must_use]
    pub const fn compatibility(self, other: Self) -> AssignmentCompatibilityV4 {
        use AssignmentCompatibilityV4::{Compatible, Conflict, Unknown};
        use AssignmentValueV4::{Required, Satisfied};
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Unknown,
            (Satisfied, Satisfied) | (Required, Required) => Compatible,
            (Satisfied, Required) | (Required, Satisfied) => Conflict,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GluingResultV4 {
    Unknown,
    Failed,
    Candidate,
    GluedWithQualification,
    Glued,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GluingObstructionKindV4 {
    RequiredSectionMissing,
    RequiredOverlapMissing,
    SectionUnknown,
    AssignmentConflict,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GluingRequiredResolutionV4 {
    RecordRequiredSection,
    ResolveContextOverlap,
    ResolveUnknownDuplicateProtectionAssignment,
    ResolveDuplicateProtectionResponsibility,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum M5SeverityV4 {
    High,
    Critical,
}

// Wire representations are deliberately private. Public M5 records are
// validated values and cannot be constructed through serde. A wire value is
// useful only as a closed, bounded decoding intermediate.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AssignmentValueWire {
    Satisfied,
    Required,
    Unknown,
}

impl From<AssignmentValueWire> for AssignmentValueV4 {
    fn from(value: AssignmentValueWire) -> Self {
        match value {
            AssignmentValueWire::Satisfied => Self::Satisfied,
            AssignmentValueWire::Required => Self::Required,
            AssignmentValueWire::Unknown => Self::Unknown,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum GluingResultWire {
    Unknown,
    Failed,
    Candidate,
    GluedWithQualification,
    Glued,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum GluingObstructionKindWire {
    RequiredSectionMissing,
    RequiredOverlapMissing,
    SectionUnknown,
    AssignmentConflict,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum GluingRequiredResolutionWire {
    RecordRequiredSection,
    ResolveContextOverlap,
    ResolveUnknownDuplicateProtectionAssignment,
    ResolveDuplicateProtectionResponsibility,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum M5SeverityWire {
    High,
    Critical,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GluingInputDescriptorWire {
    schema: String,
    id: StableId,
    run_id: StableId,
    snapshot_id: StableId,
    universe_id: StableId,
    plan_id: StableId,
    profile_descriptor_id: String,
    context_id: StableId,
    assignment_key: String,
    assignment_value: AssignmentValueWire,
    qualification_source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextCoverWire {
    schema: String,
    id: StableId,
    run_id: StableId,
    snapshot_id: StableId,
    universe_id: StableId,
    plan_id: StableId,
    profile_descriptor_id: String,
    selected_obligation_ids: BTreeSet<StableId>,
    required_context_ids: Vec<StableId>,
    cover_domain_ids: BTreeSet<StableId>,
    covered_domain_ids: BTreeSet<StableId>,
    uncovered_domain_ids: BTreeSet<StableId>,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SectionWire {
    schema: String,
    id: StableId,
    cover_id: StableId,
    context_id: StableId,
    snapshot_id: StableId,
    property_id: String,
    invariant_id: StableId,
    obligation_id: StableId,
    claim_id: StableId,
    claim_assessment_id: StableId,
    input_descriptor_id: StableId,
    input_registration_id: StableId,
    assignment_key: String,
    assignment_value: AssignmentValueWire,
    passed_current_verification: bool,
    source_ids: BTreeSet<StableId>,
    qualification_source_ids: BTreeSet<StableId>,
    binding_ids: BTreeSet<StableId>,
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_ids: BTreeSet<StableId>,
    finding_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestrictionWire {
    schema: String,
    id: StableId,
    section_id: StableId,
    context_pair: Vec<StableId>,
    overlap_member_ids: BTreeSet<StableId>,
    assignment_key: String,
    assignment_value: AssignmentValueWire,
    source_ids: BTreeSet<StableId>,
    qualification_source_ids: BTreeSet<StableId>,
    claim_ids: BTreeSet<StableId>,
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_ids: BTreeSet<StableId>,
    finding_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlobalCandidateWire {
    schema: String,
    id: StableId,
    cover_id: StableId,
    invariant_id: StableId,
    property_id: String,
    required_section_ids: Vec<StableId>,
    restriction_ids: Vec<StableId>,
    qualification_source_ids: BTreeSet<StableId>,
    source_ids: BTreeSet<StableId>,
    claim_ids: BTreeSet<StableId>,
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_ids: BTreeSet<StableId>,
    finding_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GluingAttemptWire {
    schema: String,
    id: StableId,
    cover_id: StableId,
    snapshot_id: StableId,
    property_id: String,
    invariant_id: StableId,
    input_descriptor_ids: Vec<StableId>,
    section_ids: Vec<StableId>,
    restriction_ids: Vec<StableId>,
    result: GluingResultWire,
    global_candidate_id: Option<StableId>,
    obstruction_id: Option<StableId>,
    source_ids: BTreeSet<StableId>,
    claim_ids: BTreeSet<StableId>,
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_ids: BTreeSet<StableId>,
    finding_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GluingObstructionWire {
    schema: String,
    id: StableId,
    attempt_id: StableId,
    kind: GluingObstructionKindWire,
    conflicting_context_ids: Vec<StableId>,
    section_ids: Vec<StableId>,
    overlap_member_ids: BTreeSet<StableId>,
    assignment_key: String,
    left_assignment_value: Option<AssignmentValueWire>,
    right_assignment_value: Option<AssignmentValueWire>,
    source_ids: BTreeSet<StableId>,
    claim_ids: BTreeSet<StableId>,
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_ids: BTreeSet<StableId>,
    finding_ids: BTreeSet<StableId>,
    affected_invariant_id: StableId,
    severity: M5SeverityWire,
    required_resolution: GluingRequiredResolutionWire,
    human_decision_required: bool,
    blocks: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GluingBundleWire {
    schema: String,
    cover: ContextCoverWire,
    input_descriptor_ids: Vec<StableId>,
    sections: Vec<SectionWire>,
    restrictions: Vec<RestrictionWire>,
    attempt: GluingAttemptWire,
    global_candidate: Option<GlobalCandidateWire>,
    obstruction: Option<GluingObstructionWire>,
}

// Fields are deliberately declared in lexical key order. The streaming
// canonical hasher relies on this and avoids a `serde_json::Value` tree.
#[derive(Serialize)]
struct DescriptorIdentity<'a> {
    assignment_key: &'a str,
    assignment_value: AssignmentValueV4,
    context_id: &'a StableId,
    plan_id: &'a StableId,
    profile_descriptor_id: &'a str,
    qualification_source_ids: &'a BTreeSet<StableId>,
    run_id: &'a StableId,
    snapshot_id: &'a StableId,
    universe_id: &'a StableId,
}

#[derive(Serialize)]
struct CoverIdentity<'a> {
    cover_domain_ids: &'a BTreeSet<StableId>,
    plan_id: &'a StableId,
    profile_descriptor_id: &'a str,
    required_context_ids: &'a Vec<StableId>,
    run_id: &'a StableId,
    selected_obligation_ids: &'a BTreeSet<StableId>,
    snapshot_id: &'a StableId,
    universe_id: &'a StableId,
}

#[derive(Serialize)]
struct SectionIdentity<'a> {
    assignment_key: &'a str,
    assignment_value: AssignmentValueV4,
    claim_id: &'a StableId,
    context_id: &'a StableId,
    cover_id: &'a StableId,
    input_descriptor_id: &'a StableId,
    input_registration_id: &'a StableId,
    invariant_id: &'a StableId,
    obligation_id: &'a StableId,
    property_id: &'a str,
    qualification_source_ids: &'a BTreeSet<StableId>,
    snapshot_id: &'a StableId,
}

#[derive(Serialize)]
struct RestrictionIdentity<'a> {
    assignment_key: &'a str,
    assignment_value: AssignmentValueV4,
    context_pair: &'a Vec<StableId>,
    overlap_member_ids: &'a BTreeSet<StableId>,
    qualification_source_ids: &'a BTreeSet<StableId>,
    section_id: &'a StableId,
}

#[derive(Serialize)]
struct CandidateIdentity<'a> {
    cover_id: &'a StableId,
    invariant_id: &'a StableId,
    property_id: &'a str,
    qualification_source_ids: &'a BTreeSet<StableId>,
    required_section_ids: &'a Vec<StableId>,
    restriction_ids: &'a Vec<StableId>,
}

#[derive(Serialize)]
struct AttemptIdentity<'a> {
    cover_id: &'a StableId,
    input_descriptor_ids: &'a Vec<StableId>,
    invariant_id: &'a StableId,
    property_id: &'a str,
    restriction_ids: &'a Vec<StableId>,
    result: GluingResultV4,
    section_ids: &'a Vec<StableId>,
    snapshot_id: &'a StableId,
}

#[derive(Serialize)]
struct ObstructionIdentity<'a> {
    affected_invariant_id: &'a StableId,
    assignment_key: &'a str,
    attempt_id: &'a StableId,
    blocks: &'a BTreeSet<StableId>,
    conflicting_context_ids: &'a Vec<StableId>,
    kind: GluingObstructionKindV4,
    left_assignment_value: Option<AssignmentValueV4>,
    overlap_member_ids: &'a BTreeSet<StableId>,
    required_resolution: GluingRequiredResolutionV4,
    right_assignment_value: Option<AssignmentValueV4>,
    section_ids: &'a Vec<StableId>,
    severity: M5SeverityV4,
}

fn fixed_id(value: &str) -> StableId {
    StableId::parse(value).expect("ADR 0022 fixed IDs are valid")
}

fn identity(kind: &str, body: &impl Serialize) -> M5Result<StableId> {
    StableId::derived_streaming(
        kind,
        body,
        MAX_M5_BUNDLE_CANONICAL_BYTES,
        "M5 identity bytes",
    )
    .map_err(Into::into)
}

fn bounded<T>(set: &BTreeSet<T>, limit: usize, operation: &'static str) -> M5Result<()> {
    bounded_len(set.len(), limit, operation)
}

fn bounded_len(observed: usize, limit: usize, operation: &'static str) -> M5Result<()> {
    if observed > limit {
        return Err(M5Error::Incomplete {
            operation,
            limit,
            observed,
        });
    }
    Ok(())
}

fn bounded_bytes<T: Serialize>(record: &T, limit: usize, operation: &'static str) -> M5Result<()> {
    crate::canonical::canonical_json_count_bounded(record, limit, operation)?;
    Ok(())
}

fn require_id_bytes(id: &StableId, field: &'static str) -> M5Result<()> {
    if id.as_str().len() > MAX_M5_STABLE_ID_BYTES {
        return Err(M5Error::Incomplete {
            operation: field,
            limit: MAX_M5_STABLE_ID_BYTES,
            observed: id.as_str().len(),
        });
    }
    Ok(())
}

fn checked_peak_add(
    total: u64,
    addition: u64,
    limit: usize,
    operation: &'static str,
) -> M5Result<u64> {
    let next = total.checked_add(addition).ok_or(M5Error::Incomplete {
        operation,
        limit,
        observed: usize::MAX,
    })?;
    if next > limit as u64 {
        return Err(M5Error::Incomplete {
            operation,
            limit,
            observed: usize::try_from(next).unwrap_or(usize::MAX),
        });
    }
    Ok(next)
}

fn preflight_union(
    sets: &[&BTreeSet<StableId>],
    fixed: &[&StableId],
    limit: usize,
    operation: &'static str,
) -> M5Result<usize> {
    Ok(preflight_union_admission(sets, fixed, limit, operation)?.count)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UnionAdmission {
    count: usize,
    retained_bytes: u64,
}

fn admit_union_id(
    admission: &mut UnionAdmission,
    id: &StableId,
    limit: usize,
    operation: &'static str,
) -> M5Result<()> {
    admission.count = admission.count.checked_add(1).ok_or(M5Error::Incomplete {
        operation,
        limit,
        observed: usize::MAX,
    })?;
    bounded_len(admission.count, limit, operation)?;
    require_id_bytes(id, operation)?;
    let retained = std::mem::size_of::<StableId>()
        .checked_add(id.as_str().len())
        .ok_or(M5Error::Incomplete {
            operation: "M5 union retained working bytes",
            limit: MAX_M5_BUNDLE_CANONICAL_BYTES,
            observed: usize::MAX,
        })?;
    admission.retained_bytes = checked_peak_add(
        admission.retained_bytes,
        u64::try_from(retained).unwrap_or(u64::MAX),
        MAX_M5_BUNDLE_CANONICAL_BYTES,
        "M5 union retained working bytes",
    )?;
    Ok(())
}

fn preflight_union_admission(
    sets: &[&BTreeSet<StableId>],
    fixed: &[&StableId],
    limit: usize,
    operation: &'static str,
) -> M5Result<UnionAdmission> {
    let mut admission = UnionAdmission {
        count: 0,
        retained_bytes: 0,
    };
    for (set_index, set) in sets.iter().enumerate() {
        for id in *set {
            if sets[..set_index].iter().any(|earlier| earlier.contains(id)) {
                continue;
            }
            admit_union_id(&mut admission, id, limit, operation)?;
        }
    }
    for (index, id) in fixed.iter().enumerate() {
        if fixed[..index].contains(id) || sets.iter().any(|set| set.contains(*id)) {
            continue;
        }
        admit_union_id(&mut admission, id, limit, operation)?;
    }
    Ok(admission)
}

#[cfg(test)]
thread_local! {
    static M5_COVER_DOMAIN_TARGET_ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static M5_OBSTRUCTION_SOURCE_TARGET_ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn note_cover_domain_target_allocation() {
    #[cfg(test)]
    M5_COVER_DOMAIN_TARGET_ALLOCATIONS.with(|count| count.set(count.get() + 1));
}

fn note_obstruction_source_target_allocation() {
    #[cfg(test)]
    M5_OBSTRUCTION_SOURCE_TARGET_ALLOCATIONS.with(|count| count.set(count.get() + 1));
}

fn preflight_mint_cover_domain(
    invariant_scope_ids: &BTreeSet<StableId>,
    payment_member_ids: &BTreeSet<StableId>,
    ui_event_member_ids: &BTreeSet<StableId>,
    obligations: &[&Obligation],
) -> M5Result<UnionAdmission> {
    let mut admission = UnionAdmission {
        count: 0,
        retained_bytes: 0,
    };
    for id in invariant_scope_ids {
        admit_union_id(
            &mut admission,
            id,
            MAX_M5_COVER_DOMAIN_IDS,
            "M5 mint cover domain",
        )?;
    }
    for id in payment_member_ids {
        if !invariant_scope_ids.contains(id) {
            admit_union_id(
                &mut admission,
                id,
                MAX_M5_COVER_DOMAIN_IDS,
                "M5 mint cover domain",
            )?;
        }
    }
    for id in ui_event_member_ids {
        if !invariant_scope_ids.contains(id) && !payment_member_ids.contains(id) {
            admit_union_id(
                &mut admission,
                id,
                MAX_M5_COVER_DOMAIN_IDS,
                "M5 mint cover domain",
            )?;
        }
    }
    let in_base = |id: &StableId| {
        invariant_scope_ids.contains(id)
            || payment_member_ids.contains(id)
            || ui_event_member_ids.contains(id)
    };
    for (index, obligation) in obligations.iter().enumerate() {
        let seen_earlier = |id: &StableId| {
            obligations[..index].iter().any(|earlier| {
                earlier.normalized_target_refs().contains(id)
                    || earlier.normalized_source_ids().contains(id)
            })
        };
        for id in obligation.normalized_target_refs() {
            if !in_base(id) && !seen_earlier(id) {
                admit_union_id(
                    &mut admission,
                    id,
                    MAX_M5_COVER_DOMAIN_IDS,
                    "M5 mint cover domain",
                )?;
            }
        }
        for id in obligation.normalized_source_ids() {
            if !in_base(id)
                && !obligation.normalized_target_refs().contains(id)
                && !seen_earlier(id)
            {
                admit_union_id(
                    &mut admission,
                    id,
                    MAX_M5_COVER_DOMAIN_IDS,
                    "M5 mint cover domain",
                )?;
            }
        }
    }
    Ok(admission)
}

fn preflight_obstruction_sources(
    attempt_source_ids: &BTreeSet<StableId>,
    conflicting_context_ids: &[StableId],
    overlap_member_ids: &BTreeSet<StableId>,
) -> M5Result<UnionAdmission> {
    let mut admission = preflight_union_admission(
        &[attempt_source_ids, overlap_member_ids],
        &[],
        MAX_M5_ATTEMPT_SOURCE_IDS,
        "M5 Obstruction source_ids",
    )?;
    for (index, id) in conflicting_context_ids.iter().enumerate() {
        if attempt_source_ids.contains(id)
            || overlap_member_ids.contains(id)
            || conflicting_context_ids[..index].contains(id)
        {
            continue;
        }
        admit_union_id(
            &mut admission,
            id,
            MAX_M5_ATTEMPT_SOURCE_IDS,
            "M5 Obstruction source_ids",
        )?;
    }
    Ok(admission)
}

fn build_obstruction_sources(
    attempt_source_ids: &BTreeSet<StableId>,
    conflicting_context_ids: &[StableId],
    overlap_member_ids: &BTreeSet<StableId>,
) -> M5Result<BTreeSet<StableId>> {
    let admission = preflight_obstruction_sources(
        attempt_source_ids,
        conflicting_context_ids,
        overlap_member_ids,
    )?;
    note_obstruction_source_target_allocation();
    let mut source_ids = attempt_source_ids.clone();
    source_ids.extend(conflicting_context_ids.iter().cloned());
    source_ids.extend(overlap_member_ids.iter().cloned());
    if source_ids.len() != admission.count {
        return Err(M5Error::Validation(
            "M5 Obstruction source union changed after admission".into(),
        ));
    }
    Ok(source_ids)
}

#[derive(Clone, Copy)]
enum WireShape {
    Descriptor,
    Bundle,
    Cover,
    Section,
    Restriction,
    Candidate,
    Attempt,
    Obstruction,
    Scalar,
}

fn wire_array_rule(shape: WireShape, key: &[u8]) -> Option<(usize, WireShape)> {
    use WireShape::{Obstruction, Scalar};
    let scalar = |limit| Some((limit, Scalar));
    match (shape, key) {
        (WireShape::Descriptor, b"qualification_source_ids") => {
            scalar(MAX_M5_DESCRIPTOR_QUALIFICATION_IDS)
        }
        (WireShape::Bundle, b"input_descriptor_ids") => scalar(2),
        (WireShape::Bundle, b"sections") => Some((MAX_M5_SECTIONS, WireShape::Section)),
        (WireShape::Bundle, b"restrictions") => Some((MAX_M5_SECTIONS, WireShape::Restriction)),
        (WireShape::Cover, b"selected_obligation_ids") => scalar(MAX_M5_SELECTED_OBLIGATIONS),
        (WireShape::Cover, b"required_context_ids") => scalar(MAX_M5_REQUIRED_CONTEXTS),
        (
            WireShape::Cover,
            b"cover_domain_ids" | b"covered_domain_ids" | b"uncovered_domain_ids",
        ) => scalar(MAX_M5_COVER_DOMAIN_IDS),
        (WireShape::Cover, b"source_ids") => scalar(MAX_M5_COVER_SOURCE_IDS),
        (WireShape::Section, b"source_ids") => scalar(MAX_M5_SECTION_SOURCE_IDS),
        (WireShape::Section, b"qualification_source_ids") => {
            scalar(MAX_M5_DESCRIPTOR_QUALIFICATION_IDS)
        }
        (
            WireShape::Section,
            b"binding_ids" | b"evidence_ids" | b"verification_ids" | b"decision_ids"
            | b"finding_ids",
        ) => scalar(MAX_M5_SECTION_TRACE_IDS),
        (WireShape::Restriction, b"context_pair") => scalar(MAX_M5_REQUIRED_CONTEXTS),
        (WireShape::Restriction, b"overlap_member_ids") => scalar(MAX_M5_OVERLAP_IDS),
        (WireShape::Restriction, b"source_ids") => scalar(MAX_M5_RESTRICTION_SOURCE_IDS),
        (WireShape::Restriction, b"qualification_source_ids") => {
            scalar(MAX_M5_DESCRIPTOR_QUALIFICATION_IDS)
        }
        (WireShape::Restriction, b"claim_ids") => scalar(1),
        (
            WireShape::Restriction,
            b"evidence_ids" | b"verification_ids" | b"decision_ids" | b"finding_ids",
        ) => scalar(MAX_M5_SECTION_TRACE_IDS),
        (WireShape::Candidate, b"required_section_ids" | b"restriction_ids") => scalar(2),
        (WireShape::Candidate, b"qualification_source_ids") => scalar(64),
        (WireShape::Candidate, b"source_ids") => scalar(MAX_M5_ATTEMPT_SOURCE_IDS),
        (WireShape::Candidate, b"claim_ids") => scalar(2),
        (
            WireShape::Candidate,
            b"evidence_ids" | b"verification_ids" | b"decision_ids" | b"finding_ids",
        ) => scalar(MAX_M5_TRACE_IDS),
        (WireShape::Attempt, b"input_descriptor_ids" | b"section_ids" | b"restriction_ids") => {
            scalar(2)
        }
        (WireShape::Attempt, b"source_ids") => scalar(MAX_M5_ATTEMPT_SOURCE_IDS),
        (WireShape::Attempt, b"claim_ids") => scalar(2),
        (
            WireShape::Attempt,
            b"evidence_ids" | b"verification_ids" | b"decision_ids" | b"finding_ids",
        ) => scalar(MAX_M5_TRACE_IDS),
        (Obstruction, b"conflicting_context_ids" | b"section_ids") => scalar(2),
        (Obstruction, b"overlap_member_ids") => scalar(MAX_M5_OVERLAP_IDS),
        (Obstruction, b"source_ids") => scalar(MAX_M5_ATTEMPT_SOURCE_IDS),
        (Obstruction, b"claim_ids") => scalar(2),
        (Obstruction, b"evidence_ids" | b"verification_ids" | b"decision_ids" | b"finding_ids") => {
            scalar(MAX_M5_TRACE_IDS)
        }
        (Obstruction, b"blocks") => scalar(1),
        _ => None,
    }
}

fn wire_object_shape(shape: WireShape, key: &[u8]) -> WireShape {
    match (shape, key) {
        (WireShape::Bundle, b"cover") => WireShape::Cover,
        (WireShape::Bundle, b"attempt") => WireShape::Attempt,
        (WireShape::Bundle, b"global_candidate") => WireShape::Candidate,
        (WireShape::Bundle, b"obstruction") => WireShape::Obstruction,
        _ => WireShape::Scalar,
    }
}

struct JsonPreflight<'a> {
    input: &'a [u8],
    at: usize,
    operation: &'static str,
}

impl JsonPreflight<'_> {
    fn error(&self, reason: &str) -> M5Error {
        M5Error::Validation(format!(
            "{} preflight at byte {}: {reason}",
            self.operation, self.at
        ))
    }

    fn whitespace(&mut self) {
        while self.input.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn byte(&mut self, expected: u8) -> M5Result<()> {
        self.whitespace();
        if self.input.get(self.at) != Some(&expected) {
            return Err(self.error("unexpected JSON token"));
        }
        self.at += 1;
        Ok(())
    }

    fn string(&mut self) -> M5Result<(usize, usize)> {
        self.whitespace();
        if self.input.get(self.at) != Some(&b'"') {
            return Err(self.error("expected JSON string"));
        }
        self.at += 1;
        let start = self.at;
        while let Some(byte) = self.input.get(self.at).copied() {
            match byte {
                b'"' => {
                    let end = self.at;
                    let value_len = end - start;
                    if value_len > MAX_M5_STABLE_ID_BYTES {
                        return Err(M5Error::Incomplete {
                            operation: "M5 JSON string bytes",
                            limit: MAX_M5_STABLE_ID_BYTES,
                            observed: value_len,
                        });
                    }
                    self.at += 1;
                    return Ok((start, end));
                }
                b'\\' | 0..=31 | 128..=u8::MAX => {
                    return Err(self.error("M5 wire strings must be unescaped ASCII"));
                }
                _ => self.at += 1,
            }
        }
        Err(self.error("unterminated JSON string"))
    }

    fn scalar(&mut self) -> M5Result<()> {
        self.whitespace();
        if self.input.get(self.at) == Some(&b'"') {
            self.string()?;
            return Ok(());
        }
        let start = self.at;
        while self
            .input
            .get(self.at)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b',' | b']' | b'}'))
        {
            self.at += 1;
        }
        if self.at == start {
            return Err(self.error("expected JSON scalar"));
        }
        Ok(())
    }

    fn array(&mut self, limit: usize, element_shape: WireShape, depth: usize) -> M5Result<()> {
        if depth > 16 {
            return Err(self.error("M5 JSON nesting is too deep"));
        }
        self.byte(b'[')?;
        self.whitespace();
        if self.input.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(());
        }
        let mut observed = 0_usize;
        loop {
            observed = observed.checked_add(1).ok_or(M5Error::Incomplete {
                operation: "M5 JSON array cardinality",
                limit,
                observed: usize::MAX,
            })?;
            bounded_len(observed, limit, "M5 JSON array cardinality")?;
            self.value(element_shape, depth + 1)?;
            self.whitespace();
            match self.input.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(());
                }
                _ => return Err(self.error("unterminated JSON array")),
            }
        }
    }

    fn object(&mut self, shape: WireShape, depth: usize) -> M5Result<()> {
        if depth > 16 {
            return Err(self.error("M5 JSON nesting is too deep"));
        }
        self.byte(b'{')?;
        self.whitespace();
        if self.input.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(());
        }
        loop {
            let (key_start, key_end) = self.string()?;
            let array_rule = wire_array_rule(shape, &self.input[key_start..key_end]);
            let object_shape = wire_object_shape(shape, &self.input[key_start..key_end]);
            self.byte(b':')?;
            self.whitespace();
            match self.input.get(self.at) {
                Some(b'[') => {
                    let (limit, element_shape) =
                        array_rule.ok_or_else(|| self.error("unrecognized M5 array field"))?;
                    self.array(limit, element_shape, depth + 1)?;
                }
                Some(b'{') => self.object(object_shape, depth + 1)?,
                _ => self.scalar()?,
            }
            self.whitespace();
            match self.input.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(());
                }
                _ => return Err(self.error("unterminated JSON object")),
            }
        }
    }

    fn value(&mut self, shape: WireShape, depth: usize) -> M5Result<()> {
        self.whitespace();
        match self.input.get(self.at) {
            Some(b'{') => self.object(shape, depth),
            Some(b'[') => Err(self.error("unexpected unbound M5 JSON array")),
            _ => self.scalar(),
        }
    }
}

fn preflight_wire_json(
    input: &[u8],
    limit: usize,
    operation: &'static str,
    root: WireShape,
) -> M5Result<()> {
    if input.len() > limit {
        return Err(M5Error::Incomplete {
            operation,
            limit,
            observed: input.len(),
        });
    }
    let mut scanner = JsonPreflight {
        input,
        at: 0,
        operation,
    };
    scanner.value(root, 0)?;
    scanner.whitespace();
    if scanner.at != input.len() {
        return Err(scanner.error("trailing JSON bytes"));
    }
    Ok(())
}

fn union<'a>(sets: impl IntoIterator<Item = &'a BTreeSet<StableId>>) -> BTreeSet<StableId> {
    sets.into_iter().flat_map(BTreeSet::iter).cloned().collect()
}

fn require_same(field: &'static str, expected: &StableId, actual: &StableId) -> M5Result<()> {
    if expected != actual {
        return Err(M5Error::BindingMismatch {
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

fn validate_context_qualifications(
    cover: &ContextCoverV4,
    descriptors: [&GluingInputDescriptorV4; 2],
    sections: &[SectionV4],
) -> M5Result<()> {
    for descriptor in descriptors {
        let selected_section = sections
            .iter()
            .find(|section| section.context_id == descriptor.context_id);
        for qualification_id in &descriptor.qualification_source_ids {
            let in_domain = cover.cover_domain_ids.contains(qualification_id);
            let in_selected_assessment = selected_section.is_some_and(|section| {
                section.binding_ids.contains(qualification_id)
                    || section.evidence_ids.contains(qualification_id)
                    || section.verification_ids.contains(qualification_id)
                    || section.decision_ids.contains(qualification_id)
                    || section.finding_ids.contains(qualification_id)
            });
            if !in_domain && !in_selected_assessment {
                return Err(M5Error::Validation(
                    "M5 qualification source is outside its context's chosen closure".into(),
                ));
            }
        }
    }
    Ok(())
}

fn exact_current_assessment_for<'a>(
    assessment: Option<&'a ClaimAssessmentV3>,
    claim: &ExecutionClaimV2,
    run_id: &StableId,
    snapshot_id: &StableId,
    universe_id: &StableId,
) -> Option<&'a ClaimAssessmentV3> {
    assessment.filter(|assessment| {
        assessment.claim_id() == claim.id()
            && assessment.run_id() == run_id
            && assessment.snapshot_id() == snapshot_id
            && assessment.universe_id() == universe_id
    })
}

/// Non-serializable proof derived only from the retained current M4 closure.
struct CurrentVerificationProofV4 {
    passed: bool,
}

impl CurrentVerificationProofV4 {
    fn derive(
        assessment: &ClaimAssessmentV3,
        claim: &ExecutionClaimV2,
        obligation_id: &StableId,
        snapshot_id: &StableId,
    ) -> M5Result<Self> {
        if assessment.claim_id() != claim.id() || !claim.obligation_ids().contains(obligation_id) {
            return Err(M5Error::Validation(
                "M5 assessment/claim/obligation closure mismatch".into(),
            ));
        }
        if assessment.snapshot_id() != snapshot_id {
            return Err(M5Error::SnapshotMismatch {
                expected: snapshot_id.clone(),
                actual: assessment.snapshot_id().clone(),
            });
        }
        let passed = assessment.verifications().iter().any(|verification| {
            verification.outcome() == VerificationOutcomeV3::Passed
                && verification.claim_id() == claim.id()
                && !verification.evidence_ids().is_empty()
                && verification.evidence_ids().iter().all(|evidence_id| {
                    assessment.evidence().iter().any(|evidence| {
                        evidence.id() == evidence_id && evidence.snapshot_id() == snapshot_id
                    }) && assessment.bindings().iter().any(|binding| {
                        binding.evidence_id() == evidence_id
                            && binding.claim_id() == claim.id()
                            && binding.relation() == EvidenceRelationV3::Reproduces
                    })
                })
        });
        Ok(Self { passed })
    }
}

/// Exact current-state registration closure shared by event-v4 admission and
/// final M5 minting. It owns only the deterministic cover domain and the one
/// chosen assessment trace per eligible fixed context.
pub(crate) struct M5RegistrationClosureV4 {
    cover: ContextCoverV4,
    overlap_member_ids: BTreeSet<StableId>,
    chosen_contexts: Vec<M5ChosenContextClosureV4>,
}

struct M5ChosenContextClosureV4 {
    context_id: StableId,
    context_member_ids: BTreeSet<StableId>,
    obligation_id: StableId,
    claim_id: StableId,
    claim_source_ids: BTreeSet<StableId>,
    binding_ids: BTreeSet<StableId>,
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_ids: BTreeSet<StableId>,
    finding_ids: BTreeSet<StableId>,
    verification_passed: bool,
}

impl M5ChosenContextClosureV4 {
    fn contains_trace_id(&self, id: &StableId) -> bool {
        self.binding_ids.contains(id)
            || self.evidence_ids.contains(id)
            || self.verification_ids.contains(id)
            || self.decision_ids.contains(id)
            || self.finding_ids.contains(id)
    }
}

impl M5RegistrationClosureV4 {
    pub(crate) fn allows_qualification(
        &self,
        context_id: &StableId,
        qualification_id: &StableId,
    ) -> bool {
        self.cover.cover_domain_ids.contains(qualification_id)
            || self
                .chosen_contexts
                .iter()
                .find(|chosen| &chosen.context_id == context_id)
                .is_some_and(|chosen| chosen.contains_trace_id(qualification_id))
    }

    /// Projects the exact source trace and fixed qualification policy used by
    /// the runtime profile-basis seam. No caller-provided IDs participate.
    pub(crate) fn runtime_profile_projection(
        &self,
    ) -> M5Result<(BTreeSet<StableId>, BTreeSet<StableId>)> {
        let required_overlap = fixed_id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID);
        if !self.overlap_member_ids.contains(&required_overlap) {
            return Err(M5Error::Validation(
                "runtime M5 profile requires the fixed overlap member".into(),
            ));
        }
        for context_id in required_contexts() {
            if self
                .chosen_contexts
                .iter()
                .filter(|chosen| chosen.context_id == context_id)
                .count()
                > 1
            {
                return Err(M5Error::Validation(
                    "runtime M5 profile context assessment is ambiguous".into(),
                ));
            }
        }
        let payment_context = fixed_id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID);
        let payment = self
            .chosen_contexts
            .iter()
            .find(|chosen| chosen.context_id == payment_context);

        let qualification_count = payment
            .map_or(0, |chosen| chosen.evidence_ids.len())
            .checked_add(1)
            .ok_or(M5Error::Incomplete {
                operation: "runtime M5 payment qualifications",
                limit: MAX_M5_DESCRIPTOR_QUALIFICATION_IDS,
                observed: usize::MAX,
            })?;
        bounded_len(
            qualification_count,
            MAX_M5_DESCRIPTOR_QUALIFICATION_IDS,
            "runtime M5 payment qualifications",
        )?;
        let mut payment_qualifications = payment
            .map(|chosen| chosen.evidence_ids.clone())
            .unwrap_or_default();
        payment_qualifications.insert(required_overlap);

        let mut source_sets = Vec::with_capacity(2 + self.chosen_contexts.len() * 7);
        source_sets.push(&self.cover.source_ids);
        source_sets.push(&self.overlap_member_ids);
        let mut fixed_sources = Vec::with_capacity(self.chosen_contexts.len() * 3);
        for chosen in &self.chosen_contexts {
            source_sets.push(&chosen.context_member_ids);
            source_sets.push(&chosen.claim_source_ids);
            source_sets.push(&chosen.binding_ids);
            source_sets.push(&chosen.evidence_ids);
            source_sets.push(&chosen.verification_ids);
            source_sets.push(&chosen.decision_ids);
            source_sets.push(&chosen.finding_ids);
            fixed_sources.extend([&chosen.context_id, &chosen.obligation_id, &chosen.claim_id]);
        }
        let admission = preflight_profile_source_union(&source_sets, &fixed_sources)?;
        let mut source_ids = self.cover.source_ids.clone();
        source_ids.extend(self.overlap_member_ids.iter().cloned());
        for chosen in &self.chosen_contexts {
            source_ids.insert(chosen.context_id.clone());
            source_ids.insert(chosen.obligation_id.clone());
            source_ids.insert(chosen.claim_id.clone());
            source_ids.extend(chosen.context_member_ids.iter().cloned());
            source_ids.extend(chosen.claim_source_ids.iter().cloned());
            source_ids.extend(chosen.binding_ids.iter().cloned());
            source_ids.extend(chosen.evidence_ids.iter().cloned());
            source_ids.extend(chosen.verification_ids.iter().cloned());
            source_ids.extend(chosen.decision_ids.iter().cloned());
            source_ids.extend(chosen.finding_ids.iter().cloned());
        }
        if source_ids.len() != admission.count {
            return Err(M5Error::Validation(
                "runtime M5 profile source union changed after admission".into(),
            ));
        }
        Ok((source_ids, payment_qualifications))
    }
}

fn preflight_profile_source_union(
    sets: &[&BTreeSet<StableId>],
    fixed: &[&StableId],
) -> M5Result<UnionAdmission> {
    let mut admission = UnionAdmission {
        count: 0,
        retained_bytes: 0,
    };
    for (set_index, set) in sets.iter().enumerate() {
        for id in *set {
            if sets[..set_index].iter().any(|earlier| earlier.contains(id)) {
                continue;
            }
            admit_profile_source_id(&mut admission, id)?;
        }
    }
    for (index, id) in fixed.iter().enumerate() {
        if fixed[..index].contains(id) || sets.iter().any(|set| set.contains(*id)) {
            continue;
        }
        admit_profile_source_id(&mut admission, id)?;
    }
    Ok(admission)
}

fn admit_profile_source_id(admission: &mut UnionAdmission, id: &StableId) -> M5Result<()> {
    admission.count = admission.count.checked_add(1).ok_or(M5Error::Incomplete {
        operation: "runtime M5 profile source IDs",
        limit: MAX_M5_PROFILE_SOURCE_IDS,
        observed: usize::MAX,
    })?;
    bounded_len(
        admission.count,
        MAX_M5_PROFILE_SOURCE_IDS,
        "runtime M5 profile source IDs",
    )?;
    require_id_bytes(id, "runtime M5 profile source ID bytes")?;
    let retained = std::mem::size_of::<StableId>()
        .checked_add(id.as_str().len())
        .ok_or(M5Error::Incomplete {
            operation: "runtime M5 profile source retained bytes",
            limit: MAX_M5_PROFILE_SOURCE_RETAINED_BYTES,
            observed: usize::MAX,
        })?;
    admission.retained_bytes = admission
        .retained_bytes
        .checked_add(u64::try_from(retained).unwrap_or(u64::MAX))
        .ok_or(M5Error::Incomplete {
            operation: "runtime M5 profile source retained bytes",
            limit: MAX_M5_PROFILE_SOURCE_RETAINED_BYTES,
            observed: usize::MAX,
        })?;
    if admission.retained_bytes > MAX_M5_PROFILE_SOURCE_RETAINED_BYTES as u64 {
        return Err(M5Error::Incomplete {
            operation: "runtime M5 profile source retained bytes",
            limit: MAX_M5_PROFILE_SOURCE_RETAINED_BYTES,
            observed: usize::try_from(admission.retained_bytes).unwrap_or(usize::MAX),
        });
    }
    Ok(())
}

pub(crate) fn extend_runtime_profile_sources(
    source_ids: &mut BTreeSet<StableId>,
    existing_ids: &[&StableId],
) -> M5Result<()> {
    let admission = preflight_profile_source_union(&[source_ids], existing_ids)?;
    for id in existing_ids {
        source_ids.insert((*id).clone());
    }
    if source_ids.len() != admission.count {
        return Err(M5Error::Validation(
            "runtime M5 existing source union changed after admission".into(),
        ));
    }
    Ok(())
}

pub(crate) fn derive_registration_closure_v4<'a>(
    aggregate: &ReviewAggregate,
    run_id: &StableId,
    plan_id: &StableId,
    assessment_for: impl Fn(&StableId) -> Option<&'a ClaimAssessmentV3>,
) -> M5Result<M5RegistrationClosureV4> {
    let program = aggregate.program();
    let universe = aggregate.universe();
    if program.profile_key() != DOUBLE_SUBMIT_PROFILE_ID {
        return Err(M5Error::BindingMismatch {
            field: "M5 profile",
            expected: DOUBLE_SUBMIT_PROFILE_ID.to_owned(),
            actual: program.profile_key(),
        });
    }
    let plan = aggregate
        .review_plan(plan_id)
        .ok_or_else(|| M5Error::Validation("M5 plan is not current durable state".into()))?;
    require_same("plan snapshot", program.snapshot_id(), plan.snapshot_id())?;
    require_same("plan universe", universe.id(), plan.universe_id())?;

    let plan_obligation_ids = || plan.waves().iter().flat_map(|wave| wave.obligation_ids());
    let mut selected_count = 0_usize;
    for (position, obligation_id) in plan_obligation_ids().enumerate() {
        if plan_obligation_ids()
            .take(position)
            .any(|earlier| earlier == obligation_id)
        {
            continue;
        }
        let Some(obligation) = aggregate.obligation(obligation_id) else {
            continue;
        };
        if obligation.property_id() != DOUBLE_SUBMIT_PROPERTY_ID {
            continue;
        }
        require_id_bytes(obligation_id, "M5 selected obligation ID bytes")?;
        selected_count = selected_count.checked_add(1).ok_or(M5Error::Incomplete {
            operation: "M5 selected obligations",
            limit: MAX_M5_SELECTED_OBLIGATIONS,
            observed: usize::MAX,
        })?;
        bounded_len(
            selected_count,
            MAX_M5_SELECTED_OBLIGATIONS,
            "M5 selected obligations",
        )?;
    }
    if selected_count == 0 {
        return Err(M5Error::Empty {
            field: "selected payment obligations",
        });
    }
    let mut selected_obligations = Vec::with_capacity(selected_count);
    let mut selected_obligation_ids = BTreeSet::new();
    for obligation_id in plan_obligation_ids() {
        let Some(obligation) = aggregate.obligation(obligation_id) else {
            continue;
        };
        if obligation.property_id() == DOUBLE_SUBMIT_PROPERTY_ID
            && selected_obligation_ids.insert(obligation_id.clone())
        {
            selected_obligations.push(obligation);
        }
    }
    debug_assert_eq!(selected_obligations.len(), selected_count);
    if !selected_obligation_ids.is_subset(universe.obligation_ids()) {
        return Err(M5Error::Validation(
            "M5 selected obligations are outside the current universe".into(),
        ));
    }

    let mut invariants = program.invariants().iter().filter(|item| {
        item.id.as_str() == DOUBLE_SUBMIT_INVARIANT_ID
            && item.property_id == DOUBLE_SUBMIT_PROPERTY_ID
    });
    let Some(invariant) = invariants.next() else {
        return Err(M5Error::Validation(
            "M5 requires exactly one fixed payment invariant".into(),
        ));
    };
    if invariants.next().is_some() {
        return Err(M5Error::Validation(
            "M5 requires exactly one fixed payment invariant".into(),
        ));
    }

    let context_ids = required_contexts();
    let find_context = |id: &StableId| -> M5Result<_> {
        let mut found = program
            .contexts()
            .iter()
            .filter(|context| &context.id == id);
        let context = found.next().ok_or_else(|| {
            M5Error::Validation("M5 requires each fixed context exactly once".into())
        })?;
        if found.next().is_some() {
            return Err(M5Error::Validation(
                "M5 requires each fixed context exactly once".into(),
            ));
        }
        bounded_len(
            context.member_ids.len(),
            MAX_M5_CONTEXT_MEMBER_IDS,
            "M5 context members",
        )?;
        for member_id in &context.member_ids {
            require_id_bytes(member_id, "M5 context member ID bytes")?;
        }
        Ok(context)
    };
    let local_contexts = [
        find_context(&context_ids[0])?,
        find_context(&context_ids[1])?,
    ];
    let cover_domain_admission = preflight_mint_cover_domain(
        &invariant.scope_ids,
        &local_contexts[0].member_ids,
        &local_contexts[1].member_ids,
        &selected_obligations,
    )?;
    let overlap_count = local_contexts[0]
        .member_ids
        .intersection(&local_contexts[1].member_ids)
        .try_fold(0_usize, |count, id| {
            require_id_bytes(id, "M5 overlap ID bytes")?;
            let next = count.checked_add(1).ok_or(M5Error::Incomplete {
                operation: "M5 overlap_member_ids",
                limit: MAX_M5_OVERLAP_IDS,
                observed: usize::MAX,
            })?;
            bounded_len(next, MAX_M5_OVERLAP_IDS, "M5 overlap_member_ids")?;
            Ok::<usize, M5Error>(next)
        })?;
    let overlap_member_ids = local_contexts[0]
        .member_ids
        .intersection(&local_contexts[1].member_ids)
        .cloned()
        .collect::<BTreeSet<_>>();
    debug_assert_eq!(overlap_member_ids.len(), overlap_count);
    note_cover_domain_target_allocation();
    let mut obligation_domain = BTreeSet::new();
    for obligation in &selected_obligations {
        obligation_domain.extend(obligation.normalized_target_refs().iter().cloned());
        obligation_domain.extend(obligation.normalized_source_ids().iter().cloned());
    }
    let combined_domain_count = preflight_union(
        &[
            &invariant.scope_ids,
            &local_contexts[0].member_ids,
            &local_contexts[1].member_ids,
            &obligation_domain,
        ],
        &[],
        MAX_M5_COVER_DOMAIN_IDS,
        "M5 admitted cover domain",
    )?;
    if combined_domain_count != cover_domain_admission.count {
        return Err(M5Error::Validation(
            "M5 cover domain changed after borrowed admission".into(),
        ));
    }
    let cover = ContextCoverV4::derive(
        run_id.clone(),
        program.snapshot_id().clone(),
        universe.id().clone(),
        plan.id().clone(),
        selected_obligation_ids,
        invariant.scope_ids.clone(),
        local_contexts[0].member_ids.clone(),
        local_contexts[1].member_ids.clone(),
        obligation_domain,
    )?;

    let mut chosen_contexts = Vec::new();
    for context in local_contexts {
        let mut eligible = None;
        for claim in aggregate.execution_claims() {
            if claim.property_id() != DOUBLE_SUBMIT_PROPERTY_ID
                || claim.obligation_ids().len() != 1
                || claim.source_ids().is_disjoint(&context.member_ids)
            {
                continue;
            }
            let Some(obligation_id) = claim.obligation_ids().first() else {
                continue;
            };
            let Some(obligation) = selected_obligations
                .iter()
                .find(|item| item.id() == obligation_id)
                .copied()
            else {
                continue;
            };
            if !obligation.normalized_context_ids().contains(&context.id) {
                continue;
            }
            let Some(assessment) = exact_current_assessment_for(
                assessment_for(claim.id()),
                claim,
                run_id,
                program.snapshot_id(),
                universe.id(),
            ) else {
                continue;
            };
            let proof = CurrentVerificationProofV4::derive(
                assessment,
                claim,
                obligation.id(),
                program.snapshot_id(),
            )?;
            if eligible.is_some() {
                return Err(M5Error::Validation(
                    "M5 Section eligibility is ambiguous".into(),
                ));
            }
            eligible = Some((claim, obligation, assessment, proof));
        }
        let Some((claim, obligation, assessment, proof)) = eligible else {
            continue;
        };
        chosen_contexts.push(M5ChosenContextClosureV4 {
            context_id: context.id.clone(),
            context_member_ids: context.member_ids.clone(),
            obligation_id: obligation.id().clone(),
            claim_id: claim.id().clone(),
            claim_source_ids: claim.source_ids().clone(),
            binding_ids: assessment.binding_ids().iter().cloned().collect(),
            evidence_ids: assessment.evidence_ids().iter().cloned().collect(),
            verification_ids: assessment.verification_ids().iter().cloned().collect(),
            decision_ids: assessment.decision_ids().iter().cloned().collect(),
            finding_ids: assessment.finding_ids().iter().cloned().collect(),
            verification_passed: proof.passed,
        });
    }
    Ok(M5RegistrationClosureV4 {
        cover,
        overlap_member_ids,
        chosen_contexts,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GluingInputDescriptorV4 {
    schema: String,
    id: StableId,
    run_id: StableId,
    snapshot_id: StableId,
    universe_id: StableId,
    plan_id: StableId,
    profile_descriptor_id: String,
    context_id: StableId,
    assignment_key: String,
    assignment_value: AssignmentValueV4,
    qualification_source_ids: BTreeSet<StableId>,
}

impl GluingInputDescriptorV4 {
    pub(crate) fn retained_bytes_for_v5(&self) -> u64 {
        let mut total = u64::try_from(std::mem::size_of::<Self>()).unwrap_or(u64::MAX);
        for bytes in [
            self.schema.capacity(),
            self.id.allocated_bytes(),
            self.run_id.allocated_bytes(),
            self.snapshot_id.allocated_bytes(),
            self.universe_id.allocated_bytes(),
            self.plan_id.allocated_bytes(),
            self.profile_descriptor_id.capacity(),
            self.context_id.allocated_bytes(),
            self.assignment_key.capacity(),
            retained_id_set_bytes(&self.qualification_source_ids),
        ] {
            total = total.saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
        }
        total
    }
    pub(crate) fn complete_body_hash(&self) -> crate::Result<ContentHash> {
        Ok(ContentHash::sha256(&crate::canonical_json(self)?))
    }
    pub fn new(
        run_id: StableId,
        snapshot_id: StableId,
        universe_id: StableId,
        plan_id: StableId,
        context_id: StableId,
        assignment_value: AssignmentValueV4,
        qualification_source_ids: BTreeSet<StableId>,
    ) -> M5Result<Self> {
        if context_id != fixed_id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID)
            && context_id != fixed_id(DOUBLE_SUBMIT_UI_CONTEXT_ID)
        {
            return Err(M5Error::Validation(
                "descriptor context is outside the fixed cover".into(),
            ));
        }
        bounded(
            &qualification_source_ids,
            MAX_M5_DESCRIPTOR_QUALIFICATION_IDS,
            "M5 descriptor qualification_source_ids",
        )?;
        for id in [&run_id, &snapshot_id, &universe_id, &plan_id, &context_id]
            .into_iter()
            .chain(qualification_source_ids.iter())
        {
            require_id_bytes(id, "M5 descriptor input StableId bytes")?;
        }
        let profile_descriptor_id = DOUBLE_SUBMIT_GLUING_DESCRIPTOR_ID.to_owned();
        let assignment_key = DOUBLE_SUBMIT_ASSIGNMENT_KEY.to_owned();
        let id = identity(
            "gluing-input-descriptor-v4",
            &DescriptorIdentity {
                assignment_key: &assignment_key,
                assignment_value,
                context_id: &context_id,
                plan_id: &plan_id,
                profile_descriptor_id: &profile_descriptor_id,
                qualification_source_ids: &qualification_source_ids,
                run_id: &run_id,
                snapshot_id: &snapshot_id,
                universe_id: &universe_id,
            },
        )?;
        let record = Self {
            schema: "reviewgraphen.gluing_input_descriptor.v4".to_owned(),
            id,
            run_id,
            snapshot_id,
            universe_id,
            plan_id,
            profile_descriptor_id,
            context_id,
            assignment_key,
            assignment_value,
            qualification_source_ids,
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> M5Result<()> {
        if self.schema != "reviewgraphen.gluing_input_descriptor.v4"
            || self.profile_descriptor_id != DOUBLE_SUBMIT_GLUING_DESCRIPTOR_ID
            || self.assignment_key != DOUBLE_SUBMIT_ASSIGNMENT_KEY
            || (self.context_id != fixed_id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID)
                && self.context_id != fixed_id(DOUBLE_SUBMIT_UI_CONTEXT_ID))
        {
            return Err(M5Error::Validation(
                "invalid fixed M5 descriptor fields".into(),
            ));
        }
        bounded(
            &self.qualification_source_ids,
            MAX_M5_DESCRIPTOR_QUALIFICATION_IDS,
            "M5 descriptor qualification_source_ids",
        )?;
        for id in [
            &self.id,
            &self.run_id,
            &self.snapshot_id,
            &self.universe_id,
            &self.plan_id,
            &self.context_id,
        ]
        .into_iter()
        .chain(self.qualification_source_ids.iter())
        {
            require_id_bytes(id, "M5 descriptor StableId bytes")?;
        }
        let expected = identity(
            "gluing-input-descriptor-v4",
            &DescriptorIdentity {
                assignment_key: &self.assignment_key,
                assignment_value: self.assignment_value,
                context_id: &self.context_id,
                plan_id: &self.plan_id,
                profile_descriptor_id: &self.profile_descriptor_id,
                qualification_source_ids: &self.qualification_source_ids,
                run_id: &self.run_id,
                snapshot_id: &self.snapshot_id,
                universe_id: &self.universe_id,
            },
        )?;
        require_same("descriptor derived ID", &expected, &self.id)?;
        bounded_bytes(
            self,
            MAX_M5_DESCRIPTOR_CANONICAL_BYTES,
            "M5 canonical descriptor bytes",
        )
    }

    pub fn from_json_bytes(input: &[u8]) -> M5Result<Self> {
        preflight_wire_json(
            input,
            MAX_M5_DESCRIPTOR_CANONICAL_BYTES,
            "M5 descriptor JSON",
            WireShape::Descriptor,
        )?;
        let wire: GluingInputDescriptorWire = serde_json::from_slice(input)
            .map_err(|error| M5Error::Validation(format!("M5 descriptor JSON: {error}")))?;
        let value = Self {
            schema: wire.schema,
            id: wire.id,
            run_id: wire.run_id,
            snapshot_id: wire.snapshot_id,
            universe_id: wire.universe_id,
            plan_id: wire.plan_id,
            profile_descriptor_id: wire.profile_descriptor_id,
            context_id: wire.context_id,
            assignment_key: wire.assignment_key,
            assignment_value: wire.assignment_value.into(),
            qualification_source_ids: wire.qualification_source_ids,
        };
        value.validate()?;
        if crate::canonical_json(&value)? != input {
            return Err(M5Error::Validation(
                "M5 descriptor JSON is not exact canonical JSON".into(),
            ));
        }
        Ok(value)
    }

    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn run_id(&self) -> &StableId {
        &self.run_id
    }
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    pub fn universe_id(&self) -> &StableId {
        &self.universe_id
    }
    pub fn plan_id(&self) -> &StableId {
        &self.plan_id
    }
    pub fn context_id(&self) -> &StableId {
        &self.context_id
    }
    pub const fn assignment_value(&self) -> AssignmentValueV4 {
        self.assignment_value
    }
    pub fn qualification_source_ids(&self) -> &BTreeSet<StableId> {
        &self.qualification_source_ids
    }
    pub(crate) fn projection_schema(&self) -> &str {
        &self.schema
    }
    pub(crate) fn projection_profile_descriptor_id(&self) -> &str {
        &self.profile_descriptor_id
    }
    pub(crate) fn projection_assignment_key(&self) -> &str {
        &self.assignment_key
    }
}

/// Exact post-registration input closure. Event-v4 integration is the only
/// production caller allowed to mint this non-serializable, non-clone token.
pub(crate) struct RegisteredGluingInputV4 {
    descriptor: GluingInputDescriptorV4,
    registration_id: StableId,
}

impl RegisteredGluingInputV4 {
    pub(crate) fn seal(
        descriptor: GluingInputDescriptorV4,
        registration_id: StableId,
    ) -> M5Result<Self> {
        if registration_id.kind() != "registration-v4" {
            return Err(M5Error::Validation(
                "M5 registered input requires registration-v4".into(),
            ));
        }
        Ok(Self {
            descriptor,
            registration_id,
        })
    }
}

/// Opaque current-state capability. It owns the already-derived bundle so no
/// later caller can replace members, overlap, trace IDs, or verification state.
pub(crate) struct ValidatedM5SourceV4 {
    bundle: GluingBundleV4,
}

impl ValidatedM5SourceV4 {
    fn derive_bundle(&self) -> M5Result<GluingBundleV4> {
        self.bundle.validate()?;
        Ok(self.bundle.clone())
    }

    pub(crate) fn mint_from_v4_replay<'a>(
        aggregate: &ReviewAggregate,
        run_id: &StableId,
        assessment_for: impl Fn(&StableId) -> Option<&'a ClaimAssessmentV3>,
        registered_inputs: [RegisteredGluingInputV4; 2],
    ) -> M5Result<Self> {
        let program = aggregate.program();
        let universe = aggregate.universe();
        let contexts = required_contexts();
        for (index, input) in registered_inputs.iter().enumerate() {
            require_same(
                "registered descriptor context",
                &contexts[index],
                input.descriptor.context_id(),
            )?;
            require_same(
                "registered descriptor run",
                run_id,
                input.descriptor.run_id(),
            )?;
            require_same(
                "registered descriptor snapshot",
                program.snapshot_id(),
                input.descriptor.snapshot_id(),
            )?;
            require_same(
                "registered descriptor universe",
                universe.id(),
                input.descriptor.universe_id(),
            )?;
        }
        let plan_id = registered_inputs[0].descriptor.plan_id().clone();
        require_same(
            "descriptor plan",
            &plan_id,
            registered_inputs[1].descriptor.plan_id(),
        )?;
        let closure = derive_registration_closure_v4(aggregate, run_id, &plan_id, assessment_for)?;
        let M5RegistrationClosureV4 {
            cover,
            overlap_member_ids,
            chosen_contexts,
        } = closure;
        let mut sections = Vec::with_capacity(chosen_contexts.len());
        for chosen in chosen_contexts {
            let index = contexts
                .iter()
                .position(|context_id| context_id == &chosen.context_id)
                .ok_or_else(|| {
                    M5Error::Validation("M5 chosen context is outside the fixed cover".into())
                })?;
            let input = &registered_inputs[index];
            sections.push(SectionV4::derive_with_verification_state(
                &cover,
                &input.descriptor,
                input.registration_id.clone(),
                chosen.obligation_id,
                chosen.claim_id,
                CurrentVerificationProofV4 {
                    passed: chosen.verification_passed,
                },
                &chosen.context_member_ids,
                chosen.claim_source_ids,
                chosen.binding_ids,
                chosen.evidence_ids,
                chosen.verification_ids,
                chosen.decision_ids,
                chosen.finding_ids,
            )?);
        }
        validate_context_qualifications(
            &cover,
            [
                &registered_inputs[0].descriptor,
                &registered_inputs[1].descriptor,
            ],
            &sections,
        )?;
        let [left, right] = registered_inputs;
        let registration_ids = [left.registration_id.clone(), right.registration_id.clone()];
        let descriptors = [left.descriptor, right.descriptor];
        let bundle = GluingBundleV4::derive(
            cover,
            descriptors,
            registration_ids,
            sections,
            overlap_member_ids,
        )?;
        Ok(Self { bundle })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextCoverV4 {
    schema: String,
    id: StableId,
    run_id: StableId,
    snapshot_id: StableId,
    universe_id: StableId,
    plan_id: StableId,
    profile_descriptor_id: String,
    selected_obligation_ids: BTreeSet<StableId>,
    required_context_ids: Vec<StableId>,
    cover_domain_ids: BTreeSet<StableId>,
    covered_domain_ids: BTreeSet<StableId>,
    uncovered_domain_ids: BTreeSet<StableId>,
    source_ids: BTreeSet<StableId>,
}

impl ContextCoverV4 {
    pub(crate) fn complete_body_hash(&self) -> crate::Result<ContentHash> {
        Ok(ContentHash::sha256(&crate::canonical_json(self)?))
    }
    #[allow(clippy::too_many_arguments)]
    fn derive(
        run_id: StableId,
        snapshot_id: StableId,
        universe_id: StableId,
        plan_id: StableId,
        selected_obligation_ids: BTreeSet<StableId>,
        invariant_scope_ids: BTreeSet<StableId>,
        payment_member_ids: BTreeSet<StableId>,
        ui_event_member_ids: BTreeSet<StableId>,
        selected_obligation_target_and_source_ids: BTreeSet<StableId>,
    ) -> M5Result<Self> {
        if selected_obligation_ids.is_empty() {
            return Err(M5Error::Empty {
                field: "selected_obligation_ids",
            });
        }
        bounded(
            &selected_obligation_ids,
            MAX_M5_SELECTED_OBLIGATIONS,
            "M5 selected obligations",
        )?;
        bounded(
            &payment_member_ids,
            MAX_M5_CONTEXT_MEMBER_IDS,
            "M5 payment context members",
        )?;
        bounded(
            &ui_event_member_ids,
            MAX_M5_CONTEXT_MEMBER_IDS,
            "M5 ui-event context members",
        )?;
        for id in [&run_id, &snapshot_id, &universe_id, &plan_id]
            .into_iter()
            .chain(selected_obligation_ids.iter())
            .chain(invariant_scope_ids.iter())
            .chain(payment_member_ids.iter())
            .chain(ui_event_member_ids.iter())
            .chain(selected_obligation_target_and_source_ids.iter())
        {
            require_id_bytes(id, "M5 cover input StableId bytes")?;
        }
        let required_context_ids = vec![
            fixed_id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
            fixed_id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
        ];
        let invariant_id = fixed_id(DOUBLE_SUBMIT_INVARIANT_ID);
        let exact_domain_count = preflight_union(
            &[
                &invariant_scope_ids,
                &payment_member_ids,
                &ui_event_member_ids,
                &selected_obligation_target_and_source_ids,
            ],
            &[],
            MAX_M5_COVER_DOMAIN_IDS,
            "M5 cover_domain_ids",
        )?;
        note_cover_domain_target_allocation();
        let cover_domain_ids = union([
            &invariant_scope_ids,
            &payment_member_ids,
            &ui_event_member_ids,
            &selected_obligation_target_and_source_ids,
        ]);
        debug_assert_eq!(cover_domain_ids.len(), exact_domain_count);
        bounded(
            &cover_domain_ids,
            MAX_M5_COVER_DOMAIN_IDS,
            "M5 cover_domain_ids",
        )?;
        let mut known = union([&payment_member_ids, &ui_event_member_ids]);
        known.extend(required_context_ids.iter().cloned());
        known.insert(invariant_id.clone());
        let covered_domain_ids = cover_domain_ids
            .intersection(&known)
            .cloned()
            .collect::<BTreeSet<_>>();
        let uncovered_domain_ids = cover_domain_ids
            .difference(&known)
            .cloned()
            .collect::<BTreeSet<_>>();
        let exact_source_count = preflight_union(
            &[&cover_domain_ids],
            &[
                &required_context_ids[0],
                &required_context_ids[1],
                &invariant_id,
            ],
            MAX_M5_COVER_SOURCE_IDS,
            "M5 cover source_ids",
        )?;
        let mut source_ids = cover_domain_ids.clone();
        source_ids.extend(required_context_ids.iter().cloned());
        source_ids.insert(invariant_id);
        debug_assert_eq!(source_ids.len(), exact_source_count);
        bounded(&source_ids, MAX_M5_COVER_SOURCE_IDS, "M5 cover source_ids")?;
        let profile_descriptor_id = DOUBLE_SUBMIT_GLUING_DESCRIPTOR_ID.to_owned();
        let id = identity(
            "context-cover-v4",
            &CoverIdentity {
                cover_domain_ids: &cover_domain_ids,
                plan_id: &plan_id,
                profile_descriptor_id: &profile_descriptor_id,
                required_context_ids: &required_context_ids,
                run_id: &run_id,
                selected_obligation_ids: &selected_obligation_ids,
                snapshot_id: &snapshot_id,
                universe_id: &universe_id,
            },
        )?;
        let record = Self {
            schema: "reviewgraphen.context_cover.v4".to_owned(),
            id,
            run_id,
            snapshot_id,
            universe_id,
            plan_id,
            profile_descriptor_id,
            selected_obligation_ids,
            required_context_ids,
            cover_domain_ids,
            covered_domain_ids,
            uncovered_domain_ids,
            source_ids,
        };
        record.validate()?;
        Ok(record)
    }
    fn validate(&self) -> M5Result<()> {
        if self.schema != "reviewgraphen.context_cover.v4"
            || self.profile_descriptor_id != DOUBLE_SUBMIT_GLUING_DESCRIPTOR_ID
            || self.required_context_ids != required_contexts()
            || self.selected_obligation_ids.is_empty()
        {
            return Err(M5Error::Validation("invalid M5 cover fixed fields".into()));
        }
        bounded(
            &self.selected_obligation_ids,
            MAX_M5_SELECTED_OBLIGATIONS,
            "M5 selected obligations",
        )?;
        bounded(
            &self.cover_domain_ids,
            MAX_M5_COVER_DOMAIN_IDS,
            "M5 cover_domain_ids",
        )?;
        bounded(
            &self.covered_domain_ids,
            MAX_M5_COVER_DOMAIN_IDS,
            "M5 covered_domain_ids",
        )?;
        bounded(
            &self.uncovered_domain_ids,
            MAX_M5_COVER_DOMAIN_IDS,
            "M5 uncovered_domain_ids",
        )?;
        bounded(
            &self.source_ids,
            MAX_M5_COVER_SOURCE_IDS,
            "M5 cover source_ids",
        )?;
        for id in [
            &self.id,
            &self.run_id,
            &self.snapshot_id,
            &self.universe_id,
            &self.plan_id,
        ]
        .into_iter()
        .chain(self.selected_obligation_ids.iter())
        .chain(self.required_context_ids.iter())
        .chain(self.cover_domain_ids.iter())
        .chain(self.covered_domain_ids.iter())
        .chain(self.uncovered_domain_ids.iter())
        .chain(self.source_ids.iter())
        {
            require_id_bytes(id, "M5 cover StableId bytes")?;
        }
        if !self
            .covered_domain_ids
            .is_disjoint(&self.uncovered_domain_ids)
            || self
                .covered_domain_ids
                .union(&self.uncovered_domain_ids)
                .cloned()
                .collect::<BTreeSet<_>>()
                != self.cover_domain_ids
        {
            return Err(M5Error::Validation("invalid M5 cover partition".into()));
        }
        let mut expected_sources = self.cover_domain_ids.clone();
        expected_sources.extend(required_contexts());
        expected_sources.insert(fixed_id(DOUBLE_SUBMIT_INVARIANT_ID));
        if self.source_ids != expected_sources {
            return Err(M5Error::Validation(
                "invalid M5 cover source closure".into(),
            ));
        }
        let expected = identity(
            "context-cover-v4",
            &CoverIdentity {
                cover_domain_ids: &self.cover_domain_ids,
                plan_id: &self.plan_id,
                profile_descriptor_id: &self.profile_descriptor_id,
                required_context_ids: &self.required_context_ids,
                run_id: &self.run_id,
                selected_obligation_ids: &self.selected_obligation_ids,
                snapshot_id: &self.snapshot_id,
                universe_id: &self.universe_id,
            },
        )?;
        require_same("cover derived ID", &expected, &self.id)
    }
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn run_id(&self) -> &StableId {
        &self.run_id
    }
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    pub fn universe_id(&self) -> &StableId {
        &self.universe_id
    }
    pub fn plan_id(&self) -> &StableId {
        &self.plan_id
    }
    pub fn selected_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.selected_obligation_ids
    }
    pub fn cover_domain_ids(&self) -> &BTreeSet<StableId> {
        &self.cover_domain_ids
    }
    pub fn covered_domain_ids(&self) -> &BTreeSet<StableId> {
        &self.covered_domain_ids
    }
    pub fn uncovered_domain_ids(&self) -> &BTreeSet<StableId> {
        &self.uncovered_domain_ids
    }
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub(crate) fn projection_schema(&self) -> &str {
        &self.schema
    }
    pub(crate) fn projection_profile_descriptor_id(&self) -> &str {
        &self.profile_descriptor_id
    }
    pub(crate) fn projection_required_context_ids(&self) -> &[StableId] {
        &self.required_context_ids
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SectionV4 {
    schema: String,
    id: StableId,
    cover_id: StableId,
    context_id: StableId,
    snapshot_id: StableId,
    property_id: String,
    invariant_id: StableId,
    obligation_id: StableId,
    claim_id: StableId,
    claim_assessment_id: StableId,
    input_descriptor_id: StableId,
    input_registration_id: StableId,
    assignment_key: String,
    assignment_value: AssignmentValueV4,
    passed_current_verification: bool,
    source_ids: BTreeSet<StableId>,
    qualification_source_ids: BTreeSet<StableId>,
    binding_ids: BTreeSet<StableId>,
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_ids: BTreeSet<StableId>,
    finding_ids: BTreeSet<StableId>,
}

impl SectionV4 {
    pub(crate) fn complete_body_hash(&self) -> crate::Result<ContentHash> {
        Ok(ContentHash::sha256(&crate::canonical_json(self)?))
    }
    #[allow(clippy::too_many_arguments)]
    fn derive_unverified(
        cover: &ContextCoverV4,
        descriptor: &GluingInputDescriptorV4,
        input_registration_id: StableId,
        obligation_id: StableId,
        claim_id: StableId,
        context_member_ids: &BTreeSet<StableId>,
        claim_source_ids: BTreeSet<StableId>,
        binding_ids: BTreeSet<StableId>,
        evidence_ids: BTreeSet<StableId>,
        verification_ids: BTreeSet<StableId>,
        decision_ids: BTreeSet<StableId>,
        finding_ids: BTreeSet<StableId>,
    ) -> M5Result<Self> {
        Self::derive_with_verification_state(
            cover,
            descriptor,
            input_registration_id,
            obligation_id,
            claim_id,
            CurrentVerificationProofV4 { passed: false },
            context_member_ids,
            claim_source_ids,
            binding_ids,
            evidence_ids,
            verification_ids,
            decision_ids,
            finding_ids,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn derive_with_verification_state(
        cover: &ContextCoverV4,
        descriptor: &GluingInputDescriptorV4,
        input_registration_id: StableId,
        obligation_id: StableId,
        claim_id: StableId,
        verification_proof: CurrentVerificationProofV4,
        context_member_ids: &BTreeSet<StableId>,
        claim_source_ids: BTreeSet<StableId>,
        binding_ids: BTreeSet<StableId>,
        evidence_ids: BTreeSet<StableId>,
        verification_ids: BTreeSet<StableId>,
        decision_ids: BTreeSet<StableId>,
        finding_ids: BTreeSet<StableId>,
    ) -> M5Result<Self> {
        if cover.snapshot_id() != descriptor.snapshot_id() {
            return Err(M5Error::SnapshotMismatch {
                expected: cover.snapshot_id().clone(),
                actual: descriptor.snapshot_id().clone(),
            });
        }
        require_same("descriptor run_id", cover.run_id(), descriptor.run_id())?;
        require_same(
            "descriptor universe_id",
            cover.universe_id(),
            descriptor.universe_id(),
        )?;
        require_same("descriptor plan_id", cover.plan_id(), descriptor.plan_id())?;
        if !cover.selected_obligation_ids.contains(&obligation_id) {
            return Err(M5Error::Validation(
                "section obligation is not selected by the cover".into(),
            ));
        }
        bounded(
            &claim_source_ids,
            MAX_M5_CLAIM_SOURCE_IDS,
            "M5 claim source_ids retained per Section",
        )?;
        for (set, operation) in [
            (&binding_ids, "M5 Section binding_ids"),
            (&evidence_ids, "M5 Section evidence_ids"),
            (&verification_ids, "M5 Section verification_ids"),
            (&decision_ids, "M5 Section decision_ids"),
            (&finding_ids, "M5 Section finding_ids"),
        ] {
            bounded(set, MAX_M5_SECTION_TRACE_IDS, operation)?;
        }
        for id in [&input_registration_id, &obligation_id, &claim_id]
            .into_iter()
            .chain(context_member_ids.iter())
            .chain(claim_source_ids.iter())
            .chain(binding_ids.iter())
            .chain(evidence_ids.iter())
            .chain(verification_ids.iter())
            .chain(decision_ids.iter())
            .chain(finding_ids.iter())
        {
            require_id_bytes(id, "M5 Section input StableId bytes")?;
        }
        let retained_sources = claim_source_ids
            .intersection(context_member_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        let context_id = descriptor.context_id.clone();
        let claim_assessment_id = claim_id.clone();
        let invariant_id = fixed_id(DOUBLE_SUBMIT_INVARIANT_ID);
        let property_id = DOUBLE_SUBMIT_PROPERTY_ID.to_owned();
        let assignment_key = DOUBLE_SUBMIT_ASSIGNMENT_KEY.to_owned();
        let assignment_value = descriptor.assignment_value;
        let qualification_source_ids = descriptor.qualification_source_ids.clone();
        let id = identity(
            "section-v4",
            &SectionIdentity {
                assignment_key: &assignment_key,
                assignment_value,
                claim_id: &claim_id,
                context_id: &context_id,
                cover_id: &cover.id,
                input_descriptor_id: &descriptor.id,
                input_registration_id: &input_registration_id,
                invariant_id: &invariant_id,
                obligation_id: &obligation_id,
                property_id: &property_id,
                qualification_source_ids: &qualification_source_ids,
                snapshot_id: &cover.snapshot_id,
            },
        )?;
        let fixed_sources = [
            &cover.id,
            &context_id,
            &invariant_id,
            &obligation_id,
            &claim_id,
            &descriptor.id,
            &input_registration_id,
        ];
        let exact_source_count = preflight_union(
            &[
                &retained_sources,
                &binding_ids,
                &evidence_ids,
                &verification_ids,
                &decision_ids,
                &finding_ids,
            ],
            &fixed_sources,
            MAX_M5_SECTION_SOURCE_IDS,
            "M5 Section source_ids",
        )?;
        let mut source_ids = fixed_sources
            .iter()
            .map(|id| (*id).clone())
            .collect::<BTreeSet<_>>();
        for set in [
            &retained_sources,
            &binding_ids,
            &evidence_ids,
            &verification_ids,
            &decision_ids,
            &finding_ids,
        ] {
            source_ids.extend(set.iter().cloned());
        }
        debug_assert_eq!(source_ids.len(), exact_source_count);
        bounded(
            &source_ids,
            MAX_M5_SECTION_SOURCE_IDS,
            "M5 Section source_ids",
        )?;
        let record = Self {
            schema: "reviewgraphen.section.v4".to_owned(),
            id,
            cover_id: cover.id.clone(),
            context_id,
            snapshot_id: cover.snapshot_id.clone(),
            property_id,
            invariant_id,
            obligation_id,
            claim_id,
            claim_assessment_id,
            input_descriptor_id: descriptor.id.clone(),
            input_registration_id,
            assignment_key,
            assignment_value,
            passed_current_verification: verification_proof.passed,
            source_ids,
            qualification_source_ids,
            binding_ids,
            evidence_ids,
            verification_ids,
            decision_ids,
            finding_ids,
        };
        record.validate()?;
        Ok(record)
    }
    fn validate(&self) -> M5Result<()> {
        if self.schema != "reviewgraphen.section.v4"
            || self.property_id != DOUBLE_SUBMIT_PROPERTY_ID
            || self.invariant_id != fixed_id(DOUBLE_SUBMIT_INVARIANT_ID)
            || self.assignment_key != DOUBLE_SUBMIT_ASSIGNMENT_KEY
            || self.claim_assessment_id != self.claim_id
        {
            return Err(M5Error::Validation(
                "invalid M5 Section fixed fields".into(),
            ));
        }
        bounded(
            &self.qualification_source_ids,
            MAX_M5_DESCRIPTOR_QUALIFICATION_IDS,
            "M5 Section qualification_source_ids",
        )?;
        for (set, operation) in [
            (&self.binding_ids, "M5 Section binding_ids"),
            (&self.evidence_ids, "M5 Section evidence_ids"),
            (&self.verification_ids, "M5 Section verification_ids"),
            (&self.decision_ids, "M5 Section decision_ids"),
            (&self.finding_ids, "M5 Section finding_ids"),
        ] {
            bounded(set, MAX_M5_SECTION_TRACE_IDS, operation)?;
        }
        bounded(
            &self.source_ids,
            MAX_M5_SECTION_SOURCE_IDS,
            "M5 Section source_ids",
        )?;
        for id in [
            &self.id,
            &self.cover_id,
            &self.context_id,
            &self.snapshot_id,
            &self.invariant_id,
            &self.obligation_id,
            &self.claim_id,
            &self.claim_assessment_id,
            &self.input_descriptor_id,
            &self.input_registration_id,
        ]
        .into_iter()
        .chain(self.source_ids.iter())
        .chain(self.qualification_source_ids.iter())
        .chain(self.binding_ids.iter())
        .chain(self.evidence_ids.iter())
        .chain(self.verification_ids.iter())
        .chain(self.decision_ids.iter())
        .chain(self.finding_ids.iter())
        {
            require_id_bytes(id, "M5 Section StableId bytes")?;
        }
        let required = BTreeSet::from([
            self.cover_id.clone(),
            self.context_id.clone(),
            self.invariant_id.clone(),
            self.obligation_id.clone(),
            self.claim_id.clone(),
            self.input_descriptor_id.clone(),
            self.input_registration_id.clone(),
        ]);
        if !required.is_subset(&self.source_ids)
            || [
                &self.binding_ids,
                &self.evidence_ids,
                &self.verification_ids,
                &self.decision_ids,
                &self.finding_ids,
            ]
            .iter()
            .any(|set| !set.is_subset(&self.source_ids))
        {
            return Err(M5Error::Validation(
                "invalid M5 Section source closure".into(),
            ));
        }
        let expected = identity(
            "section-v4",
            &SectionIdentity {
                assignment_key: &self.assignment_key,
                assignment_value: self.assignment_value,
                claim_id: &self.claim_id,
                context_id: &self.context_id,
                cover_id: &self.cover_id,
                input_descriptor_id: &self.input_descriptor_id,
                input_registration_id: &self.input_registration_id,
                invariant_id: &self.invariant_id,
                obligation_id: &self.obligation_id,
                property_id: &self.property_id,
                qualification_source_ids: &self.qualification_source_ids,
                snapshot_id: &self.snapshot_id,
            },
        )?;
        require_same("Section derived ID", &expected, &self.id)
    }
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn context_id(&self) -> &StableId {
        &self.context_id
    }
    pub const fn assignment_value(&self) -> AssignmentValueV4 {
        self.assignment_value
    }
    pub const fn passed_current_verification(&self) -> bool {
        self.passed_current_verification
    }
    pub(crate) fn projection_schema(&self) -> &str {
        &self.schema
    }
    pub(crate) fn projection_cover_id(&self) -> &StableId {
        &self.cover_id
    }
    pub(crate) fn projection_snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    pub(crate) fn projection_property_id(&self) -> &str {
        &self.property_id
    }
    pub(crate) fn projection_invariant_id(&self) -> &StableId {
        &self.invariant_id
    }
    pub(crate) fn projection_obligation_id(&self) -> &StableId {
        &self.obligation_id
    }
    pub(crate) fn projection_claim_id(&self) -> &StableId {
        &self.claim_id
    }
    pub(crate) fn projection_claim_assessment_id(&self) -> &StableId {
        &self.claim_assessment_id
    }
    pub(crate) fn projection_input_descriptor_id(&self) -> &StableId {
        &self.input_descriptor_id
    }
    pub(crate) fn projection_input_registration_id(&self) -> &StableId {
        &self.input_registration_id
    }
    pub(crate) fn projection_assignment_key(&self) -> &str {
        &self.assignment_key
    }
    pub(crate) fn projection_source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub(crate) fn projection_qualification_source_ids(&self) -> &BTreeSet<StableId> {
        &self.qualification_source_ids
    }
    pub(crate) fn projection_binding_ids(&self) -> &BTreeSet<StableId> {
        &self.binding_ids
    }
    pub(crate) fn projection_evidence_ids(&self) -> &BTreeSet<StableId> {
        &self.evidence_ids
    }
    pub(crate) fn projection_verification_ids(&self) -> &BTreeSet<StableId> {
        &self.verification_ids
    }
    pub(crate) fn projection_decision_ids(&self) -> &BTreeSet<StableId> {
        &self.decision_ids
    }
    pub(crate) fn projection_finding_ids(&self) -> &BTreeSet<StableId> {
        &self.finding_ids
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestrictionV4 {
    schema: String,
    id: StableId,
    section_id: StableId,
    context_pair: Vec<StableId>,
    overlap_member_ids: BTreeSet<StableId>,
    assignment_key: String,
    assignment_value: AssignmentValueV4,
    source_ids: BTreeSet<StableId>,
    qualification_source_ids: BTreeSet<StableId>,
    claim_ids: BTreeSet<StableId>,
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_ids: BTreeSet<StableId>,
    finding_ids: BTreeSet<StableId>,
}

impl RestrictionV4 {
    pub(crate) fn complete_body_hash(&self) -> crate::Result<ContentHash> {
        Ok(ContentHash::sha256(&crate::canonical_json(self)?))
    }
    fn derive(section: &SectionV4, overlap_member_ids: &BTreeSet<StableId>) -> M5Result<Self> {
        bounded(
            overlap_member_ids,
            MAX_M5_OVERLAP_IDS,
            "M5 restriction overlap_member_ids",
        )?;
        for id in overlap_member_ids {
            require_id_bytes(id, "M5 Restriction overlap StableId bytes")?;
        }
        let context_pair = required_contexts();
        let id = identity(
            "restriction-v4",
            &RestrictionIdentity {
                assignment_key: &section.assignment_key,
                assignment_value: section.assignment_value,
                context_pair: &context_pair,
                overlap_member_ids,
                qualification_source_ids: &section.qualification_source_ids,
                section_id: &section.id,
            },
        )?;
        let claim_ids = BTreeSet::from([section.claim_id.clone()]);
        let fixed_sources = [
            &section.id,
            &section.context_id,
            &section.cover_id,
            &section.invariant_id,
            &section.obligation_id,
            &section.claim_id,
            &section.input_descriptor_id,
            &section.input_registration_id,
        ];
        let exact_source_count = preflight_union(
            &[
                overlap_member_ids,
                &section.binding_ids,
                &section.evidence_ids,
                &section.verification_ids,
                &section.decision_ids,
                &section.finding_ids,
            ],
            &fixed_sources,
            MAX_M5_RESTRICTION_SOURCE_IDS,
            "M5 Restriction source_ids",
        )?;
        let mut source_ids = fixed_sources
            .iter()
            .map(|id| (*id).clone())
            .collect::<BTreeSet<_>>();
        for set in [
            overlap_member_ids,
            &section.binding_ids,
            &section.evidence_ids,
            &section.verification_ids,
            &section.decision_ids,
            &section.finding_ids,
        ] {
            source_ids.extend(set.iter().cloned());
        }
        debug_assert_eq!(source_ids.len(), exact_source_count);
        bounded(
            &source_ids,
            MAX_M5_RESTRICTION_SOURCE_IDS,
            "M5 Restriction source_ids",
        )?;
        let record = Self {
            schema: "reviewgraphen.restriction.v4".to_owned(),
            id,
            section_id: section.id.clone(),
            context_pair,
            overlap_member_ids: overlap_member_ids.clone(),
            assignment_key: section.assignment_key.clone(),
            assignment_value: section.assignment_value,
            source_ids,
            qualification_source_ids: section.qualification_source_ids.clone(),
            claim_ids,
            evidence_ids: section.evidence_ids.clone(),
            verification_ids: section.verification_ids.clone(),
            decision_ids: section.decision_ids.clone(),
            finding_ids: section.finding_ids.clone(),
        };
        record.validate()?;
        Ok(record)
    }
    fn validate(&self) -> M5Result<()> {
        if self.schema != "reviewgraphen.restriction.v4"
            || self.context_pair != required_contexts()
            || self.assignment_key != DOUBLE_SUBMIT_ASSIGNMENT_KEY
            || self.claim_ids.len() != 1
        {
            return Err(M5Error::Validation(
                "invalid M5 Restriction fixed fields".into(),
            ));
        }
        bounded(
            &self.overlap_member_ids,
            MAX_M5_OVERLAP_IDS,
            "M5 restriction overlap_member_ids",
        )?;
        bounded(
            &self.source_ids,
            MAX_M5_RESTRICTION_SOURCE_IDS,
            "M5 Restriction source_ids",
        )?;
        for (set, operation) in [
            (&self.evidence_ids, "M5 Restriction evidence_ids"),
            (&self.verification_ids, "M5 Restriction verification_ids"),
            (&self.decision_ids, "M5 Restriction decision_ids"),
            (&self.finding_ids, "M5 Restriction finding_ids"),
        ] {
            bounded(set, MAX_M5_SECTION_TRACE_IDS, operation)?;
        }
        let expected = identity(
            "restriction-v4",
            &RestrictionIdentity {
                assignment_key: &self.assignment_key,
                assignment_value: self.assignment_value,
                context_pair: &self.context_pair,
                overlap_member_ids: &self.overlap_member_ids,
                qualification_source_ids: &self.qualification_source_ids,
                section_id: &self.section_id,
            },
        )?;
        require_same("Restriction derived ID", &expected, &self.id)
    }
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub(crate) fn projection_schema(&self) -> &str {
        &self.schema
    }
    pub(crate) fn projection_section_id(&self) -> &StableId {
        &self.section_id
    }
    pub(crate) fn projection_context_pair(&self) -> &[StableId] {
        &self.context_pair
    }
    pub(crate) fn projection_overlap_member_ids(&self) -> &BTreeSet<StableId> {
        &self.overlap_member_ids
    }
    pub(crate) fn projection_assignment_key(&self) -> &str {
        &self.assignment_key
    }
    pub(crate) fn projection_assignment_value(&self) -> AssignmentValueV4 {
        self.assignment_value
    }
    pub(crate) fn projection_source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub(crate) fn projection_qualification_source_ids(&self) -> &BTreeSet<StableId> {
        &self.qualification_source_ids
    }
    pub(crate) fn projection_claim_ids(&self) -> &BTreeSet<StableId> {
        &self.claim_ids
    }
    pub(crate) fn projection_evidence_ids(&self) -> &BTreeSet<StableId> {
        &self.evidence_ids
    }
    pub(crate) fn projection_verification_ids(&self) -> &BTreeSet<StableId> {
        &self.verification_ids
    }
    pub(crate) fn projection_decision_ids(&self) -> &BTreeSet<StableId> {
        &self.decision_ids
    }
    pub(crate) fn projection_finding_ids(&self) -> &BTreeSet<StableId> {
        &self.finding_ids
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalCandidateV4 {
    schema: String,
    id: StableId,
    cover_id: StableId,
    invariant_id: StableId,
    property_id: String,
    required_section_ids: Vec<StableId>,
    restriction_ids: Vec<StableId>,
    qualification_source_ids: BTreeSet<StableId>,
    source_ids: BTreeSet<StableId>,
    claim_ids: BTreeSet<StableId>,
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_ids: BTreeSet<StableId>,
    finding_ids: BTreeSet<StableId>,
}

impl GlobalCandidateV4 {
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    pub(crate) fn complete_body_hash(&self) -> crate::Result<ContentHash> {
        Ok(ContentHash::sha256(&crate::canonical_json(self)?))
    }
    fn validate(&self) -> M5Result<()> {
        if self.schema != "reviewgraphen.global_candidate.v4"
            || self.property_id != DOUBLE_SUBMIT_PROPERTY_ID
            || self.invariant_id != fixed_id(DOUBLE_SUBMIT_INVARIANT_ID)
            || self.required_section_ids.len() != 2
            || self.restriction_ids.len() != 2
        {
            return Err(M5Error::Validation(
                "invalid M5 Candidate fixed fields".into(),
            ));
        }
        bounded(
            &self.source_ids,
            MAX_M5_ATTEMPT_SOURCE_IDS,
            "M5 Candidate source_ids",
        )?;
        bounded(
            &self.qualification_source_ids,
            64,
            "M5 Candidate qualification_source_ids",
        )?;
        let expected = identity(
            "global-candidate-v4",
            &CandidateIdentity {
                cover_id: &self.cover_id,
                invariant_id: &self.invariant_id,
                property_id: &self.property_id,
                qualification_source_ids: &self.qualification_source_ids,
                required_section_ids: &self.required_section_ids,
                restriction_ids: &self.restriction_ids,
            },
        )?;
        require_same("Candidate derived ID", &expected, &self.id)
    }
    pub(crate) fn projection_schema(&self) -> &str {
        &self.schema
    }
    pub(crate) fn projection_id(&self) -> &StableId {
        &self.id
    }
    pub(crate) fn projection_cover_id(&self) -> &StableId {
        &self.cover_id
    }
    pub(crate) fn projection_invariant_id(&self) -> &StableId {
        &self.invariant_id
    }
    pub(crate) fn projection_property_id(&self) -> &str {
        &self.property_id
    }
    pub(crate) fn projection_required_section_ids(&self) -> &[StableId] {
        &self.required_section_ids
    }
    pub(crate) fn projection_restriction_ids(&self) -> &[StableId] {
        &self.restriction_ids
    }
    pub(crate) fn projection_qualification_source_ids(&self) -> &BTreeSet<StableId> {
        &self.qualification_source_ids
    }
    pub(crate) fn projection_source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub(crate) fn projection_claim_ids(&self) -> &BTreeSet<StableId> {
        &self.claim_ids
    }
    pub(crate) fn projection_evidence_ids(&self) -> &BTreeSet<StableId> {
        &self.evidence_ids
    }
    pub(crate) fn projection_verification_ids(&self) -> &BTreeSet<StableId> {
        &self.verification_ids
    }
    pub(crate) fn projection_decision_ids(&self) -> &BTreeSet<StableId> {
        &self.decision_ids
    }
    pub(crate) fn projection_finding_ids(&self) -> &BTreeSet<StableId> {
        &self.finding_ids
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GluingAttemptV4 {
    schema: String,
    id: StableId,
    cover_id: StableId,
    snapshot_id: StableId,
    property_id: String,
    invariant_id: StableId,
    input_descriptor_ids: Vec<StableId>,
    section_ids: Vec<StableId>,
    restriction_ids: Vec<StableId>,
    result: GluingResultV4,
    global_candidate_id: Option<StableId>,
    obstruction_id: Option<StableId>,
    source_ids: BTreeSet<StableId>,
    claim_ids: BTreeSet<StableId>,
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_ids: BTreeSet<StableId>,
    finding_ids: BTreeSet<StableId>,
}
impl GluingAttemptV4 {
    pub(crate) fn complete_body_hash(&self) -> crate::Result<ContentHash> {
        Ok(ContentHash::sha256(&crate::canonical_json(self)?))
    }
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub const fn result(&self) -> GluingResultV4 {
        self.result
    }
    pub(crate) fn projection_schema(&self) -> &str {
        &self.schema
    }
    pub(crate) fn projection_cover_id(&self) -> &StableId {
        &self.cover_id
    }
    pub(crate) fn projection_snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    pub(crate) fn projection_property_id(&self) -> &str {
        &self.property_id
    }
    pub(crate) fn projection_invariant_id(&self) -> &StableId {
        &self.invariant_id
    }
    pub(crate) fn projection_input_descriptor_ids(&self) -> &[StableId] {
        &self.input_descriptor_ids
    }
    pub(crate) fn projection_section_ids(&self) -> &[StableId] {
        &self.section_ids
    }
    pub(crate) fn projection_restriction_ids(&self) -> &[StableId] {
        &self.restriction_ids
    }
    pub(crate) fn projection_global_candidate_id(&self) -> Option<&StableId> {
        self.global_candidate_id.as_ref()
    }
    pub(crate) fn projection_obstruction_id(&self) -> Option<&StableId> {
        self.obstruction_id.as_ref()
    }
    pub(crate) fn projection_source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub(crate) fn projection_claim_ids(&self) -> &BTreeSet<StableId> {
        &self.claim_ids
    }
    pub(crate) fn projection_evidence_ids(&self) -> &BTreeSet<StableId> {
        &self.evidence_ids
    }
    pub(crate) fn projection_verification_ids(&self) -> &BTreeSet<StableId> {
        &self.verification_ids
    }
    pub(crate) fn projection_decision_ids(&self) -> &BTreeSet<StableId> {
        &self.decision_ids
    }
    pub(crate) fn projection_finding_ids(&self) -> &BTreeSet<StableId> {
        &self.finding_ids
    }
    fn validate(&self) -> M5Result<()> {
        if self.schema != "reviewgraphen.gluing_attempt.v4"
            || self.property_id != DOUBLE_SUBMIT_PROPERTY_ID
            || self.invariant_id != fixed_id(DOUBLE_SUBMIT_INVARIANT_ID)
            || self.input_descriptor_ids.len() != 2
            || self.section_ids.len() > 2
            || self.restriction_ids.len() > 2
        {
            return Err(M5Error::Validation(
                "invalid M5 Attempt fixed fields".into(),
            ));
        }
        let option_ok = match self.result {
            GluingResultV4::Unknown | GluingResultV4::Failed => {
                self.global_candidate_id.is_none() && self.obstruction_id.is_some()
            }
            GluingResultV4::Candidate
            | GluingResultV4::GluedWithQualification
            | GluingResultV4::Glued => {
                self.global_candidate_id.is_some() && self.obstruction_id.is_none()
            }
        };
        if !option_ok {
            return Err(M5Error::Validation(
                "invalid M5 Attempt option cardinality".into(),
            ));
        }
        bounded(
            &self.source_ids,
            MAX_M5_ATTEMPT_SOURCE_IDS,
            "M5 Attempt source_ids",
        )?;
        let expected = identity(
            "gluing-attempt-v4",
            &AttemptIdentity {
                cover_id: &self.cover_id,
                input_descriptor_ids: &self.input_descriptor_ids,
                invariant_id: &self.invariant_id,
                property_id: &self.property_id,
                restriction_ids: &self.restriction_ids,
                result: self.result,
                section_ids: &self.section_ids,
                snapshot_id: &self.snapshot_id,
            },
        )?;
        require_same("Attempt derived ID", &expected, &self.id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GluingObstructionV4 {
    schema: String,
    id: StableId,
    attempt_id: StableId,
    kind: GluingObstructionKindV4,
    conflicting_context_ids: Vec<StableId>,
    section_ids: Vec<StableId>,
    overlap_member_ids: BTreeSet<StableId>,
    assignment_key: String,
    left_assignment_value: Option<AssignmentValueV4>,
    right_assignment_value: Option<AssignmentValueV4>,
    source_ids: BTreeSet<StableId>,
    claim_ids: BTreeSet<StableId>,
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_ids: BTreeSet<StableId>,
    finding_ids: BTreeSet<StableId>,
    affected_invariant_id: StableId,
    severity: M5SeverityV4,
    required_resolution: GluingRequiredResolutionV4,
    human_decision_required: bool,
    blocks: BTreeSet<StableId>,
}
impl GluingObstructionV4 {
    pub(crate) fn complete_body_hash(&self) -> crate::Result<ContentHash> {
        Ok(ContentHash::sha256(&crate::canonical_json(self)?))
    }
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub const fn kind(&self) -> GluingObstructionKindV4 {
        self.kind
    }
    pub const fn human_decision_required(&self) -> bool {
        self.human_decision_required
    }
    pub(crate) fn projection_schema(&self) -> &str {
        &self.schema
    }
    pub(crate) fn projection_attempt_id(&self) -> &StableId {
        &self.attempt_id
    }
    pub(crate) fn projection_conflicting_context_ids(&self) -> &[StableId] {
        &self.conflicting_context_ids
    }
    pub(crate) fn projection_section_ids(&self) -> &[StableId] {
        &self.section_ids
    }
    pub(crate) fn projection_overlap_member_ids(&self) -> &BTreeSet<StableId> {
        &self.overlap_member_ids
    }
    pub(crate) fn projection_assignment_key(&self) -> &str {
        &self.assignment_key
    }
    pub(crate) fn projection_left_assignment_value(&self) -> Option<AssignmentValueV4> {
        self.left_assignment_value
    }
    pub(crate) fn projection_right_assignment_value(&self) -> Option<AssignmentValueV4> {
        self.right_assignment_value
    }
    pub(crate) fn projection_source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub(crate) fn projection_claim_ids(&self) -> &BTreeSet<StableId> {
        &self.claim_ids
    }
    pub(crate) fn projection_evidence_ids(&self) -> &BTreeSet<StableId> {
        &self.evidence_ids
    }
    pub(crate) fn projection_verification_ids(&self) -> &BTreeSet<StableId> {
        &self.verification_ids
    }
    pub(crate) fn projection_decision_ids(&self) -> &BTreeSet<StableId> {
        &self.decision_ids
    }
    pub(crate) fn projection_finding_ids(&self) -> &BTreeSet<StableId> {
        &self.finding_ids
    }
    pub(crate) fn projection_affected_invariant_id(&self) -> &StableId {
        &self.affected_invariant_id
    }
    pub(crate) fn projection_severity(&self) -> M5SeverityV4 {
        self.severity
    }
    pub(crate) fn projection_required_resolution(&self) -> GluingRequiredResolutionV4 {
        self.required_resolution
    }
    pub(crate) fn projection_blocks(&self) -> &BTreeSet<StableId> {
        &self.blocks
    }
    fn validate(&self) -> M5Result<()> {
        if self.schema != "reviewgraphen.gluing_obstruction.v4"
            || self.assignment_key != DOUBLE_SUBMIT_ASSIGNMENT_KEY
            || self.affected_invariant_id != fixed_id(DOUBLE_SUBMIT_INVARIANT_ID)
            || self.blocks != BTreeSet::from([self.affected_invariant_id.clone()])
        {
            return Err(M5Error::Validation(
                "invalid M5 Obstruction fixed fields".into(),
            ));
        }
        let exact = match self.kind {
            GluingObstructionKindV4::RequiredSectionMissing => (
                GluingRequiredResolutionV4::RecordRequiredSection,
                M5SeverityV4::High,
                false,
                true,
            ),
            GluingObstructionKindV4::RequiredOverlapMissing => (
                GluingRequiredResolutionV4::ResolveContextOverlap,
                M5SeverityV4::High,
                false,
                true,
            ),
            GluingObstructionKindV4::SectionUnknown => (
                GluingRequiredResolutionV4::ResolveUnknownDuplicateProtectionAssignment,
                M5SeverityV4::High,
                false,
                false,
            ),
            GluingObstructionKindV4::AssignmentConflict => (
                GluingRequiredResolutionV4::ResolveDuplicateProtectionResponsibility,
                M5SeverityV4::Critical,
                true,
                false,
            ),
        };
        if self.required_resolution != exact.0
            || self.severity != exact.1
            || self.human_decision_required != exact.2
            || ((self.left_assignment_value.is_none() || self.right_assignment_value.is_none())
                != exact.3)
        {
            return Err(M5Error::Validation("invalid M5 Obstruction matrix".into()));
        }
        bounded(
            &self.source_ids,
            MAX_M5_ATTEMPT_SOURCE_IDS,
            "M5 Obstruction source_ids",
        )?;
        let expected = identity(
            "gluing-obstruction-v4",
            &ObstructionIdentity {
                affected_invariant_id: &self.affected_invariant_id,
                assignment_key: &self.assignment_key,
                attempt_id: &self.attempt_id,
                blocks: &self.blocks,
                conflicting_context_ids: &self.conflicting_context_ids,
                kind: self.kind,
                left_assignment_value: self.left_assignment_value,
                overlap_member_ids: &self.overlap_member_ids,
                required_resolution: self.required_resolution,
                right_assignment_value: self.right_assignment_value,
                section_ids: &self.section_ids,
                severity: self.severity,
            },
        )?;
        require_same("Obstruction derived ID", &expected, &self.id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GluingBundleV4 {
    schema: String,
    cover: ContextCoverV4,
    input_descriptor_ids: Vec<StableId>,
    sections: Vec<SectionV4>,
    restrictions: Vec<RestrictionV4>,
    attempt: GluingAttemptV4,
    global_candidate: Option<GlobalCandidateV4>,
    obstruction: Option<GluingObstructionV4>,
}

fn retained_id_set_bytes(values: &BTreeSet<StableId>) -> usize {
    values.iter().fold(
        values.len().saturating_mul(std::mem::size_of::<StableId>()),
        |total, value| total.saturating_add(value.allocated_bytes()),
    )
}

fn retained_id_vec_bytes(values: &Vec<StableId>) -> usize {
    values.iter().fold(
        values
            .capacity()
            .saturating_mul(std::mem::size_of::<StableId>()),
        |total, value| total.saturating_add(value.allocated_bytes()),
    )
}

/// Allocation-free upper bound used before V5 reads either descriptor CAS
/// object. Canonical bytes cover every dynamic string/ID byte; the additional
/// slots cover typed ownership which is absent from the wire representation.
pub(crate) fn v5_terminal_m5_typed_retained_upper_bound(
    descriptor_canonical_bytes: u64,
    bundle_payload_bytes: u64,
) -> crate::Result<u64> {
    let id_slot = u64::try_from(std::mem::size_of::<StableId>()).unwrap_or(u64::MAX);
    let descriptor_slots = u64::try_from(std::mem::size_of::<GluingInputDescriptorV4>())
        .unwrap_or(u64::MAX)
        .checked_mul(2)
        .and_then(|value| {
            value.checked_add(
                u64::try_from(MAX_M5_DESCRIPTOR_QUALIFICATION_IDS)
                    .unwrap_or(u64::MAX)
                    .checked_mul(id_slot)?
                    .checked_mul(2)?,
            )
        })
        .ok_or(crate::DomainError::Incomplete {
            operation: "V5 M5 descriptor typed retained upper bound",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;
    // Every retained ID requires at least one JSON byte. Charging one complete
    // StableId slot per bundle byte is deliberately conservative and closes
    // every nested set/vector without trusting wire size as object size.
    let bundle_slots = bundle_payload_bytes
        .checked_mul(id_slot)
        .and_then(|value| {
            value.checked_add(
                u64::try_from(
                    std::mem::size_of::<GluingBundleV4>()
                        + 2 * std::mem::size_of::<SectionV4>()
                        + 2 * std::mem::size_of::<RestrictionV4>(),
                )
                .unwrap_or(u64::MAX),
            )
        })
        .ok_or(crate::DomainError::Incomplete {
            operation: "V5 M5 bundle typed retained upper bound",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;
    descriptor_canonical_bytes
        .checked_add(bundle_payload_bytes)
        .and_then(|value| value.checked_add(descriptor_slots))
        .and_then(|value| value.checked_add(bundle_slots))
        .ok_or(crate::DomainError::Incomplete {
            operation: "V5 M5 typed retained upper bound",
            limit: usize::MAX,
            observed: usize::MAX,
        })
}

fn retained_strings_and_ids(strings: &[&String], ids: &[&StableId]) -> u64 {
    strings
        .iter()
        .map(|value| value.capacity())
        .chain(ids.iter().map(|value| value.allocated_bytes()))
        .fold(0_u64, |total, bytes| {
            total.saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX))
        })
}

fn retained_cover_bytes(value: &ContextCoverV4) -> u64 {
    retained_strings_and_ids(
        &[&value.schema, &value.profile_descriptor_id],
        &[
            &value.id,
            &value.run_id,
            &value.snapshot_id,
            &value.universe_id,
            &value.plan_id,
        ],
    )
    .saturating_add(
        u64::try_from(retained_id_set_bytes(&value.selected_obligation_ids)).unwrap_or(u64::MAX),
    )
    .saturating_add(
        u64::try_from(retained_id_vec_bytes(&value.required_context_ids)).unwrap_or(u64::MAX),
    )
    .saturating_add(
        u64::try_from(retained_id_set_bytes(&value.cover_domain_ids)).unwrap_or(u64::MAX),
    )
    .saturating_add(
        u64::try_from(retained_id_set_bytes(&value.covered_domain_ids)).unwrap_or(u64::MAX),
    )
    .saturating_add(
        u64::try_from(retained_id_set_bytes(&value.uncovered_domain_ids)).unwrap_or(u64::MAX),
    )
    .saturating_add(u64::try_from(retained_id_set_bytes(&value.source_ids)).unwrap_or(u64::MAX))
}

fn retained_section_bytes(value: &SectionV4) -> u64 {
    let mut total = retained_strings_and_ids(
        &[&value.schema, &value.property_id, &value.assignment_key],
        &[
            &value.id,
            &value.cover_id,
            &value.context_id,
            &value.snapshot_id,
            &value.invariant_id,
            &value.obligation_id,
            &value.claim_id,
            &value.claim_assessment_id,
            &value.input_descriptor_id,
            &value.input_registration_id,
        ],
    );
    for set in [
        &value.source_ids,
        &value.qualification_source_ids,
        &value.binding_ids,
        &value.evidence_ids,
        &value.verification_ids,
        &value.decision_ids,
        &value.finding_ids,
    ] {
        total = total.saturating_add(u64::try_from(retained_id_set_bytes(set)).unwrap_or(u64::MAX));
    }
    total
}

fn retained_restriction_bytes(value: &RestrictionV4) -> u64 {
    let mut total = retained_strings_and_ids(
        &[&value.schema, &value.assignment_key],
        &[&value.id, &value.section_id],
    )
    .saturating_add(u64::try_from(retained_id_vec_bytes(&value.context_pair)).unwrap_or(u64::MAX));
    for set in [
        &value.overlap_member_ids,
        &value.source_ids,
        &value.qualification_source_ids,
        &value.claim_ids,
        &value.evidence_ids,
        &value.verification_ids,
        &value.decision_ids,
        &value.finding_ids,
    ] {
        total = total.saturating_add(u64::try_from(retained_id_set_bytes(set)).unwrap_or(u64::MAX));
    }
    total
}

fn retained_candidate_bytes(value: &GlobalCandidateV4) -> u64 {
    let mut total = retained_strings_and_ids(
        &[&value.schema, &value.property_id],
        &[&value.id, &value.cover_id, &value.invariant_id],
    )
    .saturating_add(
        u64::try_from(retained_id_vec_bytes(&value.required_section_ids)).unwrap_or(u64::MAX),
    )
    .saturating_add(
        u64::try_from(retained_id_vec_bytes(&value.restriction_ids)).unwrap_or(u64::MAX),
    );
    for set in [
        &value.qualification_source_ids,
        &value.source_ids,
        &value.claim_ids,
        &value.evidence_ids,
        &value.verification_ids,
        &value.decision_ids,
        &value.finding_ids,
    ] {
        total = total.saturating_add(u64::try_from(retained_id_set_bytes(set)).unwrap_or(u64::MAX));
    }
    total
}

fn retained_attempt_bytes(value: &GluingAttemptV4) -> u64 {
    let mut total = retained_strings_and_ids(
        &[&value.schema, &value.property_id],
        &[
            &value.id,
            &value.cover_id,
            &value.snapshot_id,
            &value.invariant_id,
        ],
    );
    for values in [
        &value.input_descriptor_ids,
        &value.section_ids,
        &value.restriction_ids,
    ] {
        total =
            total.saturating_add(u64::try_from(retained_id_vec_bytes(values)).unwrap_or(u64::MAX));
    }
    for id in [&value.global_candidate_id, &value.obstruction_id]
        .into_iter()
        .flatten()
    {
        total = total.saturating_add(u64::try_from(id.allocated_bytes()).unwrap_or(u64::MAX));
    }
    for set in [
        &value.source_ids,
        &value.claim_ids,
        &value.evidence_ids,
        &value.verification_ids,
        &value.decision_ids,
        &value.finding_ids,
    ] {
        total = total.saturating_add(u64::try_from(retained_id_set_bytes(set)).unwrap_or(u64::MAX));
    }
    total
}

fn retained_obstruction_bytes(value: &GluingObstructionV4) -> u64 {
    let mut total = retained_strings_and_ids(
        &[&value.schema, &value.assignment_key],
        &[&value.id, &value.attempt_id, &value.affected_invariant_id],
    );
    for values in [&value.conflicting_context_ids, &value.section_ids] {
        total =
            total.saturating_add(u64::try_from(retained_id_vec_bytes(values)).unwrap_or(u64::MAX));
    }
    for set in [
        &value.overlap_member_ids,
        &value.source_ids,
        &value.claim_ids,
        &value.evidence_ids,
        &value.verification_ids,
        &value.decision_ids,
        &value.finding_ids,
        &value.blocks,
    ] {
        total = total.saturating_add(u64::try_from(retained_id_set_bytes(set)).unwrap_or(u64::MAX));
    }
    total
}

#[derive(Default)]
struct TraceUnions {
    claims: BTreeSet<StableId>,
    evidence: BTreeSet<StableId>,
    verifications: BTreeSet<StableId>,
    decisions: BTreeSet<StableId>,
    findings: BTreeSet<StableId>,
    qualifications: BTreeSet<StableId>,
}

fn required_contexts() -> Vec<StableId> {
    vec![
        fixed_id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
        fixed_id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
    ]
}

fn collect_traces(sections: &[SectionV4]) -> M5Result<TraceUnions> {
    let mut out = TraceUnions::default();
    for section in sections {
        out.claims.insert(section.claim_id.clone());
        out.evidence.extend(section.evidence_ids.iter().cloned());
        out.verifications
            .extend(section.verification_ids.iter().cloned());
        out.decisions.extend(section.decision_ids.iter().cloned());
        out.findings.extend(section.finding_ids.iter().cloned());
        out.qualifications
            .extend(section.qualification_source_ids.iter().cloned());
    }
    bounded(&out.claims, 2, "M5 Attempt claim_ids")?;
    for (set, op) in [
        (&out.evidence, "M5 Attempt evidence_ids"),
        (&out.verifications, "M5 Attempt verification_ids"),
        (&out.decisions, "M5 Attempt decision_ids"),
        (&out.findings, "M5 Attempt finding_ids"),
    ] {
        bounded(set, MAX_M5_TRACE_IDS, op)?;
    }
    bounded(
        &out.qualifications,
        64,
        "M5 Candidate qualification_source_ids",
    )?;
    Ok(out)
}

impl GluingBundleV4 {
    pub(crate) fn retained_bytes_for_v5(&self) -> u64 {
        let mut total = u64::try_from(std::mem::size_of::<Self>())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(self.schema.capacity()).unwrap_or(u64::MAX))
            .saturating_add(retained_cover_bytes(&self.cover))
            .saturating_add(
                u64::try_from(retained_id_vec_bytes(&self.input_descriptor_ids))
                    .unwrap_or(u64::MAX),
            )
            .saturating_add(retained_attempt_bytes(&self.attempt));
        total = total.saturating_add(
            u64::try_from(
                self.sections
                    .capacity()
                    .saturating_mul(std::mem::size_of::<SectionV4>()),
            )
            .unwrap_or(u64::MAX),
        );
        for section in &self.sections {
            total = total.saturating_add(retained_section_bytes(section));
        }
        total = total.saturating_add(
            u64::try_from(
                self.restrictions
                    .capacity()
                    .saturating_mul(std::mem::size_of::<RestrictionV4>()),
            )
            .unwrap_or(u64::MAX),
        );
        for restriction in &self.restrictions {
            total = total.saturating_add(retained_restriction_bytes(restriction));
        }
        if let Some(candidate) = &self.global_candidate {
            total = total.saturating_add(retained_candidate_bytes(candidate));
        }
        if let Some(obstruction) = &self.obstruction {
            total = total.saturating_add(retained_obstruction_bytes(obstruction));
        }
        total
    }

    pub(crate) fn from_validated(source: ValidatedM5SourceV4) -> Self {
        source.bundle
    }

    fn derive(
        cover: ContextCoverV4,
        descriptors: [GluingInputDescriptorV4; 2],
        input_registration_ids: [StableId; 2],
        sections: Vec<SectionV4>,
        overlap_member_ids: BTreeSet<StableId>,
    ) -> M5Result<Self> {
        bounded_len(sections.len(), MAX_M5_SECTIONS, "M5 Sections")?;
        bounded(
            &overlap_member_ids,
            MAX_M5_OVERLAP_IDS,
            "M5 overlap_member_ids",
        )?;
        cover.validate()?;
        for descriptor in &descriptors {
            descriptor.validate()?;
        }
        for section in &sections {
            section.validate()?;
        }
        for id in input_registration_ids
            .iter()
            .chain(overlap_member_ids.iter())
        {
            require_id_bytes(id, "M5 Bundle input StableId bytes")?;
        }
        if input_registration_ids[0] == input_registration_ids[1] {
            return Err(M5Error::Duplicate {
                field: "input_registration_ids",
            });
        }
        for registration_id in &input_registration_ids {
            if registration_id.kind() != "registration-v4" {
                return Err(M5Error::Validation(
                    "input registration must use the registration-v4 ID kind".into(),
                ));
            }
        }
        let contexts = required_contexts();
        for (index, descriptor) in descriptors.iter().enumerate() {
            require_same(
                "descriptor context order",
                &contexts[index],
                descriptor.context_id(),
            )?;
            require_same("descriptor run_id", cover.run_id(), descriptor.run_id())?;
            require_same(
                "descriptor snapshot_id",
                cover.snapshot_id(),
                descriptor.snapshot_id(),
            )?;
            require_same(
                "descriptor universe_id",
                cover.universe_id(),
                descriptor.universe_id(),
            )?;
            require_same("descriptor plan_id", cover.plan_id(), descriptor.plan_id())?;
        }
        let unique = sections
            .iter()
            .map(|section| section.context_id.clone())
            .collect::<BTreeSet<_>>();
        if unique.len() != sections.len() {
            return Err(M5Error::Duplicate {
                field: "sections.context_id",
            });
        }
        if sections
            .windows(2)
            .any(|pair| pair[0].context_id >= pair[1].context_id)
        {
            return Err(M5Error::ContextOrder { field: "sections" });
        }
        for section in &sections {
            require_same("section cover_id", cover.id(), &section.cover_id)?;
            require_same(
                "section snapshot_id",
                cover.snapshot_id(),
                &section.snapshot_id,
            )?;
            let index = contexts
                .iter()
                .position(|id| id == &section.context_id)
                .ok_or_else(|| {
                    M5Error::Validation("section context is outside the fixed cover".into())
                })?;
            require_same(
                "section descriptor",
                descriptors[index].id(),
                &section.input_descriptor_id,
            )?;
            require_same(
                "section registration",
                &input_registration_ids[index],
                &section.input_registration_id,
            )?;
        }
        let anchor_present =
            overlap_member_ids.contains(&fixed_id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID));
        let restrictions = if anchor_present {
            sections
                .iter()
                .map(|section| RestrictionV4::derive(section, &overlap_member_ids))
                .collect::<M5Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let traces = collect_traces(&sections)?;
        let input_descriptor_ids = descriptors.iter().map(|d| d.id.clone()).collect::<Vec<_>>();
        let section_ids = sections.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
        let restriction_ids = restrictions
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>();
        let invariant_source_id = fixed_id(DOUBLE_SUBMIT_INVARIANT_ID);
        let empty_ids = BTreeSet::new();
        let attempt_sets = [
            sections
                .first()
                .map_or(&empty_ids, |section| &section.source_ids),
            sections
                .get(1)
                .map_or(&empty_ids, |section| &section.source_ids),
            restrictions
                .first()
                .map_or(&empty_ids, |restriction| &restriction.source_ids),
            restrictions
                .get(1)
                .map_or(&empty_ids, |restriction| &restriction.source_ids),
            &traces.claims,
            &traces.evidence,
            &traces.verifications,
            &traces.decisions,
            &traces.findings,
            &traces.qualifications,
        ];
        let attempt_fixed = [
            &cover.id,
            &invariant_source_id,
            &descriptors[0].id,
            &descriptors[1].id,
            &input_registration_ids[0],
            &input_registration_ids[1],
            sections.first().map_or(&cover.id, |section| &section.id),
            sections.get(1).map_or(&cover.id, |section| &section.id),
            restrictions
                .first()
                .map_or(&cover.id, |restriction| &restriction.id),
            restrictions
                .get(1)
                .map_or(&cover.id, |restriction| &restriction.id),
        ];
        let exact_attempt_source_count = preflight_union(
            &attempt_sets,
            &attempt_fixed,
            MAX_M5_ATTEMPT_SOURCE_IDS,
            "M5 Attempt source_ids",
        )?;
        let mut attempt_base = BTreeSet::from([cover.id.clone(), invariant_source_id.clone()]);
        attempt_base.extend(input_descriptor_ids.iter().cloned());
        attempt_base.extend(input_registration_ids);
        attempt_base.extend(section_ids.iter().cloned());
        attempt_base.extend(restriction_ids.iter().cloned());
        for section in &sections {
            attempt_base.extend(section.source_ids.iter().cloned());
        }
        for restriction in &restrictions {
            attempt_base.extend(restriction.source_ids.iter().cloned());
        }
        for set in [
            &traces.claims,
            &traces.evidence,
            &traces.verifications,
            &traces.decisions,
            &traces.findings,
            &traces.qualifications,
        ] {
            attempt_base.extend(set.iter().cloned());
        }
        debug_assert_eq!(attempt_base.len(), exact_attempt_source_count);
        bounded(
            &attempt_base,
            MAX_M5_ATTEMPT_SOURCE_IDS,
            "M5 Attempt source_ids",
        )?;
        let result = if sections.len() != 2
            || !anchor_present
            || sections
                .iter()
                .any(|s| s.assignment_value == AssignmentValueV4::Unknown)
        {
            GluingResultV4::Unknown
        } else if sections[0]
            .assignment_value
            .compatibility(sections[1].assignment_value)
            == AssignmentCompatibilityV4::Conflict
        {
            GluingResultV4::Failed
        } else if sections.iter().any(|s| !s.passed_current_verification) {
            GluingResultV4::Candidate
        } else if !traces.qualifications.is_empty() {
            GluingResultV4::GluedWithQualification
        } else {
            GluingResultV4::Glued
        };
        let invariant_id = fixed_id(DOUBLE_SUBMIT_INVARIANT_ID);
        let property_id = DOUBLE_SUBMIT_PROPERTY_ID.to_owned();
        let attempt_id = identity(
            "gluing-attempt-v4",
            &AttemptIdentity {
                cover_id: &cover.id,
                input_descriptor_ids: &input_descriptor_ids,
                invariant_id: &invariant_id,
                property_id: &property_id,
                restriction_ids: &restriction_ids,
                result,
                section_ids: &section_ids,
                snapshot_id: &cover.snapshot_id,
            },
        )?;
        let candidate = if matches!(
            result,
            GluingResultV4::Candidate
                | GluingResultV4::GluedWithQualification
                | GluingResultV4::Glued
        ) {
            let id = identity(
                "global-candidate-v4",
                &CandidateIdentity {
                    cover_id: &cover.id,
                    invariant_id: &invariant_id,
                    property_id: &property_id,
                    qualification_source_ids: &traces.qualifications,
                    required_section_ids: &section_ids,
                    restriction_ids: &restriction_ids,
                },
            )?;
            Some(GlobalCandidateV4 {
                schema: "reviewgraphen.global_candidate.v4".to_owned(),
                id,
                cover_id: cover.id.clone(),
                invariant_id: invariant_id.clone(),
                property_id: property_id.clone(),
                required_section_ids: section_ids.clone(),
                restriction_ids: restriction_ids.clone(),
                qualification_source_ids: traces.qualifications.clone(),
                source_ids: attempt_base.clone(),
                claim_ids: traces.claims.clone(),
                evidence_ids: traces.evidence.clone(),
                verification_ids: traces.verifications.clone(),
                decision_ids: traces.decisions.clone(),
                finding_ids: traces.findings.clone(),
            })
        } else {
            None
        };
        let obstruction = if matches!(result, GluingResultV4::Unknown | GluingResultV4::Failed) {
            let (kind, conflicting_context_ids, left, right, resolution, severity, human) =
                if sections.len() != 2 {
                    let present = sections
                        .iter()
                        .map(|s| s.context_id.clone())
                        .collect::<BTreeSet<_>>();
                    (
                        GluingObstructionKindV4::RequiredSectionMissing,
                        contexts
                            .iter()
                            .filter(|id| !present.contains(*id))
                            .cloned()
                            .collect(),
                        None,
                        None,
                        GluingRequiredResolutionV4::RecordRequiredSection,
                        M5SeverityV4::High,
                        false,
                    )
                } else if !anchor_present {
                    (
                        GluingObstructionKindV4::RequiredOverlapMissing,
                        contexts.clone(),
                        None,
                        None,
                        GluingRequiredResolutionV4::ResolveContextOverlap,
                        M5SeverityV4::High,
                        false,
                    )
                } else if sections
                    .iter()
                    .any(|s| s.assignment_value == AssignmentValueV4::Unknown)
                {
                    (
                        GluingObstructionKindV4::SectionUnknown,
                        sections
                            .iter()
                            .filter(|s| s.assignment_value == AssignmentValueV4::Unknown)
                            .map(|s| s.context_id.clone())
                            .collect(),
                        Some(sections[0].assignment_value),
                        Some(sections[1].assignment_value),
                        GluingRequiredResolutionV4::ResolveUnknownDuplicateProtectionAssignment,
                        M5SeverityV4::High,
                        false,
                    )
                } else {
                    (
                        GluingObstructionKindV4::AssignmentConflict,
                        contexts.clone(),
                        Some(sections[0].assignment_value),
                        Some(sections[1].assignment_value),
                        GluingRequiredResolutionV4::ResolveDuplicateProtectionResponsibility,
                        M5SeverityV4::Critical,
                        true,
                    )
                };
            let source_ids = build_obstruction_sources(
                &attempt_base,
                &conflicting_context_ids,
                &overlap_member_ids,
            )?;
            let blocks = BTreeSet::from([invariant_id.clone()]);
            let id = identity(
                "gluing-obstruction-v4",
                &ObstructionIdentity {
                    affected_invariant_id: &invariant_id,
                    assignment_key: DOUBLE_SUBMIT_ASSIGNMENT_KEY,
                    attempt_id: &attempt_id,
                    blocks: &blocks,
                    conflicting_context_ids: &conflicting_context_ids,
                    kind,
                    left_assignment_value: left,
                    overlap_member_ids: &overlap_member_ids,
                    required_resolution: resolution,
                    right_assignment_value: right,
                    section_ids: &section_ids,
                    severity,
                },
            )?;
            Some(GluingObstructionV4 {
                schema: "reviewgraphen.gluing_obstruction.v4".to_owned(),
                id,
                attempt_id: attempt_id.clone(),
                kind,
                conflicting_context_ids,
                section_ids: section_ids.clone(),
                overlap_member_ids: overlap_member_ids.clone(),
                assignment_key: DOUBLE_SUBMIT_ASSIGNMENT_KEY.to_owned(),
                left_assignment_value: left,
                right_assignment_value: right,
                source_ids,
                claim_ids: traces.claims.clone(),
                evidence_ids: traces.evidence.clone(),
                verification_ids: traces.verifications.clone(),
                decision_ids: traces.decisions.clone(),
                finding_ids: traces.findings.clone(),
                affected_invariant_id: invariant_id.clone(),
                severity,
                required_resolution: resolution,
                human_decision_required: human,
                blocks,
            })
        } else {
            None
        };
        let attempt = GluingAttemptV4 {
            schema: "reviewgraphen.gluing_attempt.v4".to_owned(),
            id: attempt_id,
            cover_id: cover.id.clone(),
            snapshot_id: cover.snapshot_id.clone(),
            property_id,
            invariant_id,
            input_descriptor_ids: input_descriptor_ids.clone(),
            section_ids,
            restriction_ids,
            result,
            global_candidate_id: candidate.as_ref().map(|x| x.id.clone()),
            obstruction_id: obstruction.as_ref().map(|x| x.id.clone()),
            source_ids: attempt_base,
            claim_ids: traces.claims,
            evidence_ids: traces.evidence,
            verification_ids: traces.verifications,
            decision_ids: traces.decisions,
            finding_ids: traces.findings,
        };
        let bundle = Self {
            schema: "reviewgraphen.gluing_bundle.v4".to_owned(),
            cover,
            input_descriptor_ids,
            sections,
            restrictions,
            attempt,
            global_candidate: candidate,
            obstruction,
        };
        bundle.validate()?;
        Ok(bundle)
    }

    fn validate(&self) -> M5Result<()> {
        if self.schema != "reviewgraphen.gluing_bundle.v4"
            || self.input_descriptor_ids.len() != 2
            || self.input_descriptor_ids[0] == self.input_descriptor_ids[1]
            || self.sections.len() > 2
            || self.restrictions.len() > 2
        {
            return Err(M5Error::Validation("invalid M5 Bundle fixed fields".into()));
        }
        self.cover.validate()?;
        if self
            .sections
            .windows(2)
            .any(|pair| pair[0].context_id >= pair[1].context_id)
            || self
                .restrictions
                .iter()
                .zip(&self.sections)
                .any(|(restriction, section)| restriction.section_id != section.id)
        {
            return Err(M5Error::ContextOrder {
                field: "bundle positional arrays",
            });
        }
        for section in &self.sections {
            section.validate()?;
            require_same("bundle Section cover", self.cover.id(), &section.cover_id)?;
        }
        for restriction in &self.restrictions {
            restriction.validate()?;
        }
        self.attempt.validate()?;
        require_same(
            "bundle Attempt cover",
            self.cover.id(),
            &self.attempt.cover_id,
        )?;
        if self.attempt.input_descriptor_ids != self.input_descriptor_ids
            || self.attempt.section_ids
                != self
                    .sections
                    .iter()
                    .map(|item| item.id.clone())
                    .collect::<Vec<_>>()
            || self.attempt.restriction_ids
                != self
                    .restrictions
                    .iter()
                    .map(|item| item.id.clone())
                    .collect::<Vec<_>>()
        {
            return Err(M5Error::Validation(
                "invalid M5 Bundle attempt references".into(),
            ));
        }
        let overlap = self
            .restrictions
            .first()
            .map(|item| &item.overlap_member_ids)
            .or_else(|| {
                self.obstruction
                    .as_ref()
                    .map(|item| &item.overlap_member_ids)
            })
            .cloned()
            .unwrap_or_default();
        let anchor = overlap.contains(&fixed_id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID));
        let expected_result = if self.sections.len() != 2
            || !anchor
            || self
                .sections
                .iter()
                .any(|item| item.assignment_value == AssignmentValueV4::Unknown)
        {
            GluingResultV4::Unknown
        } else if self.sections[0]
            .assignment_value
            .compatibility(self.sections[1].assignment_value)
            == AssignmentCompatibilityV4::Conflict
        {
            GluingResultV4::Failed
        } else if self
            .sections
            .iter()
            .any(|item| !item.passed_current_verification)
        {
            GluingResultV4::Candidate
        } else if self
            .sections
            .iter()
            .any(|item| !item.qualification_source_ids.is_empty())
        {
            GluingResultV4::GluedWithQualification
        } else {
            GluingResultV4::Glued
        };
        if self.attempt.result != expected_result {
            return Err(M5Error::Validation("invalid M5 result matrix".into()));
        }
        match (&self.global_candidate, &self.obstruction) {
            (Some(candidate), None) => {
                candidate.validate()?;
                if self.attempt.global_candidate_id.as_ref() != Some(&candidate.id)
                    || candidate.source_ids != self.attempt.source_ids
                {
                    return Err(M5Error::Validation("invalid M5 Candidate ownership".into()));
                }
            }
            (None, Some(obstruction)) => {
                obstruction.validate()?;
                if self.attempt.obstruction_id.as_ref() != Some(&obstruction.id)
                    || obstruction.attempt_id != self.attempt.id
                {
                    return Err(M5Error::Validation(
                        "invalid M5 Obstruction ownership".into(),
                    ));
                }
            }
            _ => {
                return Err(M5Error::Validation(
                    "invalid M5 Bundle option ownership".into(),
                ));
            }
        }
        bounded_bytes(
            self,
            MAX_M5_BUNDLE_CANONICAL_BYTES,
            "M5 canonical gluing bundle bytes",
        )
    }

    pub(crate) fn from_json_bytes(input: &[u8], source: &ValidatedM5SourceV4) -> M5Result<Self> {
        preflight_wire_json(
            input,
            MAX_M5_BUNDLE_CANONICAL_BYTES,
            "M5 bundle JSON",
            WireShape::Bundle,
        )?;
        let _: GluingBundleWire = serde_json::from_slice(input)
            .map_err(|error| M5Error::Validation(format!("M5 bundle JSON: {error}")))?;
        let expected = source.derive_bundle()?;
        if crate::canonical_json(&expected)? != input {
            return Err(M5Error::Validation(
                "M5 bundle JSON does not equal its retained validated source".into(),
            ));
        }
        Ok(expected)
    }

    pub(crate) fn preflight_json_bytes(input: &[u8]) -> M5Result<()> {
        preflight_wire_json(
            input,
            MAX_M5_BUNDLE_CANONICAL_BYTES,
            "M5 bundle JSON",
            WireShape::Bundle,
        )
    }
    pub(crate) fn projection_schema(&self) -> &str {
        &self.schema
    }
    pub(crate) fn projection_input_descriptor_ids(&self) -> &[StableId] {
        &self.input_descriptor_ids
    }
    pub fn cover(&self) -> &ContextCoverV4 {
        &self.cover
    }
    pub fn sections(&self) -> &[SectionV4] {
        &self.sections
    }
    pub fn restrictions(&self) -> &[RestrictionV4] {
        &self.restrictions
    }
    pub fn attempt(&self) -> &GluingAttemptV4 {
        &self.attempt
    }
    pub fn global_candidate(&self) -> Option<&GlobalCandidateV4> {
        self.global_candidate.as_ref()
    }
    pub fn obstruction(&self) -> Option<&GluingObstructionV4> {
        self.obstruction.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> StableId {
        StableId::parse(s).unwrap()
    }
    fn ids(prefix: &str, n: usize) -> BTreeSet<StableId> {
        (0..n).map(|i| id(&format!("{prefix}:{i:05}"))).collect()
    }
    fn base() -> (
        ContextCoverV4,
        [GluingInputDescriptorV4; 2],
        [StableId; 2],
        BTreeSet<StableId>,
    ) {
        let run = id("run:m5");
        let snapshot = id("snapshot:m5");
        let universe = id("universe:m5");
        let plan = id("plan:m5");
        let payment = BTreeSet::from([
            id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID),
            id("function:payment"),
        ]);
        let ui = BTreeSet::from([id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID), id("function:ui")]);
        let cover = ContextCoverV4::derive(
            run.clone(),
            snapshot.clone(),
            universe.clone(),
            plan.clone(),
            BTreeSet::from([id("obligation:payment"), id("obligation:ui")]),
            BTreeSet::new(),
            payment.clone(),
            ui.clone(),
            BTreeSet::new(),
        )
        .unwrap();
        let descriptors = [
            GluingInputDescriptorV4::new(
                run.clone(),
                snapshot.clone(),
                universe.clone(),
                plan.clone(),
                id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
                AssignmentValueV4::Required,
                BTreeSet::new(),
            )
            .unwrap(),
            GluingInputDescriptorV4::new(
                run,
                snapshot,
                universe,
                plan,
                id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
                AssignmentValueV4::Required,
                BTreeSet::new(),
            )
            .unwrap(),
        ];
        (
            cover,
            descriptors,
            [id("registration-v4:payment"), id("registration-v4:ui")],
            payment.intersection(&ui).cloned().collect(),
        )
    }
    fn section(cover: &ContextCoverV4, d: &GluingInputDescriptorV4, r: StableId) -> SectionV4 {
        let suffix = if d.context_id() == &id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID) {
            "payment"
        } else {
            "ui"
        };
        let members = BTreeSet::from([
            id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID),
            id(&format!("function:{suffix}")),
        ]);
        SectionV4::derive_unverified(
            cover,
            d,
            r,
            id(&format!("obligation:{suffix}")),
            id(&format!("claim:{suffix}")),
            &members,
            BTreeSet::from([id(&format!("function:{suffix}"))]),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        )
        .unwrap()
    }

    // This deliberately reconstructs the complete M5 wire DTO only through
    // the opaque projection accessors.  It is a parity oracle for the Store
    // visitor: a missing scalar, a changed iterator order, or a different
    // enum spelling changes the canonical bytes and therefore the hash.
    fn opaque_bundle_wire(
        projection: crate::BorrowedGluingBundleProjectionV4<'_>,
    ) -> serde_json::Value {
        let cover = projection.cover();
        let cover = serde_json::json!({
            "schema": cover.schema(),
            "id": cover.id(),
            "run_id": cover.run_id(),
            "snapshot_id": cover.snapshot_id(),
            "universe_id": cover.universe_id(),
            "plan_id": cover.plan_id(),
            "profile_descriptor_id": cover.profile_descriptor_id(),
            "selected_obligation_ids": cover.selected_obligation_ids().collect::<Vec<_>>(),
            "required_context_ids": cover.required_context_ids().collect::<Vec<_>>(),
            "cover_domain_ids": cover.cover_domain_ids().collect::<Vec<_>>(),
            "covered_domain_ids": cover.covered_domain_ids().collect::<Vec<_>>(),
            "uncovered_domain_ids": cover.uncovered_domain_ids().collect::<Vec<_>>(),
            "source_ids": cover.source_ids().collect::<Vec<_>>(),
        });
        let sections = projection
            .sections()
            .map(|section| {
                serde_json::json!({
                    "schema": section.schema(),
                    "id": section.id(),
                    "cover_id": section.cover_id(),
                    "context_id": section.context_id(),
                    "snapshot_id": section.snapshot_id(),
                    "property_id": section.property_id(),
                    "invariant_id": section.invariant_id(),
                    "obligation_id": section.obligation_id(),
                    "claim_id": section.claim_id(),
                    "claim_assessment_id": section.claim_assessment_id(),
                    "input_descriptor_id": section.input_descriptor_id(),
                    "input_registration_id": section.input_registration_id(),
                    "assignment_key": section.assignment_key(),
                    "assignment_value": section.assignment_value(),
                    "passed_current_verification": section.passed_current_verification(),
                    "source_ids": section.source_ids().collect::<Vec<_>>(),
                    "qualification_source_ids": section.qualification_source_ids().collect::<Vec<_>>(),
                    "binding_ids": section.binding_ids().collect::<Vec<_>>(),
                    "evidence_ids": section.evidence_ids().collect::<Vec<_>>(),
                    "verification_ids": section.verification_ids().collect::<Vec<_>>(),
                    "decision_ids": section.decision_ids().collect::<Vec<_>>(),
                    "finding_ids": section.finding_ids().collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>();
        let restrictions = projection
            .restrictions()
            .map(|restriction| {
                serde_json::json!({
                    "schema": restriction.schema(),
                    "id": restriction.id(),
                    "section_id": restriction.section_id(),
                    "context_pair": restriction.context_pair().collect::<Vec<_>>(),
                    "overlap_member_ids": restriction.overlap_member_ids().collect::<Vec<_>>(),
                    "assignment_key": restriction.assignment_key(),
                    "assignment_value": restriction.assignment_value(),
                    "source_ids": restriction.source_ids().collect::<Vec<_>>(),
                    "qualification_source_ids": restriction.qualification_source_ids().collect::<Vec<_>>(),
                    "claim_ids": restriction.claim_ids().collect::<Vec<_>>(),
                    "evidence_ids": restriction.evidence_ids().collect::<Vec<_>>(),
                    "verification_ids": restriction.verification_ids().collect::<Vec<_>>(),
                    "decision_ids": restriction.decision_ids().collect::<Vec<_>>(),
                    "finding_ids": restriction.finding_ids().collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>();
        let attempt = projection.attempt();
        let attempt = serde_json::json!({
            "schema": attempt.schema(),
            "id": attempt.id(),
            "cover_id": attempt.cover_id(),
            "snapshot_id": attempt.snapshot_id(),
            "property_id": attempt.property_id(),
            "invariant_id": attempt.invariant_id(),
            "input_descriptor_ids": attempt.input_descriptor_ids().collect::<Vec<_>>(),
            "section_ids": attempt.section_ids().collect::<Vec<_>>(),
            "restriction_ids": attempt.restriction_ids().collect::<Vec<_>>(),
            "result": attempt.result(),
            "global_candidate_id": attempt.global_candidate_id(),
            "obstruction_id": attempt.obstruction_id(),
            "source_ids": attempt.source_ids().collect::<Vec<_>>(),
            "claim_ids": attempt.claim_ids().collect::<Vec<_>>(),
            "evidence_ids": attempt.evidence_ids().collect::<Vec<_>>(),
            "verification_ids": attempt.verification_ids().collect::<Vec<_>>(),
            "decision_ids": attempt.decision_ids().collect::<Vec<_>>(),
            "finding_ids": attempt.finding_ids().collect::<Vec<_>>(),
        });
        let global_candidate = projection.global_candidate().map(|candidate| {
            serde_json::json!({
                "schema": candidate.schema(),
                "id": candidate.id(),
                "cover_id": candidate.cover_id(),
                "invariant_id": candidate.invariant_id(),
                "property_id": candidate.property_id(),
                "required_section_ids": candidate.required_section_ids().collect::<Vec<_>>(),
                "restriction_ids": candidate.restriction_ids().collect::<Vec<_>>(),
                "qualification_source_ids": candidate.qualification_source_ids().collect::<Vec<_>>(),
                "source_ids": candidate.source_ids().collect::<Vec<_>>(),
                "claim_ids": candidate.claim_ids().collect::<Vec<_>>(),
                "evidence_ids": candidate.evidence_ids().collect::<Vec<_>>(),
                "verification_ids": candidate.verification_ids().collect::<Vec<_>>(),
                "decision_ids": candidate.decision_ids().collect::<Vec<_>>(),
                "finding_ids": candidate.finding_ids().collect::<Vec<_>>(),
            })
        });
        let obstruction = projection.obstruction().map(|obstruction| {
            serde_json::json!({
                "schema": obstruction.schema(),
                "id": obstruction.id(),
                "attempt_id": obstruction.attempt_id(),
                "kind": obstruction.kind(),
                "conflicting_context_ids": obstruction.conflicting_context_ids().collect::<Vec<_>>(),
                "section_ids": obstruction.section_ids().collect::<Vec<_>>(),
                "overlap_member_ids": obstruction.overlap_member_ids().collect::<Vec<_>>(),
                "assignment_key": obstruction.assignment_key(),
                "left_assignment_value": obstruction.left_assignment_value(),
                "right_assignment_value": obstruction.right_assignment_value(),
                "source_ids": obstruction.source_ids().collect::<Vec<_>>(),
                "claim_ids": obstruction.claim_ids().collect::<Vec<_>>(),
                "evidence_ids": obstruction.evidence_ids().collect::<Vec<_>>(),
                "verification_ids": obstruction.verification_ids().collect::<Vec<_>>(),
                "decision_ids": obstruction.decision_ids().collect::<Vec<_>>(),
                "finding_ids": obstruction.finding_ids().collect::<Vec<_>>(),
                "affected_invariant_id": obstruction.affected_invariant_id(),
                "severity": obstruction.severity(),
                "required_resolution": obstruction.required_resolution(),
                "human_decision_required": obstruction.human_decision_required(),
                "blocks": obstruction.blocks().collect::<Vec<_>>(),
            })
        });
        serde_json::json!({
            "schema": projection.schema(),
            "cover": cover,
            "input_descriptor_ids": projection.input_descriptor_ids().collect::<Vec<_>>(),
            "sections": sections,
            "restrictions": restrictions,
            "attempt": attempt,
            "global_candidate": global_candidate,
            "obstruction": obstruction,
        })
    }

    fn assert_opaque_bundle_canonical_parity(bundle: &GluingBundleV4) {
        let projected =
            opaque_bundle_wire(crate::BorrowedGluingBundleProjectionV4 { value: bundle });
        let projected_bytes = crate::canonical_json(&projected).expect("projected canonical bytes");
        let durable_bytes = crate::canonical_json(bundle).expect("durable canonical bytes");
        assert_eq!(projected_bytes, durable_bytes);
        assert_eq!(
            crate::ContentHash::sha256(&projected_bytes),
            crate::ContentHash::sha256(&durable_bytes)
        );
    }

    #[test]
    fn compatibility_table_is_exhaustive() {
        use AssignmentCompatibilityV4::{Compatible, Conflict};
        use AssignmentValueV4::{Required, Satisfied};
        assert_eq!(Satisfied.compatibility(Satisfied), Compatible);
        assert_eq!(Required.compatibility(Required), Compatible);
        assert_eq!(Satisfied.compatibility(Required), Conflict);
        assert_eq!(Required.compatibility(Satisfied), Conflict);
        for x in [Satisfied, Required, AssignmentValueV4::Unknown] {
            assert_eq!(
                AssignmentValueV4::Unknown.compatibility(x),
                AssignmentCompatibilityV4::Unknown
            );
            assert_eq!(
                x.compatibility(AssignmentValueV4::Unknown),
                AssignmentCompatibilityV4::Unknown
            );
        }
    }

    #[test]
    fn current_verification_proof_comes_from_real_passed_m4_closure() {
        let (claim, assessment) = crate::m4::m5_test_passed_assessment();
        let obligation_id = claim.obligation_ids().first().unwrap();
        let proof = CurrentVerificationProofV4::derive(
            &assessment,
            &claim,
            obligation_id,
            assessment.snapshot_id(),
        )
        .unwrap();
        assert!(proof.passed);
        assert!(matches!(
            CurrentVerificationProofV4::derive(
                &assessment,
                &claim,
                obligation_id,
                &id("snapshot:other")
            ),
            Err(M5Error::SnapshotMismatch { .. })
        ));
    }

    #[test]
    fn ambiguity_counts_only_claims_with_their_exact_current_assessment() {
        let (claim, assessment) = crate::m4::m5_test_passed_assessment();
        let mut second_value = serde_json::to_value(&claim).unwrap();
        second_value["execution_id"] = serde_json::json!("execution:m5-unassessed");
        let second_identity = serde_json::json!({
            "assumptions": second_value["assumptions"].clone(),
            "execution_id": second_value["execution_id"].clone(),
            "obligation_ids": second_value["obligation_ids"].clone(),
            "polarity": second_value["polarity"].clone(),
            "property_id": second_value["property_id"].clone(),
            "requested_evidence": second_value["requested_evidence"].clone(),
            "source_ids": second_value["source_ids"].clone(),
            "summary": second_value["summary"].clone(),
            "target_refs": second_value["target_refs"].clone(),
        });
        second_value["id"] = serde_json::json!(format!(
            "claim:{}",
            crate::canonical_hash(&second_identity).unwrap()
        ));
        let second: ExecutionClaimV2 = serde_json::from_value(second_value).unwrap();
        let candidates = [(&claim, Some(&assessment)), (&second, None)];
        let eligible = candidates
            .iter()
            .filter(|(candidate, candidate_assessment)| {
                exact_current_assessment_for(
                    *candidate_assessment,
                    candidate,
                    assessment.run_id(),
                    assessment.snapshot_id(),
                    assessment.universe_id(),
                )
                .is_some()
            })
            .count();
        assert_eq!(eligible, 1);
        assert!(
            exact_current_assessment_for(
                Some(&assessment),
                &second,
                assessment.run_id(),
                assessment.snapshot_id(),
                assessment.universe_id(),
            )
            .is_none()
        );
    }

    fn profile_chosen_context(
        context: &str,
        evidence_ids: BTreeSet<StableId>,
    ) -> M5ChosenContextClosureV4 {
        M5ChosenContextClosureV4 {
            context_id: id(context),
            context_member_ids: BTreeSet::from([id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID)]),
            obligation_id: id(&format!("obligation:{context}")),
            claim_id: id(&format!("claim:{context}")),
            claim_source_ids: BTreeSet::new(),
            binding_ids: BTreeSet::new(),
            evidence_ids,
            verification_ids: BTreeSet::new(),
            decision_ids: BTreeSet::new(),
            finding_ids: BTreeSet::new(),
            verification_passed: false,
        }
    }

    fn runtime_projection_closure(
        chosen_contexts: Vec<M5ChosenContextClosureV4>,
    ) -> M5RegistrationClosureV4 {
        let (cover, _, _, overlap_member_ids) = base();
        M5RegistrationClosureV4 {
            cover,
            overlap_member_ids,
            chosen_contexts,
        }
    }

    #[test]
    fn runtime_profile_qualifications_preserve_missing_and_empty_assessment_semantics() {
        let overlap = id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID);
        for chosen_contexts in [
            Vec::new(),
            vec![profile_chosen_context(
                DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
                BTreeSet::new(),
            )],
            vec![profile_chosen_context(
                DOUBLE_SUBMIT_UI_CONTEXT_ID,
                BTreeSet::from([id("evidence:ui-is-not-a-qualification")]),
            )],
        ] {
            let (_, qualifications) = runtime_projection_closure(chosen_contexts)
                .runtime_profile_projection()
                .expect("missing/empty assessment remains descriptor-eligible");
            assert_eq!(qualifications, BTreeSet::from([overlap.clone()]));
        }

        let payment_evidence = id("evidence:payment-current");
        let (_, qualifications) = runtime_projection_closure(vec![profile_chosen_context(
            DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
            BTreeSet::from([payment_evidence.clone()]),
        )])
        .runtime_profile_projection()
        .expect("payment evidence qualification");
        assert_eq!(qualifications, BTreeSet::from([overlap, payment_evidence]));
    }

    #[test]
    fn runtime_profile_projection_refuses_each_context_ambiguity() {
        for context in [
            DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
            DOUBLE_SUBMIT_UI_CONTEXT_ID,
        ] {
            let closure = runtime_projection_closure(vec![
                profile_chosen_context(context, BTreeSet::new()),
                profile_chosen_context(context, BTreeSet::new()),
            ]);
            assert!(matches!(
                closure.runtime_profile_projection(),
                Err(M5Error::Validation(message)) if message.contains("ambiguous")
            ));
        }
    }

    #[test]
    fn runtime_profile_source_bound_checks_exact_plus_one_and_arithmetic_overflow() {
        let exact = ids("source", MAX_M5_PROFILE_SOURCE_IDS);
        let exact_admission = preflight_profile_source_union(&[&exact], &[])
            .expect("exact runtime profile source bound");
        assert_eq!(exact_admission.count, MAX_M5_PROFILE_SOURCE_IDS);

        let over = ids("source", MAX_M5_PROFILE_SOURCE_IDS + 1);
        assert!(matches!(
            preflight_profile_source_union(&[&over], &[]),
            Err(M5Error::Incomplete { observed, .. })
                if observed == MAX_M5_PROFILE_SOURCE_IDS + 1
        ));

        let existing = [
            id("gluing-input-descriptor-v4:payment"),
            id("registration-v4:payment"),
            id("gluing-input-descriptor-v4:ui"),
            id("registration-v4:ui"),
        ];
        let existing_refs = existing.iter().collect::<Vec<_>>();
        let mut exact_with_existing = ids("source", MAX_M5_PROFILE_SOURCE_IDS - existing.len());
        extend_runtime_profile_sources(&mut exact_with_existing, &existing_refs)
            .expect("exact final two-prefix source bound");
        assert_eq!(exact_with_existing.len(), MAX_M5_PROFILE_SOURCE_IDS);

        let mut over_with_existing = ids("source", MAX_M5_PROFILE_SOURCE_IDS - existing.len() + 1);
        let preflight_len = over_with_existing.len();
        assert!(matches!(
            extend_runtime_profile_sources(&mut over_with_existing, &existing_refs),
            Err(M5Error::Incomplete { .. })
        ));
        assert_eq!(over_with_existing.len(), preflight_len);

        let sample = id("source:overflow");
        let mut count_overflow = UnionAdmission {
            count: usize::MAX,
            retained_bytes: 0,
        };
        assert!(matches!(
            admit_profile_source_id(&mut count_overflow, &sample),
            Err(M5Error::Incomplete {
                observed: usize::MAX,
                ..
            })
        ));
        let mut byte_overflow = UnionAdmission {
            count: 0,
            retained_bytes: u64::MAX,
        };
        assert!(matches!(
            admit_profile_source_id(&mut byte_overflow, &sample),
            Err(M5Error::Incomplete {
                observed: usize::MAX,
                ..
            })
        ));

        let retained = std::mem::size_of::<StableId>() + sample.as_str().len();
        let mut exact_bytes = UnionAdmission {
            count: 0,
            retained_bytes: u64::try_from(MAX_M5_PROFILE_SOURCE_RETAINED_BYTES - retained).unwrap(),
        };
        admit_profile_source_id(&mut exact_bytes, &sample).expect("exact retained-byte boundary");
        assert_eq!(
            exact_bytes.retained_bytes,
            MAX_M5_PROFILE_SOURCE_RETAINED_BYTES as u64
        );
        let mut plus_one_byte = UnionAdmission {
            count: 0,
            retained_bytes: u64::try_from(MAX_M5_PROFILE_SOURCE_RETAINED_BYTES - retained + 1)
                .unwrap(),
        };
        assert!(matches!(
            admit_profile_source_id(&mut plus_one_byte, &sample),
            Err(M5Error::Incomplete { .. })
        ));
    }

    #[test]
    fn cover_partition_and_ids_are_deterministic() {
        let (a, _, _, _) = base();
        let (b, _, _, _) = base();
        assert_eq!(a, b);
        assert_eq!(a.id().kind(), "context-cover-v4");
        assert!(a.covered_domain_ids().is_disjoint(a.uncovered_domain_ids()));
        assert_eq!(
            a.covered_domain_ids()
                .union(a.uncovered_domain_ids())
                .cloned()
                .collect::<BTreeSet<_>>(),
            *a.cover_domain_ids()
        );
    }

    #[test]
    fn descriptor_identity_golden_and_strict_decode_reject_tamper() {
        let (_, descriptors, _, _) = base();
        let descriptor = &descriptors[0];
        let identity_bytes = crate::canonical_json(&DescriptorIdentity {
            assignment_key: &descriptor.assignment_key,
            assignment_value: descriptor.assignment_value,
            context_id: &descriptor.context_id,
            plan_id: &descriptor.plan_id,
            profile_descriptor_id: &descriptor.profile_descriptor_id,
            qualification_source_ids: &descriptor.qualification_source_ids,
            run_id: &descriptor.run_id,
            snapshot_id: &descriptor.snapshot_id,
            universe_id: &descriptor.universe_id,
        })
        .unwrap();
        assert_eq!(identity_bytes, br#"{"assignment_key":"caller_duplicate_protection","assignment_value":"required","context_id":"context:payment","plan_id":"plan:m5","profile_descriptor_id":"reviewgraphen.double_submit_gluing@1","qualification_source_ids":[],"run_id":"run:m5","snapshot_id":"snapshot:m5","universe_id":"universe:m5"}"#);
        assert_eq!(
            descriptor.id.as_str(),
            "gluing-input-descriptor-v4:sha256:3fd10af016a893a59b76e681d3553da23509ee359af515004288f5f239361982"
        );
        let bytes = crate::canonical_json(descriptor).unwrap();
        assert_eq!(
            GluingInputDescriptorV4::from_json_bytes(&bytes).unwrap(),
            *descriptor
        );

        let mut unknown = serde_json::to_value(descriptor).unwrap();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), serde_json::json!(true));
        assert!(
            GluingInputDescriptorV4::from_json_bytes(&crate::canonical_json(&unknown).unwrap())
                .is_err()
        );
        let mut tampered = serde_json::to_value(descriptor).unwrap();
        tampered["id"] = serde_json::json!("gluing-input-descriptor-v4:sha256:00000000");
        assert!(
            GluingInputDescriptorV4::from_json_bytes(&crate::canonical_json(&tampered).unwrap())
                .is_err()
        );
        let mut duplicated = serde_json::to_value(descriptor).unwrap();
        duplicated["qualification_source_ids"] = serde_json::json!(["source:a", "source:a"]);
        assert!(
            GluingInputDescriptorV4::from_json_bytes(&crate::canonical_json(&duplicated).unwrap())
                .is_err()
        );
        let mut noncanonical = bytes.clone();
        noncanonical.push(b' ');
        assert!(GluingInputDescriptorV4::from_json_bytes(&noncanonical).is_err());
    }

    #[test]
    fn bundle_strict_decode_rejects_positional_order_and_identity_tamper() {
        let (cover, descriptors, regs, overlap) = base();
        let sections = vec![
            section(&cover, &descriptors[0], regs[0].clone()),
            section(&cover, &descriptors[1], regs[1].clone()),
        ];
        let bundle = GluingBundleV4::derive(cover, descriptors, regs, sections, overlap).unwrap();
        let bytes = crate::canonical_json(&bundle).unwrap();
        let source = ValidatedM5SourceV4 {
            bundle: bundle.clone(),
        };
        assert_eq!(
            GluingBundleV4::from_json_bytes(&bytes, &source).unwrap(),
            bundle
        );
        let mut reordered = serde_json::to_value(&bundle).unwrap();
        reordered["sections"].as_array_mut().unwrap().reverse();
        assert!(
            GluingBundleV4::from_json_bytes(&crate::canonical_json(&reordered).unwrap(), &source,)
                .is_err()
        );
        let mut tampered = serde_json::to_value(&bundle).unwrap();
        tampered["attempt"]["id"] = serde_json::json!("gluing-attempt-v4:sha256:00000000");
        assert!(
            GluingBundleV4::from_json_bytes(&crate::canonical_json(&tampered).unwrap(), &source,)
                .is_err()
        );
        let mut extra_source = serde_json::to_value(&bundle).unwrap();
        extra_source["attempt"]["source_ids"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("artifact:not-retained"));
        assert!(
            GluingBundleV4::from_json_bytes(
                &crate::canonical_json(&extra_source).unwrap(),
                &source,
            )
            .is_err()
        );
    }

    #[test]
    fn qualifications_are_context_local_and_missing_sections_allow_only_cover_domain() {
        let (cover, _, regs, _) = base();
        let payment_qualifier = id("evidence:ui-only");
        let descriptors = [
            GluingInputDescriptorV4::new(
                cover.run_id().clone(),
                cover.snapshot_id().clone(),
                cover.universe_id().clone(),
                cover.plan_id().clone(),
                id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
                AssignmentValueV4::Required,
                BTreeSet::from([payment_qualifier.clone()]),
            )
            .unwrap(),
            GluingInputDescriptorV4::new(
                cover.run_id().clone(),
                cover.snapshot_id().clone(),
                cover.universe_id().clone(),
                cover.plan_id().clone(),
                id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
                AssignmentValueV4::Required,
                BTreeSet::new(),
            )
            .unwrap(),
        ];
        let members = BTreeSet::from([id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID)]);
        let payment = SectionV4::derive_unverified(
            &cover,
            &descriptors[0],
            regs[0].clone(),
            id("obligation:payment"),
            id("claim:payment-local"),
            &members,
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        )
        .unwrap();
        let ui = SectionV4::derive_unverified(
            &cover,
            &descriptors[1],
            regs[1].clone(),
            id("obligation:ui"),
            id("claim:ui-local"),
            &members,
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::from([payment_qualifier]),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
        )
        .unwrap();
        assert!(
            validate_context_qualifications(
                &cover,
                [&descriptors[0], &descriptors[1]],
                &[payment, ui],
            )
            .is_err()
        );

        let domain_descriptor = GluingInputDescriptorV4::new(
            cover.run_id().clone(),
            cover.snapshot_id().clone(),
            cover.universe_id().clone(),
            cover.plan_id().clone(),
            id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
            AssignmentValueV4::Required,
            BTreeSet::from([id("function:payment")]),
        )
        .unwrap();
        let empty_ui = GluingInputDescriptorV4::new(
            cover.run_id().clone(),
            cover.snapshot_id().clone(),
            cover.universe_id().clone(),
            cover.plan_id().clone(),
            id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
            AssignmentValueV4::Required,
            BTreeSet::new(),
        )
        .unwrap();
        assert!(
            validate_context_qualifications(&cover, [&domain_descriptor, &empty_ui], &[],).is_ok()
        );

        let outside_descriptor = GluingInputDescriptorV4::new(
            cover.run_id().clone(),
            cover.snapshot_id().clone(),
            cover.universe_id().clone(),
            cover.plan_id().clone(),
            id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
            AssignmentValueV4::Required,
            BTreeSet::from([id("evidence:no-section")]),
        )
        .unwrap();
        assert!(
            validate_context_qualifications(&cover, [&outside_descriptor, &empty_ui], &[],)
                .is_err()
        );
    }

    #[test]
    fn allocation_free_wire_preflight_enforces_exact_array_and_string_seams() {
        let exact_sections = br#"{"sections":[{},{}]}"#;
        assert!(
            preflight_wire_json(
                exact_sections,
                MAX_M5_BUNDLE_CANONICAL_BYTES,
                "test bundle scanner",
                WireShape::Bundle,
            )
            .is_ok()
        );
        let over_sections = br#"{"sections":[{},{},{}]}"#;
        assert!(matches!(
            preflight_wire_json(
                over_sections,
                MAX_M5_BUNDLE_CANONICAL_BYTES,
                "test bundle scanner",
                WireShape::Bundle,
            ),
            Err(M5Error::Incomplete { observed: 3, .. })
        ));

        let exact_string = format!("{{\"schema\":\"{}\"}}", "x".repeat(MAX_M5_STABLE_ID_BYTES));
        assert!(
            preflight_wire_json(
                exact_string.as_bytes(),
                MAX_M5_DESCRIPTOR_CANONICAL_BYTES,
                "test descriptor scanner",
                WireShape::Descriptor,
            )
            .is_ok()
        );
        let over_string = format!(
            "{{\"schema\":\"{}\"}}",
            "x".repeat(MAX_M5_STABLE_ID_BYTES + 1)
        );
        assert!(matches!(
            preflight_wire_json(
                over_string.as_bytes(),
                MAX_M5_DESCRIPTOR_CANONICAL_BYTES,
                "test descriptor scanner",
                WireShape::Descriptor,
            ),
            Err(M5Error::Incomplete { observed, .. }) if observed == MAX_M5_STABLE_ID_BYTES + 1
        ));
    }

    #[test]
    fn missing_section_unknown_and_conflict_are_typed_obstructions() {
        let (cover, descriptors, regs, overlap) = base();
        let one = section(&cover, &descriptors[0], regs[0].clone());
        let b = GluingBundleV4::derive(cover, descriptors, regs, vec![one], overlap).unwrap();
        assert_eq!(b.attempt().result(), GluingResultV4::Unknown);
        assert_eq!(
            b.obstruction().unwrap().kind(),
            GluingObstructionKindV4::RequiredSectionMissing
        );

        let (cover, mut descriptors, regs, overlap) = base();
        descriptors[1] = GluingInputDescriptorV4::new(
            cover.run_id().clone(),
            cover.snapshot_id().clone(),
            cover.universe_id().clone(),
            cover.plan_id().clone(),
            id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
            AssignmentValueV4::Unknown,
            BTreeSet::new(),
        )
        .unwrap();
        let sections = vec![
            section(&cover, &descriptors[0], regs[0].clone()),
            section(&cover, &descriptors[1], regs[1].clone()),
        ];
        let b = GluingBundleV4::derive(cover, descriptors, regs, sections, overlap).unwrap();
        assert_eq!(
            b.obstruction().unwrap().kind(),
            GluingObstructionKindV4::SectionUnknown
        );

        let (cover, mut descriptors, regs, overlap) = base();
        descriptors[1] = GluingInputDescriptorV4::new(
            cover.run_id().clone(),
            cover.snapshot_id().clone(),
            cover.universe_id().clone(),
            cover.plan_id().clone(),
            id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
            AssignmentValueV4::Satisfied,
            BTreeSet::new(),
        )
        .unwrap();
        let sections = vec![
            section(&cover, &descriptors[0], regs[0].clone()),
            section(&cover, &descriptors[1], regs[1].clone()),
        ];
        let b = GluingBundleV4::derive(cover, descriptors, regs, sections, overlap).unwrap();
        assert_eq!(b.attempt().result(), GluingResultV4::Failed);
        assert!(b.obstruction().unwrap().human_decision_required());
    }

    #[test]
    fn unverified_compatible_sections_are_candidate_not_glued() {
        let (cover, descriptors, regs, overlap) = base();
        let sections = vec![
            section(&cover, &descriptors[0], regs[0].clone()),
            section(&cover, &descriptors[1], regs[1].clone()),
        ];
        let b = GluingBundleV4::derive(cover, descriptors, regs, sections, overlap).unwrap();
        assert_eq!(b.attempt().result(), GluingResultV4::Candidate);
        assert!(b.global_candidate().is_some());
        assert!(b.obstruction().is_none());
    }

    #[test]
    fn exact_result_matrix_covers_overlap_unknown_conflict_and_verified_compatible_rows() {
        fn derive_case(
            left: AssignmentValueV4,
            right: AssignmentValueV4,
            passed: bool,
            qualified: bool,
            anchor_present: bool,
        ) -> GluingBundleV4 {
            let (cover, _, regs, mut overlap) = base();
            if !anchor_present {
                overlap.remove(&id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID));
            }
            let qualification_source_ids = if qualified {
                BTreeSet::from([id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID)])
            } else {
                BTreeSet::new()
            };
            let descriptors = [
                GluingInputDescriptorV4::new(
                    cover.run_id().clone(),
                    cover.snapshot_id().clone(),
                    cover.universe_id().clone(),
                    cover.plan_id().clone(),
                    id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
                    left,
                    qualification_source_ids,
                )
                .unwrap(),
                GluingInputDescriptorV4::new(
                    cover.run_id().clone(),
                    cover.snapshot_id().clone(),
                    cover.universe_id().clone(),
                    cover.plan_id().clone(),
                    id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
                    right,
                    BTreeSet::new(),
                )
                .unwrap(),
            ];
            let mut sections = vec![
                section(&cover, &descriptors[0], regs[0].clone()),
                section(&cover, &descriptors[1], regs[1].clone()),
            ];
            for section in &mut sections {
                section.passed_current_verification = passed;
            }
            GluingBundleV4::derive(cover, descriptors, regs, sections, overlap).unwrap()
        }

        for retained_position in [None, Some(0_usize), Some(1_usize)] {
            let (cover, descriptors, regs, overlap) = base();
            let sections = retained_position
                .map(|position| {
                    vec![section(
                        &cover,
                        &descriptors[position],
                        regs[position].clone(),
                    )]
                })
                .unwrap_or_default();
            let missing = GluingBundleV4::derive(cover, descriptors, regs, sections, overlap)
                .expect("missing assessment/Section result");
            assert_eq!(missing.attempt().result(), GluingResultV4::Unknown);
            assert_eq!(
                missing.obstruction().unwrap().kind(),
                GluingObstructionKindV4::RequiredSectionMissing
            );
        }

        let missing_overlap = derive_case(
            AssignmentValueV4::Required,
            AssignmentValueV4::Required,
            true,
            false,
            false,
        );
        assert_eq!(missing_overlap.attempt().result(), GluingResultV4::Unknown);
        assert_eq!(
            missing_overlap.obstruction().unwrap().kind(),
            GluingObstructionKindV4::RequiredOverlapMissing
        );

        for (left, right) in [
            (AssignmentValueV4::Unknown, AssignmentValueV4::Satisfied),
            (AssignmentValueV4::Unknown, AssignmentValueV4::Required),
            (AssignmentValueV4::Unknown, AssignmentValueV4::Unknown),
            (AssignmentValueV4::Satisfied, AssignmentValueV4::Unknown),
            (AssignmentValueV4::Required, AssignmentValueV4::Unknown),
        ] {
            let bundle = derive_case(left, right, true, false, true);
            assert_eq!(bundle.attempt().result(), GluingResultV4::Unknown);
            assert_eq!(
                bundle.obstruction().unwrap().kind(),
                GluingObstructionKindV4::SectionUnknown
            );
        }

        for (left, right) in [
            (AssignmentValueV4::Satisfied, AssignmentValueV4::Required),
            (AssignmentValueV4::Required, AssignmentValueV4::Satisfied),
        ] {
            let bundle = derive_case(left, right, true, false, true);
            assert_eq!(bundle.attempt().result(), GluingResultV4::Failed);
            assert_eq!(
                bundle.obstruction().unwrap().kind(),
                GluingObstructionKindV4::AssignmentConflict
            );
        }

        for (qualified, expected) in [
            (false, GluingResultV4::Glued),
            (true, GluingResultV4::GluedWithQualification),
        ] {
            let bundle = derive_case(
                AssignmentValueV4::Required,
                AssignmentValueV4::Required,
                true,
                qualified,
                true,
            );
            assert_eq!(bundle.attempt().result(), expected);
            assert!(bundle.global_candidate().is_some());
            assert!(bundle.obstruction().is_none());
        }
    }

    #[test]
    fn opaque_bundle_projection_matches_complete_canonical_bytes_for_success_conflict_and_unknown()
    {
        let (cover, mut descriptors, registrations, overlap) = base();
        descriptors[1] = GluingInputDescriptorV4::new(
            cover.run_id().clone(),
            cover.snapshot_id().clone(),
            cover.universe_id().clone(),
            cover.plan_id().clone(),
            id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
            AssignmentValueV4::Unknown,
            BTreeSet::new(),
        )
        .expect("unknown descriptor");
        let unknown = GluingBundleV4::derive(
            cover.clone(),
            descriptors,
            registrations.clone(),
            vec![
                section(
                    &cover,
                    &GluingInputDescriptorV4::new(
                        cover.run_id().clone(),
                        cover.snapshot_id().clone(),
                        cover.universe_id().clone(),
                        cover.plan_id().clone(),
                        id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
                        AssignmentValueV4::Required,
                        BTreeSet::new(),
                    )
                    .expect("payment descriptor"),
                    registrations[0].clone(),
                ),
                section(
                    &cover,
                    &GluingInputDescriptorV4::new(
                        cover.run_id().clone(),
                        cover.snapshot_id().clone(),
                        cover.universe_id().clone(),
                        cover.plan_id().clone(),
                        id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
                        AssignmentValueV4::Unknown,
                        BTreeSet::new(),
                    )
                    .expect("unknown descriptor"),
                    registrations[1].clone(),
                ),
            ],
            overlap.clone(),
        )
        .expect("unknown bundle");
        assert_eq!(unknown.attempt().result(), GluingResultV4::Unknown);

        let (cover, mut descriptors, registrations, overlap) = base();
        descriptors[1] = GluingInputDescriptorV4::new(
            cover.run_id().clone(),
            cover.snapshot_id().clone(),
            cover.universe_id().clone(),
            cover.plan_id().clone(),
            id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
            AssignmentValueV4::Satisfied,
            BTreeSet::new(),
        )
        .expect("conflict descriptor");
        let conflict = GluingBundleV4::derive(
            cover.clone(),
            descriptors,
            registrations.clone(),
            vec![
                section(
                    &cover,
                    &GluingInputDescriptorV4::new(
                        cover.run_id().clone(),
                        cover.snapshot_id().clone(),
                        cover.universe_id().clone(),
                        cover.plan_id().clone(),
                        id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
                        AssignmentValueV4::Required,
                        BTreeSet::new(),
                    )
                    .expect("payment descriptor"),
                    registrations[0].clone(),
                ),
                section(
                    &cover,
                    &GluingInputDescriptorV4::new(
                        cover.run_id().clone(),
                        cover.snapshot_id().clone(),
                        cover.universe_id().clone(),
                        cover.plan_id().clone(),
                        id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
                        AssignmentValueV4::Satisfied,
                        BTreeSet::new(),
                    )
                    .expect("conflict descriptor"),
                    registrations[1].clone(),
                ),
            ],
            overlap.clone(),
        )
        .expect("conflict bundle");
        assert_eq!(conflict.attempt().result(), GluingResultV4::Failed);

        let (cover, descriptors, registrations, overlap) = base();
        let mut sections = vec![
            section(&cover, &descriptors[0], registrations[0].clone()),
            section(&cover, &descriptors[1], registrations[1].clone()),
        ];
        for section in &mut sections {
            section.passed_current_verification = true;
        }
        let success = GluingBundleV4::derive(cover, descriptors, registrations, sections, overlap)
            .expect("success bundle");
        assert_eq!(success.attempt().result(), GluingResultV4::Glued);

        for bundle in [&unknown, &conflict, &success] {
            assert_opaque_bundle_canonical_parity(bundle);
        }
    }

    #[test]
    fn cross_snapshot_is_refused_instead_of_becoming_stale() {
        let (cover, _, _, _) = base();
        let d = GluingInputDescriptorV4::new(
            cover.run_id().clone(),
            id("snapshot:other"),
            cover.universe_id().clone(),
            cover.plan_id().clone(),
            id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
            AssignmentValueV4::Required,
            BTreeSet::new(),
        )
        .unwrap();
        assert!(matches!(
            SectionV4::derive_unverified(
                &cover,
                &d,
                id("registration-v4:x"),
                id("obligation:payment"),
                id("claim:x"),
                &BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new()
            ),
            Err(M5Error::SnapshotMismatch { .. })
        ));
    }

    #[test]
    fn inclusive_collection_limits_and_plus_one_refusals() {
        let run = id("run:x");
        let snap = id("snapshot:x");
        let uni = id("universe:x");
        let plan = id("plan:x");
        assert!(
            GluingInputDescriptorV4::new(
                run.clone(),
                snap.clone(),
                uni.clone(),
                plan.clone(),
                id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
                AssignmentValueV4::Required,
                ids("source", MAX_M5_DESCRIPTOR_QUALIFICATION_IDS)
            )
            .is_ok()
        );
        assert!(
            matches!(GluingInputDescriptorV4::new(run.clone(),snap.clone(),uni.clone(),plan.clone(),id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),AssignmentValueV4::Required,ids("source",MAX_M5_DESCRIPTOR_QUALIFICATION_IDS+1)),Err(M5Error::Incomplete{observed,..}) if observed==MAX_M5_DESCRIPTOR_QUALIFICATION_IDS+1)
        );
        let selected = ids("obligation", MAX_M5_SELECTED_OBLIGATIONS);
        assert!(
            ContextCoverV4::derive(
                run.clone(),
                snap.clone(),
                uni.clone(),
                plan.clone(),
                selected,
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeSet::new()
            )
            .is_ok()
        );
        assert!(
            matches!(ContextCoverV4::derive(run,snap,uni,plan,ids("obligation",MAX_M5_SELECTED_OBLIGATIONS+1),BTreeSet::new(),BTreeSet::new(),BTreeSet::new(),BTreeSet::new()),Err(M5Error::Incomplete{observed,..}) if observed==MAX_M5_SELECTED_OBLIGATIONS+1)
        );
    }

    #[test]
    fn byte_and_checked_arithmetic_seams_accept_exact_and_refuse_next_or_overflow() {
        assert_eq!(
            checked_peak_add(
                (MAX_M5_BUNDLE_CANONICAL_BYTES - 1) as u64,
                1,
                MAX_M5_BUNDLE_CANONICAL_BYTES,
                "test peak",
            )
            .unwrap(),
            MAX_M5_BUNDLE_CANONICAL_BYTES as u64
        );
        assert!(matches!(
            checked_peak_add(
                MAX_M5_BUNDLE_CANONICAL_BYTES as u64,
                1,
                MAX_M5_BUNDLE_CANONICAL_BYTES,
                "test peak",
            ),
            Err(M5Error::Incomplete { observed, .. }) if observed == MAX_M5_BUNDLE_CANONICAL_BYTES + 1
        ));
        assert!(matches!(
            checked_peak_add(u64::MAX, 1, usize::MAX, "test overflow"),
            Err(M5Error::Incomplete {
                observed: usize::MAX,
                ..
            })
        ));

        let exact_bytes = vec![b'0'; MAX_M5_DESCRIPTOR_CANONICAL_BYTES];
        assert_eq!(exact_bytes.len(), MAX_M5_DESCRIPTOR_CANONICAL_BYTES);
        preflight_wire_json(
            &exact_bytes,
            MAX_M5_DESCRIPTOR_CANONICAL_BYTES,
            "test canonical seam",
            WireShape::Scalar,
        )
        .unwrap();
        let over_bytes = vec![b'0'; MAX_M5_DESCRIPTOR_CANONICAL_BYTES + 1];
        assert!(matches!(
            preflight_wire_json(
                &over_bytes,
                MAX_M5_DESCRIPTOR_CANONICAL_BYTES,
                "test canonical seam",
                WireShape::Scalar,
            ),
            Err(M5Error::Incomplete { observed, .. }) if observed == MAX_M5_DESCRIPTOR_CANONICAL_BYTES + 1
        ));

        let exact_run = id(&format!("run:{}", "x".repeat(MAX_M5_STABLE_ID_BYTES - 4)));
        assert_eq!(exact_run.as_str().len(), MAX_M5_STABLE_ID_BYTES);
        assert!(
            GluingInputDescriptorV4::new(
                exact_run,
                id("snapshot:id-bound"),
                id("universe:id-bound"),
                id("plan:id-bound"),
                id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
                AssignmentValueV4::Required,
                BTreeSet::new(),
            )
            .is_ok()
        );
        let over_run = id(&format!("run:{}", "x".repeat(MAX_M5_STABLE_ID_BYTES - 3)));
        assert!(matches!(
            GluingInputDescriptorV4::new(
                over_run, id("snapshot:id-bound"), id("universe:id-bound"), id("plan:id-bound"),
                id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID), AssignmentValueV4::Required, BTreeSet::new(),
            ),
            Err(M5Error::Incomplete { observed, .. }) if observed == MAX_M5_STABLE_ID_BYTES + 1
        ));
    }

    #[test]
    fn cover_member_domain_and_source_limits_are_exact() {
        let run = id("run:cover-bounds");
        let snap = id("snapshot:cover-bounds");
        let uni = id("universe:cover-bounds");
        let plan = id("plan:cover-bounds");
        let selected = BTreeSet::from([id("obligation:cover-bounds")]);
        let exact_members = ids("function", MAX_M5_CONTEXT_MEMBER_IDS);
        assert!(
            ContextCoverV4::derive(
                run.clone(),
                snap.clone(),
                uni.clone(),
                plan.clone(),
                selected.clone(),
                BTreeSet::new(),
                exact_members,
                BTreeSet::new(),
                BTreeSet::new(),
            )
            .is_ok()
        );
        assert!(matches!(
            ContextCoverV4::derive(
                run.clone(), snap.clone(), uni.clone(), plan.clone(), selected.clone(),
                BTreeSet::new(), ids("function", MAX_M5_CONTEXT_MEMBER_IDS + 1),
                BTreeSet::new(), BTreeSet::new(),
            ),
            Err(M5Error::Incomplete { observed, .. }) if observed == MAX_M5_CONTEXT_MEMBER_IDS + 1
        ));

        let exact_domain = ids("artifact", MAX_M5_COVER_DOMAIN_IDS);
        let cover = ContextCoverV4::derive(
            run.clone(),
            snap.clone(),
            uni.clone(),
            plan.clone(),
            selected.clone(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            exact_domain,
        )
        .unwrap();
        assert_eq!(cover.cover_domain_ids.len(), MAX_M5_COVER_DOMAIN_IDS);
        assert_eq!(cover.source_ids.len(), MAX_M5_COVER_SOURCE_IDS);
        assert!(matches!(
            ContextCoverV4::derive(
                run, snap, uni, plan, selected, BTreeSet::new(), BTreeSet::new(),
                BTreeSet::new(), ids("artifact", MAX_M5_COVER_DOMAIN_IDS + 1),
            ),
            Err(M5Error::Incomplete { observed, .. }) if observed == MAX_M5_COVER_DOMAIN_IDS + 1
        ));
    }

    #[test]
    fn cover_domain_plus_one_refuses_before_target_union_allocation() {
        let run = id("run:domain-allocation-seam");
        let snapshot = id("snapshot:domain-allocation-seam");
        let universe = id("universe:domain-allocation-seam");
        let plan = id("plan:domain-allocation-seam");
        let selected = BTreeSet::from([id("obligation:domain-allocation-seam")]);

        M5_COVER_DOMAIN_TARGET_ALLOCATIONS.with(|count| count.set(0));
        let exact = ContextCoverV4::derive(
            run.clone(),
            snapshot.clone(),
            universe.clone(),
            plan.clone(),
            selected.clone(),
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            ids("artifact", MAX_M5_COVER_DOMAIN_IDS),
        );
        assert!(exact.is_ok());
        assert_eq!(
            M5_COVER_DOMAIN_TARGET_ALLOCATIONS.with(std::cell::Cell::get),
            1
        );

        M5_COVER_DOMAIN_TARGET_ALLOCATIONS.with(|count| count.set(0));
        let over = ContextCoverV4::derive(
            run,
            snapshot,
            universe,
            plan,
            selected,
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::new(),
            ids("artifact", MAX_M5_COVER_DOMAIN_IDS + 1),
        );
        assert!(matches!(
            over,
            Err(M5Error::Incomplete { observed, .. }) if observed == MAX_M5_COVER_DOMAIN_IDS + 1
        ));
        assert_eq!(
            M5_COVER_DOMAIN_TARGET_ALLOCATIONS.with(std::cell::Cell::get),
            0
        );
    }

    #[test]
    fn obstruction_source_plus_one_refuses_before_target_union_allocation() {
        let addition = id("context:obstruction-allocation-seam");
        let overlap = BTreeSet::new();

        M5_OBSTRUCTION_SOURCE_TARGET_ALLOCATIONS.with(|count| count.set(0));
        let exact_attempt = ids("artifact", MAX_M5_ATTEMPT_SOURCE_IDS - 1);
        let exact =
            build_obstruction_sources(&exact_attempt, std::slice::from_ref(&addition), &overlap)
                .unwrap();
        assert_eq!(exact.len(), MAX_M5_ATTEMPT_SOURCE_IDS);
        assert_eq!(
            M5_OBSTRUCTION_SOURCE_TARGET_ALLOCATIONS.with(std::cell::Cell::get),
            1
        );

        M5_OBSTRUCTION_SOURCE_TARGET_ALLOCATIONS.with(|count| count.set(0));
        let full_attempt = ids("artifact", MAX_M5_ATTEMPT_SOURCE_IDS);
        assert!(matches!(
            build_obstruction_sources(
                &full_attempt,
                std::slice::from_ref(&addition),
                &overlap,
            ),
            Err(M5Error::Incomplete { observed, .. }) if observed == MAX_M5_ATTEMPT_SOURCE_IDS + 1
        ));
        assert_eq!(
            M5_OBSTRUCTION_SOURCE_TARGET_ALLOCATIONS.with(std::cell::Cell::get),
            0
        );
    }

    #[test]
    fn section_restriction_and_attempt_joint_maxima_are_reachable() {
        let run = id("run:joint-max");
        let snap = id("snapshot:joint-max");
        let uni = id("universe:joint-max");
        let plan = id("plan:joint-max");
        let mut overlap = ids("overlap", 3_967);
        overlap.insert(id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID));
        let payment_sources = ids("payment-source", MAX_M5_CLAIM_SOURCE_IDS);
        let ui_sources = ids("ui-source", MAX_M5_CLAIM_SOURCE_IDS);
        let mut payment_members = overlap.clone();
        payment_members.extend(payment_sources.iter().cloned());
        let mut ui_members = overlap.clone();
        ui_members.extend(ui_sources.iter().cloned());
        assert_eq!(payment_members.len(), MAX_M5_CONTEXT_MEMBER_IDS);
        assert_eq!(ui_members.len(), MAX_M5_CONTEXT_MEMBER_IDS);
        let cover = ContextCoverV4::derive(
            run.clone(),
            snap.clone(),
            uni.clone(),
            plan.clone(),
            BTreeSet::from([id("obligation:payment"), id("obligation:ui")]),
            BTreeSet::new(),
            payment_members.clone(),
            ui_members.clone(),
            BTreeSet::new(),
        )
        .unwrap();
        let descriptors = [
            GluingInputDescriptorV4::new(
                run.clone(),
                snap.clone(),
                uni.clone(),
                plan.clone(),
                id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
                AssignmentValueV4::Required,
                ids("payment-qualification", MAX_M5_DESCRIPTOR_QUALIFICATION_IDS),
            )
            .unwrap(),
            GluingInputDescriptorV4::new(
                run,
                snap,
                uni,
                plan,
                id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
                AssignmentValueV4::Required,
                ids("ui-qualification", MAX_M5_DESCRIPTOR_QUALIFICATION_IDS),
            )
            .unwrap(),
        ];
        let regs = [
            id("registration-v4:payment-max"),
            id("registration-v4:ui-max"),
        ];
        let make = |index: usize,
                    context_members: &BTreeSet<StableId>,
                    claim_sources: BTreeSet<StableId>|
         -> SectionV4 {
            let side = if index == 0 { "payment" } else { "ui" };
            SectionV4::derive_unverified(
                &cover,
                &descriptors[index],
                regs[index].clone(),
                id(&format!("obligation:{side}")),
                id(&format!("claim:{side}")),
                context_members,
                claim_sources,
                ids(&format!("binding-{side}"), MAX_M5_SECTION_TRACE_IDS),
                ids(&format!("evidence-{side}"), MAX_M5_SECTION_TRACE_IDS),
                ids(&format!("verification-v3-{side}"), MAX_M5_SECTION_TRACE_IDS),
                ids(&format!("decision-v3-{side}"), MAX_M5_SECTION_TRACE_IDS),
                ids(&format!("finding-v3-{side}"), MAX_M5_SECTION_TRACE_IDS),
            )
            .unwrap()
        };
        let sections = vec![
            make(0, &payment_members, payment_sources),
            make(1, &ui_members, ui_sources),
        ];
        assert!(
            sections
                .iter()
                .all(|section| section.source_ids.len() == MAX_M5_SECTION_SOURCE_IDS)
        );
        let bundle = GluingBundleV4::derive(cover, descriptors, regs, sections, overlap).unwrap();
        assert_eq!(bundle.attempt.source_ids.len(), MAX_M5_ATTEMPT_SOURCE_IDS);
        assert_eq!(bundle.attempt.result, GluingResultV4::Candidate);
        assert_eq!(
            bundle.global_candidate.as_ref().unwrap().source_ids.len(),
            MAX_M5_ATTEMPT_SOURCE_IDS
        );
    }

    #[test]
    fn overlap_and_per_section_trace_plus_one_are_refused() {
        let (cover, descriptors, regs, _) = base();
        assert!(matches!(
            GluingBundleV4::derive(
                cover.clone(), descriptors.clone(), regs.clone(), Vec::new(),
                ids("overlap", MAX_M5_OVERLAP_IDS + 1),
            ),
            Err(M5Error::Incomplete { observed, .. }) if observed == MAX_M5_OVERLAP_IDS + 1
        ));
        let members = BTreeSet::from([id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID)]);
        assert!(matches!(
            SectionV4::derive_unverified(
                &cover, &descriptors[0], regs[0].clone(), id("obligation:payment"),
                id("claim:trace-plus-one"),
                &members, BTreeSet::new(), ids("binding", MAX_M5_SECTION_TRACE_IDS + 1),
                BTreeSet::new(), BTreeSet::new(), BTreeSet::new(), BTreeSet::new(),
            ),
            Err(M5Error::Incomplete { observed, .. }) if observed == MAX_M5_SECTION_TRACE_IDS + 1
        ));
        assert!(matches!(
            SectionV4::derive_unverified(
                &cover, &descriptors[0], regs[0].clone(), id("obligation:payment"),
                id("claim:source-plus-one"),
                &ids("claim-source", MAX_M5_CLAIM_SOURCE_IDS + 1),
                ids("claim-source", MAX_M5_CLAIM_SOURCE_IDS + 1), BTreeSet::new(),
                BTreeSet::new(), BTreeSet::new(), BTreeSet::new(), BTreeSet::new(),
            ),
            Err(M5Error::Incomplete { observed, .. }) if observed == MAX_M5_CLAIM_SOURCE_IDS + 1
        ));
    }

    #[test]
    fn restriction_source_limit_is_reachable() {
        let run = id("run:restriction-max");
        let snap = id("snapshot:restriction-max");
        let uni = id("universe:restriction-max");
        let plan = id("plan:restriction-max");
        let mut overlap = ids("restriction-overlap", MAX_M5_OVERLAP_IDS - 1);
        overlap.insert(id(DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID));
        let cover = ContextCoverV4::derive(
            run.clone(),
            snap.clone(),
            uni.clone(),
            plan.clone(),
            BTreeSet::from([id("obligation:payment"), id("obligation:ui")]),
            BTreeSet::new(),
            overlap.clone(),
            overlap.clone(),
            BTreeSet::new(),
        )
        .unwrap();
        let descriptors = [
            GluingInputDescriptorV4::new(
                run.clone(),
                snap.clone(),
                uni.clone(),
                plan.clone(),
                id(DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID),
                AssignmentValueV4::Required,
                BTreeSet::new(),
            )
            .unwrap(),
            GluingInputDescriptorV4::new(
                run,
                snap,
                uni,
                plan,
                id(DOUBLE_SUBMIT_UI_CONTEXT_ID),
                AssignmentValueV4::Required,
                BTreeSet::new(),
            )
            .unwrap(),
        ];
        let regs = [
            id("registration-v4:payment-restriction"),
            id("registration-v4:ui-restriction"),
        ];
        let sections: Vec<SectionV4> = (0..2)
            .map(|index| {
                let side = if index == 0 { "payment" } else { "ui" };
                SectionV4::derive_unverified(
                    &cover,
                    &descriptors[index],
                    regs[index].clone(),
                    id(&format!("obligation:{side}")),
                    id(&format!("claim:{side}")),
                    &overlap,
                    BTreeSet::new(),
                    ids(&format!("binding-r-{side}"), MAX_M5_SECTION_TRACE_IDS),
                    ids(&format!("evidence-r-{side}"), MAX_M5_SECTION_TRACE_IDS),
                    ids(
                        &format!("verification-v3-r-{side}"),
                        MAX_M5_SECTION_TRACE_IDS,
                    ),
                    ids(&format!("decision-v3-r-{side}"), MAX_M5_SECTION_TRACE_IDS),
                    ids(&format!("finding-v3-r-{side}"), MAX_M5_SECTION_TRACE_IDS),
                )
                .unwrap()
            })
            .collect();
        let restrictions = sections
            .iter()
            .map(|section| RestrictionV4::derive(section, &overlap).unwrap())
            .collect::<Vec<_>>();
        assert!(
            restrictions.iter().all(|restriction| {
                restriction.source_ids.len() == MAX_M5_RESTRICTION_SOURCE_IDS
            })
        );
    }

    #[test]
    fn v5_terminal_typed_upper_bound_is_monotone_and_overflow_closed() {
        let small = v5_terminal_m5_typed_retained_upper_bound(256, 1_024).unwrap();
        let large = v5_terminal_m5_typed_retained_upper_bound(
            u64::try_from(2 * MAX_M5_DESCRIPTOR_CANONICAL_BYTES).unwrap(),
            u64::try_from(MAX_M5_BUNDLE_CANONICAL_BYTES).unwrap(),
        )
        .unwrap();
        assert!(large > small);
        assert!(v5_terminal_m5_typed_retained_upper_bound(u64::MAX, 1).is_err());
        assert!(v5_terminal_m5_typed_retained_upper_bound(1, u64::MAX).is_err());
    }
}
