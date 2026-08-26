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
use std::mem::size_of;
use std::sync::{Arc, Mutex};
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

#[cfg(test)]
thread_local! {
    static OMIT_ONE_ACCEPTED_FILE_IN_BUILDER_MUTANT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static OMIT_ONE_REACHED_FILE_IN_BUILDER_MUTANT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static OMIT_ONE_ANCHOR_FILE_IN_BUILDER_MUTANT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Non-authoritative observation emitted by context construction.
///
/// This vocabulary deliberately contains identifiers and operation counts but
/// never source bytes. It is diagnostics/test data and is not serialized into
/// any canonical ReviewGraphen record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextBuildEffect {
    GraphIndexLookup {
        index: &'static str,
        key: StableId,
    },
    DenominatorCommitmentLookup,
    CandidateMetadataVisit {
        artifact_id: StableId,
    },
    CandidateMaterialized {
        artifact_id: StableId,
    },
    SourceBytesRequested {
        artifact_id: StableId,
    },
    SourceSubmitted {
        artifact_id: StableId,
    },
    SubjectOutcome {
        endpoint_id: StableId,
        submitted: bool,
    },
    LimitFailure {
        operation: &'static str,
        limit: usize,
        observed: usize,
    },
    FullArtifactScan,
    FullRelationScan,
}

/// Injected read-only effect observer. Implementations cannot influence
/// selection and never receive source content.
pub trait ContextBuildProbe: std::fmt::Debug + Send + Sync {
    fn observe(&self, effect: ContextBuildEffect);
}

/// Thread-safe ordered recorder suitable for runtime integration tests.
#[derive(Clone, Debug, Default)]
pub struct ContextBuildTrace(Arc<Mutex<Vec<ContextBuildEffect>>>);

impl ContextBuildTrace {
    #[must_use]
    pub fn snapshot(&self) -> Vec<ContextBuildEffect> {
        self.0.lock().expect("context trace mutex poisoned").clone()
    }
}

impl ContextBuildProbe for ContextBuildTrace {
    fn observe(&self, effect: ContextBuildEffect) {
        self.0
            .lock()
            .expect("context trace mutex poisoned")
            .push(effect);
    }
}

fn probe(probe: &Option<Arc<dyn ContextBuildProbe>>, effect: ContextBuildEffect) {
    if let Some(probe) = probe {
        probe.observe(effect);
    }
}

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

/// Fixed subject-first window policy for the v2 generic review path.
///
/// This is deliberately a zero-sized closed DTO: its only admitted value is
/// the exact ADR 0038 policy body below.  Keeping the body literal avoids a
/// serializer-dependent change to a policy identity that is part of every v2
/// projection hash.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContextSubjectWindowsPolicyV2;

impl ContextSubjectWindowsPolicyV2 {
    pub const ID: &'static str = "context.subject_windows@2";
    pub const GOLDEN_HASH: &'static str =
        "sha256:7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26";

    #[must_use]
    pub const fn fixed() -> Self {
        Self
    }

    pub fn canonical_bytes(&self) -> ContextResult<Vec<u8>> {
        Ok(br#"{"anchors_per_file":1024,"assumptions":"empty","callees_depth":3,"callers_depth":2,"candidate_order":["subject_priority","distance","path_rank","artifact_id"],"canonical_envelope_bytes":786432,"contains_edges":1000000,"discovery_paths":20,"edge_kind_direction_order":["calls:forward","calls:reverse","contains:forward","contains:reverse","covers:forward","covers:reverse"],"excerpt_lines":400,"final_window_order":["source_artifact_id","start_line","end_line","window_id"],"included_files":64,"loss_reason_precedence":["missing_location","missing_source","giant_line","per_window_lines","per_window_bytes","per_file_window_cap","total_window_cap","total_excerpt_bytes","overlap_unmergeable","path_cap","test_cap","not_reached","included_file_cap","artifact_bytes_cap","total_resolved_bytes_cap"],"max_assumptions":64,"max_candidates":4096,"max_discovered_structural_ids":4096,"max_excerpt_bytes":262144,"max_losses":64,"max_resolved_artifact_bytes":1048576,"max_resolved_bytes":8388608,"max_string_bytes":16384,"max_total_excerpt_bytes":1048576,"max_unknowns":64,"obligations_per_envelope":1,"policy_id":"context.subject_windows@2","related_tests":10,"relation_scan":1000000,"seed_fields":["source_ids","target_refs","context_ids"],"source_candidate_denominator":"all_accepted_file_artifacts_with_exact_snapshot_source_registration_closure","subject_endpoints":2,"subject_order":["callee","caller"],"support_anchor_denominator":"reached_range_bearing_accepted_artifacts_with_exact_path_reverse_contains_file","unknown_reason_ids":["unresolved_invariant_scope","unresolved_relation_endpoint","unresolved_review_context_member","unresolved_seed_reference"],"window_candidate_order":["priority","role","source_artifact_id","start_line","end_line","owner_id"],"window_merge":"same_source_overlap_or_adjacent_if_union_within_per_window_bounds","windows_per_envelope":8,"windows_per_file":4}"#.to_vec())
    }

    pub fn hash(&self) -> ContentHash {
        ContentHash::sha256(&self.canonical_bytes().expect("fixed v2 policy bytes"))
    }
}

impl Serialize for ContextSubjectWindowsPolicyV2 {
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

impl<'de> Deserialize<'de> for ContextSubjectWindowsPolicyV2 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FixedPolicyVisitor;
        impl<'de> serde::de::Visitor<'de> for FixedPolicyVisitor {
            type Value = ContextSubjectWindowsPolicyV2;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("the exact context.subject_windows@2 policy object")
            }

            fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut object = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if object.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate context policy field {key}"
                        )));
                    }
                    object.insert(key, map.next_value()?);
                }
                let policy = ContextSubjectWindowsPolicyV2::fixed();
                let expected: serde_json::Value = serde_json::from_slice(
                    &policy.canonical_bytes().map_err(serde::de::Error::custom)?,
                )
                .map_err(serde::de::Error::custom)?;
                if serde_json::Value::Object(object) != expected {
                    return Err(serde::de::Error::custom(
                        "context policy is not the fixed subject-window v2 DTO",
                    ));
                }
                Ok(policy)
            }
        }
        deserializer.deserialize_map(FixedPolicyVisitor)
    }
}

/// Fixed repository-scale subject-window policy. This is a distinct wire
/// family from [`ContextSubjectWindowsPolicyV2`]; neither policy decodes the
/// other's canonical bytes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContextSubjectWindowsPolicyV3;

impl ContextSubjectWindowsPolicyV3 {
    pub const ID: &'static str = "context.subject_windows@3";
    pub const GOLDEN_HASH: &'static str =
        "sha256:932bfa18c5d286c63196366d6d2dc1aaf402f50baa1f1ab5f075b1007be55dd8";

    #[must_use]
    pub const fn fixed() -> Self {
        Self
    }

    pub fn canonical_bytes(&self) -> ContextResult<Vec<u8>> {
        Ok(br#"{"accepted_file_denominator_bound":"request.ingest.max_files","anchors_per_file":1024,"assumptions":"empty","callees_depth":3,"callers_depth":2,"candidate_order":["subject_priority","distance","path_rank","artifact_id"],"canonical_envelope_bytes":786432,"contains_edges":1000000,"discovery_paths":20,"edge_kind_direction_order":["calls:forward","calls:reverse","contains:forward","contains:reverse","covers:forward","covers:reverse"],"excerpt_lines":400,"final_window_order":["source_artifact_id","start_line","end_line","window_id"],"included_files":64,"latent_cardinality":"known_zero_or_unknown_with_qualification_ids","loss_reason_precedence":["missing_location","missing_source","giant_line","per_window_lines","per_window_bytes","per_file_window_cap","total_window_cap","total_excerpt_bytes","overlap_unmergeable","path_cap","test_cap","not_reached","included_file_cap","artifact_bytes_cap","total_resolved_bytes_cap"],"materialized_source_denominator":"subject_file_ids_union_reached_file_ids","max_assumptions":64,"max_discovered_structural_ids":4096,"max_excerpt_bytes":262144,"max_materialized_source_candidates":4096,"max_resolved_artifact_bytes":1048576,"max_resolved_bytes":8388608,"max_string_bytes":16384,"max_subject_losses":2,"max_support_loss_summaries":15,"max_total_excerpt_bytes":1048576,"max_unknowns":64,"obligations_per_envelope":1,"policy_id":"context.subject_windows@3","related_tests":10,"relation_scan":1000000,"seed_fields":["source_ids","target_refs","context_ids"],"source_candidate_denominator":"all_accepted_file_ids_known_count_and_sorted_id_set_sha256","subject_endpoints":2,"subject_order":["callee","caller"],"support_anchor_denominator":"reached_range_bearing_exact_path_anchor_ids_known_count_and_sorted_id_set_sha256","support_loss_summary":"reason_known_count_and_sorted_anchor_id_set_sha256","unknown_reason_ids":["unresolved_invariant_scope","unresolved_relation_endpoint","unresolved_review_context_member","unresolved_seed_reference"],"window_candidate_order":["priority","role","source_artifact_id","start_line","end_line","owner_id"],"window_merge":"same_source_overlap_or_adjacent_if_union_within_per_window_bounds","windows_per_envelope":8,"windows_per_file":4}"#.to_vec())
    }

    pub fn hash(&self) -> ContentHash {
        ContentHash::sha256(&self.canonical_bytes().expect("fixed v3 policy bytes"))
    }
}

impl Serialize for ContextSubjectWindowsPolicyV3 {
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

impl<'de> Deserialize<'de> for ContextSubjectWindowsPolicyV3 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FixedPolicyVisitor;
        impl<'de> serde::de::Visitor<'de> for FixedPolicyVisitor {
            type Value = ContextSubjectWindowsPolicyV3;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("the exact context.subject_windows@3 policy object")
            }

            fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut object = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if object.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate context policy field {key}"
                        )));
                    }
                    object.insert(key, map.next_value()?);
                }
                let policy = ContextSubjectWindowsPolicyV3::fixed();
                let expected: serde_json::Value = serde_json::from_slice(
                    &policy.canonical_bytes().map_err(serde::de::Error::custom)?,
                )
                .map_err(serde::de::Error::custom)?;
                if serde_json::Value::Object(object) != expected {
                    return Err(serde::de::Error::custom(
                        "context policy is not the fixed subject-window v3 DTO",
                    ));
                }
                Ok(policy)
            }
        }
        deserializer.deserialize_map(FixedPolicyVisitor)
    }
}

/// Typed failure emitted by the context protocol. Constructors are private so
/// callers cannot manufacture an apparently policy-derived failure.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContextError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    SubjectBinding(#[from] ContextSubjectBindingErrorV2),
    #[error(transparent)]
    SubjectWindowsV3Validation(#[from] ContextSubjectWindowsV3ValidationError),
    #[error(
        "context v3 observed unknowns exceed {limit}: observed {observed}, set digest {sorted_unknown_set_sha256}"
    )]
    V3UnknownOverflow {
        limit: usize,
        observed: usize,
        sorted_unknown_set_sha256: ContentHash,
    },
    #[error("context session protocol violation: {0}")]
    Protocol(&'static str),
}

/// Closed read-only failures for a canonical `context.subject_windows@3`
/// value. These errors validate a sealed projection and never admit it into
/// aggregate state.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContextSubjectWindowsV3ValidationError {
    #[error("context value is not context.subject_windows@3 (observed {observed:?})")]
    WrongPolicy { observed: Option<String> },
    #[error("malformed context.subject_windows@3 value: {message}")]
    Malformed { message: String },
    #[error("context.subject_windows@3 policy hash mismatch")]
    PolicyHash,
    #[error("invalid context.subject_windows@3 denominator `{name}`")]
    Denominator { name: &'static str },
    #[error("invalid context.subject_windows@3 latent cardinality")]
    LatentCardinality,
    #[error("invalid context.subject_windows@3 subject outcomes")]
    SubjectOutcomes,
    #[error("invalid context.subject_windows@3 materialized sources or windows")]
    Windows,
    #[error("invalid context.subject_windows@3 support-loss partition")]
    SupportPartition,
    #[error("context.subject_windows@3 projection hash mismatch")]
    ProjectionHash,
    #[error("context.subject_windows@3 context ID mismatch")]
    ContextId,
    #[error("context.subject_windows@3 does not match its trusted validation basis")]
    BasisMismatch,
}

