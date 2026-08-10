//! Bounded, deterministic, CAS-free context projection construction.
//!
//! Bytes enter this module only through the ordered resolver session.  The
//! aggregate supplies accepted metadata; this module never opens a CAS object
//! or a workspace path.

#[cfg(test)]
use crate::ArtifactRegistered;
use crate::{
    Artifact, ArtifactSensitivity, ContentHash, DomainError, Obligation, ProgramSpace, Result,
    ReviewAggregate, Severity, SnapshotSourceRecordEntry, StableId,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const MAX: usize = 4_096;
const MAX_RELATIONS: usize = 1_000_000;
const MAX_CONTAINS: usize = 1_000_000;
const MAX_FILES: usize = 64;
const MAX_FILE_BYTES: u64 = 1_048_576;
const MAX_RESOLVED: u64 = 8_388_608;
const MAX_EXCERPT: usize = 262_144;
const MAX_TOTAL_EXCERPT: usize = 1_048_576;
const MAX_LINES: usize = 400;
const MAX_ANCHORS: usize = 1_024;
const MAX_PATHS: usize = 20;
const MAX_TESTS: usize = 10;
const MAX_TEXT: usize = 16_384;
const MAX_BODY: usize = 786_432;

/// Closed successful exclusion vocabulary, in ADR precedence order.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    PathCap,
    TestCap,
    NotReached,
    IncludedFileCap,
    ArtifactBytesCap,
    TotalResolvedBytesCap,
    GiantLine,
    ExcerptBytesCap,
    TotalExcerptBytesCap,
}

impl ExclusionReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PathCap => "path_cap",
            Self::TestCap => "test_cap",
            Self::NotReached => "not_reached",
            Self::IncludedFileCap => "included_file_cap",
            Self::ArtifactBytesCap => "artifact_bytes_cap",
            Self::TotalResolvedBytesCap => "total_resolved_bytes_cap",
            Self::GiantLine => "giant_line",
            Self::ExcerptBytesCap => "excerpt_bytes_cap",
            Self::TotalExcerptBytesCap => "total_excerpt_bytes_cap",
        }
    }

    const fn loss_description(self) -> &'static str {
        match self {
            Self::PathCap => "context_loss:path_cap",
            Self::TestCap => "context_loss:test_cap",
            Self::NotReached => "context_loss:not_reached",
            Self::IncludedFileCap => "context_loss:included_file_cap",
            Self::ArtifactBytesCap => "context_loss:artifact_bytes_cap",
            Self::TotalResolvedBytesCap => "context_loss:total_resolved_bytes_cap",
            Self::GiantLine => "context_loss:giant_line",
            Self::ExcerptBytesCap => "context_loss:excerpt_bytes_cap",
            Self::TotalExcerptBytesCap => "context_loss:total_excerpt_bytes_cap",
        }
    }
}

/// Fixed D1 context projection policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPolicyV1 {
    anchors_per_file: usize,
    callees_depth: usize,
    callers_depth: usize,
    canonical_envelope_bytes: usize,
    contains_edges: usize,
    discovery_paths: usize,
    excerpt_lines: usize,
    included_files: usize,
    max_candidates: usize,
    max_discovered_structural_ids: usize,
    max_excerpt_bytes: usize,
    max_losses: usize,
    max_resolved_artifact_bytes: u64,
    max_resolved_bytes: u64,
    max_string_bytes: usize,
    max_total_excerpt_bytes: usize,
    max_unknowns: usize,
    obligations_per_envelope: usize,
    related_tests: usize,
    relation_scan: usize,
}

impl ContextPolicyV1 {
    pub const VERSION: &'static str = "context.baseline@1";

    #[must_use]
    pub const fn baseline() -> Self {
        Self {
            anchors_per_file: MAX_ANCHORS,
            callees_depth: 3,
            callers_depth: 2,
            canonical_envelope_bytes: MAX_BODY,
            contains_edges: MAX_CONTAINS,
            discovery_paths: MAX_PATHS,
            excerpt_lines: MAX_LINES,
            included_files: MAX_FILES,
            max_candidates: MAX,
            max_discovered_structural_ids: MAX,
            max_excerpt_bytes: MAX_EXCERPT,
            max_losses: 64,
            max_resolved_artifact_bytes: MAX_FILE_BYTES,
            max_resolved_bytes: MAX_RESOLVED,
            max_string_bytes: MAX_TEXT,
            max_total_excerpt_bytes: MAX_TOTAL_EXCERPT,
            max_unknowns: 64,
            obligations_per_envelope: 1,
            related_tests: MAX_TESTS,
            relation_scan: MAX_RELATIONS,
        }
    }

    pub const fn anchors_per_file(&self) -> usize {
        self.anchors_per_file
    }
    pub const fn callees_depth(&self) -> usize {
        self.callees_depth
    }
    pub const fn callers_depth(&self) -> usize {
        self.callers_depth
    }
    pub const fn canonical_envelope_bytes(&self) -> usize {
        self.canonical_envelope_bytes
    }
    pub const fn contains_edges(&self) -> usize {
        self.contains_edges
    }
    pub const fn discovery_paths(&self) -> usize {
        self.discovery_paths
    }
    pub const fn excerpt_lines(&self) -> usize {
        self.excerpt_lines
    }
    pub const fn included_files(&self) -> usize {
        self.included_files
    }
    pub const fn max_candidates(&self) -> usize {
        self.max_candidates
    }
    pub const fn max_discovered_structural_ids(&self) -> usize {
        self.max_discovered_structural_ids
    }
    pub const fn max_excerpt_bytes(&self) -> usize {
        self.max_excerpt_bytes
    }
    pub const fn max_losses(&self) -> usize {
        self.max_losses
    }
    pub const fn max_resolved_artifact_bytes(&self) -> u64 {
        self.max_resolved_artifact_bytes
    }
    pub const fn max_resolved_bytes(&self) -> u64 {
        self.max_resolved_bytes
    }
    pub const fn max_string_bytes(&self) -> usize {
        self.max_string_bytes
    }
    pub const fn max_total_excerpt_bytes(&self) -> usize {
        self.max_total_excerpt_bytes
    }
    pub const fn max_unknowns(&self) -> usize {
        self.max_unknowns
    }
    pub const fn obligations_per_envelope(&self) -> usize {
        self.obligations_per_envelope
    }
    pub const fn related_tests(&self) -> usize {
        self.related_tests
    }
    pub const fn relation_scan(&self) -> usize {
        self.relation_scan
    }

    /// Exact canonical sub-policy bytes mandated by ADR 0016.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        let text = r#"{"anchors_per_file":1024,"callees_depth":3,"callers_depth":2,"canonical_envelope_bytes":786432,"contains_edges":1000000,"discovery_paths":20,"edge_kind_direction_order":["calls:forward","calls:reverse","contains:forward","contains:reverse","covers:forward","covers:reverse"],"exclusion_reason_precedence":["path_cap","test_cap","not_reached","included_file_cap","artifact_bytes_cap","total_resolved_bytes_cap","giant_line","excerpt_bytes_cap","total_excerpt_bytes_cap"],"excerpt_lines":400,"included_files":64,"loss_descriptions":[["artifact_bytes_cap","context_loss:artifact_bytes_cap"],["excerpt_bytes_cap","context_loss:excerpt_bytes_cap"],["excerpt_window_truncated","context_loss:excerpt_window_truncated"],["giant_line","context_loss:giant_line"],["included_file_cap","context_loss:included_file_cap"],["not_reached","context_loss:not_reached"],["path_cap","context_loss:path_cap"],["test_cap","context_loss:test_cap"],["total_excerpt_bytes_cap","context_loss:total_excerpt_bytes_cap"],["total_resolved_bytes_cap","context_loss:total_resolved_bytes_cap"]],"max_assumptions":64,"max_candidates":4096,"max_discovered_structural_ids":4096,"max_excerpt_bytes":262144,"max_losses":64,"max_resolved_artifact_bytes":1048576,"max_resolved_bytes":8388608,"max_string_bytes":16384,"max_total_excerpt_bytes":1048576,"max_unknowns":64,"obligations_per_envelope":1,"related_tests":10,"relation_scan":1000000,"rules":["all_candidates_metadata_closure","anchors_out_of_range_domain_failure","anchors_reached_contains_locations","baseline_assumptions_empty","baseline_exactly_one_obligation","bfs_visited_edge_kind_direction_depth","bounded_canonical_serialization","calls_caller_to_callee","canonical_bfs_predecessor_paths","contains_file_to_member","context_identity_bounded_writer","covers_test_to_subject","event_admissions_context_projection","excerpts_lf_raw","full_range_some_normalizes_none","giant_line_exclude_loss","live_projection_private_admission","loss_adr0013_grouped_v1","max_string_all_canonical_text","offline_replay_metadata_only","ranked_greedy_two_phase_resolution","seed_kind_exact_expansion","selection_path_test_caps_exclude_loss","safety_caps_incomplete","source_artifact_excerpt_integrity"],"unknown_descriptions":["context_unknown:unresolved_invariant_scope","context_unknown:unresolved_relation_endpoint","context_unknown:unresolved_review_context_member","context_unknown:unresolved_seed_reference"],"version":"context.baseline@1"}"#;
        if text.len() > MAX_BODY {
            return Err(incomplete("context policy bytes", MAX_BODY, text.len()));
        }
        Ok(text.as_bytes().to_vec())
    }
    pub fn hash(&self) -> Result<ContentHash> {
        Ok(ContentHash::sha256(&self.canonical_bytes()?))
    }
}

impl Serialize for ContextPolicyV1 {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value: serde_json::Value =
            serde_json::from_slice(&self.canonical_bytes().map_err(serde::ser::Error::custom)?)
                .map_err(serde::ser::Error::custom)?;
        value.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ContextPolicyV1 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let policy = Self::baseline();
        let expected: serde_json::Value =
            serde_json::from_slice(&policy.canonical_bytes().map_err(serde::de::Error::custom)?)
                .map_err(serde::de::Error::custom)?;
        if value != expected {
            return Err(serde::de::Error::custom(
                "context policy is not the fixed v1 DTO",
            ));
        }
        Ok(policy)
    }
}

/// Typed failure emitted by the context protocol. Constructors are private so
/// callers cannot manufacture an apparently policy-derived failure.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContextError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("context session protocol violation: {0}")]
    Protocol(&'static str),
}
type ContextResult<T> = std::result::Result<T, ContextError>;
pub(crate) fn context_domain_error(error: ContextError) -> DomainError {
    match error {
        ContextError::Domain(error) => error,
        ContextError::Protocol(message) => DomainError::Validation(message.to_owned()),
    }
}
fn incomplete(operation: &'static str, limit: usize, observed: usize) -> DomainError {
    DomainError::Incomplete {
        operation,
        limit,
        observed,
    }
}

