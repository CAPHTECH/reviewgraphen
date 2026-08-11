use crate::context::context_domain_error;
use crate::{
    ArtifactRegistered, ArtifactRegisteredV3, ArtifactSensitivity, ArtifactSource,
    ArtifactSourceV3, ContentHash, DomainError, Evidence, ExecutionClaimV2, ExecutionRecord,
    ProgramSpace, Result, ReviewContextEnvelope, ReviewPlan, RunGenesisManifest,
    SnapshotSourcesRecorded, StableId, UniverseDescriptor, VersionTuple,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

fn id_set_allocated(values: &BTreeSet<StableId>) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<StableId>())
        .saturating_add(values.iter().map(StableId::allocated_bytes).sum::<usize>())
}

/// Human review status, independent from claim disposition and verification.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    /// No human review has occurred.
    Unreviewed,
    /// A human reviewed it but did not accept it as a decision.
    HumanReviewed,
    /// A human authority explicitly accepted it.
    Accepted,
    /// A human authority explicitly rejected it.
    Rejected,
    /// Historical record replaced by another record.
    Superseded,
}

/// Lifecycle of an obligation, not a claim or verification state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationLifecycle {
    /// Synthesized into the universe.
    Generated,
    /// Selected for execution.
    Planned,
    /// An execution is active.
    InProgress,
    /// Execution finished; this says nothing about evidence or acceptance.
    Completed,
    /// Historical state no longer applies to a future snapshot.
    Stale,
    /// Replaced by another obligation.
    Superseded,
    /// Explicitly cancelled by policy.
    Cancelled,
}

impl ObligationLifecycle {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Generated => "generated",
            Self::Planned => "planned",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Stale => "stale",
            Self::Superseded => "superseded",
            Self::Cancelled => "cancelled",
        }
    }

    pub(crate) const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Generated,
                Self::Planned | Self::Stale | Self::Superseded | Self::Cancelled
            ) | (
                Self::Planned,
                Self::InProgress | Self::Stale | Self::Superseded | Self::Cancelled
            ) | (
                Self::InProgress,
                Self::Completed | Self::Stale | Self::Superseded | Self::Cancelled
            ) | (Self::Completed, Self::Stale | Self::Superseded)
        )
    }
}

/// Polarity of a reviewer conclusion.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimPolarity {
    /// A problem is present.
    IssuePresent,
    /// No problem was found within the declared scope.
    IssueAbsent,
    /// Evidence or context is insufficient.
    Inconclusive,
    /// Rule preconditions do not apply.
    NotApplicable,
    /// Local conclusions conflict.
    Conflict,
}

/// Evidentiary disposition of a claim, independent from human review status.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimDisposition {
    /// Structured reviewer output awaiting evidence.
    Proposed,
    /// Bound evidence supports the claim.
    Supported,
    /// Bound evidence refutes the claim.
    Refuted,
    /// A human decision accepted the claim after trace checks.
    Accepted,
    /// A human decision rejected the claim.
    Rejected,
    /// The claim was replaced but remains historical.
    Superseded,
}

/// Freshness derived from snapshot-bound evidence by the aggregate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    /// Verification applies to the current fixed snapshot.
    Fresh,
    /// Verification is historical and cannot sign off the current snapshot.
    Stale,
    /// Freshness could not be established.
    Unknown,
}

/// Outcome reported by an explicit verifier record.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationOutcome {
    /// No verifier ran.
    NotAttempted,
    /// Verifier completed its bounded check successfully.
    Passed,
    /// Verifier disproved or failed its check.
    Failed,
    /// Verifier could not reach a conclusion.
    Inconclusive,
    /// No verifier supports the requested property.
    Unsupported,
    /// Verifier evidence has expired.
    Expired,
}

/// Relation of an evidence item to a claim.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRelation {
    /// Evidence supports the claim.
    Supports,
    /// Evidence refutes the claim.
    Refutes,
    /// Evidence narrows the claim.
    Qualifies,
    /// Evidence reproduces the claim.
    Reproduces,
    /// Evidence contradicts the claim.
    Contradicts,
    /// Evidence supersedes an earlier record.
    Supersedes,
}

/// Decision outcome made by an authority.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionOutcome {
    /// Accept the target claim.
    Accept,
    /// Reject the target claim.
    Reject,
    /// Permit a scoped exception without erasing history.
    Exception,
    /// Defer the decision.
    Defer,
}

/// Authority category permitted to create an M1 decision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionAuthority {
    /// An accountable human reviewer.
    Human,
}

/// Reportable lifecycle of a finding projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingStatus {
    /// A claim exists without verification.
    UnverifiedCandidate,
    /// A claim has verification but no acceptance decision.
    VerifiedCandidate,
    /// A human decision accepted the finding.
    Accepted,
    /// A human decision rejected the finding.
    Rejected,
    /// The finding is resolved in a later record.
    Resolved,
    /// The finding is replaced but retained historically.
    Superseded,
}

/// A deterministic, version-bound obligation. It is not a reviewer conclusion.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Obligation {
    id: StableId,
    target_kind: String,
    target_refs: Vec<StableId>,
    normalized_target_refs: BTreeSet<StableId>,
    semantic_key: String,
    property_id: String,
    property_version: String,
    context_ids: Vec<StableId>,
    normalized_context_ids: BTreeSet<StableId>,
    required_capabilities: BTreeSet<String>,
    evidence_required: bool,
    accepted_evidence_modes: BTreeSet<String>,
    applicability_status: String,
    applicability_reasons: BTreeSet<String>,
    qualification_ids: BTreeSet<StableId>,
    weight: f64,
    version: VersionTuple,
    lifecycle: ObligationLifecycle,
    depends_on: Vec<StableId>,
    normalized_depends_on: BTreeSet<StableId>,
    generator_ids: BTreeSet<StableId>,
    source_ids: Vec<StableId>,
    normalized_source_ids: BTreeSet<StableId>,
}

pub(crate) struct ObligationParts {
    pub(crate) id: StableId,
    pub(crate) target_kind: String,
    pub(crate) target_refs: Vec<StableId>,
    pub(crate) normalized_target_refs: BTreeSet<StableId>,
    pub(crate) semantic_key: String,
    pub(crate) property_id: String,
    pub(crate) property_version: String,
    pub(crate) context_ids: Vec<StableId>,
    pub(crate) normalized_context_ids: BTreeSet<StableId>,
    pub(crate) required_capabilities: BTreeSet<String>,
    pub(crate) evidence_required: bool,
    pub(crate) accepted_evidence_modes: BTreeSet<String>,
    pub(crate) applicability_status: String,
    pub(crate) applicability_reasons: BTreeSet<String>,
    pub(crate) qualification_ids: BTreeSet<StableId>,
    pub(crate) weight: f64,
    pub(crate) version: VersionTuple,
    pub(crate) depends_on: Vec<StableId>,
    pub(crate) normalized_depends_on: BTreeSet<StableId>,
    pub(crate) generator_ids: BTreeSet<StableId>,
    pub(crate) source_ids: Vec<StableId>,
    pub(crate) normalized_source_ids: BTreeSet<StableId>,
}

impl Obligation {
    pub(crate) fn allocated_bytes(&self) -> usize {
        fn id_vec(values: &Vec<StableId>) -> usize {
            values
                .capacity()
                .saturating_mul(std::mem::size_of::<StableId>())
                .saturating_add(values.iter().map(StableId::allocated_bytes).sum::<usize>())
        }
        fn id_set(values: &BTreeSet<StableId>) -> usize {
            values
                .len()
                .saturating_mul(std::mem::size_of::<StableId>())
                .saturating_add(values.iter().map(StableId::allocated_bytes).sum::<usize>())
        }
        fn string_set(values: &BTreeSet<String>) -> usize {
            values
                .len()
                .saturating_mul(std::mem::size_of::<String>())
                .saturating_add(values.iter().map(String::capacity).sum::<usize>())
        }
        self.id
            .allocated_bytes()
            .saturating_add(self.target_kind.capacity())
            .saturating_add(id_vec(&self.target_refs))
            .saturating_add(id_set(&self.normalized_target_refs))
            .saturating_add(self.semantic_key.capacity())
            .saturating_add(self.property_id.capacity())
            .saturating_add(self.property_version.capacity())
            .saturating_add(id_vec(&self.context_ids))
            .saturating_add(id_set(&self.normalized_context_ids))
            .saturating_add(string_set(&self.required_capabilities))
            .saturating_add(string_set(&self.accepted_evidence_modes))
            .saturating_add(self.applicability_status.capacity())
            .saturating_add(string_set(&self.applicability_reasons))
            .saturating_add(id_set(&self.qualification_ids))
            .saturating_add(self.version.allocated_bytes())
            .saturating_add(id_vec(&self.depends_on))
            .saturating_add(id_set(&self.normalized_depends_on))
            .saturating_add(id_set(&self.generator_ids))
            .saturating_add(id_vec(&self.source_ids))
            .saturating_add(id_set(&self.normalized_source_ids))
    }

    pub(crate) fn new(parts: ObligationParts) -> Result<Self> {
        if parts.target_kind.is_empty()
            || parts.semantic_key.is_empty()
            || parts.property_id.is_empty()
            || parts.property_version.is_empty()
        {
            return Err(DomainError::Validation(
                "obligation target and property fields must not be empty".to_owned(),
            ));
        }
        if !matches!(
            parts.applicability_status.as_str(),
            "applicable" | "not_applicable" | "unknown"
        ) || parts.applicability_reasons.iter().any(String::is_empty)
        {
            return Err(DomainError::Validation(
                "obligation applicability must use a declared status and non-empty reasons"
                    .to_owned(),
            ));
        }
        if parts.target_refs.is_empty()
            || parts.source_ids.is_empty()
            || !parts.weight.is_finite()
            || parts.weight <= 0.0
        {
            return Err(DomainError::Validation(
                "obligation requires target/source IDs and a positive finite weight".to_owned(),
            ));
        }
        for (field, ordered, normalized) in [
            (
                "obligation.target_refs",
                &parts.target_refs,
                &parts.normalized_target_refs,
            ),
            (
                "obligation.context_ids",
                &parts.context_ids,
                &parts.normalized_context_ids,
            ),
            (
                "obligation.depends_on",
                &parts.depends_on,
                &parts.normalized_depends_on,
            ),
            (
                "obligation.source_ids",
                &parts.source_ids,
                &parts.normalized_source_ids,
            ),
        ] {
            if ordered.len() != normalized.len()
                || ordered.iter().any(|id| !normalized.contains(id))
            {
                return Err(DomainError::Validation(format!(
                    "{field} must be an ordered, duplicate-free view of its normalized IDs"
                )));
            }
        }
        Ok(Self {
            id: parts.id,
            target_kind: parts.target_kind,
            target_refs: parts.target_refs,
            normalized_target_refs: parts.normalized_target_refs,
            semantic_key: parts.semantic_key,
            property_id: parts.property_id,
            property_version: parts.property_version,
            context_ids: parts.context_ids,
            normalized_context_ids: parts.normalized_context_ids,
            required_capabilities: parts.required_capabilities,
            evidence_required: parts.evidence_required,
            accepted_evidence_modes: parts.accepted_evidence_modes,
            applicability_status: parts.applicability_status,
            applicability_reasons: parts.applicability_reasons,
            qualification_ids: parts.qualification_ids,
            weight: parts.weight,
            version: parts.version,
            lifecycle: ObligationLifecycle::Generated,
            depends_on: parts.depends_on,
            normalized_depends_on: parts.normalized_depends_on,
            generator_ids: parts.generator_ids,
            source_ids: parts.source_ids,
            normalized_source_ids: parts.normalized_source_ids,
        })
    }

    /// Re-runs every constructor invariant after deserializing a durable DTO.
    /// Lifecycle is event-owned mutable state, so it is checked separately
    /// from the immutable obligation definition reconstructed here.
    pub(crate) fn validate_full(&self) -> Result<()> {
        let rebuilt = Self::new(ObligationParts {
            id: self.id.clone(),
            target_kind: self.target_kind.clone(),
            target_refs: self.target_refs.clone(),
            normalized_target_refs: self.normalized_target_refs.clone(),
            semantic_key: self.semantic_key.clone(),
            property_id: self.property_id.clone(),
            property_version: self.property_version.clone(),
            context_ids: self.context_ids.clone(),
            normalized_context_ids: self.normalized_context_ids.clone(),
            required_capabilities: self.required_capabilities.clone(),
            evidence_required: self.evidence_required,
            accepted_evidence_modes: self.accepted_evidence_modes.clone(),
            applicability_status: self.applicability_status.clone(),
            applicability_reasons: self.applicability_reasons.clone(),
            qualification_ids: self.qualification_ids.clone(),
            weight: self.weight,
            version: self.version.clone(),
            depends_on: self.depends_on.clone(),
            normalized_depends_on: self.normalized_depends_on.clone(),
            generator_ids: self.generator_ids.clone(),
            source_ids: self.source_ids.clone(),
            normalized_source_ids: self.normalized_source_ids.clone(),
        })?;
        let mut expected = self.clone();
        expected.lifecycle = ObligationLifecycle::Generated;
        if rebuilt != expected {
            return Err(DomainError::Validation(
                "obligations must retain their constructor-valid definition".to_owned(),
            ));
        }
        Ok(())
    }

    /// Stable obligation ID.
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    /// Current lifecycle only; it implies no claim or verification state.
    #[must_use]
    pub fn lifecycle(&self) -> ObligationLifecycle {
        self.lifecycle
    }

    /// Risk weight used only for weighted coverage.
    #[must_use]
    pub fn weight(&self) -> f64 {
        self.weight
    }

    /// Ordered source trace retained by the obligation contract.
    #[must_use]
    pub fn source_ids(&self) -> &[StableId] {
        &self.source_ids
    }

    /// Normalized source IDs used for set-membership validation.
    #[must_use]
    pub fn normalized_source_ids(&self) -> &BTreeSet<StableId> {
        &self.normalized_source_ids
    }

    /// Target category used by the obligation contract.
    #[must_use]
    pub fn target_kind(&self) -> &str {
        &self.target_kind
    }

    /// Ordered program references that define this target. Path targets retain
    /// their temporal traversal order here.
    #[must_use]
    pub fn target_refs(&self) -> &[StableId] {
        &self.target_refs
    }

    /// Normalized target references used by set-membership validation and the
    /// unordered portion of deterministic identity. Path traversal order is
    /// retained separately by [`Self::target_refs`].
    #[must_use]
    pub fn normalized_target_refs(&self) -> &BTreeSet<StableId> {
        &self.normalized_target_refs
    }

    /// Stable semantic target key used in deterministic ID derivation.
    #[must_use]
    pub fn semantic_key(&self) -> &str {
        &self.semantic_key
    }

    /// Versioned property identifier.
    #[must_use]
    pub fn property_id(&self) -> &str {
        &self.property_id
    }

    /// Property contract version.
    #[must_use]
    pub fn property_version(&self) -> &str {
        &self.property_version
    }

    /// Ordered execution contexts required by this obligation.
    #[must_use]
    pub fn context_ids(&self) -> &[StableId] {
        &self.context_ids
    }