/// Closed failures for binding an explicit caller/callee pair to one accepted
/// ADR-0038 D obligation. These are input-contract failures, never projection
/// loss records.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContextSubjectBindingErrorV2 {
    #[error("subject-window obligation has rule `{observed}`, expected the D rule")]
    WrongRule { observed: String },
    #[error("subject-window obligation has property `{observed}`, expected the D property")]
    WrongProperty { observed: String },
    #[error("subject-window obligation has target kind `{observed}`, expected `relation`")]
    WrongTargetKind { observed: String },
    #[error(
        "subject-window obligation must have exactly one relation target (observed {observed})"
    )]
    ObligationTargetCardinality { observed: usize },
    #[error("subject-window obligation target `{relation_id}` is not an accepted relation")]
    TargetRelationNotAccepted { relation_id: StableId },
    #[error(
        "accepted target relation `{relation_id}` must have exactly one target (observed {observed})"
    )]
    RelationTargetCardinality {
        relation_id: StableId,
        observed: usize,
    },
    #[error("accepted target relation endpoint `{artifact_id}` is not an accepted artifact")]
    RelationEndpointNotAccepted { artifact_id: StableId },
    #[error("provided {role} artifact `{artifact_id}` is not accepted")]
    ProvidedEndpointNotAccepted {
        role: &'static str,
        artifact_id: StableId,
    },
    #[error("provided caller `{observed}` does not match relation source `{expected}`")]
    CallerMismatch {
        expected: StableId,
        observed: StableId,
    },
    #[error("provided callee `{observed}` does not match relation target `{expected}`")]
    CalleeMismatch {
        expected: StableId,
        observed: StableId,
    },
}
type ContextResult<T> = std::result::Result<T, ContextError>;
pub(crate) fn context_domain_error(error: ContextError) -> DomainError {
    match error {
        ContextError::Domain(error) => error,
        ContextError::SubjectBinding(error) => DomainError::Validation(error.to_string()),
        ContextError::SubjectWindowsV3Validation(error) => {
            DomainError::Validation(error.to_string())
        }
        error @ ContextError::V3UnknownOverflow { .. } => {
            DomainError::Validation(error.to_string())
        }
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

    /// Portable dynamic allocation made by `self.clone()`. Unlike
    /// [`Self::allocated_bytes`], this models `Clone`'s freshly allocated
    /// buffers (vector capacity and string backing equal the source length),
    /// so replay reducers can charge a cloned context without inheriting spare
    /// capacity from a decoded event DTO.
    #[allow(dead_code)] // Consumed by the V5 replay reducer's clone accounting.
    pub(crate) fn cloned_allocated_bytes(&self) -> usize {
        fn id(value: &StableId) -> usize {
            value.as_str().len()
        }
        fn hash(value: &ContentHash) -> usize {
            value.as_str().len()
        }
        fn ids(values: &BTreeSet<StableId>) -> usize {
            values
                .len()
                .saturating_mul(std::mem::size_of::<StableId>())
                .saturating_add(values.iter().map(id).sum::<usize>())
        }
        fn strings(values: &BTreeSet<String>) -> usize {
            values
                .len()
                .saturating_mul(std::mem::size_of::<String>())
                .saturating_add(values.iter().map(String::len).sum::<usize>())
        }
        let included = self.included_sources.iter().fold(
            self.included_sources
                .len()
                .saturating_mul(std::mem::size_of::<SourceArtifactRef>()),
            |total, source| {
                total
                    .saturating_add(id(&source.registration_id))
                    .saturating_add(id(&source.artifact_id))
                    .saturating_add(hash(&source.content_hash))
                    .saturating_add(hash(&source.cas_hash))
                    .saturating_add(hash(&source.excerpt_hash))
            },
        );
        let excluded = self.excluded_sources.iter().fold(
            self.excluded_sources
                .len()
                .saturating_mul(std::mem::size_of::<ExcludedSourceRef>()),
            |total, source| total.saturating_add(id(&source.artifact_id)),
        );
        let unknowns = self.unknowns.iter().fold(
            self.unknowns
                .len()
                .saturating_mul(std::mem::size_of::<EnvelopeUnknown>()),
            |total, unknown| {
                total
                    .saturating_add(unknown.description.len())
                    .saturating_add(ids(&unknown.source_ids))
            },
        );
        let losses = self.losses.iter().fold(
            self.losses
                .len()
                .saturating_mul(std::mem::size_of::<EnvelopeLoss>()),
            |total, loss| {
                total
                    .saturating_add(loss.description.len())
                    .saturating_add(strings(&loss.affected_properties))
                    .saturating_add(ids(&loss.source_ids))
            },
        );
        id(&self.id)
            .saturating_add(ids(&self.obligation_ids))
            .saturating_add(id(&self.snapshot_id))
            .saturating_add(self.projection_policy_version.len())
            .saturating_add(hash(&self.context_policy_hash))
            .saturating_add(ids(&self.candidate_source_ids))
            .saturating_add(included)
            .saturating_add(ids(&self.normalized_included_source_ids))
            .saturating_add(excluded)
            .saturating_add(unknowns)
            .saturating_add(
                self.assumptions
                    .len()
                    .saturating_mul(std::mem::size_of::<String>())
                    .saturating_add(self.assumptions.iter().map(String::len).sum::<usize>()),
            )
            .saturating_add(losses)
            .saturating_add(hash(&self.projection_hash))
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
    #[cfg(test)]
    finish_actual_bytes: u64,
}
impl BuiltContextProjection {
    pub fn envelope(&self) -> &ReviewContextEnvelope {
        &self.envelope
    }

    #[allow(dead_code)] // consumed by event.rs in the next implementation unit.
    pub(crate) fn into_parts(self) -> (ReviewContextEnvelope, ContextProjectionAdmission) {
        (self.envelope, self.admission)
    }

    #[cfg(test)]
    fn finish_actual_bytes(&self) -> u64 {
        self.finish_actual_bytes
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
    excluded: Vec<ExcludedSourceRef>,
    resolved_bytes: u64,
    excerpt_bytes: usize,
    /// Sealed category-by-category working reservation. Its unused capacity
    /// remains live until `finish`, so byte-dependent branches cannot borrow
    /// capacity from an unrelated category.
    #[allow(dead_code)] // Capacity is the working contract; inspected by tests.
    working_reservation: ContextWorkingReservation,
    session_digest: ContentHash,
    #[allow(dead_code)] // Exposed to reservation-accounting tests only.
    reservation: ContextResourceOracle,
}

/// CAS-free resource reservation for rebuilding one live context projection.
///
/// The projection event is deliberately not an input: its included-source
/// list is output from `prepare_context`, not authority for choosing a CAS
/// object.  `max_source_cas_bytes` is therefore the largest source which may
/// survive the metadata-only source-request gate.  The byte-dependent excerpt
/// outcome is intentionally not guessed before that object is read.
#[allow(dead_code)] // Consumed by the V5 D2 pre-CAS replay gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ContextResourceOracle {
    candidate_count: u64,
    max_source_cas_bytes: u64,
    session_retained_bytes: u64,
    session_working_bytes: u64,
    layout: ContextReservationLayout,
    typed_capacities: ContextTypedCapacities,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ContextReservationLayout {
    candidate_clone_backing: u64,
    source_clone_backing: u64,
    request_backing: u64,
    anchor_and_discovery_backing: u64,
    partition_and_loss_backing: u64,
    finish_and_admission_backing: u64,
    excerpt_backing: u64,
    canonical_backing: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ContextTypedCapacities {
    candidates: u64,
    anchors: u64,
    unknowns: u64,
    included: u64,
    excluded: u64,
    losses: u64,
    discovery_vectors: u64,
}

impl ContextTypedCapacities {
    fn requested(candidate_count: u64) -> ContextResult<Self> {
        Ok(Self {
            candidates: candidate_count,
            anchors: checked_resource_mul(
                candidate_count,
                u64::try_from(MAX_ANCHORS).unwrap_or(u64::MAX),
                "context requested anchor capacity",
            )?,
            unknowns: 64,
            included: u64::try_from(MAX_FILES).unwrap_or(u64::MAX),
            excluded: candidate_count,
            losses: 64,
            discovery_vectors: checked_resource_mul(
                u64::try_from(MAX).unwrap_or(u64::MAX),
                u64::try_from(
                    size_of::<StableId>() * 4
                        + size_of::<(StableId, (usize, Vec<u8>, Vec<StableId>))>(),
                )
                .unwrap_or(u64::MAX),
                "context requested discovery vector capacity",
            )?,
        })
    }
}

impl ContextReservationLayout {
    fn arena_bytes(self) -> ContextResult<u64> {
        [
            self.candidate_clone_backing,
            self.source_clone_backing,
            self.request_backing,
            self.anchor_and_discovery_backing,
            self.partition_and_loss_backing,
            self.finish_and_admission_backing,
            self.excerpt_backing,
            self.canonical_backing,
        ]
        .into_iter()
        .try_fold(0_u64, |total, value| {
            checked_resource_add(total, value, "context reservation arena bytes")
        })
    }

    #[cfg(test)]
    fn covers(self, actual: Self) -> bool {
        self.candidate_clone_backing >= actual.candidate_clone_backing
            && self.source_clone_backing >= actual.source_clone_backing
            && self.request_backing >= actual.request_backing
            && self.anchor_and_discovery_backing >= actual.anchor_and_discovery_backing
            && self.partition_and_loss_backing >= actual.partition_and_loss_backing
            && self.finish_and_admission_backing >= actual.finish_and_admission_backing
            && self.excerpt_backing >= actual.excerpt_backing
            && self.canonical_backing >= actual.canonical_backing
    }
}

#[derive(Debug)]
struct ContextWorkingReservation {
    candidate_clone_backing: Vec<u8>,
    source_clone_backing: Vec<u8>,
    request_backing: Vec<u8>,
    anchor_and_discovery_backing: Vec<u8>,
    partition_and_loss_backing: Vec<u8>,
    finish_and_admission_backing: Vec<u8>,
    excerpt_backing: Vec<u8>,
    canonical_backing: Vec<u8>,
    #[allow(dead_code)] // Checked by the independent actual walker.
    sealed_allocated_bytes: u64,
}

impl ContextWorkingReservation {
    fn seal(layout: ContextReservationLayout) -> ContextResult<Self> {
        fn arena(bytes: u64, operation: &'static str) -> ContextResult<Vec<u8>> {
            let bytes = usize::try_from(bytes)
                .map_err(|_| incomplete(operation, usize::MAX, usize::MAX))?;
            let mut arena = Vec::new();
            reserve_exact(&mut arena, bytes, operation)?;
            Ok(arena)
        }
        let value = Self {
            candidate_clone_backing: arena(
                layout.candidate_clone_backing,
                "context candidate clone arena",
            )?,
            source_clone_backing: arena(layout.source_clone_backing, "context source clone arena")?,
            request_backing: arena(layout.request_backing, "context request arena")?,
            anchor_and_discovery_backing: arena(
                layout.anchor_and_discovery_backing,
                "context anchor/discovery arena",
            )?,
            partition_and_loss_backing: arena(
                layout.partition_and_loss_backing,
                "context partition/loss arena",
            )?,
            finish_and_admission_backing: arena(
                layout.finish_and_admission_backing,
                "context finish/admission arena",
            )?,
            excerpt_backing: arena(layout.excerpt_backing, "context excerpt arena")?,
            canonical_backing: arena(layout.canonical_backing, "context canonical arena")?,
            sealed_allocated_bytes: 0,
        };
        let sealed_allocated_bytes = value.allocated_bytes()?;
        Ok(Self {
            sealed_allocated_bytes,
            ..value
        })
    }

    fn allocated_bytes(&self) -> ContextResult<u64> {
        [
            self.candidate_clone_backing.capacity(),
            self.source_clone_backing.capacity(),
            self.request_backing.capacity(),
            self.anchor_and_discovery_backing.capacity(),
            self.partition_and_loss_backing.capacity(),
            self.finish_and_admission_backing.capacity(),
            self.excerpt_backing.capacity(),
            self.canonical_backing.capacity(),
        ]
        .into_iter()
        .try_fold(0_u64, |total, value| {
            checked_resource_add(
                total,
                u64::try_from(value).unwrap_or(u64::MAX),
                "context sealed reservation bytes",
            )
        })
    }

    fn sealed_layout(&self) -> ContextReservationLayout {
        let capacity = |value: &Vec<u8>| u64::try_from(value.capacity()).unwrap_or(u64::MAX);
        ContextReservationLayout {
            candidate_clone_backing: capacity(&self.candidate_clone_backing),
            source_clone_backing: capacity(&self.source_clone_backing),
            request_backing: capacity(&self.request_backing),
            anchor_and_discovery_backing: capacity(&self.anchor_and_discovery_backing),
            partition_and_loss_backing: capacity(&self.partition_and_loss_backing),
            finish_and_admission_backing: capacity(&self.finish_and_admission_backing),
            excerpt_backing: capacity(&self.excerpt_backing),
            canonical_backing: capacity(&self.canonical_backing),
        }
    }
}

#[allow(dead_code)] // The event-side gate is wired in a separate implementation unit.
impl ContextResourceOracle {
    pub(crate) const fn candidate_count(self) -> u64 {
        self.candidate_count
    }

    pub(crate) const fn max_source_cas_bytes(self) -> u64 {
        self.max_source_cas_bytes
    }

    pub(crate) const fn session_retained_bytes(self) -> u64 {
        self.session_retained_bytes
    }

    pub(crate) const fn session_working_bytes(self) -> u64 {
        self.session_working_bytes
    }
}

fn reserve_exact<T>(
    values: &mut Vec<T>,
    additional: usize,
    operation: &'static str,
) -> ContextResult<()> {
    values
        .try_reserve_exact(additional)
        .map_err(|_| incomplete(operation, additional, additional))?;
    Ok(())
}

fn value_clone_slots(value: &serde_json::Value) -> ContextResult<u64> {
    let slot = |count: usize, size: usize| {
        checked_resource_mul(
            u64::try_from(count).unwrap_or(u64::MAX),
            u64::try_from(size).unwrap_or(u64::MAX),
            "context artifact attribute slots",
        )
    };
    match value {
        serde_json::Value::Array(values) => values.iter().try_fold(
            slot(values.len(), size_of::<serde_json::Value>())?,
            |total, value| {
                checked_resource_add(
                    total,
                    value_clone_slots(value)?,
                    "context artifact attribute slots",
                )
            },
        ),
        serde_json::Value::Object(values) => values.iter().try_fold(
            slot(values.len(), size_of::<(String, serde_json::Value)>())?,
            |total, (key, value)| {
                checked_resource_add(
                    checked_resource_add(
                        total,
                        u64::try_from(key.len()).unwrap_or(u64::MAX),
                        "context artifact attribute slots",
                    )?,
                    value_clone_slots(value)?,
                    "context artifact attribute slots",
                )
            },
        ),
        serde_json::Value::String(value) => Ok(u64::try_from(value.len()).unwrap_or(u64::MAX)),
        _ => Ok(0),
    }
}

/// Allocation-free serialized size is a conservative backing arena for every
/// private provenance string. Attribute container slots are added separately
/// because a compact JSON representation can be smaller than `Value` slots.
fn serialized_size(value: &impl Serialize) -> ContextResult<u64> {
    struct Counter(u64);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
                .ok_or_else(|| std::io::Error::other("context artifact JSON size overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value)
        .map_err(|error| DomainError::Json(error.to_string()))?;
    Ok(counter.0)
}

fn artifact_clone_backing_reservation(artifact: &Artifact) -> ContextResult<u64> {
    let serialized = serialized_size(artifact)?;
    artifact
        .attributes
        .values()
        .try_fold(serialized, |total, value| {
            checked_resource_add(
                total,
                value_clone_slots(value)?,
                "context candidate clone backing",
            )
        })
}

fn source_clone_backing_reservation(source: &SnapshotSourceRecordEntry) -> ContextResult<u64> {
    [
        source.artifact_id().as_str().len(),
        source.path().len(),
        source.content_hash().as_str().len(),
        source.registration_id().as_str().len(),
        source.cas_hash().as_str().len(),
    ]
    .into_iter()
    .try_fold(0_u64, |total, value| {
        checked_resource_add(
            total,
            u64::try_from(value).unwrap_or(u64::MAX),
            "context source clone backing",
        )
    })
}

/// Prediction-side owned backing for the aggregate obligation that will be
/// cloned into the session. Container slots and every nested backing buffer
/// come from the type's ownership accessor; wire size is never a resource
/// proxy.
fn obligation_clone_backing_reservation(obligation: &Obligation) -> ContextResult<u64> {
    u64::try_from(obligation.allocated_bytes())
        .map_err(|_| incomplete("context obligation clone backing", usize::MAX, usize::MAX).into())
}

/// Computes the CAS and owned-session reservation before a context session,
/// a resolver buffer, or any discovery collection is materialized.
///
/// This is a counting visitor over the same accepted snapshot-source closure
/// that `candidates` clones into the builder.  It intentionally reserves for
/// the complete bounded discovery/output domain: source-byte-dependent
/// `giant_line` and excerpt exclusions cannot safely shrink a pre-CAS
/// reservation.  A caller can reject `working_bytes` before its first source
/// read even if a persisted envelope omits, adds, or reorders sources.
#[allow(dead_code)] // The event-side gate is wired in a separate implementation unit.
pub(crate) fn context_resource_oracle(
    aggregate: &ReviewAggregate,
    obligation_id: &StableId,
) -> ContextResult<ContextResourceOracle> {
    let obligation =
        aggregate
            .obligation(obligation_id)
            .ok_or_else(|| DomainError::DanglingReference {
                owner: "context resource oracle",
                owner_id: obligation_id.clone(),
                reference: obligation_id.clone(),
            })?;
    let program = aggregate.program();
    let snapshot_id = program.snapshot_id();
    if obligation.version().snapshot() != snapshot_id {
        return Err(DomainError::Validation(
            "context obligation must bind aggregate snapshot".to_owned(),
        )
        .into());
    }
    text(obligation.property_id())?;
    let sources = aggregate.snapshot_sources_for(snapshot_id).ok_or_else(|| {
        DomainError::Validation("missing exact snapshot source closure".to_owned())
    })?;

    let mut count = 0_usize;
    let mut max_source = 0_u64;
    let mut candidate_clone_backing = 0_u64;
    let mut source_clone_backing = 0_u64;
    let mut candidate_id_backing = 0_u64;
    let mut max_candidate_id_backing = 0_u64;
    let mut request_backing = 0_u64;
    visit_candidate_metadata(
        aggregate,
        program,
        sources,
        snapshot_id,
        |artifact, source, size| {
            count = count
                .checked_add(1)
                .ok_or_else(|| incomplete("context resource candidate count", MAX, usize::MAX))?;
            let artifact_backing = artifact_clone_backing_reservation(artifact)?;
            candidate_clone_backing = checked_resource_add(
                candidate_clone_backing,
                checked_resource_add(
                    artifact_backing,
                    u64::try_from(artifact.id.as_str().len()).unwrap_or(u64::MAX),
                    "context candidate rank ID backing",
                )?,
                "context candidate clone backing",
            )?;
            let source_backing = source_clone_backing_reservation(source)?;
            source_clone_backing = checked_resource_add(
                source_clone_backing,
                source_backing,
                "context source clone backing",
            )?;
            let artifact_id = u64::try_from(artifact.id.as_str().len()).unwrap_or(u64::MAX);
            candidate_id_backing = checked_resource_add(
                candidate_id_backing,
                artifact_id,
                "context candidate ID backing",
            )?;
            max_candidate_id_backing = max_candidate_id_backing.max(artifact_id);
            let request = [
                artifact.id.as_str().len(),
                source.registration_id().as_str().len(),
                source.content_hash().as_str().len(),
                source.cas_hash().as_str().len(),
                71,
            ]
            .into_iter()
            .try_fold(0_u64, |total, value| {
                checked_resource_add(
                    total,
                    u64::try_from(value).unwrap_or(u64::MAX),
                    "context request backing",
                )
            })?;
            request_backing = request_backing.max(request);
            // These are exactly the metadata-only exclusions applied before a
            // `ContextSourceRequest` is emitted.  A later candidate can always
            // be the first request after byte-dependent exclusions, so do not use
            // an envelope's historical included list to narrow this maximum.
            if size <= MAX_FILE_BYTES {
                max_source = max_source.max(size);
            }
            Ok(())
        },
    )?;
    let candidate_count = u64::try_from(count)
        .map_err(|_| incomplete("context resource candidate count", MAX, usize::MAX))?;
    let mut max_structural_id_backing = max_candidate_id_backing;
    for artifact in program.artifacts() {
        max_structural_id_backing = max_structural_id_backing
            .max(u64::try_from(artifact.id.as_str().len()).unwrap_or(u64::MAX));
    }
    for relation in program.relations() {
        max_structural_id_backing = max_structural_id_backing
            .max(u64::try_from(relation.id.as_str().len()).unwrap_or(u64::MAX))
            .max(u64::try_from(relation.source_id.as_str().len()).unwrap_or(u64::MAX));
        for target in &relation.target_ids {
            max_structural_id_backing = max_structural_id_backing
                .max(u64::try_from(target.as_str().len()).unwrap_or(u64::MAX));
        }
    }
    for context in program.contexts() {
        max_structural_id_backing = max_structural_id_backing
            .max(u64::try_from(context.id.as_str().len()).unwrap_or(u64::MAX));
        for member in &context.member_ids {
            max_structural_id_backing = max_structural_id_backing
                .max(u64::try_from(member.as_str().len()).unwrap_or(u64::MAX));
        }
    }
    for invariant in program.invariants() {
        max_structural_id_backing = max_structural_id_backing
            .max(u64::try_from(invariant.id.as_str().len()).unwrap_or(u64::MAX));
        for scope in &invariant.scope_ids {
            max_structural_id_backing = max_structural_id_backing
                .max(u64::try_from(scope.as_str().len()).unwrap_or(u64::MAX));
        }
    }
    candidate_clone_backing = [
        obligation_clone_backing_reservation(obligation)?,
        u64::try_from(snapshot_id.as_str().len()).unwrap_or(u64::MAX),
        71,
        71,
    ]
    .into_iter()
    .try_fold(candidate_clone_backing, |total, value| {
        checked_resource_add(total, value, "context session identity backing")
    })?;
    let anchor_ids = checked_resource_mul(
        checked_resource_mul(
            candidate_count,
            u64::try_from(MAX_ANCHORS).unwrap_or(u64::MAX),
            "context anchor ID backing",
        )?,
        max_structural_id_backing,
        "context anchor ID backing",
    )?;
    let discovery_ids = checked_resource_mul(
        checked_resource_mul(
            u64::try_from(MAX).unwrap_or(u64::MAX),
            9,
            "context discovery ID backing",
        )?,
        max_structural_id_backing.max(1),
        "context discovery ID backing",
    )?;
    let partition_backing =
        checked_resource_mul(candidate_id_backing, 4, "context partition/loss backing")?;
    let finish_backing = checked_resource_add(
        checked_resource_mul(candidate_id_backing, 3, "context finish/admission backing")?,
        checked_resource_mul(source_clone_backing, 2, "context finish/admission backing")?,
        "context finish/admission backing",
    )?;
    let layout = ContextReservationLayout {
        candidate_clone_backing,
        source_clone_backing,
        request_backing,
        anchor_and_discovery_backing: checked_resource_add(
            anchor_ids,
            discovery_ids,
            "context anchor/discovery backing",
        )?,
        partition_and_loss_backing: partition_backing,
        finish_and_admission_backing: finish_backing,
        excerpt_backing: u64::try_from(MAX_TOTAL_EXCERPT).unwrap_or(u64::MAX),
        canonical_backing: u64::try_from(MAX_BODY).unwrap_or(u64::MAX),
    };
    let typed_capacities = ContextTypedCapacities::requested(candidate_count)?;
    let session_retained_bytes =
        context_resource_formula(candidate_count, max_source, layout, typed_capacities)?;
    let session_working_bytes = checked_resource_add(
        checked_resource_add(
            session_retained_bytes,
            max_source,
            "context resource working bytes",
        )?,
        u64::try_from(MAX_BODY)
            .map_err(|_| incomplete("context resource working bytes", usize::MAX, usize::MAX))?,
        "context resource working bytes",
    )?;
    Ok(ContextResourceOracle {
        candidate_count,
        max_source_cas_bytes: max_source,
        session_retained_bytes,
        session_working_bytes,
        layout,
        typed_capacities,
    })
}

/// Portable owned-byte formula.  It charges the fixed session plus all
/// bounded containers which can coexist while discovery, resolution, and
/// final envelope canonicalization overlap.  These are reservation slots,
/// not allocator-private B-tree node headers.
fn checked_resource_add(left: u64, right: u64, operation: &'static str) -> ContextResult<u64> {
    left.checked_add(right)
        .ok_or_else(|| incomplete(operation, usize::MAX, usize::MAX).into())
}

fn checked_resource_mul(left: u64, right: u64, operation: &'static str) -> ContextResult<u64> {
    left.checked_mul(right)
        .ok_or_else(|| incomplete(operation, usize::MAX, usize::MAX).into())
}

fn typed_vec_capacity_bytes<T>(capacity: usize, operation: &'static str) -> ContextResult<u64> {
    checked_resource_mul(
        u64::try_from(capacity).map_err(|_| incomplete(operation, usize::MAX, usize::MAX))?,
        u64::try_from(size_of::<T>()).map_err(|_| incomplete(operation, usize::MAX, usize::MAX))?,
        operation,
    )
}

fn context_resource_formula(
    candidate_count: u64,
    max_source: u64,
    layout: ContextReservationLayout,
    typed: ContextTypedCapacities,
) -> ContextResult<u64> {
    fn count(value: usize, operation: &'static str) -> ContextResult<u64> {
        u64::try_from(value).map_err(|_| incomplete(operation, usize::MAX, usize::MAX).into())
    }
    fn add(total: &mut u64, value: u64, operation: &'static str) -> ContextResult<()> {
        *total = checked_resource_add(*total, value, operation)?;
        Ok(())
    }

    if candidate_count > u64::try_from(MAX).unwrap_or(u64::MAX) {
        return Err(incomplete("context resource candidate count", MAX, usize::MAX).into());
    }
    if max_source > MAX_FILE_BYTES {
        return Err(incomplete(
            "context resource CAS source bytes",
            MAX_FILE_BYTES as usize,
            usize::MAX,
        )
        .into());
    }
    let discovered = u64::try_from(MAX).unwrap_or(u64::MAX);
    let mut total = count(size_of::<ContextBuildSession>(), "context resource session")?;
    // Candidate vector plus the complete discovery state (structural IDs,
    // predecessor keys/paths, containing-file map, rank/distance maps and
    // path/test partitions).  The traversal has nine predecessor domains per
    // discovered ID: calls 3+2, contains 1+1, covers 1+1.
    add(
        &mut total,
        checked_resource_mul(
            typed.candidates,
            count(size_of::<Candidate>(), "context resource candidate slots")?,
            "context resource candidate slots",
        )?,
        "context resource retained bytes",
    )?;
    add(
        &mut total,
        typed.discovery_vectors,
        "context resource discovery vector capacity",
    )?;
    add(
        &mut total,
        checked_resource_mul(
            typed.anchors,
            count(
                size_of::<(u64, u64, StableId)>(),
                "context resource anchor slots",
            )?,
            "context resource anchor slots",
        )?,
        "context resource retained bytes",
    )?;
    add(
        &mut total,
        checked_resource_mul(
            discovered,
            count(size_of::<StableId>(), "context resource structural IDs")?,
            "context resource structural IDs",
        )?,
        "context resource retained bytes",
    )?;
    add(
        &mut total,
        checked_resource_mul(
            discovered,
            count(
                size_of::<(u8, StableId, usize)>(),
                "context resource predecessor keys",
            )?
            .checked_mul(9)
            .ok_or_else(|| {
                incomplete("context resource predecessor keys", usize::MAX, usize::MAX)
            })?,
            "context resource predecessor keys",
        )?,
        "context resource retained bytes",
    )?;
    // Included/excluded refs, unknown/loss descriptors, and all normalized
    // excerpt bookkeeping can coexist with candidates until `finish`.
    add(
        &mut total,
        checked_resource_mul(
            typed.included,
            count(
                size_of::<SourceArtifactRef>(),
                "context resource included slots",
            )?,
            "context resource included slots",
        )?,
        "context resource retained bytes",
    )?;
    add(
        &mut total,
        checked_resource_mul(
            typed.excluded,
            count(
                size_of::<ExcludedSourceRef>(),
                "context resource excluded slots",
            )?,
            "context resource excluded slots",
        )?,
        "context resource retained bytes",
    )?;
    add(
        &mut total,
        checked_resource_mul(
            typed.unknowns,
            count(
                size_of::<EnvelopeUnknown>(),
                "context resource unknown slots",
            )?,
            "context resource unknown slots",
        )?,
        "context resource retained bytes",
    )?;
    add(
        &mut total,
        checked_resource_mul(
            typed.losses,
            count(size_of::<EnvelopeLoss>(), "context resource loss slots")?,
            "context resource loss slots",
        )?,
        "context resource retained bytes",
    )?;
    // Each category owns its sealed byte arena, and separately covers the
    // real typed backing that the builder cannot allocate from a byte arena.
    // Counting twice is intentional and category-local, never cross-category
    // slack: one copy is live arena ownership and one is the permitted actual
    // backing for that same category.
    add(
        &mut total,
        checked_resource_mul(
            layout.arena_bytes()?,
            2,
            "context reservation arena and covered backing",
        )?,
        "context resource retained bytes",
    )?;
    Ok(total)
}

/// Validates aggregate metadata then prepares a resolver-driven session.
pub fn prepare_context(
    aggregate: &ReviewAggregate,
    obligation_id: StableId,
) -> ContextResult<ContextBuildSession> {
    let mut reservation = context_resource_oracle(aggregate, &obligation_id)?;
    // Seal every arena before candidate/discovery materialization. Event
    // callers perform the scalar limit check before entering this function;
    // no CAS read occurs until the returned session emits a request.
    let working_reservation = ContextWorkingReservation::seal(reservation.layout)?;
    let sealed_layout = working_reservation.sealed_layout();
    reservation.layout = sealed_layout;
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
    if u64::try_from(candidates.len()).unwrap_or(u64::MAX) != reservation.candidate_count() {
        return Err(DomainError::Validation(
            "context resource candidate count drifted from builder metadata".to_owned(),
        )
        .into());
    }
    let (
        reached,
        structural,
        file_for,
        direct_files,
        distances,
        ranks,
        path_cap,
        test_cap,
        mut unknowns,
        discovery_vector_capacity,
    ) = discover(program, &obligation)?;
    let missing_unknown_capacity = 64_usize.saturating_sub(unknowns.capacity());
    reserve_exact(
        &mut unknowns,
        missing_unknown_capacity,
        "context resource unknown reservation",
    )?;
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
        // ADR 0016 defines a file's anchors as exactly the range-bearing
        // locations whose owner is reached *and* whose path equals that
        // file's path through the accepted reverse-`contains` closure. An
        // artifact that lies outside every file's containment closure
        // therefore satisfies no file's anchor definition: it is not an
        // anchor, and that is not a failure.
        //
        // ProgramSpace never promised that every range-bearing artifact has
        // a containing file. `contains` is one relation kind among many
        // (docs/03_conceptual_model.md), and M2's containment family is
        // exactly file -> module and module -> declared symbol
        // (docs/20_m2_ingestion_contract.md). A `state:*` write target is a
        // syntactic assignment target, not a declared symbol, so it is
        // deliberately linked by `writes` alone -- and since the change
        // family gained its own `contains` edges it became *reachable*
        // without ever becoming *contained*. Demanding a containment parent
        // here asked ProgramSpace to be shaped the way this one traversal
        // happened to expect. Discovery already tolerates the shape
        // everywhere else: a node with an empty containing-file closure
        // simply contributes no candidate file, no path-cap entry, and no
        // test-cap entry. Only the exact-path check below is a real
        // integrity claim -- an artifact located in one file must never be
        // contained by another.
        let Some(owners) = file_for.get(&artifact.id) else {
            continue;
        };
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
    let mut session = ContextBuildSession {
        snapshot_id,
        obligation,
        policy_hash,
        candidates,
        unknowns,
        index: 0,
        pending: None,
        included: {
            let mut included = Vec::new();
            reserve_exact(
                &mut included,
                MAX_FILES,
                "context resource included reservation",
            )?;
            included
        },
        excluded: {
            let mut excluded = Vec::new();
            reserve_exact(
                &mut excluded,
                usize::try_from(reservation.candidate_count()).unwrap_or(usize::MAX),
                "context resource excluded reservation",
            )?;
            excluded
        },
        resolved_bytes: 0,
        excerpt_bytes: 0,
        working_reservation,
        session_digest,
        reservation,
    };
    session.reservation.typed_capacities.discovery_vectors = discovery_vector_capacity;
    let typed_capacities = session.measured_typed_capacities()?;
    session.reservation.typed_capacities = typed_capacities;
    session.reservation.session_retained_bytes = context_resource_formula(
        session.reservation.candidate_count,
        session.reservation.max_source_cas_bytes,
        session.reservation.layout,
        typed_capacities,
    )?;
    session.reservation.session_working_bytes = checked_resource_add(
        checked_resource_add(
            session.reservation.session_retained_bytes,
            session.reservation.max_source_cas_bytes,
            "context sealed working bytes",
        )?,
        u64::try_from(MAX_BODY)
            .map_err(|_| incomplete("context sealed working bytes", usize::MAX, usize::MAX))?,
        "context sealed working bytes",
    )?;
    Ok(session)
}

impl ContextBuildSession {
    fn measured_typed_capacities(&self) -> ContextResult<ContextTypedCapacities> {
        let anchors = self.candidates.iter().try_fold(0_u64, |total, candidate| {
            checked_resource_add(
                total,
                u64::try_from(candidate.anchors.capacity()).unwrap_or(u64::MAX),
                "context sealed anchor capacity",
            )
        })?;
        Ok(ContextTypedCapacities {
            candidates: u64::try_from(self.candidates.capacity()).unwrap_or(u64::MAX),
            anchors,
            unknowns: u64::try_from(self.unknowns.capacity()).unwrap_or(u64::MAX),
            included: u64::try_from(self.included.capacity()).unwrap_or(u64::MAX),
            excluded: u64::try_from(self.excluded.capacity()).unwrap_or(u64::MAX),
            losses: 64,
            // Discovery vectors are transient; `discover` measured their
            // allocator capacities before dropping them and threaded the
            // byte total into this reservation.
            discovery_vectors: self.reservation.typed_capacities.discovery_vectors,
        })
    }

    /// Returns allocator-measured capacities after every category arena and
    /// typed vector has been sealed, and before the first source request can
    /// be emitted. Event replay may apply its exact/-1 limit at this seam
    /// without reading CAS bytes.
    #[allow(dead_code)] // Event integration is maintained in event.rs.
    pub(crate) const fn sealed_resource_oracle(&self) -> ContextResourceOracle {
        self.reservation
    }

    #[cfg(test)]
    fn actual_dynamic_layout(&self) -> ContextResult<ContextReservationLayout> {
        fn add(total: &mut u64, value: usize, operation: &'static str) -> ContextResult<()> {
            *total =
                checked_resource_add(*total, u64::try_from(value).unwrap_or(u64::MAX), operation)?;
            Ok(())
        }
        let mut actual = ContextReservationLayout {
            candidate_clone_backing: [
                u64::try_from(self.obligation.allocated_bytes()).unwrap_or(u64::MAX),
                u64::try_from(self.snapshot_id.allocated_bytes()).unwrap_or(u64::MAX),
                u64::try_from(self.policy_hash.allocated_bytes()).unwrap_or(u64::MAX),
                u64::try_from(self.session_digest.allocated_bytes()).unwrap_or(u64::MAX),
            ]
            .into_iter()
            .try_fold(0_u64, |total, value| {
                checked_resource_add(total, value, "context actual candidate backing")
            })?,
            ..ContextReservationLayout::default()
        };
        for candidate in &self.candidates {
            actual.candidate_clone_backing = checked_resource_add(
                actual.candidate_clone_backing,
                checked_resource_add(
                    artifact_clone_backing_reservation(&candidate.artifact)?,
                    u64::try_from(candidate.rank.3.allocated_bytes()).unwrap_or(u64::MAX),
                    "context actual candidate backing",
                )?,
                "context actual candidate backing",
            )?;
            actual.source_clone_backing = checked_resource_add(
                actual.source_clone_backing,
                source_clone_backing_reservation(&candidate.source)?,
                "context actual source backing",
            )?;
            for (_, _, owner) in &candidate.anchors {
                add(
                    &mut actual.anchor_and_discovery_backing,
                    owner.allocated_bytes(),
                    "context actual anchor backing",
                )?;
            }
        }
        if let Some(pending) = &self.pending {
            for value in [
                pending.artifact_id.allocated_bytes(),
                pending.registration_id.allocated_bytes(),
                pending.content_hash.allocated_bytes(),
                pending.cas_hash.allocated_bytes(),
                pending.digest.allocated_bytes(),
            ] {
                add(
                    &mut actual.request_backing,
                    value,
                    "context actual request backing",
                )?;
            }
        }
        for unknown in &self.unknowns {
            add(
                &mut actual.partition_and_loss_backing,
                unknown.description.capacity(),
                "context actual partition backing",
            )?;
            for source_id in &unknown.source_ids {
                add(
                    &mut actual.partition_and_loss_backing,
                    source_id.allocated_bytes(),
                    "context actual partition backing",
                )?;
            }
        }
        for source in &self.excluded {
            add(
                &mut actual.partition_and_loss_backing,
                source.artifact_id.allocated_bytes(),
                "context actual partition backing",
            )?;
        }
        for source in &self.included {
            for value in [
                source.registration_id.allocated_bytes(),
                source.artifact_id.allocated_bytes(),
                source.content_hash.allocated_bytes(),
                source.cas_hash.allocated_bytes(),
                source.excerpt_hash.allocated_bytes(),
            ] {
                add(
                    &mut actual.finish_and_admission_backing,
                    value,
                    "context actual finish backing",
                )?;
            }
        }
        Ok(actual)
    }

    /// Observable live reservation owned after `prepare_context` and before
    /// `finish`. This deliberately counts unused vector capacity: the fixed
    /// pre-CAS reservation, rather than byte-dependent projection content, is
    /// the working-memory contract.
    #[cfg(test)]
    fn realized_reservation_bytes(&self) -> ContextResult<u64> {
        fn add(total: &mut u64, value: usize) -> ContextResult<()> {
            *total = checked_resource_add(
                *total,
                u64::try_from(value).map_err(|_| {
                    incomplete("context realized reservation", usize::MAX, usize::MAX)
                })?,
                "context realized reservation",
            )?;
            Ok(())
        }
        let mut total = 0_u64;
        add(&mut total, size_of::<Self>())?;
        add(&mut total, self.snapshot_id.allocated_bytes())?;
        add(&mut total, self.obligation.allocated_bytes())?;
        add(&mut total, self.policy_hash.allocated_bytes())?;
        add(&mut total, self.session_digest.allocated_bytes())?;
        add(
            &mut total,
            self.candidates
                .capacity()
                .saturating_mul(size_of::<Candidate>()),
        )?;
        for candidate in &self.candidates {
            add(
                &mut total,
                usize::try_from(artifact_clone_backing_reservation(&candidate.artifact)?)
                    .unwrap_or(usize::MAX),
            )?;
            add(
                &mut total,
                usize::try_from(source_clone_backing_reservation(&candidate.source)?)
                    .unwrap_or(usize::MAX),
            )?;
            add(&mut total, candidate.rank.3.allocated_bytes())?;
            add(
                &mut total,
                candidate
                    .anchors
                    .capacity()
                    .saturating_mul(size_of::<(u64, u64, StableId)>()),
            )?;
            for (_, _, owner) in &candidate.anchors {
                add(&mut total, owner.allocated_bytes())?;
            }
        }
        add(
            &mut total,
            self.unknowns
                .capacity()
                .saturating_mul(size_of::<EnvelopeUnknown>()),
        )?;
        for unknown in &self.unknowns {
            add(&mut total, unknown.description.capacity())?;
            add(
                &mut total,
                unknown
                    .source_ids
                    .len()
                    .saturating_mul(size_of::<StableId>()),
            )?;
            for source_id in &unknown.source_ids {
                add(&mut total, source_id.allocated_bytes())?;
            }
        }
        add(
            &mut total,
            self.included
                .capacity()
                .saturating_mul(size_of::<SourceArtifactRef>()),
        )?;
        for source in &self.included {
            for value in [
                source.registration_id.allocated_bytes(),
                source.artifact_id.allocated_bytes(),
                source.content_hash.allocated_bytes(),
                source.cas_hash.allocated_bytes(),
                source.excerpt_hash.allocated_bytes(),
            ] {
                add(&mut total, value)?;
            }
        }
        add(
            &mut total,
            self.excluded
                .capacity()
                .saturating_mul(size_of::<ExcludedSourceRef>()),
        )?;
        for source in &self.excluded {
            add(&mut total, source.artifact_id.allocated_bytes())?;
        }
        if let Some(pending) = &self.pending {
            for value in [
                pending.artifact_id.allocated_bytes(),
                pending.registration_id.allocated_bytes(),
                pending.content_hash.allocated_bytes(),
                pending.cas_hash.allocated_bytes(),
                pending.digest.allocated_bytes(),
            ] {
                add(&mut total, value)?;
            }
        }
        add(
            &mut total,
            usize::try_from(self.working_reservation.allocated_bytes()?).unwrap_or(usize::MAX),
        )?;
        debug_assert_eq!(
            self.working_reservation.sealed_allocated_bytes,
            self.working_reservation.allocated_bytes()?
        );
        debug_assert!(
            self.reservation
                .layout
                .covers(self.actual_dynamic_layout()?)
        );
        Ok(total)
    }

    #[cfg(test)]
    fn reservation(&self) -> ContextResourceOracle {
        self.reservation
    }

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
            || self
                .excluded
                .iter()
                .any(|source| included_ids.contains(&source.artifact_id))
        {
            return Err(DomainError::Validation(
                "context candidate partition is incomplete".to_owned(),
            )
            .into());
        }
        let mut excluded_sources = self.excluded;
        excluded_sources.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
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
        let canonical_envelope = envelope.canonical_bytes()?;
        #[cfg(not(test))]
        drop(canonical_envelope);
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
        #[cfg(test)]
        let finish_actual_bytes = {
            let mut remaining_candidates = checked_resource_mul(
                u64::try_from(self.candidates.capacity()).unwrap_or(u64::MAX),
                u64::try_from(size_of::<Candidate>()).unwrap_or(u64::MAX),
                "context finish remaining candidates",
            )?;
            for candidate in &self.candidates {
                for value in [
                    artifact_clone_backing_reservation(&candidate.artifact)?,
                    source_clone_backing_reservation(&candidate.source)?,
                    u64::try_from(candidate.rank.3.allocated_bytes()).unwrap_or(u64::MAX),
                    checked_resource_mul(
                        u64::try_from(candidate.anchors.capacity()).unwrap_or(u64::MAX),
                        u64::try_from(size_of::<(u64, u64, StableId)>()).unwrap_or(u64::MAX),
                        "context finish remaining anchors",
                    )?,
                ] {
                    remaining_candidates = checked_resource_add(
                        remaining_candidates,
                        value,
                        "context finish remaining candidates",
                    )?;
                }
                for (_, _, owner) in &candidate.anchors {
                    remaining_candidates = checked_resource_add(
                        remaining_candidates,
                        u64::try_from(owner.allocated_bytes()).unwrap_or(u64::MAX),
                        "context finish remaining anchor IDs",
                    )?;
                }
            }
            [
                u64::try_from(envelope.allocated_bytes()).unwrap_or(u64::MAX),
                u64::try_from(admission.allocated_bytes()).unwrap_or(u64::MAX),
                u64::try_from(body.capacity()).unwrap_or(u64::MAX),
                u64::try_from(canonical_envelope.capacity()).unwrap_or(u64::MAX),
                self.working_reservation.allocated_bytes()?,
                u64::try_from(self.obligation.allocated_bytes()).unwrap_or(u64::MAX),
                remaining_candidates,
            ]
            .into_iter()
            .try_fold(0_u64, |total, value| {
                checked_resource_add(total, value, "context finish actual bytes")
            })?
        };
        #[cfg(test)]
        debug_assert!(finish_actual_bytes <= self.reservation.session_working_bytes());
        Ok(BuiltContextProjection {
            envelope,
            admission,
            #[cfg(test)]
            finish_actual_bytes,
        })
    }
    fn exclude(&mut self, id: StableId, reason: ExclusionReason) {
        if !self.excluded.iter().any(|source| source.artifact_id == id) {
            self.excluded.push(ExcludedSourceRef {
                artifact_id: id,
                reason,
            });
        }
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
    let candidate_count = candidate_count(program)?;
    let mut out = Vec::new();
    reserve_exact(
        &mut out,
        candidate_count,
        "context resource candidate reservation",
    )?;
    visit_candidate_metadata(
        aggregate,
        program,
        sources,
        snapshot,
        |artifact, source, size| {
            let mut anchors = Vec::new();
            reserve_exact(
                &mut anchors,
                MAX_ANCHORS,
                "context resource anchor reservation",
            )?;
            out.push(Candidate {
                artifact: artifact.clone(),
                source: source.clone(),
                registration_size: size,
                rank: (1, usize::MAX, usize::MAX, artifact.id.clone()),
                exclusion: None,
                anchors,
            });
            Ok(())
        },
    )?;
    Ok(out)
}

fn candidate_count(program: &ProgramSpace) -> ContextResult<usize> {
    let count = program
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
        .try_fold(0_usize, |count, _| {
            count
                .checked_add(1)
                .ok_or_else(|| incomplete("context candidate files", MAX, usize::MAX))
        })?;
    if count > MAX {
        return Err(incomplete("context candidate files", MAX, count).into());
    }
    Ok(count)
}

/// Shared, allocation-free candidate-closure visitor.  `prepare_context` and
/// the pre-CAS resource oracle use this exact validation and candidate order;
/// a persisted projection cannot replace this accepted-state derivation.
fn visit_candidate_metadata(
    aggregate: &ReviewAggregate,
    program: &ProgramSpace,
    sources: &crate::SnapshotSourcesRecorded,
    snapshot: &StableId,
    mut visit: impl FnMut(&Artifact, &SnapshotSourceRecordEntry, u64) -> ContextResult<()>,
) -> ContextResult<()> {
    let candidate_count = candidate_count(program)?;
    if sources.entries().len() != candidate_count {
        return Err(DomainError::Validation(
            "snapshot source closure must exactly cover file candidates".to_owned(),
        )
        .into());
    }
    for artifact in program
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
    {
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
        if source.path()
            != artifact
                .location
                .as_ref()
                .map_or("", |location| location.path.as_str())
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
        visit(artifact, source, registration.size())?;
    }
    Ok(())
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
    u64,
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
    // Capability-gap obligations intentionally retain the repository as
    // their stable review source. For context construction, additionally
    // traverse the accepted facts named by each incomplete capability
    // declaration; this does not change the obligation/universe identity or
    // promote the capability state, and it avoids asking a reviewer to assess
    // an empty projection when concrete partial facts are available.
    if obligation.property_id() == "reviewgraphen.capability_gap" {
        for capability in obligation.required_capabilities() {
            if let Some(declaration) = program.extraction().capabilities.get(capability) {
                all.extend(declaration.source_ids.iter().cloned());
            }
        }
    }
    all.sort();
    all.dedup();
    let seed_vector_capacity = typed_vec_capacity_bytes::<StableId>(
        all.capacity(),
        "context sealed discovery seed vector capacity",
    )?;
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
    let (adjacency, adjacency_vector_capacity) = adjacency(program)?;
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
    let mut todo_vector_capacity = 0_u64;
    let mut file_for = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    for id in &structural {
        let mut todo = vec![id.clone()];
        todo_vector_capacity = todo_vector_capacity.max(typed_vec_capacity_bytes::<StableId>(
            todo.capacity(),
            "context sealed discovery todo vector capacity",
        )?);
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
                    todo.push(parent.clone());
                    todo_vector_capacity =
                        todo_vector_capacity.max(typed_vec_capacity_bytes::<StableId>(
                            todo.capacity(),
                            "context sealed discovery todo vector capacity",
                        )?);
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
    let path_vector_capacity = path_entries.iter().try_fold(
        typed_vec_capacity_bytes::<(StableId, (usize, Vec<u8>, Vec<StableId>))>(
            path_entries.capacity(),
            "context sealed discovery path vector capacity",
        )?,
        |total, (_, (_, tokens, nodes))| {
            let total = checked_resource_add(
                total,
                typed_vec_capacity_bytes::<u8>(
                    tokens.capacity(),
                    "context sealed discovery token vector capacity",
                )?,
                "context sealed discovery vector capacity",
            )?;
            checked_resource_add(
                total,
                typed_vec_capacity_bytes::<StableId>(
                    nodes.capacity(),
                    "context sealed discovery node vector capacity",
                )?,
                "context sealed discovery vector capacity",
            )
        },
    )?;
    let test_vector_capacity =
        typed_vec_capacity_bytes::<&(StableId, (usize, Vec<u8>, Vec<StableId>))>(
            tests.capacity(),
            "context sealed discovery test vector capacity",
        )?;
    let discovery_vector_capacity = [
        seed_vector_capacity,
        adjacency_vector_capacity,
        todo_vector_capacity,
        path_vector_capacity,
        test_vector_capacity,
    ]
    .into_iter()
    .try_fold(0_u64, |total, bytes| {
        checked_resource_add(total, bytes, "context sealed discovery vector capacity")
    })?;
    Ok((
        candidates,
        structural,
        file_for,
        direct,
        dist,
        rank,
        path_cap,
        test_cap,
        unknowns,
        discovery_vector_capacity,
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
) -> ContextResult<(BTreeMap<StableId, Vec<(u8, StableId, usize)>>, u64)> {
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
    let vector_capacity = result.values().try_fold(0_u64, |total, edges| {
        checked_resource_add(
            total,
            typed_vec_capacity_bytes::<(u8, StableId, usize)>(
                edges.capacity(),
                "context sealed discovery adjacency vector capacity",
            )?,
            "context sealed discovery adjacency vector capacity",
        )
    })?;
    Ok((result, vector_capacity))
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
    let mut losses = Vec::new();
    reserve_exact(&mut losses, 64, "context resource loss reservation")?;
    for (description, source_ids) in groups {
        losses.push(EnvelopeLoss {
            description,
            severity: Severity::Low,
            affected_properties: props.clone(),
            source_ids,
        });
    }
    Ok(losses)
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

/// The role by which a range entered a subject-window projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextWindowRoleV2 {
    Callee,
    Caller,
    Support,
}

/// A source-backed, inclusive range admitted for the v2 reviewer packet.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContextWindowV2 {
    id: StableId,
    source_artifact_id: StableId,
    registration_id: StableId,
    content_hash: ContentHash,
    cas_hash: ContentHash,
    range: ExcerptRange,
    owner_ids: BTreeSet<StableId>,
    roles: BTreeSet<ContextWindowRoleV2>,
    excerpt_byte_length: u64,
    excerpt_hash: ContentHash,
}

impl ContextWindowV2 {
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn source_artifact_id(&self) -> &StableId {
        &self.source_artifact_id
    }
    pub fn registration_id(&self) -> &StableId {
        &self.registration_id
    }
    pub fn content_hash(&self) -> &ContentHash {
        &self.content_hash
    }
    pub fn cas_hash(&self) -> &ContentHash {
        &self.cas_hash
    }
    pub fn range(&self) -> &ExcerptRange {
        &self.range
    }
    pub fn owner_ids(&self) -> &BTreeSet<StableId> {
        &self.owner_ids
    }
    pub fn roles(&self) -> &BTreeSet<ContextWindowRoleV2> {
        &self.roles
    }
    pub fn excerpt_byte_length(&self) -> u64 {
        self.excerpt_byte_length
    }
    pub fn excerpt_hash(&self) -> &ContentHash {
        &self.excerpt_hash
    }
}

/// One resolver-owned range candidate.  Its bytes are borrowed only while the
/// resolver runs and are never retained in [`ContextWindowV2`].
#[derive(Debug)]
pub struct ContextWindowInputV2<'a> {
    pub source_artifact_id: StableId,
    pub registration_id: StableId,
    pub content_hash: ContentHash,
    pub cas_hash: ContentHash,
    pub bytes: &'a [u8],
    pub start_line: u32,
    pub end_line: u32,
    pub owner_id: StableId,
    pub role: ContextWindowRoleV2,
}

/// One immutable member of the complete v2 window-candidate denominator.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContextWindowCandidateV2 {
    source_artifact_id: StableId,
    registration_id: StableId,
    content_hash: ContentHash,
    cas_hash: ContentHash,
    range: ExcerptRange,
    owner_id: StableId,
    role: ContextWindowRoleV2,
}

impl ContextWindowCandidateV2 {
    pub fn source_artifact_id(&self) -> &StableId {
        &self.source_artifact_id
    }
    pub fn registration_id(&self) -> &StableId {
        &self.registration_id
    }
    pub fn content_hash(&self) -> &ContentHash {
        &self.content_hash
    }
    pub fn cas_hash(&self) -> &ContentHash {
        &self.cas_hash
    }
    pub fn range(&self) -> &ExcerptRange {
        &self.range
    }
    pub fn owner_id(&self) -> &StableId {
        &self.owner_id
    }
    pub const fn role(&self) -> ContextWindowRoleV2 {
        self.role
    }
}

/// A typed record for a caller or callee that could not be represented by an
/// admitted source window.  It is deliberately distinct from an AI claim.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContextSubjectLossV2 {
    id: StableId,
    endpoint_id: StableId,
    role: ContextWindowRoleV2,
    reason: ContextWindowLossReasonV2,
    source_artifact_id: Option<StableId>,
    requested_range: Option<ExcerptRange>,
    property_id: Option<String>,
    severity: Severity,
}

fn checked_v2_usize_add(
    operation: &'static str,
    limit: usize,
    left: usize,
    right: usize,
) -> ContextResult<usize> {
    left.checked_add(right)
        .ok_or_else(|| incomplete(operation, limit, usize::MAX).into())
}

fn inclusive_line_span(start: u32, end: u32) -> ContextResult<usize> {
    let span = end
        .checked_sub(start)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| incomplete("window line span", MAX_LINES, usize::MAX))?;
    usize::try_from(span).map_err(|_| incomplete("window line span", MAX_LINES, usize::MAX).into())
}

/// Closed v2 loss vocabulary, in the exact ADR 0038 precedence order.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextWindowLossReasonV2 {
    MissingLocation,
    MissingSource,
    GiantLine,
    PerWindowLines,
    PerWindowBytes,
    PerFileWindowCap,
    TotalWindowCap,
    TotalExcerptBytes,
    OverlapUnmergeable,
    PathCap,
    TestCap,
    NotReached,
    IncludedFileCap,
    ArtifactBytesCap,
    TotalResolvedBytesCap,
}

impl ContextWindowLossReasonV2 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingLocation => "missing_location",
            Self::MissingSource => "missing_source",
            Self::GiantLine => "giant_line",
            Self::PerWindowLines => "per_window_lines",
            Self::PerWindowBytes => "per_window_bytes",
            Self::PerFileWindowCap => "per_file_window_cap",
            Self::TotalWindowCap => "total_window_cap",
            Self::TotalExcerptBytes => "total_excerpt_bytes",
            Self::OverlapUnmergeable => "overlap_unmergeable",
            Self::PathCap => "path_cap",
            Self::TestCap => "test_cap",
            Self::NotReached => "not_reached",
            Self::IncludedFileCap => "included_file_cap",
            Self::ArtifactBytesCap => "artifact_bytes_cap",
            Self::TotalResolvedBytesCap => "total_resolved_bytes_cap",
        }
    }
}

