use crate::{
    ArtifactRegistered, ArtifactRegisteredV3, ArtifactSensitivity, ArtifactSource,
    ArtifactSourceV3, ClaimAuthorKind, ClaimDisposition, ClaimPolarity, ContentHash, DomainError,
    Result, ReviewStatus, StableId,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeMap};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

pub const MAX_D2_CLAIMS: usize = 16;
pub const MAX_D2_TARGET_REFS: usize = 64;
pub const MAX_D2_SOURCE_IDS: usize = 128;
pub const MAX_D2_ASSUMPTIONS: usize = 32;
pub const MAX_D2_REQUESTED_EVIDENCE: usize = 32;
pub const MAX_D2_INFERENCE_ENTRIES: usize = 32;
pub const MAX_D2_TRACE_BYTES: usize = 256;
pub const MAX_D2_INFERENCE_VALUE_BYTES: usize = 1_024;
pub const MAX_D2_SUMMARY_BYTES: usize = 8_192;
pub const MAX_D2_LIST_ITEM_BYTES: usize = 2_048;
pub const MAX_D2_OUTCOME_TEXT_BYTES: usize = 4_096;
pub const MAX_D2_CLAIM_BYTES: usize = 32_768;
pub const MAX_D2_EXECUTION_BYTES: usize = 131_072;
pub const MAX_D2_RAW_REVIEWER_BYTES: usize = 1_048_576;
pub const MAX_D2_RESOLVED_SOURCE_BYTES: usize = 8_388_608;
pub const MAX_D2_WORKING_BYTES: usize = 16_777_216;

pub const FAKE_REVIEWER_KIND: &str = "fake";
pub const FAKE_REVIEWER_ID: &str = "reviewgraphen.fake_reviewer@1";
pub const NO_TOOLS_SYSTEM_PROMPT_VERSION: &str = "reviewgraphen.system.no_tools@1";
pub const FIXTURE_PROMPT_TEMPLATE_VERSION: &str = "fixture@1";
pub const NO_TOOLS_POLICY_VERSION: &str = "reviewgraphen.tool_policy.none@1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AbstentionReason {
    InsufficientContext,
    UnresolvedSymbol,
    RequiredEvidenceUnavailable,
    PropertyNotUnderstood,
    ConflictingSources,
    ToolCapabilityMissing,
    BudgetExhausted,
    PromptInjectionSuspected,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MalformedOutputReason {
    SchemaViolation,
    UnresolvedSourceId,
    UnknownObligationId,
    InvalidPolarity,
    ConfidenceOutOfRange,
    UnknownField,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionOutcome {
    Structured,
    Abstained {
        reason: AbstentionReason,
        detail: String,
    },
    Malformed {
        reason: MalformedOutputReason,
        diagnostic: String,
    },
    ProviderFailure {
        retryable: bool,
        diagnostic: String,
    },
}

impl ExecutionOutcome {
    #[must_use]
    pub const fn is_structured(&self) -> bool {
        matches!(self, Self::Structured)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let text = match self {
            Self::Structured => return Ok(()),
            Self::Abstained { detail, .. } => detail,
            Self::Malformed { diagnostic, .. } | Self::ProviderFailure { diagnostic, .. } => {
                diagnostic
            }
        };
        require_nonempty_bounded(text, MAX_D2_OUTCOME_TEXT_BYTES, "D2 outcome text")
    }
}

impl Serialize for ExecutionOutcome {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Structured => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("kind", "structured")?;
                map.end()
            }
            Self::Abstained { reason, detail } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("detail", detail)?;
                map.serialize_entry("kind", "abstained")?;
                map.serialize_entry("reason", reason)?;
                map.end()
            }
            Self::Malformed { reason, diagnostic } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("diagnostic", diagnostic)?;
                map.serialize_entry("kind", "malformed")?;
                map.serialize_entry("reason", reason)?;
                map.end()
            }
            Self::ProviderFailure {
                retryable,
                diagnostic,
            } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("diagnostic", diagnostic)?;
                map.serialize_entry("kind", "provider_failure")?;
                map.serialize_entry("retryable", retryable)?;
                map.end()
            }
        }
    }
}