fn is_sha256(hash: &ContentHash) -> bool {
    let value = hash.to_string();
    value.len() == 71
        && value
            .strip_prefix("sha256:")
            .is_some_and(|hex| hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// Inclusive one-based line range. `None` denotes the whole raw file.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExcerptRange {
    start_line: u32,
    end_line: u32,
}
impl ExcerptRange {
    pub fn start_line(&self) -> u32 {
        self.start_line
    }
    pub fn end_line(&self) -> u32 {
        self.end_line
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceArtifactRef {
    registration_id: StableId,
    artifact_id: StableId,
    content_hash: ContentHash,
    cas_hash: ContentHash,
    excerpt: Option<ExcerptRange>,
    excerpt_byte_length: u64,
    excerpt_hash: ContentHash,
}
impl SourceArtifactRef {
    pub fn registration_id(&self) -> &StableId {
        &self.registration_id
    }
    pub fn artifact_id(&self) -> &StableId {
        &self.artifact_id
    }
    pub fn content_hash(&self) -> &ContentHash {
        &self.content_hash
    }
    pub fn cas_hash(&self) -> &ContentHash {
        &self.cas_hash
    }
    pub fn excerpt(&self) -> Option<&ExcerptRange> {
        self.excerpt.as_ref()
    }
    pub fn excerpt_byte_length(&self) -> u64 {
        self.excerpt_byte_length
    }
    pub fn excerpt_hash(&self) -> &ContentHash {
        &self.excerpt_hash
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludedSourceRef {
    artifact_id: StableId,
    reason: ExclusionReason,
}
impl ExcludedSourceRef {
    pub fn artifact_id(&self) -> &StableId {
        &self.artifact_id
    }
    pub const fn reason(&self) -> ExclusionReason {
        self.reason
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeUnknown {
    description: String,
    source_ids: BTreeSet<StableId>,
}
impl EnvelopeUnknown {
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeLoss {
    description: String,
    severity: Severity,
    affected_properties: BTreeSet<String>,
    source_ids: BTreeSet<StableId>,
}
impl EnvelopeLoss {
    pub fn description(&self) -> &str {
        &self.description
    }
    pub const fn severity(&self) -> Severity {
        self.severity
    }
    pub fn affected_properties(&self) -> &BTreeSet<String> {
        &self.affected_properties
    }
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
}

/// Canonical bounded projection, with no raw source bytes retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewContextEnvelope {
    id: StableId,
    obligation_ids: BTreeSet<StableId>,
    snapshot_id: StableId,
    projection_policy_version: String,
    context_policy: ContextPolicyV1,
    context_policy_hash: ContentHash,
    candidate_source_ids: BTreeSet<StableId>,
    included_sources: Vec<SourceArtifactRef>,
    normalized_included_source_ids: BTreeSet<StableId>,
    excluded_sources: Vec<ExcludedSourceRef>,
    unknowns: Vec<EnvelopeUnknown>,
    assumptions: Vec<String>,
    losses: Vec<EnvelopeLoss>,
    projection_hash: ContentHash,
}
impl ReviewContextEnvelope {
    /// Returns the observable retained heap charge of this envelope's owned
    /// vectors, sets, strings, and identifier backing strings.
    ///
    /// This is allocation accounting for bounded D2 construction, not an RSS
    /// or allocator-internal node-overhead claim.
    #[must_use]
    pub fn allocated_bytes(&self) -> usize {
        fn id_set_bytes(values: &BTreeSet<StableId>) -> usize {
            values
                .len()
                .saturating_mul(std::mem::size_of::<StableId>())
                .saturating_add(values.iter().map(StableId::allocated_bytes).sum::<usize>())
        }
        fn string_set_bytes(values: &BTreeSet<String>) -> usize {
            values
                .len()
                .saturating_mul(std::mem::size_of::<String>())
                .saturating_add(values.iter().map(String::capacity).sum::<usize>())
        }
        let included = self.included_sources.iter().fold(
            self.included_sources
                .capacity()
                .saturating_mul(std::mem::size_of::<SourceArtifactRef>()),
            |total, source| {
                total
                    .saturating_add(source.registration_id.allocated_bytes())
                    .saturating_add(source.artifact_id.allocated_bytes())
                    .saturating_add(source.content_hash.allocated_bytes())
                    .saturating_add(source.cas_hash.allocated_bytes())
                    .saturating_add(source.excerpt_hash.allocated_bytes())
            },
        );
        let excluded = self.excluded_sources.iter().fold(
            self.excluded_sources
                .capacity()
                .saturating_mul(std::mem::size_of::<ExcludedSourceRef>()),
            |total, source| total.saturating_add(source.artifact_id.allocated_bytes()),
        );
        let unknowns = self.unknowns.iter().fold(
            self.unknowns
                .capacity()
                .saturating_mul(std::mem::size_of::<EnvelopeUnknown>()),
            |total, unknown| {
                total
                    .saturating_add(unknown.description.capacity())
                    .saturating_add(id_set_bytes(&unknown.source_ids))
            },
        );
        let assumptions = self
            .assumptions
            .capacity()
            .saturating_mul(std::mem::size_of::<String>())
            .saturating_add(self.assumptions.iter().map(String::capacity).sum::<usize>());
        let losses = self.losses.iter().fold(
            self.losses
                .capacity()
                .saturating_mul(std::mem::size_of::<EnvelopeLoss>()),
            |total, loss| {
                total
                    .saturating_add(loss.description.capacity())
                    .saturating_add(string_set_bytes(&loss.affected_properties))
                    .saturating_add(id_set_bytes(&loss.source_ids))
            },
        );
        self.id
            .allocated_bytes()
            .saturating_add(id_set_bytes(&self.obligation_ids))
            .saturating_add(self.snapshot_id.allocated_bytes())
            .saturating_add(self.projection_policy_version.capacity())
            .saturating_add(self.context_policy_hash.allocated_bytes())
            .saturating_add(id_set_bytes(&self.candidate_source_ids))
            .saturating_add(included)
            .saturating_add(id_set_bytes(&self.normalized_included_source_ids))
            .saturating_add(excluded)
            .saturating_add(unknowns)
            .saturating_add(assumptions)
            .saturating_add(losses)
            .saturating_add(self.projection_hash.allocated_bytes())
    }

    pub(crate) fn from_event_bytes(input: &[u8]) -> ContextResult<Self> {
        if input.len() > MAX_BODY {
            return Err(
                incomplete("context envelope canonical bytes", MAX_BODY, input.len()).into(),
            );
        }
        let raw: RawContextEnvelope =
            serde_json::from_slice(input).map_err(|error| DomainError::Json(error.to_string()))?;
        let envelope = Self {
            id: raw.id,
            obligation_ids: raw.obligation_ids,
            snapshot_id: raw.snapshot_id,
            projection_policy_version: raw.projection_policy_version,
            context_policy: raw.context_policy,
            context_policy_hash: raw.context_policy_hash,
            candidate_source_ids: raw.candidate_source_ids,
            included_sources: raw.included_sources,
            normalized_included_source_ids: raw.normalized_included_source_ids,
            excluded_sources: raw.excluded_sources,
            unknowns: raw.unknowns,
            assumptions: raw.assumptions,
            losses: raw.losses,
            projection_hash: raw.projection_hash,
        };
        envelope.validate_local()?;
        let expected = envelope.canonical_bytes()?;
        if expected != input {
            return Err(DomainError::Validation(format!(
                "context envelope must use exact canonical bytes (expected {}, observed {})",
                ContentHash::sha256(&expected),
                ContentHash::sha256(input)
            ))
            .into());
        }
        Ok(envelope)
    }

    /// Strict canonical metadata-only decode. This checks the exact aggregate
    /// closure and identity, but deliberately cannot mint a live admission
    /// because no source bytes are supplied.
    pub fn from_canonical_bytes(input: &[u8], aggregate: &ReviewAggregate) -> ContextResult<Self> {
        let envelope = Self::from_event_bytes(input)?;
        envelope.validate_metadata(aggregate)?;
        Ok(envelope)
    }

    fn validate_local(&self) -> ContextResult<()> {
        if self.id.kind() != "context-envelope"
            || self.snapshot_id.kind() != "snapshot"
            || self.projection_policy_version != ContextPolicyV1::VERSION
            || self.context_policy != ContextPolicyV1::baseline()
            || self.context_policy_hash != self.context_policy.hash()?
            || !self.assumptions.is_empty()
            || self.obligation_ids.len() != 1
            || self
                .obligation_ids
                .iter()
                .any(|id| id.kind() != "obligation")
            || self
                .candidate_source_ids
                .iter()
                .any(|id| id.kind() != "file")
        {
            return Err(DomainError::Validation(
                "invalid fixed context envelope local metadata".to_owned(),
            )
            .into());
        }
        let included_ids = self
            .included_sources
            .iter()
            .map(|source| source.artifact_id.clone())
            .collect::<BTreeSet<_>>();
        let excluded_ids = self
            .excluded_sources
            .iter()
            .map(|source| source.artifact_id.clone())
            .collect::<BTreeSet<_>>();
        if included_ids != self.normalized_included_source_ids
            || included_ids.len() != self.included_sources.len()
            || excluded_ids.len() != self.excluded_sources.len()
            || !included_ids.is_disjoint(&excluded_ids)
            || included_ids
                .union(&excluded_ids)
                .cloned()
                .collect::<BTreeSet<_>>()
                != self.candidate_source_ids
            || self
                .included_sources
                .windows(2)
                .any(|pair| pair[0].artifact_id >= pair[1].artifact_id)
            || self
                .excluded_sources
                .windows(2)
                .any(|pair| pair[0].artifact_id >= pair[1].artifact_id)
        {
            return Err(DomainError::Validation(
                "context source partition is not canonical and complete".to_owned(),
            )
            .into());
        }
        for source in &self.included_sources {
            if source.registration_id.kind() != "registration"
                || source.artifact_id.kind() != "file"
                || !is_sha256(&source.content_hash)
                || !is_sha256(&source.cas_hash)
                || !is_sha256(&source.excerpt_hash)
                || source.excerpt_byte_length > MAX_EXCERPT as u64
            {
                return Err(DomainError::Validation(
                    "invalid included source local metadata".to_owned(),
                )
                .into());
            }
            if let Some(range) = &source.excerpt
                && (range.start_line == 0
                    || range.end_line < range.start_line
                    || u64::from(range.end_line - range.start_line) >= MAX_LINES as u64)
            {
                return Err(DomainError::Validation(
                    "invalid included source excerpt range".to_owned(),
                )
                .into());
            }
        }
        if self.unknowns.len() > ContextPolicyV1::baseline().max_unknowns()
            || self.losses.len() > ContextPolicyV1::baseline().max_losses()
            || self.unknowns.iter().any(|unknown| {
                !matches!(
                    unknown.description.as_str(),
                    "context_unknown:unresolved_seed_reference"
                        | "context_unknown:unresolved_relation_endpoint"
                        | "context_unknown:unresolved_review_context_member"
                        | "context_unknown:unresolved_invariant_scope"
                )
            })
            || self.unknowns.windows(2).any(|pair| {
                (&pair[0].description, &pair[0].source_ids)
                    >= (&pair[1].description, &pair[1].source_ids)
            })
            || self.losses.windows(2).any(|pair| {
                (
                    &pair[0].description,
                    &pair[0].affected_properties,
                    &pair[0].source_ids,
                ) >= (
                    &pair[1].description,
                    &pair[1].affected_properties,
                    &pair[1].source_ids,
                )
            })
            || self.losses.iter().any(|loss| {
                loss.severity != Severity::Low
                    || loss.affected_properties.len() != 1
                    || loss.source_ids.is_empty()
            })
        {
            return Err(DomainError::Validation(
                "invalid canonical context unknown or loss groups".to_owned(),
            )
            .into());
        }
        let obligation_id = self.obligation_ids.first().expect("length checked");
        let body = identity_bytes(
            &self.snapshot_id,
            obligation_id,
            &self.context_policy_hash,
            &self.candidate_source_ids,
            &self.included_sources,
            &self.excluded_sources,
            &self.unknowns,
            &self.losses,
        )?;
        let projection_hash = ContentHash::sha256(&body);
        if self.projection_hash != projection_hash
            || self.id != StableId::parse(format!("context-envelope:{projection_hash}"))?
        {
            return Err(
                DomainError::Validation("context identity body hash mismatch".to_owned()).into(),
            );
        }
        Ok(())
    }

    fn validate_metadata(&self, aggregate: &ReviewAggregate) -> ContextResult<()> {
        self.validate_local()?;
        if self.projection_policy_version != ContextPolicyV1::VERSION
            || self.context_policy != ContextPolicyV1::baseline()
            || self.context_policy_hash != self.context_policy.hash()?
            || self.assumptions != Vec::<String>::new()
            || self.obligation_ids.len() != 1
            || &self.snapshot_id != aggregate.program().snapshot_id()
        {
            return Err(DomainError::Validation(
                "invalid fixed context envelope metadata".to_owned(),
            )
            .into());
        }
        let obligation_id = self.obligation_ids.first().expect("length checked");
        let obligation =
            aggregate
                .obligation(obligation_id)
                .ok_or_else(|| DomainError::DanglingReference {
                    owner: "context envelope",
                    owner_id: self.id.clone(),
                    reference: obligation_id.clone(),
                })?;
        let prepared = prepare_context(aggregate, obligation_id.clone())?;
        let expected_unknowns = prepared.unknowns.clone();
        let candidates = prepared.candidates;
        let expected_candidates = candidates
            .iter()
            .map(|candidate| candidate.artifact.id.clone())
            .collect::<BTreeSet<_>>();
        if self.candidate_source_ids != expected_candidates {
            return Err(DomainError::Validation(
                "context candidate denominator mismatch".to_owned(),
            )
            .into());
        }
        let included_ids = self
            .included_sources
            .iter()
            .map(|source| source.artifact_id.clone())
            .collect::<BTreeSet<_>>();
        let excluded_ids = self
            .excluded_sources
            .iter()
            .map(|source| source.artifact_id.clone())
            .collect::<BTreeSet<_>>();
        if included_ids != self.normalized_included_source_ids
            || included_ids.len() != self.included_sources.len()
            || excluded_ids.len() != self.excluded_sources.len()
            || !included_ids.is_disjoint(&excluded_ids)
            || included_ids
                .union(&excluded_ids)
                .cloned()
                .collect::<BTreeSet<_>>()
                != self.candidate_source_ids
            || self
                .excluded_sources
                .windows(2)
                .any(|pair| pair[0].artifact_id >= pair[1].artifact_id)
        {
            return Err(DomainError::Validation(
                "context source partition is not canonical and complete".to_owned(),
            )
            .into());
        }
        if self
            .included_sources
            .windows(2)
            .any(|pair| pair[0].artifact_id >= pair[1].artifact_id)
        {
            return Err(DomainError::Validation(
                "included sources are not in exact StableId order".to_owned(),
            )
            .into());
        }
        for source in &self.included_sources {
            let candidate = candidates
                .iter()
                .find(|candidate| candidate.artifact.id == source.artifact_id)
                .expect("candidate partition checked");
            if source.registration_id != *candidate.source.registration_id()
                || source.content_hash != *candidate.source.content_hash()
                || source.cas_hash != *candidate.source.cas_hash()
                || source.excerpt_byte_length > MAX_EXCERPT as u64
                || !is_sha256(&source.excerpt_hash)
            {
                return Err(DomainError::Validation(
                    "included source metadata mismatch".to_owned(),
                )
                .into());
            }
            if let Some(range) = &source.excerpt
                && (range.start_line == 0
                    || range.end_line < range.start_line
                    || u64::from(range.end_line) > candidate.source.line_count()
                    || (range.start_line == 1
                        && u64::from(range.end_line) == candidate.source.line_count())
                    || u64::from(range.end_line - range.start_line) >= MAX_LINES as u64)
            {
                return Err(DomainError::Validation(
                    "invalid or non-normalized excerpt range".to_owned(),
                )
                .into());
            }
        }
        if self.unknowns != expected_unknowns
            || self.unknowns.len() > 64
            || self.unknowns.iter().any(|unknown| {
                !matches!(
                    unknown.description.as_str(),
                    "context_unknown:unresolved_seed_reference"
                        | "context_unknown:unresolved_relation_endpoint"
                        | "context_unknown:unresolved_review_context_member"
                        | "context_unknown:unresolved_invariant_scope"
                )
            })
        {
            return Err(DomainError::Validation(
                "context unknown groups do not match deterministic discovery".to_owned(),
            )
            .into());
        }
        let expected_losses = losses(
            &self.excluded_sources,
            &self.included_sources,
            obligation.property_id(),
        )?;
        if self.losses != expected_losses {
            return Err(DomainError::Validation(
                "context loss groups do not match the partition".to_owned(),
            )
            .into());
        }
        let body = identity_bytes(
            &self.snapshot_id,
            obligation_id,
            &self.context_policy_hash,
            &self.candidate_source_ids,
            &self.included_sources,
            &self.excluded_sources,
            &self.unknowns,
            &self.losses,
        )?;
        let projection_hash = ContentHash::sha256(&body);
        if self.projection_hash != projection_hash
            || self.id != StableId::parse(format!("context-envelope:{projection_hash}"))?
        {
            return Err(
                DomainError::Validation("context identity body hash mismatch".to_owned()).into(),
            );
        }
        Ok(())
    }

    pub(crate) fn validate_for_event(&self, aggregate: &ReviewAggregate) -> Result<()> {
        self.validate_metadata(aggregate)
            .map_err(context_domain_error)
    }

    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn projection_hash(&self) -> &ContentHash {
        &self.projection_hash
    }
    pub fn included_sources(&self) -> &[SourceArtifactRef] {
        &self.included_sources
    }
    pub fn excluded_sources(&self) -> &[ExcludedSourceRef] {
        &self.excluded_sources
    }
    pub fn candidate_source_ids(&self) -> &BTreeSet<StableId> {
        &self.candidate_source_ids
    }
    pub fn normalized_included_source_ids(&self) -> &BTreeSet<StableId> {
        &self.normalized_included_source_ids
    }
    pub fn context_policy(&self) -> &ContextPolicyV1 {
        &self.context_policy
    }
    pub fn context_policy_hash(&self) -> &ContentHash {
        &self.context_policy_hash
    }
    pub fn projection_policy_version(&self) -> &str {
        &self.projection_policy_version
    }
    pub fn obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.obligation_ids
    }
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    pub fn unknowns(&self) -> &[EnvelopeUnknown] {
        &self.unknowns
    }
    pub fn losses(&self) -> &[EnvelopeLoss] {
        &self.losses
    }
    pub fn assumptions(&self) -> &[String] {
        &self.assumptions
    }
    /// Counts the exact canonical UTF-8 bytes without allocating the output
    /// vector or any formatting scratch.
    pub fn canonical_byte_len(&self) -> ContextResult<usize> {
        envelope_byte_len(self)
    }
    pub fn canonical_bytes(&self) -> ContextResult<Vec<u8>> {
        envelope_bytes(self)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContextEnvelope {
    assumptions: Vec<String>,
    candidate_source_ids: BTreeSet<StableId>,
    context_policy: ContextPolicyV1,
    context_policy_hash: ContentHash,
    excluded_sources: Vec<ExcludedSourceRef>,
    id: StableId,
    included_sources: Vec<SourceArtifactRef>,
    losses: Vec<EnvelopeLoss>,
    normalized_included_source_ids: BTreeSet<StableId>,
    obligation_ids: BTreeSet<StableId>,
    projection_hash: ContentHash,
    projection_policy_version: String,
    snapshot_id: StableId,
    unknowns: Vec<EnvelopeUnknown>,
}

impl Serialize for ReviewContextEnvelope {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ReviewContextEnvelope", 14)?;
        state.serialize_field("assumptions", &self.assumptions)?;
        state.serialize_field("candidate_source_ids", &self.candidate_source_ids)?;
        state.serialize_field("context_policy", &self.context_policy)?;
        state.serialize_field("context_policy_hash", &self.context_policy_hash)?;
        state.serialize_field("excluded_sources", &self.excluded_sources)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("included_sources", &self.included_sources)?;
        state.serialize_field("losses", &self.losses)?;
        state.serialize_field(
            "normalized_included_source_ids",
            &self.normalized_included_source_ids,
        )?;
        state.serialize_field("obligation_ids", &self.obligation_ids)?;
        state.serialize_field("projection_hash", &self.projection_hash)?;
        state.serialize_field("projection_policy_version", &self.projection_policy_version)?;
        state.serialize_field("snapshot_id", &self.snapshot_id)?;
        state.serialize_field("unknowns", &self.unknowns)?;
        state.end()
    }
}

/// Opaque byte-reverified admission. It has no serializer or public constructor.
#[allow(dead_code)] // fields are consumed by event.rs in the next implementation unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContextProjectionAdmission {
    pub(crate) manifest_digest: ContentHash,
    pub(crate) envelope_id: StableId,
    pub(crate) projection_hash: ContentHash,
    pub(crate) sources: Vec<AdmittedSource>,
}

#[allow(dead_code)] // consumed by the event admission seam in the next unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AdmittedSource {
    pub(crate) registration_id: StableId,
    pub(crate) excerpt: Option<ExcerptRange>,
    pub(crate) excerpt_byte_length: u64,
    pub(crate) excerpt_hash: ContentHash,
}

impl ContextProjectionAdmission {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.manifest_digest.allocated_bytes()
            + self.envelope_id.allocated_bytes()
            + self.projection_hash.allocated_bytes()
            + self.sources.capacity() * std::mem::size_of::<AdmittedSource>()
            + self
                .sources
                .iter()
                .map(|source| {
                    source.registration_id.allocated_bytes() + source.excerpt_hash.allocated_bytes()
                })
                .sum::<usize>()
    }

    pub(crate) fn matches_projection(&self, envelope: &ReviewContextEnvelope) -> bool {
        let expected_sources = envelope
            .included_sources
            .iter()
            .map(|source| AdmittedSource {
                registration_id: source.registration_id.clone(),
                excerpt: source.excerpt.clone(),
                excerpt_byte_length: source.excerpt_byte_length,
                excerpt_hash: source.excerpt_hash.clone(),
            })
            .collect::<Vec<_>>();
        self.envelope_id == envelope.id
            && self.projection_hash == envelope.projection_hash
            && self.sources == expected_sources
    }

    pub(crate) fn matches(
        &self,
        envelope: &ReviewContextEnvelope,
        aggregate: &ReviewAggregate,
    ) -> Result<bool> {
        envelope
            .validate_metadata(aggregate)
            .map_err(context_domain_error)?;
        let obligation_id = envelope.obligation_ids.first().ok_or_else(|| {
            DomainError::Validation("context envelope has no obligation".to_owned())
        })?;
        let prepared =
            prepare_context(aggregate, obligation_id.clone()).map_err(context_domain_error)?;
        let expected_manifest = manifest_digest(
            &prepared.snapshot_id,
            prepared.obligation.id(),
            &prepared.policy_hash,
            &prepared.candidates,
        )
        .map_err(context_domain_error)?;
        Ok(self.manifest_digest == expected_manifest && self.matches_projection(envelope))
    }
}
/// A successful builder result, deliberately coupled to its private admission.
#[derive(Debug)]
pub struct BuiltContextProjection {
    envelope: ReviewContextEnvelope,
    #[allow(dead_code)] // deliberately opaque; event.rs owns its eventual consumption.
    admission: ContextProjectionAdmission,
}
impl BuiltContextProjection {
    pub fn envelope(&self) -> &ReviewContextEnvelope {
        &self.envelope
    }

    #[allow(dead_code)] // consumed by event.rs in the next implementation unit.
    pub(crate) fn into_parts(self) -> (ReviewContextEnvelope, ContextProjectionAdmission) {
        (self.envelope, self.admission)
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    artifact: Artifact,
    source: SnapshotSourceRecordEntry,
    registration_size: u64,
    rank: (u8, usize, usize, StableId),
    exclusion: Option<ExclusionReason>,
    anchors: Vec<(u64, u64, StableId)>,
}

/// A one-use request bound to one prepared session and its exact rank.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextSourceRequest {
    artifact_id: StableId,
    registration_id: StableId,
    content_hash: ContentHash,
    cas_hash: ContentHash,
    expected_length: u64,
    line_count: u64,
    ordinal: usize,
    digest: ContentHash,
}
impl ContextSourceRequest {
    pub fn artifact_id(&self) -> &StableId {
        &self.artifact_id
    }
    pub fn cas_hash(&self) -> &ContentHash {
        &self.cas_hash
    }
    pub fn registration_id(&self) -> &StableId {
        &self.registration_id
    }
    pub fn content_hash(&self) -> &ContentHash {
        &self.content_hash
    }
    pub fn expected_length(&self) -> u64 {
        self.expected_length
    }
    pub const fn line_count(&self) -> u64 {
        self.line_count
    }
}

/// Ordered two-stage context construction state.
#[derive(Debug)]
pub struct ContextBuildSession {
    snapshot_id: StableId,
    obligation: Obligation,
    policy_hash: ContentHash,
    candidates: Vec<Candidate>,
    unknowns: Vec<EnvelopeUnknown>,
    index: usize,
    pending: Option<ContextSourceRequest>,
    included: Vec<SourceArtifactRef>,
    excluded: BTreeMap<StableId, ExclusionReason>,
    resolved_bytes: u64,
    excerpt_bytes: usize,
    session_digest: ContentHash,
}

/// Validates aggregate metadata then prepares a resolver-driven session.
pub fn prepare_context(
    aggregate: &ReviewAggregate,
    obligation_id: StableId,
) -> ContextResult<ContextBuildSession> {
    let obligation = aggregate
        .obligation(&obligation_id)
        .ok_or_else(|| DomainError::DanglingReference {
            owner: "context builder",
            owner_id: obligation_id.clone(),
            reference: obligation_id.clone(),
        })?
        .clone();
    let program = aggregate.program();
    let snapshot_id = program.snapshot_id().clone();
    if obligation.version().snapshot() != &snapshot_id {
        return Err(DomainError::Validation(
            "context obligation must bind aggregate snapshot".to_owned(),
        )
        .into());
    }
    text(obligation.property_id())?;
    let policy = ContextPolicyV1::baseline();
    let policy_hash = policy.hash()?;
    let sources = aggregate
        .snapshot_sources_for(&snapshot_id)
        .ok_or_else(|| {
            DomainError::Validation("missing exact snapshot source closure".to_owned())
        })?;
    let candidates = candidates(aggregate, program, sources, &snapshot_id)?;
    let (
        reached,
        structural,
        file_for,
        direct_files,
        distances,
        ranks,
        path_cap,
        test_cap,
        unknowns,
    ) = discover(program, &obligation)?;
    let mut candidates = candidates;
    for candidate in &mut candidates {
        candidate.exclusion =
            discovery_exclusion(&candidate.artifact.id, &path_cap, &test_cap, &reached);
        candidate.rank = (
            if direct_files.contains(&candidate.artifact.id) {
                0
            } else {
                1
            },
            *distances.get(&candidate.artifact.id).unwrap_or(&usize::MAX),
            *ranks.get(&candidate.artifact.id).unwrap_or(&usize::MAX),
            candidate.artifact.id.clone(),
        );
        candidate.anchors.clear();
    }
    let candidate_indexes = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| (candidate.artifact.id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    for artifact in program
        .artifacts()
        .iter()
        .filter(|artifact| structural.contains(&artifact.id))
    {
        let Some(location) = artifact.location.as_ref() else {
            continue;
        };
        let (Some(start), Some(end)) = (location.start_line, location.end_line) else {
            continue;
        };
        let owners = file_for.get(&artifact.id).ok_or_else(|| {
            DomainError::Validation(format!(
                "reached range-bearing artifact {} has no containing candidate file",
                artifact.id
            ))
        })?;
        let mut matched = false;
        for owner in owners {
            let Some(index) = candidate_indexes.get(owner).copied() else {
                continue;
            };
            if candidates[index].source.path() == location.path {
                candidates[index]
                    .anchors
                    .push((start, end, artifact.id.clone()));
                matched = true;
            }
        }
        if !matched {
            return Err(DomainError::Validation(format!(
                "reached range-bearing artifact {} has no exact-path containing candidate file",
                artifact.id
            ))
            .into());
        }
    }
    for candidate in &mut candidates {
        candidate.anchors.sort();
        candidate.anchors.dedup();
        if candidate.anchors.len() > MAX_ANCHORS {
            return Err(incomplete("context anchors", MAX_ANCHORS, candidate.anchors.len()).into());
        }
        if candidate.anchors.iter().any(|(start, end, _)| {
            *start == 0 || end < start || *end > candidate.source.line_count()
        }) {
            return Err(DomainError::Validation(
                "context anchor is invalid for registered source line count".to_owned(),
            )
            .into());
        }
    }
    candidates.sort_by(|a, b| a.rank.cmp(&b.rank));
    let session_digest = manifest_digest(&snapshot_id, obligation.id(), &policy_hash, &candidates)?;
    Ok(ContextBuildSession {
        snapshot_id,
        obligation,
        policy_hash,
        candidates,
        unknowns,
        index: 0,
        pending: None,
        included: Vec::new(),
        excluded: BTreeMap::new(),
        resolved_bytes: 0,
        excerpt_bytes: 0,
        session_digest,
    })
}

impl ContextBuildSession {
    /// Returns the next required source in exact rank order, performing only
    /// metadata-only exclusions before asking the resolver for bytes.
    pub fn next_source_request(&mut self) -> ContextResult<Option<ContextSourceRequest>> {
        if self.pending.is_some() {
            return Err(ContextError::Protocol("a source request is still pending"));
        }
        while self.index < self.candidates.len() {
            let candidate = &self.candidates[self.index];
            if let Some(reason) = candidate.exclusion {
                self.exclude(candidate.artifact.id.clone(), reason);
                self.index += 1;
                continue;
            }
            if let Some(reason) = metadata_exclusion(
                self.included.len(),
                candidate.registration_size,
                self.resolved_bytes,
            )? {
                self.exclude(candidate.artifact.id.clone(), reason);
                self.index += 1;
                continue;
            }
            let request = ContextSourceRequest {
                artifact_id: candidate.artifact.id.clone(),
                registration_id: candidate.source.registration_id().clone(),
                content_hash: candidate.source.content_hash().clone(),
                cas_hash: candidate.source.cas_hash().clone(),
                expected_length: candidate.registration_size,
                line_count: candidate.source.line_count(),
                ordinal: self.index,
                digest: self.session_digest.clone(),
            };
            self.pending = Some(request.clone());
            return Ok(Some(request));
        }
        Ok(None)
    }
    pub fn submit_source(
        &mut self,
        request: &ContextSourceRequest,
        bytes: &[u8],
    ) -> ContextResult<()> {
        let Some(expected) = self.pending.as_ref() else {
            return Err(ContextError::Protocol("no source request is pending"));
        };
        if expected != request {
            return Err(ContextError::Protocol(
                "stale, replayed, or out-of-order source request",
            ));
        }
        let candidate = &self.candidates[self.index];
        let byte_length = u64::try_from(bytes.len())
            .map_err(|_| incomplete("resolved artifact byte length", usize::MAX, bytes.len()))?;
        let actual_hash = ContentHash::sha256(bytes);
        let actual_line_count = bytes
            .iter()
            .try_fold(1_u64, |count, byte| {
                if *byte == b'\n' {
                    count.checked_add(1)
                } else {
                    Some(count)
                }
            })
            .ok_or_else(|| incomplete("resolved artifact line count", usize::MAX, usize::MAX))?;
        if byte_length != expected.expected_length
            || actual_hash != expected.content_hash
            || actual_hash != expected.cas_hash
            || actual_line_count != expected.line_count
        {
            return Err(DomainError::Validation(
                "resolved source bytes do not match registered metadata".to_owned(),
            )
            .into());
        }
        let next_resolved_bytes =
            self.resolved_bytes
                .checked_add(byte_length)
                .ok_or_else(|| {
                    incomplete("context resolved bytes", MAX_RESOLVED as usize, usize::MAX)
                })?;
        enum Commit {
            Include(SourceArtifactRef, usize),
            Exclude(ExclusionReason),
        }
        let commit = match excerpt(bytes, &candidate.anchors, self.excerpt_bytes)? {
            Excerpt::Include(range, data) => Commit::Include(
                SourceArtifactRef {
                    registration_id: candidate.source.registration_id().clone(),
                    artifact_id: candidate.artifact.id.clone(),
                    content_hash: candidate.source.content_hash().clone(),
                    cas_hash: candidate.source.cas_hash().clone(),
                    excerpt: range,
                    excerpt_byte_length: data.len() as u64,
                    excerpt_hash: ContentHash::sha256(data),
                },
                data.len(),
            ),
            Excerpt::Exclude(reason) => Commit::Exclude(reason),
        };
        let artifact_id = candidate.artifact.id.clone();
        let next_excerpt_bytes = match &commit {
            Commit::Include(_, delta) => {
                self.excerpt_bytes.checked_add(*delta).ok_or_else(|| {
                    incomplete("total context excerpt bytes", MAX_TOTAL_EXCERPT, usize::MAX)
                })?
            }
            Commit::Exclude(_) => self.excerpt_bytes,
        };

        // Commit only after every validation, excerpt calculation, hash, and
        // prospective counter operation has succeeded.
        self.pending = None;
        self.resolved_bytes = next_resolved_bytes;
        self.excerpt_bytes = next_excerpt_bytes;
        match commit {
            Commit::Include(source, _) => self.included.push(source),
            Commit::Exclude(reason) => self.exclude(artifact_id, reason),
        }
        self.index += 1;
        Ok(())
    }
    pub fn finish(self) -> ContextResult<BuiltContextProjection> {
        if self.pending.is_some() || self.index != self.candidates.len() {
            return Err(ContextError::Protocol(
                "all requested and metadata-only candidates must be processed before finish",
            ));
        }
        let mut included_sources = self.included;
        included_sources.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
        let included_ids = included_sources
            .iter()
            .map(|s| s.artifact_id.clone())
            .collect::<BTreeSet<_>>();
        let candidates = self
            .candidates
            .iter()
            .map(|c| c.artifact.id.clone())
            .collect::<BTreeSet<_>>();
        if included_ids.len() + self.excluded.len() != candidates.len()
            || !included_ids.is_disjoint(&self.excluded.keys().cloned().collect())
        {
            return Err(DomainError::Validation(
                "context candidate partition is incomplete".to_owned(),
            )
            .into());
        }
        let excluded_sources = self
            .excluded
            .into_iter()
            .map(|(artifact_id, reason)| ExcludedSourceRef {
                artifact_id,
                reason,
            })
            .collect::<Vec<_>>();
        let losses = losses(
            &excluded_sources,
            &included_sources,
            self.obligation.property_id(),
        )?;
        let body = identity_bytes(
            &self.snapshot_id,
            self.obligation.id(),
            &self.policy_hash,
            &candidates,
            &included_sources,
            &excluded_sources,
            &self.unknowns,
            &losses,
        )?;
        let projection_hash = ContentHash::sha256(&body);
        let id = StableId::parse(format!("context-envelope:{projection_hash}"))?;
        let envelope = ReviewContextEnvelope {
            id,
            obligation_ids: BTreeSet::from([self.obligation.id().clone()]),
            snapshot_id: self.snapshot_id,
            projection_policy_version: ContextPolicyV1::VERSION.to_owned(),
            context_policy: ContextPolicyV1::baseline(),
            context_policy_hash: self.policy_hash,
            candidate_source_ids: candidates,
            normalized_included_source_ids: included_ids,
            included_sources,
            excluded_sources,
            unknowns: self.unknowns,
            assumptions: Vec::new(),
            losses,
            projection_hash: projection_hash.clone(),
        };
        envelope.canonical_bytes()?;
        let admitted_sources = envelope
            .included_sources
            .iter()
            .map(|source| AdmittedSource {
                registration_id: source.registration_id.clone(),
                excerpt: source.excerpt.clone(),
                excerpt_byte_length: source.excerpt_byte_length,
                excerpt_hash: source.excerpt_hash.clone(),
            })
            .collect();
        let admission = ContextProjectionAdmission {
            manifest_digest: self.session_digest,
            envelope_id: envelope.id.clone(),
            projection_hash,
            sources: admitted_sources,
        };
        Ok(BuiltContextProjection {
            envelope,
            admission,
        })
    }
    fn exclude(&mut self, id: StableId, reason: ExclusionReason) {
        self.excluded.entry(id).or_insert(reason);
    }
}

fn discovery_exclusion(
    id: &StableId,
    path_cap: &BTreeSet<StableId>,
    test_cap: &BTreeSet<StableId>,
    reached: &BTreeSet<StableId>,
) -> Option<ExclusionReason> {
    if path_cap.contains(id) {
        Some(ExclusionReason::PathCap)
    } else if test_cap.contains(id) {
        Some(ExclusionReason::TestCap)
    } else if !reached.contains(id) {
        Some(ExclusionReason::NotReached)
    } else {
        None
    }
}

fn metadata_exclusion(
    included_files: usize,
    artifact_bytes: u64,
    resolved_bytes: u64,
) -> ContextResult<Option<ExclusionReason>> {
    if included_files >= MAX_FILES {
        return Ok(Some(ExclusionReason::IncludedFileCap));
    }
    if artifact_bytes > MAX_FILE_BYTES {
        return Ok(Some(ExclusionReason::ArtifactBytesCap));
    }
    let next = resolved_bytes
        .checked_add(artifact_bytes)
        .ok_or_else(|| incomplete("context resolved bytes", MAX_RESOLVED as usize, usize::MAX))?;
    if next > MAX_RESOLVED {
        return Ok(Some(ExclusionReason::TotalResolvedBytesCap));
    }
    Ok(None)
}

fn candidates(
    aggregate: &ReviewAggregate,
    program: &ProgramSpace,
    sources: &crate::SnapshotSourcesRecorded,
    snapshot: &StableId,
) -> ContextResult<Vec<Candidate>> {
    let mut files = Vec::new();
    for artifact in program.artifacts().iter().filter(|a| a.kind == "file") {
        if files.len() == MAX {
            return Err(incomplete("context candidate files", MAX, MAX + 1).into());
        }
        files.push(artifact);
    }
    if sources.entries().len() != files.len() {
        return Err(DomainError::Validation(
            "snapshot source closure must exactly cover file candidates".to_owned(),
        )
        .into());
    }
    let mut out = Vec::new();
    for artifact in files {
        let source = sources
            .entries()
            .iter()
            .find(|entry| entry.artifact_id() == &artifact.id)
            .ok_or_else(|| {
                DomainError::Validation("missing source entry for candidate".to_owned())
            })?;
        let registration = aggregate
            .artifact_registration(source.registration_id())
            .ok_or_else(|| {
                DomainError::Validation("missing candidate artifact registration".to_owned())
            })?;
        if source.path() != artifact.location.as_ref().map_or("", |l| l.path.as_str())
            || artifact.content_hash.as_ref() != Some(source.content_hash())
            || registration.cas_hash() != source.cas_hash()
            || registration.sensitivity() != ArtifactSensitivity::WorkspaceSource
            || !registration.is_snapshot_ingest(snapshot)
        {
            return Err(DomainError::Validation(
                "candidate metadata closure does not match accepted snapshot artifact".to_owned(),
            )
            .into());
        }
        out.push(Candidate {
            artifact: (*artifact).clone(),
            source: source.clone(),
            registration_size: registration.size(),
            rank: (1, usize::MAX, usize::MAX, artifact.id.clone()),
            exclusion: None,
            anchors: Vec::new(),
        });
    }
    Ok(out)
}

// Returns reached candidate files and deterministic path-derived annotations.
#[allow(clippy::type_complexity)]
fn discover(
    program: &ProgramSpace,
    obligation: &Obligation,
) -> ContextResult<(
    BTreeSet<StableId>,
    BTreeSet<StableId>,
    BTreeMap<StableId, BTreeSet<StableId>>,
    BTreeSet<StableId>,
    BTreeMap<StableId, usize>,
    BTreeMap<StableId, usize>,
    BTreeSet<StableId>,
    BTreeSet<StableId>,
    Vec<EnvelopeUnknown>,
)> {
    if program.relations().len() > MAX_RELATIONS {
        return Err(incomplete(
            "context relation scan",
            MAX_RELATIONS,
            program.relations().len(),
        )
        .into());
    }
    let mut all = obligation.source_ids().to_vec();
    all.extend(obligation.target_refs().iter().cloned());
    all.extend(obligation.context_ids().iter().cloned());
    all.sort();
    all.dedup();
    let mut structural = BTreeSet::new();
    let mut unknown = BTreeMap::<&str, BTreeSet<StableId>>::new();
    let contexts = program
        .contexts()
        .iter()
        .map(|c| (&c.id, c))
        .collect::<BTreeMap<_, _>>();
    for seed in all {
        expand_seed(program, &contexts, &seed, &mut structural, &mut unknown)?;
    }
    if structural.len() > MAX {
        return Err(incomplete("context discovered structural IDs", MAX, structural.len()).into());
    }
    let seeds = structural.clone();
    let adjacency = adjacency(program)?;
    // There is no independent queue cap: the exact predecessor domain is
    // already finite at nine states per structural ID (calls 3+2,
    // contains 1+1, covers 1+1), and structural IDs are capped at MAX before
    // enqueue. A smaller ad-hoc state cap would reject legal mixed paths.
    let mut queue = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut paths = Vec::<(StableId, (usize, Vec<u8>, Vec<StableId>))>::new();
    for seed in &seeds {
        for (token, next, maximum) in adjacency.get(seed).into_iter().flatten() {
            if seeds.contains(next) {
                continue;
            }
            let remaining = initial_remaining(*token, *maximum);
            let predecessor_key = (*token, next.clone(), remaining);
            if visited.contains(&predecessor_key) {
                continue;
            }
            let state = (
                1_usize,
                vec![*token],
                vec![seed.clone(), next.clone()],
                *token,
                next.clone(),
                remaining,
            );
            if !structural.contains(next) && structural.len() == MAX {
                return Err(incomplete("context discovered structural IDs", MAX, MAX + 1).into());
            }
            structural.insert(next.clone());
            visited.insert(predecessor_key);
            queue.insert(state);
        }
    }
    while let Some(state) = queue.pop_first() {
        let (distance, sequence, nodes, incoming, node, remaining) = state;
        paths.push((node.clone(), (distance, sequence.clone(), nodes.clone())));
        for (token, next, maximum) in adjacency.get(&node).into_iter().flatten() {
            if seeds.contains(next) || nodes.contains(next) {
                continue;
            }
            let next_remaining = if *token == incoming {
                if matches!(*token, 2 | 3) {
                    remaining
                } else if remaining == 0 {
                    continue;
                } else {
                    remaining - 1
                }
            } else {
                initial_remaining(*token, *maximum)
            };
            let predecessor_key = (*token, next.clone(), next_remaining);
            if visited.contains(&predecessor_key) {
                continue;
            }
            let mut nseq = sequence.clone();
            nseq.push(*token);
            let mut nnodes = nodes.clone();
            nnodes.push(next.clone());
            let state = (
                distance + 1,
                nseq,
                nnodes,
                *token,
                next.clone(),
                next_remaining,
            );
            if !structural.contains(next) && structural.len() == MAX {
                return Err(incomplete("context discovered structural IDs", MAX, MAX + 1).into());
            }
            structural.insert(next.clone());
            visited.insert(predecessor_key);
            queue.insert(state);
        }
    }
    let mut contains = 0usize;
    let mut file_for = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    for id in &structural {
        let mut todo = vec![id.clone()];
        let mut seen = BTreeSet::new();
        while let Some(child) = todo.pop() {
            for (_, parent, _) in adjacency
                .get(&child)
                .into_iter()
                .flatten()
                .filter(|(token, _, _)| *token == 3)
            {
                contains = contains.checked_add(1).ok_or_else(|| {
                    incomplete("context contains edges", MAX_CONTAINS, usize::MAX)
                })?;
                if contains > MAX_CONTAINS {
                    return Err(incomplete("context contains edges", MAX_CONTAINS, contains).into());
                }
                if program.artifact(parent).is_some_and(|a| a.kind == "file") {
                    file_for
                        .entry(id.clone())
                        .or_default()
                        .insert(parent.clone());
                } else if seen.insert(parent.clone()) {
                    todo.push(parent.clone())
                }
            }
        }
        if program.artifact(id).is_some_and(|a| a.kind == "file") {
            file_for.entry(id.clone()).or_default().insert(id.clone());
        }
    }
    let mut candidates = BTreeSet::new();
    let mut direct = BTreeSet::new();
    let mut dist = BTreeMap::new();
    let mut rank = BTreeMap::new();
    let mut path_entries = paths;
    path_entries.sort_by(|a, b| a.1.cmp(&b.1));
    for (idx, (node, (d, _, _))) in path_entries.iter().enumerate() {
        for file in file_for.get(node).into_iter().flatten() {
            candidates.insert(file.clone());
            dist.entry(file.clone()).or_insert(*d);
            rank.entry(file.clone()).or_insert(idx);
        }
    }
    for seed in &seeds {
        for file in file_for.get(seed).into_iter().flatten() {
            candidates.insert(file.clone());
            if program.artifact(seed).is_some_and(|a| a.kind == "file") {
                direct.insert(file.clone());
            }
            dist.entry(file.clone()).or_insert(0);
            rank.entry(file.clone()).or_insert(0);
        }
    }
    let selected_files = path_entries
        .iter()
        .take(MAX_PATHS)
        .flat_map(|(node, _)| file_for.get(node).into_iter().flatten().cloned())
        .collect::<BTreeSet<_>>();
    let unselected_files = path_entries
        .iter()
        .skip(MAX_PATHS)
        .flat_map(|(node, _)| file_for.get(node).into_iter().flatten().cloned())
        .collect::<BTreeSet<_>>();
    let path_cap = unselected_files
        .difference(&selected_files)
        .filter(|file| !direct.contains(*file))
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut tests = path_entries
        .iter()
        .filter(|(id, _)| program.artifact(id).is_some_and(|a| a.kind == "test"))
        .collect::<Vec<_>>();
    tests.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    let selected_test_files = tests
        .iter()
        .take(MAX_TESTS)
        .flat_map(|(id, _)| file_for.get(id).into_iter().flatten().cloned())
        .collect::<BTreeSet<_>>();
    let unselected_test_files = tests
        .iter()
        .skip(MAX_TESTS)
        .flat_map(|(id, _)| file_for.get(id).into_iter().flatten().cloned())
        .collect::<BTreeSet<_>>();
    let test_cap = unselected_test_files
        .difference(&selected_test_files)
        .filter(|file| !direct.contains(*file))
        .cloned()
        .collect::<BTreeSet<_>>();
    let unknowns = unknown
        .into_iter()
        .map(|(description, source_ids)| EnvelopeUnknown {
            description: description.to_owned(),
            source_ids,
        })
        .collect();
    Ok((
        candidates, structural, file_for, direct, dist, rank, path_cap, test_cap, unknowns,
    ))
}

#[allow(clippy::too_many_arguments)]
fn expand_seed(
    program: &ProgramSpace,
    contexts: &BTreeMap<&StableId, &crate::ReviewContext>,
    id: &StableId,
    structural: &mut BTreeSet<StableId>,
    unknown: &mut BTreeMap<&'static str, BTreeSet<StableId>>,
) -> ContextResult<()> {
    if program.artifact(id).is_some() {
        return add_seed_reference(
            program,
            contexts,
            id,
            "context_unknown:unresolved_seed_reference",
            structural,
            unknown,
        );
    }
    if let Some(relation) = program.relation(id) {
        for endpoint in std::iter::once(&relation.source_id).chain(relation.target_ids.iter()) {
            add_seed_reference(
                program,
                contexts,
                endpoint,
                "context_unknown:unresolved_relation_endpoint",
                structural,
                unknown,
            )?;
        }
    } else if let Some(context) = contexts.get(id) {
        for member in &context.member_ids {
            add_seed_reference(
                program,
                contexts,
                member,
                "context_unknown:unresolved_review_context_member",
                structural,
                unknown,
            )?;
        }
    } else if let Some(invariant) = program.invariant(id) {
        for scope in &invariant.scope_ids {
            add_seed_reference(
                program,
                contexts,
                scope,
                "context_unknown:unresolved_invariant_scope",
                structural,
                unknown,
            )?;
        }
    } else {
        unknown
            .entry("context_unknown:unresolved_seed_reference")
            .or_default()
            .insert(id.clone());
    }
    Ok(())
}

fn add_seed_reference(
    program: &ProgramSpace,
    contexts: &BTreeMap<&StableId, &crate::ReviewContext>,
    id: &StableId,
    unresolved: &'static str,
    structural: &mut BTreeSet<StableId>,
    unknown: &mut BTreeMap<&'static str, BTreeSet<StableId>>,
) -> ContextResult<()> {
    let accepted = program.artifact(id).is_some()
        || program.relation(id).is_some()
        || contexts.contains_key(id)
        || program.invariant(id).is_some();
    if accepted {
        if !structural.contains(id) && structural.len() == MAX {
            return Err(incomplete("context discovered structural IDs", MAX, MAX + 1).into());
        }
        structural.insert(id.clone());
    } else {
        unknown.entry(unresolved).or_default().insert(id.clone());
    }
    Ok(())
}

#[allow(clippy::type_complexity)]
fn adjacency(
    program: &ProgramSpace,
) -> ContextResult<BTreeMap<StableId, Vec<(u8, StableId, usize)>>> {
    let mut result = BTreeMap::<StableId, Vec<(u8, StableId, usize)>>::new();
    let mut scanned = 0_usize;
    for relation in program.relations() {
        scanned = scanned
            .checked_add(1)
            .ok_or_else(|| incomplete("context relation scan", MAX_RELATIONS, usize::MAX))?;
        if scanned > MAX_RELATIONS {
            return Err(incomplete("context relation scan", MAX_RELATIONS, scanned).into());
        }
        if !relation.directed {
            continue;
        }
        let (forward, reverse, depth) = match relation.kind.as_str() {
            "calls" => (0, 1, 3),
            "contains" => (2, 3, usize::MAX),
            "covers" => (4, 5, 1),
            _ => continue,
        };
        for target in &relation.target_ids {
            result.entry(relation.source_id.clone()).or_default().push((
                forward,
                target.clone(),
                depth,
            ));
            let reverse_depth = if relation.kind == "calls" { 2 } else { depth };
            result.entry(target.clone()).or_default().push((
                reverse,
                relation.source_id.clone(),
                reverse_depth,
            ));
        }
    }
    for edges in result.values_mut() {
        edges.sort();
        edges.dedup();
    }
    Ok(result)
}

const fn initial_remaining(token: u8, maximum: usize) -> usize {
    if matches!(token, 2 | 3) {
        usize::MAX
    } else {
        maximum.saturating_sub(1)
    }
}

enum Excerpt<'a> {
    Include(Option<ExcerptRange>, &'a [u8]),
    Exclude(ExclusionReason),
}
fn excerpt<'a>(
    bytes: &'a [u8],
    anchors: &[(u64, u64, StableId)],
    aggregate: usize,
) -> ContextResult<Excerpt<'a>> {
    let starts = lines(bytes);
    let line_count = starts.len();
    let mut valid_anchors = Vec::new();
    for (s, e, owner) in anchors {
        if *s > line_count as u64 || *e > line_count as u64 {
            return Err(DomainError::Validation(
                "context anchor outside source line range".to_owned(),
            )
            .into());
        }
        valid_anchors.push((*s, *e, owner.clone()));
    }
    valid_anchors.sort();
    valid_anchors.dedup();
    if valid_anchors.len() > MAX_ANCHORS {
        return Err(incomplete("context anchors", MAX_ANCHORS, valid_anchors.len()).into());
    };
    let (start, end) = if let Some(first) = valid_anchors.first() {
        (
            first.0,
            valid_anchors
                .iter()
                .fold(first.1, |maximum, (_, end, _)| maximum.max(*end)),
        )
    } else {
        (1, line_count as u64)
    };
    let fit = |s: u64, e: u64| {
        let part = slice_lines(bytes, &starts, s, e);
        e - s < MAX_LINES as u64 && part.len() <= MAX_EXCERPT
    };
    let (s, e) = if fit(start, end) {
        (start, end)
    } else {
        let s = start;
        let mut e = s;
        while e <= line_count as u64
            && e - s < MAX_LINES as u64
            && slice_lines(bytes, &starts, s, e).len() <= MAX_EXCERPT
        {
            e += 1
        }
        if e == s {
            return Ok(Excerpt::Exclude(ExclusionReason::GiantLine));
        }
        (s, e - 1)
    };
    let data = slice_lines(bytes, &starts, s, e);
    if data.len() > MAX_EXCERPT {
        return Ok(Excerpt::Exclude(ExclusionReason::ExcerptBytesCap));
    }
    if aggregate
        .checked_add(data.len())
        .ok_or_else(|| incomplete("total context excerpt bytes", MAX_TOTAL_EXCERPT, usize::MAX))?
        > MAX_TOTAL_EXCERPT
    {
        return Ok(Excerpt::Exclude(ExclusionReason::TotalExcerptBytesCap));
    }
    let range = if s == 1 && e == line_count as u64 {
        None
    } else {
        Some(ExcerptRange {
            start_line: u32::try_from(s)
                .map_err(|_| incomplete("excerpt line", u32::MAX as usize, usize::MAX))?,
            end_line: u32::try_from(e)
                .map_err(|_| incomplete("excerpt line", u32::MAX as usize, usize::MAX))?,
        })
    };
    Ok(Excerpt::Include(range, data))
}
fn lines(bytes: &[u8]) -> Vec<usize> {
    let mut s = vec![0];
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            s.push(i + 1)
        }
    }
    s
}
fn slice_lines<'a>(b: &'a [u8], s: &[usize], start: u64, end: u64) -> &'a [u8] {
    let a = s[start as usize - 1];
    let z = if end as usize == s.len() {
        b.len()
    } else {
        s[end as usize]
    };
    &b[a..z]
}