/// Resolves ordered range candidates into non-overlapping subject windows.
/// The caller/callee role ordering is part of this function rather than a
/// post-processing convention, so a support candidate cannot consume a window
/// slot before a representable subject candidate is considered.
pub fn resolve_subject_windows_v2(
    snapshot_id: &StableId,
    obligation_id: &StableId,
    inputs: &[ContextWindowInputV2<'_>],
) -> ContextResult<(Vec<ContextWindowV2>, Vec<ContextSubjectLossV2>)> {
    resolve_subject_windows_for_policy(
        snapshot_id,
        obligation_id,
        None,
        ContextSubjectWindowsPolicyV2::ID,
        64,
        inputs,
    )
}

fn resolve_subject_windows_for_policy(
    snapshot_id: &StableId,
    obligation_id: &StableId,
    property_id: Option<&str>,
    policy_id: &'static str,
    loss_limit: usize,
    inputs: &[ContextWindowInputV2<'_>],
) -> ContextResult<(Vec<ContextWindowV2>, Vec<ContextSubjectLossV2>)> {
    let maximum_inputs_without_subjects = MAX_FILES
        .checked_mul(MAX_ANCHORS)
        .ok_or_else(|| incomplete("context window candidates", usize::MAX, usize::MAX))?;
    let maximum_inputs = checked_v2_usize_add(
        "context window candidates",
        usize::MAX,
        maximum_inputs_without_subjects,
        2,
    )?;
    let maximum_source_inputs = checked_v2_usize_add(
        "context source window candidates",
        usize::MAX,
        MAX_ANCHORS,
        2,
    )?;
    if inputs.len() > maximum_inputs {
        return Err(incomplete("context window candidates", maximum_inputs, inputs.len()).into());
    }
    let mut source_identities = BTreeMap::<StableId, (StableId, ContentHash, ContentHash)>::new();
    let mut source_input_counts = BTreeMap::<StableId, usize>::new();
    for input in inputs {
        if ContentHash::sha256(input.bytes) != input.content_hash
            || input.content_hash != input.cas_hash
        {
            return Err(DomainError::Validation(
                "window source bytes do not match registration hashes".to_owned(),
            )
            .into());
        }
        let identity = (
            input.registration_id.clone(),
            input.content_hash.clone(),
            input.cas_hash.clone(),
        );
        if !source_identities.contains_key(&input.source_artifact_id)
            && source_identities.len() == MAX
        {
            let observed = checked_v2_usize_add("context candidate files", MAX, MAX, 1)?;
            return Err(incomplete("context candidate files", MAX, observed).into());
        }
        if source_identities
            .insert(input.source_artifact_id.clone(), identity.clone())
            .is_some_and(|previous| previous != identity)
        {
            return Err(DomainError::Validation(
                "one source artifact has inconsistent window source identity".to_owned(),
            )
            .into());
        }
        let count = source_input_counts
            .entry(input.source_artifact_id.clone())
            .or_default();
        *count = checked_v2_usize_add(
            "context source window candidates",
            maximum_source_inputs,
            *count,
            1,
        )?;
        if *count > maximum_source_inputs {
            return Err(incomplete(
                "context source window candidates",
                maximum_source_inputs,
                *count,
            )
            .into());
        }
    }
    let mut ordered = inputs.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        (
            window_role_priority(left.role),
            left.source_artifact_id.clone(),
            left.start_line,
            left.end_line,
            left.owner_id.clone(),
        )
            .cmp(&(
                window_role_priority(right.role),
                right.source_artifact_id.clone(),
                right.start_line,
                right.end_line,
                right.owner_id.clone(),
            ))
    });
    let mut windows = Vec::<ContextWindowV2>::new();
    let mut losses = Vec::<ContextSubjectLossV2>::new();
    let mut total_bytes = 0_usize;
    for input in ordered {
        let starts = lines(input.bytes);
        let line_count = u32::try_from(starts.len())
            .map_err(|_| incomplete("window source line count", u32::MAX as usize, starts.len()))?;
        let fail = |reason: ContextWindowLossReasonV2,
                    losses: &mut Vec<ContextSubjectLossV2>|
         -> ContextResult<()> {
            push_window_loss(
                losses,
                snapshot_id,
                obligation_id,
                policy_id,
                loss_limit,
                &input.owner_id,
                input.role,
                reason,
                Some(input.source_artifact_id.clone()),
                Some(ExcerptRange {
                    start_line: input.start_line,
                    end_line: input.end_line,
                }),
                property_id,
            )
        };
        if input.start_line == 0 || input.end_line < input.start_line || input.end_line > line_count
        {
            fail(ContextWindowLossReasonV2::MissingLocation, &mut losses)?;
            continue;
        }
        let candidate_bytes = slice_lines(
            input.bytes,
            &starts,
            u64::from(input.start_line),
            u64::from(input.end_line),
        );
        if inclusive_line_span(input.start_line, input.end_line)? > MAX_LINES {
            fail(ContextWindowLossReasonV2::PerWindowLines, &mut losses)?;
            continue;
        }
        if candidate_bytes.len() > MAX_EXCERPT {
            fail(
                if input.start_line == input.end_line {
                    ContextWindowLossReasonV2::GiantLine
                } else {
                    ContextWindowLossReasonV2::PerWindowBytes
                },
                &mut losses,
            )?;
            continue;
        }

        let input_end_adjacent = u64::from(input.end_line)
            .checked_add(1)
            .ok_or_else(|| incomplete("window adjacency", usize::MAX, usize::MAX))?;
        let mut touching = Vec::new();
        for (index, window) in windows.iter().enumerate() {
            let window_end_adjacent = u64::from(window.range.end_line)
                .checked_add(1)
                .ok_or_else(|| incomplete("window adjacency", usize::MAX, usize::MAX))?;
            if window.source_artifact_id == input.source_artifact_id
                && u64::from(input.start_line) <= window_end_adjacent
                && u64::from(window.range.start_line) <= input_end_adjacent
            {
                touching.push(index);
            }
        }
        let mut start = input.start_line;
        let mut end = input.end_line;
        for index in &touching {
            start = start.min(windows[*index].range.start_line);
            end = end.max(windows[*index].range.end_line);
        }
        let bytes = slice_lines(input.bytes, &starts, u64::from(start), u64::from(end));
        if !touching.is_empty()
            && (inclusive_line_span(start, end)? > MAX_LINES || bytes.len() > MAX_EXCERPT)
        {
            fail(ContextWindowLossReasonV2::OverlapUnmergeable, &mut losses)?;
            continue;
        }
        let replacing = touching.iter().try_fold(0_usize, |total, index| {
            let window_bytes = usize::try_from(windows[*index].excerpt_byte_length)
                .map_err(|_| incomplete("replaced window bytes", MAX_TOTAL_EXCERPT, usize::MAX))?;
            checked_v2_usize_add(
                "replaced window bytes",
                MAX_TOTAL_EXCERPT,
                total,
                window_bytes,
            )
        })?;
        let file_windows = windows
            .iter()
            .filter(|window| window.source_artifact_id == input.source_artifact_id)
            .count();
        if touching.is_empty() && file_windows >= 4 {
            fail(ContextWindowLossReasonV2::PerFileWindowCap, &mut losses)?;
            continue;
        }
        if touching.is_empty() && windows.len() >= 8 {
            fail(ContextWindowLossReasonV2::TotalWindowCap, &mut losses)?;
            continue;
        }
        let retained_bytes = total_bytes
            .checked_sub(replacing)
            .ok_or_else(|| incomplete("total window bytes", MAX_TOTAL_EXCERPT, usize::MAX))?;
        if checked_v2_usize_add(
            "total window bytes",
            MAX_TOTAL_EXCERPT,
            retained_bytes,
            bytes.len(),
        )? > MAX_TOTAL_EXCERPT
        {
            fail(ContextWindowLossReasonV2::TotalExcerptBytes, &mut losses)?;
            continue;
        }
        let mut owners = BTreeSet::from([input.owner_id.clone()]);
        let mut roles = BTreeSet::from([input.role]);
        for index in &touching {
            let window = &windows[*index];
            if window.registration_id != input.registration_id
                || window.content_hash != input.content_hash
                || window.cas_hash != input.cas_hash
            {
                return Err(DomainError::Validation(
                    "one source artifact has inconsistent window source identity".to_owned(),
                )
                .into());
            }
            owners.extend(window.owner_ids.iter().cloned());
            roles.extend(window.roles.iter().copied());
        }
        let excerpt_byte_length = u64::try_from(bytes.len())
            .map_err(|_| incomplete("window excerpt bytes", MAX_EXCERPT, usize::MAX))?;
        let excerpt_hash = ContentHash::sha256(bytes);
        let bindings = BTreeMap::from([
            (
                "policy_id".to_owned(),
                serde_json::Value::String(policy_id.to_owned()),
            ),
            (
                "snapshot_id".to_owned(),
                serde_json::Value::String(snapshot_id.to_string()),
            ),
            (
                "obligation_id".to_owned(),
                serde_json::Value::String(obligation_id.to_string()),
            ),
            (
                "source_artifact_id".to_owned(),
                serde_json::Value::String(input.source_artifact_id.to_string()),
            ),
            (
                "registration_id".to_owned(),
                serde_json::Value::String(input.registration_id.to_string()),
            ),
            (
                "content_hash".to_owned(),
                serde_json::Value::String(input.content_hash.to_string()),
            ),
            (
                "cas_hash".to_owned(),
                serde_json::Value::String(input.cas_hash.to_string()),
            ),
            ("start_line".to_owned(), serde_json::json!(start)),
            ("end_line".to_owned(), serde_json::json!(end)),
            ("owner_ids".to_owned(), serde_json::json!(owners)),
            ("roles".to_owned(), serde_json::json!(roles)),
            (
                "excerpt_byte_length".to_owned(),
                serde_json::json!(excerpt_byte_length),
            ),
            (
                "excerpt_hash".to_owned(),
                serde_json::Value::String(excerpt_hash.to_string()),
            ),
        ]);
        let window = ContextWindowV2 {
            id: StableId::derived("context-window", &bindings)?,
            source_artifact_id: input.source_artifact_id.clone(),
            registration_id: input.registration_id.clone(),
            content_hash: input.content_hash.clone(),
            cas_hash: input.cas_hash.clone(),
            range: ExcerptRange {
                start_line: start,
                end_line: end,
            },
            owner_ids: owners,
            roles,
            excerpt_byte_length,
            excerpt_hash,
        };
        let retained_bytes = total_bytes
            .checked_sub(replacing)
            .ok_or_else(|| incomplete("total window bytes", MAX_TOTAL_EXCERPT, usize::MAX))?;
        total_bytes = checked_v2_usize_add(
            "total window bytes",
            MAX_TOTAL_EXCERPT,
            retained_bytes,
            bytes.len(),
        )?;
        for index in touching.iter().rev() {
            windows.remove(*index);
        }
        windows.push(window);
    }
    windows.sort_by(|left, right| {
        (
            left.source_artifact_id.clone(),
            left.range.start_line,
            left.range.end_line,
            left.id.clone(),
        )
            .cmp(&(
                right.source_artifact_id.clone(),
                right.range.start_line,
                right.range.end_line,
                right.id.clone(),
            ))
    });
    sort_window_losses(&mut losses);
    Ok((windows, losses))
}

const fn window_role_priority(role: ContextWindowRoleV2) -> u8 {
    match role {
        ContextWindowRoleV2::Callee => 0,
        ContextWindowRoleV2::Caller => 1,
        ContextWindowRoleV2::Support => 2,
    }
}

impl ContextSubjectLossV2 {
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn endpoint_id(&self) -> &StableId {
        &self.endpoint_id
    }
    pub const fn role(&self) -> ContextWindowRoleV2 {
        self.role
    }
    pub const fn reason(&self) -> ContextWindowLossReasonV2 {
        self.reason
    }
    pub fn source_artifact_id(&self) -> Option<&StableId> {
        self.source_artifact_id.as_ref()
    }
    pub fn requested_range(&self) -> Option<&ExcerptRange> {
        self.requested_range.as_ref()
    }
    pub fn property_id(&self) -> Option<&str> {
        self.property_id.as_deref()
    }
    pub const fn severity(&self) -> Severity {
        self.severity
    }
}

#[allow(clippy::too_many_arguments)]
fn push_window_loss(
    losses: &mut Vec<ContextSubjectLossV2>,
    snapshot_id: &StableId,
    obligation_id: &StableId,
    policy_id: &'static str,
    loss_limit: usize,
    endpoint_id: &StableId,
    role: ContextWindowRoleV2,
    reason: ContextWindowLossReasonV2,
    source_artifact_id: Option<StableId>,
    requested_range: Option<ExcerptRange>,
    property_id: Option<&str>,
) -> ContextResult<()> {
    if losses.len() == loss_limit {
        let observed = checked_v2_usize_add("context window losses", loss_limit, loss_limit, 1)?;
        return Err(incomplete("context window losses", loss_limit, observed).into());
    }
    let severity = if role == ContextWindowRoleV2::Support {
        Severity::Low
    } else {
        Severity::High
    };
    let bindings = BTreeMap::from([
        (
            "policy_id".to_owned(),
            serde_json::Value::String(policy_id.to_owned()),
        ),
        (
            "snapshot_id".to_owned(),
            serde_json::Value::String(snapshot_id.to_string()),
        ),
        (
            "obligation_id".to_owned(),
            serde_json::Value::String(obligation_id.to_string()),
        ),
        (
            "endpoint_id".to_owned(),
            serde_json::Value::String(endpoint_id.to_string()),
        ),
        ("role".to_owned(), serde_json::json!(role)),
        ("reason".to_owned(), serde_json::json!(reason)),
        (
            "source_artifact_id".to_owned(),
            serde_json::json!(source_artifact_id),
        ),
        (
            "requested_range".to_owned(),
            serde_json::json!(requested_range),
        ),
        ("property_id".to_owned(), serde_json::json!(property_id)),
        ("severity".to_owned(), serde_json::json!(severity)),
    ]);
    losses.push(ContextSubjectLossV2 {
        id: StableId::derived("context-loss", &bindings)?,
        endpoint_id: endpoint_id.clone(),
        role,
        reason,
        source_artifact_id,
        requested_range,
        property_id: property_id.map(str::to_owned),
        severity,
    });
    Ok(())
}

fn sort_window_losses(losses: &mut [ContextSubjectLossV2]) {
    losses.sort_by(|left, right| {
        (
            left.reason,
            left.severity,
            left.source_artifact_id.clone(),
            left.requested_range.clone(),
            left.endpoint_id.clone(),
            left.role,
            left.id.clone(),
        )
            .cmp(&(
                right.reason,
                right.severity,
                right.source_artifact_id.clone(),
                right.requested_range.clone(),
                right.endpoint_id.clone(),
                right.role,
                right.id.clone(),
            ))
    });
}

#[derive(Clone, Debug)]
struct SubjectExpectationV2 {
    endpoint_id: StableId,
    role: ContextWindowRoleV2,
    source_artifact_id: Option<StableId>,
    range: Option<ExcerptRange>,
    preflight_loss: Option<ContextWindowLossReasonV2>,
}

#[derive(Debug)]
struct ResolvedSourceV2 {
    bytes: Vec<u8>,
}

/// The completed subject-first window projection.  C7 owns its persistence in
/// a v2 envelope; this value contains no raw source bytes.
#[derive(Debug)]
pub struct BuiltContextSubjectWindowsV2 {
    id: StableId,
    projection_hash: ContentHash,
    policy: ContextSubjectWindowsPolicyV2,
    policy_hash: ContentHash,
    snapshot_id: StableId,
    obligation_id: StableId,
    property_id: String,
    caller_artifact_id: StableId,
    callee_artifact_id: StableId,
    candidate_source_ids: BTreeSet<StableId>,
    window_candidates: Vec<ContextWindowCandidateV2>,
    windows: Vec<ContextWindowV2>,
    subject_losses: Vec<ContextSubjectLossV2>,
    excluded_sources: Vec<ExcludedSourceRef>,
    unknowns: Vec<EnvelopeUnknown>,
}

impl BuiltContextSubjectWindowsV2 {
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn projection_hash(&self) -> &ContentHash {
        &self.projection_hash
    }
    pub const fn policy(&self) -> ContextSubjectWindowsPolicyV2 {
        self.policy
    }
    pub fn policy_hash(&self) -> &ContentHash {
        &self.policy_hash
    }
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    pub fn obligation_id(&self) -> &StableId {
        &self.obligation_id
    }
    pub fn property_id(&self) -> &str {
        &self.property_id
    }
    pub fn caller_artifact_id(&self) -> &StableId {
        &self.caller_artifact_id
    }
    pub fn callee_artifact_id(&self) -> &StableId {
        &self.callee_artifact_id
    }
    pub fn windows(&self) -> &[ContextWindowV2] {
        &self.windows
    }
    pub fn subject_losses(&self) -> &[ContextSubjectLossV2] {
        &self.subject_losses
    }
    pub fn losses(&self) -> &[ContextSubjectLossV2] {
        &self.subject_losses
    }
    pub fn candidate_source_ids(&self) -> &BTreeSet<StableId> {
        &self.candidate_source_ids
    }
    pub fn window_candidates(&self) -> &[ContextWindowCandidateV2] {
        &self.window_candidates
    }
    pub fn excluded_sources(&self) -> &[ExcludedSourceRef] {
        &self.excluded_sources
    }
    pub fn unknowns(&self) -> &[EnvelopeUnknown] {
        &self.unknowns
    }
    pub fn assumptions(&self) -> &[String] {
        &[]
    }
}

/// Ordered resolver session for `context.subject_windows@2`.
///
/// Its request/submit protocol is intentionally identical to
/// [`ContextBuildSession`].  It records each subject as an explicit endpoint
/// expectation, and retains an endpoint that cannot be represented as a
/// high-severity typed loss.
#[derive(Debug)]
pub struct ContextSubjectWindowsSessionV2 {
    snapshot_id: StableId,
    obligation: Obligation,
    policy_hash: ContentHash,
    caller_artifact_id: StableId,
    callee_artifact_id: StableId,
    expectations: Vec<SubjectExpectationV2>,
    candidates: Vec<Candidate>,
    unknowns: Vec<EnvelopeUnknown>,
    index: usize,
    pending: Option<ContextSourceRequest>,
    resolved: BTreeMap<StableId, ResolvedSourceV2>,
    excluded: Vec<ExcludedSourceRef>,
    resolved_bytes: u64,
    session_digest: ContentHash,
    effect_probe: Option<Arc<dyn ContextBuildProbe>>,
}

impl ContextSubjectWindowsSessionV2 {
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
                self.resolved.len(),
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
            probe(
                &self.effect_probe,
                ContextBuildEffect::SourceBytesRequested {
                    artifact_id: request.artifact_id.clone(),
                },
            );
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
        let byte_length = u64::try_from(bytes.len())
            .map_err(|_| incomplete("resolved artifact byte length", usize::MAX, bytes.len()))?;
        let actual_hash = ContentHash::sha256(bytes);
        let actual_line_count = bytes.iter().try_fold(1_u64, |count, byte| {
            if *byte == b'\n' {
                count.checked_add(1)
            } else {
                Some(count)
            }
        });
        if byte_length != expected.expected_length
            || actual_hash != expected.content_hash
            || actual_hash != expected.cas_hash
            || actual_line_count != Some(expected.line_count)
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
        if next_resolved_bytes > MAX_RESOLVED {
            return Err(incomplete(
                "context resolved bytes",
                MAX_RESOLVED as usize,
                usize::try_from(next_resolved_bytes).unwrap_or(usize::MAX),
            )
            .into());
        }
        let mut owned = Vec::new();
        owned.try_reserve_exact(bytes.len()).map_err(|_| {
            incomplete(
                "resolved source retention",
                MAX_RESOLVED as usize,
                bytes.len(),
            )
        })?;
        owned.extend_from_slice(bytes);
        let artifact_id = expected.artifact_id.clone();
        if self
            .resolved
            .insert(artifact_id.clone(), ResolvedSourceV2 { bytes: owned })
            .is_some()
        {
            return Err(DomainError::Validation(
                "v2 context source was resolved more than once".to_owned(),
            )
            .into());
        }
        self.pending = None;
        probe(
            &self.effect_probe,
            ContextBuildEffect::SourceSubmitted { artifact_id },
        );
        self.resolved_bytes = next_resolved_bytes;
        self.index = checked_v2_usize_add(
            "v2 context source index",
            self.candidates.len(),
            self.index,
            1,
        )?;
        Ok(())
    }

    pub fn finish(self) -> ContextResult<BuiltContextSubjectWindowsV2> {
        if self.pending.is_some() || self.index != self.candidates.len() {
            return Err(ContextError::Protocol(
                "all requested and metadata-only candidates must be processed before finish",
            ));
        }
        let candidate_source_ids = self
            .candidates
            .iter()
            .map(|candidate| candidate.artifact.id.clone())
            .collect::<BTreeSet<_>>();
        let partition_count = checked_v2_usize_add(
            "v2 context candidate partition",
            MAX,
            self.resolved.len(),
            self.excluded.len(),
        )?;
        if partition_count != candidate_source_ids.len() {
            return Err(DomainError::Validation(
                "v2 context candidate partition is incomplete".to_owned(),
            )
            .into());
        }
        let excluded_by_id = self
            .excluded
            .iter()
            .map(|excluded| (excluded.artifact_id.clone(), excluded.reason))
            .collect::<BTreeMap<_, _>>();
        let mut inputs = Vec::new();
        let inputs_per_source =
            checked_v2_usize_add("context window candidates", usize::MAX, MAX_ANCHORS, 2)?;
        let maximum_inputs = self
            .resolved
            .len()
            .checked_mul(inputs_per_source)
            .ok_or_else(|| incomplete("context window candidates", usize::MAX, usize::MAX))?;
        inputs
            .try_reserve(maximum_inputs)
            .map_err(|_| incomplete("context window candidates", maximum_inputs, usize::MAX))?;
        let mut window_candidates = Vec::new();
        window_candidates
            .try_reserve(maximum_inputs)
            .map_err(|_| incomplete("context window denominator", maximum_inputs, usize::MAX))?;
        let mut losses = Vec::new();
        for expectation in &self.expectations {
            if let Some(reason) = expectation.preflight_loss {
                push_window_loss(
                    &mut losses,
                    &self.snapshot_id,
                    self.obligation.id(),
                    ContextSubjectWindowsPolicyV2::ID,
                    64,
                    &expectation.endpoint_id,
                    expectation.role,
                    reason,
                    expectation.source_artifact_id.clone(),
                    expectation.range.clone(),
                    Some(self.obligation.property_id()),
                )?;
                continue;
            }
            let source_artifact_id = expectation
                .source_artifact_id
                .as_ref()
                .expect("valid expectation source");
            let range = expectation.range.as_ref().expect("valid expectation range");
            let candidate = self
                .candidates
                .iter()
                .find(|candidate| candidate.artifact.id == *source_artifact_id)
                .expect("expectation source is a candidate");
            window_candidates.push(ContextWindowCandidateV2 {
                source_artifact_id: source_artifact_id.clone(),
                registration_id: candidate.source.registration_id().clone(),
                content_hash: candidate.source.content_hash().clone(),
                cas_hash: candidate.source.cas_hash().clone(),
                range: range.clone(),
                owner_id: expectation.endpoint_id.clone(),
                role: expectation.role,
            });
            if let Some(resolved) = self.resolved.get(source_artifact_id) {
                inputs.push(ContextWindowInputV2 {
                    source_artifact_id: source_artifact_id.clone(),
                    registration_id: candidate.source.registration_id().clone(),
                    content_hash: candidate.source.content_hash().clone(),
                    cas_hash: candidate.source.cas_hash().clone(),
                    bytes: &resolved.bytes,
                    start_line: range.start_line,
                    end_line: range.end_line,
                    owner_id: expectation.endpoint_id.clone(),
                    role: expectation.role,
                });
            } else {
                let reason = excluded_by_id
                    .get(source_artifact_id)
                    .copied()
                    .map(window_reason_from_exclusion)
                    .unwrap_or(ContextWindowLossReasonV2::MissingSource);
                push_window_loss(
                    &mut losses,
                    &self.snapshot_id,
                    self.obligation.id(),
                    ContextSubjectWindowsPolicyV2::ID,
                    64,
                    &expectation.endpoint_id,
                    expectation.role,
                    reason,
                    Some(source_artifact_id.clone()),
                    Some(range.clone()),
                    Some(self.obligation.property_id()),
                )?;
            }
        }
        for candidate in &self.candidates {
            for (start, end, owner_id) in &candidate.anchors {
                window_candidates.push(ContextWindowCandidateV2 {
                    source_artifact_id: candidate.artifact.id.clone(),
                    registration_id: candidate.source.registration_id().clone(),
                    content_hash: candidate.source.content_hash().clone(),
                    cas_hash: candidate.source.cas_hash().clone(),
                    range: ExcerptRange {
                        start_line: u32::try_from(*start).map_err(|_| {
                            incomplete("support start line", u32::MAX as usize, usize::MAX)
                        })?,
                        end_line: u32::try_from(*end).map_err(|_| {
                            incomplete("support end line", u32::MAX as usize, usize::MAX)
                        })?,
                    },
                    owner_id: owner_id.clone(),
                    role: ContextWindowRoleV2::Support,
                });
            }
            if let Some(resolved) = self.resolved.get(&candidate.artifact.id) {
                for (start, end, owner_id) in &candidate.anchors {
                    inputs.push(ContextWindowInputV2 {
                        source_artifact_id: candidate.artifact.id.clone(),
                        registration_id: candidate.source.registration_id().clone(),
                        content_hash: candidate.source.content_hash().clone(),
                        cas_hash: candidate.source.cas_hash().clone(),
                        bytes: &resolved.bytes,
                        start_line: u32::try_from(*start).map_err(|_| {
                            incomplete("support start line", u32::MAX as usize, usize::MAX)
                        })?,
                        end_line: u32::try_from(*end).map_err(|_| {
                            incomplete("support end line", u32::MAX as usize, usize::MAX)
                        })?,
                        owner_id: owner_id.clone(),
                        role: ContextWindowRoleV2::Support,
                    });
                }
            }
        }
        window_candidates.sort_by(|left, right| {
            (
                window_role_priority(left.role),
                left.role,
                left.source_artifact_id.clone(),
                left.range.clone(),
                left.owner_id.clone(),
            )
                .cmp(&(
                    window_role_priority(right.role),
                    right.role,
                    right.source_artifact_id.clone(),
                    right.range.clone(),
                    right.owner_id.clone(),
                ))
        });
        let (windows, mut resolved_losses) = resolve_subject_windows_for_policy(
            &self.snapshot_id,
            self.obligation.id(),
            Some(self.obligation.property_id()),
            ContextSubjectWindowsPolicyV2::ID,
            64,
            &inputs,
        )?;
        let loss_count = checked_v2_usize_add(
            "context window losses",
            64,
            losses.len(),
            resolved_losses.len(),
        )?;
        if loss_count > 64 {
            return Err(incomplete("context window losses", 64, loss_count).into());
        }
        losses.append(&mut resolved_losses);
        sort_window_losses(&mut losses);
        let mut excluded_sources = self.excluded;
        excluded_sources.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
        let identity = SubjectWindowsIdentityV2 {
            assumptions: &[],
            callee_artifact_id: &self.callee_artifact_id,
            caller_artifact_id: &self.caller_artifact_id,
            candidate_source_ids: &candidate_source_ids,
            context_policy: ContextSubjectWindowsPolicyV2::fixed(),
            context_policy_hash: &self.policy_hash,
            excluded_sources: &excluded_sources,
            losses: &losses,
            obligation_id: self.obligation.id(),
            property_id: self.obligation.property_id(),
            snapshot_id: &self.snapshot_id,
            target_refs: self.obligation.target_refs(),
            unknowns: &self.unknowns,
            window_candidates: &window_candidates,
            windows: &windows,
        };
        let body =
            serde_json::to_vec(&identity).map_err(|error| DomainError::Json(error.to_string()))?;
        if body.len() > MAX_BODY {
            return Err(
                incomplete("context envelope canonical bytes", MAX_BODY, body.len()).into(),
            );
        }
        let projection_hash = ContentHash::sha256(&body);
        let id = StableId::parse(format!("context-envelope:{projection_hash}"))?;
        Ok(BuiltContextSubjectWindowsV2 {
            id,
            projection_hash,
            policy: ContextSubjectWindowsPolicyV2::fixed(),
            policy_hash: self.policy_hash,
            snapshot_id: self.snapshot_id,
            obligation_id: self.obligation.id().clone(),
            property_id: self.obligation.property_id().to_owned(),
            caller_artifact_id: self.caller_artifact_id,
            callee_artifact_id: self.callee_artifact_id,
            windows,
            subject_losses: losses,
            candidate_source_ids,
            window_candidates,
            excluded_sources,
            unknowns: self.unknowns,
        })
    }

    fn exclude(&mut self, id: StableId, reason: ExclusionReason) {
        if !self.excluded.iter().any(|source| source.artifact_id == id) {
            self.excluded.push(ExcludedSourceRef {
                artifact_id: id,
                reason,
            });
        }
    }
}