#[derive(Deserialize)]
enum StructuredKind {
    #[serde(rename = "structured")]
    Structured,
}
#[derive(Deserialize)]
enum AbstainedKind {
    #[serde(rename = "abstained")]
    Abstained,
}
#[derive(Deserialize)]
enum MalformedKind {
    #[serde(rename = "malformed")]
    Malformed,
}
#[derive(Deserialize)]
enum ProviderFailureKind {
    #[serde(rename = "provider_failure")]
    ProviderFailure,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StructuredOutcomeWire {
    kind: StructuredKind,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AbstainedOutcomeWire {
    detail: String,
    kind: AbstainedKind,
    reason: AbstentionReason,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MalformedOutcomeWire {
    diagnostic: String,
    kind: MalformedKind,
    reason: MalformedOutputReason,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderFailureOutcomeWire {
    diagnostic: String,
    kind: ProviderFailureKind,
    retryable: bool,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum ExecutionOutcomeWire {
    Structured(StructuredOutcomeWire),
    Abstained(AbstainedOutcomeWire),
    Malformed(MalformedOutcomeWire),
    ProviderFailure(ProviderFailureOutcomeWire),
}

impl<'de> Deserialize<'de> for ExecutionOutcome {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let outcome = match ExecutionOutcomeWire::deserialize(deserializer)? {
            ExecutionOutcomeWire::Structured(wire) => {
                let StructuredKind::Structured = wire.kind;
                Self::Structured
            }
            ExecutionOutcomeWire::Abstained(wire) => {
                let AbstainedKind::Abstained = wire.kind;
                Self::Abstained {
                    reason: wire.reason,
                    detail: wire.detail,
                }
            }
            ExecutionOutcomeWire::Malformed(wire) => {
                let MalformedKind::Malformed = wire.kind;
                Self::Malformed {
                    reason: wire.reason,
                    diagnostic: wire.diagnostic,
                }
            }
            ExecutionOutcomeWire::ProviderFailure(wire) => {
                let ProviderFailureKind::ProviderFailure = wire.kind;
                Self::ProviderFailure {
                    retryable: wire.retryable,
                    diagnostic: wire.diagnostic,
                }
            }
        };
        outcome.validate().map_err(serde::de::Error::custom)?;
        Ok(outcome)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionClaimV2 {
    assumptions: BTreeSet<String>,
    author_kind: ClaimAuthorKind,
    candidate_confidence: Option<f64>,
    disposition: ClaimDisposition,
    execution_id: StableId,
    id: StableId,
    obligation_ids: BTreeSet<StableId>,
    polarity: ClaimPolarity,
    property_id: String,
    requested_evidence: BTreeSet<String>,
    review_status: ReviewStatus,
    source_ids: BTreeSet<StableId>,
    summary: String,
    target_refs: BTreeSet<StableId>,
}

fn present_nullable<'de, D, T>(deserializer: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug)]
pub struct ExecutionClaimInputV2 {
    property_id: String,
    target_refs: BTreeSet<StableId>,
    polarity: ClaimPolarity,
    summary: String,
    source_ids: BTreeSet<StableId>,
    assumptions: BTreeSet<String>,
    requested_evidence: BTreeSet<String>,
    candidate_confidence: Option<f64>,
}

impl ExecutionClaimInputV2 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        property_id: impl Into<String>,
        target_refs: BTreeSet<StableId>,
        polarity: ClaimPolarity,
        summary: impl Into<String>,
        source_ids: BTreeSet<StableId>,
        assumptions: BTreeSet<String>,
        requested_evidence: BTreeSet<String>,
        candidate_confidence: Option<f64>,
    ) -> Result<Self> {
        let input = Self {
            property_id: property_id.into(),
            target_refs,
            polarity,
            summary: summary.into(),
            source_ids,
            assumptions,
            requested_evidence,
            candidate_confidence,
        };
        input.validate()?;
        Ok(input)
    }

    fn validate(&self) -> Result<()> {
        require_nonempty_bounded(&self.property_id, MAX_D2_TRACE_BYTES, "D2 property_id")?;
        require_nonempty_bounded(&self.summary, MAX_D2_SUMMARY_BYTES, "D2 claim summary")?;
        require_count(
            self.target_refs.len(),
            1,
            MAX_D2_TARGET_REFS,
            "D2 claim target refs",
        )?;
        require_count(
            self.source_ids.len(),
            1,
            MAX_D2_SOURCE_IDS,
            "D2 claim source IDs",
        )?;
        require_count(
            self.assumptions.len(),
            0,
            MAX_D2_ASSUMPTIONS,
            "D2 claim assumptions",
        )?;
        require_count(
            self.requested_evidence.len(),
            0,
            MAX_D2_REQUESTED_EVIDENCE,
            "D2 requested evidence",
        )?;
        for item in self
            .assumptions
            .iter()
            .chain(self.requested_evidence.iter())
        {
            require_nonempty_bounded(item, MAX_D2_LIST_ITEM_BYTES, "D2 claim list item")?;
        }
        validate_confidence(self.candidate_confidence)
    }
}

#[derive(Serialize)]
struct ClaimIdentity<'a> {
    assumptions: &'a BTreeSet<String>,
    execution_id: &'a StableId,
    obligation_ids: &'a BTreeSet<StableId>,
    polarity: ClaimPolarity,
    property_id: &'a str,
    requested_evidence: &'a BTreeSet<String>,
    source_ids: &'a BTreeSet<StableId>,
    summary: &'a str,
    target_refs: &'a BTreeSet<StableId>,
}

impl ExecutionClaimV2 {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.id
            .allocated_bytes()
            .saturating_add(self.execution_id.allocated_bytes())
            .saturating_add(
                self.obligation_ids
                    .iter()
                    .map(StableId::allocated_bytes)
                    .sum(),
            )
            .saturating_add(self.property_id.capacity())
            .saturating_add(self.target_refs.iter().map(StableId::allocated_bytes).sum())
            .saturating_add(self.summary.capacity())
            .saturating_add(self.source_ids.iter().map(StableId::allocated_bytes).sum())
            .saturating_add(self.assumptions.iter().map(String::capacity).sum())
            .saturating_add(self.requested_evidence.iter().map(String::capacity).sum())
    }
    fn from_input(
        execution_id: StableId,
        obligation_ids: BTreeSet<StableId>,
        input: ExecutionClaimInputV2,
    ) -> Result<Self> {
        input.validate()?;
        let identity = ClaimIdentity {
            assumptions: &input.assumptions,
            execution_id: &execution_id,
            obligation_ids: &obligation_ids,
            polarity: input.polarity,
            property_id: &input.property_id,
            requested_evidence: &input.requested_evidence,
            source_ids: &input.source_ids,
            summary: &input.summary,
            target_refs: &input.target_refs,
        };
        let id = derived_id(
            "claim",
            &bounded_json(&identity, MAX_D2_CLAIM_BYTES, "D2 claim identity")?,
        )?;
        let claim = Self {
            assumptions: input.assumptions,
            author_kind: ClaimAuthorKind::Ai,
            candidate_confidence: input.candidate_confidence,
            disposition: ClaimDisposition::Proposed,
            execution_id,
            id,
            obligation_ids,
            polarity: input.polarity,
            property_id: input.property_id,
            requested_evidence: input.requested_evidence,
            review_status: ReviewStatus::Unreviewed,
            source_ids: input.source_ids,
            summary: input.summary,
            target_refs: input.target_refs,
        };
        claim.validate_shape()?;
        Ok(claim)
    }

    pub(crate) fn validate_shape(&self) -> Result<()> {
        if self.id.kind() != "claim"
            || self.execution_id.kind() != "execution"
            || self.obligation_ids.len() != 1
            || self.disposition != ClaimDisposition::Proposed
            || self.author_kind != ClaimAuthorKind::Ai
            || self.review_status != ReviewStatus::Unreviewed
        {
            return Err(DomainError::Validation(
                "D2 claim requires derived identity, one obligation, and proposed AI/unreviewed state"
                    .to_owned(),
            ));
        }
        let input = ExecutionClaimInputV2 {
            property_id: self.property_id.clone(),
            target_refs: self.target_refs.clone(),
            polarity: self.polarity,
            summary: self.summary.clone(),
            source_ids: self.source_ids.clone(),
            assumptions: self.assumptions.clone(),
            requested_evidence: self.requested_evidence.clone(),
            candidate_confidence: self.candidate_confidence,
        };
        input.validate()?;
        let identity = ClaimIdentity {
            assumptions: &self.assumptions,
            execution_id: &self.execution_id,
            obligation_ids: &self.obligation_ids,
            polarity: self.polarity,
            property_id: &self.property_id,
            requested_evidence: &self.requested_evidence,
            source_ids: &self.source_ids,
            summary: &self.summary,
            target_refs: &self.target_refs,
        };
        if self.id
            != derived_id(
                "claim",
                &bounded_json(&identity, MAX_D2_CLAIM_BYTES, "D2 claim identity")?,
            )?
        {
            return Err(DomainError::Validation(
                "D2 claim ID does not match its exact identity preimage".to_owned(),
            ));
        }
        let _ = self.canonical_bytes()?;
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        bounded_json(self, MAX_D2_CLAIM_BYTES, "D2 canonical claim")
    }

    pub fn identity_body_hash(&self) -> Result<ContentHash> {
        let identity = ClaimIdentity {
            assumptions: &self.assumptions,
            execution_id: &self.execution_id,
            obligation_ids: &self.obligation_ids,
            polarity: self.polarity,
            property_id: &self.property_id,
            requested_evidence: &self.requested_evidence,
            source_ids: &self.source_ids,
            summary: &self.summary,
            target_refs: &self.target_refs,
        };
        Ok(ContentHash::sha256(&bounded_json(
            &identity,
            MAX_D2_CLAIM_BYTES,
            "D2 claim identity",
        )?))
    }

    pub fn body_hash(&self) -> Result<ContentHash> {
        Ok(ContentHash::sha256(&self.canonical_bytes()?))
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn execution_id(&self) -> &StableId {
        &self.execution_id
    }
    #[must_use]
    pub fn obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.obligation_ids
    }
    #[must_use]
    pub fn property_id(&self) -> &str {
        &self.property_id
    }
    #[must_use]
    pub fn target_refs(&self) -> &BTreeSet<StableId> {
        &self.target_refs
    }
    #[must_use]
    pub const fn polarity(&self) -> ClaimPolarity {
        self.polarity
    }
    #[must_use]
    pub const fn disposition(&self) -> ClaimDisposition {
        self.disposition
    }
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    #[must_use]
    pub fn assumptions(&self) -> &BTreeSet<String> {
        &self.assumptions
    }
    #[must_use]
    pub fn requested_evidence(&self) -> &BTreeSet<String> {
        &self.requested_evidence
    }
    #[must_use]
    pub const fn candidate_confidence(&self) -> Option<f64> {
        self.candidate_confidence
    }
    #[must_use]
    pub const fn author_kind(&self) -> ClaimAuthorKind {
        self.author_kind
    }
    #[must_use]
    pub const fn review_status(&self) -> ReviewStatus {
        self.review_status
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExecutionClaimV2 {
    assumptions: Vec<String>,
    author_kind: ClaimAuthorKind,
    #[serde(default, deserialize_with = "present_nullable")]
    candidate_confidence: Option<Option<f64>>,
    disposition: ClaimDisposition,
    execution_id: StableId,
    id: StableId,
    obligation_ids: Vec<StableId>,
    polarity: ClaimPolarity,
    property_id: String,
    requested_evidence: Vec<String>,
    review_status: ReviewStatus,
    source_ids: Vec<StableId>,
    summary: String,
    target_refs: Vec<StableId>,
}

impl<'de> Deserialize<'de> for ExecutionClaimV2 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawExecutionClaimV2::deserialize(deserializer)?;
        let claim = Self {
            assumptions: strict_sorted_strings(raw.assumptions, "D2 claim assumptions")
                .map_err(serde::de::Error::custom)?,
            author_kind: raw.author_kind,
            candidate_confidence: raw
                .candidate_confidence
                .ok_or_else(|| serde::de::Error::missing_field("candidate_confidence"))?,
            disposition: raw.disposition,
            execution_id: raw.execution_id,
            id: raw.id,
            obligation_ids: strict_sorted_ids(raw.obligation_ids, "D2 claim obligation_ids")
                .map_err(serde::de::Error::custom)?,
            polarity: raw.polarity,
            property_id: raw.property_id,
            requested_evidence: strict_sorted_strings(
                raw.requested_evidence,
                "D2 claim requested_evidence",
            )
            .map_err(serde::de::Error::custom)?,
            review_status: raw.review_status,
            source_ids: strict_sorted_ids(raw.source_ids, "D2 claim source_ids")
                .map_err(serde::de::Error::custom)?,
            summary: raw.summary,
            target_refs: strict_sorted_ids(raw.target_refs, "D2 claim target_refs")
                .map_err(serde::de::Error::custom)?,
        };
        claim.validate_shape().map_err(serde::de::Error::custom)?;
        Ok(claim)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionRecord {
    attempt: u32,
    envelope_id: StableId,
    id: StableId,
    inference_settings: BTreeMap<String, String>,
    model: Option<String>,
    model_revision: Option<String>,
    obligation_ids: BTreeSet<StableId>,
    outcome: ExecutionOutcome,
    parsed_claim_ids: BTreeSet<StableId>,
    plan_id: StableId,
    prompt_template_version: String,
    provider: Option<String>,
    raw_artifact_hash: ContentHash,
    raw_artifact_registration_id: StableId,
    reviewer_id: String,
    reviewer_kind: String,
    snapshot_id: StableId,
    system_prompt_version: String,
    tool_calls: Vec<NoToolCall>,
    tool_policy_version: String,
    wave_id: StableId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum NoToolCall {}

#[derive(Clone, Debug)]
pub struct ExecutionRecordInput {
    plan_id: StableId,
    wave_id: StableId,
    obligation_ids: BTreeSet<StableId>,
    envelope_id: StableId,
    snapshot_id: StableId,
    attempt: u32,
}

impl ExecutionRecordInput {
    pub fn fake(
        plan_id: StableId,
        wave_id: StableId,
        obligation_id: StableId,
        envelope_id: StableId,
        snapshot_id: StableId,
        attempt: u32,
    ) -> Result<Self> {
        let input = Self {
            plan_id,
            wave_id,
            obligation_ids: BTreeSet::from([obligation_id]),
            envelope_id,
            snapshot_id,
            attempt,
        };
        input.validate()?;
        Ok(input)
    }

    fn validate(&self) -> Result<()> {
        if self.plan_id.kind() != "plan"
            || self.wave_id.kind() != "schedule-wave"
            || self.envelope_id.kind() != "context-envelope"
            || self.snapshot_id.kind() != "snapshot"
            || self.obligation_ids.len() != 1
            || self.attempt == 0
        {
            return Err(DomainError::Validation(
                "D2 execution input requires plan/wave/envelope/snapshot IDs, one obligation, and a positive attempt"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    pub fn execution_id(&self) -> Result<StableId> {
        self.validate()?;
        let identity = ExecutionIdentity::from_input(self);
        derived_id(
            "execution",
            &bounded_json(&identity, MAX_D2_EXECUTION_BYTES, "D2 execution identity")?,
        )
    }
}

#[derive(Serialize)]
struct ExecutionIdentity<'a> {
    attempt: u32,
    envelope_id: &'a StableId,
    inference_settings: BTreeMap<String, String>,
    model: Option<&'a str>,
    model_revision: Option<&'a str>,
    obligation_ids: &'a BTreeSet<StableId>,
    plan_id: &'a StableId,
    prompt_template_version: &'static str,
    provider: Option<&'a str>,
    reviewer_id: &'static str,
    reviewer_kind: &'static str,
    snapshot_id: &'a StableId,
    system_prompt_version: &'static str,
    tool_policy_version: &'static str,
    wave_id: &'a StableId,
}

impl<'a> ExecutionIdentity<'a> {
    fn from_input(input: &'a ExecutionRecordInput) -> Self {
        Self {
            attempt: input.attempt,
            envelope_id: &input.envelope_id,
            inference_settings: BTreeMap::new(),
            model: None,
            model_revision: None,
            obligation_ids: &input.obligation_ids,
            plan_id: &input.plan_id,
            prompt_template_version: FIXTURE_PROMPT_TEMPLATE_VERSION,
            provider: None,
            reviewer_id: FAKE_REVIEWER_ID,
            reviewer_kind: FAKE_REVIEWER_KIND,
            snapshot_id: &input.snapshot_id,
            system_prompt_version: NO_TOOLS_SYSTEM_PROMPT_VERSION,
            tool_policy_version: NO_TOOLS_POLICY_VERSION,
            wave_id: &input.wave_id,
        }
    }
}

impl ExecutionRecord {
    pub(crate) fn allocated_bytes(&self) -> usize {
        let outcome = match &self.outcome {
            ExecutionOutcome::Structured => 0,
            ExecutionOutcome::Abstained { detail, .. } => detail.capacity(),
            ExecutionOutcome::Malformed { diagnostic, .. }
            | ExecutionOutcome::ProviderFailure { diagnostic, .. } => diagnostic.capacity(),
        };
        self.id
            .allocated_bytes()
            .saturating_add(self.plan_id.allocated_bytes())
            .saturating_add(self.wave_id.allocated_bytes())
            .saturating_add(
                self.obligation_ids
                    .iter()
                    .map(StableId::allocated_bytes)
                    .sum(),
            )
            .saturating_add(self.envelope_id.allocated_bytes())
            .saturating_add(self.snapshot_id.allocated_bytes())
            .saturating_add(self.reviewer_kind.capacity())
            .saturating_add(self.reviewer_id.capacity())
            .saturating_add(self.system_prompt_version.capacity())
            .saturating_add(self.prompt_template_version.capacity())
            .saturating_add(self.tool_policy_version.capacity())
            .saturating_add(self.raw_artifact_registration_id.allocated_bytes())
            .saturating_add(self.raw_artifact_hash.allocated_bytes())
            .saturating_add(
                self.parsed_claim_ids
                    .iter()
                    .map(StableId::allocated_bytes)
                    .sum(),
            )
            .saturating_add(outcome)
    }
    fn from_input(
        input: ExecutionRecordInput,
        registration_id: &StableId,
        raw_hash: ContentHash,
        parsed_claim_ids: BTreeSet<StableId>,
        outcome: ExecutionOutcome,
    ) -> Result<Self> {
        let id = input.execution_id()?;
        let record = Self {
            attempt: input.attempt,
            envelope_id: input.envelope_id,
            id,
            inference_settings: BTreeMap::new(),
            model: None,
            model_revision: None,
            obligation_ids: input.obligation_ids,
            outcome,
            parsed_claim_ids,
            plan_id: input.plan_id,
            prompt_template_version: FIXTURE_PROMPT_TEMPLATE_VERSION.to_owned(),
            provider: None,
            raw_artifact_hash: raw_hash,
            raw_artifact_registration_id: registration_id.clone(),
            reviewer_id: FAKE_REVIEWER_ID.to_owned(),
            reviewer_kind: FAKE_REVIEWER_KIND.to_owned(),
            snapshot_id: input.snapshot_id,
            system_prompt_version: NO_TOOLS_SYSTEM_PROMPT_VERSION.to_owned(),
            tool_calls: Vec::new(),
            tool_policy_version: NO_TOOLS_POLICY_VERSION.to_owned(),
            wave_id: input.wave_id,
        };
        record.validate_shape()?;
        Ok(record)
    }

    pub(crate) fn validate_shape(&self) -> Result<()> {
        if self.id.kind() != "execution"
            || self.plan_id.kind() != "plan"
            || self.wave_id.kind() != "schedule-wave"
            || self.envelope_id.kind() != "context-envelope"
            || self.snapshot_id.kind() != "snapshot"
            || self.obligation_ids.len() != 1
            || self.attempt == 0
            || self.reviewer_kind != FAKE_REVIEWER_KIND
            || self.reviewer_id != FAKE_REVIEWER_ID
            || self.provider.is_some()
            || self.model.is_some()
            || self.model_revision.is_some()
            || self.system_prompt_version != NO_TOOLS_SYSTEM_PROMPT_VERSION
            || self.prompt_template_version != FIXTURE_PROMPT_TEMPLATE_VERSION
            || !self.inference_settings.is_empty()
            || self.tool_policy_version != NO_TOOLS_POLICY_VERSION
            || !self.tool_calls.is_empty()
        {
            return Err(DomainError::Validation(
                "D2 execution must use the exact fake/no-tools descriptor and one-obligation scope"
                    .to_owned(),
            ));
        }
        validate_trace_map(&self.inference_settings)?;
        self.outcome.validate()?;
        if self.outcome.is_structured() == self.parsed_claim_ids.is_empty() {
            return Err(DomainError::Validation(
                "structured D2 execution requires claims and every failure outcome forbids them"
                    .to_owned(),
            ));
        }
        let input = ExecutionRecordInput {
            plan_id: self.plan_id.clone(),
            wave_id: self.wave_id.clone(),
            obligation_ids: self.obligation_ids.clone(),
            envelope_id: self.envelope_id.clone(),
            snapshot_id: self.snapshot_id.clone(),
            attempt: self.attempt,
        };
        if self.id != input.execution_id()? {
            return Err(DomainError::Validation(
                "D2 execution ID does not match its exact identity preimage".to_owned(),
            ));
        }
        let _ = self.canonical_bytes()?;
        Ok(())
    }

    pub(crate) fn same_retry_series(&self, other: &Self) -> bool {
        self.plan_id == other.plan_id
            && self.wave_id == other.wave_id
            && self.obligation_ids == other.obligation_ids
            && self.envelope_id == other.envelope_id
            && self.snapshot_id == other.snapshot_id
            && self.reviewer_kind == other.reviewer_kind
            && self.reviewer_id == other.reviewer_id
            && self.provider == other.provider
            && self.model == other.model
            && self.model_revision == other.model_revision
            && self.system_prompt_version == other.system_prompt_version
            && self.prompt_template_version == other.prompt_template_version
            && self.inference_settings == other.inference_settings
            && self.tool_policy_version == other.tool_policy_version
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        bounded_json(self, MAX_D2_EXECUTION_BYTES, "D2 canonical execution")
    }

    pub fn identity_body_hash(&self) -> Result<ContentHash> {
        let input = ExecutionRecordInput {
            plan_id: self.plan_id.clone(),
            wave_id: self.wave_id.clone(),
            obligation_ids: self.obligation_ids.clone(),
            envelope_id: self.envelope_id.clone(),
            snapshot_id: self.snapshot_id.clone(),
            attempt: self.attempt,
        };
        Ok(ContentHash::sha256(&bounded_json(
            &ExecutionIdentity::from_input(&input),
            MAX_D2_EXECUTION_BYTES,
            "D2 execution identity",
        )?))
    }

    pub fn body_hash(&self) -> Result<ContentHash> {
        Ok(ContentHash::sha256(&self.canonical_bytes()?))
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn plan_id(&self) -> &StableId {
        &self.plan_id
    }
    #[must_use]
    pub fn wave_id(&self) -> &StableId {
        &self.wave_id
    }
    #[must_use]
    pub fn obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.obligation_ids
    }
    #[must_use]
    pub fn envelope_id(&self) -> &StableId {
        &self.envelope_id
    }
    #[must_use]
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    #[must_use]
    pub fn reviewer_kind(&self) -> &str {
        &self.reviewer_kind
    }
    #[must_use]
    pub fn reviewer_id(&self) -> &str {
        &self.reviewer_id
    }
    #[must_use]
    pub fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }
    #[must_use]
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }
    #[must_use]
    pub fn model_revision(&self) -> Option<&str> {
        self.model_revision.as_deref()
    }
    #[must_use]
    pub fn system_prompt_version(&self) -> &str {
        &self.system_prompt_version
    }
    #[must_use]
    pub fn prompt_template_version(&self) -> &str {
        &self.prompt_template_version
    }
    #[must_use]
    pub fn inference_settings(&self) -> &BTreeMap<String, String> {
        &self.inference_settings
    }
    #[must_use]
    pub fn tool_policy_version(&self) -> &str {
        &self.tool_policy_version
    }
    #[must_use]
    pub fn tool_call_count(&self) -> usize {
        self.tool_calls.len()
    }
    #[must_use]
    pub const fn attempt(&self) -> u32 {
        self.attempt
    }
    #[must_use]
    pub fn raw_artifact_registration_id(&self) -> &StableId {
        &self.raw_artifact_registration_id
    }
    #[must_use]
    pub fn raw_artifact_hash(&self) -> &ContentHash {
        &self.raw_artifact_hash
    }
    #[must_use]
    pub fn parsed_claim_ids(&self) -> &BTreeSet<StableId> {
        &self.parsed_claim_ids
    }
    #[must_use]
    pub fn outcome(&self) -> &ExecutionOutcome {
        &self.outcome
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExecutionRecord {
    attempt: u32,
    envelope_id: StableId,
    id: StableId,
    inference_settings: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "present_nullable")]
    model: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_nullable")]
    model_revision: Option<Option<String>>,
    obligation_ids: Vec<StableId>,
    outcome: ExecutionOutcome,
    parsed_claim_ids: Vec<StableId>,
    plan_id: StableId,
    prompt_template_version: String,
    #[serde(default, deserialize_with = "present_nullable")]
    provider: Option<Option<String>>,
    raw_artifact_hash: ContentHash,
    raw_artifact_registration_id: StableId,
    reviewer_id: String,
    reviewer_kind: String,
    snapshot_id: StableId,
    system_prompt_version: String,
    tool_calls: Vec<NoToolCall>,
    tool_policy_version: String,
    wave_id: StableId,
}

impl<'de> Deserialize<'de> for ExecutionRecord {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawExecutionRecord::deserialize(deserializer)?;
        let record = Self {
            attempt: raw.attempt,
            envelope_id: raw.envelope_id,
            id: raw.id,
            inference_settings: raw.inference_settings,
            model: raw
                .model
                .ok_or_else(|| serde::de::Error::missing_field("model"))?,
            model_revision: raw
                .model_revision
                .ok_or_else(|| serde::de::Error::missing_field("model_revision"))?,
            obligation_ids: strict_sorted_ids(raw.obligation_ids, "D2 execution obligation_ids")
                .map_err(serde::de::Error::custom)?,
            outcome: raw.outcome,
            parsed_claim_ids: strict_sorted_ids(
                raw.parsed_claim_ids,
                "D2 execution parsed_claim_ids",
            )
            .map_err(serde::de::Error::custom)?,
            plan_id: raw.plan_id,
            prompt_template_version: raw.prompt_template_version,
            provider: raw
                .provider
                .ok_or_else(|| serde::de::Error::missing_field("provider"))?,
            raw_artifact_hash: raw.raw_artifact_hash,
            raw_artifact_registration_id: raw.raw_artifact_registration_id,
            reviewer_id: raw.reviewer_id,
            reviewer_kind: raw.reviewer_kind,
            snapshot_id: raw.snapshot_id,
            system_prompt_version: raw.system_prompt_version,
            tool_calls: raw.tool_calls,
            tool_policy_version: raw.tool_policy_version,
            wave_id: raw.wave_id,
        };
        record.validate_shape().map_err(serde::de::Error::custom)?;
        Ok(record)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewExecutionRecorded {
    pub(crate) claims: Vec<ExecutionClaimV2>,
    pub(crate) execution: ExecutionRecord,
}

impl ReviewExecutionRecorded {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.execution
            .allocated_bytes()
            .saturating_add(
                self.claims
                    .capacity()
                    .saturating_mul(std::mem::size_of::<ExecutionClaimV2>()),
            )
            .saturating_add(
                self.claims
                    .iter()
                    .map(ExecutionClaimV2::allocated_bytes)
                    .sum::<usize>(),
            )
    }

    pub(crate) fn validate_shape(&self) -> Result<()> {
        self.execution.validate_shape()?;
        let expected_count = if self.execution.outcome.is_structured() {
            require_count(self.claims.len(), 1, MAX_D2_CLAIMS, "D2 execution claims")?;
            self.claims.len()
        } else {
            require_count(self.claims.len(), 0, 0, "D2 failure execution claims")?;
            0
        };
        let mut previous = None;
        let mut ids = BTreeSet::new();
        for claim in &self.claims {
            claim.validate_shape()?;
            if !ids.insert(claim.id().clone()) {
                return Err(DomainError::IdCollision {
                    id: claim.id().clone(),
                });
            }
            if previous
                .as_ref()
                .is_some_and(|id: &StableId| id > claim.id())
            {
                return Err(DomainError::Validation(
                    "D2 claims must be ordered by claim ID".to_owned(),
                ));
            }
            previous = Some(claim.id().clone());
            if claim.execution_id() != self.execution.id()
                || claim.obligation_ids() != self.execution.obligation_ids()
            {
                return Err(DomainError::Validation(
                    "D2 claim execution and obligation scope must equal its atomic execution"
                        .to_owned(),
                ));
            }
        }
        if ids.len() != expected_count || ids != *self.execution.parsed_claim_ids() {
            return Err(DomainError::Validation(
                "D2 parsed claim IDs must exactly equal the atomic ordered claim set".to_owned(),
            ));
        }
        Ok(())
    }

    pub(crate) fn canonical_bytes(&self) -> Result<Vec<u8>> {
        let limit = 1_048_576_usize;
        bounded_json(self, limit, "D2 atomic execution payload")
    }

    pub(crate) fn validate_decode_working(&self, retained_raw_bytes: usize) -> Result<()> {
        let canonical = self.canonical_bytes()?;
        let mut total = 0_u64;
        for value in [
            retained_raw_bytes,
            retained_raw_bytes,
            self.execution.allocated_bytes(),
            checked_working_mul(
                self.claims.capacity(),
                std::mem::size_of::<ExecutionClaimV2>(),
            )?,
            canonical.capacity(),
        ] {
            total = checked_working_add(total, value)?;
        }
        for claim in &self.claims {
            total = checked_working_add(total, claim.allocated_bytes())?;
        }
        require_len(
            finish_working(total)?,
            MAX_D2_WORKING_BYTES,
            "D2 decode working bytes",
        )
    }
}

pub(crate) fn preflight_d2_decode_working(input: &[u8], limit: usize) -> Result<()> {
    preflight_d2_nested_counts(input)?;
    let observed = d2_decode_working_observed(input, limit)?;
    require_len(observed, limit, "D2 decode working bytes")
}

const MAX_D2_JSON_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum D2ObjectContext {
    Root,
    Claim,
    Execution,
    Inference,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum D2ArrayContext {
    Claims,
    TargetRefs,
    SourceIds,
    Assumptions,
    RequestedEvidence,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum D2Field {
    Claims,
    Execution,
    TargetRefs,
    SourceIds,
    Assumptions,
    RequestedEvidence,
    InferenceSettings,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum D2ContainerState {
    ValueOrKeyOrEnd,
    Colon,
    Value,
    CommaOrEnd,
}

#[derive(Clone, Copy, Debug)]
enum D2ContainerKind {
    Object(D2ObjectContext),
    Array(D2ArrayContext),
}

#[derive(Clone, Copy, Debug)]
struct D2ScanFrame {
    kind: D2ContainerKind,
    state: D2ContainerState,
    field: D2Field,
    count: usize,
}

const EMPTY_D2_FRAME: D2ScanFrame = D2ScanFrame {
    kind: D2ContainerKind::Array(D2ArrayContext::Other),
    state: D2ContainerState::ValueOrKeyOrEnd,
    field: D2Field::Other,
    count: 0,
};

fn preflight_d2_nested_counts(input: &[u8]) -> Result<()> {
    let mut stack = [EMPTY_D2_FRAME; MAX_D2_JSON_DEPTH];
    let mut depth = 0_usize;
    let mut index = 0_usize;
    let mut root_complete = false;
    while index < input.len() {
        if input[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if root_complete {
            return Err(DomainError::Json("invalid D2 JSON structure".to_owned()));
        }
        match input[index] {
            b'{' | b'[' => {
                let (object_context, array_context) = d2_child_context(&stack, depth);
                d2_accept_value(&mut stack, depth)?;
                if depth == MAX_D2_JSON_DEPTH {
                    return Err(DomainError::Incomplete {
                        operation: "D2 JSON depth",
                        limit: MAX_D2_JSON_DEPTH,
                        observed: depth + 1,
                    });
                }
                stack[depth] = if input[index] == b'{' {
                    D2ScanFrame {
                        kind: D2ContainerKind::Object(object_context),
                        ..EMPTY_D2_FRAME
                    }
                } else {
                    D2ScanFrame {
                        kind: D2ContainerKind::Array(array_context),
                        ..EMPTY_D2_FRAME
                    }
                };
                depth += 1;
                index += 1;
            }
            b'}' | b']' => {
                let Some(frame) = depth.checked_sub(1).map(|position| stack[position]) else {
                    return Err(DomainError::Json("invalid D2 JSON structure".to_owned()));
                };
                let matching = matches!(
                    (input[index], frame.kind),
                    (b'}', D2ContainerKind::Object(_)) | (b']', D2ContainerKind::Array(_))
                );
                if !matching
                    || !matches!(
                        frame.state,
                        D2ContainerState::ValueOrKeyOrEnd | D2ContainerState::CommaOrEnd
                    )
                {
                    return Err(DomainError::Json("invalid D2 JSON structure".to_owned()));
                }
                depth -= 1;
                root_complete = depth == 0;
                index += 1;
            }
            b',' => {
                let Some(frame) = depth.checked_sub(1).map(|position| &mut stack[position]) else {
                    return Err(DomainError::Json("invalid D2 JSON structure".to_owned()));
                };
                if frame.state != D2ContainerState::CommaOrEnd {
                    return Err(DomainError::Json("invalid D2 JSON structure".to_owned()));
                }
                frame.state = D2ContainerState::ValueOrKeyOrEnd;
                frame.field = D2Field::Other;
                index += 1;
            }
            b':' => {
                let Some(frame) = depth.checked_sub(1).map(|position| &mut stack[position]) else {
                    return Err(DomainError::Json("invalid D2 JSON structure".to_owned()));
                };
                if !matches!(frame.kind, D2ContainerKind::Object(_))
                    || frame.state != D2ContainerState::Colon
                {
                    return Err(DomainError::Json("invalid D2 JSON structure".to_owned()));
                }
                frame.state = D2ContainerState::Value;
                index += 1;
            }
            b'"' => {
                let end = d2_scan_string(input, index)?;
                let is_key = depth.checked_sub(1).is_some_and(|position| {
                    matches!(stack[position].kind, D2ContainerKind::Object(_))
                        && stack[position].state == D2ContainerState::ValueOrKeyOrEnd
                });
                if is_key {
                    let frame = &mut stack[depth - 1];
                    let key = &input[index + 1..end - 1];
                    if key.contains(&b'\\') {
                        return Err(DomainError::Json(
                            "D2 object keys must use unescaped canonical ASCII".to_owned(),
                        ));
                    }
                    frame.field = d2_field(frame.kind, key);
                    frame.state = D2ContainerState::Colon;
                    if matches!(
                        frame.kind,
                        D2ContainerKind::Object(D2ObjectContext::Inference)
                    ) {
                        frame.count =
                            frame.count.checked_add(1).ok_or(DomainError::Incomplete {
                                operation: "D2 inference settings",
                                limit: MAX_D2_INFERENCE_ENTRIES,
                                observed: usize::MAX,
                            })?;
                        d2_require_count(
                            frame.count,
                            MAX_D2_INFERENCE_ENTRIES,
                            "D2 inference settings",
                        )?;
                    }
                } else {
                    d2_accept_value(&mut stack, depth)?;
                    root_complete = depth == 0;
                }
                index = end;
            }
            _ => {
                let end = d2_scan_primitive(input, index)?;
                d2_accept_value(&mut stack, depth)?;
                root_complete = depth == 0;
                index = end;
            }
        }
    }
    if !root_complete || depth != 0 {
        return Err(DomainError::Json("invalid D2 JSON structure".to_owned()));
    }
    Ok(())
}

fn d2_child_context(
    stack: &[D2ScanFrame; MAX_D2_JSON_DEPTH],
    depth: usize,
) -> (D2ObjectContext, D2ArrayContext) {
    if depth == 0 {
        return (D2ObjectContext::Root, D2ArrayContext::Other);
    }
    let parent = stack[depth - 1];
    match (parent.kind, parent.field) {
        (D2ContainerKind::Array(D2ArrayContext::Claims), _) => {
            (D2ObjectContext::Claim, D2ArrayContext::Other)
        }
        (D2ContainerKind::Object(_), D2Field::Claims) => {
            (D2ObjectContext::Other, D2ArrayContext::Claims)
        }
        (D2ContainerKind::Object(_), D2Field::Execution) => {
            (D2ObjectContext::Execution, D2ArrayContext::Other)
        }
        (D2ContainerKind::Object(D2ObjectContext::Claim), D2Field::TargetRefs) => {
            (D2ObjectContext::Other, D2ArrayContext::TargetRefs)
        }
        (D2ContainerKind::Object(D2ObjectContext::Claim), D2Field::SourceIds) => {
            (D2ObjectContext::Other, D2ArrayContext::SourceIds)
        }
        (D2ContainerKind::Object(D2ObjectContext::Claim), D2Field::Assumptions) => {
            (D2ObjectContext::Other, D2ArrayContext::Assumptions)
        }
        (D2ContainerKind::Object(D2ObjectContext::Claim), D2Field::RequestedEvidence) => {
            (D2ObjectContext::Other, D2ArrayContext::RequestedEvidence)
        }
        (D2ContainerKind::Object(D2ObjectContext::Execution), D2Field::InferenceSettings) => {
            (D2ObjectContext::Inference, D2ArrayContext::Other)
        }
        _ => (D2ObjectContext::Other, D2ArrayContext::Other),
    }
}

fn d2_field(kind: D2ContainerKind, key: &[u8]) -> D2Field {
    match (kind, key) {
        (D2ContainerKind::Object(_), b"claims") => D2Field::Claims,
        (D2ContainerKind::Object(_), b"execution") => D2Field::Execution,
        (D2ContainerKind::Object(D2ObjectContext::Claim), b"target_refs") => D2Field::TargetRefs,
        (D2ContainerKind::Object(D2ObjectContext::Claim), b"source_ids") => D2Field::SourceIds,
        (D2ContainerKind::Object(D2ObjectContext::Claim), b"assumptions") => D2Field::Assumptions,
        (D2ContainerKind::Object(D2ObjectContext::Claim), b"requested_evidence") => {
            D2Field::RequestedEvidence
        }
        (D2ContainerKind::Object(D2ObjectContext::Execution), b"inference_settings") => {
            D2Field::InferenceSettings
        }
        _ => D2Field::Other,
    }
}

fn d2_accept_value(stack: &mut [D2ScanFrame; MAX_D2_JSON_DEPTH], depth: usize) -> Result<()> {
    if depth == 0 {
        return Ok(());
    }
    let frame = &mut stack[depth - 1];
    match frame.kind {
        D2ContainerKind::Object(_) if frame.state == D2ContainerState::Value => {
            frame.state = D2ContainerState::CommaOrEnd;
            Ok(())
        }
        D2ContainerKind::Array(context) if frame.state == D2ContainerState::ValueOrKeyOrEnd => {
            frame.count = frame.count.checked_add(1).ok_or(DomainError::Incomplete {
                operation: d2_array_operation(context),
                limit: d2_array_limit(context),
                observed: usize::MAX,
            })?;
            let limit = d2_array_limit(context);
            if limit != usize::MAX {
                d2_require_count(frame.count, limit, d2_array_operation(context))?;
            }
            frame.state = D2ContainerState::CommaOrEnd;
            Ok(())
        }
        _ => Err(DomainError::Json("invalid D2 JSON structure".to_owned())),
    }
}

const fn d2_array_limit(context: D2ArrayContext) -> usize {
    match context {
        D2ArrayContext::Claims => MAX_D2_CLAIMS,
        D2ArrayContext::TargetRefs => MAX_D2_TARGET_REFS,
        D2ArrayContext::SourceIds => MAX_D2_SOURCE_IDS,
        D2ArrayContext::Assumptions => MAX_D2_ASSUMPTIONS,
        D2ArrayContext::RequestedEvidence => MAX_D2_REQUESTED_EVIDENCE,
        D2ArrayContext::Other => usize::MAX,
    }
}

const fn d2_array_operation(context: D2ArrayContext) -> &'static str {
    match context {
        D2ArrayContext::Claims => "D2 execution claims",
        D2ArrayContext::TargetRefs => "D2 claim target refs",
        D2ArrayContext::SourceIds => "D2 claim source IDs",
        D2ArrayContext::Assumptions => "D2 claim assumptions",
        D2ArrayContext::RequestedEvidence => "D2 requested evidence",
        D2ArrayContext::Other => "D2 JSON array",
    }
}

fn d2_require_count(observed: usize, limit: usize, operation: &'static str) -> Result<()> {
    if observed > limit {
        return Err(DomainError::Incomplete {
            operation,
            limit,
            observed,
        });
    }
    Ok(())
}

fn d2_scan_string(input: &[u8], mut index: usize) -> Result<usize> {
    index += 1;
    while index < input.len() {
        match input[index] {
            b'"' => return Ok(index + 1),
            b'\\' => {
                index = index
                    .checked_add(2)
                    .ok_or_else(|| DomainError::Json("invalid D2 JSON string escape".to_owned()))?;
            }
            _ => index += 1,
        }
    }
    Err(DomainError::Json("unterminated D2 JSON string".to_owned()))
}

fn d2_scan_primitive(input: &[u8], mut index: usize) -> Result<usize> {
    let start = index;
    while index < input.len()
        && !input[index].is_ascii_whitespace()
        && !matches!(input[index], b',' | b']' | b'}' | b':' | b'{' | b'[' | b'"')
    {
        index += 1;
    }
    if index == start {
        return Err(DomainError::Json("invalid D2 JSON primitive".to_owned()));
    }
    Ok(index)
}

pub(crate) fn d2_decode_working_observed(input: &[u8], limit: usize) -> Result<usize> {
    let mut string_bytes = 0_u64;
    let mut object_entries = 0_u64;
    let mut list_slots = 0_u64;
    let mut index = 0_usize;
    while index < input.len() {
        match input[index] {
            b'"' => {
                let start = index + 1;
                index = start;
                while index < input.len() {
                    match input[index] {
                        b'"' => break,
                        b'\\' => {
                            index = index
                                .checked_add(2)
                                .ok_or_else(|| working_incomplete(limit, usize::MAX))?;
                            continue;
                        }
                        _ => index += 1,
                    }
                }
                if index >= input.len() {
                    return Err(DomainError::Json("unterminated D2 JSON string".to_owned()));
                }
                string_bytes = string_bytes
                    .checked_add(u64::try_from(index - start).unwrap_or(u64::MAX))
                    .ok_or_else(|| working_incomplete(limit, usize::MAX))?;
            }
            b':' => {
                object_entries = object_entries
                    .checked_add(1)
                    .ok_or_else(|| working_incomplete(limit, usize::MAX))?;
            }
            b',' | b'[' => {
                list_slots = list_slots
                    .checked_add(1)
                    .ok_or_else(|| working_incomplete(limit, usize::MAX))?;
            }
            _ => {}
        }
        index += 1;
    }
    let input_bytes = u64::try_from(input.len()).unwrap_or(u64::MAX);
    let dto_slots = object_entries
        .checked_mul(u64::try_from(std::mem::size_of::<(String, String)>()).unwrap_or(u64::MAX))
        .and_then(|value| {
            list_slots
                .checked_mul(u64::try_from(std::mem::size_of::<String>()).unwrap_or(u64::MAX))
                .and_then(|slots| value.checked_add(slots))
        })
        .ok_or_else(|| working_incomplete(limit, usize::MAX))?;
    let fixed_dto_reservation = u64::try_from(std::mem::size_of::<ReviewExecutionRecorded>())
        .unwrap_or(u64::MAX)
        .checked_add(
            u64::try_from(
                MAX_D2_CLAIMS
                    .checked_mul(std::mem::size_of::<ExecutionClaimV2>())
                    .ok_or_else(|| working_incomplete(limit, usize::MAX))?,
            )
            .unwrap_or(u64::MAX),
        )
        .ok_or_else(|| working_incomplete(limit, usize::MAX))?;
    let observed = input_bytes
        .checked_add(string_bytes)
        .and_then(|value| value.checked_add(dto_slots))
        .and_then(|value| value.checked_add(fixed_dto_reservation))
        .and_then(|value| value.checked_add(input_bytes))
        .and_then(|value| value.checked_add(input_bytes))
        .ok_or_else(|| working_incomplete(limit, usize::MAX))?;
    Ok(usize::try_from(observed).unwrap_or(usize::MAX))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewerRawClosure {
    execution_id: StableId,
    raw_artifact_hash: ContentHash,
    raw_artifact_registration_id: StableId,
    raw_artifact_size: u64,
}

impl ReviewerRawClosure {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.execution_id
            .allocated_bytes()
            .saturating_add(self.raw_artifact_hash.allocated_bytes())
            .saturating_add(self.raw_artifact_registration_id.allocated_bytes())
    }

    pub(crate) fn from_bytes(
        recorded: &ReviewExecutionRecorded,
        raw_reviewer_bytes: &[u8],
    ) -> Result<Self> {
        let raw_artifact_size =
            u64::try_from(raw_reviewer_bytes.len()).map_err(|_| DomainError::Incomplete {
                operation: "D2 raw reviewer bytes",
                limit: MAX_D2_RAW_REVIEWER_BYTES,
                observed: usize::MAX,
            })?;
        let closure = Self {
            execution_id: recorded.execution.id().clone(),
            raw_artifact_hash: ContentHash::sha256(raw_reviewer_bytes),
            raw_artifact_registration_id: recorded.execution.raw_artifact_registration_id().clone(),
            raw_artifact_size,
        };
        if !closure.matches(recorded) {
            return Err(DomainError::Validation(
                "D2 raw closure does not match its atomic execution".to_owned(),
            ));
        }
        Ok(closure)
    }

    pub(crate) fn matches(&self, recorded: &ReviewExecutionRecorded) -> bool {
        self.execution_id == *recorded.execution.id()
            && self.raw_artifact_hash == *recorded.execution.raw_artifact_hash()
            && self.raw_artifact_registration_id
                == *recorded.execution.raw_artifact_registration_id()
    }

    pub(crate) const fn raw_artifact_size(&self) -> u64 {
        self.raw_artifact_size
    }
}

#[derive(Clone, Debug)]
pub struct ValidatedExecutionBundle {
    recorded: ReviewExecutionRecorded,
    raw_closure: ReviewerRawClosure,
    working_peak: usize,
}

#[derive(Clone, Copy)]
enum ReviewerRegistrationRef<'a> {
    V2(&'a ArtifactRegistered),
    V3(&'a ArtifactRegisteredV3),
}

impl<'a> ReviewerRegistrationRef<'a> {
    fn registration_id(self) -> &'a StableId {
        match self {
            Self::V2(value) => value.registration_id(),
            Self::V3(value) => value.registration_id(),
        }
    }

    fn cas_hash(self) -> &'a ContentHash {
        match self {
            Self::V2(value) => value.cas_hash(),
            Self::V3(value) => value.cas_hash(),
        }
    }

    fn size(self) -> u64 {
        match self {
            Self::V2(value) => value.size(),
            Self::V3(value) => value.size(),
        }
    }

    fn sensitivity(self) -> ArtifactSensitivity {
        match self {
            Self::V2(value) => value.sensitivity(),
            Self::V3(value) => value.sensitivity(),
        }
    }

    fn allocated_bytes(self) -> usize {
        match self {
            Self::V2(value) => value.allocated_bytes(),
            Self::V3(value) => value.allocated_bytes(),
        }
    }

    fn is_fake_reviewer_execution(self, execution_id: &StableId) -> bool {
        match self {
            Self::V2(value) => matches!(
                value.source(),
                ArtifactSource::ReviewerExecution {
                    run_id,
                    execution_id: source_execution,
                    reviewer_id,
                } if run_id == value.run_id()
                    && source_execution == execution_id
                    && reviewer_id == FAKE_REVIEWER_ID
            ),
            Self::V3(value) => matches!(
                value.source(),
                ArtifactSourceV3::ReviewerExecution {
                    run_id,
                    execution_id: source_execution,
                    reviewer_id,
                } if run_id == value.run_id()
                    && source_execution == execution_id
                    && reviewer_id == FAKE_REVIEWER_ID
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedSourceBufferAccounting {
    len: usize,
    capacity: usize,
}

impl ResolvedSourceBufferAccounting {
    /// Creates a checked, opaque accounting record for one retained source
    /// buffer. Length can never exceed the allocation capacity.
    pub fn new(len: usize, capacity: usize) -> Result<Self> {
        require_len(
            len,
            MAX_D2_RESOLVED_SOURCE_BYTES,
            "D2 resolved source buffer length",
        )?;
        require_len(
            capacity,
            MAX_D2_WORKING_BYTES,
            "D2 resolved source buffer capacity",
        )?;
        if len > capacity {
            return Err(DomainError::Incomplete {
                operation: "D2 resolved source buffer length",
                limit: capacity,
                observed: len,
            });
        }
        Ok(Self { len, capacity })
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    fn validate(&self) -> Result<()> {
        Self::new(self.len, self.capacity).map(|_| ())
    }
}

impl ValidatedExecutionBundle {
    pub fn fake(
        input: ExecutionRecordInput,
        registration: &ArtifactRegistered,
        raw_reviewer_bytes: Vec<u8>,
        resolved_source_buffers: Vec<&Vec<u8>>,
        claim_inputs: Vec<ExecutionClaimInputV2>,
        outcome: ExecutionOutcome,
    ) -> Result<Self> {
        let mut accounting = Vec::with_capacity(resolved_source_buffers.capacity());
        for source in resolved_source_buffers {
            accounting.push(ResolvedSourceBufferAccounting::new(
                source.len(),
                source.capacity(),
            )?);
        }
        Self::fake_from_source_accounting(
            input,
            registration,
            raw_reviewer_bytes,
            accounting,
            claim_inputs,
            outcome,
        )
    }

    pub fn fake_v3(
        input: ExecutionRecordInput,
        registration: &ArtifactRegisteredV3,
        raw_reviewer_bytes: Vec<u8>,
        resolved_source_buffers: Vec<&Vec<u8>>,
        claim_inputs: Vec<ExecutionClaimInputV2>,
        outcome: ExecutionOutcome,
    ) -> Result<Self> {
        let mut accounting = Vec::with_capacity(resolved_source_buffers.capacity());
        for source in resolved_source_buffers {
            accounting.push(ResolvedSourceBufferAccounting::new(
                source.len(),
                source.capacity(),
            )?);
        }
        for source in &accounting {
            source.validate()?;
        }
        Self::fake_with_limit(
            input,
            ReviewerRegistrationRef::V3(registration),
            raw_reviewer_bytes,
            accounting,
            claim_inputs,
            outcome,
            MAX_D2_WORKING_BYTES,
        )
    }

    pub fn fake_from_source_accounting(
        input: ExecutionRecordInput,
        registration: &ArtifactRegistered,
        raw_reviewer_bytes: Vec<u8>,
        resolved_source_buffers: Vec<ResolvedSourceBufferAccounting>,
        claim_inputs: Vec<ExecutionClaimInputV2>,
        outcome: ExecutionOutcome,
    ) -> Result<Self> {
        for source in &resolved_source_buffers {
            source.validate()?;
        }
        Self::fake_with_limit(
            input,
            ReviewerRegistrationRef::V2(registration),
            raw_reviewer_bytes,
            resolved_source_buffers,
            claim_inputs,
            outcome,
            MAX_D2_WORKING_BYTES,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn fake_with_limit(
        input: ExecutionRecordInput,
        registration: ReviewerRegistrationRef<'_>,
        raw_reviewer_bytes: Vec<u8>,
        resolved_source_buffers: Vec<ResolvedSourceBufferAccounting>,
        claim_inputs: Vec<ExecutionClaimInputV2>,
        outcome: ExecutionOutcome,
        working_limit: usize,
    ) -> Result<Self> {
        require_len(
            raw_reviewer_bytes.len(),
            MAX_D2_RAW_REVIEWER_BYTES,
            "D2 raw reviewer bytes",
        )?;
        let source_bytes = resolved_source_buffers
            .iter()
            .try_fold(0_usize, |total, bytes| {
                total
                    .checked_add(bytes.len())
                    .ok_or(DomainError::Incomplete {
                        operation: "D2 resolved request source bytes",
                        limit: MAX_D2_RESOLVED_SOURCE_BYTES,
                        observed: usize::MAX,
                    })
            })?;
        require_len(
            source_bytes,
            MAX_D2_RESOLVED_SOURCE_BYTES,
            "D2 resolved request source bytes",
        )?;
        input.validate()?;
        outcome.validate()?;
        if outcome.is_structured() {
            require_count(claim_inputs.len(), 1, MAX_D2_CLAIMS, "D2 execution claims")?;
        } else if !claim_inputs.is_empty() {
            return Err(DomainError::Validation(
                "non-structured D2 outcomes cannot carry claims".to_owned(),
            ));
        }
        let preflight_working = reviewer_preflight_working(
            &input,
            registration,
            &raw_reviewer_bytes,
            &resolved_source_buffers,
            &claim_inputs,
            &outcome,
        )?;
        require_len(
            preflight_working,
            working_limit,
            "D2 reviewer-stage working bytes",
        )?;
        let execution_id = input.execution_id()?;
        validate_raw_registration(registration, &execution_id, &raw_reviewer_bytes)?;
        let mut claims = claim_inputs
            .into_iter()
            .map(|claim| {
                ExecutionClaimV2::from_input(
                    execution_id.clone(),
                    input.obligation_ids.clone(),
                    claim,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        claims.sort_by(|left, right| left.id().cmp(right.id()));
        let parsed_claim_ids = claims.iter().map(|claim| claim.id().clone()).collect();
        let raw_hash = ContentHash::sha256(&raw_reviewer_bytes);
        let execution = ExecutionRecord::from_input(
            input,
            registration.registration_id(),
            raw_hash,
            parsed_claim_ids,
            outcome,
        )?;
        let bundle = ReviewExecutionRecorded { claims, execution };
        bundle.validate_shape()?;
        let raw_closure = ReviewerRawClosure::from_bytes(&bundle, &raw_reviewer_bytes)?;
        let execution_bytes = bundle.execution.canonical_bytes()?;
        let mut claim_bytes = Vec::new();
        claim_bytes
            .try_reserve_exact(bundle.claims.len())
            .map_err(|_| working_incomplete(working_limit, bundle.claims.len()))?;
        for claim in &bundle.claims {
            claim_bytes.push(claim.canonical_bytes()?);
        }
        let payload_bytes = bundle.canonical_bytes()?;
        let source_capacity =
            resolved_source_buffers
                .iter()
                .try_fold(0_usize, |total, source| {
                    total
                        .checked_add(source.capacity())
                        .ok_or_else(|| working_incomplete(working_limit, usize::MAX))
                })?;
        let post_working = reviewer_post_working(&ReviewerPostAllocations {
            source_bytes: source_capacity,
            raw_bytes: raw_reviewer_bytes.capacity(),
            resolved_source_buffers: &resolved_source_buffers,
            registration,
            raw_closure: &raw_closure,
            bundle: &bundle,
            execution_bytes: &execution_bytes,
            claim_bytes: &claim_bytes,
            payload_bytes: &payload_bytes,
        })?;
        let working = preflight_working.max(post_working);
        require_len(working, working_limit, "D2 reviewer-stage working bytes")?;
        Ok(Self {
            recorded: bundle,
            raw_closure,
            working_peak: working,
        })
    }

    pub(crate) fn into_parts(self) -> (ReviewExecutionRecorded, ReviewerRawClosure) {
        (self.recorded, self.raw_closure)
    }

    pub(crate) fn parts(&self) -> (&ReviewExecutionRecorded, &ReviewerRawClosure) {
        (&self.recorded, &self.raw_closure)
    }
    #[must_use]
    pub fn execution(&self) -> &ExecutionRecord {
        &self.recorded.execution
    }
    #[must_use]
    pub fn claims(&self) -> &[ExecutionClaimV2] {
        &self.recorded.claims
    }

    #[must_use]
    pub const fn working_peak_bytes(&self) -> usize {
        self.working_peak
    }
}

fn working_incomplete(limit: usize, observed: usize) -> DomainError {
    DomainError::Incomplete {
        operation: "D2 reviewer-stage working bytes",
        limit,
        observed,
    }
}

fn checked_working_add(total: u64, value: usize) -> Result<u64> {
    total
        .checked_add(u64::try_from(value).unwrap_or(u64::MAX))
        .ok_or_else(|| working_incomplete(MAX_D2_WORKING_BYTES, usize::MAX))
}

fn checked_working_mul(count: usize, width: usize) -> Result<usize> {
    count
        .checked_mul(width)
        .ok_or_else(|| working_incomplete(MAX_D2_WORKING_BYTES, usize::MAX))
}

fn finish_working(total: u64) -> Result<usize> {
    usize::try_from(total).map_err(|_| working_incomplete(MAX_D2_WORKING_BYTES, usize::MAX))
}

fn stable_set_charge(values: &BTreeSet<StableId>) -> Result<usize> {
    values.iter().try_fold(
        checked_working_mul(values.len(), std::mem::size_of::<StableId>())?,
        |total, value| {
            total
                .checked_add(value.allocated_bytes())
                .ok_or_else(|| working_incomplete(MAX_D2_WORKING_BYTES, usize::MAX))
        },
    )
}

fn string_set_charge(values: &BTreeSet<String>) -> Result<usize> {
    values.iter().try_fold(
        checked_working_mul(values.len(), std::mem::size_of::<String>())?,
        |total, value| {
            total
                .checked_add(value.capacity())
                .ok_or_else(|| working_incomplete(MAX_D2_WORKING_BYTES, usize::MAX))
        },
    )
}

fn claim_input_charge(input: &ExecutionClaimInputV2) -> Result<usize> {
    [
        input.property_id.capacity(),
        stable_set_charge(&input.target_refs)?,
        input.summary.capacity(),
        stable_set_charge(&input.source_ids)?,
        string_set_charge(&input.assumptions)?,
        string_set_charge(&input.requested_evidence)?,
        std::mem::size_of::<Option<f64>>(),
    ]
    .into_iter()
    .try_fold(0_usize, |total, value| {
        total
            .checked_add(value)
            .ok_or_else(|| working_incomplete(MAX_D2_WORKING_BYTES, usize::MAX))
    })
}

fn outcome_capacity(outcome: &ExecutionOutcome) -> usize {
    match outcome {
        ExecutionOutcome::Structured => 0,
        ExecutionOutcome::Abstained { detail, .. } => detail.capacity(),
        ExecutionOutcome::Malformed { diagnostic, .. }
        | ExecutionOutcome::ProviderFailure { diagnostic, .. } => diagnostic.capacity(),
    }
}

fn reviewer_preflight_working(
    input: &ExecutionRecordInput,
    registration: ReviewerRegistrationRef<'_>,
    raw_reviewer_bytes: &Vec<u8>,
    resolved_source_buffers: &Vec<ResolvedSourceBufferAccounting>,
    claim_inputs: &Vec<ExecutionClaimInputV2>,
    outcome: &ExecutionOutcome,
) -> Result<usize> {
    let mut total = 0_u64;
    for value in [
        raw_reviewer_bytes.capacity(),
        checked_working_mul(
            resolved_source_buffers.capacity(),
            std::mem::size_of::<ResolvedSourceBufferAccounting>(),
        )?,
        checked_working_mul(
            claim_inputs.capacity(),
            std::mem::size_of::<ExecutionClaimInputV2>(),
        )?,
        registration.allocated_bytes(),
        input.plan_id.allocated_bytes(),
        input.wave_id.allocated_bytes(),
        stable_set_charge(&input.obligation_ids)?,
        input.envelope_id.allocated_bytes(),
        input.snapshot_id.allocated_bytes(),
        outcome_capacity(outcome),
    ] {
        total = checked_working_add(total, value)?;
    }
    for source in resolved_source_buffers {
        total = checked_working_add(total, source.capacity())?;
    }
    for claim in claim_inputs {
        total = checked_working_add(total, claim_input_charge(claim)?)?;
    }
    finish_working(total)
}

struct ReviewerPostAllocations<'a> {
    source_bytes: usize,
    raw_bytes: usize,
    resolved_source_buffers: &'a Vec<ResolvedSourceBufferAccounting>,
    registration: ReviewerRegistrationRef<'a>,
    raw_closure: &'a ReviewerRawClosure,
    bundle: &'a ReviewExecutionRecorded,
    execution_bytes: &'a Vec<u8>,
    claim_bytes: &'a Vec<Vec<u8>>,
    payload_bytes: &'a Vec<u8>,
}

fn reviewer_post_working(allocations: &ReviewerPostAllocations<'_>) -> Result<usize> {
    let mut total = 0_u64;
    for value in [
        allocations.source_bytes,
        allocations.raw_bytes,
        checked_working_mul(
            allocations.resolved_source_buffers.capacity(),
            std::mem::size_of::<ResolvedSourceBufferAccounting>(),
        )?,
        allocations.registration.allocated_bytes(),
        allocations.raw_closure.allocated_bytes(),
        allocations.bundle.execution.allocated_bytes(),
        checked_working_mul(
            allocations.bundle.claims.capacity(),
            std::mem::size_of::<ExecutionClaimV2>(),
        )?,
        allocations.execution_bytes.capacity(),
        checked_working_mul(
            allocations.claim_bytes.capacity(),
            std::mem::size_of::<Vec<u8>>(),
        )?,
        allocations.payload_bytes.capacity(),
    ] {
        total = checked_working_add(total, value)?;
    }
    for claim in &allocations.bundle.claims {
        total = checked_working_add(total, claim.allocated_bytes())?;
    }
    for bytes in allocations.claim_bytes {
        total = checked_working_add(total, bytes.capacity())?;
    }
    finish_working(total)
}

fn validate_raw_registration(
    registration: ReviewerRegistrationRef<'_>,
    execution_id: &StableId,
    raw_bytes: &[u8],
) -> Result<()> {
    let size = u64::try_from(raw_bytes.len()).map_err(|_| DomainError::Incomplete {
        operation: "D2 raw reviewer bytes",
        limit: MAX_D2_RAW_REVIEWER_BYTES,
        observed: usize::MAX,
    })?;
    if registration.cas_hash() != &ContentHash::sha256(raw_bytes)
        || registration.size() != size
        || registration.sensitivity() != ArtifactSensitivity::Sensitive
        || !registration.is_fake_reviewer_execution(execution_id)
    {
        return Err(DomainError::Validation(
            "D2 raw registration must exactly bind bytes, sensitive handling, run, execution, and reviewer"
                .to_owned(),
        ));
    }
    Ok(())
}

fn validate_trace_map(values: &BTreeMap<String, String>) -> Result<()> {
    require_count(
        values.len(),
        0,
        MAX_D2_INFERENCE_ENTRIES,
        "D2 inference settings",
    )?;
    for (key, value) in values {
        require_nonempty_bounded(key, MAX_D2_TRACE_BYTES, "D2 inference key")?;
        require_len(
            value.len(),
            MAX_D2_INFERENCE_VALUE_BYTES,
            "D2 inference value",
        )?;
    }
    Ok(())
}

fn validate_confidence(value: Option<f64>) -> Result<()> {
    if value.is_some_and(|number| !number.is_finite() || !(0.0..=1.0).contains(&number)) {
        return Err(DomainError::Validation(
            "D2 candidate confidence must be null or finite in [0,1]".to_owned(),
        ));
    }
    Ok(())
}

fn strict_sorted_ids(values: Vec<StableId>, field: &'static str) -> Result<BTreeSet<StableId>> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(DomainError::Validation(format!(
            "{field} must be strictly StableId-ordered and duplicate-free"
        )));
    }
    Ok(values.into_iter().collect())
}

fn strict_sorted_strings(values: Vec<String>, field: &'static str) -> Result<BTreeSet<String>> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(DomainError::Validation(format!(
            "{field} must be strictly UTF-8-lexical and duplicate-free"
        )));
    }
    Ok(values.into_iter().collect())
}

fn require_nonempty_bounded(value: &str, limit: usize, operation: &'static str) -> Result<()> {
    if value.is_empty() {
        return Err(DomainError::EmptyField { field: operation });
    }
    require_len(value.len(), limit, operation)
}

fn require_len(observed: usize, limit: usize, operation: &'static str) -> Result<()> {
    if observed > limit {
        return Err(DomainError::Incomplete {
            operation,
            limit,
            observed,
        });
    }
    Ok(())
}

fn require_count(
    observed: usize,
    minimum: usize,
    limit: usize,
    operation: &'static str,
) -> Result<()> {
    if observed < minimum {
        return Err(DomainError::EmptyField { field: operation });
    }
    require_len(observed, limit, operation)
}

fn derived_id(kind: &str, identity_bytes: &[u8]) -> Result<StableId> {
    StableId::parse(format!("{kind}:{}", ContentHash::sha256(identity_bytes)))
}

#[derive(Default)]
struct CountingWriter {
    len: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.len = self
            .len
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("canonical byte count overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn bounded_json<T: Serialize>(
    value: &T,
    limit: usize,
    operation: &'static str,
) -> Result<Vec<u8>> {
    let mut count = CountingWriter::default();
    serde_json::to_writer(&mut count, value)
        .map_err(|error| DomainError::CanonicalJson(error.to_string()))?;
    require_len(count.len, limit, operation)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(count.len)
        .map_err(|_| DomainError::Incomplete {
            operation,
            limit,
            observed: count.len,
        })?;
    serde_json::to_writer(&mut bytes, value)
        .map_err(|error| DomainError::CanonicalJson(error.to_string()))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> StableId {
        StableId::parse(value).unwrap()
    }

    fn fixture_input(attempt: u32) -> ExecutionRecordInput {
        ExecutionRecordInput::fake(
            id("plan:fixture"),
            id("schedule-wave:fixture"),
            id("obligation:fixture"),
            id("context-envelope:fixture"),
            id("snapshot:fixture"),
            attempt,
        )
        .unwrap()
    }

    fn claim(confidence: Option<f64>) -> ExecutionClaimInputV2 {
        ExecutionClaimInputV2::new(
            "property.fixture",
            BTreeSet::from([id("file:a"), id("file:b")]),
            ClaimPolarity::IssuePresent,
            "fixture claim",
            BTreeSet::from([id("file:a")]),
            BTreeSet::new(),
            BTreeSet::new(),
            confidence,
        )
        .unwrap()
    }

    fn raw_registration(input: &ExecutionRecordInput, raw: &[u8]) -> ArtifactRegistered {
        ArtifactRegistered::reviewer_execution(
            id("run:d2-execution"),
            input.execution_id().unwrap(),
            FAKE_REVIEWER_ID,
            ContentHash::sha256(raw),
            "application/json",
            u64::try_from(raw.len()).unwrap(),
        )
        .unwrap()
    }

    fn structured(attempt: u32, confidence: Option<f64>) -> ValidatedExecutionBundle {
        let input = fixture_input(attempt);
        let raw = br#"{"claims":[{"fixture":true}]}"#;
        let source = b"source".to_vec();
        let registration = raw_registration(&input, raw);
        ValidatedExecutionBundle::fake(
            input,
            &registration,
            raw.to_vec(),
            vec![&source],
            vec![claim(confidence)],
            ExecutionOutcome::Structured,
        )
        .unwrap()
    }

    #[test]
    fn exact_fake_descriptor_nulls_empty_tools_and_retry_identity_are_canonical() {
        let first = structured(1, Some(0.9));
        let second = structured(2, Some(0.9));
        assert_ne!(first.execution().id(), second.execution().id());
        let text = String::from_utf8(first.execution().canonical_bytes().unwrap()).unwrap();
        assert!(text.contains("\"provider\":null"));
        assert!(text.contains("\"model\":null"));
        assert!(text.contains("\"model_revision\":null"));
        assert!(text.contains("\"inference_settings\":{}"));
        assert!(text.contains("\"tool_calls\":[]"));
        assert!(text.contains("\"outcome\":{\"kind\":\"structured\"}"));
        assert_eq!(first.execution().tool_call_count(), 0);
    }

    #[test]
    fn confidence_changes_full_body_but_never_claim_identity_or_authority() {
        let low = structured(1, Some(0.0));
        let high = structured(1, Some(1.0));
        assert_eq!(low.claims()[0].id(), high.claims()[0].id());
        assert_ne!(
            low.claims()[0].body_hash().unwrap(),
            high.claims()[0].body_hash().unwrap()
        );
        for claim in [low.claims()[0].clone(), high.claims()[0].clone()] {
            assert_eq!(claim.disposition(), ClaimDisposition::Proposed);
            assert_eq!(claim.review_status(), ReviewStatus::Unreviewed);
            assert_eq!(claim.author_kind(), ClaimAuthorKind::Ai);
        }
        for confidence in [Some(f64::NAN), Some(f64::INFINITY), Some(-0.1), Some(1.1)] {
            assert!(
                ExecutionClaimInputV2::new(
                    "property.fixture",
                    BTreeSet::from([id("file:a")]),
                    ClaimPolarity::IssueAbsent,
                    "summary",
                    BTreeSet::from([id("file:a")]),
                    BTreeSet::new(),
                    BTreeSet::new(),
                    confidence,
                )
                .is_err()
            );
        }
    }

    #[test]
    fn every_closed_failure_outcome_is_claim_free_and_bounded() {
        let abstentions = [
            AbstentionReason::InsufficientContext,
            AbstentionReason::UnresolvedSymbol,
            AbstentionReason::RequiredEvidenceUnavailable,
            AbstentionReason::PropertyNotUnderstood,
            AbstentionReason::ConflictingSources,
            AbstentionReason::ToolCapabilityMissing,
            AbstentionReason::BudgetExhausted,
            AbstentionReason::PromptInjectionSuspected,
        ];
        let malformed = [
            MalformedOutputReason::SchemaViolation,
            MalformedOutputReason::UnresolvedSourceId,
            MalformedOutputReason::UnknownObligationId,
            MalformedOutputReason::InvalidPolarity,
            MalformedOutputReason::ConfidenceOutOfRange,
            MalformedOutputReason::UnknownField,
        ];
        let mut outcomes = abstentions
            .into_iter()
            .map(|reason| ExecutionOutcome::Abstained {
                reason,
                detail: "fixture".to_owned(),
            })
            .chain(
                malformed
                    .into_iter()
                    .map(|reason| ExecutionOutcome::Malformed {
                        reason,
                        diagnostic: "fixture".to_owned(),
                    }),
            )
            .collect::<Vec<_>>();
        outcomes.push(ExecutionOutcome::ProviderFailure {
            retryable: true,
            diagnostic: "fixture".to_owned(),
        });
        outcomes.push(ExecutionOutcome::ProviderFailure {
            retryable: false,
            diagnostic: "fixture".to_owned(),
        });
        for (offset, outcome) in outcomes.into_iter().enumerate() {
            let input = fixture_input(u32::try_from(offset + 1).unwrap());
            let raw = b"fixture";
            let registration = raw_registration(&input, raw);
            let bundle = ValidatedExecutionBundle::fake(
                input,
                &registration,
                raw.to_vec(),
                vec![],
                vec![],
                outcome,
            )
            .unwrap();
            assert!(bundle.claims().is_empty());
            assert!(bundle.execution().parsed_claim_ids().is_empty());
        }
        let exact = "x".repeat(MAX_D2_OUTCOME_TEXT_BYTES);
        assert!(
            ExecutionOutcome::Abstained {
                reason: AbstentionReason::InsufficientContext,
                detail: exact,
            }
            .validate()
            .is_ok()
        );
        assert!(
            ExecutionOutcome::Abstained {
                reason: AbstentionReason::InsufficientContext,
                detail: "x".repeat(MAX_D2_OUTCOME_TEXT_BYTES + 1),
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn raw_and_resolved_source_limits_are_inclusive() {
        let input = fixture_input(1);
        let exact_raw = vec![b'x'; MAX_D2_RAW_REVIEWER_BYTES];
        let registration = raw_registration(&input, &exact_raw);
        ValidatedExecutionBundle::fake(
            input,
            &registration,
            exact_raw,
            vec![],
            vec![],
            ExecutionOutcome::ProviderFailure {
                retryable: false,
                diagnostic: "fixture".to_owned(),
            },
        )
        .unwrap();

        let input = fixture_input(2);
        let over_raw = vec![b'x'; MAX_D2_RAW_REVIEWER_BYTES + 1];
        let registration = raw_registration(&input, &over_raw);
        assert!(matches!(
            ValidatedExecutionBundle::fake(
                input,
                &registration,
                over_raw,
                vec![],
                vec![],
                ExecutionOutcome::ProviderFailure {
                    retryable: false,
                    diagnostic: "fixture".to_owned(),
                },
            ),
            Err(DomainError::Incomplete { .. })
        ));

        let source = vec![b's'; MAX_D2_RESOLVED_SOURCE_BYTES];
        let input = fixture_input(3);
        let raw = b"x";
        let registration = raw_registration(&input, raw);
        ValidatedExecutionBundle::fake(
            input,
            &registration,
            raw.to_vec(),
            vec![&source],
            vec![],
            ExecutionOutcome::ProviderFailure {
                retryable: true,
                diagnostic: "fixture".to_owned(),
            },
        )
        .unwrap();
        let over_source = vec![b's'; MAX_D2_RESOLVED_SOURCE_BYTES + 1];
        let input = fixture_input(4);
        let registration = raw_registration(&input, raw);
        assert!(matches!(
            ValidatedExecutionBundle::fake(
                input,
                &registration,
                raw.to_vec(),
                vec![&over_source],
                vec![],
                ExecutionOutcome::ProviderFailure {
                    retryable: true,
                    diagnostic: "fixture".to_owned(),
                },
            ),
            Err(DomainError::Incomplete { .. })
        ));
    }

    #[test]
    fn reviewer_working_peak_is_exact_plus_one_and_overflow_is_typed() {
        let raw = b"working";
        let input = fixture_input(1);
        let registration = raw_registration(&input, raw);
        let baseline = ValidatedExecutionBundle::fake(
            input.clone(),
            &registration,
            raw.to_vec(),
            vec![],
            vec![],
            ExecutionOutcome::ProviderFailure {
                retryable: false,
                diagnostic: "x".to_owned(),
            },
        )
        .unwrap();
        let additional = MAX_D2_WORKING_BYTES
            .checked_sub(baseline.working_peak_bytes())
            .unwrap();
        drop(baseline);

        let mut exact_text = String::with_capacity(1 + additional);
        exact_text.push('x');
        let exact = ValidatedExecutionBundle::fake(
            input.clone(),
            &registration,
            raw.to_vec(),
            vec![],
            vec![],
            ExecutionOutcome::ProviderFailure {
                retryable: false,
                diagnostic: exact_text,
            },
        )
        .unwrap();
        assert_eq!(exact.working_peak_bytes(), MAX_D2_WORKING_BYTES);
        drop(exact);

        let mut over_text = String::with_capacity(2 + additional);
        over_text.push('x');
        assert!(matches!(
            ValidatedExecutionBundle::fake(
                input,
                &registration,
                raw.to_vec(),
                vec![],
                vec![],
                ExecutionOutcome::ProviderFailure {
                    retryable: false,
                    diagnostic: over_text,
                },
            ),
            Err(DomainError::Incomplete {
                operation: "D2 reviewer-stage working bytes",
                limit: MAX_D2_WORKING_BYTES,
                observed,
            }) if observed == MAX_D2_WORKING_BYTES + 1
        ));
        assert!(matches!(
            checked_working_add(u64::MAX, 1),
            Err(DomainError::Incomplete {
                operation: "D2 reviewer-stage working bytes",
                observed: usize::MAX,
                ..
            })
        ));
    }

    #[test]
    fn source_buffer_accounting_is_checked_opaque_and_matches_real_buffer_peak() {
        assert!(matches!(
            ResolvedSourceBufferAccounting::new(1, 0),
            Err(DomainError::Incomplete {
                operation: "D2 resolved source buffer length",
                limit: 0,
                observed: 1,
            })
        ));
        let exact = ResolvedSourceBufferAccounting::new(5, 5).unwrap();
        assert_eq!(exact.len(), 5);
        assert_eq!(exact.capacity(), 5);

        let raw = b"accounting";
        let input = fixture_input(1);
        let registration = raw_registration(&input, raw);
        let mut source = Vec::with_capacity(16);
        source.extend_from_slice(b"source");
        let from_real = ValidatedExecutionBundle::fake(
            input.clone(),
            &registration,
            raw.to_vec(),
            vec![&source],
            vec![],
            ExecutionOutcome::ProviderFailure {
                retryable: false,
                diagnostic: "fixture".to_owned(),
            },
        )
        .unwrap();
        let from_accounting = ValidatedExecutionBundle::fake_from_source_accounting(
            input,
            &registration,
            raw.to_vec(),
            vec![ResolvedSourceBufferAccounting::new(source.len(), source.capacity()).unwrap()],
            vec![],
            ExecutionOutcome::ProviderFailure {
                retryable: false,
                diagnostic: "fixture".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(
            from_accounting.working_peak_bytes(),
            from_real.working_peak_bytes()
        );
    }

    #[test]
    fn decode_preflight_accepts_exact_and_refuses_plus_one_before_dto_allocation() {
        let raw = br#"{"claims":[],"execution":{}}"#;
        let observed = d2_decode_working_observed(raw, usize::MAX).unwrap();
        preflight_d2_decode_working(raw, observed).unwrap();
        assert!(matches!(
            preflight_d2_decode_working(raw, observed - 1),
            Err(DomainError::Incomplete {
                operation: "D2 decode working bytes",
                limit,
                observed: actual,
            }) if limit + 1 == actual && actual == observed
        ));
    }

    #[test]
    fn claim_string_and_count_limits_are_inclusive() {
        let exact = ExecutionClaimInputV2::new(
            "p",
            (0..MAX_D2_TARGET_REFS)
                .map(|index| id(&format!("file:t{index:03}")))
                .collect(),
            ClaimPolarity::IssueAbsent,
            "s".repeat(MAX_D2_SUMMARY_BYTES),
            (0..MAX_D2_SOURCE_IDS)
                .map(|index| id(&format!("file:s{index:03}")))
                .collect(),
            (0..MAX_D2_ASSUMPTIONS)
                .map(|index| format!("a{index:03}"))
                .collect(),
            (0..MAX_D2_REQUESTED_EVIDENCE)
                .map(|index| format!("e{index:03}"))
                .collect(),
            None,
        );
        assert!(exact.is_ok());
        assert!(matches!(
            ExecutionClaimInputV2::new(
                "p",
                BTreeSet::from([id("file:a")]),
                ClaimPolarity::IssueAbsent,
                "s".repeat(MAX_D2_SUMMARY_BYTES + 1),
                BTreeSet::from([id("file:a")]),
                BTreeSet::new(),
                BTreeSet::new(),
                None,
            ),
            Err(DomainError::Incomplete { .. })
        ));
        assert!(
            ExecutionClaimInputV2::new(
                "p",
                (0..=MAX_D2_TARGET_REFS)
                    .map(|index| id(&format!("file:t{index:03}")))
                    .collect(),
                ClaimPolarity::IssueAbsent,
                "s",
                BTreeSet::from([id("file:a")]),
                BTreeSet::new(),
                BTreeSet::new(),
                None,
            )
            .is_err()
        );
    }

    #[test]
    fn strict_decode_rejects_reorder_duplicate_unknown_and_tools() {
        let bundle = structured(1, None);
        let claim = String::from_utf8(bundle.claims()[0].canonical_bytes().unwrap()).unwrap();
        let reordered = claim.replace(
            "\"target_refs\":[\"file:a\",\"file:b\"]",
            "\"target_refs\":[\"file:b\",\"file:a\"]",
        );
        assert!(serde_json::from_str::<ExecutionClaimV2>(&reordered).is_err());
        let duplicate = claim.replace(
            "\"target_refs\":[\"file:a\",\"file:b\"]",
            "\"target_refs\":[\"file:a\",\"file:a\"]",
        );
        assert!(serde_json::from_str::<ExecutionClaimV2>(&duplicate).is_err());
        let unknown = claim.replacen('{', "{\"unknown\":null,", 1);
        assert!(serde_json::from_str::<ExecutionClaimV2>(&unknown).is_err());
        let missing_confidence = claim.replace("\"candidate_confidence\":null,", "");
        assert_ne!(missing_confidence, claim);
        assert!(serde_json::from_str::<ExecutionClaimV2>(&missing_confidence).is_err());
        let duplicate_id = claim.replacen("\"id\":", "\"id\":\"claim:duplicate\",\"id\":", 1);
        assert!(serde_json::from_str::<ExecutionClaimV2>(&duplicate_id).is_err());

        let execution = String::from_utf8(bundle.execution().canonical_bytes().unwrap()).unwrap();
        let tools = execution.replace("\"tool_calls\":[]", "\"tool_calls\":[{}]");
        assert!(serde_json::from_str::<ExecutionRecord>(&tools).is_err());
        let tampered = execution.replace("\"attempt\":1", "\"attempt\":2");
        assert!(serde_json::from_str::<ExecutionRecord>(&tampered).is_err());
        let missing_provider = execution.replace(",\"provider\":null", "");
        assert!(serde_json::from_str::<ExecutionRecord>(&missing_provider).is_err());
        let outcome_null = execution.replace(
            "\"outcome\":{\"kind\":\"structured\"}",
            "\"outcome\":{\"kind\":\"structured\",\"reason\":null}",
        );
        assert!(serde_json::from_str::<ExecutionRecord>(&outcome_null).is_err());
    }

    #[test]
    fn atomic_claim_ids_distinguish_collision_from_distinct_ordering_failure() {
        let (mut recorded, _closure) = structured(1, None).into_parts();
        let duplicate_id = recorded.claims[0].id().clone();
        recorded.claims.push(recorded.claims[0].clone());
        assert!(matches!(
            recorded.validate_shape(),
            Err(DomainError::IdCollision { id }) if id == duplicate_id
        ));

        let input = fixture_input(2);
        let raw = br#"{"claims":[{"fixture":true}]}"#;
        let source = b"source".to_vec();
        let registration = raw_registration(&input, raw);
        let second_claim = ExecutionClaimInputV2::new(
            "property.fixture",
            BTreeSet::from([id("file:a"), id("file:b")]),
            ClaimPolarity::IssueAbsent,
            "a distinct fixture claim",
            BTreeSet::from([id("file:a")]),
            BTreeSet::new(),
            BTreeSet::new(),
            None,
        )
        .unwrap();
        let bundle = ValidatedExecutionBundle::fake(
            input,
            &registration,
            raw.to_vec(),
            vec![&source],
            vec![claim(None), second_claim],
            ExecutionOutcome::Structured,
        )
        .unwrap();
        let (mut unsorted, _closure) = bundle.into_parts();
        unsorted.claims.swap(0, 1);
        assert!(matches!(
            unsorted.validate_shape(),
            Err(DomainError::Validation(message)) if message == "D2 claims must be ordered by claim ID"
        ));
    }
}