fn losses(
    excluded: &[ExcludedSourceRef],
    included: &[SourceArtifactRef],
    property: &str,
) -> ContextResult<Vec<EnvelopeLoss>> {
    let mut groups = BTreeMap::<String, BTreeSet<StableId>>::new();
    for e in excluded {
        groups
            .entry(e.reason.loss_description().to_owned())
            .or_default()
            .insert(e.artifact_id.clone());
    }
    for s in included {
        if s.excerpt.is_some() {
            groups
                .entry("context_loss:excerpt_window_truncated".to_owned())
                .or_default()
                .insert(s.artifact_id.clone());
        }
    }
    if groups.len() > 64 {
        return Err(incomplete("context losses", 64, groups.len()).into());
    }
    let props = BTreeSet::from([property.to_owned()]);
    Ok(groups
        .into_iter()
        .map(|(description, source_ids)| EnvelopeLoss {
            description,
            severity: Severity::Low,
            affected_properties: props.clone(),
            source_ids,
        })
        .collect())
}
// Every argument is identity-bearing; grouping them would obscure the exact
// complete-preimage rule at the only identity construction boundary.
#[allow(clippy::too_many_arguments)]
fn identity_bytes(
    snapshot: &StableId,
    obligation: &StableId,
    policy: &ContentHash,
    candidates: &BTreeSet<StableId>,
    included: &[SourceArtifactRef],
    excluded: &[ExcludedSourceRef],
    unknowns: &[EnvelopeUnknown],
    losses: &[EnvelopeLoss],
) -> ContextResult<Vec<u8>> {
    let mut out = BoundedJson::new("context envelope identity body");
    out.push(b"{\"assumptions\":[],\"candidate_source_ids\":")?;
    out.ids(candidates.iter())?;
    out.push(b",\"context_policy\":")?;
    out.push(&ContextPolicyV1::baseline().canonical_bytes()?)?;
    out.push(b",\"context_policy_hash\":")?;
    out.text(&policy.to_string())?;
    out.push(b",\"excluded_sources\":")?;
    out.excluded(excluded)?;
    out.push(b",\"included_sources\":")?;
    out.included(included)?;
    out.push(b",\"losses\":")?;
    out.losses(losses)?;
    out.push(b",\"obligation_ids\":[")?;
    out.text(&obligation.to_string())?;
    out.push(b"],\"snapshot_id\":")?;
    out.text(&snapshot.to_string())?;
    out.push(b",\"unknowns\":")?;
    out.unknowns(unknowns)?;
    out.push(b"}")?;
    out.finish()
}