    /// Normalized execution contexts used as the applicability scope.
    #[must_use]
    pub fn normalized_context_ids(&self) -> &BTreeSet<StableId> {
        &self.normalized_context_ids
    }

    /// Extractor capabilities required to execute this obligation.
    #[must_use]
    pub fn required_capabilities(&self) -> &BTreeSet<String> {
        &self.required_capabilities
    }

    /// Whether the obligation requires evidence before it can be accepted.
    #[must_use]
    pub fn evidence_required(&self) -> bool {
        self.evidence_required
    }

    /// Permitted evidence modes for this obligation.
    #[must_use]
    pub fn accepted_evidence_modes(&self) -> &BTreeSet<String> {
        &self.accepted_evidence_modes
    }

    /// Applicability is explicit and remains `unknown` when extraction lacks a capability.
    #[must_use]
    pub fn applicability_status(&self) -> &str {
        &self.applicability_status
    }

    /// Deterministic reasons qualifying applicability.
    #[must_use]
    pub fn applicability_reasons(&self) -> &BTreeSet<String> {
        &self.applicability_reasons
    }

    /// Limitation or qualification records that explain applicability.
    #[must_use]
    pub fn qualification_ids(&self) -> &BTreeSet<StableId> {
        &self.qualification_ids
    }

    /// Tuple that binds this obligation to profile, rule, extractor, and snapshot.
    #[must_use]
    pub fn version(&self) -> &VersionTuple {
        &self.version
    }

    /// Earlier obligations in declared execution dependency order.
    #[must_use]
    pub fn depends_on(&self) -> &[StableId] {
        &self.depends_on
    }

    /// Normalized dependency IDs used for set-membership validation.
    #[must_use]
    pub fn normalized_depends_on(&self) -> &BTreeSet<StableId> {
        &self.normalized_depends_on
    }

    /// Program facts or path obligations that generated this obligation.
    /// Unlike execution dependencies this provenance is retained even when
    /// several paths merge into one invariant obligation.
    #[must_use]
    pub fn generator_ids(&self) -> &BTreeSet<StableId> {
        &self.generator_ids
    }

    pub(crate) fn transition(&mut self, next: ObligationLifecycle) -> Result<()> {
        if self.lifecycle.can_transition_to(next) {
            self.lifecycle = next;
            Ok(())
        } else {
            Err(DomainError::IllegalTransition {
                axis: "obligation lifecycle",
                id: self.id.clone(),
                from: self.lifecycle.as_str().to_owned(),
                to: next.as_str().to_owned(),
            })
        }
    }

    pub(crate) fn merge_dependencies(&mut self, dependencies: impl IntoIterator<Item = StableId>) {
        for dependency in dependencies {
            if self.normalized_depends_on.insert(dependency.clone()) {
                self.depends_on.push(dependency);
            }
        }
        self.depends_on.sort();
    }

    pub(crate) fn merge_generators(&mut self, generators: impl IntoIterator<Item = StableId>) {
        self.generator_ids.extend(generators);
    }
}

/// An AI, tool, or human review statement. It is never a program fact.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReviewClaim {
    id: StableId,
    execution_id: StableId,
    obligation_ids: BTreeSet<StableId>,
    polarity: ClaimPolarity,
    disposition: ClaimDisposition,
    summary: String,
    source_ids: BTreeSet<StableId>,
    candidate_confidence: Option<f64>,
    author_kind: ClaimAuthorKind,
    review_status: ReviewStatus,
}

/// Frozen M1/v1 claim body retained only for historical `ClaimProposed`
/// decoding and byte-exact replay. D2 uses [`crate::ExecutionClaimV2`].
pub type LegacyClaimV1 = ReviewClaim;

/// Claim producer class. Only `Ai` receives the mandatory proposed default.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimAuthorKind {
    /// Probabilistic reviewer output.
    Ai,
    /// A deterministic tool's structured conclusion.
    Tool,
    /// A human's review statement, still distinct from a decision.
    Human,
}

impl ReviewClaim {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.id
            .allocated_bytes()
            .saturating_add(self.execution_id.allocated_bytes())
            .saturating_add(id_set_allocated(&self.obligation_ids))
            .saturating_add(self.summary.capacity())
            .saturating_add(id_set_allocated(&self.source_ids))
    }
    /// Constructs an AI claim in its only admitted initial state: proposed and
    /// unreviewed. Confidence is descriptive and cannot alter this state.
    pub fn propose_ai(
        id: StableId,
        execution_id: StableId,
        obligation_ids: BTreeSet<StableId>,
        polarity: ClaimPolarity,
        summary: impl Into<String>,
        source_ids: BTreeSet<StableId>,
        candidate_confidence: Option<f64>,
    ) -> Result<Self> {
        let claim = Self {
            id,
            execution_id,
            obligation_ids,
            polarity,
            disposition: ClaimDisposition::Proposed,
            summary: summary.into(),
            source_ids,
            candidate_confidence,
            author_kind: ClaimAuthorKind::Ai,
            review_status: ReviewStatus::Unreviewed,
        };
        claim.validate_initial()?;
        Ok(claim)
    }

    /// Constructs a human or tool claim, still proposed until evidence/decision events.
    pub fn propose(
        id: StableId,
        execution_id: StableId,
        obligation_ids: BTreeSet<StableId>,
        polarity: ClaimPolarity,
        summary: impl Into<String>,
        source_ids: BTreeSet<StableId>,
        author_kind: ClaimAuthorKind,
    ) -> Result<Self> {
        let claim = Self {
            id,
            execution_id,
            obligation_ids,
            polarity,
            disposition: ClaimDisposition::Proposed,
            summary: summary.into(),
            source_ids,
            candidate_confidence: None,
            author_kind,
            review_status: ReviewStatus::Unreviewed,
        };
        claim.validate_initial()?;
        Ok(claim)
    }

    pub(crate) fn validate_initial(&self) -> Result<()> {
        if self.obligation_ids.is_empty() || self.source_ids.is_empty() || self.summary.is_empty() {
            return Err(DomainError::Validation(
                "claim requires obligations, source grounding, and a summary".to_owned(),
            ));
        }
        if self
            .candidate_confidence
            .is_some_and(|value| !(0.0..=1.0).contains(&value))
        {
            return Err(DomainError::Validation(
                "claim confidence must be between 0 and 1".to_owned(),
            ));
        }
        if self.disposition != ClaimDisposition::Proposed
            || self.review_status != ReviewStatus::Unreviewed
        {
            return Err(DomainError::Validation(
                "new claims must begin proposed and unreviewed".to_owned(),
            ));
        }
        Ok(())
    }

    /// Stable claim ID.
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    /// Execution that produced this claim.
    #[must_use]
    pub fn execution_id(&self) -> &StableId {
        &self.execution_id
    }

    /// Structured conclusion polarity.
    #[must_use]
    pub const fn polarity(&self) -> ClaimPolarity {
        self.polarity
    }

    /// Human-readable bounded claim text.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Descriptive AI confidence, never acceptance authority.
    #[must_use]
    pub const fn candidate_confidence(&self) -> Option<f64> {
        self.candidate_confidence
    }

    /// Current evidentiary disposition.
    #[must_use]
    pub fn disposition(&self) -> ClaimDisposition {
        self.disposition
    }

    /// Initial author kind.
    #[must_use]
    pub fn author_kind(&self) -> ClaimAuthorKind {
        self.author_kind
    }

    /// Human review state, independent from evidentiary disposition.
    #[must_use]
    pub fn review_status(&self) -> ReviewStatus {
        self.review_status
    }

    /// Obligations this scoped claim addresses.
    #[must_use]
    pub fn obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.obligation_ids
    }

    /// Program facts that ground this claim.
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }

    pub(crate) fn support(&mut self) -> Result<()> {
        if matches!(
            self.disposition,
            ClaimDisposition::Supported | ClaimDisposition::Accepted
        ) {
            return Ok(());
        }
        self.transition_disposition(ClaimDisposition::Supported, None)
    }

    pub(crate) fn refute(&mut self) -> Result<()> {
        self.transition_disposition(ClaimDisposition::Refuted, None)
    }

    pub(crate) fn accept(&mut self, decision: &Decision) -> Result<()> {
        self.transition_disposition(ClaimDisposition::Accepted, Some(decision))?;
        self.review_status = ReviewStatus::Accepted;
        Ok(())
    }

    pub(crate) fn reject(&mut self, decision: &Decision) -> Result<()> {
        self.transition_disposition(ClaimDisposition::Rejected, Some(decision))?;
        self.review_status = ReviewStatus::Rejected;
        Ok(())
    }

    fn transition_disposition(
        &mut self,
        next: ClaimDisposition,
        decision: Option<&Decision>,
    ) -> Result<()> {
        let allowed = matches!(
            (self.disposition, next),
            (
                ClaimDisposition::Proposed,
                ClaimDisposition::Supported
                    | ClaimDisposition::Refuted
                    | ClaimDisposition::Rejected
                    | ClaimDisposition::Superseded
            ) | (
                ClaimDisposition::Supported,
                ClaimDisposition::Accepted
                    | ClaimDisposition::Refuted
                    | ClaimDisposition::Rejected
                    | ClaimDisposition::Superseded
            ) | (
                ClaimDisposition::Refuted,
                ClaimDisposition::Rejected | ClaimDisposition::Superseded
            )
        );
        let acceptance_is_decided = matches!(
            (next, decision.map(|item| item.outcome)),
            (ClaimDisposition::Accepted, Some(DecisionOutcome::Accept))
                | (ClaimDisposition::Rejected, Some(DecisionOutcome::Reject))
        );
        if allowed
            && (next != ClaimDisposition::Accepted && next != ClaimDisposition::Rejected
                || acceptance_is_decided)
        {
            self.disposition = next;
            Ok(())
        } else {
            Err(DomainError::IllegalTransition {
                axis: "claim disposition",
                id: self.id.clone(),
                from: format!("{:?}", self.disposition),
                to: format!("{:?}", next),
            })
        }
    }
}

/// Explicit link between independently stored evidence and a claim.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EvidenceBinding {
    id: StableId,
    claim_id: StableId,
    evidence_id: StableId,
    relation: EvidenceRelation,
    scope: BTreeMap<String, String>,
}

impl EvidenceBinding {
    pub(crate) fn allocated_bytes(&self) -> usize {
        let scope = self.scope.iter().fold(
            self.scope
                .len()
                .saturating_mul(std::mem::size_of::<(String, String)>()),
            |total, (key, value)| {
                total
                    .saturating_add(key.capacity())
                    .saturating_add(value.capacity())
            },
        );
        self.id
            .allocated_bytes()
            .saturating_add(self.claim_id.allocated_bytes())
            .saturating_add(self.evidence_id.allocated_bytes())
            .saturating_add(scope)
    }
    /// Binds one separately stored evidence record to one claim.
    pub fn new(
        id: StableId,
        claim_id: StableId,
        evidence_id: StableId,
        relation: EvidenceRelation,
        scope: BTreeMap<String, String>,
    ) -> Result<Self> {
        if scope.get("property_id").is_none_or(String::is_empty) {
            return Err(DomainError::Validation(
                "evidence binding requires a non-empty property_id scope".to_owned(),
            ));
        }
        Ok(Self {
            id,
            claim_id,
            evidence_id,
            relation,
            scope,
        })
    }

    /// Stable binding ID.
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    /// Referenced claim.
    #[must_use]
    pub fn claim_id(&self) -> &StableId {
        &self.claim_id
    }

    /// Referenced evidence.
    #[must_use]
    pub fn evidence_id(&self) -> &StableId {
        &self.evidence_id
    }

    /// Relationship kind.
    #[must_use]
    pub fn relation(&self) -> EvidenceRelation {
        self.relation
    }

    /// Optional structured scope qualifiers.
    #[must_use]
    pub fn scope(&self) -> &BTreeMap<String, String> {
        &self.scope
    }
}

/// An explicit verifier result; it does not imply a human decision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Verification {
    id: StableId,
    claim_id: StableId,
    outcome: VerificationOutcome,
    verifier_id: String,
    evidence_ids: BTreeSet<StableId>,
    freshness: Freshness,
}

impl Verification {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.id
            .allocated_bytes()
            .saturating_add(self.claim_id.allocated_bytes())
            .saturating_add(self.verifier_id.capacity())
            .saturating_add(id_set_allocated(&self.evidence_ids))
    }
    /// Constructs a verifier record. Passing verification requires cited evidence.
    pub fn new(
        id: StableId,
        claim_id: StableId,
        outcome: VerificationOutcome,
        verifier_id: impl Into<String>,
        evidence_ids: BTreeSet<StableId>,
    ) -> Result<Self> {
        let verifier_id = verifier_id.into();
        if verifier_id.is_empty() {
            return Err(DomainError::EmptyField {
                field: "verifier_id",
            });
        }
        if outcome == VerificationOutcome::Passed && evidence_ids.is_empty() {
            return Err(DomainError::Validation(
                "a passed verification requires evidence".to_owned(),
            ));
        }
        Ok(Self {
            id,
            claim_id,
            outcome,
            verifier_id,
            evidence_ids,
            freshness: Freshness::Unknown,
        })
    }

    /// Stable verification ID.
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    /// Claim under verification.
    #[must_use]
    pub fn claim_id(&self) -> &StableId {
        &self.claim_id
    }

    /// Bounded verifier result.
    #[must_use]
    pub fn outcome(&self) -> VerificationOutcome {
        self.outcome
    }

    /// Accountable verifier identity.
    #[must_use]
    pub fn verifier_id(&self) -> &str {
        &self.verifier_id
    }

    /// Evidence explicitly considered by the verifier.
    #[must_use]
    pub fn evidence_ids(&self) -> &BTreeSet<StableId> {
        &self.evidence_ids
    }

    /// Current-snapshot freshness declaration.
    #[must_use]
    pub fn freshness(&self) -> Freshness {
        self.freshness
    }

    pub(crate) fn with_derived_freshness(mut self, freshness: Freshness) -> Self {
        self.freshness = freshness;
        self
    }

    pub(crate) fn validate_event_shape(&self) -> Result<()> {
        if self.verifier_id.is_empty() {
            return Err(DomainError::EmptyField {
                field: "verifier_id",
            });
        }
        if self.outcome == VerificationOutcome::Passed && self.evidence_ids.is_empty() {
            return Err(DomainError::Validation(
                "a passed verification requires evidence".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Explicit M1 admission from a host that has already authenticated a human.
///
/// This is deliberately a narrow boundary, not an authentication system. The
/// caller must create this capability at the trusted host boundary; bare
/// strings cannot construct an accepting `Decision`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedHumanAdmission {
    actor: String,
    authority: String,
}

/// Opaque, non-serializable authorization for one exact decision at one run
/// stream position.
///
/// A trusted host mints this immediately before append/import.  It binds the
/// decision's canonical body as well as actor, authority, outcome, run, and
/// expected chain position, so a JSON record cannot borrow authority from a
/// different decision or forked stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionAdmission {
    run_id: StableId,
    genesis_hash: crate::ContentHash,
    tail_hash: crate::ContentHash,
    sequence: u64,
    universe_id: StableId,
    closure_digest: crate::ContentHash,
    decision_id: StableId,
    outcome: DecisionOutcome,
    actor: String,
    authority: String,
    digest: crate::ContentHash,
}

impl DecisionAdmission {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.run_id.allocated_bytes()
            + self.genesis_hash.allocated_bytes()
            + self.tail_hash.allocated_bytes()
            + self.universe_id.allocated_bytes()
            + self.closure_digest.allocated_bytes()
            + self.decision_id.allocated_bytes()
            + self.actor.capacity()
            + self.authority.capacity()
            + self.digest.allocated_bytes()
    }
}

impl TrustedHumanAdmission {
    /// Admits an already-authenticated human identity into the M1 in-memory run.
    pub fn from_trusted_host(
        actor: impl Into<String>,
        authority: impl Into<String>,
    ) -> Result<Self> {
        let actor = actor.into();
        let authority = authority.into();
        if !actor.starts_with("human:") || actor.len() == "human:".len() || authority.is_empty() {
            return Err(DomainError::Validation(
                "trusted human admission requires a non-empty human actor and authority".to_owned(),
            ));
        }
        Ok(Self { actor, authority })
    }

    /// Event actor bound to decisions made through this admission.
    #[must_use]
    pub fn actor(&self) -> &str {
        &self.actor
    }

    /// Authorizes a decision for the exact aggregate closure selected by an
    /// event log. This is crate-private so callers cannot mint a token without
    /// binding it to that log's immutable genesis and universe.
    pub(crate) fn admit_decision_for_event_log(
        &self,
        position: crate::event::StreamPosition,
        universe_id: StableId,
        closure_digest: crate::ContentHash,
        decision: &Decision,
    ) -> Result<DecisionAdmission> {
        if position.run_id.kind() != "run"
            || position.sequence == 0
            || !decision.matches_human(self)
        {
            return Err(DomainError::Validation(
                "decision admission requires its authenticated human actor, authority, and run"
                    .to_owned(),
            ));
        }
        Ok(DecisionAdmission {
            run_id: position.run_id,
            genesis_hash: position.genesis_hash,
            tail_hash: position.tail_hash,
            sequence: position.sequence,
            universe_id,
            closure_digest,
            decision_id: decision.id.clone(),
            outcome: decision.outcome,
            actor: decision.actor.clone(),
            authority: decision.authority.clone(),
            digest: crate::ContentHash::sha256(&crate::canonical_json(decision)?),
        })
    }
}

/// A human or explicit policy authority decision about one claim.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Decision {
    id: StableId,
    target_claim_id: StableId,
    outcome: DecisionOutcome,
    authority_kind: DecisionAuthority,
    authority: String,
    actor: String,
    rationale: String,
    source_ids: BTreeSet<StableId>,
}