#[derive(Serialize)]
struct SubjectWindowsIdentityV2<'a> {
    assumptions: &'a [String],
    callee_artifact_id: &'a StableId,
    caller_artifact_id: &'a StableId,
    candidate_source_ids: &'a BTreeSet<StableId>,
    context_policy: ContextSubjectWindowsPolicyV2,
    context_policy_hash: &'a ContentHash,
    excluded_sources: &'a [ExcludedSourceRef],
    losses: &'a [ContextSubjectLossV2],
    obligation_id: &'a StableId,
    property_id: &'a str,
    snapshot_id: &'a StableId,
    target_refs: &'a [StableId],
    unknowns: &'a [EnvelopeUnknown],
    window_candidates: &'a [ContextWindowCandidateV2],
    windows: &'a [ContextWindowV2],
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextKnownCardinalityV3 {
    Known,
}

/// Exact commitment to one rebuilt v3 denominator. Detail row counts are not
/// accepted as a substitute for this value.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextDenominatorCommitmentV3 {
    cardinality: ContextKnownCardinalityV3,
    observed_count: u64,
    sorted_id_set_sha256: ContentHash,
}

impl ContextDenominatorCommitmentV3 {
    pub const fn cardinality(&self) -> ContextKnownCardinalityV3 {
        self.cardinality
    }
    pub const fn observed_count(&self) -> u64 {
        self.observed_count
    }
    pub fn sorted_id_set_sha256(&self) -> &ContentHash {
        &self.sorted_id_set_sha256
    }
}

fn sorted_id_set_sha256(ids: &BTreeSet<StableId>) -> ContextResult<ContentHash> {
    let bytes = serde_json::to_vec(ids).map_err(|error| DomainError::Json(error.to_string()))?;
    Ok(ContentHash::sha256(&bytes))
}

fn denominator_commitment_v3(
    operation: &'static str,
    ids: &BTreeSet<StableId>,
) -> ContextResult<ContextDenominatorCommitmentV3> {
    let observed_count =
        u64::try_from(ids.len()).map_err(|_| incomplete(operation, usize::MAX, ids.len()))?;
    Ok(ContextDenominatorCommitmentV3 {
        cardinality: ContextKnownCardinalityV3::Known,
        observed_count,
        sorted_id_set_sha256: sorted_id_set_sha256(ids)?,
    })
}

/// Unknown latent structure is never assigned a numeric value. A known zero
/// is available only when every capability governing context discovery is
/// complete.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, tag = "state", rename_all = "snake_case")]
pub enum ContextLatentCardinalityV3 {
    KnownZero,
    Unknown {
        capability_states: BTreeMap<String, crate::CapabilityState>,
        qualification_ids: BTreeSet<StableId>,
    },
}

fn latent_cardinality_v3(program: &ProgramSpace) -> ContextResult<ContextLatentCardinalityV3> {
    const GOVERNING: [&str; 4] = ["ast", "containment", "direct_calls", "test_mapping"];
    let mut states = BTreeMap::new();
    let mut incomplete_names = BTreeSet::new();
    for name in GOVERNING {
        let declaration = program.extraction().capabilities.get(name).ok_or_else(|| {
            DomainError::Validation(format!(
                "context v3 governing capability `{name}` is not declared"
            ))
        })?;
        states.insert(name.to_owned(), declaration.state);
        if declaration.state != crate::CapabilityState::Complete {
            incomplete_names.insert(name.to_owned());
        }
    }
    if incomplete_names.is_empty() {
        return Ok(ContextLatentCardinalityV3::KnownZero);
    }
    let qualification_ids = program
        .extraction()
        .limitations
        .iter()
        .filter(|limitation| {
            limitation
                .related_capabilities
                .iter()
                .any(|name| incomplete_names.contains(name))
        })
        .map(|limitation| limitation.id.clone())
        .collect::<BTreeSet<_>>();
    if qualification_ids.is_empty() {
        return Err(DomainError::Validation(
            "context v3 unknown latent cardinality requires source-backed qualification IDs"
                .to_owned(),
        )
        .into());
    }
    Ok(ContextLatentCardinalityV3::Unknown {
        capability_states: states,
        qualification_ids,
    })
}