const CONTEXT_POLICY_CANONICAL_BYTE_LEN: usize = 2_451;

fn write_envelope(envelope: &ReviewContextEnvelope, out: &mut BoundedJson) -> ContextResult<()> {
    out.push(b"{\"assumptions\":[],\"candidate_source_ids\":")?;
    out.ids(envelope.candidate_source_ids.iter())?;
    out.push(b",\"context_policy\":")?;
    if out.count_only {
        out.advance(CONTEXT_POLICY_CANONICAL_BYTE_LEN)?;
    } else {
        out.push(&envelope.context_policy.canonical_bytes()?)?;
    }
    out.push(b",\"context_policy_hash\":")?;
    out.text(envelope.context_policy_hash.as_str())?;
    out.push(b",\"excluded_sources\":")?;
    out.excluded(&envelope.excluded_sources)?;
    out.push(b",\"id\":")?;
    out.text(envelope.id.as_str())?;
    out.push(b",\"included_sources\":")?;
    out.included(&envelope.included_sources)?;
    out.push(b",\"losses\":")?;
    out.losses(&envelope.losses)?;
    out.push(b",\"normalized_included_source_ids\":")?;
    out.ids(envelope.normalized_included_source_ids.iter())?;
    out.push(b",\"obligation_ids\":")?;
    out.ids(envelope.obligation_ids.iter())?;
    out.push(b",\"projection_hash\":")?;
    out.text(envelope.projection_hash.as_str())?;
    out.push(b",\"projection_policy_version\":")?;
    out.text(&envelope.projection_policy_version)?;
    out.push(b",\"snapshot_id\":")?;
    out.text(envelope.snapshot_id.as_str())?;
    out.push(b",\"unknowns\":")?;
    out.unknowns(&envelope.unknowns)?;
    out.push(b"}")
}