impl Decision {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.id
            .allocated_bytes()
            .saturating_add(self.target_claim_id.allocated_bytes())
            .saturating_add(self.authority.capacity())
            .saturating_add(self.actor.capacity())
            .saturating_add(self.rationale.capacity())
            .saturating_add(id_set_allocated(&self.source_ids))
    }
    /// Constructs an explicit accountable human decision.
    pub fn human(
        id: StableId,
        target_claim_id: StableId,
        outcome: DecisionOutcome,
        admission: TrustedHumanAdmission,
        rationale: impl Into<String>,
        source_ids: BTreeSet<StableId>,
    ) -> Result<Self> {
        let rationale = rationale.into();
        if rationale.is_empty() || source_ids.is_empty() {
            return Err(DomainError::Validation(
                "decision rationale and source trace must not be empty".to_owned(),
            ));
        }
        Ok(Self {
            id,
            target_claim_id,
            outcome,
            authority_kind: DecisionAuthority::Human,
            authority: admission.authority,
            actor: admission.actor,
            rationale,
            source_ids,
        })
    }

    /// Stable decision ID.
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    /// Claim ID being decided.
    #[must_use]
    pub fn target_claim_id(&self) -> &StableId {
        &self.target_claim_id
    }

    /// Decision outcome.
    #[must_use]
    pub fn outcome(&self) -> DecisionOutcome {
        self.outcome
    }

    /// Authority category.
    #[must_use]
    pub fn authority_kind(&self) -> DecisionAuthority {
        self.authority_kind
    }

    /// Accountable authority identifier.
    #[must_use]
    pub fn authority(&self) -> &str {
        &self.authority
    }

    /// Event actor authenticated by the trusted M1 admission boundary.
    #[must_use]
    pub fn actor(&self) -> &str {
        &self.actor
    }

    /// Explicit rationale.
    #[must_use]
    pub fn rationale(&self) -> &str {
        &self.rationale
    }

    /// Claim, evidence, verification, and program sources cited by the decision.
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }

    pub(crate) fn validate_event_admission(&self) -> Result<()> {
        if self.authority_kind != DecisionAuthority::Human
            || !self.actor.starts_with("human:")
            || self.actor.len() == "human:".len()
            || self.authority.is_empty()
            || self.rationale.is_empty()
            || self.source_ids.is_empty()
        {
            return Err(DomainError::Validation(
                "persisted decision does not satisfy the M1 trusted human admission boundary"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    pub(crate) fn matches_human(&self, admission: &TrustedHumanAdmission) -> bool {
        self.actor == admission.actor && self.authority == admission.authority
    }

    pub(crate) fn matches_decision_admission(
        &self,
        position: &crate::event::StreamPosition,
        universe_id: &StableId,
        closure_digest: &crate::ContentHash,
        admission: &DecisionAdmission,
    ) -> bool {
        admission.run_id == position.run_id
            && admission.genesis_hash == position.genesis_hash
            && admission.tail_hash == position.tail_hash
            && admission.sequence == position.sequence
            && admission.universe_id == *universe_id
            && admission.closure_digest == *closure_digest
            && admission.decision_id == self.id
            && admission.outcome == self.outcome
            && admission.actor == self.actor
            && admission.authority == self.authority
            && crate::canonical_json(self)
                .is_ok_and(|bytes| crate::ContentHash::sha256(&bytes) == admission.digest)
    }
}

/// A report projection of a claim; its accepted state has the strongest trace rule.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Finding {
    id: StableId,
    claim_id: StableId,
    status: FindingStatus,
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_id: Option<StableId>,
    source_ids: BTreeSet<StableId>,
}

/// Trace data supplied when constructing a finding. Aggregate validation decides
/// whether this trace is sufficient for its requested status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FindingTrace {
    evidence_ids: BTreeSet<StableId>,
    verification_ids: BTreeSet<StableId>,
    decision_id: Option<StableId>,
    source_ids: BTreeSet<StableId>,
}

impl FindingTrace {
    /// Creates an explicit finding trace without granting acceptance authority.
    pub fn new(
        evidence_ids: BTreeSet<StableId>,
        verification_ids: BTreeSet<StableId>,
        decision_id: Option<StableId>,
        source_ids: BTreeSet<StableId>,
    ) -> Self {
        Self {
            evidence_ids,
            verification_ids,
            decision_id,
            source_ids,
        }
    }
}

impl Finding {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.id
            .allocated_bytes()
            .saturating_add(self.claim_id.allocated_bytes())
            .saturating_add(id_set_allocated(&self.evidence_ids))
            .saturating_add(id_set_allocated(&self.verification_ids))
            .saturating_add(
                self.decision_id
                    .as_ref()
                    .map_or(0, StableId::allocated_bytes),
            )
            .saturating_add(id_set_allocated(&self.source_ids))
    }
    /// Creates a report finding. `Accepted` is checked only when it is added to
    /// a validated aggregate with its joined claim/evidence/verification/decision trace.
    #[must_use]
    pub fn new(
        id: StableId,
        claim_id: StableId,
        status: FindingStatus,
        trace: FindingTrace,
    ) -> Self {
        Self {
            id,
            claim_id,
            status,
            evidence_ids: trace.evidence_ids,
            verification_ids: trace.verification_ids,
            decision_id: trace.decision_id,
            source_ids: trace.source_ids,
        }
    }

    /// Stable finding ID.
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    /// Source claim ID.
    #[must_use]
    pub fn claim_id(&self) -> &StableId {
        &self.claim_id
    }

    /// Reported status.
    #[must_use]
    pub fn status(&self) -> FindingStatus {
        self.status
    }

    /// Evidence trace.
    #[must_use]
    pub fn evidence_ids(&self) -> &BTreeSet<StableId> {
        &self.evidence_ids
    }

    /// Verification trace.
    #[must_use]
    pub fn verification_ids(&self) -> &BTreeSet<StableId> {
        &self.verification_ids
    }

    /// Decision cited by this finding, if any.
    #[must_use]
    pub fn decision_id(&self) -> Option<&StableId> {
        self.decision_id.as_ref()
    }

    /// Program facts used to locate the finding.
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
}