fn support_anchor_id_v3(
    snapshot_id: &StableId,
    source_artifact_id: &StableId,
    start_line: u32,
    end_line: u32,
    owner_artifact_id: &StableId,
) -> ContextResult<StableId> {
    StableId::derived(
        "context-support-anchor",
        &BTreeMap::from([
            (
                "anchor_contract".to_owned(),
                serde_json::json!("context.support_anchor@1"),
            ),
            ("end_line".to_owned(), serde_json::json!(end_line)),
            (
                "owner_artifact_id".to_owned(),
                serde_json::json!(owner_artifact_id),
            ),
            ("snapshot_id".to_owned(), serde_json::json!(snapshot_id)),
            (
                "source_artifact_id".to_owned(),
                serde_json::json!(source_artifact_id),
            ),
            ("start_line".to_owned(), serde_json::json!(start_line)),
        ]),
    )
    .map_err(Into::into)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSupportLossSummaryV3 {
    reason: ContextWindowLossReasonV2,
    cardinality: ContextKnownCardinalityV3,
    observed_count: u64,
    sorted_anchor_id_set_sha256: ContentHash,
}

impl ContextSupportLossSummaryV3 {
    pub const fn reason(&self) -> ContextWindowLossReasonV2 {
        self.reason
    }
    pub const fn observed_count(&self) -> u64 {
        self.observed_count
    }
    pub fn sorted_anchor_id_set_sha256(&self) -> &ContentHash {
        &self.sorted_anchor_id_set_sha256
    }
}

fn support_loss_summaries_v3(
    losses: &BTreeMap<ContextWindowLossReasonV2, BTreeSet<StableId>>,
) -> ContextResult<Vec<ContextSupportLossSummaryV3>> {
    if losses.len() > 15 {
        return Err(incomplete("context v3 support loss summaries", 15, losses.len()).into());
    }
    losses
        .iter()
        .map(|(reason, ids)| {
            if ids.is_empty() {
                return Err(DomainError::Validation(
                    "context v3 support loss summary cannot be empty".to_owned(),
                )
                .into());
            }
            let commitment = denominator_commitment_v3("context v3 support losses", ids)?;
            Ok(ContextSupportLossSummaryV3 {
                reason: *reason,
                cardinality: commitment.cardinality,
                observed_count: commitment.observed_count,
                sorted_anchor_id_set_sha256: commitment.sorted_id_set_sha256,
            })
        })
        .collect()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextWindowV3 {
    id: StableId,
    source_artifact_id: StableId,
    registration_id: StableId,
    content_hash: ContentHash,
    cas_hash: ContentHash,
    range: ExcerptRange,
    owner_ids: BTreeSet<StableId>,
    roles: BTreeSet<ContextWindowRoleV2>,
    support_anchor_ids: BTreeSet<StableId>,
    excerpt_byte_length: u64,
    excerpt_hash: ContentHash,
}

impl ContextWindowV3 {
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn source_artifact_id(&self) -> &StableId {
        &self.source_artifact_id
    }
    pub fn range(&self) -> &ExcerptRange {
        &self.range
    }
    pub fn roles(&self) -> &BTreeSet<ContextWindowRoleV2> {
        &self.roles
    }
    pub fn support_anchor_ids(&self) -> &BTreeSet<StableId> {
        &self.support_anchor_ids
    }
}

struct ContextWindowIdentityV3<'a> {
    source_artifact_id: &'a StableId,
    registration_id: &'a StableId,
    content_hash: &'a ContentHash,
    cas_hash: &'a ContentHash,
    range: &'a ExcerptRange,
    owner_ids: &'a BTreeSet<StableId>,
    roles: &'a BTreeSet<ContextWindowRoleV2>,
    support_anchor_ids: &'a BTreeSet<StableId>,
    excerpt_byte_length: u64,
    excerpt_hash: &'a ContentHash,
}

fn context_window_id_v3(
    snapshot_id: &StableId,
    obligation_id: &StableId,
    identity: &ContextWindowIdentityV3<'_>,
) -> ContextResult<StableId> {
    StableId::derived(
        "context-window",
        &BTreeMap::from([
            (
                "policy_id".to_owned(),
                serde_json::json!(ContextSubjectWindowsPolicyV3::ID),
            ),
            ("snapshot_id".to_owned(), serde_json::json!(snapshot_id)),
            ("obligation_id".to_owned(), serde_json::json!(obligation_id)),
            (
                "source_artifact_id".to_owned(),
                serde_json::json!(identity.source_artifact_id),
            ),
            (
                "registration_id".to_owned(),
                serde_json::json!(identity.registration_id),
            ),
            (
                "content_hash".to_owned(),
                serde_json::json!(identity.content_hash),
            ),
            ("cas_hash".to_owned(), serde_json::json!(identity.cas_hash)),
            ("range".to_owned(), serde_json::json!(identity.range)),
            (
                "owner_ids".to_owned(),
                serde_json::json!(identity.owner_ids),
            ),
            ("roles".to_owned(), serde_json::json!(identity.roles)),
            (
                "support_anchor_ids".to_owned(),
                serde_json::json!(identity.support_anchor_ids),
            ),
            (
                "excerpt_byte_length".to_owned(),
                serde_json::json!(identity.excerpt_byte_length),
            ),
            (
                "excerpt_hash".to_owned(),
                serde_json::json!(identity.excerpt_hash),
            ),
        ]),
    )
    .map_err(Into::into)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSubjectLossV3 {
    id: StableId,
    endpoint_id: StableId,
    role: ContextWindowRoleV2,
    reason: ContextWindowLossReasonV2,
    source_artifact_id: Option<StableId>,
    requested_range: Option<ExcerptRange>,
    property_id: String,
    severity: Severity,
}

impl ContextSubjectLossV3 {
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn endpoint_id(&self) -> &StableId {
        &self.endpoint_id
    }
    pub const fn reason(&self) -> ContextWindowLossReasonV2 {
        self.reason
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, tag = "state", rename_all = "snake_case")]
pub enum ContextSubjectOutcomeV3 {
    Admitted {
        endpoint_id: StableId,
        role: ContextWindowRoleV2,
        source_artifact_id: StableId,
        requested_range: ExcerptRange,
        window_id: StableId,
    },
    Lost {
        loss: ContextSubjectLossV3,
    },
}

impl ContextSubjectOutcomeV3 {
    pub fn endpoint_id(&self) -> &StableId {
        match self {
            Self::Admitted { endpoint_id, .. } => endpoint_id,
            Self::Lost { loss } => &loss.endpoint_id,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMaterializedSourceV3 {
    artifact_id: StableId,
    registration_id: StableId,
    content_hash: ContentHash,
    cas_hash: ContentHash,
    path: String,
    size: u64,
    line_count: u64,
    exclusion: Option<ExclusionReason>,
}

impl ContextMaterializedSourceV3 {
    pub fn artifact_id(&self) -> &StableId {
        &self.artifact_id
    }
    pub const fn exclusion(&self) -> Option<ExclusionReason> {
        self.exclusion
    }
}

const fn window_reason_from_exclusion(reason: ExclusionReason) -> ContextWindowLossReasonV2 {
    match reason {
        ExclusionReason::PathCap => ContextWindowLossReasonV2::PathCap,
        ExclusionReason::TestCap => ContextWindowLossReasonV2::TestCap,
        ExclusionReason::NotReached => ContextWindowLossReasonV2::NotReached,
        ExclusionReason::IncludedFileCap => ContextWindowLossReasonV2::IncludedFileCap,
        ExclusionReason::ArtifactBytesCap => ContextWindowLossReasonV2::ArtifactBytesCap,
        ExclusionReason::TotalResolvedBytesCap => ContextWindowLossReasonV2::TotalResolvedBytesCap,
        ExclusionReason::GiantLine => ContextWindowLossReasonV2::GiantLine,
        ExclusionReason::ExcerptBytesCap => ContextWindowLossReasonV2::PerWindowBytes,
        ExclusionReason::TotalExcerptBytesCap => ContextWindowLossReasonV2::TotalExcerptBytes,
    }
}

const SUBJECT_WINDOWS_D_RULE: &str = "relation.changed_public_callee@1";
const SUBJECT_WINDOWS_D_PROPERTY: &str = "rust.callee_contract_review@1";

fn validate_subject_binding_v2(
    program: &ProgramSpace,
    obligation: &Obligation,
    caller_artifact_id: &StableId,
    callee_artifact_id: &StableId,
) -> ContextResult<()> {
    if obligation.version().rule() != SUBJECT_WINDOWS_D_RULE {
        return Err(ContextSubjectBindingErrorV2::WrongRule {
            observed: obligation.version().rule().to_owned(),
        }
        .into());
    }
    if obligation.property_id() != SUBJECT_WINDOWS_D_PROPERTY {
        return Err(ContextSubjectBindingErrorV2::WrongProperty {
            observed: obligation.property_id().to_owned(),
        }
        .into());
    }
    if obligation.target_kind() != "relation" {
        return Err(ContextSubjectBindingErrorV2::WrongTargetKind {
            observed: obligation.target_kind().to_owned(),
        }
        .into());
    }
    let [relation_id] = obligation.target_refs() else {
        return Err(ContextSubjectBindingErrorV2::ObligationTargetCardinality {
            observed: obligation.target_refs().len(),
        }
        .into());
    };
    let relation = program.relation(relation_id).ok_or_else(|| {
        ContextSubjectBindingErrorV2::TargetRelationNotAccepted {
            relation_id: relation_id.clone(),
        }
    })?;
    if relation.target_ids.len() != 1 {
        return Err(ContextSubjectBindingErrorV2::RelationTargetCardinality {
            relation_id: relation.id.clone(),
            observed: relation.target_ids.len(),
        }
        .into());
    }
    let expected_callee = relation
        .target_ids
        .first()
        .expect("relation target cardinality checked");
    for endpoint_id in [&relation.source_id, expected_callee] {
        if program.artifact(endpoint_id).is_none() {
            return Err(ContextSubjectBindingErrorV2::RelationEndpointNotAccepted {
                artifact_id: endpoint_id.clone(),
            }
            .into());
        }
    }
    for (role, endpoint_id) in [
        ("caller", caller_artifact_id),
        ("callee", callee_artifact_id),
    ] {
        if program.artifact(endpoint_id).is_none() {
            return Err(ContextSubjectBindingErrorV2::ProvidedEndpointNotAccepted {
                role,
                artifact_id: endpoint_id.clone(),
            }
            .into());
        }
    }
    if caller_artifact_id != &relation.source_id {
        return Err(ContextSubjectBindingErrorV2::CallerMismatch {
            expected: relation.source_id.clone(),
            observed: caller_artifact_id.clone(),
        }
        .into());
    }
    if callee_artifact_id != expected_callee {
        return Err(ContextSubjectBindingErrorV2::CalleeMismatch {
            expected: expected_callee.clone(),
            observed: callee_artifact_id.clone(),
        }
        .into());
    }
    Ok(())
}

/// Prepares a v2 subject-window resolver session for explicit relation
/// endpoints. C7 must pass the resolved caller and callee artifact IDs; this
/// function validates them against the accepted, single-target relation named
/// by the substantive D obligation before requesting any source bytes.
pub fn prepare_subject_windows_v2(
    aggregate: &ReviewAggregate,
    obligation_id: StableId,
    caller_artifact_id: StableId,
    callee_artifact_id: StableId,
) -> ContextResult<ContextSubjectWindowsSessionV2> {
    prepare_subject_windows_v2_with_probe(
        aggregate,
        obligation_id,
        caller_artifact_id,
        callee_artifact_id,
        None,
    )
}

/// Probe-enabled form of [`prepare_subject_windows_v2`]. The observer is
/// operational only and cannot affect the returned session.
pub fn prepare_subject_windows_v2_with_probe(
    aggregate: &ReviewAggregate,
    obligation_id: StableId,
    caller_artifact_id: StableId,
    callee_artifact_id: StableId,
    effect_probe: Option<Arc<dyn ContextBuildProbe>>,
) -> ContextResult<ContextSubjectWindowsSessionV2> {
    let program = aggregate.program();
    let obligation = aggregate
        .obligation(&obligation_id)
        .ok_or_else(|| DomainError::DanglingReference {
            owner: "v2 context builder",
            owner_id: obligation_id.clone(),
            reference: obligation_id.clone(),
        })?
        .clone();
    validate_subject_binding_v2(
        program,
        &obligation,
        &caller_artifact_id,
        &callee_artifact_id,
    )?;
    let snapshot_id = program.snapshot_id().clone();
    if obligation.version().snapshot() != &snapshot_id {
        return Err(DomainError::Validation(
            "context obligation must bind aggregate snapshot".to_owned(),
        )
        .into());
    }
    text(obligation.property_id())?;
    let sources = aggregate
        .snapshot_sources_for(&snapshot_id)
        .ok_or_else(|| {
            DomainError::Validation("missing exact snapshot source closure".to_owned())
        })?;
    let mut preflight_count = 0_usize;
    for artifact in program
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
    {
        preflight_count = preflight_count
            .checked_add(1)
            .ok_or_else(|| incomplete("context candidate files", MAX, usize::MAX))?;
        probe(
            &effect_probe,
            ContextBuildEffect::CandidateMetadataVisit {
                artifact_id: artifact.id.clone(),
            },
        );
        if preflight_count > MAX {
            probe(
                &effect_probe,
                ContextBuildEffect::LimitFailure {
                    operation: "context candidate files",
                    limit: MAX,
                    observed: preflight_count,
                },
            );
            return Err(incomplete("context candidate files", MAX, preflight_count).into());
        }
    }
    let mut candidates = candidates(aggregate, program, sources, &snapshot_id)?;
    let (
        reached,
        structural,
        file_for,
        _direct_files,
        distances,
        ranks,
        path_cap,
        test_cap,
        mut unknowns,
        _,
    ) = discover(program, &obligation)?;
    if unknowns.len() > 64 {
        return Err(incomplete("context unknowns", 64, unknowns.len()).into());
    }
    unknowns.sort_by(|left, right| {
        (&left.description, &left.source_ids).cmp(&(&right.description, &right.source_ids))
    });

    let mut expectations = Vec::with_capacity(2);
    for (endpoint_id, role) in [
        (callee_artifact_id.clone(), ContextWindowRoleV2::Callee),
        (caller_artifact_id.clone(), ContextWindowRoleV2::Caller),
    ] {
        let Some(artifact) = program.artifact(&endpoint_id) else {
            expectations.push(SubjectExpectationV2 {
                endpoint_id,
                role,
                source_artifact_id: None,
                range: None,
                preflight_loss: Some(ContextWindowLossReasonV2::MissingSource),
            });
            continue;
        };
        let Some(location) = artifact.location.as_ref() else {
            expectations.push(SubjectExpectationV2 {
                endpoint_id,
                role,
                source_artifact_id: None,
                range: None,
                preflight_loss: Some(ContextWindowLossReasonV2::MissingLocation),
            });
            continue;
        };
        let (Some(start), Some(end)) = (location.start_line, location.end_line) else {
            expectations.push(SubjectExpectationV2 {
                endpoint_id,
                role,
                source_artifact_id: None,
                range: None,
                preflight_loss: Some(ContextWindowLossReasonV2::MissingLocation),
            });
            continue;
        };
        let matching = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.source.path() == location.path)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if matching.len() > 1 {
            return Err(DomainError::Validation(format!(
                "subject endpoint {} path resolves to multiple source candidates",
                endpoint_id
            ))
            .into());
        }
        let Some(index) = matching.first().copied() else {
            expectations.push(SubjectExpectationV2 {
                endpoint_id,
                role,
                source_artifact_id: None,
                range: None,
                preflight_loss: Some(ContextWindowLossReasonV2::MissingSource),
            });
            continue;
        };
        let candidate = &candidates[index];
        if start == 0 || end < start || end > candidate.source.line_count() {
            expectations.push(SubjectExpectationV2 {
                endpoint_id,
                role,
                source_artifact_id: Some(candidate.artifact.id.clone()),
                range: u32::try_from(start).ok().zip(u32::try_from(end).ok()).map(
                    |(start_line, end_line)| ExcerptRange {
                        start_line,
                        end_line,
                    },
                ),
                preflight_loss: Some(ContextWindowLossReasonV2::MissingLocation),
            });
            continue;
        }
        expectations.push(SubjectExpectationV2 {
            endpoint_id,
            role,
            source_artifact_id: Some(candidate.artifact.id.clone()),
            range: Some(ExcerptRange {
                start_line: u32::try_from(start)
                    .map_err(|_| incomplete("subject start line", u32::MAX as usize, usize::MAX))?,
                end_line: u32::try_from(end)
                    .map_err(|_| incomplete("subject end line", u32::MAX as usize, usize::MAX))?,
            }),
            preflight_loss: None,
        });
    }

    let subject_priority = expectations
        .iter()
        .filter_map(|expectation| {
            expectation.source_artifact_id.as_ref().map(|source_id| {
                (
                    source_id.clone(),
                    match expectation.role {
                        ContextWindowRoleV2::Callee => 0_u8,
                        ContextWindowRoleV2::Caller => 1_u8,
                        ContextWindowRoleV2::Support => 2_u8,
                    },
                )
            })
        })
        .fold(
            BTreeMap::<StableId, u8>::new(),
            |mut priorities, (id, priority)| {
                priorities
                    .entry(id)
                    .and_modify(|current| *current = (*current).min(priority))
                    .or_insert(priority);
                priorities
            },
        );
    for candidate in &mut candidates {
        candidate.exclusion = if subject_priority.contains_key(&candidate.artifact.id) {
            None
        } else {
            discovery_exclusion(&candidate.artifact.id, &path_cap, &test_cap, &reached)
        };
        candidate.rank = (
            subject_priority
                .get(&candidate.artifact.id)
                .copied()
                .unwrap_or(2),
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
        let Some(owners) = file_for.get(&artifact.id) else {
            continue;
        };
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
    candidates.sort_by(|left, right| left.rank.cmp(&right.rank));
    let policy_hash = ContextSubjectWindowsPolicyV2::fixed().hash();
    let session_digest = manifest_digest(&snapshot_id, obligation.id(), &policy_hash, &candidates)?;
    Ok(ContextSubjectWindowsSessionV2 {
        snapshot_id,
        obligation,
        policy_hash,
        caller_artifact_id,
        callee_artifact_id,
        expectations,
        candidates,
        unknowns,
        index: 0,
        pending: None,
        resolved: BTreeMap::new(),
        excluded: Vec::new(),
        resolved_bytes: 0,
        session_digest,
        effect_probe,
    })
}

fn validate_accepted_file_closure_v3(
    aggregate: &ReviewAggregate,
    program: &ProgramSpace,
    sources: &crate::SnapshotSourcesRecorded,
    snapshot_id: &StableId,
    accepted_file_bound: usize,
) -> ContextResult<BTreeSet<StableId>> {
    let accepted = program
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
        .map(|artifact| artifact.id.clone())
        .collect::<BTreeSet<_>>();
    if accepted.len() > accepted_file_bound {
        return Err(incomplete(
            "context v3 accepted file denominator",
            accepted_file_bound,
            accepted.len(),
        )
        .into());
    }
    if sources.entries().len() != accepted.len() {
        return Err(DomainError::Validation(
            "v3 snapshot source closure must exactly cover accepted files".to_owned(),
        )
        .into());
    }
    for artifact in program
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
    {
        let source = sources
            .entries()
            .iter()
            .find(|entry| entry.artifact_id() == &artifact.id)
            .ok_or_else(|| DomainError::Validation("missing v3 accepted file source".to_owned()))?;
        let registration = aggregate
            .artifact_registration(source.registration_id())
            .ok_or_else(|| {
                DomainError::Validation("missing v3 accepted file registration".to_owned())
            })?;
        if source.path()
            != artifact
                .location
                .as_ref()
                .map_or("", |location| location.path.as_str())
            || artifact.content_hash.as_ref() != Some(source.content_hash())
            || registration.cas_hash() != source.cas_hash()
            || registration.sensitivity() != ArtifactSensitivity::WorkspaceSource
            || !registration.is_snapshot_ingest(snapshot_id)
        {
            return Err(DomainError::Validation(
                "v3 accepted file closure does not match accepted snapshot state".to_owned(),
            )
            .into());
        }
    }
    #[cfg(test)]
    let accepted = {
        let mut accepted = accepted;
        OMIT_ONE_ACCEPTED_FILE_IN_BUILDER_MUTANT.with(|enabled| {
            if enabled.get()
                && let Some(last) = accepted.last().cloned()
            {
                accepted.remove(&last);
            }
        });
        accepted
    };
    Ok(accepted)
}

fn materialized_denominators_v3(
    accepted: &BTreeSet<StableId>,
    reached: &BTreeSet<StableId>,
    subject_files: &BTreeSet<StableId>,
) -> ContextResult<(
    ContextDenominatorCommitmentV3,
    ContextDenominatorCommitmentV3,
    BTreeSet<StableId>,
    ContextDenominatorCommitmentV3,
)> {
    if !reached.is_subset(accepted) || !subject_files.is_subset(accepted) {
        return Err(DomainError::Validation(
            "v3 reached and subject files must belong to the accepted file denominator".to_owned(),
        )
        .into());
    }
    let materialized = reached
        .union(subject_files)
        .cloned()
        .collect::<BTreeSet<_>>();
    if materialized.len() > MAX {
        return Err(incomplete(
            "context v3 materialized source candidates",
            MAX,
            materialized.len(),
        )
        .into());
    }
    Ok((
        denominator_commitment_v3("context v3 accepted files", accepted)?,
        denominator_commitment_v3("context v3 reached files", reached)?,
        materialized.clone(),
        denominator_commitment_v3("context v3 materialized sources", &materialized)?,
    ))
}

fn candidate_for_v3(
    aggregate: &ReviewAggregate,
    program: &ProgramSpace,
    sources: &crate::SnapshotSourcesRecorded,
    artifact_id: &StableId,
) -> ContextResult<Candidate> {
    let artifact = program.artifact(artifact_id).ok_or_else(|| {
        DomainError::Validation("v3 materialized source is not accepted".to_owned())
    })?;
    if artifact.kind != "file" {
        return Err(
            DomainError::Validation("v3 materialized source is not a file".to_owned()).into(),
        );
    }
    let source = sources
        .entries()
        .iter()
        .find(|entry| entry.artifact_id() == artifact_id)
        .ok_or_else(|| DomainError::Validation("missing v3 materialized source".to_owned()))?;
    let registration = aggregate
        .artifact_registration(source.registration_id())
        .ok_or_else(|| DomainError::Validation("missing v3 source registration".to_owned()))?;
    Ok(Candidate {
        artifact: artifact.clone(),
        source: source.clone(),
        registration_size: registration.size(),
        rank: (2, usize::MAX, usize::MAX, artifact.id.clone()),
        exclusion: None,
        anchors: Vec::new(),
    })
}

struct SubjectLossV3Input {
    endpoint_id: StableId,
    role: ContextWindowRoleV2,
    reason: ContextWindowLossReasonV2,
    source_artifact_id: Option<StableId>,
    requested_range: Option<ExcerptRange>,
}

fn subject_loss_v3(
    snapshot_id: &StableId,
    obligation_id: &StableId,
    property_id: &str,
    input: SubjectLossV3Input,
) -> ContextResult<ContextSubjectLossV3> {
    let id = subject_loss_id_v3(snapshot_id, obligation_id, property_id, &input)?;
    let SubjectLossV3Input {
        endpoint_id,
        role,
        reason,
        source_artifact_id,
        requested_range,
    } = input;
    Ok(ContextSubjectLossV3 {
        id,
        endpoint_id,
        role,
        reason,
        source_artifact_id,
        requested_range,
        property_id: property_id.to_owned(),
        severity: Severity::High,
    })
}

fn subject_loss_id_v3(
    snapshot_id: &StableId,
    obligation_id: &StableId,
    property_id: &str,
    input: &SubjectLossV3Input,
) -> ContextResult<StableId> {
    let bindings = BTreeMap::from([
        (
            "policy_id".to_owned(),
            serde_json::json!(ContextSubjectWindowsPolicyV3::ID),
        ),
        ("snapshot_id".to_owned(), serde_json::json!(snapshot_id)),
        ("obligation_id".to_owned(), serde_json::json!(obligation_id)),
        (
            "endpoint_id".to_owned(),
            serde_json::json!(input.endpoint_id),
        ),
        ("role".to_owned(), serde_json::json!(input.role)),
        ("reason".to_owned(), serde_json::json!(input.reason)),
        (
            "source_artifact_id".to_owned(),
            serde_json::json!(input.source_artifact_id),
        ),
        (
            "requested_range".to_owned(),
            serde_json::json!(input.requested_range),
        ),
        ("property_id".to_owned(), serde_json::json!(property_id)),
        ("severity".to_owned(), serde_json::json!(Severity::High)),
    ]);
    StableId::derived("context-loss", &bindings).map_err(Into::into)
}

#[derive(Debug)]
pub struct ContextSubjectWindowsSessionV3 {
    snapshot_id: StableId,
    obligation: Obligation,
    policy_hash: ContentHash,
    caller_artifact_id: StableId,
    callee_artifact_id: StableId,
    expectations: Vec<SubjectExpectationV2>,
    candidates: Vec<Candidate>,
    unknowns: Vec<EnvelopeUnknown>,
    accepted_file_denominator: ContextDenominatorCommitmentV3,
    reached_file_denominator: ContextDenominatorCommitmentV3,
    materialized_source_denominator: ContextDenominatorCommitmentV3,
    support_anchor_denominator: ContextDenominatorCommitmentV3,
    support_anchor_ids: BTreeMap<(StableId, u64, u64, StableId), StableId>,
    latent_cardinality: ContextLatentCardinalityV3,
    index: usize,
    pending: Option<ContextSourceRequest>,
    resolved: BTreeMap<StableId, ResolvedSourceV2>,
    excluded: Vec<ExcludedSourceRef>,
    resolved_bytes: u64,
    session_digest: ContentHash,
    effect_probe: Option<Arc<dyn ContextBuildProbe>>,
}

fn individual_unknowns_v3(
    grouped_unknowns: Vec<EnvelopeUnknown>,
) -> ContextResult<Vec<EnvelopeUnknown>> {
    let mut unknowns = grouped_unknowns
        .into_iter()
        .flat_map(|unknown| {
            unknown
                .source_ids
                .into_iter()
                .map(move |source_id| EnvelopeUnknown {
                    description: unknown.description.clone(),
                    source_ids: BTreeSet::from([source_id]),
                })
        })
        .collect::<Vec<_>>();
    unknowns.sort_by(|left, right| {
        (&left.description, &left.source_ids).cmp(&(&right.description, &right.source_ids))
    });
    if unknowns.len() > 64 {
        let digest = ContentHash::sha256(
            &serde_json::to_vec(&unknowns).map_err(|error| DomainError::Json(error.to_string()))?,
        );
        return Err(ContextError::V3UnknownOverflow {
            limit: 64,
            observed: unknowns.len(),
            sorted_unknown_set_sha256: digest,
        });
    }
    Ok(unknowns)
}

/// Prepares the request-v3-only context session. `accepted_file_bound` must be
/// the already validated `request.ingest.max_files` value; it is not a policy
/// parameter and is committed by the request/run version tuple owned by C7.
pub fn prepare_subject_windows_v3(
    aggregate: &ReviewAggregate,
    obligation_id: StableId,
    caller_artifact_id: StableId,
    callee_artifact_id: StableId,
    accepted_file_bound: usize,
) -> ContextResult<ContextSubjectWindowsSessionV3> {
    prepare_subject_windows_v3_with_probe(
        aggregate,
        obligation_id,
        caller_artifact_id,
        callee_artifact_id,
        accepted_file_bound,
        None,
    )
}

/// Probe-enabled form of [`prepare_subject_windows_v3`].
pub fn prepare_subject_windows_v3_with_probe(
    aggregate: &ReviewAggregate,
    obligation_id: StableId,
    caller_artifact_id: StableId,
    callee_artifact_id: StableId,
    accepted_file_bound: usize,
    effect_probe: Option<Arc<dyn ContextBuildProbe>>,
) -> ContextResult<ContextSubjectWindowsSessionV3> {
    let program = aggregate.program();
    let obligation = aggregate
        .obligation(&obligation_id)
        .ok_or_else(|| DomainError::DanglingReference {
            owner: "v3 context builder",
            owner_id: obligation_id.clone(),
            reference: obligation_id,
        })?
        .clone();
    validate_subject_binding_v2(
        program,
        &obligation,
        &caller_artifact_id,
        &callee_artifact_id,
    )?;
    let snapshot_id = program.snapshot_id().clone();
    if obligation.version().snapshot() != &snapshot_id {
        return Err(DomainError::Validation(
            "v3 context obligation must bind aggregate snapshot".to_owned(),
        )
        .into());
    }
    let sources = aggregate
        .snapshot_sources_for(&snapshot_id)
        .ok_or_else(|| {
            DomainError::Validation("missing exact v3 snapshot source closure".to_owned())
        })?;
    let accepted_files = validate_accepted_file_closure_v3(
        aggregate,
        program,
        sources,
        &snapshot_id,
        accepted_file_bound,
    )?;
    let (
        reached,
        structural,
        file_for,
        _direct_files,
        distances,
        ranks,
        path_cap,
        test_cap,
        grouped_unknowns,
        _,
    ) = discover(program, &obligation)?;
    #[cfg(test)]
    let mut reached = reached;
    #[cfg(test)]
    OMIT_ONE_REACHED_FILE_IN_BUILDER_MUTANT.with(|enabled| {
        if enabled.get()
            && let Some(last) = reached.last().cloned()
        {
            reached.remove(&last);
        }
    });
    let unknowns = individual_unknowns_v3(grouped_unknowns)?;

    let mut expectations = Vec::with_capacity(2);
    for (endpoint_id, role) in [
        (callee_artifact_id.clone(), ContextWindowRoleV2::Callee),
        (caller_artifact_id.clone(), ContextWindowRoleV2::Caller),
    ] {
        let artifact = program
            .artifact(&endpoint_id)
            .expect("binding validated endpoint");
        let Some(location) = artifact.location.as_ref() else {
            expectations.push(SubjectExpectationV2 {
                endpoint_id,
                role,
                source_artifact_id: None,
                range: None,
                preflight_loss: Some(ContextWindowLossReasonV2::MissingLocation),
            });
            continue;
        };
        let matching = program
            .artifacts()
            .iter()
            .filter(|candidate| {
                candidate.kind == "file"
                    && candidate
                        .location
                        .as_ref()
                        .is_some_and(|candidate_location| candidate_location.path == location.path)
            })
            .collect::<Vec<_>>();
        if matching.len() > 1 {
            return Err(DomainError::Validation(format!(
                "v3 subject endpoint {endpoint_id} path resolves to multiple accepted files"
            ))
            .into());
        }
        let Some(file) = matching.first() else {
            expectations.push(SubjectExpectationV2 {
                endpoint_id,
                role,
                source_artifact_id: None,
                range: None,
                preflight_loss: Some(ContextWindowLossReasonV2::MissingSource),
            });
            continue;
        };
        let range = location
            .start_line
            .zip(location.end_line)
            .and_then(|(start, end)| {
                u32::try_from(start).ok().zip(u32::try_from(end).ok()).map(
                    |(start_line, end_line)| ExcerptRange {
                        start_line,
                        end_line,
                    },
                )
            });
        let source = sources
            .entries()
            .iter()
            .find(|entry| entry.artifact_id() == &file.id)
            .expect("accepted closure validated");
        let invalid = range.as_ref().is_none_or(|range| {
            range.start_line == 0
                || range.end_line < range.start_line
                || u64::from(range.end_line) > source.line_count()
        });
        expectations.push(SubjectExpectationV2 {
            endpoint_id,
            role,
            source_artifact_id: Some(file.id.clone()),
            range,
            preflight_loss: invalid.then_some(ContextWindowLossReasonV2::MissingLocation),
        });
    }
    let subject_files = expectations
        .iter()
        .filter_map(|expectation| expectation.source_artifact_id.clone())
        .collect::<BTreeSet<_>>();
    let (
        accepted_file_denominator,
        reached_file_denominator,
        materialized_ids,
        materialized_source_denominator,
    ) = materialized_denominators_v3(&accepted_files, &reached, &subject_files)?;
    probe(
        &effect_probe,
        ContextBuildEffect::DenominatorCommitmentLookup,
    );
    for artifact_id in &materialized_ids {
        probe(
            &effect_probe,
            ContextBuildEffect::CandidateMaterialized {
                artifact_id: artifact_id.clone(),
            },
        );
        probe(
            &effect_probe,
            ContextBuildEffect::CandidateMetadataVisit {
                artifact_id: artifact_id.clone(),
            },
        );
    }

    let subject_priority = expectations
        .iter()
        .filter_map(|expectation| {
            expectation
                .source_artifact_id
                .as_ref()
                .map(|id| (id.clone(), window_role_priority(expectation.role)))
        })
        .fold(
            BTreeMap::<StableId, u8>::new(),
            |mut priorities, (id, priority)| {
                priorities
                    .entry(id)
                    .and_modify(|current| *current = (*current).min(priority))
                    .or_insert(priority);
                priorities
            },
        );
    let mut candidates = materialized_ids
        .iter()
        .map(|id| candidate_for_v3(aggregate, program, sources, id))
        .collect::<ContextResult<Vec<_>>>()?;
    for candidate in &mut candidates {
        candidate.exclusion = if subject_priority.contains_key(&candidate.artifact.id) {
            None
        } else {
            discovery_exclusion(&candidate.artifact.id, &path_cap, &test_cap, &reached)
        };
        candidate.rank = (
            subject_priority
                .get(&candidate.artifact.id)
                .copied()
                .unwrap_or(2),
            *distances.get(&candidate.artifact.id).unwrap_or(&usize::MAX),
            *ranks.get(&candidate.artifact.id).unwrap_or(&usize::MAX),
            candidate.artifact.id.clone(),
        );
    }
    let indexes = candidates
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
        let Some(owners) = file_for.get(&artifact.id) else {
            continue;
        };
        let mut matched = false;
        for owner in owners {
            let Some(index) = indexes.get(owner).copied() else {
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
                "v3 reached range-bearing artifact {} has no exact-path containing file",
                artifact.id
            ))
            .into());
        }
    }
    #[cfg(test)]
    OMIT_ONE_ANCHOR_FILE_IN_BUILDER_MUTANT.with(|enabled| {
        if enabled.get()
            && let Some(candidate) = candidates
                .iter_mut()
                .find(|candidate| !candidate.anchors.is_empty())
        {
            candidate.anchors.clear();
        }
    });
    let mut support_anchor_ids = BTreeMap::new();
    for candidate in &mut candidates {
        candidate.anchors.sort();
        candidate.anchors.dedup();
        if candidate.anchors.len() > MAX_ANCHORS {
            return Err(incomplete(
                "context v3 anchors per file",
                MAX_ANCHORS,
                candidate.anchors.len(),
            )
            .into());
        }
        for (start, end, owner) in &candidate.anchors {
            if *start == 0 || end < start || *end > candidate.source.line_count() {
                return Err(DomainError::Validation(
                    "v3 support anchor range is invalid".to_owned(),
                )
                .into());
            }
            let start_line = u32::try_from(*start)
                .map_err(|_| incomplete("v3 support start line", u32::MAX as usize, usize::MAX))?;
            let end_line = u32::try_from(*end)
                .map_err(|_| incomplete("v3 support end line", u32::MAX as usize, usize::MAX))?;
            let anchor_id = support_anchor_id_v3(
                &snapshot_id,
                &candidate.artifact.id,
                start_line,
                end_line,
                owner,
            )?;
            support_anchor_ids.insert(
                (candidate.artifact.id.clone(), *start, *end, owner.clone()),
                anchor_id,
            );
        }
    }
    let support_ids = support_anchor_ids
        .values()
        .cloned()
        .collect::<BTreeSet<_>>();
    let support_anchor_denominator =
        denominator_commitment_v3("context v3 support anchors", &support_ids)?;
    candidates.sort_by(|left, right| left.rank.cmp(&right.rank));
    let policy_hash = ContextSubjectWindowsPolicyV3::fixed().hash();
    let session_digest = manifest_digest(&snapshot_id, obligation.id(), &policy_hash, &candidates)?;
    Ok(ContextSubjectWindowsSessionV3 {
        snapshot_id,
        obligation,
        policy_hash,
        caller_artifact_id,
        callee_artifact_id,
        expectations,
        candidates,
        unknowns,
        accepted_file_denominator,
        reached_file_denominator,
        materialized_source_denominator,
        support_anchor_denominator,
        support_anchor_ids,
        latent_cardinality: latent_cardinality_v3(program)?,
        index: 0,
        pending: None,
        resolved: BTreeMap::new(),
        excluded: Vec::new(),
        resolved_bytes: 0,
        session_digest,
        effect_probe,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BuiltContextSubjectWindowsV3 {
    #[serde(rename = "context_id")]
    id: StableId,
    projection_hash: ContentHash,
    #[serde(rename = "context_policy")]
    policy: ContextSubjectWindowsPolicyV3,
    #[serde(rename = "context_policy_hash")]
    policy_hash: ContentHash,
    snapshot_id: StableId,
    obligation_id: StableId,
    property_id: String,
    target_refs: Vec<StableId>,
    caller_artifact_id: StableId,
    callee_artifact_id: StableId,
    accepted_file_denominator: ContextDenominatorCommitmentV3,
    reached_file_denominator: ContextDenominatorCommitmentV3,
    materialized_source_denominator: ContextDenominatorCommitmentV3,
    support_anchor_denominator: ContextDenominatorCommitmentV3,
    latent_cardinality: ContextLatentCardinalityV3,
    subject_outcomes: Vec<ContextSubjectOutcomeV3>,
    materialized_sources: Vec<ContextMaterializedSourceV3>,
    windows: Vec<ContextWindowV3>,
    support_loss_summaries: Vec<ContextSupportLossSummaryV3>,
    unknowns: Vec<EnvelopeUnknown>,
}

impl BuiltContextSubjectWindowsV3 {
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn projection_hash(&self) -> &ContentHash {
        &self.projection_hash
    }
    pub const fn policy(&self) -> ContextSubjectWindowsPolicyV3 {
        self.policy
    }
    pub fn policy_hash(&self) -> &ContentHash {
        &self.policy_hash
    }
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    pub fn obligation_id(&self) -> &StableId {
        &self.obligation_id
    }
    pub fn property_id(&self) -> &str {
        &self.property_id
    }
    pub fn target_refs(&self) -> &[StableId] {
        &self.target_refs
    }
    pub fn caller_artifact_id(&self) -> &StableId {
        &self.caller_artifact_id
    }
    pub fn callee_artifact_id(&self) -> &StableId {
        &self.callee_artifact_id
    }
    pub fn accepted_file_denominator(&self) -> &ContextDenominatorCommitmentV3 {
        &self.accepted_file_denominator
    }
    pub fn reached_file_denominator(&self) -> &ContextDenominatorCommitmentV3 {
        &self.reached_file_denominator
    }
    pub fn materialized_source_denominator(&self) -> &ContextDenominatorCommitmentV3 {
        &self.materialized_source_denominator
    }
    pub fn support_anchor_denominator(&self) -> &ContextDenominatorCommitmentV3 {
        &self.support_anchor_denominator
    }
    pub fn latent_cardinality(&self) -> &ContextLatentCardinalityV3 {
        &self.latent_cardinality
    }
    pub fn subject_outcomes(&self) -> &[ContextSubjectOutcomeV3] {
        &self.subject_outcomes
    }
    pub fn materialized_sources(&self) -> &[ContextMaterializedSourceV3] {
        &self.materialized_sources
    }
    pub fn windows(&self) -> &[ContextWindowV3] {
        &self.windows
    }
    pub fn support_loss_summaries(&self) -> &[ContextSupportLossSummaryV3] {
        &self.support_loss_summaries
    }
    pub fn unknowns(&self) -> &[EnvelopeUnknown] {
        &self.unknowns
    }

    pub fn canonical_value(&self) -> ContextResult<serde_json::Value> {
        serde_json::to_value(self).map_err(|error| DomainError::Json(error.to_string()).into())
    }
}

#[derive(Serialize)]
struct SubjectWindowsIdentityV3<'a> {
    accepted_file_denominator: &'a ContextDenominatorCommitmentV3,
    callee_artifact_id: &'a StableId,
    caller_artifact_id: &'a StableId,
    context_policy: ContextSubjectWindowsPolicyV3,
    context_policy_hash: &'a ContentHash,
    latent_cardinality: &'a ContextLatentCardinalityV3,
    materialized_source_denominator: &'a ContextDenominatorCommitmentV3,
    materialized_sources: &'a [ContextMaterializedSourceV3],
    obligation_id: &'a StableId,
    property_id: &'a str,
    reached_file_denominator: &'a ContextDenominatorCommitmentV3,
    snapshot_id: &'a StableId,
    subject_outcomes: &'a [ContextSubjectOutcomeV3],
    support_anchor_denominator: &'a ContextDenominatorCommitmentV3,
    support_loss_summaries: &'a [ContextSupportLossSummaryV3],
    target_refs: &'a [StableId],
    unknowns: &'a [EnvelopeUnknown],
    windows: &'a [ContextWindowV3],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContextSubjectWindowsV3 {
    context_id: StableId,
    projection_hash: ContentHash,
    context_policy: ContextSubjectWindowsPolicyV3,
    context_policy_hash: ContentHash,
    snapshot_id: StableId,
    obligation_id: StableId,
    property_id: String,
    target_refs: Vec<StableId>,
    caller_artifact_id: StableId,
    callee_artifact_id: StableId,
    accepted_file_denominator: ContextDenominatorCommitmentV3,
    reached_file_denominator: ContextDenominatorCommitmentV3,
    materialized_source_denominator: ContextDenominatorCommitmentV3,
    support_anchor_denominator: ContextDenominatorCommitmentV3,
    latent_cardinality: ContextLatentCardinalityV3,
    subject_outcomes: Vec<ContextSubjectOutcomeV3>,
    materialized_sources: Vec<ContextMaterializedSourceV3>,
    windows: Vec<ContextWindowV3>,
    support_loss_summaries: Vec<ContextSupportLossSummaryV3>,
    unknowns: Vec<EnvelopeUnknown>,
}

impl RawContextSubjectWindowsV3 {
    fn into_context(self) -> BuiltContextSubjectWindowsV3 {
        BuiltContextSubjectWindowsV3 {
            id: self.context_id,
            projection_hash: self.projection_hash,
            policy: self.context_policy,
            policy_hash: self.context_policy_hash,
            snapshot_id: self.snapshot_id,
            obligation_id: self.obligation_id,
            property_id: self.property_id,
            target_refs: self.target_refs,
            caller_artifact_id: self.caller_artifact_id,
            callee_artifact_id: self.callee_artifact_id,
            accepted_file_denominator: self.accepted_file_denominator,
            reached_file_denominator: self.reached_file_denominator,
            materialized_source_denominator: self.materialized_source_denominator,
            support_anchor_denominator: self.support_anchor_denominator,
            latent_cardinality: self.latent_cardinality,
            subject_outcomes: self.subject_outcomes,
            materialized_sources: self.materialized_sources,
            windows: self.windows,
            support_loss_summaries: self.support_loss_summaries,
            unknowns: self.unknowns,
        }
    }
}

/// Opaque success token for local, read-only validation of one sealed v3
/// context projection. It does not contain source bytes or an aggregate
/// admission capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WireValidatedContextSubjectWindowsV3 {
    context: BuiltContextSubjectWindowsV3,
}

impl WireValidatedContextSubjectWindowsV3 {
    pub fn context(&self) -> &BuiltContextSubjectWindowsV3 {
        &self.context
    }

    pub fn context_id(&self) -> &StableId {
        self.context.id()
    }

    pub fn projection_hash(&self) -> &ContentHash {
        self.context.projection_hash()
    }
}

/// Deprecated compatibility name for the bytes-only wire-validation token.
/// It never represented denominator completeness relative to ProgramSpace.
pub type ValidatedContextSubjectWindowsV3 = WireValidatedContextSubjectWindowsV3;

/// Trusted immutable basis constructed from accepted snapshot state.
///
/// [`Self::from_accepted_snapshot`] is its only construction path. All fields
/// are private, so canonical context bytes or caller-supplied denominator sets
/// cannot manufacture a basis.
#[derive(Clone)]
pub struct ContextValidationBasisV3 {
    snapshot_id: StableId,
    extractor_set_hash: ContentHash,
    policy_hash: ContentHash,
    accepted_file_bound: usize,
    obligation_id: StableId,
    caller_artifact_id: StableId,
    callee_artifact_id: StableId,
    aggregate: Arc<ReviewAggregate>,
    source_bytes_by_artifact_id: Arc<BTreeMap<StableId, Vec<u8>>>,
}

impl std::fmt::Debug for ContextValidationBasisV3 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ContextValidationBasisV3")
            .field("snapshot_id", &self.snapshot_id)
            .field("extractor_set_hash", &self.extractor_set_hash)
            .field("policy_hash", &self.policy_hash)
            .field("accepted_file_bound", &self.accepted_file_bound)
            .field("obligation_id", &self.obligation_id)
            .field("caller_artifact_id", &self.caller_artifact_id)
            .field("callee_artifact_id", &self.callee_artifact_id)
            .field(
                "accepted_artifact_count",
                &self.aggregate.program().artifacts().len(),
            )
            .field(
                "source_entry_count",
                &self.source_bytes_by_artifact_id.len(),
            )
            .field(
                "aggregate_strong_count",
                &Arc::strong_count(&self.aggregate),
            )
            .field(
                "sources_strong_count",
                &Arc::strong_count(&self.source_bytes_by_artifact_id),
            )
            .finish()
    }
}

impl ContextValidationBasisV3 {
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }

    /// Constructs a semantic basis exclusively from accepted snapshot state
    /// and its exact source index. No context, denominator, or builder-derived
    /// set is accepted as input.
    pub fn from_accepted_snapshot(
        aggregate: Arc<ReviewAggregate>,
        obligation_id: StableId,
        caller_artifact_id: StableId,
        callee_artifact_id: StableId,
        accepted_file_bound: usize,
        source_bytes_by_artifact_id: Arc<BTreeMap<StableId, Vec<u8>>>,
    ) -> ContextResult<Self> {
        let program = aggregate.program();
        let snapshot_id = program.snapshot_id().clone();
        let obligation =
            aggregate
                .obligation(&obligation_id)
                .ok_or_else(|| DomainError::DanglingReference {
                    owner: "context v3 validation basis",
                    owner_id: obligation_id.clone(),
                    reference: obligation_id.clone(),
                })?;
        validate_subject_binding_v2(
            program,
            obligation,
            &caller_artifact_id,
            &callee_artifact_id,
        )?;
        let sources = aggregate
            .snapshot_sources_for(&snapshot_id)
            .ok_or_else(|| {
                DomainError::Validation("missing basis snapshot source closure".to_owned())
            })?;
        let accepted_ids = program
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
            .map(|artifact| artifact.id.clone())
            .collect::<BTreeSet<_>>();
        if accepted_ids.len() > accepted_file_bound
            || sources.entries().len() != accepted_ids.len()
            || source_bytes_by_artifact_id.len() != accepted_ids.len()
            || source_bytes_by_artifact_id
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                != accepted_ids
        {
            return Err(DomainError::Validation(
                "basis source index must exactly cover accepted snapshot files".to_owned(),
            )
            .into());
        }
        for entry in sources.entries() {
            let bytes = &source_bytes_by_artifact_id[entry.artifact_id()];
            let length = u64::try_from(bytes.len())
                .map_err(|_| incomplete("basis source byte length", usize::MAX, bytes.len()))?;
            let line_count = bytes.iter().filter(|byte| **byte == b'\n').count() as u64 + 1;
            let hash = ContentHash::sha256(bytes);
            let registration = aggregate
                .artifact_registration(entry.registration_id())
                .ok_or_else(|| DomainError::Validation("missing basis registration".to_owned()))?;
            if hash != *entry.content_hash()
                || hash != *entry.cas_hash()
                || length != registration.size()
                || line_count != entry.line_count()
                || !registration.is_snapshot_ingest(&snapshot_id)
            {
                return Err(DomainError::Validation(
                    "basis source index does not match registered snapshot closure".to_owned(),
                )
                .into());
            }
        }
        Ok(Self {
            snapshot_id,
            extractor_set_hash: program.extractor_set_hash().clone(),
            policy_hash: ContextSubjectWindowsPolicyV3::fixed().hash(),
            accepted_file_bound,
            obligation_id,
            caller_artifact_id,
            callee_artifact_id,
            aggregate,
            source_bytes_by_artifact_id,
        })
    }
}

/// Opaque result of basis-bound semantic reconstruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticallyValidatedContextSubjectWindowsV3 {
    context: BuiltContextSubjectWindowsV3,
}

impl SemanticallyValidatedContextSubjectWindowsV3 {
    pub fn context(&self) -> &BuiltContextSubjectWindowsV3 {
        &self.context
    }
}

fn v3_validation(error: ContextSubjectWindowsV3ValidationError) -> ContextError {
    ContextError::SubjectWindowsV3Validation(error)
}

fn validate_denominator_shape_v3(
    name: &'static str,
    commitment: &ContextDenominatorCommitmentV3,
) -> ContextResult<()> {
    if commitment.cardinality != ContextKnownCardinalityV3::Known
        || !is_sha256(&commitment.sorted_id_set_sha256)
    {
        return Err(v3_validation(
            ContextSubjectWindowsV3ValidationError::Denominator { name },
        ));
    }
    Ok(())
}

fn checked_v3_u64_add(left: u64, right: u64) -> ContextResult<u64> {
    left.checked_add(right)
        .ok_or_else(|| v3_validation(ContextSubjectWindowsV3ValidationError::SupportPartition))
}

impl BuiltContextSubjectWindowsV3 {
    fn identity_hash(&self) -> ContextResult<ContentHash> {
        let identity = SubjectWindowsIdentityV3 {
            accepted_file_denominator: &self.accepted_file_denominator,
            callee_artifact_id: &self.callee_artifact_id,
            caller_artifact_id: &self.caller_artifact_id,
            context_policy: self.policy,
            context_policy_hash: &self.policy_hash,
            latent_cardinality: &self.latent_cardinality,
            materialized_source_denominator: &self.materialized_source_denominator,
            materialized_sources: &self.materialized_sources,
            obligation_id: &self.obligation_id,
            property_id: &self.property_id,
            reached_file_denominator: &self.reached_file_denominator,
            snapshot_id: &self.snapshot_id,
            subject_outcomes: &self.subject_outcomes,
            support_anchor_denominator: &self.support_anchor_denominator,
            support_loss_summaries: &self.support_loss_summaries,
            target_refs: &self.target_refs,
            unknowns: &self.unknowns,
            windows: &self.windows,
        };
        let bytes =
            serde_json::to_vec(&identity).map_err(|error| DomainError::Json(error.to_string()))?;
        if bytes.len() > MAX_BODY {
            return Err(
                incomplete("context v3 envelope canonical bytes", MAX_BODY, bytes.len()).into(),
            );
        }
        Ok(ContentHash::sha256(&bytes))
    }