fn envelope_byte_len(envelope: &ReviewContextEnvelope) -> ContextResult<usize> {
    let mut out = BoundedJson::counting("context envelope canonical bytes");
    write_envelope(envelope, &mut out)?;
    Ok(out.len)
}

fn envelope_bytes(envelope: &ReviewContextEnvelope) -> ContextResult<Vec<u8>> {
    let mut out = BoundedJson::new("context envelope canonical bytes");
    write_envelope(envelope, &mut out)?;
    out.finish()
}

struct BoundedJson {
    bytes: Vec<u8>,
    len: usize,
    count_only: bool,
    operation: &'static str,
}

impl BoundedJson {
    fn new(operation: &'static str) -> Self {
        Self {
            bytes: Vec::new(),
            len: 0,
            count_only: false,
            operation,
        }
    }
    fn counting(operation: &'static str) -> Self {
        Self {
            bytes: Vec::new(),
            len: 0,
            count_only: true,
            operation,
        }
    }
    fn advance(&mut self, length: usize) -> ContextResult<()> {
        let next = self
            .len
            .checked_add(length)
            .ok_or_else(|| incomplete(self.operation, MAX_BODY, usize::MAX))?;
        if next > MAX_BODY {
            return Err(incomplete(self.operation, MAX_BODY, next).into());
        }
        self.len = next;
        Ok(())
    }
    fn push(&mut self, value: &[u8]) -> ContextResult<()> {
        self.advance(value.len())?;
        if self.count_only {
            return Ok(());
        }
        self.bytes
            .try_reserve_exact(value.len())
            .map_err(|_| incomplete(self.operation, MAX_BODY, self.len))?;
        self.bytes.extend_from_slice(value);
        Ok(())
    }
    fn text(&mut self, value: &str) -> ContextResult<()> {
        text(value)?;
        self.push(b"\"")?;
        for character in value.chars() {
            match character {
                '"' => self.push(b"\\\"")?,
                '\\' => self.push(b"\\\\")?,
                '\u{08}' => self.push(b"\\b")?,
                '\t' => self.push(b"\\t")?,
                '\n' => self.push(b"\\n")?,
                '\u{0c}' => self.push(b"\\f")?,
                '\r' => self.push(b"\\r")?,
                control if control <= '\u{1f}' => {
                    let escape = format!("\\u{:04x}", u32::from(control));
                    self.push(escape.as_bytes())?;
                }
                scalar => {
                    let mut encoded = [0_u8; 4];
                    self.push(scalar.encode_utf8(&mut encoded).as_bytes())?;
                }
            }
        }
        self.push(b"\"")
    }
    fn number(&mut self, value: u64) -> ContextResult<()> {
        let mut digits = [0_u8; 20];
        let mut cursor = digits.len();
        let mut remaining = value;
        loop {
            cursor -= 1;
            digits[cursor] = b'0' + u8::try_from(remaining % 10).expect("decimal digit");
            remaining /= 10;
            if remaining == 0 {
                break;
            }
        }
        self.push(&digits[cursor..])
    }
    fn ids<'a>(&mut self, values: impl Iterator<Item = &'a StableId>) -> ContextResult<()> {
        self.push(b"[")?;
        for (index, id) in values.enumerate() {
            if index != 0 {
                self.push(b",")?;
            }
            self.text(id.as_str())?;
        }
        self.push(b"]")
    }
    fn excerpt(&mut self, range: Option<&ExcerptRange>) -> ContextResult<()> {
        let Some(range) = range else {
            return self.push(b"null");
        };
        self.push(b"{\"end_line\":")?;
        self.number(u64::from(range.end_line))?;
        self.push(b",\"start_line\":")?;
        self.number(u64::from(range.start_line))?;
        self.push(b"}")
    }
    fn included(&mut self, values: &[SourceArtifactRef]) -> ContextResult<()> {
        self.push(b"[")?;
        for (index, source) in values.iter().enumerate() {
            if index != 0 {
                self.push(b",")?;
            }
            self.push(b"{\"artifact_id\":")?;
            self.text(source.artifact_id.as_str())?;
            self.push(b",\"cas_hash\":")?;
            self.text(source.cas_hash.as_str())?;
            self.push(b",\"content_hash\":")?;
            self.text(source.content_hash.as_str())?;
            self.push(b",\"excerpt\":")?;
            self.excerpt(source.excerpt.as_ref())?;
            self.push(b",\"excerpt_byte_length\":")?;
            self.number(source.excerpt_byte_length)?;
            self.push(b",\"excerpt_hash\":")?;
            self.text(source.excerpt_hash.as_str())?;
            self.push(b",\"registration_id\":")?;
            self.text(source.registration_id.as_str())?;
            self.push(b"}")?;
        }
        self.push(b"]")
    }
    fn excluded(&mut self, values: &[ExcludedSourceRef]) -> ContextResult<()> {
        self.push(b"[")?;
        for (index, value) in values.iter().enumerate() {
            if index != 0 {
                self.push(b",")?;
            }
            self.push(b"{\"artifact_id\":")?;
            self.text(value.artifact_id.as_str())?;
            self.push(b",\"reason\":")?;
            self.text(value.reason.as_str())?;
            self.push(b"}")?;
        }
        self.push(b"]")
    }
    fn unknowns(&mut self, values: &[EnvelopeUnknown]) -> ContextResult<()> {
        self.push(b"[")?;
        for (index, value) in values.iter().enumerate() {
            if index != 0 {
                self.push(b",")?;
            }
            self.push(b"{\"description\":")?;
            self.text(&value.description)?;
            self.push(b",\"source_ids\":")?;
            self.ids(value.source_ids.iter())?;
            self.push(b"}")?;
        }
        self.push(b"]")
    }
    fn losses(&mut self, values: &[EnvelopeLoss]) -> ContextResult<()> {
        self.push(b"[")?;
        for (index, value) in values.iter().enumerate() {
            if index != 0 {
                self.push(b",")?;
            }
            self.push(b"{\"affected_properties\":[")?;
            for (property_index, property) in value.affected_properties.iter().enumerate() {
                if property_index != 0 {
                    self.push(b",")?;
                }
                self.text(property)?;
            }
            self.push(b"],\"description\":")?;
            self.text(&value.description)?;
            self.push(b",\"severity\":\"low\",\"source_ids\":")?;
            self.ids(value.source_ids.iter())?;
            self.push(b"}")?;
        }
        self.push(b"]")
    }
    fn finish(self) -> ContextResult<Vec<u8>> {
        Ok(self.bytes)
    }
}

struct ManifestJson(sha2::Sha256);

impl ManifestJson {
    fn new() -> Self {
        use sha2::Digest;
        Self(sha2::Sha256::new())
    }

    fn push(&mut self, value: &[u8]) -> ContextResult<()> {
        use sha2::Digest;
        self.0.update(value);
        Ok(())
    }

    fn text(&mut self, value: &str) -> ContextResult<()> {
        text(value)?;
        self.push(b"\"")?;
        for character in value.chars() {
            match character {
                '"' => self.push(b"\\\"")?,
                '\\' => self.push(b"\\\\")?,
                '\u{08}' => self.push(b"\\b")?,
                '\t' => self.push(b"\\t")?,
                '\n' => self.push(b"\\n")?,
                '\u{0c}' => self.push(b"\\f")?,
                '\r' => self.push(b"\\r")?,
                control if control <= '\u{1f}' => {
                    let escape = format!("\\u{:04x}", u32::from(control));
                    self.push(escape.as_bytes())?;
                }
                scalar => {
                    let mut encoded = [0_u8; 4];
                    self.push(scalar.encode_utf8(&mut encoded).as_bytes())?;
                }
            }
        }
        self.push(b"\"")
    }

    fn number(&mut self, value: u64) -> ContextResult<()> {
        self.push(value.to_string().as_bytes())
    }

    fn finish(self) -> ContextResult<ContentHash> {
        use sha2::Digest;
        ContentHash::parse(format!("sha256:{:x}", self.0.finalize())).map_err(Into::into)
    }
}