fn unique_event_ids(values: Vec<StableId>, field: &'static str) -> Result<BTreeSet<StableId>> {
    let mut result = BTreeSet::new();
    for id in values {
        if !result.insert(id.clone()) {
            return Err(DomainError::Validation(format!(
                "{field} must not contain duplicate IDs"
            )));
        }
    }
    Ok(result)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEventClaim {
    id: StableId,
    execution_id: StableId,
    obligation_ids: Vec<StableId>,
    polarity: ClaimPolarity,
    disposition: ClaimDisposition,
    summary: String,
    source_ids: Vec<StableId>,
    candidate_confidence: Option<f64>,
    author_kind: ClaimAuthorKind,
    review_status: ReviewStatus,
}

impl ReviewClaim {
    pub(crate) fn from_event_value(value: serde_json::Value) -> Result<Self> {
        let raw: RawEventClaim =
            serde_json::from_value(value).map_err(|error| DomainError::Json(error.to_string()))?;
        if raw.disposition != ClaimDisposition::Proposed
            || raw.review_status != ReviewStatus::Unreviewed
        {
            return Err(DomainError::Validation(
                "persisted new claims must be proposed and unreviewed".to_owned(),
            ));
        }
        let obligation_ids = unique_event_ids(raw.obligation_ids, "claim.obligation_ids")?;
        let source_ids = unique_event_ids(raw.source_ids, "claim.source_ids")?;
        match raw.author_kind {
            ClaimAuthorKind::Ai => Self::propose_ai(
                raw.id,
                raw.execution_id,
                obligation_ids,
                raw.polarity,
                raw.summary,
                source_ids,
                raw.candidate_confidence,
            ),
            author_kind => {
                if raw.candidate_confidence.is_some() {
                    return Err(DomainError::Validation(
                        "only AI claims may carry candidate confidence".to_owned(),
                    ));
                }
                Self::propose(
                    raw.id,
                    raw.execution_id,
                    obligation_ids,
                    raw.polarity,
                    raw.summary,
                    source_ids,
                    author_kind,
                )
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEventBinding {
    id: StableId,
    claim_id: StableId,
    evidence_id: StableId,
    relation: EvidenceRelation,
    #[serde(default)]
    scope: BTreeMap<String, String>,
}

impl EvidenceBinding {
    pub(crate) fn from_event_value(value: serde_json::Value) -> Result<Self> {
        let raw: RawEventBinding =
            serde_json::from_value(value).map_err(|error| DomainError::Json(error.to_string()))?;
        Self::new(
            raw.id,
            raw.claim_id,
            raw.evidence_id,
            raw.relation,
            raw.scope,
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEventVerification {
    id: StableId,
    claim_id: StableId,
    outcome: VerificationOutcome,
    verifier_id: String,
    evidence_ids: Vec<StableId>,
    freshness: Freshness,
}

impl Verification {
    pub(crate) fn from_event_value(value: serde_json::Value) -> Result<Self> {
        let raw: RawEventVerification =
            serde_json::from_value(value).map_err(|error| DomainError::Json(error.to_string()))?;
        let evidence_ids = unique_event_ids(raw.evidence_ids, "verification.evidence_ids")?;
        Self::new(
            raw.id,
            raw.claim_id,
            raw.outcome,
            raw.verifier_id,
            evidence_ids,
        )
        .map(|verification| verification.with_derived_freshness(raw.freshness))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEventDecision {
    id: StableId,
    target_claim_id: StableId,
    outcome: DecisionOutcome,
    authority_kind: DecisionAuthority,
    authority: String,
    actor: String,
    rationale: String,
    source_ids: Vec<StableId>,
}

impl Decision {
    pub(crate) fn from_event_value(value: serde_json::Value) -> Result<Self> {
        let raw: RawEventDecision =
            serde_json::from_value(value).map_err(|error| DomainError::Json(error.to_string()))?;
        let decision = Self {
            id: raw.id,
            target_claim_id: raw.target_claim_id,
            outcome: raw.outcome,
            authority_kind: raw.authority_kind,
            authority: raw.authority,
            actor: raw.actor,
            rationale: raw.rationale,
            source_ids: unique_event_ids(raw.source_ids, "decision.source_ids")?,
        };
        decision.validate_event_admission()?;
        Ok(decision)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEventFinding {
    id: StableId,
    claim_id: StableId,
    status: FindingStatus,
    evidence_ids: Vec<StableId>,
    verification_ids: Vec<StableId>,
    decision_id: Option<StableId>,
    source_ids: Vec<StableId>,
}

impl Finding {
    pub(crate) fn from_event_value(value: serde_json::Value) -> Result<Self> {
        let raw: RawEventFinding =
            serde_json::from_value(value).map_err(|error| DomainError::Json(error.to_string()))?;
        Ok(Self::new(
            raw.id,
            raw.claim_id,
            raw.status,
            FindingTrace::new(
                unique_event_ids(raw.evidence_ids, "finding.evidence_ids")?,
                unique_event_ids(raw.verification_ids, "finding.verification_ids")?,
                raw.decision_id,
                unique_event_ids(raw.source_ids, "finding.source_ids")?,
            ),
        ))
    }
}

/// Validated aggregate rebuilt only from its ProgramSpace, universe, and events.
#[derive(Clone, Debug, Serialize)]
pub struct ReviewAggregate {
    program: ProgramSpace,
    universe: UniverseDescriptor,
    obligations: BTreeMap<StableId, Obligation>,
    claims: BTreeMap<StableId, ReviewClaim>,
    evidence: BTreeMap<StableId, Evidence>,
    bindings: BTreeMap<StableId, EvidenceBinding>,
    verifications: BTreeMap<StableId, Verification>,
    decisions: BTreeMap<StableId, Decision>,
    findings: BTreeMap<StableId, Finding>,
    #[serde(skip)]
    genesis_manifest: Option<RunGenesisManifest>,
    #[serde(skip)]
    registered_artifacts: BTreeMap<StableId, ArtifactRegistered>,
    #[serde(skip)]
    registered_artifacts_v3: BTreeMap<StableId, ArtifactRegisteredV3>,
    #[serde(skip)]
    snapshot_sources: BTreeMap<StableId, SnapshotSourcesRecorded>,
    #[serde(skip)]
    plans: BTreeMap<StableId, ReviewPlan>,
    #[serde(skip)]
    envelopes: BTreeMap<StableId, ReviewContextEnvelope>,
    #[serde(skip)]
    offline_execution_envelopes: BTreeMap<StableId, ReviewContextEnvelope>,
    #[serde(skip)]
    executions: BTreeMap<StableId, ExecutionRecord>,
    #[serde(skip)]
    execution_claims: BTreeMap<StableId, ExecutionClaimV2>,
    #[serde(skip)]
    execution_raw_sizes: BTreeMap<StableId, u64>,
}

/// Immutable, validated source-registration closure for one current snapshot
/// artifact. It is a read-only runtime projection and carries no admission or
/// mutation capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextSourceRegistration {
    pub artifact_id: StableId,
    pub registration_id: StableId,
    pub content_hash: crate::ContentHash,
    pub cas_hash: crate::ContentHash,
    pub line_count: u64,
    pub size: u64,
    pub media_type: String,
    pub sensitivity: ArtifactSensitivity,
    pub source: ArtifactSource,
}

#[derive(Clone, Copy)]
pub(crate) enum ArtifactRegistrationRef<'a> {
    V2(&'a ArtifactRegistered),
    V3(&'a ArtifactRegisteredV3),
}

impl<'a> ArtifactRegistrationRef<'a> {
    fn run_id(self) -> &'a StableId {
        match self {
            Self::V2(value) => value.run_id(),
            Self::V3(value) => value.run_id(),
        }
    }

    pub(crate) fn cas_hash(self) -> &'a ContentHash {
        match self {
            Self::V2(value) => value.cas_hash(),
            Self::V3(value) => value.cas_hash(),
        }
    }

    fn media_type(self) -> &'a str {
        match self {
            Self::V2(value) => value.media_type(),
            Self::V3(value) => value.media_type(),
        }
    }

    pub(crate) fn size(self) -> u64 {
        match self {
            Self::V2(value) => value.size(),
            Self::V3(value) => value.size(),
        }
    }

    pub(crate) fn sensitivity(self) -> ArtifactSensitivity {
        match self {
            Self::V2(value) => value.sensitivity(),
            Self::V3(value) => value.sensitivity(),
        }
    }

    pub(crate) fn is_snapshot_ingest(self, snapshot_id: &StableId) -> bool {
        match self {
            Self::V2(value) => matches!(
                value.source(),
                ArtifactSource::SnapshotIngest {
                    snapshot_id: registered,
                    adapter_id,
                    ..
                } if registered == snapshot_id && !adapter_id.trim().is_empty()
            ),
            Self::V3(value) => matches!(
                value.source(),
                ArtifactSourceV3::SnapshotIngest {
                    snapshot_id: registered,
                    adapter_id,
                    ..
                } if registered == snapshot_id && !adapter_id.trim().is_empty()
            ),
        }
    }

    fn is_reviewer_execution(
        self,
        run_id: &StableId,
        execution_id: &StableId,
        reviewer_id: &str,
    ) -> bool {
        match self {
            Self::V2(value) => matches!(
                value.source(),
                ArtifactSource::ReviewerExecution {
                    run_id: registered_run,
                    execution_id: registered_execution,
                    reviewer_id: registered_reviewer,
                } if registered_run == run_id
                    && registered_execution == execution_id
                    && registered_reviewer == reviewer_id
            ),
            Self::V3(value) => matches!(
                value.source(),
                ArtifactSourceV3::ReviewerExecution {
                    run_id: registered_run,
                    execution_id: registered_execution,
                    reviewer_id: registered_reviewer,
                } if registered_run == run_id
                    && registered_execution == execution_id
                    && registered_reviewer == reviewer_id
            ),
        }
    }

    fn snapshot_source_projection(self) -> Option<ArtifactSource> {
        match self {
            Self::V2(value) => Some(value.source().clone()),
            Self::V3(value) => match value.source() {
                ArtifactSourceV3::SnapshotIngest {
                    adapter_id,
                    run_id,
                    snapshot_id,
                } => Some(ArtifactSource::SnapshotIngest {
                    run_id: run_id.clone(),
                    snapshot_id: snapshot_id.clone(),
                    adapter_id: adapter_id.clone(),
                }),
                _ => None,
            },
        }
    }
}

/// Read-only durable progress for one fake execution identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RawArtifactRegistration {
    V2(ArtifactRegistered),
    V3(Box<ArtifactRegisteredV3>),
}

impl RawArtifactRegistration {
    #[must_use]
    pub fn registration_id(&self) -> &StableId {
        match self {
            Self::V2(value) => value.registration_id(),
            Self::V3(value) => value.registration_id(),
        }
    }
}

/// Read-only durable progress for one fake execution identity.
#[derive(Clone, Debug, PartialEq)]
pub enum FakeAttemptState {
    None,
    RawRegistered {
        registration: RawArtifactRegistration,
    },
    AmbiguousRawRegistrations,
    ExecutionRecorded {
        execution: ExecutionRecord,
    },
    Completed {
        execution: ExecutionRecord,
    },
}

impl ReviewAggregate {
    /// Observable retained ownership used by the fixed event-v3 working-set
    /// contract. This charges inline map slots and every owned backing buffer;
    /// allocator-private B-tree node headers are intentionally outside the
    /// portable accounting contract.
    pub(crate) fn retained_bytes_v3(&self) -> Result<u64> {
        fn add(total: &mut u64, bytes: usize) -> Result<()> {
            *total = total
                .checked_add(u64::try_from(bytes).unwrap_or(u64::MAX))
                .ok_or(DomainError::Incomplete {
                    operation: "event-v3 ReviewAggregate retained bytes",
                    limit: usize::MAX,
                    observed: usize::MAX,
                })?;
            Ok(())
        }
        fn add_map<T>(
            total: &mut u64,
            values: &BTreeMap<StableId, T>,
            allocated: impl Fn(&T) -> usize,
        ) -> Result<()> {
            for (id, value) in values {
                add(total, std::mem::size_of::<(StableId, T)>())?;
                add(total, id.allocated_bytes())?;
                add(total, allocated(value))?;
            }
            Ok(())
        }

        let mut total = u64::try_from(std::mem::size_of::<Self>()).unwrap_or(u64::MAX);
        add(&mut total, self.program.allocated_bytes())?;
        add(&mut total, self.universe.allocated_bytes())?;
        add_map(&mut total, &self.obligations, Obligation::allocated_bytes)?;
        add_map(&mut total, &self.claims, ReviewClaim::allocated_bytes)?;
        add_map(&mut total, &self.evidence, Evidence::allocated_bytes)?;
        add_map(&mut total, &self.bindings, EvidenceBinding::allocated_bytes)?;
        add_map(
            &mut total,
            &self.verifications,
            Verification::allocated_bytes,
        )?;
        add_map(&mut total, &self.decisions, Decision::allocated_bytes)?;
        add_map(&mut total, &self.findings, Finding::allocated_bytes)?;
        if let Some(value) = &self.genesis_manifest {
            add(&mut total, value.allocated_bytes())?;
        }
        add_map(
            &mut total,
            &self.registered_artifacts,
            ArtifactRegistered::allocated_bytes,
        )?;
        add_map(
            &mut total,
            &self.registered_artifacts_v3,
            ArtifactRegisteredV3::allocated_bytes,
        )?;
        add_map(
            &mut total,
            &self.snapshot_sources,
            SnapshotSourcesRecorded::allocated_bytes,
        )?;
        add_map(&mut total, &self.plans, ReviewPlan::allocated_bytes)?;
        add_map(
            &mut total,
            &self.envelopes,
            ReviewContextEnvelope::allocated_bytes,
        )?;
        add_map(
            &mut total,
            &self.offline_execution_envelopes,
            ReviewContextEnvelope::allocated_bytes,
        )?;
        add_map(
            &mut total,
            &self.executions,
            ExecutionRecord::allocated_bytes,
        )?;
        add_map(
            &mut total,
            &self.execution_claims,
            ExecutionClaimV2::allocated_bytes,
        )?;
        for id in self.execution_raw_sizes.keys() {
            add(
                &mut total,
                std::mem::size_of::<(StableId, u64)>() + id.allocated_bytes(),
            )?;
        }
        Ok(total)
    }

    /// Starts a run from a validated ProgramSpace and deterministic universe.
    pub fn new(
        program: ProgramSpace,
        universe: UniverseDescriptor,
        obligations: Vec<Obligation>,
    ) -> Result<Self> {
        let mut obligation_map = BTreeMap::new();
        for obligation in obligations {
            let obligation_id = obligation.id.clone();
            if obligation_map
                .insert(obligation_id.clone(), obligation)
                .is_some()
            {
                return Err(DomainError::IdCollision { id: obligation_id });
            }
        }
        let mut evidence = BTreeMap::new();
        for item in program.evidence() {
            if evidence.insert(item.id().clone(), item.clone()).is_some() {
                return Err(DomainError::IdCollision {
                    id: item.id().clone(),
                });
            }
        }
        let aggregate = Self {
            program,
            universe,
            obligations: obligation_map,
            claims: BTreeMap::new(),
            evidence,
            bindings: BTreeMap::new(),
            verifications: BTreeMap::new(),
            decisions: BTreeMap::new(),
            findings: BTreeMap::new(),
            genesis_manifest: None,
            registered_artifacts: BTreeMap::new(),
            registered_artifacts_v3: BTreeMap::new(),
            snapshot_sources: BTreeMap::new(),
            plans: BTreeMap::new(),
            envelopes: BTreeMap::new(),
            offline_execution_envelopes: BTreeMap::new(),
            executions: BTreeMap::new(),
            execution_claims: BTreeMap::new(),
            execution_raw_sizes: BTreeMap::new(),
        };
        aggregate.validate()?;
        Ok(aggregate)
    }

    /// Validates all cross-space references and trust-state rules.
    pub fn validate(&self) -> Result<()> {
        let obligation_ids = self.obligations.keys().cloned().collect::<BTreeSet<_>>();
        if obligation_ids != *self.universe.obligation_ids() {
            return Err(DomainError::Validation(
                "universe denominator IDs do not match obligation records".to_owned(),
            ));
        }
        self.universe
            .validate_against(&self.program, &self.obligations)?;
        let program_ids = self.program.known_ids();
        let mut all_ids = program_ids.clone();
        for id in &obligation_ids {
            insert_unique(&mut all_ids, id)?;
        }
        for obligation in self.obligations.values() {
            obligation.validate_full()?;
            if obligation.version.snapshot() != self.program.snapshot_id() {
                return Err(DomainError::Validation(
                    "obligation version tuple must bind the current snapshot".to_owned(),
                ));
            }
            for reference in obligation
                .normalized_target_refs
                .iter()
                .chain(obligation.normalized_source_ids.iter())
                .chain(obligation.normalized_context_ids.iter())
            {
                require_known("obligation", obligation.id(), &program_ids, reference)?;
            }
            for dependency in &obligation.normalized_depends_on {
                require_known("obligation", obligation.id(), &obligation_ids, dependency)?;
            }
            for generator in obligation.generator_ids() {
                require_known("obligation", obligation.id(), &all_ids, generator)?;
            }
            let target_kind_matches = match obligation.target_kind() {
                "node" => obligation
                    .target_refs()
                    .iter()
                    .all(|id| self.program.artifact(id).is_some()),
                "relation" | "path" => obligation
                    .target_refs()
                    .iter()
                    .all(|id| self.program.relation(id).is_some()),
                "invariant" => obligation
                    .target_refs()
                    .iter()
                    .all(|id| self.program.invariant(id).is_some()),
                "subgraph" => {
                    obligation.applicability_status() == "unknown"
                        && obligation.version().rule() == "capability_gap.origin_rule@1"
                        && obligation
                            .applicability_reasons()
                            .iter()
                            .any(|reason| reason.starts_with("origin_rule:"))
                        && obligation.target_refs() == [self.program.snapshot_id().clone()]
                }
                _ => false,
            };
            if !target_kind_matches
                || obligation.normalized_context_ids().iter().any(|id| {
                    !self
                        .program
                        .contexts()
                        .iter()
                        .any(|context| context.id == *id)
                })
            {
                return Err(DomainError::Validation(
                    "obligation target and context IDs must use their declared ProgramSpace namespaces"
                        .to_owned(),
                ));
            }
        }
        for claim in self.claims.values() {
            insert_unique(&mut all_ids, &claim.id)?;
            if claim.disposition == ClaimDisposition::Proposed
                && claim.review_status != ReviewStatus::Unreviewed
            {
                return Err(DomainError::Validation(
                    "a proposed claim cannot be marked reviewed".to_owned(),
                ));
            }
            for obligation_id in &claim.obligation_ids {
                require_known("claim", &claim.id, &obligation_ids, obligation_id)?;
            }
            for source_id in &claim.source_ids {
                require_known("claim", &claim.id, &program_ids, source_id)?;
                if !claim.obligation_ids.iter().any(|obligation_id| {
                    self.obligations
                        .get(obligation_id)
                        .is_some_and(|obligation| {
                            self.source_connects_obligation(obligation, source_id)
                        })
                }) {
                    return Err(DomainError::Validation(
                        "claim source IDs must ground a claimed obligation".to_owned(),
                    ));
                }
            }
            for obligation_id in &claim.obligation_ids {
                let obligation = self.obligations.get(obligation_id).ok_or_else(|| {
                    DomainError::DanglingReference {
                        owner: "claim",
                        owner_id: claim.id.clone(),
                        reference: obligation_id.clone(),
                    }
                })?;
                if !claim
                    .source_ids
                    .iter()
                    .any(|source| self.source_connects_obligation(obligation, source))
                {
                    return Err(DomainError::Validation(
                        "every claimed obligation requires a grounding source".to_owned(),
                    ));
                }
            }
        }
        for evidence in self.evidence.values() {
            insert_unique(&mut all_ids, evidence.id())?;
            // Current-snapshot evidence must remain grounded in the active
            // ProgramSpace. Historical observations are retained verbatim for
            // audit even when their old target was later deleted or renamed;
            // they cannot connect to a current obligation unless a current
            // target reference actually resolves in the binding check below.
            if evidence.snapshot_id() == self.program.snapshot_id() {
                for target_id in evidence.target_ids() {
                    require_known("evidence", evidence.id(), &program_ids, target_id)?;
                }
            }
        }
        let evidence_ids = self.evidence.keys().cloned().collect::<BTreeSet<_>>();
        for binding in self.bindings.values() {
            insert_unique(&mut all_ids, &binding.id)?;
            require_known(
                "evidence binding",
                &binding.id,
                &self.claims.keys().cloned().collect(),
                &binding.claim_id,
            )?;
            require_known(
                "evidence binding",
                &binding.id,
                &evidence_ids,
                &binding.evidence_id,
            )?;
            self.validate_binding_connection(binding)?;
        }
        for verification in self.verifications.values() {
            insert_unique(&mut all_ids, &verification.id)?;
            require_known(
                "verification",
                &verification.id,
                &self.claims.keys().cloned().collect(),
                &verification.claim_id,
            )?;
            for evidence_id in &verification.evidence_ids {
                require_known("verification", &verification.id, &evidence_ids, evidence_id)?;
            }
            if verification.freshness != self.derived_freshness(verification)? {
                return Err(DomainError::Validation(
                    "verification freshness must be derived from its cited snapshot-bound evidence"
                        .to_owned(),
                ));
            }
            self.validate_connected_verification(verification)?;
        }
        for decision in self.decisions.values() {
            decision.validate_event_admission()?;
            let known_before_decision = all_ids.clone();
            insert_unique(&mut all_ids, &decision.id)?;
            require_known(
                "decision",
                &decision.id,
                &self.claims.keys().cloned().collect(),
                &decision.target_claim_id,
            )?;
            for source_id in &decision.source_ids {
                require_known("decision", &decision.id, &known_before_decision, source_id)?;
            }
            if decision.outcome == DecisionOutcome::Accept {
                self.acceptance_chain(decision.target_claim_id(), decision)?;
            } else if decision.outcome == DecisionOutcome::Reject
                && !decision.source_ids.contains(decision.target_claim_id())
            {
                return Err(DomainError::Validation(
                    "reject decision must explicitly trace to its target claim".to_owned(),
                ));
            }
        }
        for finding in self.findings.values() {
            insert_unique(&mut all_ids, &finding.id)?;
            require_known(
                "finding",
                &finding.id,
                &self.claims.keys().cloned().collect(),
                &finding.claim_id,
            )?;
            for evidence_id in &finding.evidence_ids {
                require_known("finding", &finding.id, &evidence_ids, evidence_id)?;
            }
            for verification_id in &finding.verification_ids {
                require_known(
                    "finding",
                    &finding.id,
                    &self.verifications.keys().cloned().collect(),
                    verification_id,
                )?;
            }
            for source_id in &finding.source_ids {
                require_known("finding", &finding.id, &program_ids, source_id)?;
            }
            self.validate_finding(finding)?;
        }
        for claim in self.claims.values() {
            if claim.disposition == ClaimDisposition::Accepted {
                self.validate_accepted_claim(claim)?;
            }
        }
        for plan in self.plans.values() {
            if plan.snapshot_id() != self.program.snapshot_id()
                || plan.universe_id() != self.universe.id()
            {
                return Err(DomainError::Validation(
                    "recorded plan must bind the aggregate snapshot and universe".to_owned(),
                ));
            }
            let scheduled = plan
                .waves()
                .iter()
                .flat_map(|wave| wave.obligation_ids().iter().cloned())
                .chain(plan.deferred().keys().cloned())
                .collect::<BTreeSet<_>>();
            if scheduled != obligation_ids
                || plan
                    .risk_breakdown()
                    .keys()
                    .cloned()
                    .collect::<BTreeSet<_>>()
                    != obligation_ids
            {
                return Err(DomainError::Validation(
                    "recorded plan must preserve the complete obligation denominator".to_owned(),
                ));
            }
        }
        for envelope in self.envelopes.values() {
            envelope.validate_for_event(self)?;
        }
        for envelope in self.offline_execution_envelopes.values() {
            if self.envelopes.contains_key(envelope.id()) {
                return Err(DomainError::IdCollision {
                    id: envelope.id().clone(),
                });
            }
            envelope.validate_for_event(self)?;
        }
        for execution in self.executions.values() {
            insert_unique(&mut all_ids, execution.id())?;
            execution.validate_shape()?;
            self.validate_execution_closure(execution)?;
            let expected_size = self
                .execution_raw_sizes
                .get(execution.id())
                .ok_or_else(|| DomainError::DanglingReference {
                    owner: "execution raw size",
                    owner_id: execution.id().clone(),
                    reference: execution.raw_artifact_registration_id().clone(),
                })?;
            let registration = self
                .artifact_registration(execution.raw_artifact_registration_id())
                .ok_or_else(|| DomainError::DanglingReference {
                    owner: "execution raw size",
                    owner_id: execution.id().clone(),
                    reference: execution.raw_artifact_registration_id().clone(),
                })?;
            if registration.size() != *expected_size {
                return Err(DomainError::Validation(
                    "D2 registration size must equal the preserved raw byte size".to_owned(),
                ));
            }
        }
        if self.execution_raw_sizes.len() != self.executions.len() {
            return Err(DomainError::Validation(
                "D2 preserved raw sizes must exactly match executions".to_owned(),
            ));
        }
        for claim in self.execution_claims.values() {
            insert_unique(&mut all_ids, claim.id())?;
            claim.validate_shape()?;
            self.validate_execution_claim_closure(claim)?;
        }
        Ok(())
    }

    pub(crate) fn validate_pristine_for_event_log(&self) -> Result<()> {
        if !self.claims.is_empty()
            || !self.bindings.is_empty()
            || !self.verifications.is_empty()
            || !self.decisions.is_empty()
            || !self.findings.is_empty()
            || !self.plans.is_empty()
            || !self.envelopes.is_empty()
            || !self.offline_execution_envelopes.is_empty()
            || !self.executions.is_empty()
            || !self.execution_claims.is_empty()
            || !self.execution_raw_sizes.is_empty()
            || self
                .obligations
                .values()
                .any(|obligation| obligation.lifecycle != ObligationLifecycle::Generated)
        {
            return Err(DomainError::Validation(
                "event log genesis must be a pristine aggregate without review state".to_owned(),
            ));
        }
        let seeded = self.program.evidence();
        if self.evidence.len() != seeded.len()
            || self
                .evidence
                .iter()
                .zip(seeded)
                .any(|((id, retained), expected)| id != expected.id() || retained != expected)
        {
            return Err(DomainError::Validation(
                "event log genesis evidence must exactly equal ProgramSpace-seeded evidence"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    pub(crate) fn record_genesis_manifest(
        &mut self,
        expected_run_id: &StableId,
        manifest: RunGenesisManifest,
    ) -> Result<()> {
        if manifest.run_id() != expected_run_id
            || manifest.genesis_artifact().run_id() != expected_run_id
        {
            return Err(DomainError::Validation(
                "genesis manifest artifacts must bind the enclosing event run".to_owned(),
            ));
        }
        if self.genesis_manifest.is_some() {
            return Err(DomainError::IdCollision {
                id: manifest.run_id().clone(),
            });
        }
        if manifest.snapshot_id() != self.program.snapshot_id() {
            return Err(DomainError::Validation(
                "genesis manifest snapshot must match the aggregate snapshot".to_owned(),
            ));
        }
        self.register_artifact(expected_run_id, manifest.genesis_artifact().clone())?;
        self.genesis_manifest = Some(manifest);
        Ok(())
    }

    pub(crate) fn register_artifact(
        &mut self,
        expected_run_id: &StableId,
        registration: ArtifactRegistered,
    ) -> Result<()> {
        if registration.run_id() != expected_run_id {
            return Err(DomainError::Validation(
                "artifact registration must bind the enclosing event run".to_owned(),
            ));
        }
        let id = registration.registration_id().clone();
        if self.registered_artifacts.contains_key(&id)
            || self.registered_artifacts_v3.contains_key(&id)
        {
            return Err(DomainError::IdCollision { id });
        }
        self.registered_artifacts.insert(id, registration);
        Ok(())
    }

    pub(crate) fn register_artifact_v3(
        &mut self,
        expected_run_id: &StableId,
        registration: ArtifactRegisteredV3,
    ) -> Result<()> {
        if registration.run_id() != expected_run_id {
            return Err(DomainError::Validation(
                "v3 artifact registration must bind the enclosing event run".to_owned(),
            ));
        }
        let id = registration.registration_id().clone();
        if self.registered_artifacts.contains_key(&id)
            || self.registered_artifacts_v3.contains_key(&id)
        {
            return Err(DomainError::IdCollision { id });
        }
        self.registered_artifacts_v3.insert(id, registration);
        Ok(())
    }

    pub(crate) fn artifact_registration(
        &self,
        id: &StableId,
    ) -> Option<ArtifactRegistrationRef<'_>> {
        self.registered_artifacts
            .get(id)
            .map(ArtifactRegistrationRef::V2)
            .or_else(|| {
                self.registered_artifacts_v3
                    .get(id)
                    .map(ArtifactRegistrationRef::V3)
            })
    }

    pub(crate) fn record_snapshot_sources(
        &mut self,
        sources: SnapshotSourcesRecorded,
    ) -> Result<()> {
        if sources.snapshot_id() != self.program.snapshot_id() {
            return Err(DomainError::Validation(
                "snapshot source record must name the aggregate snapshot".to_owned(),
            ));
        }
        let expected = self
            .program
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
            .map(|artifact| {
                let location = artifact.location.as_ref().ok_or_else(|| {
                    DomainError::Validation("accepted file artifact has no location".to_owned())
                })?;
                let content_hash = artifact.content_hash.as_ref().ok_or_else(|| {
                    DomainError::Validation("accepted file artifact has no content hash".to_owned())
                })?;
                Ok((
                    artifact.id.clone(),
                    (location.path.clone(), content_hash.clone()),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let actual = sources
            .entries()
            .iter()
            .map(|entry| {
                let registration = self
                    .artifact_registration(entry.registration_id())
                    .ok_or_else(|| DomainError::DanglingReference {
                        owner: "snapshot source",
                        owner_id: entry.artifact_id().clone(),
                        reference: entry.registration_id().clone(),
                    })?;
                if registration.cas_hash() != entry.cas_hash() {
                    return Err(DomainError::DanglingReference {
                        owner: "snapshot source",
                        owner_id: entry.artifact_id().clone(),
                        reference: entry.registration_id().clone(),
                    });
                }
                if registration.sensitivity() != crate::ArtifactSensitivity::WorkspaceSource
                    || !registration.is_snapshot_ingest(sources.snapshot_id())
                {
                    return Err(DomainError::Validation(
                        "snapshot source registrations must be workspace-source snapshot-ingest artifacts"
                            .to_owned(),
                    ));
                }
                Ok((
                    entry.artifact_id().clone(),
                    (entry.path().to_owned(), entry.content_hash().clone()),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        if actual != expected {
            return Err(DomainError::Validation(
                "snapshot source records must exactly match accepted file artifacts".to_owned(),
            ));
        }
        let snapshot = sources.snapshot_id().clone();
        if let Some(existing) = self.snapshot_sources.get(&snapshot) {
            if existing != &sources {
                return Err(DomainError::IdCollision { id: snapshot });
            }
            return Ok(());
        }
        self.snapshot_sources.insert(snapshot, sources);
        Ok(())
    }

    pub(crate) fn record_review_plan(&mut self, plan: ReviewPlan) -> Result<()> {
        plan.validate_against(self)?;
        let id = plan.id().clone();
        if let Some(existing) = self.plans.get(&id) {
            if existing.canonical_bytes()? != plan.canonical_bytes()? {
                return Err(DomainError::IdCollision { id });
            }
            return Ok(());
        }
        self.plans.insert(id, plan);
        Ok(())
    }

    pub(crate) fn record_context_envelope(
        &mut self,
        envelope: ReviewContextEnvelope,
    ) -> Result<()> {
        envelope.validate_for_event(self)?;
        let id = envelope.id().clone();
        if let Some(existing) = self.envelopes.get(&id) {
            if existing.canonical_bytes().map_err(context_domain_error)?
                != envelope.canonical_bytes().map_err(context_domain_error)?
            {
                return Err(DomainError::IdCollision { id });
            }
            return Ok(());
        }
        self.envelopes.insert(id, envelope);
        Ok(())
    }

    pub(crate) fn record_offline_execution_envelope(
        &mut self,
        envelope: ReviewContextEnvelope,
    ) -> Result<()> {
        envelope.validate_for_event(self)?;
        let id = envelope.id().clone();
        if self.envelopes.contains_key(&id) || self.offline_execution_envelopes.contains_key(&id) {
            return Err(DomainError::IdCollision { id });
        }
        self.offline_execution_envelopes.insert(id, envelope);
        Ok(())
    }

    pub(crate) fn record_execution(
        &mut self,
        expected_run_id: &StableId,
        execution: ExecutionRecord,
        claims: Vec<ExecutionClaimV2>,
        expected_raw_size: u64,
    ) -> Result<()> {
        execution.validate_shape()?;
        if self.executions.contains_key(execution.id()) || self.known_ids().contains(execution.id())
        {
            return Err(DomainError::IdCollision {
                id: execution.id().clone(),
            });
        }
        let previous_attempt = self
            .executions
            .values()
            .filter(|existing| existing.same_retry_series(&execution))
            .map(ExecutionRecord::attempt)
            .max();
        let expected_attempt = match previous_attempt {
            Some(attempt) => attempt
                .checked_add(1)
                .ok_or_else(|| DomainError::Validation("D2 retry attempt overflow".to_owned()))?,
            None => 1,
        };
        if execution.attempt() != expected_attempt {
            return Err(DomainError::Validation(
                "D2 attempts must start at one and increment without reuse or gaps".to_owned(),
            ));
        }
        let mut seen = self.known_ids();
        seen.insert(execution.id().clone());
        for claim in &claims {
            if !seen.insert(claim.id().clone()) || self.execution_claims.contains_key(claim.id()) {
                return Err(DomainError::IdCollision {
                    id: claim.id().clone(),
                });
            }
        }
        let obligation = execution
            .obligation_ids()
            .iter()
            .next()
            .and_then(|id| self.obligations.get(id))
            .ok_or_else(|| DomainError::DanglingReference {
                owner: "execution",
                owner_id: execution.id().clone(),
                reference: execution
                    .obligation_ids()
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or_else(|| execution.id().clone()),
            })?;
        if obligation.lifecycle() != ObligationLifecycle::InProgress {
            return Err(DomainError::Validation(
                "a D2 execution may be recorded only while its obligation is in_progress"
                    .to_owned(),
            ));
        }
        let registration = self
            .artifact_registration(execution.raw_artifact_registration_id())
            .ok_or_else(|| DomainError::DanglingReference {
                owner: "execution",
                owner_id: execution.id().clone(),
                reference: execution.raw_artifact_registration_id().clone(),
            })?;
        if registration.run_id() != expected_run_id
            || registration.cas_hash() != execution.raw_artifact_hash()
            || registration.size() != expected_raw_size
            || registration.sensitivity() != ArtifactSensitivity::Sensitive
            || !registration.is_reviewer_execution(
                expected_run_id,
                execution.id(),
                execution.reviewer_id(),
            )
        {
            return Err(DomainError::Validation(
                "D2 execution raw registration must close over run, execution, reviewer, hash, and sensitivity"
                    .to_owned(),
            ));
        }
        self.validate_execution_closure(&execution)?;
        let claim_ids = claims
            .iter()
            .map(|claim| claim.id().clone())
            .collect::<BTreeSet<_>>();
        if claim_ids != *execution.parsed_claim_ids()
            || execution.outcome().is_structured() == claims.is_empty()
        {
            return Err(DomainError::Validation(
                "atomic D2 execution claims must exactly match parsed_claim_ids and outcome"
                    .to_owned(),
            ));
        }
        self.executions
            .insert(execution.id().clone(), execution.clone());
        self.execution_raw_sizes
            .insert(execution.id().clone(), expected_raw_size);
        for claim in claims {
            self.validate_execution_claim_closure_with(&execution, &claim)?;
            self.execution_claims.insert(claim.id().clone(), claim);
        }
        self.validate()
    }

    /// Validates the complete authority-free closure of a D2 execution that
    /// will remain an offline shadow.  Unlike `record_execution`, this does
    /// not insert execution or claim state and therefore cannot confer review
    /// authority or resume capability.
    pub(crate) fn validate_offline_execution_shadow(
        &self,
        expected_run_id: &StableId,
        execution: &ExecutionRecord,
        claims: &[ExecutionClaimV2],
    ) -> Result<()> {
        execution.validate_shape()?;
        let obligation = execution
            .obligation_ids()
            .iter()
            .next()
            .and_then(|id| self.obligations.get(id))
            .ok_or_else(|| DomainError::DanglingReference {
                owner: "offline execution shadow",
                owner_id: execution.id().clone(),
                reference: execution
                    .obligation_ids()
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or_else(|| execution.id().clone()),
            })?;
        if obligation.lifecycle() != ObligationLifecycle::InProgress {
            return Err(DomainError::Validation(
                "an offline D2 execution shadow requires an in_progress obligation".to_owned(),
            ));
        }
        let registration = self
            .artifact_registration(execution.raw_artifact_registration_id())
            .ok_or_else(|| DomainError::DanglingReference {
                owner: "offline execution shadow",
                owner_id: execution.id().clone(),
                reference: execution.raw_artifact_registration_id().clone(),
            })?;
        if registration.run_id() != expected_run_id
            || registration.cas_hash() != execution.raw_artifact_hash()
            || registration.sensitivity() != ArtifactSensitivity::Sensitive
            || !registration.is_reviewer_execution(
                expected_run_id,
                execution.id(),
                execution.reviewer_id(),
            )
        {
            return Err(DomainError::Validation(
                "offline D2 execution shadow raw registration closure mismatch".to_owned(),
            ));
        }
        self.validate_execution_closure(execution)?;
        let claim_ids = claims
            .iter()
            .map(|claim| claim.id().clone())
            .collect::<BTreeSet<_>>();
        if claim_ids.len() != claims.len()
            || claim_ids != *execution.parsed_claim_ids()
            || execution.outcome().is_structured() == claims.is_empty()
        {
            return Err(DomainError::Validation(
                "offline D2 execution shadow claim set/outcome mismatch".to_owned(),
            ));
        }
        for claim in claims {
            self.validate_execution_claim_closure_with(execution, claim)?;
        }
        Ok(())
    }

    fn validate_execution_closure(&self, execution: &ExecutionRecord) -> Result<()> {
        let plan =
            self.plans
                .get(execution.plan_id())
                .ok_or_else(|| DomainError::DanglingReference {
                    owner: "execution",
                    owner_id: execution.id().clone(),
                    reference: execution.plan_id().clone(),
                })?;
        let wave = plan
            .waves()
            .iter()
            .find(|wave| wave.id() == execution.wave_id())
            .ok_or_else(|| DomainError::DanglingReference {
                owner: "execution",
                owner_id: execution.id().clone(),
                reference: execution.wave_id().clone(),
            })?;
        let envelope = self
            .envelopes
            .get(execution.envelope_id())
            .or_else(|| {
                self.offline_execution_envelopes
                    .get(execution.envelope_id())
            })
            .ok_or_else(|| DomainError::DanglingReference {
                owner: "execution",
                owner_id: execution.id().clone(),
                reference: execution.envelope_id().clone(),
            })?;
        let wave_ids = wave
            .obligation_ids()
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if execution.snapshot_id() != self.program.snapshot_id()
            || execution.snapshot_id() != plan.snapshot_id()
            || execution.snapshot_id() != envelope.snapshot_id()
            || execution.obligation_ids() != envelope.obligation_ids()
            || !execution.obligation_ids().is_subset(&wave_ids)
        {
            return Err(DomainError::Validation(
                "D2 execution plan/wave/envelope/snapshot/obligation closure mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    fn validate_execution_claim_closure(&self, claim: &ExecutionClaimV2) -> Result<()> {
        let execution = self.executions.get(claim.execution_id()).ok_or_else(|| {
            DomainError::DanglingReference {
                owner: "D2 claim",
                owner_id: claim.id().clone(),
                reference: claim.execution_id().clone(),
            }
        })?;
        self.validate_execution_claim_closure_with(execution, claim)
    }

    fn validate_execution_claim_closure_with(
        &self,
        execution: &ExecutionRecord,
        claim: &ExecutionClaimV2,
    ) -> Result<()> {
        if !execution.outcome().is_structured()
            || !execution.parsed_claim_ids().contains(claim.id())
            || claim.execution_id() != execution.id()
            || claim.obligation_ids() != execution.obligation_ids()
        {
            return Err(DomainError::Validation(
                "D2 claim must close over its structured atomic execution".to_owned(),
            ));
        }
        let obligation_id =
            execution
                .obligation_ids()
                .iter()
                .next()
                .ok_or(DomainError::EmptyField {
                    field: "D2 execution obligation_ids",
                })?;
        let obligation =
            self.obligations
                .get(obligation_id)
                .ok_or_else(|| DomainError::DanglingReference {
                    owner: "D2 claim",
                    owner_id: claim.id().clone(),
                    reference: obligation_id.clone(),
                })?;
        let envelope = self
            .envelopes
            .get(execution.envelope_id())
            .or_else(|| {
                self.offline_execution_envelopes
                    .get(execution.envelope_id())
            })
            .ok_or_else(|| DomainError::DanglingReference {
                owner: "D2 claim",
                owner_id: claim.id().clone(),
                reference: execution.envelope_id().clone(),
            })?;
        if claim.property_id() != obligation.property_id()
            || !claim
                .target_refs()
                .is_subset(obligation.normalized_target_refs())
            || !claim
                .source_ids()
                .is_subset(envelope.normalized_included_source_ids())
        {
            return Err(DomainError::Validation(
                "D2 claim property/target/source closure mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    /// Computes the immutable review closure authorized by one decision before
    /// the decision event is applied. The closure includes every current
    /// claim-local binding, evidence record, and verification, rather than
    /// merely the IDs cited by the decision. This prevents a same-ID splice
    /// from borrowing an admission minted for a different review state.
    pub(crate) fn decision_closure_digest(
        &self,
        genesis_hash: &crate::ContentHash,
        decision: &Decision,
    ) -> Result<crate::ContentHash> {
        let claim = self.claims.get(&decision.target_claim_id).ok_or_else(|| {
            DomainError::DanglingReference {
                owner: "decision",
                owner_id: decision.id.clone(),
                reference: decision.target_claim_id.clone(),
            }
        })?;
        if decision.outcome == DecisionOutcome::Accept {
            self.acceptance_chain(&claim.id, decision)?;
        }
        let obligations = claim
            .obligation_ids
            .iter()
            .map(|id| {
                self.obligations
                    .get(id)
                    .ok_or_else(|| DomainError::DanglingReference {
                        owner: "claim",
                        owner_id: claim.id.clone(),
                        reference: id.clone(),
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let bindings = self
            .bindings
            .values()
            .filter(|binding| binding.claim_id == claim.id)
            .collect::<Vec<_>>();
        let evidence_ids = bindings
            .iter()
            .map(|binding| binding.evidence_id.clone())
            .collect::<BTreeSet<_>>();
        let evidence = evidence_ids
            .iter()
            .map(|id| {
                self.evidence
                    .get(id)
                    .ok_or_else(|| DomainError::DanglingReference {
                        owner: "evidence binding",
                        owner_id: claim.id.clone(),
                        reference: id.clone(),
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let verifications = self
            .verifications
            .values()
            .filter(|verification| verification.claim_id == claim.id)
            .collect::<Vec<_>>();
        let closure = (
            "reviewgraphen.decision_admission_closure.v1",
            genesis_hash,
            self.universe.id(),
            decision,
            claim,
            obligations,
            bindings,
            evidence,
            verifications,
        );
        Ok(crate::ContentHash::sha256(&crate::canonical_json(
            &closure,
        )?))
    }

    pub(crate) fn validate_binding_for_event(&self, binding: &EvidenceBinding) -> Result<()> {
        if self.bindings.contains_key(&binding.id) {
            return Err(DomainError::IdCollision {
                id: binding.id.clone(),
            });
        }
        self.validate_binding_connection(binding)
    }

    pub(crate) fn validate_verification_for_event(
        &self,
        verification: &Verification,
    ) -> Result<()> {
        if self.verifications.contains_key(&verification.id) {
            return Err(DomainError::IdCollision {
                id: verification.id.clone(),
            });
        }
        if !self.claims.contains_key(&verification.claim_id) {
            return Err(DomainError::DanglingReference {
                owner: "verification",
                owner_id: verification.id.clone(),
                reference: verification.claim_id.clone(),
            });
        }
        for evidence_id in &verification.evidence_ids {
            if !self.evidence.contains_key(evidence_id) {
                return Err(DomainError::DanglingReference {
                    owner: "verification",
                    owner_id: verification.id.clone(),
                    reference: evidence_id.clone(),
                });
            }
        }
        if verification.freshness != self.derived_freshness(verification)? {
            return Err(DomainError::Validation(
                "verification freshness must match its cited evidence before admission".to_owned(),
            ));
        }
        self.validate_connected_verification(verification)
    }

    fn validate_accepted_claim(&self, claim: &ReviewClaim) -> Result<()> {
        let decision = self
            .decisions
            .values()
            .find(|decision| {
                decision.target_claim_id == claim.id && decision.outcome == DecisionOutcome::Accept
            })
            .ok_or_else(|| {
                DomainError::Validation(
                    "accepted claim requires an explicit accept decision".to_owned(),
                )
            })?;
        self.acceptance_chain(&claim.id, decision).map(|_| ())
    }

    fn validate_accepted_finding(&self, finding: &Finding) -> Result<()> {
        let claim = self.claims.get(&finding.claim_id).ok_or_else(|| {
            DomainError::Validation("accepted finding references an absent claim".to_owned())
        })?;
        if claim.disposition != ClaimDisposition::Accepted {
            return Err(DomainError::Validation(
                "accepted finding requires an accepted claim".to_owned(),
            ));
        }
        if finding.evidence_ids.is_empty() || finding.verification_ids.is_empty() {
            return Err(DomainError::Validation(
                "accepted finding must cite its supporting evidence and verification trace"
                    .to_owned(),
            ));
        }
        let decision_id = finding.decision_id.as_ref().ok_or_else(|| {
            DomainError::Validation("accepted finding requires an explicit decision".to_owned())
        })?;
        let decision = self.decisions.get(decision_id).ok_or_else(|| {
            DomainError::Validation("accepted finding has dangling decision".to_owned())
        })?;
        if decision.outcome != DecisionOutcome::Accept
            || decision.target_claim_id != finding.claim_id
        {
            return Err(DomainError::Validation(
                "accepted finding decision must accept its claim".to_owned(),
            ));
        }
        if !finding
            .evidence_ids
            .iter()
            .all(|id| decision.source_ids.contains(id))
            || !finding
                .verification_ids
                .iter()
                .all(|id| decision.source_ids.contains(id))
        {
            return Err(DomainError::Validation(
                "accepted finding traces must be explicitly cited by its accept decision"
                    .to_owned(),
            ));
        }
        self.acceptance_chain(&finding.claim_id, decision)?;
        self.validate_finding_trace(finding, true)?;
        self.validate_finding_trace_for_each_obligation(finding, true)?;
        Ok(())
    }

    fn validate_finding(&self, finding: &Finding) -> Result<()> {
        if finding.source_ids.is_empty() {
            return Err(DomainError::Validation(
                "finding requires non-empty program source IDs".to_owned(),
            ));
        }
        let claim = self.claims.get(&finding.claim_id).ok_or_else(|| {
            DomainError::Validation("finding references an absent claim".to_owned())
        })?;
        if claim.polarity != ClaimPolarity::IssuePresent {
            return Err(DomainError::Validation(
                "an M1 finding requires an issue_present claim polarity".to_owned(),
            ));
        }
        if !finding.source_ids.is_subset(&claim.source_ids)
            || !finding
                .source_ids
                .iter()
                .all(|source| self.source_connects_claim(claim, source))
        {
            return Err(DomainError::Validation(
                "finding source IDs must be a connected subset of its claim sources".to_owned(),
            ));
        }
        for obligation_id in &claim.obligation_ids {
            let obligation = self.obligations.get(obligation_id).ok_or_else(|| {
                DomainError::DanglingReference {
                    owner: "claim",
                    owner_id: claim.id.clone(),
                    reference: obligation_id.clone(),
                }
            })?;
            if !finding
                .source_ids
                .iter()
                .any(|source| self.source_connects_obligation(obligation, source))
            {
                return Err(DomainError::Validation(
                    "finding sources must ground every claimed obligation".to_owned(),
                ));
            }
        }
        match finding.status {
            FindingStatus::UnverifiedCandidate => {
                if matches!(
                    claim.disposition,
                    ClaimDisposition::Refuted
                        | ClaimDisposition::Rejected
                        | ClaimDisposition::Accepted
                ) {
                    return Err(DomainError::Validation(
                        "an unverified finding must agree with a non-rejected, non-accepted claim"
                            .to_owned(),
                    ));
                }
                if !finding.verification_ids.is_empty() || finding.decision_id.is_some() {
                    return Err(DomainError::Validation(
                        "an unverified finding cannot cite verification or decision traces"
                            .to_owned(),
                    ));
                }
                if self.decisions.values().any(|decision| {
                    decision.target_claim_id == finding.claim_id
                        && decision.outcome == DecisionOutcome::Accept
                }) {
                    return Err(DomainError::Validation(
                        "an unverified finding cannot represent an already accepted claim"
                            .to_owned(),
                    ));
                }
                self.validate_finding_trace(finding, false)
            }
            FindingStatus::VerifiedCandidate => {
                if claim.disposition != ClaimDisposition::Supported {
                    return Err(DomainError::Validation(
                        "a verified candidate requires a supported claim disposition".to_owned(),
                    ));
                }
                if finding.verification_ids.is_empty() || finding.decision_id.is_some() {
                    return Err(DomainError::Validation(
                        "a verified candidate requires verification and no decision".to_owned(),
                    ));
                }
                if self.decisions.values().any(|decision| {
                    decision.target_claim_id == finding.claim_id
                        && decision.outcome == DecisionOutcome::Accept
                }) {
                    return Err(DomainError::Validation(
                        "a verified candidate cannot omit an existing accept decision".to_owned(),
                    ));
                }
                self.validate_finding_trace(finding, false)?;
                self.validate_finding_trace_for_each_obligation(finding, false)
            }
            FindingStatus::Accepted => self.validate_accepted_finding(finding),
            FindingStatus::Rejected => {
                let decision_id = finding.decision_id.as_ref().ok_or_else(|| {
                    DomainError::Validation(
                        "a rejected finding requires a reject decision".to_owned(),
                    )
                })?;
                let decision = self.decisions.get(decision_id).ok_or_else(|| {
                    DomainError::Validation("rejected finding has dangling decision".to_owned())
                })?;
                if decision.outcome != DecisionOutcome::Reject
                    || decision.target_claim_id != finding.claim_id
                    || claim.disposition != ClaimDisposition::Rejected
                {
                    return Err(DomainError::Validation(
                        "rejected finding must agree with its rejected claim and decision"
                            .to_owned(),
                    ));
                }
                self.validate_finding_trace(finding, false)
            }
            FindingStatus::Resolved | FindingStatus::Superseded => Err(DomainError::Validation(
                "resolved and superseded findings are not admitted by the M1 event contract"
                    .to_owned(),
            )),
        }
    }

    fn validate_finding_trace(&self, finding: &Finding, require_fresh: bool) -> Result<()> {
        for evidence_id in &finding.evidence_ids {
            if !self.bindings.values().any(|binding| {
                binding.claim_id == finding.claim_id && binding.evidence_id == *evidence_id
            }) {
                return Err(DomainError::Validation(
                    "every finding evidence ID must belong to its claim binding chain".to_owned(),
                ));
            }
        }
        for verification_id in &finding.verification_ids {
            let verification = self.verifications.get(verification_id).ok_or_else(|| {
                DomainError::Validation("finding has dangling verification".to_owned())
            })?;
            if verification.claim_id != finding.claim_id
                || verification.outcome != VerificationOutcome::Passed
                || (require_fresh && verification.freshness != Freshness::Fresh)
                || !verification.evidence_ids.is_subset(&finding.evidence_ids)
            {
                return Err(DomainError::Validation(
                    "every finding verification must be a connected passed trace for its claim"
                        .to_owned(),
                ));
            }
            self.validate_connected_verification(verification)?;
        }
        if !finding.verification_ids.is_empty()
            && !finding.evidence_ids.is_empty()
            && !finding.evidence_ids.iter().all(|evidence_id| {
                finding.verification_ids.iter().any(|verification_id| {
                    self.verifications
                        .get(verification_id)
                        .is_some_and(|verification| verification.evidence_ids.contains(evidence_id))
                })
            })
        {
            return Err(DomainError::Validation(
                "every finding evidence trace must be cited by its finding verification trace"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn validate_finding_trace_for_each_obligation(
        &self,
        finding: &Finding,
        require_fresh: bool,
    ) -> Result<()> {
        let claim =
            self.claims
                .get(&finding.claim_id)
                .ok_or_else(|| DomainError::DanglingReference {
                    owner: "finding",
                    owner_id: finding.id.clone(),
                    reference: finding.claim_id.clone(),
                })?;
        for obligation_id in &claim.obligation_ids {
            let obligation = self.obligations.get(obligation_id).ok_or_else(|| {
                DomainError::DanglingReference {
                    owner: "claim",
                    owner_id: claim.id.clone(),
                    reference: obligation_id.clone(),
                }
            })?;
            let has_trace = finding.verification_ids.iter().any(|verification_id| {
                self.verifications
                    .get(verification_id)
                    .is_some_and(|verification| {
                        verification.claim_id == claim.id
                            && verification.outcome == VerificationOutcome::Passed
                            && (!require_fresh || verification.freshness == Freshness::Fresh)
                            && verification.evidence_ids.iter().all(|evidence_id| {
                                finding.evidence_ids.contains(evidence_id)
                                    && self.bindings.values().any(|binding| {
                                        binding.claim_id == claim.id
                                            && binding.evidence_id == *evidence_id
                                            && matches!(
                                                binding.relation,
                                                EvidenceRelation::Supports
                                                    | EvidenceRelation::Reproduces
                                            )
                                            && self
                                                .binding_connects_to_obligation(binding, obligation)
                                    })
                            })
                    })
            });
            if !has_trace {
                return Err(DomainError::Validation(
                    "finding requires an obligation-specific connected verification trace for every claimed obligation"
                        .to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn acceptance_chain(&self, claim_id: &StableId, decision: &Decision) -> Result<()> {
        if decision.outcome != DecisionOutcome::Accept
            || decision.authority_kind != DecisionAuthority::Human
            || decision.target_claim_id != *claim_id
        {
            return Err(DomainError::Validation(
                "accepted decisions require an accountable human authority for their target claim"
                    .to_owned(),
            ));
        }
        if !decision.source_ids.contains(claim_id) {
            return Err(DomainError::Validation(
                "accepted decision must trace to its claim".to_owned(),
            ));
        }
        let claim = self
            .claims
            .get(claim_id)
            .ok_or_else(|| DomainError::DanglingReference {
                owner: "decision",
                owner_id: decision.id.clone(),
                reference: claim_id.clone(),
            })?;
        for obligation_id in &claim.obligation_ids {
            let obligation = self.obligations.get(obligation_id).ok_or_else(|| {
                DomainError::DanglingReference {
                    owner: "claim",
                    owner_id: claim.id.clone(),
                    reference: obligation_id.clone(),
                }
            })?;
            self.acceptance_chain_for_obligation(claim_id, obligation, decision)?;
        }
        Ok(())
    }

    fn acceptance_chain_for_obligation(
        &self,
        claim_id: &StableId,
        obligation: &Obligation,
        decision: &Decision,
    ) -> Result<()> {
        for binding in self.bindings.values() {
            if binding.claim_id != *claim_id
                || !matches!(
                    binding.relation,
                    EvidenceRelation::Supports | EvidenceRelation::Reproduces
                )
                || !self.binding_connects_to_obligation(binding, obligation)
            {
                continue;
            }
            for verification in self.verifications.values() {
                if verification.claim_id != *claim_id
                    || verification.outcome != VerificationOutcome::Passed
                    || verification.freshness != Freshness::Fresh
                    || !verification.evidence_ids.contains(&binding.evidence_id)
                {
                    continue;
                }
                if verification.evidence_ids.iter().all(|evidence_id| {
                    self.bindings.values().any(|candidate| {
                        candidate.claim_id == *claim_id
                            && candidate.evidence_id == *evidence_id
                            && matches!(
                                candidate.relation,
                                EvidenceRelation::Supports | EvidenceRelation::Reproduces
                            )
                            && self.binding_connects_to_obligation(candidate, obligation)
                    })
                }) && decision.source_ids.contains(&binding.evidence_id)
                    && decision.source_ids.contains(&verification.id)
                    && verification
                        .evidence_ids
                        .iter()
                        .all(|evidence_id| decision.source_ids.contains(evidence_id))
                {
                    return Ok(());
                }
            }
        }
        Err(DomainError::Validation(
            "accepted decision requires a joined fresh evidence and verification trace for every claimed obligation"
                .to_owned(),
        ))
    }

    fn derived_freshness(&self, verification: &Verification) -> Result<Freshness> {
        if verification.evidence_ids.is_empty() {
            return Ok(Freshness::Unknown);
        }
        let mut is_stale = false;
        for evidence_id in &verification.evidence_ids {
            let evidence =
                self.evidence
                    .get(evidence_id)
                    .ok_or_else(|| DomainError::DanglingReference {
                        owner: "verification",
                        owner_id: verification.id.clone(),
                        reference: evidence_id.clone(),
                    })?;
            if evidence.snapshot_id() != self.program.snapshot_id() {
                is_stale = true;
            }
        }
        Ok(if is_stale {
            Freshness::Stale
        } else {
            Freshness::Fresh
        })
    }

    fn validate_connected_verification(&self, verification: &Verification) -> Result<()> {
        if verification.outcome != VerificationOutcome::Passed {
            return Ok(());
        }
        if verification.evidence_ids.is_empty() {
            return Err(DomainError::Validation(
                "a passed verification requires evidence".to_owned(),
            ));
        }
        for evidence_id in &verification.evidence_ids {
            if !self.bindings.values().any(|binding| {
                binding.claim_id == verification.claim_id
                    && binding.evidence_id == *evidence_id
                    && matches!(
                        binding.relation,
                        EvidenceRelation::Supports | EvidenceRelation::Reproduces
                    )
                    && self.binding_connects_to_an_obligation(binding).is_ok()
            }) {
                return Err(DomainError::Validation(
                    "a passed verification requires every evidence ID in a connected supporting claim chain"
                        .to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn validate_binding_connection(&self, binding: &EvidenceBinding) -> Result<()> {
        self.binding_connects_to_an_obligation(binding).map(|_| ())
    }

    fn binding_connects_to_an_obligation(&self, binding: &EvidenceBinding) -> Result<StableId> {
        let claim =
            self.claims
                .get(&binding.claim_id)
                .ok_or_else(|| DomainError::DanglingReference {
                    owner: "evidence binding",
                    owner_id: binding.id.clone(),
                    reference: binding.claim_id.clone(),
                })?;
        let _evidence = self.evidence.get(&binding.evidence_id).ok_or_else(|| {
            DomainError::DanglingReference {
                owner: "evidence binding",
                owner_id: binding.id.clone(),
                reference: binding.evidence_id.clone(),
            }
        })?;
        claim.obligation_ids.iter().find(|obligation_id| {
            self.obligations.get(*obligation_id).is_some_and(|obligation| {
                self.binding_connects_to_obligation(binding, obligation)
            })
        }).cloned().ok_or_else(|| DomainError::Validation(
            "evidence binding target, property, context, and evidence mode must connect to a claimed obligation".to_owned(),
        ))
    }

    fn binding_connects_to_obligation(
        &self,
        binding: &EvidenceBinding,
        obligation: &Obligation,
    ) -> bool {
        let Some(evidence) = self.evidence.get(&binding.evidence_id) else {
            return false;
        };
        binding.scope.get("property_id") == Some(&obligation.property_id)
            && obligation.accepted_evidence_modes.contains(evidence.kind())
            && evidence.target_ids().iter().any(|target| {
                obligation.normalized_target_refs.contains(target)
                    || obligation.normalized_source_ids.contains(target)
                    || obligation.normalized_context_ids.contains(target)
            })
    }

    fn source_connects_claim(&self, claim: &ReviewClaim, source: &StableId) -> bool {
        claim.obligation_ids.iter().any(|obligation_id| {
            self.obligations
                .get(obligation_id)
                .is_some_and(|obligation| self.source_connects_obligation(obligation, source))
        })
    }

    fn source_connects_obligation(&self, obligation: &Obligation, source: &StableId) -> bool {
        obligation.normalized_source_ids.contains(source)
            || obligation.normalized_target_refs.contains(source)
            || obligation.normalized_context_ids.contains(source)
            || obligation.normalized_context_ids.iter().any(|context_id| {
                self.program
                    .contexts()
                    .iter()
                    .find(|context| context.id == *context_id)
                    .is_some_and(|context| context.member_ids.contains(source))
            })
    }

    /// Immutable program facts.
    #[must_use]
    pub fn program(&self) -> &ProgramSpace {
        &self.program
    }

    /// Versioned obligation universe.
    #[must_use]
    pub fn universe(&self) -> &UniverseDescriptor {
        &self.universe
    }

    /// Obligations in stable ID order.
    pub fn obligations(&self) -> impl Iterator<Item = &Obligation> {
        self.obligations.values()
    }

    /// Internal context-builder lookup; registrations are event-derived
    /// metadata and remain unavailable as a general mutation surface.
    #[cfg(test)]
    pub(crate) fn registered_artifact(&self, id: &StableId) -> Option<&ArtifactRegistered> {
        self.registered_artifacts.get(id)
    }

    /// Internal context-builder lookup for the exact snapshot source closure.
    pub(crate) fn snapshot_sources_for(
        &self,
        snapshot_id: &StableId,
    ) -> Option<&SnapshotSourcesRecorded> {
        self.snapshot_sources.get(snapshot_id)
    }

    /// Resolves exactly one accepted source closure for the aggregate's
    /// current snapshot. Missing or mismatched links are a typed dangling
    /// reference; callers cannot observe the backing metadata maps.
    pub fn resolve_context_source(
        &self,
        artifact_id: &StableId,
    ) -> Result<ContextSourceRegistration> {
        let snapshot_id = self.program.snapshot_id();
        let missing = || DomainError::DanglingReference {
            owner: "runtime context source",
            owner_id: snapshot_id.clone(),
            reference: artifact_id.clone(),
        };
        let sources = self.snapshot_sources.get(snapshot_id).ok_or_else(missing)?;
        let entry = sources
            .entries()
            .iter()
            .find(|entry| entry.artifact_id() == artifact_id)
            .ok_or_else(missing)?;
        let registration = self
            .artifact_registration(entry.registration_id())
            .ok_or_else(missing)?;
        if registration.cas_hash() != entry.cas_hash()
            || !registration.is_snapshot_ingest(snapshot_id)
        {
            return Err(missing());
        }
        let source = registration
            .snapshot_source_projection()
            .ok_or_else(missing)?;
        Ok(ContextSourceRegistration {
            artifact_id: entry.artifact_id().clone(),
            registration_id: entry.registration_id().clone(),
            content_hash: entry.content_hash().clone(),
            cas_hash: entry.cas_hash().clone(),
            line_count: entry.line_count(),
            size: registration.size(),
            media_type: registration.media_type().to_owned(),
            sensitivity: registration.sensitivity(),
            source,
        })
    }

    /// Projects only persisted fake-attempt progress for one exact execution
    /// ID. It has no replay admission or mutation capability.
    pub fn fake_attempt_state(&self, execution_id: &StableId) -> FakeAttemptState {
        if let Some(execution) = self.executions.get(execution_id) {
            let completed = execution.outcome().is_structured()
                && execution.obligation_ids().iter().all(|id| {
                    self.obligations.get(id).is_some_and(|obligation| {
                        obligation.lifecycle() == ObligationLifecycle::Completed
                    })
                });
            return if completed {
                FakeAttemptState::Completed {
                    execution: execution.clone(),
                }
            } else {
                FakeAttemptState::ExecutionRecorded {
                    execution: execution.clone(),
                }
            };
        }
        let mut matched = None;
        for registration in self.registered_artifacts.values() {
            if matches!(registration.source(), ArtifactSource::ReviewerExecution { execution_id: recorded, .. } if recorded == execution_id)
            {
                if matched.is_some() {
                    return FakeAttemptState::AmbiguousRawRegistrations;
                }
                matched = Some(RawArtifactRegistration::V2(registration.clone()));
            }
        }
        for registration in self.registered_artifacts_v3.values() {
            if matches!(registration.source(), ArtifactSourceV3::ReviewerExecution { execution_id: recorded, .. } if recorded == execution_id)
            {
                if matched.is_some() {
                    return FakeAttemptState::AmbiguousRawRegistrations;
                }
                matched = Some(RawArtifactRegistration::V3(Box::new(registration.clone())));
            }
        }
        match matched {
            Some(registration) => FakeAttemptState::RawRegistered { registration },
            None => FakeAttemptState::None,
        }
    }

    /// Counts durable registrations in this run that reference one exact CAS
    /// hash. This is intentionally a scalar read-only projection: callers
    /// cannot enumerate registrations or obtain an admission/mutation handle.
    #[must_use]
    pub fn artifact_registration_count_for_cas_hash(&self, cas_hash: &ContentHash) -> usize {
        self.registered_artifacts
            .values()
            .filter(|registration| registration.cas_hash() == cas_hash)
            .count()
            .saturating_add(
                self.registered_artifacts_v3
                    .values()
                    .filter(|registration| registration.cas_hash() == cas_hash)
                    .count(),
            )
    }

    /// Internal exact obligation lookup used by deterministic projections.
    pub(crate) fn obligation(&self, id: &StableId) -> Option<&Obligation> {
        self.obligations.get(id)
    }

    /// Recorded deterministic plans in StableId order.
    pub fn review_plans(&self) -> impl Iterator<Item = &ReviewPlan> {
        self.plans.values()
    }

    /// Recorded live context envelopes in StableId order. Offline metadata-only
    /// projection never enters this map.
    pub fn context_envelopes(&self) -> impl Iterator<Item = &ReviewContextEnvelope> {
        self.envelopes.values()
    }

    #[must_use]
    pub fn review_plan(&self, id: &StableId) -> Option<&ReviewPlan> {
        self.plans.get(id)
    }

    #[must_use]
    pub fn context_envelope(&self, id: &StableId) -> Option<&ReviewContextEnvelope> {
        self.envelopes.get(id)
    }

    /// Authority-free D2 executions in stable ID order.
    pub fn executions(&self) -> impl Iterator<Item = &ExecutionRecord> {
        self.executions.values()
    }

    pub(crate) fn execution(&self, id: &StableId) -> Option<&ExecutionRecord> {
        self.executions.get(id)
    }

    pub(crate) const fn execution_claim_map(&self) -> &BTreeMap<StableId, ExecutionClaimV2> {
        &self.execution_claims
    }

    /// Authority-free, always-proposed D2 claims in stable ID order.
    pub fn execution_claims(&self) -> impl Iterator<Item = &ExecutionClaimV2> {
        self.execution_claims.values()
    }

    #[cfg(test)]
    pub(crate) fn install_context_metadata_for_test(
        &mut self,
        registrations: Vec<ArtifactRegistered>,
        sources: SnapshotSourcesRecorded,
    ) {
        self.registered_artifacts = registrations
            .into_iter()
            .map(|registration| (registration.registration_id().clone(), registration))
            .collect();
        self.snapshot_sources
            .insert(sources.snapshot_id().clone(), sources);
    }

    #[cfg(test)]
    pub(crate) fn replace_snapshot_sources_for_test(&mut self, sources: SnapshotSourcesRecorded) {
        self.snapshot_sources
            .insert(sources.snapshot_id().clone(), sources);
    }

    /// Claims in stable ID order.
    pub fn claims(&self) -> impl Iterator<Item = &ReviewClaim> {
        self.claims.values()
    }

    /// Verifications in stable ID order.
    pub fn verifications(&self) -> impl Iterator<Item = &Verification> {
        self.verifications.values()
    }

    /// Evidence bindings in stable ID order.
    pub fn bindings(&self) -> impl Iterator<Item = &EvidenceBinding> {
        self.bindings.values()
    }

    /// Decisions in stable ID order.
    pub fn decisions(&self) -> impl Iterator<Item = &Decision> {
        self.decisions.values()
    }

    /// Findings in stable ID order.
    pub fn findings(&self) -> impl Iterator<Item = &Finding> {
        self.findings.values()
    }

    pub(crate) fn obligations_with_connected_verification(
        &self,
        require_fresh: bool,
    ) -> BTreeSet<StableId> {
        self.obligations
            .values()
            .filter(|obligation| {
                self.claims.values().any(|claim| {
                    claim.obligation_ids.contains(&obligation.id)
                        && self.verifications.values().any(|verification| {
                            verification.claim_id == claim.id
                                && verification.outcome == VerificationOutcome::Passed
                                && (!require_fresh || verification.freshness == Freshness::Fresh)
                                && verification.evidence_ids.iter().all(|evidence_id| {
                                    self.bindings.values().any(|binding| {
                                        binding.claim_id == claim.id
                                            && binding.evidence_id == *evidence_id
                                            && matches!(
                                                binding.relation,
                                                EvidenceRelation::Supports
                                                    | EvidenceRelation::Reproduces
                                            )
                                            && self
                                                .binding_connects_to_obligation(binding, obligation)
                                    })
                                })
                        })
                })
            })
            .map(|obligation| obligation.id.clone())
            .collect()
    }

    pub(crate) fn obligations_with_connected_support(&self) -> BTreeSet<StableId> {
        self.obligations
            .values()
            .filter(|obligation| {
                self.claims.values().any(|claim| {
                    matches!(
                        claim.disposition,
                        ClaimDisposition::Supported | ClaimDisposition::Accepted
                    ) && claim.obligation_ids.contains(&obligation.id)
                        && self.bindings.values().any(|binding| {
                            binding.claim_id == claim.id
                                && matches!(
                                    binding.relation,
                                    EvidenceRelation::Supports | EvidenceRelation::Reproduces
                                )
                                && self.evidence_is_current(&binding.evidence_id)
                                && self.binding_connects_to_obligation(binding, obligation)
                        })
                })
            })
            .map(|obligation| obligation.id.clone())
            .collect()
    }

    pub(crate) fn obligations_with_human_acceptance(&self) -> BTreeSet<StableId> {
        self.obligations
            .values()
            .filter(|obligation| {
                self.claims.values().any(|claim| {
                    claim.disposition == ClaimDisposition::Accepted
                        && claim.review_status == ReviewStatus::Accepted
                        && claim.obligation_ids.contains(&obligation.id)
                        && self.decisions.values().any(|decision| {
                            decision.target_claim_id == claim.id
                                && decision.outcome == DecisionOutcome::Accept
                                && self
                                    .acceptance_chain_for_obligation(
                                        &claim.id, obligation, decision,
                                    )
                                    .is_ok()
                        })
                })
            })
            .map(|obligation| obligation.id.clone())
            .collect()
    }

    pub(crate) fn known_ids(&self) -> BTreeSet<StableId> {
        self.program
            .known_ids()
            .into_iter()
            .chain(self.universe.obligation_ids().iter().cloned())
            .chain(self.claims.keys().cloned())
            .chain(self.evidence.keys().cloned())
            .chain(self.bindings.keys().cloned())
            .chain(self.verifications.keys().cloned())
            .chain(self.decisions.keys().cloned())
            .chain(self.findings.keys().cloned())
            .chain(self.executions.keys().cloned())
            .chain(self.execution_claims.keys().cloned())
            .collect()
    }

    pub(crate) fn transition_obligation(
        &mut self,
        id: &StableId,
        next: ObligationLifecycle,
    ) -> Result<()> {
        let obligation =
            self.obligations
                .get_mut(id)
                .ok_or_else(|| DomainError::DanglingReference {
                    owner: "event",
                    owner_id: id.clone(),
                    reference: id.clone(),
                })?;
        obligation.transition(next)
    }

    pub(crate) fn has_structured_execution(&self, obligation_id: &StableId) -> bool {
        self.executions.values().any(|execution| {
            execution.outcome().is_structured()
                && execution.obligation_ids().contains(obligation_id)
        })
    }

    pub(crate) fn add_claim(&mut self, claim: ReviewClaim) -> Result<()> {
        claim.validate_initial()?;
        if self.claims.contains_key(&claim.id) {
            return Err(DomainError::IdCollision {
                id: claim.id.clone(),
            });
        }
        if claim.disposition != ClaimDisposition::Proposed
            || claim.review_status != ReviewStatus::Unreviewed
        {
            return Err(DomainError::Validation(
                "new claims must enter proposed and unreviewed".to_owned(),
            ));
        }
        self.claims.insert(claim.id.clone(), claim);
        self.validate()
    }

    pub(crate) fn add_evidence(&mut self, evidence: Evidence) -> Result<()> {
        evidence.validate_for_review_event()?;
        if self.evidence.contains_key(evidence.id()) {
            return Err(DomainError::IdCollision {
                id: evidence.id().clone(),
            });
        }
        if evidence.target_ids().is_empty()
            || evidence.provenance().review_status() != ReviewStatus::Accepted
            || evidence.snapshot_id().kind() != "snapshot"
        {
            return Err(DomainError::Validation(
                "canonical evidence requires accepted deterministic provenance".to_owned(),
            ));
        }
        self.evidence.insert(evidence.id().clone(), evidence);
        self.validate()
    }

    pub(crate) fn bind_evidence(&mut self, binding: EvidenceBinding) -> Result<()> {
        if self.bindings.contains_key(&binding.id) {
            return Err(DomainError::IdCollision {
                id: binding.id.clone(),
            });
        }
        let evidence_is_current = self
            .evidence
            .get(&binding.evidence_id)
            .ok_or_else(|| DomainError::DanglingReference {
                owner: "evidence binding",
                owner_id: binding.id.clone(),
                reference: binding.evidence_id.clone(),
            })?
            .snapshot_id()
            == self.program.snapshot_id();
        let claim = self.claims.get_mut(&binding.claim_id).ok_or_else(|| {
            DomainError::DanglingReference {
                owner: "evidence binding",
                owner_id: binding.id.clone(),
                reference: binding.claim_id.clone(),
            }
        })?;
        // Historical evidence is retained with its binding for audit, but it
        // cannot mutate the current-snapshot disposition of a claim. Its
        // verification may still be counted on the explicitly historical
        // `verified` axis; freshness is a separate coverage axis.
        if evidence_is_current {
            match binding.relation {
                EvidenceRelation::Supports | EvidenceRelation::Reproduces => claim.support()?,
                EvidenceRelation::Refutes | EvidenceRelation::Contradicts => claim.refute()?,
                EvidenceRelation::Qualifies | EvidenceRelation::Supersedes => {}
            }
        }
        self.bindings.insert(binding.id.clone(), binding);
        self.validate()
    }

    fn evidence_is_current(&self, evidence_id: &StableId) -> bool {
        self.evidence
            .get(evidence_id)
            .is_some_and(|evidence| evidence.snapshot_id() == self.program.snapshot_id())
    }

    pub(crate) fn add_verification(&mut self, verification: Verification) -> Result<()> {
        if self.verifications.contains_key(&verification.id) {
            return Err(DomainError::IdCollision {
                id: verification.id.clone(),
            });
        }
        let freshness = self.derived_freshness(&verification)?;
        if verification.freshness != freshness {
            return Err(DomainError::Validation(
                "persisted verification freshness does not match its snapshot-bound evidence"
                    .to_owned(),
            ));
        }
        let verification = verification.with_derived_freshness(freshness);
        self.verifications
            .insert(verification.id.clone(), verification);
        self.validate()
    }

    pub(crate) fn normalize_verification(
        &self,
        verification: Verification,
    ) -> Result<Verification> {
        let freshness = self.derived_freshness(&verification)?;
        if verification.freshness != Freshness::Unknown && verification.freshness != freshness {
            return Err(DomainError::Validation(
                "verification freshness is derived by the aggregate and cannot be caller supplied"
                    .to_owned(),
            ));
        }
        Ok(verification.with_derived_freshness(freshness))
    }

    pub(crate) fn record_decision(&mut self, decision: Decision, actor: &str) -> Result<()> {
        if self.decisions.contains_key(&decision.id) {
            return Err(DomainError::IdCollision {
                id: decision.id.clone(),
            });
        }
        if decision.authority_kind != DecisionAuthority::Human || decision.actor != actor {
            return Err(DomainError::Validation(
                "decision authority admission must match the event actor".to_owned(),
            ));
        }
        if decision.outcome == DecisionOutcome::Reject
            && !decision.source_ids.contains(&decision.target_claim_id)
        {
            return Err(DomainError::Validation(
                "reject decision must explicitly trace to its target claim".to_owned(),
            ));
        }
        let claim = self
            .claims
            .get_mut(&decision.target_claim_id)
            .ok_or_else(|| DomainError::DanglingReference {
                owner: "decision",
                owner_id: decision.id.clone(),
                reference: decision.target_claim_id.clone(),
            })?;
        if !decision.source_ids.contains(&decision.target_claim_id) {
            return Err(DomainError::Validation(
                "decision must cite its target claim".to_owned(),
            ));
        }
        match decision.outcome {
            DecisionOutcome::Accept => claim.accept(&decision)?,
            DecisionOutcome::Reject => claim.reject(&decision)?,
            DecisionOutcome::Exception | DecisionOutcome::Defer => {}
        }
        self.decisions.insert(decision.id.clone(), decision);
        self.validate()
    }

    pub(crate) fn add_finding(&mut self, finding: Finding) -> Result<()> {
        if self.findings.contains_key(&finding.id) {
            return Err(DomainError::IdCollision {
                id: finding.id.clone(),
            });
        }
        self.findings.insert(finding.id.clone(), finding);
        self.validate()
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod context_source_tests {
    use super::*;
    use crate::{MvpRulePack, ProgramSpace, SnapshotSourceRecordEntry};

    fn aggregate() -> ReviewAggregate {
        let program = ProgramSpace::from_json_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        ReviewAggregate::new(program, universe, obligations).unwrap()
    }

    fn install(aggregate: &mut ReviewAggregate, registration_hash: crate::ContentHash) {
        let run_id = StableId::parse("run:context-source-test").unwrap();
        let artifact_id = StableId::parse("file:checkout-controller").unwrap();
        let source = ArtifactSource::SnapshotIngest {
            run_id: run_id.clone(),
            snapshot_id: aggregate.program().snapshot_id().clone(),
            adapter_id: "test".to_owned(),
        };
        let registration_id = ArtifactRegistered::derived_id(
            &run_id,
            &registration_hash,
            "text/plain",
            ArtifactSensitivity::WorkspaceSource,
            &source,
        )
        .unwrap();
        let registration = ArtifactRegistered::new(
            run_id,
            registration_id.clone(),
            registration_hash.clone(),
            "text/plain",
            4,
            ArtifactSensitivity::WorkspaceSource,
            source,
        )
        .unwrap();
        let entries = vec![
            SnapshotSourceRecordEntry::new(
                artifact_id,
                "src/checkout_controller.rs",
                crate::ContentHash::sha256(b"abc\n"),
                registration_id,
                crate::ContentHash::sha256(b"abc\n"),
                2,
            )
            .unwrap(),
        ];
        aggregate.install_context_metadata_for_test(
            vec![registration],
            SnapshotSourcesRecorded::new(aggregate.program().snapshot_id().clone(), entries)
                .unwrap(),
        );
    }

    #[test]
    fn context_source_projection_is_read_only_and_rejects_missing_or_mismatched_links() {
        let mut valid = aggregate();
        install(&mut valid, crate::ContentHash::sha256(b"abc\n"));
        let id = StableId::parse("file:checkout-controller").unwrap();
        let resolved = valid.resolve_context_source(&id).unwrap();
        assert_eq!(resolved.size, 4);
        assert_eq!(resolved.line_count, 2);
        assert!(matches!(
            valid.resolve_context_source(&StableId::parse("file:missing").unwrap()),
            Err(DomainError::DanglingReference { .. })
        ));

        let mut mismatched = aggregate();
        install(&mut mismatched, crate::ContentHash::sha256(b"wrong\n"));
        assert!(matches!(
            mismatched.resolve_context_source(&id),
            Err(DomainError::DanglingReference { .. })
        ));
    }
}

fn require_known(
    owner: &'static str,
    owner_id: &StableId,
    known: &BTreeSet<StableId>,
    reference: &StableId,
) -> Result<()> {
    if known.contains(reference) {
        Ok(())
    } else {
        Err(DomainError::DanglingReference {
            owner,
            owner_id: owner_id.clone(),
            reference: reference.clone(),
        })
    }
}

fn insert_unique(ids: &mut BTreeSet<StableId>, id: &StableId) -> Result<()> {
    if ids.insert(id.clone()) {
        Ok(())
    } else {
        Err(DomainError::IdCollision { id: id.clone() })
    }
}