    fn validate_read_only_semantics(&self) -> ContextResult<()> {
        if self.policy != ContextSubjectWindowsPolicyV3::fixed()
            || self.policy_hash.as_str() != ContextSubjectWindowsPolicyV3::GOLDEN_HASH
            || self.policy_hash != self.policy.hash()
        {
            return Err(v3_validation(
                ContextSubjectWindowsV3ValidationError::PolicyHash,
            ));
        }
        if self.snapshot_id.kind() != "snapshot"
            || self.obligation_id.kind() != "obligation"
            || self.property_id != SUBJECT_WINDOWS_D_PROPERTY
            || self.target_refs.len() != 1
            || self.target_refs[0].kind() != "relation"
        {
            return Err(v3_validation(
                ContextSubjectWindowsV3ValidationError::SubjectOutcomes,
            ));
        }

        for (name, commitment) in [
            ("accepted_file_denominator", &self.accepted_file_denominator),
            ("reached_file_denominator", &self.reached_file_denominator),
            (
                "materialized_source_denominator",
                &self.materialized_source_denominator,
            ),
            (
                "support_anchor_denominator",
                &self.support_anchor_denominator,
            ),
        ] {
            validate_denominator_shape_v3(name, commitment)?;
        }
        if self.reached_file_denominator.observed_count
            > self.materialized_source_denominator.observed_count
            || self.materialized_source_denominator.observed_count
                > self.accepted_file_denominator.observed_count
            || self
                .materialized_source_denominator
                .observed_count
                .checked_sub(self.reached_file_denominator.observed_count)
                .is_none_or(|subject_only| subject_only > 2)
        {
            return Err(v3_validation(
                ContextSubjectWindowsV3ValidationError::Denominator {
                    name: "accepted/reached/materialized closure",
                },
            ));
        }

        match &self.latent_cardinality {
            ContextLatentCardinalityV3::KnownZero => {}
            ContextLatentCardinalityV3::Unknown {
                capability_states,
                qualification_ids,
            } => {
                const GOVERNING: [&str; 4] = ["ast", "containment", "direct_calls", "test_mapping"];
                if capability_states.len() != GOVERNING.len()
                    || GOVERNING
                        .iter()
                        .any(|name| !capability_states.contains_key(*name))
                    || capability_states
                        .values()
                        .all(|state| *state == crate::CapabilityState::Complete)
                    || qualification_ids.is_empty()
                    || qualification_ids.iter().any(|id| id.kind() != "limitation")
                {
                    return Err(v3_validation(
                        ContextSubjectWindowsV3ValidationError::LatentCardinality,
                    ));
                }
            }
        }

        if self.materialized_sources.len() > MAX
            || self
                .materialized_sources
                .windows(2)
                .any(|pair| pair[0].artifact_id >= pair[1].artifact_id)
            || self.materialized_sources.iter().any(|source| {
                source.artifact_id.kind() != "file"
                    || source.registration_id.kind() != "registration"
                    || !is_sha256(&source.content_hash)
                    || source.content_hash != source.cas_hash
                    || source.path.len() > MAX_TEXT
                    || source.line_count == 0
            })
        {
            return Err(v3_validation(
                ContextSubjectWindowsV3ValidationError::Windows,
            ));
        }
        let materialized_ids = self
            .materialized_sources
            .iter()
            .map(|source| source.artifact_id.clone())
            .collect::<BTreeSet<_>>();
        let rebuilt_materialized = denominator_commitment_v3(
            "context v3 read-only materialized sources",
            &materialized_ids,
        )?;
        if rebuilt_materialized != self.materialized_source_denominator {
            return Err(v3_validation(
                ContextSubjectWindowsV3ValidationError::Denominator {
                    name: "materialized_source_denominator",
                },
            ));
        }

        if self.windows.len() > 8
            || self.windows.windows(2).any(|pair| {
                (&pair[0].source_artifact_id, &pair[0].range, &pair[0].id)
                    >= (&pair[1].source_artifact_id, &pair[1].range, &pair[1].id)
            })
        {
            return Err(v3_validation(
                ContextSubjectWindowsV3ValidationError::Windows,
            ));
        }
        let sources_by_id = self
            .materialized_sources
            .iter()
            .map(|source| (&source.artifact_id, source))
            .collect::<BTreeMap<_, _>>();
        let mut window_ids = BTreeSet::new();
        let mut windows_per_file = BTreeMap::<&StableId, usize>::new();
        let mut admitted_support_ids = BTreeSet::new();
        let mut total_excerpt_bytes = 0_u64;
        for window in &self.windows {
            let Some(source) = sources_by_id.get(&window.source_artifact_id) else {
                return Err(v3_validation(
                    ContextSubjectWindowsV3ValidationError::Windows,
                ));
            };
            let count = windows_per_file
                .entry(&window.source_artifact_id)
                .or_default();
            *count = count
                .checked_add(1)
                .ok_or_else(|| v3_validation(ContextSubjectWindowsV3ValidationError::Windows))?;
            total_excerpt_bytes = total_excerpt_bytes
                .checked_add(window.excerpt_byte_length)
                .ok_or_else(|| v3_validation(ContextSubjectWindowsV3ValidationError::Windows))?;
            if *count > 4
                || total_excerpt_bytes > MAX_TOTAL_EXCERPT as u64
                || !window_ids.insert(window.id.clone())
                || window.registration_id != source.registration_id
                || window.content_hash != source.content_hash
                || window.cas_hash != source.cas_hash
                || window.range.start_line == 0
                || window.range.end_line < window.range.start_line
                || u64::from(window.range.end_line) > source.line_count
                || inclusive_line_span(window.range.start_line, window.range.end_line)? > MAX_LINES
                || window.owner_ids.is_empty()
                || window.roles.is_empty()
                || window.excerpt_byte_length > MAX_EXCERPT as u64
                || !is_sha256(&window.excerpt_hash)
                || window
                    .support_anchor_ids
                    .iter()
                    .any(|id| id.kind() != "context-support-anchor")
                || context_window_id_v3(
                    &self.snapshot_id,
                    &self.obligation_id,
                    &ContextWindowIdentityV3 {
                        source_artifact_id: &window.source_artifact_id,
                        registration_id: &window.registration_id,
                        content_hash: &window.content_hash,
                        cas_hash: &window.cas_hash,
                        range: &window.range,
                        owner_ids: &window.owner_ids,
                        roles: &window.roles,
                        support_anchor_ids: &window.support_anchor_ids,
                        excerpt_byte_length: window.excerpt_byte_length,
                        excerpt_hash: &window.excerpt_hash,
                    },
                )? != window.id
            {
                return Err(v3_validation(
                    ContextSubjectWindowsV3ValidationError::Windows,
                ));
            }
            for anchor_id in &window.support_anchor_ids {
                if !admitted_support_ids.insert(anchor_id.clone()) {
                    return Err(v3_validation(
                        ContextSubjectWindowsV3ValidationError::SupportPartition,
                    ));
                }
            }
        }

        if self.subject_outcomes.len() != 2 {
            return Err(v3_validation(
                ContextSubjectWindowsV3ValidationError::SubjectOutcomes,
            ));
        }
        for (outcome, endpoint_id, role) in [
            (
                &self.subject_outcomes[0],
                &self.callee_artifact_id,
                ContextWindowRoleV2::Callee,
            ),
            (
                &self.subject_outcomes[1],
                &self.caller_artifact_id,
                ContextWindowRoleV2::Caller,
            ),
        ] {
            match outcome {
                ContextSubjectOutcomeV3::Admitted {
                    endpoint_id: observed_endpoint,
                    role: observed_role,
                    source_artifact_id,
                    requested_range,
                    window_id,
                } => {
                    let valid = observed_endpoint == endpoint_id
                        && *observed_role == role
                        && materialized_ids.contains(source_artifact_id)
                        && self.windows.iter().any(|window| {
                            &window.id == window_id
                                && window.source_artifact_id == *source_artifact_id
                                && window.owner_ids.contains(endpoint_id)
                                && window.roles.contains(&role)
                                && window.range.start_line <= requested_range.start_line
                                && window.range.end_line >= requested_range.end_line
                        });
                    if !valid {
                        return Err(v3_validation(
                            ContextSubjectWindowsV3ValidationError::SubjectOutcomes,
                        ));
                    }
                }
                ContextSubjectOutcomeV3::Lost { loss } => {
                    let input = SubjectLossV3Input {
                        endpoint_id: loss.endpoint_id.clone(),
                        role: loss.role,
                        reason: loss.reason,
                        source_artifact_id: loss.source_artifact_id.clone(),
                        requested_range: loss.requested_range.clone(),
                    };
                    let covered = loss
                        .source_artifact_id
                        .as_ref()
                        .zip(loss.requested_range.as_ref())
                        .is_some_and(|(source_id, range)| {
                            self.windows.iter().any(|window| {
                                window.source_artifact_id == *source_id
                                    && window.owner_ids.contains(endpoint_id)
                                    && window.roles.contains(&role)
                                    && window.range.start_line <= range.start_line
                                    && window.range.end_line >= range.end_line
                            })
                        });
                    if &loss.endpoint_id != endpoint_id
                        || loss.role != role
                        || loss.property_id != self.property_id
                        || loss.severity != Severity::High
                        || loss
                            .source_artifact_id
                            .as_ref()
                            .is_some_and(|source_id| !materialized_ids.contains(source_id))
                        || loss.requested_range.is_some() && loss.source_artifact_id.is_none()
                        || covered
                        || subject_loss_id_v3(
                            &self.snapshot_id,
                            &self.obligation_id,
                            &self.property_id,
                            &input,
                        )? != loss.id
                    {
                        return Err(v3_validation(
                            ContextSubjectWindowsV3ValidationError::SubjectOutcomes,
                        ));
                    }
                }
            }
        }

        if self.support_loss_summaries.len() > 15
            || self
                .support_loss_summaries
                .windows(2)
                .any(|pair| pair[0].reason >= pair[1].reason)
            || self.support_loss_summaries.iter().any(|summary| {
                summary.cardinality != ContextKnownCardinalityV3::Known
                    || summary.observed_count == 0
                    || !is_sha256(&summary.sorted_anchor_id_set_sha256)
            })
        {
            return Err(v3_validation(
                ContextSubjectWindowsV3ValidationError::SupportPartition,
            ));
        }
        let mut support_count = u64::try_from(admitted_support_ids.len())
            .map_err(|_| v3_validation(ContextSubjectWindowsV3ValidationError::SupportPartition))?;
        for summary in &self.support_loss_summaries {
            support_count = checked_v3_u64_add(support_count, summary.observed_count)?;
        }
        if support_count != self.support_anchor_denominator.observed_count
            || self
                .support_loss_summaries
                .iter()
                .enumerate()
                .any(|(index, left)| {
                    self.support_loss_summaries[index + 1..]
                        .iter()
                        .any(|right| {
                            left.observed_count == right.observed_count
                                && left.sorted_anchor_id_set_sha256
                                    == right.sorted_anchor_id_set_sha256
                        })
                })
        {
            return Err(v3_validation(
                ContextSubjectWindowsV3ValidationError::SupportPartition,
            ));
        }
        if self.support_loss_summaries.is_empty() {
            let rebuilt = denominator_commitment_v3(
                "context v3 read-only admitted support anchors",
                &admitted_support_ids,
            )?;
            if rebuilt != self.support_anchor_denominator {
                return Err(v3_validation(
                    ContextSubjectWindowsV3ValidationError::Denominator {
                        name: "support_anchor_denominator",
                    },
                ));
            }
        } else if admitted_support_ids.is_empty() && self.support_loss_summaries.len() == 1 {
            let summary = &self.support_loss_summaries[0];
            if summary.observed_count != self.support_anchor_denominator.observed_count
                || summary.sorted_anchor_id_set_sha256
                    != self.support_anchor_denominator.sorted_id_set_sha256
            {
                return Err(v3_validation(
                    ContextSubjectWindowsV3ValidationError::SupportPartition,
                ));
            }
        }

        if self.unknowns.len() > 64
            || self.unknowns.iter().any(|unknown| {
                unknown.source_ids.len() != 1
                    || !matches!(
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
        {
            return Err(v3_validation(
                ContextSubjectWindowsV3ValidationError::LatentCardinality,
            ));
        }

        let expected_projection_hash = self.identity_hash()?;
        if self.projection_hash != expected_projection_hash {
            return Err(v3_validation(
                ContextSubjectWindowsV3ValidationError::ProjectionHash,
            ));
        }
        if self.id != StableId::parse(format!("context-envelope-v3:{}", self.projection_hash))? {
            return Err(v3_validation(
                ContextSubjectWindowsV3ValidationError::ContextId,
            ));
        }
        Ok(())
    }
}

/// Validates only the closed wire structure and relationships visible in one
/// sealed v3 context. It does not reconstruct accepted/reached/lost-anchor
/// sets and therefore does not establish semantic denominator completeness.
pub fn validate_subject_windows_v3_wire_read_only(
    context_canonical_value: &serde_json::Value,
) -> ContextResult<WireValidatedContextSubjectWindowsV3> {
    let observed_policy = context_canonical_value
        .get("context_policy")
        .and_then(|policy| policy.get("policy_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    if observed_policy.as_deref() != Some(ContextSubjectWindowsPolicyV3::ID) {
        return Err(v3_validation(
            ContextSubjectWindowsV3ValidationError::WrongPolicy {
                observed: observed_policy,
            },
        ));
    }
    let raw: RawContextSubjectWindowsV3 = serde_json::from_value(context_canonical_value.clone())
        .map_err(|error| {
        v3_validation(ContextSubjectWindowsV3ValidationError::Malformed {
            message: error.to_string(),
        })
    })?;
    let context = raw.into_context();
    context.validate_read_only_semantics()?;
    Ok(WireValidatedContextSubjectWindowsV3 { context })
}

/// Backward-compatible bytes-only wire validation entry point.
pub fn validate_subject_windows_v3_read_only(
    context_canonical_value: &serde_json::Value,
) -> ContextResult<WireValidatedContextSubjectWindowsV3> {
    validate_subject_windows_v3_wire_read_only(context_canonical_value)
}

/// Reconstructs semantic truth from a separately retained trusted live-build
/// basis and byte-compares it with the wire-valid context.
pub fn validate_subject_windows_v3_against_basis(
    context_canonical_value: &serde_json::Value,
    basis: &ContextValidationBasisV3,
) -> ContextResult<SemanticallyValidatedContextSubjectWindowsV3> {
    let wire = validate_subject_windows_v3_wire_read_only(context_canonical_value)?;
    let program = basis.aggregate.program();
    if program.snapshot_id() != &basis.snapshot_id
        || program.extractor_set_hash() != &basis.extractor_set_hash
        || ContextSubjectWindowsPolicyV3::fixed().hash() != basis.policy_hash
    {
        return Err(v3_validation(
            ContextSubjectWindowsV3ValidationError::BasisMismatch,
        ));
    }
    // Independent accepted-file oracle: this is derived directly from the
    // retained ProgramSpace, never from the production builder commitment.
    let accepted_ids = program
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
        .map(|artifact| artifact.id.clone())
        .collect::<BTreeSet<_>>();
    let rebuilt_accepted =
        denominator_commitment_v3("context v3 basis accepted files", &accepted_ids)?;
    let obligation = basis
        .aggregate
        .obligation(&basis.obligation_id)
        .ok_or_else(|| v3_validation(ContextSubjectWindowsV3ValidationError::BasisMismatch))?;
    let oracle = crate::context_validation_oracle::rebuild(program, obligation)?;
    let oracle_accepted = denominator_commitment_v3(
        "context v3 oracle accepted files",
        &oracle.accepted_file_ids,
    )?;
    let oracle_reached =
        denominator_commitment_v3("context v3 oracle reached files", &oracle.reached_file_ids)?;
    let oracle_support = denominator_commitment_v3(
        "context v3 oracle support anchors",
        &oracle.support_anchor_ids,
    )?;
    if wire.context.accepted_file_denominator != rebuilt_accepted
        || wire.context.accepted_file_denominator != oracle_accepted
        || wire.context.reached_file_denominator != oracle_reached
        || wire.context.support_anchor_denominator != oracle_support
    {
        return Err(v3_validation(
            ContextSubjectWindowsV3ValidationError::BasisMismatch,
        ));
    }
    let mut rebuilt_session = prepare_subject_windows_v3(
        &basis.aggregate,
        basis.obligation_id.clone(),
        basis.caller_artifact_id.clone(),
        basis.callee_artifact_id.clone(),
        basis.accepted_file_bound,
    )?;
    while let Some(request) = rebuilt_session.next_source_request()? {
        let bytes = basis
            .source_bytes_by_artifact_id
            .get(request.artifact_id())
            .ok_or_else(|| {
                DomainError::Validation("basis source index became incomplete".to_owned())
            })?;
        rebuilt_session.submit_source(&request, bytes)?;
    }
    let rebuilt = rebuilt_session.finish()?;
    if wire.context.snapshot_id != basis.snapshot_id
        || rebuilt.accepted_file_denominator != oracle_accepted
        || rebuilt.reached_file_denominator != oracle_reached
        || rebuilt.support_anchor_denominator != oracle_support
        || wire.context != rebuilt
    {
        return Err(v3_validation(
            ContextSubjectWindowsV3ValidationError::BasisMismatch,
        ));
    }
    Ok(SemanticallyValidatedContextSubjectWindowsV3 {
        context: wire.context,
    })
}

impl ContextSubjectWindowsSessionV3 {
    pub fn next_source_request(&mut self) -> ContextResult<Option<ContextSourceRequest>> {
        if self.pending.is_some() {
            return Err(ContextError::Protocol(
                "a v3 source request is still pending",
            ));
        }
        while self.index < self.candidates.len() {
            let candidate = &self.candidates[self.index];
            if let Some(reason) = candidate.exclusion {
                self.exclude(candidate.artifact.id.clone(), reason);
                self.index = checked_v2_usize_add(
                    "v3 context source index",
                    self.candidates.len(),
                    self.index,
                    1,
                )?;
                continue;
            }
            if let Some(reason) = metadata_exclusion(
                self.resolved.len(),
                candidate.registration_size,
                self.resolved_bytes,
            )? {
                self.exclude(candidate.artifact.id.clone(), reason);
                self.index = checked_v2_usize_add(
                    "v3 context source index",
                    self.candidates.len(),
                    self.index,
                    1,
                )?;
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
            probe(
                &self.effect_probe,
                ContextBuildEffect::SourceBytesRequested {
                    artifact_id: request.artifact_id.clone(),
                },
            );
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
            return Err(ContextError::Protocol("no v3 source request is pending"));
        };
        if expected != request {
            return Err(ContextError::Protocol(
                "stale, replayed, or out-of-order v3 source request",
            ));
        }
        let byte_length = u64::try_from(bytes.len())
            .map_err(|_| incomplete("v3 resolved artifact byte length", usize::MAX, bytes.len()))?;
        let actual_hash = ContentHash::sha256(bytes);
        let actual_line_count = bytes.iter().try_fold(1_u64, |count, byte| {
            if *byte == b'\n' {
                count.checked_add(1)
            } else {
                Some(count)
            }
        });
        if byte_length != expected.expected_length
            || actual_hash != expected.content_hash
            || actual_hash != expected.cas_hash
            || actual_line_count != Some(expected.line_count)
        {
            return Err(DomainError::Validation(
                "v3 resolved source bytes do not match registered metadata".to_owned(),
            )
            .into());
        }
        let next_resolved_bytes =
            self.resolved_bytes
                .checked_add(byte_length)
                .ok_or_else(|| {
                    incomplete(
                        "v3 context resolved bytes",
                        MAX_RESOLVED as usize,
                        usize::MAX,
                    )
                })?;
        if next_resolved_bytes > MAX_RESOLVED {
            return Err(incomplete(
                "v3 context resolved bytes",
                MAX_RESOLVED as usize,
                usize::try_from(next_resolved_bytes).unwrap_or(usize::MAX),
            )
            .into());
        }
        let mut owned = Vec::new();
        owned.try_reserve_exact(bytes.len()).map_err(|_| {
            incomplete(
                "v3 resolved source retention",
                MAX_RESOLVED as usize,
                bytes.len(),
            )
        })?;
        owned.extend_from_slice(bytes);
        let submitted_artifact_id = expected.artifact_id.clone();
        if self
            .resolved
            .insert(
                submitted_artifact_id.clone(),
                ResolvedSourceV2 { bytes: owned },
            )
            .is_some()
        {
            return Err(DomainError::Validation(
                "v3 context source was resolved more than once".to_owned(),
            )
            .into());
        }
        self.pending = None;
        probe(
            &self.effect_probe,
            ContextBuildEffect::SourceSubmitted {
                artifact_id: submitted_artifact_id,
            },
        );
        self.resolved_bytes = next_resolved_bytes;
        self.index = checked_v2_usize_add(
            "v3 context source index",
            self.candidates.len(),
            self.index,
            1,
        )?;
        Ok(())
    }

    pub fn finish(self) -> ContextResult<BuiltContextSubjectWindowsV3> {
        if self.pending.is_some() || self.index != self.candidates.len() {
            return Err(ContextError::Protocol(
                "all v3 source candidates must be processed before finish",
            ));
        }
        let excluded_by_id = self
            .excluded
            .iter()
            .map(|excluded| (excluded.artifact_id.clone(), excluded.reason))
            .collect::<BTreeMap<_, _>>();
        let mut materialized_sources = self
            .candidates
            .iter()
            .map(|candidate| ContextMaterializedSourceV3 {
                artifact_id: candidate.artifact.id.clone(),
                registration_id: candidate.source.registration_id().clone(),
                content_hash: candidate.source.content_hash().clone(),
                cas_hash: candidate.source.cas_hash().clone(),
                path: candidate.source.path().to_owned(),
                size: candidate.registration_size,
                line_count: candidate.source.line_count(),
                exclusion: excluded_by_id.get(&candidate.artifact.id).copied(),
            })
            .collect::<Vec<_>>();
        materialized_sources.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));

        let mut inputs = Vec::new();
        let mut subject_losses = Vec::new();
        for expectation in &self.expectations {
            if let Some(reason) = expectation.preflight_loss {
                subject_losses.push(subject_loss_v3(
                    &self.snapshot_id,
                    self.obligation.id(),
                    self.obligation.property_id(),
                    SubjectLossV3Input {
                        endpoint_id: expectation.endpoint_id.clone(),
                        role: expectation.role,
                        reason,
                        source_artifact_id: expectation.source_artifact_id.clone(),
                        requested_range: expectation.range.clone(),
                    },
                )?);
                continue;
            }
            let source_id = expectation
                .source_artifact_id
                .as_ref()
                .expect("valid subject source");
            let range = expectation.range.as_ref().expect("valid subject range");
            let candidate = self
                .candidates
                .iter()
                .find(|candidate| candidate.artifact.id == *source_id)
                .expect("subject source materialized");
            if let Some(resolved) = self.resolved.get(source_id) {
                inputs.push(ContextWindowInputV2 {
                    source_artifact_id: source_id.clone(),
                    registration_id: candidate.source.registration_id().clone(),
                    content_hash: candidate.source.content_hash().clone(),
                    cas_hash: candidate.source.cas_hash().clone(),
                    bytes: &resolved.bytes,
                    start_line: range.start_line,
                    end_line: range.end_line,
                    owner_id: expectation.endpoint_id.clone(),
                    role: expectation.role,
                });
            } else {
                subject_losses.push(subject_loss_v3(
                    &self.snapshot_id,
                    self.obligation.id(),
                    self.obligation.property_id(),
                    SubjectLossV3Input {
                        endpoint_id: expectation.endpoint_id.clone(),
                        role: expectation.role,
                        reason: excluded_by_id
                            .get(source_id)
                            .copied()
                            .map(window_reason_from_exclusion)
                            .unwrap_or(ContextWindowLossReasonV2::MissingSource),
                        source_artifact_id: Some(source_id.clone()),
                        requested_range: Some(range.clone()),
                    },
                )?);
            }
        }

        let mut support_loss_sets =
            BTreeMap::<ContextWindowLossReasonV2, BTreeSet<StableId>>::new();
        for candidate in &self.candidates {
            for (start, end, owner) in &candidate.anchors {
                let anchor_id = self
                    .support_anchor_ids
                    .get(&(candidate.artifact.id.clone(), *start, *end, owner.clone()))
                    .expect("prepared support anchor");
                if let Some(resolved) = self.resolved.get(&candidate.artifact.id) {
                    inputs.push(ContextWindowInputV2 {
                        source_artifact_id: candidate.artifact.id.clone(),
                        registration_id: candidate.source.registration_id().clone(),
                        content_hash: candidate.source.content_hash().clone(),
                        cas_hash: candidate.source.cas_hash().clone(),
                        bytes: &resolved.bytes,
                        start_line: u32::try_from(*start).map_err(|_| {
                            incomplete("v3 support start line", u32::MAX as usize, usize::MAX)
                        })?,
                        end_line: u32::try_from(*end).map_err(|_| {
                            incomplete("v3 support end line", u32::MAX as usize, usize::MAX)
                        })?,
                        owner_id: owner.clone(),
                        role: ContextWindowRoleV2::Support,
                    });
                } else {
                    let reason = excluded_by_id
                        .get(&candidate.artifact.id)
                        .copied()
                        .map(window_reason_from_exclusion)
                        .unwrap_or(ContextWindowLossReasonV2::MissingSource);
                    support_loss_sets
                        .entry(reason)
                        .or_default()
                        .insert(anchor_id.clone());
                }
            }
        }
        let loss_limit = inputs.len().max(2);
        let (v2_windows, resolver_losses) = resolve_subject_windows_for_policy(
            &self.snapshot_id,
            self.obligation.id(),
            Some(self.obligation.property_id()),
            ContextSubjectWindowsPolicyV3::ID,
            loss_limit,
            &inputs,
        )?;
        for loss in resolver_losses {
            if loss.role == ContextWindowRoleV2::Support {
                let source = loss
                    .source_artifact_id
                    .as_ref()
                    .expect("support loss source");
                let range = loss.requested_range.as_ref().expect("support loss range");
                let anchor_id = self
                    .support_anchor_ids
                    .get(&(
                        source.clone(),
                        u64::from(range.start_line),
                        u64::from(range.end_line),
                        loss.endpoint_id.clone(),
                    ))
                    .ok_or_else(|| {
                        DomainError::Validation(
                            "v3 resolver support loss is outside the anchor denominator".to_owned(),
                        )
                    })?;
                support_loss_sets
                    .entry(loss.reason)
                    .or_default()
                    .insert(anchor_id.clone());
            } else {
                subject_losses.push(subject_loss_v3(
                    &self.snapshot_id,
                    self.obligation.id(),
                    self.obligation.property_id(),
                    SubjectLossV3Input {
                        endpoint_id: loss.endpoint_id,
                        role: loss.role,
                        reason: loss.reason,
                        source_artifact_id: loss.source_artifact_id,
                        requested_range: loss.requested_range,
                    },
                )?);
            }
        }
        if subject_losses.len() > 2 {
            return Err(incomplete("context v3 subject losses", 2, subject_losses.len()).into());
        }

        let mut windows = Vec::new();
        let mut admitted_support_ids = BTreeSet::new();
        for window in v2_windows {
            let support_anchor_ids = self
                .support_anchor_ids
                .iter()
                .filter(|((source, start, end, owner), _)| {
                    source == &window.source_artifact_id
                        && *start >= u64::from(window.range.start_line)
                        && *end <= u64::from(window.range.end_line)
                        && window.owner_ids.contains(owner)
                        && window.roles.contains(&ContextWindowRoleV2::Support)
                })
                .map(|(_, id)| id.clone())
                .collect::<BTreeSet<_>>();
            admitted_support_ids.extend(support_anchor_ids.iter().cloned());
            let id = context_window_id_v3(
                &self.snapshot_id,
                self.obligation.id(),
                &ContextWindowIdentityV3 {
                    source_artifact_id: &window.source_artifact_id,
                    registration_id: &window.registration_id,
                    content_hash: &window.content_hash,
                    cas_hash: &window.cas_hash,
                    range: &window.range,
                    owner_ids: &window.owner_ids,
                    roles: &window.roles,
                    support_anchor_ids: &support_anchor_ids,
                    excerpt_byte_length: window.excerpt_byte_length,
                    excerpt_hash: &window.excerpt_hash,
                },
            )?;
            windows.push(ContextWindowV3 {
                id,
                source_artifact_id: window.source_artifact_id,
                registration_id: window.registration_id,
                content_hash: window.content_hash,
                cas_hash: window.cas_hash,
                range: window.range,
                owner_ids: window.owner_ids,
                roles: window.roles,
                support_anchor_ids,
                excerpt_byte_length: window.excerpt_byte_length,
                excerpt_hash: window.excerpt_hash,
            });
        }
        windows.sort_by(|left, right| {
            (&left.source_artifact_id, &left.range, &left.id).cmp(&(
                &right.source_artifact_id,
                &right.range,
                &right.id,
            ))
        });
        let lost_support_ids = support_loss_sets
            .values()
            .flat_map(|ids| ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        if !admitted_support_ids.is_disjoint(&lost_support_ids) {
            return Err(DomainError::Validation(
                "v3 support anchor is both admitted and lost".to_owned(),
            )
            .into());
        }
        let partition = admitted_support_ids
            .union(&lost_support_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        let all_support_ids = self
            .support_anchor_ids
            .values()
            .cloned()
            .collect::<BTreeSet<_>>();
        if partition != all_support_ids {
            return Err(DomainError::Validation(
                "v3 support anchor partition is incomplete".to_owned(),
            )
            .into());
        }
        let support_loss_summaries = support_loss_summaries_v3(&support_loss_sets)?;

        subject_losses.sort_by(|left, right| {
            (window_role_priority(left.role), &left.endpoint_id)
                .cmp(&(window_role_priority(right.role), &right.endpoint_id))
        });
        let mut subject_outcomes = Vec::with_capacity(2);
        for expectation in &self.expectations {
            if let Some(loss) = subject_losses.iter().find(|loss| {
                loss.endpoint_id == expectation.endpoint_id && loss.role == expectation.role
            }) {
                probe(
                    &self.effect_probe,
                    ContextBuildEffect::SubjectOutcome {
                        endpoint_id: expectation.endpoint_id.clone(),
                        submitted: false,
                    },
                );
                subject_outcomes.push(ContextSubjectOutcomeV3::Lost { loss: loss.clone() });
                continue;
            }
            let source = expectation
                .source_artifact_id
                .as_ref()
                .expect("admitted subject source");
            let range = expectation.range.as_ref().expect("admitted subject range");
            let window = windows
                .iter()
                .find(|window| {
                    window.source_artifact_id == *source
                        && window.owner_ids.contains(&expectation.endpoint_id)
                        && window.roles.contains(&expectation.role)
                        && window.range.start_line <= range.start_line
                        && window.range.end_line >= range.end_line
                })
                .ok_or_else(|| {
                    DomainError::Validation(
                        "v3 subject is neither admitted nor named by a typed loss".to_owned(),
                    )
                })?;
            subject_outcomes.push(ContextSubjectOutcomeV3::Admitted {
                endpoint_id: expectation.endpoint_id.clone(),
                role: expectation.role,
                source_artifact_id: source.clone(),
                requested_range: range.clone(),
                window_id: window.id.clone(),
            });
            probe(
                &self.effect_probe,
                ContextBuildEffect::SubjectOutcome {
                    endpoint_id: expectation.endpoint_id.clone(),
                    submitted: true,
                },
            );
        }
        if subject_outcomes.len() != 2 {
            return Err(DomainError::Validation(
                "v3 must retain exactly two ordered subject outcomes".to_owned(),
            )
            .into());
        }
        let identity = SubjectWindowsIdentityV3 {
            accepted_file_denominator: &self.accepted_file_denominator,
            callee_artifact_id: &self.callee_artifact_id,
            caller_artifact_id: &self.caller_artifact_id,
            context_policy: ContextSubjectWindowsPolicyV3::fixed(),
            context_policy_hash: &self.policy_hash,
            latent_cardinality: &self.latent_cardinality,
            materialized_source_denominator: &self.materialized_source_denominator,
            materialized_sources: &materialized_sources,
            obligation_id: self.obligation.id(),
            property_id: self.obligation.property_id(),
            reached_file_denominator: &self.reached_file_denominator,
            snapshot_id: &self.snapshot_id,
            subject_outcomes: &subject_outcomes,
            support_anchor_denominator: &self.support_anchor_denominator,
            support_loss_summaries: &support_loss_summaries,
            target_refs: self.obligation.target_refs(),
            unknowns: &self.unknowns,
            windows: &windows,
        };
        let body =
            serde_json::to_vec(&identity).map_err(|error| DomainError::Json(error.to_string()))?;
        if body.len() > MAX_BODY {
            return Err(
                incomplete("context v3 envelope canonical bytes", MAX_BODY, body.len()).into(),
            );
        }
        let projection_hash = ContentHash::sha256(&body);
        let id = StableId::parse(format!("context-envelope-v3:{projection_hash}"))?;
        let built = BuiltContextSubjectWindowsV3 {
            id,
            projection_hash,
            policy: ContextSubjectWindowsPolicyV3::fixed(),
            policy_hash: self.policy_hash,
            snapshot_id: self.snapshot_id,
            obligation_id: self.obligation.id().clone(),
            property_id: self.obligation.property_id().to_owned(),
            target_refs: self.obligation.target_refs().to_vec(),
            caller_artifact_id: self.caller_artifact_id,
            callee_artifact_id: self.callee_artifact_id,
            accepted_file_denominator: self.accepted_file_denominator,
            reached_file_denominator: self.reached_file_denominator,
            materialized_source_denominator: self.materialized_source_denominator,
            support_anchor_denominator: self.support_anchor_denominator,
            latent_cardinality: self.latent_cardinality,
            subject_outcomes,
            materialized_sources,
            windows,
            support_loss_summaries,
            unknowns: self.unknowns,
        };
        built.validate_read_only_semantics()?;
        Ok(built)
    }

    fn exclude(&mut self, id: StableId, reason: ExclusionReason) {
        if !self.excluded.iter().any(|source| source.artifact_id == id) {
            self.excluded.push(ExcludedSourceRef {
                artifact_id: id,
                reason,
            });
        }
    }
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

    fn d_context_ready_program_value() -> Value {
        let mut value = context_ready_program_value();
        value["profile"]["id"] = json!("rust.production.v1");
        value["profile"]["version"] = json!("1");
        value["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["id"] == "file:payment-repository")
            .unwrap()["attributes"]["changed"] = json!(true);
        value["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["id"] == "function:payment-charge")
            .unwrap()["attributes"]["public"] = json!(true);
        value["relations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|relation| relation["id"] == "relation:submit-calls-payment")
            .unwrap()["attributes"]["resolution"] = json!("syntactic_unique");
        value["extraction"]["capabilities"]["containment"] = json!({
            "state": "complete",
            "source_ids": ["relation:file-contains-payment-charge"]
        });
        value["extraction"]["capabilities"]["changed_structure"] = json!({
            "state": "complete",
            "source_ids": ["file:payment-repository"]
        });
        value["extraction"]["capabilities"]["direct_calls"]["state"] = json!("partial");
        value["extraction"]["limitations"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id": "limitation:direct-calls-context",
                "kind": "projection_loss",
                "description": "Only syntactically unique local calls are enumerated.",
                "severity": "medium",
                "source_ids": ["relation:submit-calls-payment"],
                "related_capabilities": ["direct_calls"]
            }));
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

    fn d_binding_fixture() -> (ProgramSpace, Obligation) {
        let program: ProgramSpace =
            serde_json::from_value(d_context_ready_program_value()).unwrap();
        let bundle = MvpRulePack::synthesize_changed_public_callee(&program).unwrap();
        let obligation = bundle
            .obligations()
            .iter()
            .find(|obligation| obligation.version().rule() == SUBJECT_WINDOWS_D_RULE)
            .unwrap()
            .clone();
        (program, obligation)
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
        let bundle = MvpRulePack::synthesize(&program).unwrap();
        let mut aggregate = if bundle
            .universe()
            .candidate_space_gap_obligation_ids()
            .is_some()
        {
            ReviewAggregate::read_only_from_d_two_layer_bundle(program.clone(), &bundle).unwrap()
        } else {
            let (universe, obligations) = bundle.into_parts();
            ReviewAggregate::new(program.clone(), universe, obligations).unwrap()
        };
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

    fn d_fixture_with_accepted_file_count(
        count: usize,
    ) -> (ReviewAggregate, BTreeMap<StableId, Vec<u8>>) {
        d_fixture_with_scale(count, 0)
    }

    fn d_fixture_with_scale(
        count: usize,
        unrelated_relations: usize,
    ) -> (ReviewAggregate, BTreeMap<StableId, Vec<u8>>) {
        assert!(count >= 2);
        let mut value = d_context_ready_program_value();
        for index in 2..count {
            append_artifact(
                &mut value,
                &format!("file:scale-{index:04}"),
                "file",
                Some(&format!("synthetic/scale-{index:04}.rs")),
            );
        }
        for index in 0..unrelated_relations {
            let source = format!("function:unrelated-source-{index:04}");
            let target = format!("function:unrelated-target-{index:04}");
            append_artifact(&mut value, &source, "function", None);
            append_artifact(&mut value, &target, "function", None);
            for id in [&source, &target] {
                let artifact = value["artifacts"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|artifact| artifact["id"] == id.as_str())
                    .unwrap();
                artifact["attributes"]["changed"] = json!(false);
                artifact["attributes"]["public"] = json!(false);
            }
            append_relation(
                &mut value,
                &format!("relation:unrelated-{index:04}"),
                "calls",
                &source,
                &[target],
            );
        }
        let mut bytes_by_path = BTreeMap::new();
        let default_bytes = default_source_bytes();
        for (index, artifact) in value["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .filter(|artifact| artifact["kind"] == "file")
            .enumerate()
        {
            let path = artifact["location"]["path"].as_str().unwrap().to_owned();
            let bytes = default_bytes
                .get(path.as_str())
                .cloned()
                .unwrap_or_else(|| format!("scale-{index:04}\n").into_bytes());
            artifact["content_hash"] = json!(ContentHash::sha256(&bytes).to_string());
            bytes_by_path.insert(path, bytes);
        }
        let program: ProgramSpace = serde_json::from_value(value).unwrap();
        let bundle = MvpRulePack::synthesize(&program).unwrap();
        let mut aggregate =
            ReviewAggregate::read_only_from_d_two_layer_bundle(program.clone(), &bundle).unwrap();
        let run_id = StableId::parse("run:v3-scale-test").unwrap();
        let mut registrations = Vec::with_capacity(count);
        let mut entries = Vec::with_capacity(count);
        let mut by_id = BTreeMap::new();
        for artifact in program
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
        {
            let path = artifact.location.as_ref().unwrap().path.clone();
            let bytes = bytes_by_path[path.as_str()].clone();
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
                    u64::try_from(bytes.len()).unwrap(),
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
                    u64::try_from(bytes.iter().filter(|byte| **byte == b'\n').count()).unwrap() + 1,
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

    fn d_fixture_with_support_anchors(
        count: usize,
    ) -> (ReviewAggregate, BTreeMap<StableId, Vec<u8>>) {
        let mut value = d_context_ready_program_value();
        let mut support_ids = Vec::new();
        for index in 0..count {
            let id = format!("function:support-{index:03}");
            append_artifact(&mut value, &id, "function", None);
            let artifact = value["artifacts"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|artifact| artifact["id"] == id)
                .unwrap();
            artifact["location"]["path"] = json!("src/payment_repository.rs");
            artifact["location"]["start_line"] = json!(index * 2 + 1);
            artifact["location"]["end_line"] = json!(index * 2 + 1);
            support_ids.push(id);
        }
        let contains = value["relations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|relation| relation["id"] == "relation:file-contains-payment-charge")
            .unwrap();
        contains["target_ids"]
            .as_array_mut()
            .unwrap()
            .extend(support_ids.into_iter().map(Value::String));
        let mut source_bytes = default_source_bytes();
        source_bytes.insert("src/payment_repository.rs", b"x\n".repeat(count * 2 + 100));
        fixture_from_program_value(value, source_bytes)
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
        let sealed = session.sealed_resource_oracle();
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
        assert!(session.realized_reservation_bytes().unwrap() <= sealed.session_retained_bytes());
        let built = session.finish().unwrap();
        assert!(built.finish_actual_bytes() <= sealed.session_working_bytes());
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
        assert!(
            session.realized_reservation_bytes().unwrap()
                <= session.sealed_resource_oracle().session_retained_bytes()
        );
    }

    /// A range-bearing artifact outside every file's reverse-`contains`
    /// closure is not an anchor of any file, which is what ADR 0016's anchor
    /// definition says and not a defect: `contains` is one relation kind
    /// among many, and ProgramSpace never promised that everything with a
    /// location is a member of a file. Discovery already draws that
    /// conclusion silently -- such a node contributes no candidate file --
    /// so anchoring must draw it too. The exact-path claim is different: an
    /// artifact that *is* contained by a file must be located in that file,
    /// and a violation is still a typed failure.
    #[test]
    fn reached_range_locations_without_containment_are_not_anchors_but_owner_path_must_match() {
        let mut missing = context_ready_program_value();
        missing["relations"]
            .as_array_mut()
            .unwrap()
            .retain(|relation| relation["kind"] != "contains");
        let (missing_aggregate, _) = fixture_from_program_value(missing, default_source_bytes());
        let obligation = missing_aggregate.obligations().next().unwrap().id().clone();
        let session = prepare_context(&missing_aggregate, obligation)
            .expect("an unparented range-bearing artifact is not an anchor, not a failure");
        assert!(
            session
                .candidates
                .iter()
                .all(|candidate| candidate.anchors.is_empty()),
            "with no accepted containment there is no anchor to attribute to any file"
        );

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

    #[test]
    fn resource_oracle_uses_accepted_metadata_not_projected_sources() {
        let (aggregate, bytes) = fixture();
        let obligation = aggregate.obligations().next().unwrap().id().clone();
        let oracle = context_resource_oracle(&aggregate, &obligation).unwrap();
        assert_eq!(oracle.candidate_count(), 2);

        let expected = bytes
            .values()
            .map(|value| u64::try_from(value.len()).unwrap())
            .max()
            .unwrap();
        assert_eq!(oracle.max_source_cas_bytes(), expected);

        // Real source requests are byte-dependent after their first request;
        // the oracle must still reserve for their observed maximum without
        // consulting a persisted envelope.
        let mut session = prepare_context(&aggregate, obligation).unwrap();
        let sealed = session.sealed_resource_oracle();
        assert_eq!(session.reservation(), sealed);
        assert_eq!(sealed.candidate_count(), oracle.candidate_count());
        assert_eq!(sealed.max_source_cas_bytes(), oracle.max_source_cas_bytes());
        assert!(sealed.session_retained_bytes() <= oracle.session_retained_bytes());
        let admits = |limit| sealed.session_working_bytes() <= limit;
        assert!(admits(sealed.session_working_bytes()));
        assert!(!admits(sealed.session_working_bytes() - 1));
        assert!(session.realized_reservation_bytes().unwrap() <= sealed.session_retained_bytes());
        let mut realized = 0_u64;
        while let Some(request) = session.next_source_request().unwrap() {
            realized = realized.max(request.expected_length());
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
        }
        assert!(realized <= oracle.max_source_cas_bytes());
        assert!(session.realized_reservation_bytes().unwrap() <= oracle.session_retained_bytes());
    }

    #[test]
    fn cloned_envelope_accounting_models_fresh_clone_capacity() {
        let (aggregate, bytes) = fixture();
        let envelope = build_projection(&aggregate, &bytes).envelope().clone();
        assert_eq!(
            envelope.clone().allocated_bytes(),
            envelope.cloned_allocated_bytes()
        );
    }

    #[test]
    fn resource_oracle_is_sensitive_to_registered_source_sizes() {
        let mut small = default_source_bytes();
        small.insert("src/payment_repository.rs", b"x\n".to_vec());
        let (small_aggregate, _) = fixture_with_source_bytes(small);
        let small_obligation = small_aggregate.obligations().next().unwrap().id().clone();
        let small_oracle = context_resource_oracle(&small_aggregate, &small_obligation).unwrap();

        let mut large = default_source_bytes();
        large.insert("src/payment_repository.rs", b"x\n".repeat(500));
        let (large_aggregate, _) = fixture_with_source_bytes(large);
        let large_obligation = large_aggregate.obligations().next().unwrap().id().clone();
        let large_oracle = context_resource_oracle(&large_aggregate, &large_obligation).unwrap();

        assert!(large_oracle.max_source_cas_bytes() > small_oracle.max_source_cas_bytes());
        assert!(large_oracle.session_working_bytes() > small_oracle.session_working_bytes());
    }

    #[test]
    fn byte_dependent_giant_line_branch_keeps_the_same_pre_cas_reservation() {
        let mut giant_checkout = Vec::new();
        for _ in 0..13 {
            giant_checkout.extend_from_slice(b"x\n");
        }
        giant_checkout.extend(std::iter::repeat_n(b'x', 299_799));
        giant_checkout.push(b'\n');
        for _ in 0..87 {
            giant_checkout.extend_from_slice(b"x\n");
        }
        assert_eq!(giant_checkout.len(), 300_000);
        assert_eq!(
            giant_checkout.iter().filter(|byte| **byte == b'\n').count(),
            101
        );

        let mut normal_checkout = Vec::new();
        for _ in 0..101 {
            normal_checkout.extend(std::iter::repeat_n(b'x', 2_969));
            normal_checkout.push(b'\n');
        }
        normal_checkout.extend(std::iter::repeat_n(b'x', 30));
        assert_eq!(normal_checkout.len(), giant_checkout.len());
        assert_eq!(
            normal_checkout
                .iter()
                .filter(|byte| **byte == b'\n')
                .count(),
            giant_checkout.iter().filter(|byte| **byte == b'\n').count()
        );

        let mut giant_sources = default_source_bytes();
        giant_sources.insert("src/checkout_controller.rs", giant_checkout);
        let (giant_aggregate, giant_bytes) = fixture_with_source_bytes(giant_sources);
        let giant_obligation = giant_aggregate.obligations().next().unwrap().id().clone();
        let giant_oracle = context_resource_oracle(&giant_aggregate, &giant_obligation).unwrap();
        let mut giant_session = prepare_context(&giant_aggregate, giant_obligation).unwrap();
        let giant_actual = giant_session.realized_reservation_bytes().unwrap();
        while let Some(request) = giant_session.next_source_request().unwrap() {
            giant_session
                .submit_source(&request, &giant_bytes[request.artifact_id()])
                .unwrap();
        }
        let giant_projection = giant_session.finish().unwrap();

        let mut normal_sources = default_source_bytes();
        normal_sources.insert("src/checkout_controller.rs", normal_checkout);
        let (normal_aggregate, normal_bytes) = fixture_with_source_bytes(normal_sources);
        let normal_obligation = normal_aggregate.obligations().next().unwrap().id().clone();
        let normal_oracle = context_resource_oracle(&normal_aggregate, &normal_obligation).unwrap();
        let mut normal_session = prepare_context(&normal_aggregate, normal_obligation).unwrap();
        let normal_actual = normal_session.realized_reservation_bytes().unwrap();
        while let Some(request) = normal_session.next_source_request().unwrap() {
            normal_session
                .submit_source(&request, &normal_bytes[request.artifact_id()])
                .unwrap();
        }
        let normal_projection = normal_session.finish().unwrap();

        assert_eq!(giant_oracle, normal_oracle);
        assert_eq!(giant_actual, normal_actual);
        assert!(giant_actual <= giant_oracle.session_retained_bytes());
        assert!(normal_actual <= normal_oracle.session_retained_bytes());
        assert!(giant_projection.finish_actual_bytes() <= giant_oracle.session_working_bytes());
        assert!(normal_projection.finish_actual_bytes() <= normal_oracle.session_working_bytes());
        let checkout = StableId::parse("file:checkout-controller").unwrap();
        assert!(
            giant_projection
                .envelope()
                .excluded_sources()
                .iter()
                .any(|source| {
                    source.artifact_id() == &checkout
                        && source.reason() == ExclusionReason::GiantLine
                })
        );
        assert!(
            normal_projection
                .envelope()
                .included_sources()
                .iter()
                .any(|source| { source.artifact_id() == &checkout })
        );
    }

    #[test]
    fn resource_formula_has_an_inclusive_exact_boundary() {
        let retained = context_resource_formula(
            2,
            17,
            ContextReservationLayout::default(),
            ContextTypedCapacities::requested(2).unwrap(),
        )
        .unwrap();
        let required = checked_resource_add(
            checked_resource_add(retained, 17, "test working bytes").unwrap(),
            u64::try_from(MAX_BODY).unwrap(),
            "test working bytes",
        )
        .unwrap();
        assert_eq!(required, retained + 17 + u64::try_from(MAX_BODY).unwrap());
        let admits = |limit| required <= limit;
        assert!(admits(required));
        assert!(!admits(required - 1));
    }

    #[test]
    fn resource_formula_refuses_each_checked_arithmetic_overflow_phase() {
        for operation in [
            "context resource retained bytes",
            "context resource working bytes",
            "context resource predecessor keys",
        ] {
            assert!(matches!(
                checked_resource_add(u64::MAX, 1, operation),
                Err(ContextError::Domain(DomainError::Incomplete { operation: actual, .. }))
                    if actual == operation
            ));
            assert!(matches!(
                checked_resource_mul(u64::MAX, 2, operation),
                Err(ContextError::Domain(DomainError::Incomplete { operation: actual, .. }))
                    if actual == operation
            ));
        }
        assert!(matches!(
            context_resource_formula(
                u64::try_from(MAX).unwrap() + 1,
                0,
                ContextReservationLayout::default(),
                ContextTypedCapacities::default(),
            ),
            Err(ContextError::Domain(DomainError::Incomplete {
                operation: "context resource candidate count",
                ..
            }))
        ));
        assert!(matches!(
            context_resource_formula(
                0,
                MAX_FILE_BYTES + 1,
                ContextReservationLayout::default(),
                ContextTypedCapacities::default(),
            ),
            Err(ContextError::Domain(DomainError::Incomplete {
                operation: "context resource CAS source bytes",
                ..
            }))
        ));
    }

    #[test]
    fn resource_formula_charges_allocator_excess_typed_capacity_exactly() {
        let requested = ContextTypedCapacities::requested(2).unwrap();
        let mut excess = requested;
        excess.candidates += 3;
        excess.anchors += 5;
        excess.unknowns += 7;
        excess.included += 11;
        excess.excluded += 13;
        excess.losses += 17;
        excess.discovery_vectors += 19;
        let base =
            context_resource_formula(2, 0, ContextReservationLayout::default(), requested).unwrap();
        let observed =
            context_resource_formula(2, 0, ContextReservationLayout::default(), excess).unwrap();
        let expected_delta = 3 * size_of::<Candidate>()
            + 5 * size_of::<(u64, u64, StableId)>()
            + 7 * size_of::<EnvelopeUnknown>()
            + 11 * size_of::<SourceArtifactRef>()
            + 13 * size_of::<ExcludedSourceRef>()
            + 17 * size_of::<EnvelopeLoss>()
            + 19;
        assert_eq!(observed - base, u64::try_from(expected_delta).unwrap());
    }

    #[test]
    fn obligation_reservation_uses_deep_owned_collections_not_wire_size() {
        let base = obligation_with_seed("file:seed-00");
        let mut value = serde_json::to_value(&base).unwrap();
        let ids = (0..64)
            .map(|index| format!("file:s{index:02}"))
            .collect::<Vec<_>>();
        for field in [
            "target_refs",
            "normalized_target_refs",
            "source_ids",
            "normalized_source_ids",
        ] {
            value[field] = json!(ids);
        }
        let expanded: Obligation = serde_json::from_value(value).unwrap();
        let predicted = obligation_clone_backing_reservation(&expanded).unwrap();
        let actual = u64::try_from(expanded.allocated_bytes()).unwrap();
        assert_eq!(predicted, actual);
        assert!(predicted > obligation_clone_backing_reservation(&base).unwrap());
        assert!(actual > serialized_size(&expanded).unwrap());
    }

    #[test]
    fn v2_checked_usize_add_reports_overflow_as_typed_incomplete() {
        assert!(matches!(
            checked_v2_usize_add("context window losses", 64, usize::MAX, 1),
            Err(ContextError::Domain(DomainError::Incomplete {
                operation: "context window losses",
                limit: 64,
                observed: usize::MAX,
            }))
        ));
    }

    #[test]
    fn v3_4816_file_denominator_materializes_only_subject_and_reached_files() {
        let accepted = (0..4_816)
            .map(|index| StableId::parse(format!("file:f{index:04}")).unwrap())
            .collect::<BTreeSet<_>>();
        let reached = BTreeSet::from([StableId::parse("file:f0000").unwrap()]);
        let subjects = BTreeSet::from([
            StableId::parse("file:f0001").unwrap(),
            StableId::parse("file:f0002").unwrap(),
        ]);
        let (accepted_commitment, reached_commitment, materialized, materialized_commitment) =
            materialized_denominators_v3(&accepted, &reached, &subjects).unwrap();
        assert_eq!(accepted_commitment.observed_count(), 4_816);
        assert_eq!(reached_commitment.observed_count(), 1);
        assert_eq!(materialized.len(), 3);
        assert_eq!(materialized_commitment.observed_count(), 3);
        assert_eq!(
            accepted_commitment.sorted_id_set_sha256(),
            &sorted_id_set_sha256(&accepted).unwrap()
        );
    }

    #[test]
    fn v3_session_completes_the_4816_file_case_without_raising_v2_caps() {
        let (aggregate, bytes) = d_fixture_with_accepted_file_count(4_816);
        let obligation_id = aggregate
            .obligations()
            .find(|obligation| obligation.version().rule() == SUBJECT_WINDOWS_D_RULE)
            .unwrap()
            .id()
            .clone();
        let caller_id = StableId::parse("function:checkout-submit").unwrap();
        let callee_id = StableId::parse("function:payment-charge").unwrap();
        assert!(matches!(
            prepare_subject_windows_v3(
                &aggregate,
                obligation_id.clone(),
                caller_id.clone(),
                callee_id.clone(),
                4_815,
            ),
            Err(ContextError::Domain(DomainError::Incomplete {
                operation: "context v3 accepted file denominator",
                limit: 4_815,
                observed: 4_816,
            }))
        ));
        assert!(matches!(
            prepare_subject_windows_v2(
                &aggregate,
                obligation_id.clone(),
                caller_id.clone(),
                callee_id.clone(),
            ),
            Err(ContextError::Domain(DomainError::Incomplete {
                operation: "context candidate files",
                limit: MAX,
                observed: 4_097,
            }))
        ));
        let mut session =
            prepare_subject_windows_v3(&aggregate, obligation_id, caller_id, callee_id, 4_816)
                .unwrap();
        let mut requests = 0;
        while let Some(request) = session.next_source_request().unwrap() {
            requests += 1;
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
        }
        let built = session.finish().unwrap();
        assert_eq!(built.accepted_file_denominator().observed_count(), 4_816);
        assert!(built.materialized_source_denominator().observed_count() <= 3);
        assert!(requests <= 3);
        assert_eq!(built.subject_outcomes().len(), 2);
    }

    #[test]
    fn v3_support_loss_summary_retains_all_4816_known_anchor_ids() {
        let anchors = (0..4_816)
            .map(|index| StableId::parse(format!("context-support-anchor:a{index:04}")).unwrap())
            .collect::<BTreeSet<_>>();
        let summaries = support_loss_summaries_v3(&BTreeMap::from([(
            ContextWindowLossReasonV2::NotReached,
            anchors.clone(),
        )]))
        .unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].observed_count(), 4_816);
        assert_eq!(
            summaries[0].sorted_anchor_id_set_sha256(),
            &sorted_id_set_sha256(&anchors).unwrap()
        );
    }

    #[test]
    fn v3_session_aggregates_more_than_64_support_losses_without_dropping_anchors() {
        let (aggregate, bytes) = d_fixture_with_support_anchors(80);
        let obligation_id = aggregate
            .obligations()
            .find(|obligation| obligation.version().rule() == SUBJECT_WINDOWS_D_RULE)
            .unwrap()
            .id()
            .clone();
        let mut session = prepare_subject_windows_v3(
            &aggregate,
            obligation_id,
            StableId::parse("function:checkout-submit").unwrap(),
            StableId::parse("function:payment-charge").unwrap(),
            2,
        )
        .unwrap();
        while let Some(request) = session.next_source_request().unwrap() {
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
        }
        let built = session.finish().unwrap();
        let admitted = built
            .windows()
            .iter()
            .map(|window| window.support_anchor_ids().len())
            .sum::<usize>();
        let lost = built
            .support_loss_summaries()
            .iter()
            .map(|summary| usize::try_from(summary.observed_count()).unwrap())
            .sum::<usize>();
        assert!(lost > 64);
        assert_eq!(
            admitted + lost,
            usize::try_from(built.support_anchor_denominator().observed_count()).unwrap()
        );
        assert!(built.support_loss_summaries().len() <= 15);
    }

    #[test]
    fn v3_denominator_digest_has_an_independent_sorted_array_oracle() {
        let ids = BTreeSet::from([
            StableId::parse("file:b").unwrap(),
            StableId::parse("file:a").unwrap(),
        ]);
        let commitment = denominator_commitment_v3("test denominator", &ids).unwrap();
        assert_eq!(commitment.observed_count(), 2);
        assert_eq!(
            commitment.sorted_id_set_sha256().as_str(),
            "sha256:1822b2052e89d2fc05805c0862856d6a8a8a4da4f104dd6aca29f8e39d60053f"
        );
    }

    #[test]
    fn v3_observed_unknown_remains_an_individual_typed_record() {
        let (program, obligation) = d_binding_fixture();
        let unknown_id = StableId::parse("artifact:not-accepted-context-seed").unwrap();
        let mutated = mutated_obligation(&obligation, |value| {
            value["context_ids"] = json!([unknown_id.as_str()]);
            value["normalized_context_ids"] = json!([unknown_id.as_str()]);
        });
        let grouped = discover(&program, &mutated).unwrap().8;
        let unknowns = individual_unknowns_v3(grouped).unwrap();
        assert!(unknowns.iter().any(|unknown| {
            unknown.description == "context_unknown:unresolved_seed_reference"
                && unknown.source_ids == BTreeSet::from([unknown_id.clone()])
        }));
    }

    #[test]
    fn v3_unknown_cap_accepts_exact_and_rejects_plus_one_with_exact_digest() {
        let source_ids = (0..65)
            .map(|index| StableId::parse(format!("artifact:unknown-{index:02}")).unwrap())
            .collect::<BTreeSet<_>>();
        let grouped = |ids: BTreeSet<StableId>| {
            vec![EnvelopeUnknown {
                description: "context_unknown:unresolved_seed_reference".to_owned(),
                source_ids: ids,
            }]
        };
        let exact_ids = source_ids.iter().take(64).cloned().collect::<BTreeSet<_>>();
        assert_eq!(
            individual_unknowns_v3(grouped(exact_ids)).unwrap().len(),
            64
        );

        let expected = source_ids
            .iter()
            .cloned()
            .map(|source_id| EnvelopeUnknown {
                description: "context_unknown:unresolved_seed_reference".to_owned(),
                source_ids: BTreeSet::from([source_id]),
            })
            .collect::<Vec<_>>();
        let expected_digest = ContentHash::sha256(&serde_json::to_vec(&expected).unwrap());
        assert!(matches!(
            individual_unknowns_v3(grouped(source_ids)),
            Err(ContextError::V3UnknownOverflow {
                limit: 64,
                observed: 65,
                sorted_unknown_set_sha256,
            }) if sorted_unknown_set_sha256 == expected_digest
        ));
    }

    #[test]
    fn v3_materialized_source_cap_accepts_exact_and_rejects_plus_one() {
        let exact = (0..MAX)
            .map(|index| StableId::parse(format!("file:m{index:04}")).unwrap())
            .collect::<BTreeSet<_>>();
        assert!(materialized_denominators_v3(&exact, &exact, &BTreeSet::new()).is_ok());
        let plus_one = (0..=MAX)
            .map(|index| StableId::parse(format!("file:m{index:04}")).unwrap())
            .collect::<BTreeSet<_>>();
        assert!(matches!(
            materialized_denominators_v3(&plus_one, &plus_one, &BTreeSet::new()),
            Err(ContextError::Domain(DomainError::Incomplete {
                operation: "context v3 materialized source candidates",
                limit: MAX,
                observed,
            })) if observed == MAX + 1
        ));
    }

    #[test]
    fn v3_latent_cardinality_distinguishes_known_zero_from_unknown() {
        let partial: ProgramSpace =
            serde_json::from_value(d_context_ready_program_value()).unwrap();
        assert!(matches!(
            latent_cardinality_v3(&partial).unwrap(),
            ContextLatentCardinalityV3::Unknown {
                capability_states,
                qualification_ids,
            } if capability_states["direct_calls"] == crate::CapabilityState::Partial
                && !qualification_ids.is_empty()
        ));

        let mut complete_value = d_context_ready_program_value();
        complete_value["extraction"]["capabilities"]["direct_calls"]["state"] = json!("complete");
        complete_value["extraction"]["limitations"]
            .as_array_mut()
            .unwrap()
            .retain(|limitation| limitation["id"] != "limitation:direct-calls-context");
        let complete: ProgramSpace = serde_json::from_value(complete_value).unwrap();
        assert_eq!(
            latent_cardinality_v3(&complete).unwrap(),
            ContextLatentCardinalityV3::KnownZero
        );
    }

    #[test]
    fn v3_session_retains_four_denominators_and_two_subject_outcomes() {
        let (aggregate, bytes) =
            fixture_from_program_value(d_context_ready_program_value(), default_source_bytes());
        let obligation_id = aggregate
            .obligations()
            .find(|obligation| obligation.version().rule() == SUBJECT_WINDOWS_D_RULE)
            .unwrap()
            .id()
            .clone();
        let mut session = prepare_subject_windows_v3(
            &aggregate,
            obligation_id,
            StableId::parse("function:checkout-submit").unwrap(),
            StableId::parse("function:payment-charge").unwrap(),
            2,
        )
        .unwrap();
        while let Some(request) = session.next_source_request().unwrap() {
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
        }
        let built = session.finish().unwrap();
        assert_eq!(
            built.policy_hash().as_str(),
            ContextSubjectWindowsPolicyV3::GOLDEN_HASH
        );
        assert_eq!(built.accepted_file_denominator().observed_count(), 2);
        assert!(built.reached_file_denominator().observed_count() >= 1);
        assert!(built.materialized_source_denominator().observed_count() <= 2);
        assert!(built.support_anchor_denominator().observed_count() >= 1);
        assert_eq!(built.subject_outcomes().len(), 2);
        assert_eq!(
            built.subject_outcomes()[0].endpoint_id(),
            &StableId::parse("function:payment-charge").unwrap()
        );
        assert_eq!(
            built.subject_outcomes()[1].endpoint_id(),
            &StableId::parse("function:checkout-submit").unwrap()
        );
        assert!(matches!(
            built.latent_cardinality(),
            ContextLatentCardinalityV3::Unknown { .. }
        ));
    }

    fn built_v3_read_only_fixture(support_anchors: Option<usize>) -> BuiltContextSubjectWindowsV3 {
        let (aggregate, bytes) = support_anchors.map_or_else(
            || fixture_from_program_value(d_context_ready_program_value(), default_source_bytes()),
            d_fixture_with_support_anchors,
        );
        let obligation_id = aggregate
            .obligations()
            .find(|obligation| obligation.version().rule() == SUBJECT_WINDOWS_D_RULE)
            .unwrap()
            .id()
            .clone();
        let mut session = prepare_subject_windows_v3(
            &aggregate,
            obligation_id,
            StableId::parse("function:checkout-submit").unwrap(),
            StableId::parse("function:payment-charge").unwrap(),
            2,
        )
        .unwrap();
        while let Some(request) = session.next_source_request().unwrap() {
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
        }
        session.finish().unwrap()
    }

    fn reseal_v3_value_for_semantic_mutation(value: &mut Value) {
        let raw: RawContextSubjectWindowsV3 = serde_json::from_value(value.clone()).unwrap();
        let context = raw.into_context();
        let projection_hash = context.identity_hash().unwrap();
        value["projection_hash"] = json!(projection_hash);
        value["context_id"] = json!(format!("context-envelope-v3:{projection_hash}"));
    }

    #[test]
    fn v3_read_only_validator_accepts_builder_canonical_value() {
        let built = built_v3_read_only_fixture(None);
        let canonical_value = built.canonical_value().unwrap();
        let validated = validate_subject_windows_v3_read_only(&canonical_value).unwrap();
        assert_eq!(validated.context_id(), built.id());
        assert_eq!(validated.projection_hash(), built.projection_hash());
        assert_eq!(validated.context(), &built);
    }

    #[test]
    fn v3_read_only_validator_rejects_each_denominator_count_and_hash_tamper() {
        let canonical_value = built_v3_read_only_fixture(None).canonical_value().unwrap();
        for denominator in [
            "accepted_file_denominator",
            "reached_file_denominator",
            "materialized_source_denominator",
            "support_anchor_denominator",
        ] {
            let mut count_tamper = canonical_value.clone();
            count_tamper[denominator]["observed_count"] = json!(4_294_967_291_u64);
            assert!(matches!(
                validate_subject_windows_v3_read_only(&count_tamper),
                Err(ContextError::SubjectWindowsV3Validation(_))
            ));

            let mut hash_tamper = canonical_value.clone();
            hash_tamper[denominator]["sorted_id_set_sha256"] =
                json!(ContentHash::sha256(denominator.as_bytes()));
            assert!(matches!(
                validate_subject_windows_v3_read_only(&hash_tamper),
                Err(ContextError::SubjectWindowsV3Validation(_))
            ));
        }
    }

    #[test]
    fn v3_read_only_validator_rejects_resealed_admitted_to_fake_loss_tamper() {
        let built = built_v3_read_only_fixture(None);
        let mut value = built.canonical_value().unwrap();
        let admitted = value["subject_outcomes"][0].clone();
        assert_eq!(admitted["state"], "admitted");
        let ContextSubjectOutcomeV3::Admitted {
            endpoint_id,
            role,
            source_artifact_id,
            requested_range,
            ..
        } = &built.subject_outcomes()[0]
        else {
            panic!("fixture callee must be admitted");
        };
        let forged_loss_id = subject_loss_id_v3(
            built.snapshot_id(),
            built.obligation_id(),
            built.property_id(),
            &SubjectLossV3Input {
                endpoint_id: endpoint_id.clone(),
                role: *role,
                reason: ContextWindowLossReasonV2::MissingSource,
                source_artifact_id: Some(source_artifact_id.clone()),
                requested_range: Some(requested_range.clone()),
            },
        )
        .unwrap();
        value["subject_outcomes"][0] = json!({
            "state": "lost",
            "loss": {
                "id": forged_loss_id,
                "endpoint_id": admitted["endpoint_id"],
                "role": admitted["role"],
                "reason": "missing_source",
                "source_artifact_id": admitted["source_artifact_id"],
                "requested_range": admitted["requested_range"],
                "property_id": built.property_id(),
                "severity": "high"
            }
        });
        reseal_v3_value_for_semantic_mutation(&mut value);
        assert!(matches!(
            validate_subject_windows_v3_read_only(&value),
            Err(ContextError::SubjectWindowsV3Validation(
                ContextSubjectWindowsV3ValidationError::SubjectOutcomes
            ))
        ));
    }

    #[test]
    fn v3_read_only_validator_rejects_resealed_support_partition_total_tamper() {
        let built = built_v3_read_only_fixture(Some(80));
        let mut value = built.canonical_value().unwrap();
        assert!(
            !value["support_loss_summaries"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let observed = value["support_loss_summaries"][0]["observed_count"]
            .as_u64()
            .unwrap();
        value["support_loss_summaries"][0]["observed_count"] = json!(observed + 1);
        reseal_v3_value_for_semantic_mutation(&mut value);
        assert!(matches!(
            validate_subject_windows_v3_read_only(&value),
            Err(ContextError::SubjectWindowsV3Validation(
                ContextSubjectWindowsV3ValidationError::SupportPartition
            ))
        ));
    }

    #[test]
    fn v3_read_only_validator_rejects_projection_hash_tamper() {
        let mut value = built_v3_read_only_fixture(None).canonical_value().unwrap();
        value["projection_hash"] = json!(ContentHash::sha256(b"forged projection"));
        assert!(matches!(
            validate_subject_windows_v3_read_only(&value),
            Err(ContextError::SubjectWindowsV3Validation(
                ContextSubjectWindowsV3ValidationError::ProjectionHash
            ))
        ));
    }

    #[test]
    fn v3_read_only_validator_typed_rejects_v2_cross_decode() {
        let v2_value = json!({
            "context_policy": ContextSubjectWindowsPolicyV2::fixed()
        });
        assert!(matches!(
            validate_subject_windows_v3_read_only(&v2_value),
            Err(ContextError::SubjectWindowsV3Validation(
                ContextSubjectWindowsV3ValidationError::WrongPolicy {
                    observed: Some(observed),
                }
            )) if observed == ContextSubjectWindowsPolicyV2::ID
        ));
    }

    #[test]
    fn v2_prepare_rejects_a_legacy_obligation_before_source_resolution() {
        let (aggregate, _) = fixture();
        let obligation_id = aggregate.obligations().next().unwrap().id().clone();
        assert!(matches!(
            prepare_subject_windows_v2(
                &aggregate,
                obligation_id,
                StableId::parse("function:checkout-submit").unwrap(),
                StableId::parse("function:payment-charge").unwrap(),
            ),
            Err(ContextError::SubjectBinding(
                ContextSubjectBindingErrorV2::WrongRule { .. }
            ))
        ));
    }

    fn mutated_obligation(obligation: &Obligation, mutate: impl FnOnce(&mut Value)) -> Obligation {
        let mut value = serde_json::to_value(obligation).unwrap();
        mutate(&mut value);
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn v2_binding_rejects_wrong_property() {
        let (program, obligation) = d_binding_fixture();
        let caller = StableId::parse("function:checkout-submit").unwrap();
        let callee = StableId::parse("function:payment-charge").unwrap();

        let wrong_property = mutated_obligation(&obligation, |value| {
            value["property_id"] = json!("payment.idempotency_contract")
        });
        assert!(matches!(
            validate_subject_binding_v2(&program, &wrong_property, &caller, &callee),
            Err(ContextError::SubjectBinding(
                ContextSubjectBindingErrorV2::WrongProperty { .. }
            ))
        ));
    }

    #[test]
    fn v2_binding_rejects_wrong_target_kind() {
        let (program, obligation) = d_binding_fixture();
        let caller = StableId::parse("function:checkout-submit").unwrap();
        let callee = StableId::parse("function:payment-charge").unwrap();
        let wrong_kind =
            mutated_obligation(&obligation, |value| value["target_kind"] = json!("node"));
        assert!(matches!(
            validate_subject_binding_v2(&program, &wrong_kind, &caller, &callee),
            Err(ContextError::SubjectBinding(
                ContextSubjectBindingErrorV2::WrongTargetKind { .. }
            ))
        ));
    }

    #[test]
    fn v2_binding_rejects_multiple_obligation_targets() {
        let (program, obligation) = d_binding_fixture();
        let caller = StableId::parse("function:checkout-submit").unwrap();
        let callee = StableId::parse("function:payment-charge").unwrap();
        let multiple_targets = mutated_obligation(&obligation, |value| {
            value["target_refs"] = json!([
                "relation:submit-calls-payment",
                "relation:payment-calls-stripe"
            ]);
            value["normalized_target_refs"] = json!([
                "relation:payment-calls-stripe",
                "relation:submit-calls-payment"
            ]);
        });
        assert!(matches!(
            validate_subject_binding_v2(&program, &multiple_targets, &caller, &callee,),
            Err(ContextError::SubjectBinding(
                ContextSubjectBindingErrorV2::ObligationTargetCardinality { observed: 2 }
            ))
        ));
    }

    #[test]
    fn v2_binding_rejects_nonaccepted_target_relation() {
        let (program, obligation) = d_binding_fixture();
        let missing = StableId::parse("relation:not-accepted").unwrap();
        let missing_target = mutated_obligation(&obligation, |value| {
            value["target_refs"] = json!([missing.as_str()]);
            value["normalized_target_refs"] = json!([missing.as_str()]);
        });
        assert!(matches!(
            validate_subject_binding_v2(
                &program,
                &missing_target,
                &StableId::parse("function:checkout-submit").unwrap(),
                &StableId::parse("function:payment-charge").unwrap(),
            ),
            Err(ContextError::SubjectBinding(
                ContextSubjectBindingErrorV2::TargetRelationNotAccepted { relation_id }
            )) if relation_id == missing
        ));
    }

    #[test]
    fn v2_binding_rejects_multi_target_relation() {
        let (_, obligation) = d_binding_fixture();
        let caller = StableId::parse("function:checkout-submit").unwrap();
        let callee = StableId::parse("function:payment-charge").unwrap();

        let mut multi_value = d_context_ready_program_value();
        multi_value["relations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|relation| relation["id"] == "relation:submit-calls-payment")
            .unwrap()["target_ids"] = json!(["function:payment-charge", "function:stripe-charge"]);
        let multi_program: ProgramSpace = serde_json::from_value(multi_value).unwrap();
        assert!(matches!(
            validate_subject_binding_v2(&multi_program, &obligation, &caller, &callee),
            Err(ContextError::SubjectBinding(
                ContextSubjectBindingErrorV2::RelationTargetCardinality { observed: 2, .. }
            ))
        ));
    }

    #[test]
    fn v2_binding_rejects_wrong_caller() {
        let (program, obligation) = d_binding_fixture();
        let callee = StableId::parse("function:payment-charge").unwrap();
        let wrong_caller = StableId::parse("function:payment-charge").unwrap();
        assert!(matches!(
            validate_subject_binding_v2(&program, &obligation, &wrong_caller, &callee),
            Err(ContextError::SubjectBinding(
                ContextSubjectBindingErrorV2::CallerMismatch { .. }
            ))
        ));
    }

    #[test]
    fn v2_binding_rejects_wrong_callee() {
        let (program, obligation) = d_binding_fixture();
        let caller = StableId::parse("function:checkout-submit").unwrap();
        let wrong_callee = StableId::parse("function:stripe-charge").unwrap();
        assert!(matches!(
            validate_subject_binding_v2(&program, &obligation, &caller, &wrong_callee),
            Err(ContextError::SubjectBinding(
                ContextSubjectBindingErrorV2::CalleeMismatch { .. }
            ))
        ));
    }

    #[test]
    fn v2_binding_rejects_nonaccepted_endpoint_instead_of_recording_projection_loss() {
        let (program, obligation) = d_binding_fixture();
        let missing = StableId::parse("function:not-accepted").unwrap();
        assert!(matches!(
            validate_subject_binding_v2(
                &program,
                &obligation,
                &missing,
                &StableId::parse("function:payment-charge").unwrap(),
            ),
            Err(ContextError::SubjectBinding(
                ContextSubjectBindingErrorV2::ProvidedEndpointNotAccepted {
                    role: "caller",
                    artifact_id,
                }
            )) if artifact_id == missing
        ));
    }

    #[test]
    fn v2_binding_rejects_wrong_rule_even_when_other_fields_look_like_d() {
        let (program, obligation) = d_binding_fixture();
        let wrong_rule = mutated_obligation(&obligation, |value| {
            value["version"]["rule"] = json!("relation.changed_call_contract@1")
        });
        assert!(matches!(
            validate_subject_binding_v2(
                &program,
                &wrong_rule,
                &StableId::parse("function:checkout-submit").unwrap(),
                &StableId::parse("function:payment-charge").unwrap(),
            ),
            Err(ContextError::SubjectBinding(
                ContextSubjectBindingErrorV2::WrongRule { .. }
            ))
        ));
    }

    #[test]
    fn v2_binding_accepts_only_the_exact_d_relation_endpoints() {
        let (program, obligation) = d_binding_fixture();
        validate_subject_binding_v2(
            &program,
            &obligation,
            &StableId::parse("function:checkout-submit").unwrap(),
            &StableId::parse("function:payment-charge").unwrap(),
        )
        .unwrap();
    }

    fn d_obligation_id(aggregate: &ReviewAggregate) -> StableId {
        aggregate
            .obligations()
            .find(|obligation| obligation.version().rule() == SUBJECT_WINDOWS_D_RULE)
            .expect("D obligation")
            .id()
            .clone()
    }

    #[test]
    fn v2_effect_preflight_stops_on_the_4097th_candidate_without_downstream_effects() {
        let (aggregate, _) = d_fixture_with_accepted_file_count(4_816);
        let trace = ContextBuildTrace::default();
        let result = prepare_subject_windows_v2_with_probe(
            &aggregate,
            d_obligation_id(&aggregate),
            StableId::parse("function:checkout-submit").unwrap(),
            StableId::parse("function:payment-charge").unwrap(),
            Some(Arc::new(trace.clone())),
        );
        assert!(matches!(
            result,
            Err(ContextError::Domain(DomainError::Incomplete {
                operation: "context candidate files",
                limit: 4_096,
                observed: 4_097,
            }))
        ));
        let effects = trace.snapshot();
        assert_eq!(
            effects
                .iter()
                .filter(|effect| matches!(
                    effect,
                    ContextBuildEffect::CandidateMetadataVisit { .. }
                ))
                .count(),
            4_097
        );
        assert_eq!(
            effects.last(),
            Some(&ContextBuildEffect::LimitFailure {
                operation: "context candidate files",
                limit: 4_096,
                observed: 4_097,
            })
        );
        assert!(!effects.iter().any(|effect| matches!(
            effect,
            ContextBuildEffect::CandidateMaterialized { .. }
                | ContextBuildEffect::SourceBytesRequested { .. }
                | ContextBuildEffect::SourceSubmitted { .. }
                | ContextBuildEffect::SubjectOutcome { .. }
        )));
    }

    fn v3_effect_trace(
        accepted_files: usize,
        unrelated_relations: usize,
    ) -> Vec<ContextBuildEffect> {
        let (aggregate, bytes) = d_fixture_with_scale(accepted_files, unrelated_relations);
        let trace = ContextBuildTrace::default();
        let mut session = prepare_subject_windows_v3_with_probe(
            &aggregate,
            d_obligation_id(&aggregate),
            StableId::parse("function:checkout-submit").unwrap(),
            StableId::parse("function:payment-charge").unwrap(),
            accepted_files,
            Some(Arc::new(trace.clone())),
        )
        .unwrap();
        while let Some(request) = session.next_source_request().unwrap() {
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
        }
        session.finish().unwrap();
        trace.snapshot()
    }

    #[test]
    fn v3_effect_probe_observes_only_materialized_source_access() {
        let effects = v3_effect_trace(4_816, 0);
        let ids = |predicate: fn(&ContextBuildEffect) -> Option<&StableId>| {
            effects
                .iter()
                .filter_map(predicate)
                .cloned()
                .collect::<Vec<_>>()
        };
        let materialized = ids(|effect| match effect {
            ContextBuildEffect::CandidateMaterialized { artifact_id } => Some(artifact_id),
            _ => None,
        });
        let metadata = ids(|effect| match effect {
            ContextBuildEffect::CandidateMetadataVisit { artifact_id } => Some(artifact_id),
            _ => None,
        });
        let requested = ids(|effect| match effect {
            ContextBuildEffect::SourceBytesRequested { artifact_id } => Some(artifact_id),
            _ => None,
        });
        let submitted = ids(|effect| match effect {
            ContextBuildEffect::SourceSubmitted { artifact_id } => Some(artifact_id),
            _ => None,
        });
        assert_eq!(materialized, metadata);
        let materialized_set = materialized.iter().cloned().collect::<BTreeSet<_>>();
        assert_eq!(materialized_set, requested.iter().cloned().collect());
        assert_eq!(materialized_set, submitted.iter().cloned().collect());
        assert_eq!(materialized.len(), requested.len());
        assert_eq!(materialized.len(), submitted.len());
        assert!(materialized.len() <= 3);
    }

    #[test]
    fn v3_context_effect_trace_is_independent_of_unrelated_denominator_scale() {
        let one = v3_effect_trace(3, 1);
        let eight = v3_effect_trace(24, 8);
        let sixty_four = v3_effect_trace(192, 64);
        assert_eq!(one, eight);
        assert_eq!(one, sixty_four);
        assert_eq!(
            one.iter()
                .filter(|effect| matches!(effect, ContextBuildEffect::DenominatorCommitmentLookup))
                .count(),
            1
        );
        assert!(!one.iter().any(|effect| matches!(
            effect,
            ContextBuildEffect::FullArtifactScan | ContextBuildEffect::FullRelationScan
        )));
    }

    fn reseal_v3_context(mut context: BuiltContextSubjectWindowsV3) -> Value {
        context.projection_hash = context.identity_hash().unwrap();
        context.id =
            StableId::parse(format!("context-envelope-v3:{}", context.projection_hash)).unwrap();
        serde_json::to_value(context).unwrap()
    }

    fn dummy_ids(kind: &str, count: u64, salt: &str) -> BTreeSet<StableId> {
        (0..count)
            .map(|index| StableId::parse(format!("{kind}:{salt}-{index:04}")).unwrap())
            .collect()
    }

    #[test]
    fn coherent_reseals_are_wire_valid_but_rejected_by_the_trusted_basis() {
        let (aggregate, bytes) = d_fixture_with_support_anchors(80);
        let aggregate = Arc::new(aggregate);
        let bytes = Arc::new(bytes);
        let basis = ContextValidationBasisV3::from_accepted_snapshot(
            aggregate.clone(),
            d_obligation_id(&aggregate),
            StableId::parse("function:checkout-submit").unwrap(),
            StableId::parse("function:payment-charge").unwrap(),
            2,
            bytes.clone(),
        )
        .unwrap();
        let mut session = prepare_subject_windows_v3(
            &aggregate,
            d_obligation_id(&aggregate),
            StableId::parse("function:checkout-submit").unwrap(),
            StableId::parse("function:payment-charge").unwrap(),
            2,
        )
        .unwrap();
        while let Some(request) = session.next_source_request().unwrap() {
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
        }
        let context = session.finish().unwrap();
        assert!(!context.support_loss_summaries.is_empty());
        assert!(!context.windows.is_empty());

        let mut mutants = Vec::<(&str, BuiltContextSubjectWindowsV3)>::new();

        let mut accepted = context.clone();
        let accepted_ids = dummy_ids(
            "file",
            accepted.accepted_file_denominator.observed_count + 1,
            "accepted-reseal",
        );
        accepted.accepted_file_denominator =
            denominator_commitment_v3("accepted reseal mutant", &accepted_ids).unwrap();
        mutants.push(("accepted denominator", accepted));

        let mut reached = context.clone();
        let reached_ids = dummy_ids(
            "file",
            reached.reached_file_denominator.observed_count,
            "reached-reseal",
        );
        reached.reached_file_denominator =
            denominator_commitment_v3("reached reseal mutant", &reached_ids).unwrap();
        mutants.push(("reached denominator", reached));

        let mut loss = context.clone();
        let lost_ids = dummy_ids(
            "context-support-anchor",
            loss.support_loss_summaries[0].observed_count,
            "loss-reseal",
        );
        loss.support_loss_summaries[0].sorted_anchor_id_set_sha256 =
            sorted_id_set_sha256(&lost_ids).unwrap();
        mutants.push(("support loss digest", loss));

        let mut support = context.clone();
        let support_ids = dummy_ids(
            "context-support-anchor",
            support.support_anchor_denominator.observed_count,
            "support-reseal",
        );
        support.support_anchor_denominator =
            denominator_commitment_v3("support reseal mutant", &support_ids).unwrap();
        mutants.push(("support anchor denominator", support));

        let mut materialized = context.clone();
        let old_id = materialized.materialized_sources[0].artifact_id.clone();
        let new_id = StableId::parse("file:zz-materialized-reseal").unwrap();
        materialized.materialized_sources[0].artifact_id = new_id.clone();
        let mut remapped_windows = BTreeMap::new();
        for window in &mut materialized.windows {
            if window.source_artifact_id == old_id {
                let old_window_id = window.id.clone();
                window.source_artifact_id = new_id.clone();
                window.id = context_window_id_v3(
                    &materialized.snapshot_id,
                    &materialized.obligation_id,
                    &ContextWindowIdentityV3 {
                        source_artifact_id: &window.source_artifact_id,
                        registration_id: &window.registration_id,
                        content_hash: &window.content_hash,
                        cas_hash: &window.cas_hash,
                        range: &window.range,
                        owner_ids: &window.owner_ids,
                        roles: &window.roles,
                        support_anchor_ids: &window.support_anchor_ids,
                        excerpt_byte_length: window.excerpt_byte_length,
                        excerpt_hash: &window.excerpt_hash,
                    },
                )
                .unwrap();
                remapped_windows.insert(old_window_id, window.id.clone());
            }
        }
        for outcome in &mut materialized.subject_outcomes {
            match outcome {
                ContextSubjectOutcomeV3::Admitted {
                    source_artifact_id,
                    window_id,
                    ..
                } => {
                    if *source_artifact_id == old_id {
                        *source_artifact_id = new_id.clone();
                    }
                    if let Some(new_window_id) = remapped_windows.get(window_id) {
                        *window_id = new_window_id.clone();
                    }
                }
                ContextSubjectOutcomeV3::Lost { loss } => {
                    if loss.source_artifact_id.as_ref() == Some(&old_id) {
                        loss.source_artifact_id = Some(new_id.clone());
                        let input = SubjectLossV3Input {
                            endpoint_id: loss.endpoint_id.clone(),
                            role: loss.role,
                            reason: loss.reason,
                            source_artifact_id: loss.source_artifact_id.clone(),
                            requested_range: loss.requested_range.clone(),
                        };
                        loss.id = subject_loss_id_v3(
                            &materialized.snapshot_id,
                            &materialized.obligation_id,
                            &materialized.property_id,
                            &input,
                        )
                        .unwrap();
                    }
                }
            }
        }
        materialized
            .materialized_sources
            .sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
        materialized.windows.sort_by(|left, right| {
            (&left.source_artifact_id, &left.range, &left.id).cmp(&(
                &right.source_artifact_id,
                &right.range,
                &right.id,
            ))
        });
        let materialized_ids = materialized
            .materialized_sources
            .iter()
            .map(|source| source.artifact_id.clone())
            .collect::<BTreeSet<_>>();
        materialized.materialized_source_denominator =
            denominator_commitment_v3("materialized reseal mutant", &materialized_ids).unwrap();
        mutants.push(("materialized denominator", materialized));

        for (name, mutant) in mutants {
            let value = reseal_v3_context(mutant);
            validate_subject_windows_v3_wire_read_only(&value)
                .unwrap_or_else(|error| panic!("wire rejected coherent {name}: {error}"));
            assert!(matches!(
                validate_subject_windows_v3_against_basis(&value, &basis),
                Err(ContextError::SubjectWindowsV3Validation(
                    ContextSubjectWindowsV3ValidationError::BasisMismatch
                ))
            ));
        }
    }

    #[test]
    fn independently_rebuilt_basis_rejects_a_coherently_wrong_builder_set() {
        let (aggregate, bytes) = d_fixture_with_accepted_file_count(3);
        let aggregate = Arc::new(aggregate);
        let bytes = Arc::new(bytes);
        let obligation_id = d_obligation_id(&aggregate);
        let caller_id = StableId::parse("function:checkout-submit").unwrap();
        let callee_id = StableId::parse("function:payment-charge").unwrap();
        let basis = ContextValidationBasisV3::from_accepted_snapshot(
            aggregate.clone(),
            obligation_id.clone(),
            caller_id.clone(),
            callee_id.clone(),
            3,
            bytes.clone(),
        )
        .unwrap();

        OMIT_ONE_ACCEPTED_FILE_IN_BUILDER_MUTANT.with(|enabled| enabled.set(true));
        let built = (|| {
            let mut session =
                prepare_subject_windows_v3(&aggregate, obligation_id, caller_id, callee_id, 3)?;
            while let Some(request) = session.next_source_request()? {
                session.submit_source(&request, &bytes[request.artifact_id()])?;
            }
            session.finish()
        })();
        let mutant = built.unwrap();
        assert_eq!(mutant.accepted_file_denominator.observed_count, 2);
        let value = mutant.canonical_value().unwrap();
        validate_subject_windows_v3_wire_read_only(&value).unwrap();
        let semantic = validate_subject_windows_v3_against_basis(&value, &basis);
        OMIT_ONE_ACCEPTED_FILE_IN_BUILDER_MUTANT.with(|enabled| enabled.set(false));
        assert!(matches!(
            semantic,
            Err(ContextError::SubjectWindowsV3Validation(
                ContextSubjectWindowsV3ValidationError::BasisMismatch
            ))
        ));
    }

    #[test]
    fn oracle_rejects_a_coherently_missing_reached_file() {
        let (aggregate, bytes) = d_fixture_with_accepted_file_count(3);
        let aggregate = Arc::new(aggregate);
        let bytes = Arc::new(bytes);
        let obligation_id = d_obligation_id(&aggregate);
        let caller_id = StableId::parse("function:checkout-submit").unwrap();
        let callee_id = StableId::parse("function:payment-charge").unwrap();
        let basis = ContextValidationBasisV3::from_accepted_snapshot(
            aggregate.clone(),
            obligation_id.clone(),
            caller_id.clone(),
            callee_id.clone(),
            3,
            bytes.clone(),
        )
        .unwrap();
        OMIT_ONE_REACHED_FILE_IN_BUILDER_MUTANT.with(|enabled| enabled.set(true));
        let mut session =
            prepare_subject_windows_v3(&aggregate, obligation_id, caller_id, callee_id, 3).unwrap();
        while let Some(request) = session.next_source_request().unwrap() {
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
        }
        let mutant = session.finish().unwrap();
        let value = mutant.canonical_value().unwrap();
        validate_subject_windows_v3_wire_read_only(&value).unwrap();
        let semantic = validate_subject_windows_v3_against_basis(&value, &basis);
        OMIT_ONE_REACHED_FILE_IN_BUILDER_MUTANT.with(|enabled| enabled.set(false));
        assert!(matches!(
            semantic,
            Err(ContextError::SubjectWindowsV3Validation(
                ContextSubjectWindowsV3ValidationError::BasisMismatch
            ))
        ));
    }

    #[test]
    fn oracle_rejects_a_coherently_missing_anchor_file() {
        let (aggregate, bytes) = d_fixture_with_support_anchors(80);
        let aggregate = Arc::new(aggregate);
        let bytes = Arc::new(bytes);
        let obligation_id = d_obligation_id(&aggregate);
        let caller_id = StableId::parse("function:checkout-submit").unwrap();
        let callee_id = StableId::parse("function:payment-charge").unwrap();
        let basis = ContextValidationBasisV3::from_accepted_snapshot(
            aggregate.clone(),
            obligation_id.clone(),
            caller_id.clone(),
            callee_id.clone(),
            2,
            bytes.clone(),
        )
        .unwrap();
        OMIT_ONE_ANCHOR_FILE_IN_BUILDER_MUTANT.with(|enabled| enabled.set(true));
        let mut session =
            prepare_subject_windows_v3(&aggregate, obligation_id, caller_id, callee_id, 2).unwrap();
        while let Some(request) = session.next_source_request().unwrap() {
            session
                .submit_source(&request, &bytes[request.artifact_id()])
                .unwrap();
        }
        let mutant = session.finish().unwrap();
        let value = mutant.canonical_value().unwrap();
        validate_subject_windows_v3_wire_read_only(&value).unwrap();
        let semantic = validate_subject_windows_v3_against_basis(&value, &basis);
        OMIT_ONE_ANCHOR_FILE_IN_BUILDER_MUTANT.with(|enabled| enabled.set(false));
        assert!(matches!(
            semantic,
            Err(ContextError::SubjectWindowsV3Validation(
                ContextSubjectWindowsV3ValidationError::BasisMismatch
            ))
        ));
    }

    #[test]
    fn validation_basis_debug_is_bounded_and_never_descends_into_arc_contents() {
        let (aggregate, bytes) = d_fixture_with_accepted_file_count(4_816);
        let obligation_id = d_obligation_id(&aggregate);
        let aggregate = Arc::new(aggregate);
        let bytes = Arc::new(bytes);
        let basis = ContextValidationBasisV3::from_accepted_snapshot(
            aggregate,
            obligation_id,
            StableId::parse("function:checkout-submit").unwrap(),
            StableId::parse("function:payment-charge").unwrap(),
            4_816,
            bytes,
        )
        .unwrap();
        let debug = format!("{basis:?}");
        assert!(
            debug.len() <= 1_024,
            "basis Debug grew to {} bytes",
            debug.len()
        );
        assert!(!debug.contains("synthetic/scale-4815.rs"));
        assert!(!debug.contains("scale-4815"));
        assert!(!debug.contains("registered_artifacts"));
        assert!(!debug.contains("source_bytes_by_artifact_id"));
    }
}