fn text(s: &str) -> ContextResult<()> {
    if s.len() > MAX_TEXT {
        Err(incomplete("context string", MAX_TEXT, s.len()).into())
    } else {
        Ok(())
    }
}
fn manifest_digest(
    snapshot: &StableId,
    obligation: &StableId,
    policy: &ContentHash,
    candidates: &[Candidate],
) -> ContextResult<ContentHash> {
    let mut out = ManifestJson::new();
    out.push(b"{\"candidates\":[")?;
    for (index, candidate) in candidates.iter().enumerate() {
        if index != 0 {
            out.push(b",")?;
        }
        out.push(b"{\"anchors\":[")?;
        for (anchor_index, (start, end, owner)) in candidate.anchors.iter().enumerate() {
            if anchor_index != 0 {
                out.push(b",")?;
            }
            out.push(b"[")?;
            out.number(*start)?;
            out.push(b",")?;
            out.number(*end)?;
            out.push(b",")?;
            out.text(&owner.to_string())?;
            out.push(b"]")?;
        }
        out.push(b"],\"artifact_id\":")?;
        out.text(&candidate.artifact.id.to_string())?;
        out.push(b",\"cas_hash\":")?;
        out.text(&candidate.source.cas_hash().to_string())?;
        out.push(b",\"content_hash\":")?;
        out.text(&candidate.source.content_hash().to_string())?;
        out.push(b",\"exclusion\":")?;
        if let Some(reason) = candidate.exclusion {
            out.text(reason.as_str())?;
        } else {
            out.push(b"null")?;
        }
        out.push(b",\"line_count\":")?;
        out.number(candidate.source.line_count())?;
        out.push(b",\"path\":")?;
        out.text(candidate.source.path())?;
        out.push(b",\"rank\":[")?;
        out.number(u64::from(candidate.rank.0))?;
        out.push(b",")?;
        out.number(
            u64::try_from(candidate.rank.1)
                .map_err(|_| incomplete("context rank", usize::MAX, candidate.rank.1))?,
        )?;
        out.push(b",")?;
        out.number(
            u64::try_from(candidate.rank.2)
                .map_err(|_| incomplete("context rank", usize::MAX, candidate.rank.2))?,
        )?;
        out.push(b",")?;
        out.text(&candidate.rank.3.to_string())?;
        out.push(b"],\"registration_id\":")?;
        out.text(&candidate.source.registration_id().to_string())?;
        out.push(b",\"size\":")?;
        out.number(candidate.registration_size)?;
        out.push(b"}")?;
    }
    out.push(b"],\"obligation_id\":")?;
    out.text(&obligation.to_string())?;
    out.push(b",\"policy_hash\":")?;
    out.text(&policy.to_string())?;
    out.push(b",\"snapshot_id\":")?;
    out.text(&snapshot.to_string())?;
    out.push(b"}")?;
    out.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ArtifactSource, EventAdmissions, EventCommand, EventContractVersion, EventEnvelope,
        EventLog, EventStreamGenesis, MvpRulePack, ObligationLifecycle, OfflineProjectionState,
        PlanBudget, PlannerPolicyV1, ProgramSpace, ReviewAggregate, SnapshotSourceRecordEntry,
        SnapshotSourcesRecorded, plan,
    };
    use serde_json::{Value, json};

    fn reference_program_value() -> Value {
        serde_json::from_str(include_str!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap()
    }

    fn context_ready_program_value() -> Value {
        let mut value = reference_program_value();
        append_relation(
            &mut value,
            "relation:file-contains-payment-charge",
            "contains",
            "file:payment-repository",
            &["function:payment-charge".to_owned()],
        );
        let test = value["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["id"] == "test:double-submit")
            .unwrap();
        test["location"]["start_line"] = Value::Null;
        test["location"]["end_line"] = Value::Null;
        value
    }

    fn append_artifact(value: &mut Value, id: &str, kind: &str, path: Option<&str>) {
        let template = value["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|artifact| artifact["kind"] == kind)
            .unwrap()
            .clone();
        let mut artifact = template;
        artifact["id"] = json!(id);
        artifact["label"] = json!(id);
        if let Some(path) = path {
            artifact["location"]["path"] = json!(path);
            artifact["location"]["start_line"] = Value::Null;
            artifact["location"]["end_line"] = Value::Null;
        }
        value["artifacts"].as_array_mut().unwrap().push(artifact);
    }

    fn append_relation(value: &mut Value, id: &str, kind: &str, source: &str, targets: &[String]) {
        let mut relation = value["relations"][0].clone();
        relation["id"] = json!(id);
        relation["kind"] = json!(kind);
        relation["source_id"] = json!(source);
        relation["target_ids"] = json!(targets);
        relation["directed"] = json!(true);
        value["relations"].as_array_mut().unwrap().push(relation);
    }

    fn obligation_with_seed(seed: &str) -> Obligation {
        let program: ProgramSpace = serde_json::from_value(reference_program_value()).unwrap();
        let (_, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let mut value = serde_json::to_value(&obligations[0]).unwrap();
        value["target_refs"] = json!([seed]);
        value["normalized_target_refs"] = json!([seed]);
        value["source_ids"] = json!([seed]);
        value["normalized_source_ids"] = json!([seed]);
        value["context_ids"] = json!([]);
        value["normalized_context_ids"] = json!([]);
        serde_json::from_value(value).unwrap()
    }

    fn fixture() -> (ReviewAggregate, BTreeMap<StableId, Vec<u8>>) {
        fixture_with_source_bytes(default_source_bytes())
    }

    fn default_source_bytes() -> BTreeMap<&'static str, Vec<u8>> {
        BTreeMap::from([
            ("src/checkout_controller.rs", {
                let mut bytes = b"x\r\n".repeat(100);
                bytes.extend_from_slice(b"tail");
                bytes
            }),
            ("src/payment_repository.rs", {
                let mut bytes = vec![0xff, b'\n'];
                bytes.extend(b"x\n".repeat(100));
                bytes
            }),
        ])
    }

    fn fixture_with_source_bytes(
        source_bytes: BTreeMap<&'static str, Vec<u8>>,
    ) -> (ReviewAggregate, BTreeMap<StableId, Vec<u8>>) {
        fixture_from_program_value(context_ready_program_value(), source_bytes)
    }

    fn build_projection(
        aggregate: &ReviewAggregate,
        bytes: &BTreeMap<StableId, Vec<u8>>,
    ) -> BuiltContextProjection {
        let obligation = aggregate.obligations().next().unwrap().id().clone();
        let mut session = prepare_context(aggregate, obligation).unwrap();
        while let Some(request) = session.next_source_request().unwrap() {
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
        }
        session.finish().unwrap()
    }

    fn event_initial(ready: &ReviewAggregate) -> ReviewAggregate {
        ReviewAggregate::new(
            ready.program().clone(),
            ready.universe().clone(),
            ready.obligations().cloned().collect(),
        )
        .unwrap()
    }

    fn fixture_from_program_value(
        mut value: Value,
        source_bytes: BTreeMap<&'static str, Vec<u8>>,
    ) -> (ReviewAggregate, BTreeMap<StableId, Vec<u8>>) {
        for artifact in value["artifacts"].as_array_mut().unwrap() {
            if artifact["kind"] == "file" {
                let path = artifact["location"]["path"].as_str().unwrap();
                artifact["content_hash"] =
                    json!(ContentHash::sha256(&source_bytes[path]).to_string());
            }
        }
        let program: ProgramSpace = serde_json::from_value(value).unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let mut aggregate = ReviewAggregate::new(program.clone(), universe, obligations).unwrap();
        let run_id = StableId::parse("run:context-test").unwrap();
        let mut registrations = Vec::new();
        let mut entries = Vec::new();
        let mut by_id = BTreeMap::new();
        for artifact in program
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
        {
            let path = artifact.location.as_ref().unwrap().path.clone();
            let bytes = source_bytes[path.as_str()].clone();
            let hash = ContentHash::sha256(&bytes);
            let source = ArtifactSource::SnapshotIngest {
                run_id: run_id.clone(),
                snapshot_id: program.snapshot_id().clone(),
                adapter_id: "fixture@1".to_owned(),
            };
            let media_type = "application/octet-stream";
            let registration_id = StableId::derived(
                "registration",
                &BTreeMap::from([
                    ("run_id".to_owned(), Value::String(run_id.to_string())),
                    ("cas_hash".to_owned(), Value::String(hash.to_string())),
                    (
                        "media_type".to_owned(),
                        Value::String(media_type.to_owned()),
                    ),
                    (
                        "sensitivity".to_owned(),
                        Value::String("workspace_source".to_owned()),
                    ),
                    ("source".to_owned(), serde_json::to_value(&source).unwrap()),
                ]),
            )
            .unwrap();
            registrations.push(
                ArtifactRegistered::new(
                    run_id.clone(),
                    registration_id.clone(),
                    hash.clone(),
                    media_type,
                    bytes.len() as u64,
                    ArtifactSensitivity::WorkspaceSource,
                    source,
                )
                .unwrap(),
            );
            entries.push(
                SnapshotSourceRecordEntry::new(
                    artifact.id.clone(),
                    path,
                    hash.clone(),
                    registration_id,
                    hash,
                    bytes.iter().filter(|byte| **byte == b'\n').count() as u64 + 1,
                )
                .unwrap(),
            );
            by_id.insert(artifact.id.clone(), bytes);
        }
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        aggregate.install_context_metadata_for_test(
            registrations,
            SnapshotSourcesRecorded::new(program.snapshot_id().clone(), entries).unwrap(),
        );
        (aggregate, by_id)
    }

    fn fixture_with_candidate_count(count: usize) -> ReviewAggregate {
        assert!((2..=MAX).contains(&count));
        let bytes = b"x\n";
        let hash = ContentHash::sha256(bytes);
        let mut value = context_ready_program_value();
        for index in 2..count {
            append_artifact(
                &mut value,
                &format!("file:manifest-{index:04}"),
                "file",
                Some(&format!("synthetic/manifest-{index:04}.rs")),
            );
        }
        for artifact in value["artifacts"].as_array_mut().unwrap() {
            if artifact["kind"] == "file" {
                artifact["content_hash"] = json!(hash.to_string());
            }
        }
        let program: ProgramSpace = serde_json::from_value(value).unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let mut aggregate = ReviewAggregate::new(program.clone(), universe, obligations).unwrap();
        let run_id = StableId::parse("run:manifest-limit-test").unwrap();
        let source = ArtifactSource::SnapshotIngest {
            run_id: run_id.clone(),
            snapshot_id: program.snapshot_id().clone(),
            adapter_id: "manifest-fixture@1".to_owned(),
        };
        let media_type = "application/octet-stream";
        let registration_id = StableId::derived(
            "registration",
            &BTreeMap::from([
                ("run_id".to_owned(), Value::String(run_id.to_string())),
                ("cas_hash".to_owned(), Value::String(hash.to_string())),
                (
                    "media_type".to_owned(),
                    Value::String(media_type.to_owned()),
                ),
                (
                    "sensitivity".to_owned(),
                    Value::String("workspace_source".to_owned()),
                ),
                ("source".to_owned(), serde_json::to_value(&source).unwrap()),
            ]),
        )
        .unwrap();
        let registration = ArtifactRegistered::new(
            run_id,
            registration_id.clone(),
            hash.clone(),
            media_type,
            bytes.len() as u64,
            ArtifactSensitivity::WorkspaceSource,
            source,
        )
        .unwrap();
        let mut entries = program
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
            .map(|artifact| {
                SnapshotSourceRecordEntry::new(
                    artifact.id.clone(),
                    artifact.location.as_ref().unwrap().path.clone(),
                    hash.clone(),
                    registration_id.clone(),
                    hash.clone(),
                    100,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        aggregate.install_context_metadata_for_test(
            vec![registration],
            SnapshotSourcesRecorded::new(program.snapshot_id().clone(), entries).unwrap(),
        );
        aggregate
    }

    #[test]
    fn policy_matches_the_adr_golden_record() {
        let context = ContextPolicyV1::baseline().canonical_bytes().unwrap();
        let decoded: ContextPolicyV1 = serde_json::from_slice(&context).unwrap();
        assert_eq!(decoded, ContextPolicyV1::baseline());
        assert_eq!(
            ContextPolicyV1::baseline().hash().unwrap().to_string(),
            "sha256:7302b62f293833478aa86ed5a82c9c7bdb3fb66f7579a23ae7a44c02060b1f6a"
        );
        let planner = PlannerPolicyV1::baseline().canonical_bytes().unwrap();
        let mut combined = b"{\"context\":".to_vec();
        combined.extend(context);
        combined.extend(b",\"planner\":");
        combined.extend(planner);
        combined.push(b'}');
        assert_eq!(combined.len(), 3_392);
        assert_eq!(
            ContentHash::sha256(&combined).to_string(),
            "sha256:5243db119b42b57d8f3e0d418e9eb202acdd43f8861f7834b9e74ef559a498fa"
        );
    }

    #[test]
    fn raw_lf_line_model_keeps_cr_and_trailing_empty_line() {
        let bytes = b"a\r\nb\n";
        let starts = lines(bytes);
        assert_eq!(starts, vec![0, 3, 5]);
        assert_eq!(slice_lines(bytes, &starts, 1, 1), b"a\r\n");
        assert_eq!(slice_lines(bytes, &starts, 2, 3), b"b\n");
    }

    #[test]
    fn policy_bounds_are_exact_at_and_over_the_limit() {
        assert!(text(&"x".repeat(MAX_TEXT)).is_ok());
        assert!(matches!(
            text(&"x".repeat(MAX_TEXT + 1)),
            Err(ContextError::Domain(DomainError::Incomplete { .. }))
        ));
    }

    #[test]
    fn ordered_session_is_atomic_and_projection_round_trips_strictly() {
        let (aggregate, bytes) = fixture();
        let obligation = aggregate.obligations().next().unwrap().id().clone();
        let mut session = prepare_context(&aggregate, obligation).unwrap();
        let mut saw_request = false;
        while let Some(request) = session.next_source_request().unwrap() {
            saw_request = true;
            assert_eq!(
                request.expected_length(),
                bytes[request.artifact_id()].len() as u64
            );
            assert_eq!(
                request.content_hash(),
                &ContentHash::sha256(&bytes[request.artifact_id()])
            );
            let mut out_of_order = request.clone();
            out_of_order.ordinal += 1;
            assert!(
                session
                    .submit_source(&out_of_order, &bytes[request.artifact_id()])
                    .is_err()
            );
            assert!(session.submit_source(&request, b"wrong").is_err());
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
            assert!(
                session
                    .submit_source(&request, &bytes[request.artifact_id()])
                    .is_err()
            );
        }
        assert!(saw_request);
        let built = session.finish().unwrap();
        let canonical = built.envelope().canonical_bytes().unwrap();
        assert_eq!(
            built.envelope().canonical_byte_len().unwrap(),
            canonical.len()
        );
        assert_eq!(
            ContextPolicyV1::baseline().canonical_bytes().unwrap().len(),
            CONTEXT_POLICY_CANONICAL_BYTE_LEN
        );
        let decoded = ReviewContextEnvelope::from_canonical_bytes(&canonical, &aggregate).unwrap();
        assert_eq!(&decoded, built.envelope());
        assert!(decoded.allocated_bytes() > 0);
        let (envelope, _admission) = built.into_parts();
        assert_eq!(
            envelope.normalized_included_source_ids(),
            &envelope
                .included_sources()
                .iter()
                .map(|source| source.artifact_id().clone())
                .collect()
        );
        let partition = envelope
            .normalized_included_source_ids()
            .union(
                &envelope
                    .excluded_sources()
                    .iter()
                    .map(|source| source.artifact_id().clone())
                    .collect(),
            )
            .cloned()
            .collect::<BTreeSet<_>>();
        assert_eq!(&partition, envelope.candidate_source_ids());
        assert_eq!(
            envelope.projection_policy_version(),
            ContextPolicyV1::VERSION
        );
        assert!(envelope.assumptions().is_empty());
        for unknown in envelope.unknowns() {
            assert!(!unknown.description().is_empty());
            assert!(!unknown.source_ids().is_empty());
        }
        for loss in envelope.losses() {
            assert!(!loss.description().is_empty());
            assert_eq!(loss.severity(), Severity::Low);
            assert!(!loss.affected_properties().is_empty());
            assert!(!loss.source_ids().is_empty());
        }
        let mut forged_unknowns = envelope.clone();
        forged_unknowns.unknowns = vec![EnvelopeUnknown {
            description: "context_unknown:unresolved_seed_reference".to_owned(),
            source_ids: BTreeSet::from([envelope.candidate_source_ids().first().unwrap().clone()]),
        }];
        let forged_body = identity_bytes(
            forged_unknowns.snapshot_id(),
            forged_unknowns.obligation_ids().first().unwrap(),
            forged_unknowns.context_policy_hash(),
            forged_unknowns.candidate_source_ids(),
            forged_unknowns.included_sources(),
            forged_unknowns.excluded_sources(),
            forged_unknowns.unknowns(),
            forged_unknowns.losses(),
        )
        .unwrap();
        forged_unknowns.projection_hash = ContentHash::sha256(&forged_body);
        forged_unknowns.id = StableId::parse(format!(
            "context-envelope:{}",
            forged_unknowns.projection_hash
        ))
        .unwrap();
        let forged_bytes = forged_unknowns.canonical_bytes().unwrap();
        assert!(ReviewContextEnvelope::from_canonical_bytes(&forged_bytes, &aggregate).is_err());
        let mut wrong_algorithm = canonical.clone();
        let excerpt_hash = envelope.included_sources()[0].excerpt_hash().to_string();
        let offset = wrong_algorithm
            .windows(excerpt_hash.len())
            .position(|window| window == excerpt_hash.as_bytes())
            .unwrap();
        wrong_algorithm[offset..offset + 6].copy_from_slice(b"blake3");
        assert!(ReviewContextEnvelope::from_canonical_bytes(&wrong_algorithm, &aggregate).is_err());
        let mut tampered = canonical;
        let position = tampered.iter().position(|byte| *byte == b'{').unwrap();
        tampered.insert(position + 1, b' ');
        assert!(ReviewContextEnvelope::from_canonical_bytes(&tampered, &aggregate).is_err());
    }

    #[test]
    fn raw_line_count_and_excerpt_boundaries_cover_empty_binary_and_trailing_lf() {
        assert_eq!(lines(b""), vec![0]);
        assert_eq!(lines(&[0xff, b'\r', b'\n']), vec![0, 3]);
        let starts = lines(b"a\n");
        assert_eq!(slice_lines(b"a\n", &starts, 1, 2), b"a\n");
        let giant = vec![b'x'; MAX_EXCERPT + 1];
        assert!(matches!(
            excerpt(&giant, &[], 0).unwrap(),
            Excerpt::Exclude(ExclusionReason::GiantLine)
        ));
        let anchored = b"1\n2\n3\n4\n5\n";
        let Excerpt::Include(Some(range), bytes) = excerpt(
            anchored,
            &[
                (2, 4, StableId::parse("artifact:a").unwrap()),
                (3, 3, StableId::parse("artifact:b").unwrap()),
            ],
            0,
        )
        .unwrap() else {
            panic!("bounded anchors produce a range")
        };
        assert_eq!((range.start_line(), range.end_line()), (2, 4));
        assert_eq!(bytes, b"2\n3\n4\n");
        assert!(
            excerpt(
                anchored,
                &[(1, 99, StableId::parse("artifact:a").unwrap())],
                0
            )
            .is_err()
        );
        let Excerpt::Include(None, all) = excerpt(b"x\n", &[], 0).unwrap() else {
            panic!("full range normalizes to None")
        };
        assert_eq!(all, b"x\n");
    }

    #[test]
    fn bounded_writer_accepts_exact_and_refuses_plus_one_before_append() {
        let mut exact = BoundedJson::new("test bounded writer");
        exact.push(&vec![b'x'; MAX_BODY]).unwrap();
        assert_eq!(exact.finish().unwrap().len(), MAX_BODY);
        let mut over = BoundedJson::new("test bounded writer");
        over.push(&vec![b'x'; MAX_BODY]).unwrap();
        assert!(matches!(
            over.push(b"x"),
            Err(ContextError::Domain(DomainError::Incomplete { .. }))
        ));
        let mut escaped = BoundedJson::new("test escaped string");
        escaped
            .text("quote:\" slash:\\ control:\u{1f} 日本")
            .unwrap();
        assert_eq!(
            escaped.finish().unwrap(),
            serde_json::to_vec("quote:\" slash:\\ control:\u{1f} 日本").unwrap()
        );
    }

    #[test]
    fn d1_events_require_live_context_admission_but_offline_retain_metadata_only() {
        let (ready, bytes) = fixture();
        let initial = event_initial(&ready);
        let run_id = StableId::parse("run:context-test").unwrap();
        let sources = ready
            .snapshot_sources_for(ready.program().snapshot_id())
            .unwrap()
            .clone();
        let mut log = EventLog::new(run_id.clone(), initial.clone()).unwrap();
        let registration_ids = sources
            .entries()
            .iter()
            .map(|entry| entry.registration_id().clone())
            .collect::<BTreeSet<_>>();
        for registration_id in registration_ids {
            log.append(EventCommand::artifact_registered(
                ready.registered_artifact(&registration_id).unwrap().clone(),
            ))
            .unwrap();
        }
        log.append(EventCommand::snapshot_sources_recorded(sources))
            .unwrap();

        let review_plan = plan(log.aggregate(), PlanBudget::new(16, 2).unwrap()).unwrap();
        let plan_id = review_plan.id().clone();
        log.append(EventCommand::review_plan_recorded(review_plan.clone()))
            .unwrap();
        let count_before_duplicate = log.events().len();
        let tail_before_duplicate = log.tail_hash().clone();
        assert!(matches!(
            log.append(EventCommand::review_plan_recorded(review_plan)),
            Err(DomainError::IdCollision { .. })
        ));
        assert_eq!(log.events().len(), count_before_duplicate);
        assert_eq!(log.tail_hash(), &tail_before_duplicate);
        let mut spliced_log = log.clone();
        let built = build_projection(log.aggregate(), &bytes);
        let context_id = built.envelope().id().clone();
        log.append(EventCommand::context_envelope_projected(built))
            .unwrap();
        let duplicate_context = build_projection(log.aggregate(), &bytes);
        let count_before_duplicate = log.events().len();
        let tail_before_duplicate = log.tail_hash().clone();
        assert!(matches!(
            log.append(EventCommand::context_envelope_projected(duplicate_context)),
            Err(DomainError::IdCollision { .. })
        ));
        assert_eq!(log.events().len(), count_before_duplicate);
        assert_eq!(log.tail_hash(), &tail_before_duplicate);
        assert!(log.aggregate().review_plan(&plan_id).is_some());
        assert!(log.aggregate().context_envelope(&context_id).is_some());
        assert_eq!(log.aggregate().review_plans().count(), 1);
        assert_eq!(log.aggregate().context_envelopes().count(), 1);

        let context_event = log.envelopes().last().unwrap();
        let context_event_bytes = context_event.canonical_bytes().unwrap();
        assert!(context_event_bytes.len() < 1_048_576);
        assert_eq!(
            EventEnvelope::from_json_slice(&context_event_bytes)
                .unwrap()
                .canonical_bytes()
                .unwrap(),
            context_event_bytes
        );

        let replayed = EventLog::replay(
            EventContractVersion::V2,
            run_id.clone(),
            initial.clone(),
            log.events(),
        )
        .unwrap();
        assert!(replayed.aggregate().review_plan(&plan_id).is_some());
        assert!(replayed.aggregate().context_envelope(&context_id).is_some());

        let envelopes = log.envelopes().cloned().collect::<Vec<_>>();
        assert!(
            EventLog::replay_envelopes(
                EventContractVersion::V2,
                run_id.clone(),
                initial.clone(),
                &envelopes,
                &EventAdmissions::default(),
            )
            .is_err()
        );
        let spliced_obligation = spliced_log
            .aggregate()
            .obligations()
            .next()
            .unwrap()
            .id()
            .clone();
        spliced_log
            .append(EventCommand::obligation_transition(
                spliced_obligation,
                ObligationLifecycle::Planned,
            ))
            .unwrap();
        let spliced_context = build_projection(spliced_log.aggregate(), &bytes);
        assert_eq!(spliced_context.envelope().id(), &context_id);
        spliced_log
            .append(EventCommand::context_envelope_projected(spliced_context))
            .unwrap();
        let spliced_envelopes = spliced_log.envelopes().cloned().collect::<Vec<_>>();

        let rebuilt = build_projection(log.aggregate(), &bytes);
        let admissions = EventAdmissions::default()
            .with_context_projections(vec![(envelopes.last().unwrap().clone(), rebuilt)])
            .unwrap();
        assert!(
            EventLog::replay_envelopes(
                EventContractVersion::V2,
                run_id.clone(),
                initial.clone(),
                &spliced_envelopes,
                &admissions,
            )
            .is_err()
        );
        assert!(
            EventLog::replay_envelopes(
                EventContractVersion::V2,
                run_id.clone(),
                initial.clone(),
                &envelopes,
                &admissions,
            )
            .is_ok()
        );

        let duplicate_context_event = envelopes
            .last()
            .unwrap()
            .next_context_duplicate_for_test(
                log.aggregate()
                    .context_envelope(&context_id)
                    .unwrap()
                    .clone(),
            )
            .unwrap();
        let mut duplicate_envelopes = envelopes.clone();
        duplicate_envelopes.push(duplicate_context_event.clone());
        let duplicate_admissions = EventAdmissions::default()
            .with_context_projections(vec![
                (
                    envelopes.last().unwrap().clone(),
                    build_projection(log.aggregate(), &bytes),
                ),
                (
                    duplicate_context_event.clone(),
                    build_projection(log.aggregate(), &bytes),
                ),
            ])
            .unwrap();
        assert!(matches!(
            EventLog::replay_envelopes(
                EventContractVersion::V2,
                run_id.clone(),
                initial.clone(),
                &duplicate_envelopes,
                &duplicate_admissions,
            ),
            Err(DomainError::IdCollision { .. })
        ));

        let mut duplicate_resume = EventLog::replay_envelopes(
            EventContractVersion::V2,
            run_id.clone(),
            initial.clone(),
            &envelopes,
            &admissions,
        )
        .unwrap();
        let duplicate_resume_tail = duplicate_resume.tail_hash().clone();
        let duplicate_resume_count = duplicate_resume.events().len();
        let duplicate_only_admission = EventAdmissions::default()
            .with_context_projections(vec![(
                duplicate_context_event.clone(),
                build_projection(log.aggregate(), &bytes),
            )])
            .unwrap();
        assert!(matches!(
            duplicate_resume.resume_envelopes(
                std::slice::from_ref(&duplicate_context_event),
                &duplicate_only_admission,
            ),
            Err(DomainError::IdCollision { .. })
        ));
        assert_eq!(duplicate_resume.tail_hash(), &duplicate_resume_tail);
        assert_eq!(duplicate_resume.events().len(), duplicate_resume_count);
        assert_eq!(duplicate_resume.aggregate().context_envelopes().count(), 1);

        let prefix = &envelopes[..envelopes.len() - 1];
        let mut resumed = EventLog::replay_envelopes(
            EventContractVersion::V2,
            run_id.clone(),
            initial.clone(),
            prefix,
            &EventAdmissions::default(),
        )
        .unwrap();
        let prior_tail = resumed.tail_hash().clone();
        let prior_count = resumed.events().len();
        assert!(
            resumed
                .resume_envelopes(
                    &envelopes[envelopes.len() - 1..],
                    &EventAdmissions::default()
                )
                .is_err()
        );
        assert_eq!(resumed.tail_hash(), &prior_tail);
        assert_eq!(resumed.events().len(), prior_count);
        assert_eq!(resumed.aggregate().context_envelopes().count(), 0);

        let genesis = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let view = EventEnvelope::validated_view(
            EventContractVersion::V2,
            &run_id,
            EventStreamGenesis::V2(&genesis),
            &envelopes,
        )
        .unwrap();
        let mut offline = OfflineProjectionState::new(&view, initial.clone()).unwrap();
        let mut plan_applied = false;
        let mut context_metadata_only = false;
        for event in view.events() {
            let accepted = offline.apply(event).unwrap();
            match event.payload() {
                crate::DecodedPayload::ReviewPlanRecorded(_) => plan_applied = accepted,
                crate::DecodedPayload::ContextEnvelopeProjected(_) => {
                    context_metadata_only = !accepted
                }
                _ => {}
            }
        }
        assert!(plan_applied);
        assert!(context_metadata_only);
        assert!(offline.aggregate().review_plan(&plan_id).is_some());
        assert_eq!(offline.aggregate().context_envelopes().count(), 0);
        assert_eq!(offline.projected_context_envelopes().len(), 1);
        assert_eq!(offline.projected_context_envelopes()[0].id(), &context_id);
        assert!(offline.is_complete());
        assert_eq!(offline.tail_hash(), log.tail_hash());

        let duplicate_view = EventEnvelope::validated_view(
            EventContractVersion::V2,
            &run_id,
            EventStreamGenesis::V2(&genesis),
            &duplicate_envelopes,
        )
        .unwrap();
        let mut duplicate_offline =
            OfflineProjectionState::new(&duplicate_view, initial.clone()).unwrap();
        for event in &duplicate_view.events()[..duplicate_view.events().len() - 1] {
            duplicate_offline.apply(event).unwrap();
        }
        let duplicate_offline_tail = duplicate_offline.tail_hash().clone();
        assert!(matches!(
            duplicate_offline.apply(duplicate_view.events().last().unwrap()),
            Err(DomainError::IdCollision { .. })
        ));
        assert_eq!(duplicate_offline.tail_hash(), &duplicate_offline_tail);
        assert_eq!(duplicate_offline.projected_context_envelopes().len(), 1);
        assert!(!duplicate_offline.is_complete());

        let mut v1 = EventLog::new_v1_for_test(run_id, initial).unwrap();
        let v1_plan = plan(v1.aggregate(), PlanBudget::new(16, 2).unwrap()).unwrap();
        assert!(
            v1.append(EventCommand::review_plan_recorded(v1_plan))
                .is_err()
        );
        let v1_context = build_projection(&ready, &bytes);
        assert!(
            v1.append(EventCommand::context_envelope_projected(v1_context))
                .is_err()
        );
    }

    #[test]
    fn production_envelope_accepts_exact_body_limit_and_refuses_the_next_byte() {
        let (aggregate, bytes) = fixture();
        let obligation = aggregate.obligations().next().unwrap().id().clone();
        let mut session = prepare_context(&aggregate, obligation).unwrap();
        while let Some(request) = session.next_source_request().unwrap() {
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
        }
        let (mut envelope, _) = session.finish().unwrap().into_parts();
        envelope.unknowns.clear();
        loop {
            let base = envelope.canonical_bytes().unwrap().len();
            let mut empty = envelope.clone();
            empty.unknowns.push(EnvelopeUnknown {
                description: String::new(),
                source_ids: BTreeSet::new(),
            });
            let overhead = empty.canonical_bytes().unwrap().len() - base;
            if base + overhead + (MAX_TEXT - 1) < MAX_BODY {
                envelope.unknowns.push(EnvelopeUnknown {
                    description: "x".repeat(MAX_TEXT - 1),
                    source_ids: BTreeSet::new(),
                });
                continue;
            }
            let remaining = MAX_BODY - base - overhead;
            assert!(remaining < MAX_TEXT);
            envelope.unknowns.push(EnvelopeUnknown {
                description: "x".repeat(remaining),
                source_ids: BTreeSet::new(),
            });
            break;
        }
        assert_eq!(envelope.canonical_bytes().unwrap().len(), MAX_BODY);
        envelope.unknowns.last_mut().unwrap().description.push('x');
        assert!(matches!(
            envelope.canonical_bytes(),
            Err(ContextError::Domain(DomainError::Incomplete {
                operation: "context envelope canonical bytes",
                limit: MAX_BODY,
                observed,
            })) if observed == MAX_BODY + 1
        ));
    }

    #[test]
    fn seed_kinds_expand_by_accepted_tables_and_undirected_edges_do_not_participate() {
        let program: ProgramSpace = serde_json::from_str(include_str!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let (_, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let base = &obligations[0];
        let with_seed = |seed: &StableId| {
            let mut value = serde_json::to_value(base).unwrap();
            value["target_refs"] = json!([seed]);
            value["normalized_target_refs"] = json!([seed]);
            value["source_ids"] = json!([seed]);
            value["normalized_source_ids"] = json!([seed]);
            value["context_ids"] = json!([]);
            value["normalized_context_ids"] = json!([]);
            serde_json::from_value::<Obligation>(value).unwrap()
        };
        let artifact = &program.artifacts()[0].id;
        let relation = &program.relations()[0];
        let context = &program.contexts()[0];
        let invariant = &program.invariants()[0];
        assert!(
            discover(&program, &with_seed(artifact))
                .unwrap()
                .1
                .contains(artifact)
        );
        let relation_result = discover(&program, &with_seed(&relation.id)).unwrap();
        assert!(relation_result.1.contains(&relation.source_id));
        assert!(
            relation
                .target_ids
                .iter()
                .all(|id| relation_result.1.contains(id))
        );
        let context_result = discover(&program, &with_seed(&context.id)).unwrap();
        assert!(
            context
                .member_ids
                .iter()
                .all(|id| { program.artifact(id).is_none() || context_result.1.contains(id) })
        );
        let invariant_result = discover(&program, &with_seed(&invariant.id)).unwrap();
        assert!(
            invariant
                .scope_ids
                .iter()
                .all(|id| { program.artifact(id).is_none() || invariant_result.1.contains(id) })
        );

        let baseline = discover(&program, &with_seed(artifact)).unwrap();
        let mut value = serde_json::to_value(&program).unwrap();
        let mut added = value["relations"][0].clone();
        added["id"] = json!("relation:undirected-context-test");
        added["kind"] = json!("calls");
        added["source_id"] = json!(artifact);
        added["target_ids"] = json!([program.artifacts().last().unwrap().id]);
        added["directed"] = json!(false);
        value["relations"].as_array_mut().unwrap().push(added);
        let modified: ProgramSpace = serde_json::from_value(value).unwrap();
        let modified_result = discover(&modified, &with_seed(artifact)).unwrap();
        assert_eq!(baseline.1, modified_result.1);
        assert!(baseline.3.is_disjoint(&baseline.6));
        assert!(baseline.3.is_disjoint(&baseline.7));
        assert_eq!(baseline, discover(&program, &with_seed(artifact)).unwrap());
    }

    #[test]
    fn accepted_nested_seed_references_are_added_once_without_kind_expansion() {
        let mut relation_value = reference_program_value();
        let outer_relation = relation_value["relations"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let nested_relation = relation_value["relations"][1]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        relation_value["relations"][0]["target_ids"] = json!([nested_relation]);
        let relation_program: ProgramSpace = serde_json::from_value(relation_value).unwrap();
        let relation_result =
            discover(&relation_program, &obligation_with_seed(&outer_relation)).unwrap();
        assert!(
            relation_result
                .1
                .contains(&StableId::parse(&nested_relation).unwrap())
        );
        assert!(relation_result.8.is_empty());

        let mut context_value = reference_program_value();
        let context_id = context_value["contexts"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        context_value["contexts"][0]["member_ids"] = json!([nested_relation]);
        let context_program: ProgramSpace = serde_json::from_value(context_value).unwrap();
        let context_result =
            discover(&context_program, &obligation_with_seed(&context_id)).unwrap();
        assert!(
            context_result
                .1
                .contains(&StableId::parse(&nested_relation).unwrap())
        );
        assert!(context_result.8.is_empty());

        let mut invariant_value = reference_program_value();
        let invariant_id = invariant_value["invariants"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let nested_context = invariant_value["contexts"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        invariant_value["invariants"][0]["scope_ids"] = json!([nested_context]);
        let invariant_program: ProgramSpace = serde_json::from_value(invariant_value).unwrap();
        let invariant_result =
            discover(&invariant_program, &obligation_with_seed(&invariant_id)).unwrap();
        assert!(
            invariant_result
                .1
                .contains(&StableId::parse(&nested_context).unwrap())
        );
        assert!(invariant_result.8.is_empty());

        let contexts = invariant_program
            .contexts()
            .iter()
            .map(|context| (&context.id, context))
            .collect::<BTreeMap<_, _>>();
        for description in [
            "context_unknown:unresolved_relation_endpoint",
            "context_unknown:unresolved_review_context_member",
            "context_unknown:unresolved_invariant_scope",
        ] {
            let missing = StableId::parse(format!(
                "artifact:missing-{}",
                description.rsplit(':').next().unwrap()
            ))
            .unwrap();
            let mut structural = BTreeSet::new();
            let mut unknown = BTreeMap::new();
            add_seed_reference(
                &invariant_program,
                &contexts,
                &missing,
                description,
                &mut structural,
                &mut unknown,
            )
            .unwrap();
            assert!(structural.is_empty());
            assert_eq!(unknown.get(description), Some(&BTreeSet::from([missing])));
        }
    }

    #[test]
    fn mixed_depth_paths_reset_only_on_an_exact_edge_direction_state() {
        let mut value = reference_program_value();
        let seed = "function:mixed-seed";
        append_artifact(&mut value, seed, "function", None);
        for index in 0..=5 {
            append_artifact(
                &mut value,
                &format!("function:mixed-{index}"),
                "function",
                None,
            );
        }
        append_relation(
            &mut value,
            "relation:mixed-switch",
            "covers",
            seed,
            &["function:mixed-0".to_owned()],
        );
        for index in 0..5 {
            append_relation(
                &mut value,
                &format!("relation:mixed-call-{index}"),
                "calls",
                &format!("function:mixed-{index}"),
                &[format!("function:mixed-{}", index + 1)],
            );
        }
        let program: ProgramSpace = serde_json::from_value(value).unwrap();
        let discovered = discover(&program, &obligation_with_seed(seed)).unwrap().1;
        assert!(discovered.contains(&StableId::parse("function:mixed-3").unwrap()));
        assert!(!discovered.contains(&StableId::parse("function:mixed-4").unwrap()));

        let mut value = reference_program_value();
        append_artifact(&mut value, seed, "function", None);
        for prefix in ["forward", "reverse"] {
            for index in 0..=4 {
                append_artifact(
                    &mut value,
                    &format!("function:{prefix}-{index}"),
                    "function",
                    None,
                );
            }
        }
        let mut previous = seed.to_owned();
        for index in 0..=4 {
            let next = format!("function:forward-{index}");
            append_relation(
                &mut value,
                &format!("relation:forward-{index}"),
                "calls",
                &previous,
                std::slice::from_ref(&next),
            );
            previous = next;
        }
        previous = seed.to_owned();
        for index in 0..=4 {
            let next = format!("function:reverse-{index}");
            append_relation(
                &mut value,
                &format!("relation:reverse-{index}"),
                "calls",
                &next,
                std::slice::from_ref(&previous),
            );
            previous = next;
        }
        let program: ProgramSpace = serde_json::from_value(value).unwrap();
        let discovered = discover(&program, &obligation_with_seed(seed)).unwrap().1;
        assert!(discovered.contains(&StableId::parse("function:forward-2").unwrap()));
        assert!(!discovered.contains(&StableId::parse("function:forward-3").unwrap()));
        assert!(discovered.contains(&StableId::parse("function:reverse-1").unwrap()));
        assert!(!discovered.contains(&StableId::parse("function:reverse-2").unwrap()));
    }

    #[test]
    fn all_nine_legal_predecessor_states_do_not_trigger_a_false_incomplete() {
        let mut value = reference_program_value();
        let seed = "function:nine-seed";
        let target = "function:nine-target";
        for id in [
            seed,
            target,
            "function:nine-a",
            "function:nine-b",
            "function:nine-c",
            "function:nine-d",
        ] {
            append_artifact(&mut value, id, "function", None);
        }
        for (id, source, target) in [
            ("relation:nine-call-direct", seed, target),
            ("relation:nine-call-a", seed, "function:nine-a"),
            ("relation:nine-call-a-target", "function:nine-a", target),
            ("relation:nine-call-b", seed, "function:nine-b"),
            (
                "relation:nine-call-b-c",
                "function:nine-b",
                "function:nine-c",
            ),
            ("relation:nine-call-c-target", "function:nine-c", target),
            ("relation:nine-reverse-direct", target, seed),
            ("relation:nine-reverse-d", "function:nine-d", seed),
            ("relation:nine-reverse-target", target, "function:nine-d"),
        ] {
            append_relation(&mut value, id, "calls", source, &[target.to_owned()]);
        }
        for (index, (kind, source, destination)) in [
            ("contains", seed, target),
            ("contains", target, seed),
            ("covers", seed, target),
            ("covers", target, seed),
        ]
        .into_iter()
        .enumerate()
        {
            append_relation(
                &mut value,
                &format!("relation:nine-extra-{index}"),
                kind,
                source,
                &[destination.to_owned()],
            );
        }
        let program: ProgramSpace = serde_json::from_value(value).unwrap();
        let discovered = discover(&program, &obligation_with_seed(seed)).unwrap();
        assert!(discovered.1.contains(&StableId::parse(target).unwrap()));
    }

    #[test]
    fn selected_and_unselected_path_and_test_files_are_subtracted_before_caps() {
        let mut value = reference_program_value();
        let seed = "function:cap-seed";
        append_artifact(&mut value, seed, "function", None);
        for index in 0..=20 {
            let node = format!("function:path-{index:02}");
            append_artifact(&mut value, &node, "function", None);
            append_relation(
                &mut value,
                &format!("relation:path-{index:02}"),
                "calls",
                seed,
                std::slice::from_ref(&node),
            );
        }
        for index in 0..=10 {
            let test = format!("test:cap-{index:02}");
            append_artifact(&mut value, &test, "test", None);
            append_relation(
                &mut value,
                &format!("relation:test-{index:02}"),
                "covers",
                &test,
                &[seed.to_owned()],
            );
        }
        for (id, targets) in [
            (
                "file:path-shared",
                vec!["function:path-00".to_owned(), "function:path-20".to_owned()],
            ),
            ("file:path-late", vec!["function:path-20".to_owned()]),
            (
                "file:test-shared",
                vec!["test:cap-00".to_owned(), "test:cap-10".to_owned()],
            ),
            ("file:test-late", vec!["test:cap-10".to_owned()]),
        ] {
            append_artifact(&mut value, id, "file", Some(&format!("synthetic/{id}.rs")));
            append_relation(
                &mut value,
                &format!("relation:contains-{id}"),
                "contains",
                id,
                &targets,
            );
        }
        let program: ProgramSpace = serde_json::from_value(value).unwrap();
        let result = discover(&program, &obligation_with_seed(seed)).unwrap();
        let path_cap = result.6;
        let test_cap = result.7;
        assert!(path_cap.contains(&StableId::parse("file:path-late").unwrap()));
        assert!(!path_cap.contains(&StableId::parse("file:path-shared").unwrap()));
        assert!(test_cap.contains(&StableId::parse("file:test-late").unwrap()));
        assert!(!test_cap.contains(&StableId::parse("file:test-shared").unwrap()));
        let overlap = StableId::parse("file:test-late").unwrap();
        assert!(path_cap.contains(&overlap));
        assert_eq!(
            discovery_exclusion(&overlap, &path_cap, &test_cap, &result.0),
            Some(ExclusionReason::PathCap)
        );
    }

    #[test]
    fn exclusion_precedence_and_giant_line_backfill_are_exact() {
        assert_eq!(
            metadata_exclusion(MAX_FILES, MAX_FILE_BYTES + 1, MAX_RESOLVED).unwrap(),
            Some(ExclusionReason::IncludedFileCap)
        );
        assert_eq!(
            metadata_exclusion(0, MAX_FILE_BYTES + 1, MAX_RESOLVED).unwrap(),
            Some(ExclusionReason::ArtifactBytesCap)
        );
        assert_eq!(
            metadata_exclusion(0, 1, MAX_RESOLVED).unwrap(),
            Some(ExclusionReason::TotalResolvedBytesCap)
        );

        let (baseline, _) = fixture();
        let obligation = baseline.obligations().next().unwrap().id().clone();
        let baseline_session = prepare_context(&baseline, obligation.clone()).unwrap();
        let first_path = baseline_session
            .candidates
            .first()
            .unwrap()
            .source
            .path()
            .to_owned();
        let first_anchor_line = baseline_session.candidates[0]
            .anchors
            .iter()
            .map(|(start, _, _)| *start)
            .min()
            .unwrap_or(1);
        let mut source_bytes = BTreeMap::from([
            (
                "src/checkout_controller.rs",
                b"small checkout\n".repeat(100),
            ),
            ("src/payment_repository.rs", b"small payment\n".repeat(100)),
        ]);
        let mut giant = b"x\n".repeat(first_anchor_line.saturating_sub(1) as usize);
        giant.extend(std::iter::repeat_n(b'x', MAX_EXCERPT + 1));
        giant.extend(std::iter::repeat_n(b'\n', 100));
        source_bytes.insert(
            if first_path == "src/checkout_controller.rs" {
                "src/checkout_controller.rs"
            } else {
                "src/payment_repository.rs"
            },
            giant,
        );
        let (aggregate, bytes) = fixture_with_source_bytes(source_bytes);
        let mut session = prepare_context(&aggregate, obligation).unwrap();
        for candidate in &mut session.candidates {
            candidate.exclusion = None;
        }
        let first = session.next_source_request().unwrap().unwrap();
        let first_id = first.artifact_id().clone();
        session
            .submit_source(&first, &bytes[first.artifact_id()])
            .unwrap();
        let second = session.next_source_request().unwrap().unwrap();
        let second_id = second.artifact_id().clone();
        session
            .submit_source(&second, &bytes[second.artifact_id()])
            .unwrap();
        assert!(session.next_source_request().unwrap().is_none());
        let built = session.finish().unwrap();
        assert_eq!(
            built
                .envelope()
                .excluded_sources()
                .iter()
                .find(|source| source.artifact_id() == &first_id)
                .unwrap()
                .reason(),
            ExclusionReason::GiantLine
        );
        assert!(
            built
                .envelope()
                .included_sources()
                .iter()
                .any(|source| source.artifact_id() == &second_id)
        );
    }

    #[test]
    fn candidate_ranking_matches_direct_distance_path_rank_and_stable_id() {
        let (aggregate, _) = fixture();
        let obligation = aggregate.obligations().next().unwrap().clone();
        let discovered = discover(aggregate.program(), &obligation).unwrap();
        let direct = discovered.3;
        let distances = discovered.4;
        let path_ranks = discovered.5;
        let mut expected = aggregate
            .program()
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
            .map(|artifact| artifact.id.clone())
            .collect::<Vec<_>>();
        expected.sort_by_key(|id| {
            (
                if direct.contains(id) { 0 } else { 1 },
                *distances.get(id).unwrap_or(&usize::MAX),
                *path_ranks.get(id).unwrap_or(&usize::MAX),
                id.clone(),
            )
        });
        let session = prepare_context(&aggregate, obligation.id().clone()).unwrap();
        assert_eq!(
            session
                .candidates
                .iter()
                .map(|candidate| candidate.artifact.id.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(
            session
                .candidates
                .windows(2)
                .all(|pair| pair[0].rank < pair[1].rank)
        );
    }

    #[test]
    fn resolver_uses_rank_order_but_wire_sources_use_stable_id_order() {
        let (aggregate, bytes) = fixture();
        let obligation = aggregate.obligations().next().unwrap().id().clone();
        let mut session = prepare_context(&aggregate, obligation).unwrap();
        assert_eq!(session.candidates.len(), 2);
        for candidate in &mut session.candidates {
            candidate.exclusion = None;
        }
        session.candidates[0].rank.0 = 1;
        session.candidates[1].rank.0 = 0;
        session
            .candidates
            .sort_by(|left, right| left.rank.cmp(&right.rank));
        session.session_digest = manifest_digest(
            &session.snapshot_id,
            session.obligation.id(),
            &session.policy_hash,
            &session.candidates,
        )
        .unwrap();
        let mut resolution_order = Vec::new();
        while let Some(request) = session.next_source_request().unwrap() {
            resolution_order.push(request.artifact_id().clone());
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
        }
        assert!(resolution_order[0] > resolution_order[1]);
        let built = session.finish().unwrap();
        let wire_order = built
            .envelope()
            .included_sources()
            .iter()
            .map(|source| source.artifact_id().clone())
            .collect::<Vec<_>>();
        assert!(wire_order[0] < wire_order[1]);
        let canonical = built.envelope().canonical_bytes().unwrap();
        assert!(ReviewContextEnvelope::from_canonical_bytes(&canonical, &aggregate).is_ok());
    }

    #[test]
    fn candidate_fixture_refuses_the_actual_four_thousand_and_ninety_seventh_file() {
        let (aggregate, _) = fixture();
        let mut value = reference_program_value();
        for index in 2..=MAX {
            append_artifact(
                &mut value,
                &format!("file:cap-{index:04}"),
                "file",
                Some(&format!("synthetic/cap-{index:04}.rs")),
            );
        }
        let program: ProgramSpace = serde_json::from_value(value).unwrap();
        assert_eq!(
            program
                .artifacts()
                .iter()
                .filter(|artifact| artifact.kind == "file")
                .count(),
            MAX + 1
        );
        let sources = aggregate
            .snapshot_sources_for(aggregate.program().snapshot_id())
            .unwrap();
        assert!(matches!(
            candidates(&aggregate, &program, sources, program.snapshot_id()),
            Err(ContextError::Domain(DomainError::Incomplete {
                operation: "context candidate files",
                limit: MAX,
                observed,
            })) if observed == MAX + 1
        ));
    }

    #[test]
    fn prepare_accepts_all_four_thousand_and_ninety_six_legal_manifest_candidates() {
        let aggregate = fixture_with_candidate_count(MAX);
        let obligation = aggregate.obligations().next().unwrap().id().clone();
        let session = prepare_context(&aggregate, obligation).unwrap();
        assert_eq!(session.candidates.len(), MAX);
        assert!(is_sha256(&session.session_digest));
    }

    #[test]
    fn reached_range_locations_require_containment_and_exact_owner_path() {
        let mut missing = context_ready_program_value();
        missing["relations"]
            .as_array_mut()
            .unwrap()
            .retain(|relation| relation["kind"] != "contains");
        let (missing_aggregate, _) = fixture_from_program_value(missing, default_source_bytes());
        let obligation = missing_aggregate.obligations().next().unwrap().id().clone();
        assert!(matches!(
            prepare_context(&missing_aggregate, obligation),
            Err(ContextError::Domain(DomainError::Validation(message)))
                if message.contains("has no containing candidate file")
        ));

        let mut mismatched = context_ready_program_value();
        for artifact in mismatched["artifacts"].as_array_mut().unwrap() {
            if artifact["location"]["start_line"].is_number() {
                artifact["location"]["path"] = json!("src/not-the-owner.rs");
            }
        }
        let (mismatched_aggregate, _) =
            fixture_from_program_value(mismatched, default_source_bytes());
        let obligation = mismatched_aggregate
            .obligations()
            .next()
            .unwrap()
            .id()
            .clone();
        assert!(matches!(
            prepare_context(&mismatched_aggregate, obligation),
            Err(ContextError::Domain(DomainError::Validation(message)))
                if message.contains("has no exact-path containing candidate file")
        ));
    }

    #[test]
    fn registered_line_count_mismatch_refuses_without_consuming_pending_request() {
        let (mut aggregate, bytes) = fixture();
        let snapshot = aggregate.program().snapshot_id().clone();
        let mut entries = aggregate
            .snapshot_sources_for(&snapshot)
            .unwrap()
            .entries()
            .to_vec();
        let first = &entries[0];
        entries[0] = SnapshotSourceRecordEntry::new(
            first.artifact_id().clone(),
            first.path(),
            first.content_hash().clone(),
            first.registration_id().clone(),
            first.cas_hash().clone(),
            first.line_count() + 1,
        )
        .unwrap();
        aggregate.replace_snapshot_sources_for_test(
            SnapshotSourcesRecorded::new(snapshot, entries).unwrap(),
        );
        let obligation = aggregate.obligations().next().unwrap().id().clone();
        let mut session = prepare_context(&aggregate, obligation).unwrap();
        let request = session.next_source_request().unwrap().unwrap();
        assert!(
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .is_err()
        );
        assert!(
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .is_err()
        );
    }
}
