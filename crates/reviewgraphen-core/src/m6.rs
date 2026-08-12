//! Pure deterministic M6 incremental-review domain contracts.
//!
//! This module owns no journal, authority capability, CAS handle, runtime
//! operation, report projection, or historical mutable state.  It constructs
//! only validated, versioned values described by ADR 0023.  In particular,
//! structural preservation is audit evidence and never becomes a native M4
//! verification, human decision, finding, gluing authority, or gate credit.

use crate::event::{
    HistoricalPrefixAdmissionV4, HistoricalPrefixProjectionV4, HistoricalSourceRecordKindV4,
    HistoricalSourceRecordProjectionV4, HistoricalSourceRecordValueV4,
    TargetActualRecordInventoryV5, TargetActualRecordProjectionV5, TargetActualRecordV5,
    V5PreIncrementalStructuralPrefixProjection,
};
use crate::{
    ArtifactSourceV3, ArtifactSourceV4, AuthorityReplayBasisV4, ContentHash, DecisionOutcomeV3,
    DomainError, EventLogV4, EventLogV5, FindingStatusV3, M5CompletedGluingProfileV4, Obligation,
    ProgramSpace, ReviewAggregate, StableId, VerificationOutcomeV3,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

pub const PROGRAM_MAPPING_POLICY_V5: &str = "reviewgraphen.program_mapping@1";
pub const RUST_SYMBOL_ANCHOR_V1: &str = "reviewgraphen.rust_symbol_anchor@1";
pub const GIT_CHANGE_PROVENANCE_V1: &str = "reviewgraphen.ingest.git.changed_structure.v1";
pub const OBLIGATION_CORRESPONDENCE_POLICY_V5: &str = "reviewgraphen.obligation_correspondence@1";
pub const MAX_M6_PROGRAM_DOMAIN_IDS: usize = 4_096;
pub const MAX_M6_MAPPINGS: usize = 8_192;
pub const MAX_M6_MAPPING_SIDE_IDS: usize = 64;
pub const MAX_M6_MAPPING_LINK_IDS: usize = 64;
pub const MAX_M6_CANONICAL_BYTES: usize = 1_048_576;
pub const MAX_M6_MAPPING_DTO_BYTES: usize = 1_048_576;
pub const MAX_M6_CLOSURE_DTO_BYTES: usize = 1_048_576;
pub const MAX_M6_MORPHISM_DTO_BYTES: usize = 1_048_576;
pub const MAX_M6_EVENT_LINE_BYTES: usize = 1_048_576;
pub const MAX_M6_MAPPING_WORKING_BYTES: usize = 536_870_912;
pub const MAX_M6_OBLIGATIONS_PER_UNIVERSE: usize = 2_048;
pub const MAX_M6_CORRESPONDENCE_ENTRIES: usize = 4_096;
pub const MAX_M6_CORRESPONDENCE_SIDE_IDS: usize = 64;
pub const MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS: usize = 64;
pub const MAX_M6_CORRESPONDENCE_DTO_BYTES: usize = 1_048_576;
pub const MAX_M6_CORRESPONDENCE_WORKING_BYTES: usize = 536_870_912;
pub const MAX_M6_STALENESS_WORKING_BYTES: usize = 536_870_912;
pub const MAX_M6_RECORD_METADATA_IDS: usize = 512;

pub type M6Result<T> = std::result::Result<T, M6Error>;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum M6Error {
    #[error("invalid M6 source/target closure: {0}")]
    InvalidSourceClosure(&'static str),
    #[error("invalid full lowercase SHA-256 in {field}: {value}")]
    InvalidFullSha256 { field: &'static str, value: String },
    #[error("invalid full lowercase Git object ID in {field}: {value}")]
    InvalidGitObjectId { field: &'static str, value: String },
    #[error("invalid M6 mapping component: {0}")]
    InvalidMapping(&'static str),
    #[error("invalid M6 obligation universe: {0}")]
    InvalidObligationUniverse(&'static str),
    #[error("invalid M6 historical staleness topology: {0}")]
    InvalidHistoricalTopology(&'static str),
    #[error("missing accepted M6 {kind} fact for {object_id}")]
    MissingAcceptedMappingFact {
        kind: &'static str,
        object_id: StableId,
    },
    #[error("{operation} exceeds limit {limit} (observed {observed})")]
    Incomplete {
        operation: &'static str,
        limit: usize,
        observed: usize,
    },
    #[error("canonical M6 construction failed: {0}")]
    Canonical(String),
    #[error("invalid M6 wire value: {0}")]
    InvalidWire(String),
}

impl From<DomainError> for M6Error {
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
            other => Self::Canonical(other.to_string()),
        }
    }
}

fn bounded(observed: usize, limit: usize, operation: &'static str) -> M6Result<()> {
    if observed > limit {
        return Err(M6Error::Incomplete {
            operation,
            limit,
            observed,
        });
    }
    Ok(())
}

fn derive(kind: &str, identity: &impl Serialize) -> M6Result<StableId> {
    bounded_serialized(identity, MAX_M6_CANONICAL_BYTES, "M6 identity bytes")?;
    let hash = ContentHash::sha256(&crate::canonical_json(identity)?);
    StableId::parse(format!("{kind}:{hash}")).map_err(Into::into)
}

fn body_hash(value: &impl Serialize) -> M6Result<ContentHash> {
    bounded_serialized(value, MAX_M6_CANONICAL_BYTES, "M6 canonical body bytes")?;
    Ok(ContentHash::sha256(&crate::canonical_json(value)?))
}

fn bounded_serialized(
    value: &impl Serialize,
    limit: usize,
    operation: &'static str,
) -> M6Result<()> {
    crate::canonical::canonical_json_count_bounded(value, limit, operation)?;
    Ok(())
}

fn bounded_event_dto(
    value: &impl Serialize,
    limit: usize,
    operation: &'static str,
) -> M6Result<()> {
    let bytes = usize::try_from(crate::canonical::canonical_json_count_bounded(
        value, limit, operation,
    )?)
    .map_err(|_| M6Error::Incomplete {
        operation,
        limit,
        observed: usize::MAX,
    })?;
    preflight_event_line(bytes, 1)?;
    Ok(())
}

fn checked_working_add(total: usize, addition: usize) -> M6Result<usize> {
    let observed = total.checked_add(addition).ok_or(M6Error::Incomplete {
        operation: "M6 mapping retained working bytes",
        limit: MAX_M6_MAPPING_WORKING_BYTES,
        observed: usize::MAX,
    })?;
    bounded(
        observed,
        MAX_M6_MAPPING_WORKING_BYTES,
        "M6 mapping retained working bytes",
    )?;
    Ok(observed)
}

fn full_sha256(field: &'static str, value: &ContentHash) -> M6Result<()> {
    let Some(hex) = value.as_str().strip_prefix("sha256:") else {
        return Err(M6Error::InvalidFullSha256 {
            field,
            value: value.to_string(),
        });
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(M6Error::InvalidFullSha256 {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn git_oid(field: &'static str, value: &str) -> M6Result<()> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(M6Error::InvalidGitObjectId {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn git_tree(field: &'static str, value: &ContentHash) -> M6Result<()> {
    let Some(hex) = value.as_str().strip_prefix("git:") else {
        return Err(M6Error::InvalidGitObjectId {
            field,
            value: value.to_string(),
        });
    };
    git_oid(field, hex)
}

fn require_kind(id: &StableId, kind: &'static str, field: &'static str) -> M6Result<()> {
    if id.kind() != kind {
        return Err(M6Error::Canonical(format!(
            "{field} must have kind {kind}, got {id}"
        )));
    }
    Ok(())
}

fn digest_ids(ids: &BTreeSet<StableId>) -> M6Result<ContentHash> {
    body_hash(&ids.iter().collect::<Vec<_>>())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IdBodyHashV5 {
    body_hash: ContentHash,
    id: StableId,
}

impl<'de> Deserialize<'de> for IdBodyHashV5 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            body_hash: ContentHash,
            id: StableId,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.id, wire.body_hash).map_err(serde::de::Error::custom)
    }
}

impl IdBodyHashV5 {
    pub fn new(id: StableId, body_hash: ContentHash) -> M6Result<Self> {
        full_sha256("body_hash", &body_hash)?;
        Ok(Self { body_hash, id })
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    #[must_use]
    pub fn body_hash(&self) -> &ContentHash {
        &self.body_hash
    }
}

fn digest_records(records: &[IdBodyHashV5]) -> M6Result<ContentHash> {
    if records.windows(2).any(|pair| pair[0].id >= pair[1].id) {
        return Err(M6Error::Canonical(
            "M6 digest records must be strictly ID ordered".to_owned(),
        ));
    }
    crate::canonical::compact_json_sha256_streaming(&records).map_err(Into::into)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[doc(hidden)]
pub(crate) struct IncrementalStructuralInputV5 {
    repository_id: StableId,
    repository_identity_hash: ContentHash,
    source_run_id: StableId,
    source_genesis_hash: ContentHash,
    source_confirmed_offset: u64,
    source_tail_hash: ContentHash,
    source_event_count: u64,
    source_snapshot_id: StableId,
    source_universe_id: StableId,
    source_index_snapshot_hash: ContentHash,
    source_authority_policy_revision_hash: ContentHash,
    source_authority_replay_basis_digest: ContentHash,
    source_resolved_target_commit_oid: String,
    source_target_tree_hash: ContentHash,
    source_gluing_bundle_id: StableId,
    target_run_id: StableId,
    target_genesis_hash: ContentHash,
    target_predecessor_offset: u64,
    target_predecessor_tail_hash: ContentHash,
    target_predecessor_event_count: u64,
    target_snapshot_id: StableId,
    target_universe_id: StableId,
    target_predecessor_index_snapshot_hash: ContentHash,
    target_authority_policy_revision_hash: ContentHash,
    target_pre_incremental_authority_replay_basis_digest: ContentHash,
    target_resolved_base_commit_oid: String,
    target_base_tree_hash: ContentHash,
    target_resolved_target_commit_oid: String,
    target_target_tree_hash: ContentHash,
}

#[derive(Clone, Debug)]
pub(crate) struct ValidatedIncrementalStructureV5 {
    input: IncrementalStructuralInputV5,
}

impl ValidatedIncrementalStructureV5 {
    /// Validates Store's inert projection of an opaque locked dual-run proof.
    /// This value grants no append or replay authority.
    #[doc(hidden)]
    pub(crate) fn validate_store_projection(
        source: &ProgramSpace,
        target: &ProgramSpace,
        input: IncrementalStructuralInputV5,
    ) -> M6Result<Self> {
        if source.repository_id() != target.repository_id()
            || source.repository_id() != &input.repository_id
        {
            return Err(M6Error::InvalidSourceClosure(
                "source and target must be the same accepted repository",
            ));
        }
        if source.m6_is_dirty() || target.m6_is_dirty() {
            return Err(M6Error::InvalidSourceClosure(
                "source and target ProgramSpace snapshots must be clean",
            ));
        }
        if source.snapshot_id() != &input.source_snapshot_id
            || target.snapshot_id() != &input.target_snapshot_id
            || source.m6_tree_hash() != &input.source_target_tree_hash
            || target.m6_tree_hash() != &input.target_target_tree_hash
            || source.target_revision() != input.source_resolved_target_commit_oid
            || target.base_revision() != input.target_resolved_base_commit_oid
            || target.target_revision() != input.target_resolved_target_commit_oid
        {
            return Err(M6Error::InvalidSourceClosure(
                "session proof does not match the accepted ProgramSpace revisions",
            ));
        }
        if input.source_gluing_bundle_id.kind() != "event" {
            return Err(M6Error::InvalidSourceClosure(
                "source gluing bundle must bind its durable event ID",
            ));
        }
        Ok(Self { input })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IncrementalSourceClosureV5 {
    schema: &'static str,
    id: StableId,
    #[serde(flatten)]
    input: IncrementalStructuralInputV5,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IncrementalSourceClosureWireV5 {
    schema: String,
    id: StableId,
    #[serde(flatten)]
    input: IncrementalStructuralInputV5,
}

impl IncrementalSourceClosureV5 {
    #[doc(hidden)]
    pub(crate) fn from_validated_structure(
        proof: ValidatedIncrementalStructureV5,
    ) -> M6Result<Self> {
        let input = proof.input;
        require_kind(&input.repository_id, "repository", "repository_id")?;
        require_kind(&input.source_run_id, "run", "source_run_id")?;
        require_kind(&input.target_run_id, "run", "target_run_id")?;
        require_kind(&input.source_snapshot_id, "snapshot", "source_snapshot_id")?;
        require_kind(&input.target_snapshot_id, "snapshot", "target_snapshot_id")?;
        require_kind(&input.source_universe_id, "universe", "source_universe_id")?;
        require_kind(&input.target_universe_id, "universe", "target_universe_id")?;
        for (field, hash) in [
            ("repository_identity_hash", &input.repository_identity_hash),
            ("source_genesis_hash", &input.source_genesis_hash),
            ("source_tail_hash", &input.source_tail_hash),
            (
                "source_index_snapshot_hash",
                &input.source_index_snapshot_hash,
            ),
            (
                "source_authority_policy_revision_hash",
                &input.source_authority_policy_revision_hash,
            ),
            (
                "source_authority_replay_basis_digest",
                &input.source_authority_replay_basis_digest,
            ),
            ("target_genesis_hash", &input.target_genesis_hash),
            (
                "target_predecessor_tail_hash",
                &input.target_predecessor_tail_hash,
            ),
            (
                "target_predecessor_index_snapshot_hash",
                &input.target_predecessor_index_snapshot_hash,
            ),
            (
                "target_authority_policy_revision_hash",
                &input.target_authority_policy_revision_hash,
            ),
            (
                "target_pre_incremental_authority_replay_basis_digest",
                &input.target_pre_incremental_authority_replay_basis_digest,
            ),
        ] {
            full_sha256(field, hash)?;
        }
        git_oid(
            "source_resolved_target_commit_oid",
            &input.source_resolved_target_commit_oid,
        )?;
        git_oid(
            "target_resolved_base_commit_oid",
            &input.target_resolved_base_commit_oid,
        )?;
        git_oid(
            "target_resolved_target_commit_oid",
            &input.target_resolved_target_commit_oid,
        )?;
        git_tree("source_target_tree_hash", &input.source_target_tree_hash)?;
        git_tree("target_base_tree_hash", &input.target_base_tree_hash)?;
        git_tree("target_target_tree_hash", &input.target_target_tree_hash)?;
        if input.source_event_count == 0 || input.target_predecessor_event_count == 0 {
            return Err(M6Error::InvalidSourceClosure(
                "both pinned prefixes must be nonempty",
            ));
        }
        if input.source_run_id == input.target_run_id
            || input.source_snapshot_id == input.target_snapshot_id
        {
            return Err(M6Error::InvalidSourceClosure(
                "source and target run/snapshot IDs must differ",
            ));
        }
        if input.source_resolved_target_commit_oid != input.target_resolved_base_commit_oid
            || input.source_target_tree_hash != input.target_base_tree_hash
        {
            return Err(M6Error::InvalidSourceClosure(
                "source target must equal target base commit and tree",
            ));
        }
        if input.target_resolved_base_commit_oid == input.target_resolved_target_commit_oid {
            return Err(M6Error::InvalidSourceClosure(
                "target base and target commit must differ",
            ));
        }
        let id = derive("incremental-source-closure-v5", &input)?;
        let value = Self {
            schema: "reviewgraphen.incremental_source_closure.v5",
            id,
            input,
        };
        bounded_event_dto(&value, MAX_M6_CLOSURE_DTO_BYTES, "M6 closure DTO bytes")?;
        Ok(value)
    }

    pub(crate) fn from_json_bytes(input: &[u8], expected: &Self) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_CLOSURE_DTO_BYTES,
            "M6 closure JSON bytes",
        )?;
        let wire: IncrementalSourceClosureWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.incremental_source_closure.v5"
            || wire.id != expected.id
            || wire.input != expected.input
            || crate::canonical_json(expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "closure wire is not exact canonical session-proof content".to_owned(),
            ));
        }
        Ok(expected.clone())
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    #[must_use]
    pub(crate) fn input(&self) -> &IncrementalStructuralInputV5 {
        &self.input
    }

    // These deliberately remain crate-private.  They bind the reducer input
    // to an already validated dual-run proof; they do not expose a way to
    // manufacture or amend that proof.
    pub(crate) fn source_tail_hash(&self) -> &ContentHash {
        &self.input.source_tail_hash
    }

    pub(crate) fn target_predecessor_tail_hash(&self) -> &ContentHash {
        &self.input.target_predecessor_tail_hash
    }

    pub(crate) fn source_snapshot_id(&self) -> &StableId {
        &self.input.source_snapshot_id
    }

    pub(crate) fn target_snapshot_id(&self) -> &StableId {
        &self.input.target_snapshot_id
    }

    pub(crate) fn source_universe_id(&self) -> &StableId {
        &self.input.source_universe_id
    }

    pub(crate) fn target_universe_id(&self) -> &StableId {
        &self.input.target_universe_id
    }

    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

/// Deterministic M6 proposal rebuilt from already-replayed Core prefixes.
/// It carries no durable-store, CAS, index, append, report, or acceptance
/// authority. Store may retain it only inside its separate locked accepted
/// owner after independently validating the supplied index hashes.
///
/// The structural DTO and closure mint are deliberately not nameable outside
/// Core; callers can only obtain this zero-authority proposal by replay:
/// ```compile_fail
/// fn bypass(_: reviewgraphen_core::m6::IncrementalStructuralInputV5) {}
/// ```
pub struct UntrustedIncrementalMappingProposalV5 {
    closure: IncrementalSourceClosureV5,
    phase: M6MappingPhaseV5,
}

impl UntrustedIncrementalMappingProposalV5 {
    #[must_use]
    pub const fn closure(&self) -> &IncrementalSourceClosureV5 {
        &self.closure
    }

    #[must_use]
    pub const fn mapping_phase(&self) -> &M6MappingPhaseV5 {
        &self.phase
    }

    #[must_use]
    pub fn morphism(&self) -> &ChangeMorphismV5 {
        self.phase.morphism()
    }
}

#[derive(Serialize)]
struct ProposalRepositoryIdentityV5<'a> {
    repository_id: &'a StableId,
    repository_identity: &'a str,
}

#[derive(Serialize)]
struct ProposalTargetPolicyV5<'a> {
    schema: &'static str,
    profile_id: &'a str,
    profile_version: &'a str,
    policy_version: &'a str,
    rule_set_hash: &'a ContentHash,
    extractor_set_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct ProposalTargetReplayBasisV5<'a> {
    schema: &'static str,
    run_id: &'a StableId,
    genesis_hash: &'a ContentHash,
    confirmed_offset: u64,
    tail_hash: &'a ContentHash,
    event_count: u64,
    snapshot_id: &'a StableId,
    universe_id: &'a StableId,
    plan_id: &'a StableId,
    plan_body_hash: &'a ContentHash,
    policy_revision_hash: &'a ContentHash,
}

/// Rebuilds a zero-authority proposal from exact replay-owned source/target
/// logs. The only external scalars are hashes of Store's already locked index
/// snapshots; they remain proposal inputs and gain authority only through the
/// Store-owned accepted wrapper that retains those index handles.
#[doc(hidden)]
pub fn derive_untrusted_incremental_mapping_proposal_v5(
    source_log: &EventLogV4,
    source_basis: &AuthorityReplayBasisV4,
    completed: &M5CompletedGluingProfileV4,
    source_index_snapshot_hash: &ContentHash,
    target_log: &EventLogV5,
    target_index_snapshot_hash: &ContentHash,
) -> M6Result<UntrustedIncrementalMappingProposalV5> {
    let source_genesis = crate::RunGenesisSnapshot::from_canonical_v4_bytes_for_store(
        source_log.canonical_genesis_bytes(),
    )?;
    let source_program = source_genesis.program_space_for_store();
    let target_state = target_log.replay_pre_incremental_state_for_store()?;
    let target_projection = target_state.projection();
    let target_program = target_projection.program_space();
    let source_git =
        source_program
            .accepted_git_revision_closure()
            .ok_or(M6Error::InvalidSourceClosure(
                "source ProgramSpace lacks accepted Git revision closure",
            ))?;
    let target_git =
        target_program
            .accepted_git_revision_closure()
            .ok_or(M6Error::InvalidSourceClosure(
                "target ProgramSpace lacks accepted Git revision closure",
            ))?;
    let source_event_count =
        u64::try_from(source_log.envelopes().len()).map_err(|_| M6Error::Incomplete {
            operation: "M6 source replay event count",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;
    let target_event_count =
        u64::try_from(target_log.envelopes().len()).map_err(|_| M6Error::Incomplete {
            operation: "M6 target replay event count",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;
    if source_basis.run_id() != source_log.run_id()
        || source_basis.genesis_hash() != source_log.genesis_hash()
        || source_basis.confirmed_tail_hash() != source_log.tail_hash()
        || source_basis.confirmed_event_count() != source_event_count
        || completed.confirmed_tail_hash() != source_log.tail_hash()
        || completed.confirmed_event_count() != source_event_count
        || !source_log
            .envelopes()
            .iter()
            .any(|event| event.id() == completed.event_id())
    {
        return Err(M6Error::InvalidSourceClosure(
            "source replay basis and completed M5 event must equal the replayed prefix",
        ));
    }
    let repository_identity_hash =
        ContentHash::sha256(&crate::canonical_json(&ProposalRepositoryIdentityV5 {
            repository_id: source_program.repository_id(),
            repository_identity: source_program.repository_identity(),
        })?);
    let target_policy_revision_hash =
        ContentHash::sha256(&crate::canonical_json(&ProposalTargetPolicyV5 {
            schema: "reviewgraphen.target_policy_identity.v6",
            profile_id: target_program.profile_id(),
            profile_version: target_program.profile_version(),
            policy_version: target_program.policy_version(),
            rule_set_hash: target_program.rule_set_hash(),
            extractor_set_hash: target_program.extractor_set_hash(),
        })?);
    let target_plan_body_hash = ContentHash::sha256(&target_projection.plan().canonical_bytes()?);
    let target_confirmed_offset = target_log.canonical_prefix_bytes_for_store();
    let target_replay_basis_digest =
        ContentHash::sha256(&crate::canonical_json(&ProposalTargetReplayBasisV5 {
            schema: "reviewgraphen.target_pre_incremental_replay_basis.v6",
            run_id: target_log.run_id(),
            genesis_hash: target_log.genesis_hash(),
            confirmed_offset: target_confirmed_offset,
            tail_hash: target_log.tail_hash(),
            event_count: target_event_count,
            snapshot_id: target_program.snapshot_id(),
            universe_id: target_projection.universe().id(),
            plan_id: target_projection.plan().id(),
            plan_body_hash: &target_plan_body_hash,
            policy_revision_hash: &target_policy_revision_hash,
        })?);
    let input = IncrementalStructuralInputV5 {
        repository_id: source_program.repository_id().clone(),
        repository_identity_hash,
        source_run_id: source_log.run_id().clone(),
        source_genesis_hash: source_log.genesis_hash().clone(),
        source_confirmed_offset: source_log.canonical_prefix_bytes_for_store()?,
        source_tail_hash: source_log.tail_hash().clone(),
        source_event_count,
        source_snapshot_id: source_program.snapshot_id().clone(),
        source_universe_id: source_genesis.universe().id().clone(),
        source_index_snapshot_hash: source_index_snapshot_hash.clone(),
        source_authority_policy_revision_hash: source_basis.policy_revision_hash().clone(),
        source_authority_replay_basis_digest: source_basis.basis_digest().clone(),
        source_resolved_target_commit_oid: source_git.target_commit_oid().to_owned(),
        source_target_tree_hash: source_git.target_tree_hash().clone(),
        source_gluing_bundle_id: completed.event_id().clone(),
        target_run_id: target_log.run_id().clone(),
        target_genesis_hash: target_log.genesis_hash().clone(),
        target_predecessor_offset: target_confirmed_offset,
        target_predecessor_tail_hash: target_log.tail_hash().clone(),
        target_predecessor_event_count: target_event_count,
        target_snapshot_id: target_program.snapshot_id().clone(),
        target_universe_id: target_projection.universe().id().clone(),
        target_predecessor_index_snapshot_hash: target_index_snapshot_hash.clone(),
        target_authority_policy_revision_hash: target_policy_revision_hash,
        target_pre_incremental_authority_replay_basis_digest: target_replay_basis_digest,
        target_resolved_base_commit_oid: target_git.base_commit_oid().to_owned(),
        target_base_tree_hash: target_git.base_tree_hash().clone(),
        target_resolved_target_commit_oid: target_git.target_commit_oid().to_owned(),
        target_target_tree_hash: target_git.target_tree_hash().clone(),
    };
    let validated = ValidatedIncrementalStructureV5::validate_store_projection(
        source_program,
        target_program,
        input,
    )?;
    let closure = IncrementalSourceClosureV5::from_validated_structure(validated)?;
    let phase = ChangeMorphismV5::derive_from_accepted_program_facts(
        &closure,
        source_program,
        target_program,
    )?;
    Ok(UntrustedIncrementalMappingProposalV5 { closure, phase })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RustSymbolKindV1 {
    Function,
    Method,
    Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RustSymbolAnchorV1 {
    descriptor: &'static str,
    language: &'static str,
    symbol_kind: RustSymbolKindV1,
    signature_shape_hash: ContentHash,
    normalized_body_hash: ContentHash,
}

impl RustSymbolAnchorV1 {
    pub fn new(
        symbol_kind: RustSymbolKindV1,
        signature_shape_hash: ContentHash,
        normalized_body_hash: ContentHash,
    ) -> M6Result<Self> {
        full_sha256("signature_shape_hash", &signature_shape_hash)?;
        full_sha256("normalized_body_hash", &normalized_body_hash)?;
        Ok(Self {
            descriptor: RUST_SYMBOL_ANCHOR_V1,
            language: "rust",
            symbol_kind,
            signature_shape_hash,
            normalized_body_hash,
        })
    }

    #[must_use]
    pub fn symbol_kind(&self) -> RustSymbolKindV1 {
        self.symbol_kind
    }

    #[must_use]
    pub fn signature_shape_hash(&self) -> &ContentHash {
        &self.signature_shape_hash
    }

    #[must_use]
    pub fn normalized_body_hash(&self) -> &ContentHash {
        &self.normalized_body_hash
    }

    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramObjectKindV5 {
    Repository,
    Snapshot,
    Artifact,
    Relation,
    Context,
    Invariant,
    Limitation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingStatusV5 {
    Preserved,
    Modified,
    Added,
    Removed,
    Split,
    Merged,
    Unresolved,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKeyKindV5 {
    RepositoryIdentity,
    SnapshotPair,
    SamePath,
    GitRenameSameContent,
    RustSymbolAnchorV1,
    SameKindLabelLanguageLocation,
    MappedDirectedEndpoints,
    MappedMembers,
    MappedScope,
    MappedLimitationSources,
    NoCandidate,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MappingStatusCountsV5 {
    pub preserved: u64,
    pub modified: u64,
    pub added: u64,
    pub removed: u64,
    pub split: u64,
    pub merged: u64,
    pub unresolved: u64,
}

impl MappingStatusCountsV5 {
    fn record(&mut self, status: MappingStatusV5) {
        match status {
            MappingStatusV5::Preserved => self.preserved += 1,
            MappingStatusV5::Modified => self.modified += 1,
            MappingStatusV5::Added => self.added += 1,
            MappingStatusV5::Removed => self.removed += 1,
            MappingStatusV5::Split => self.split += 1,
            MappingStatusV5::Merged => self.merged += 1,
            MappingStatusV5::Unresolved => self.unresolved += 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProgramMappingPartsV5 {
    source_closure_id: StableId,
    source_snapshot_id: StableId,
    target_snapshot_id: StableId,
    object_kind: ProgramObjectKindV5,
    from_ids: BTreeSet<StableId>,
    to_ids: BTreeSet<StableId>,
    status: MappingStatusV5,
    candidate_key_kind: CandidateKeyKindV5,
    source_body_hashes: Vec<IdBodyHashV5>,
    target_body_hashes: Vec<IdBodyHashV5>,
    change_fact_ids: BTreeSet<StableId>,
    predecessor_mapping_ids: BTreeSet<StableId>,
}

#[derive(Serialize)]
struct ProgramMappingIdentityV5<'a> {
    source_closure_id: &'a StableId,
    source_snapshot_id: &'a StableId,
    target_snapshot_id: &'a StableId,
    object_kind: ProgramObjectKindV5,
    from_ids: &'a BTreeSet<StableId>,
    to_ids: &'a BTreeSet<StableId>,
    status: MappingStatusV5,
    candidate_key_kind: CandidateKeyKindV5,
    source_body_hashes: &'a Vec<IdBodyHashV5>,
    target_body_hashes: &'a Vec<IdBodyHashV5>,
    change_fact_ids: &'a BTreeSet<StableId>,
    predecessor_mapping_ids: &'a BTreeSet<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProgramMappingV5 {
    schema: &'static str,
    id: StableId,
    source_closure_id: StableId,
    source_snapshot_id: StableId,
    target_snapshot_id: StableId,
    object_kind: ProgramObjectKindV5,
    from_ids: BTreeSet<StableId>,
    to_ids: BTreeSet<StableId>,
    status: MappingStatusV5,
    candidate_key_kind: CandidateKeyKindV5,
    source_body_hashes: Vec<IdBodyHashV5>,
    target_body_hashes: Vec<IdBodyHashV5>,
    change_fact_ids: BTreeSet<StableId>,
    predecessor_mapping_ids: BTreeSet<StableId>,
    successor_ids: BTreeSet<StableId>,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramMappingWireV5 {
    schema: String,
    id: StableId,
    source_closure_id: StableId,
    source_snapshot_id: StableId,
    target_snapshot_id: StableId,
    object_kind: ProgramObjectKindV5,
    from_ids: BTreeSet<StableId>,
    to_ids: BTreeSet<StableId>,
    status: MappingStatusV5,
    candidate_key_kind: CandidateKeyKindV5,
    source_body_hashes: Vec<IdBodyHashV5>,
    target_body_hashes: Vec<IdBodyHashV5>,
    change_fact_ids: BTreeSet<StableId>,
    predecessor_mapping_ids: BTreeSet<StableId>,
    successor_ids: BTreeSet<StableId>,
    source_ids: BTreeSet<StableId>,
}

impl ProgramMappingV5 {
    fn allocated_bytes(&self) -> usize {
        fn records(values: &Vec<IdBodyHashV5>) -> usize {
            values
                .capacity()
                .saturating_mul(std::mem::size_of::<IdBodyHashV5>())
                .saturating_add(
                    values
                        .iter()
                        .map(|value| {
                            value
                                .id
                                .allocated_bytes()
                                .saturating_add(value.body_hash.allocated_bytes())
                        })
                        .sum::<usize>(),
                )
        }
        [
            std::mem::size_of::<Self>(),
            self.id.allocated_bytes(),
            self.source_closure_id.allocated_bytes(),
            self.source_snapshot_id.allocated_bytes(),
            self.target_snapshot_id.allocated_bytes(),
            id_set_heap(&self.from_ids),
            id_set_heap(&self.to_ids),
            records(&self.source_body_hashes),
            records(&self.target_body_hashes),
            id_set_heap(&self.change_fact_ids),
            id_set_heap(&self.predecessor_mapping_ids),
            id_set_heap(&self.successor_ids),
            id_set_heap(&self.source_ids),
        ]
        .into_iter()
        .fold(0_usize, usize::saturating_add)
    }

    fn from_parts(input: ProgramMappingPartsV5) -> M6Result<Self> {
        require_kind(
            &input.source_closure_id,
            "incremental-source-closure-v5",
            "source_closure_id",
        )?;
        require_kind(&input.source_snapshot_id, "snapshot", "source_snapshot_id")?;
        require_kind(&input.target_snapshot_id, "snapshot", "target_snapshot_id")?;
        bounded(
            input.from_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 mapping source side",
        )?;
        bounded(
            input.to_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 mapping target side",
        )?;
        bounded(
            input.change_fact_ids.len(),
            MAX_M6_MAPPING_LINK_IDS,
            "M6 mapping change facts",
        )?;
        bounded(
            input.predecessor_mapping_ids.len(),
            MAX_M6_MAPPING_LINK_IDS,
            "M6 mapping predecessors",
        )?;
        validate_component_shape(input.from_ids.len(), input.to_ids.len(), input.status)?;
        if matches!(
            input.status,
            MappingStatusV5::Added | MappingStatusV5::Removed
        ) != matches!(input.candidate_key_kind, CandidateKeyKindV5::NoCandidate)
        {
            return Err(M6Error::InvalidMapping(
                "no_candidate is required exactly for added/removed components",
            ));
        }
        if !candidate_key_matches_object(input.object_kind, input.candidate_key_kind) {
            return Err(M6Error::InvalidMapping(
                "candidate key is not valid for the mapped object kind",
            ));
        }
        validate_body_hash_records(
            "source_body_hashes",
            &input.from_ids,
            &input.source_body_hashes,
        )?;
        validate_body_hash_records(
            "target_body_hashes",
            &input.to_ids,
            &input.target_body_hashes,
        )?;

        let identity = ProgramMappingIdentityV5 {
            source_closure_id: &input.source_closure_id,
            source_snapshot_id: &input.source_snapshot_id,
            target_snapshot_id: &input.target_snapshot_id,
            object_kind: input.object_kind,
            from_ids: &input.from_ids,
            to_ids: &input.to_ids,
            status: input.status,
            candidate_key_kind: input.candidate_key_kind,
            source_body_hashes: &input.source_body_hashes,
            target_body_hashes: &input.target_body_hashes,
            change_fact_ids: &input.change_fact_ids,
            predecessor_mapping_ids: &input.predecessor_mapping_ids,
        };
        let id = derive("program-mapping-v5", &identity)?;
        let successor_ids = if input.from_ids.is_empty() {
            BTreeSet::new()
        } else {
            input.to_ids.clone()
        };
        let source_ids = std::iter::once(input.source_closure_id.clone())
            .chain(input.from_ids.iter().cloned())
            .chain(input.to_ids.iter().cloned())
            .chain(input.change_fact_ids.iter().cloned())
            .chain(input.predecessor_mapping_ids.iter().cloned())
            .collect();
        Ok(Self {
            schema: "reviewgraphen.program_mapping.v5",
            id,
            source_closure_id: input.source_closure_id,
            source_snapshot_id: input.source_snapshot_id,
            target_snapshot_id: input.target_snapshot_id,
            object_kind: input.object_kind,
            from_ids: input.from_ids,
            to_ids: input.to_ids,
            status: input.status,
            candidate_key_kind: input.candidate_key_kind,
            source_body_hashes: input.source_body_hashes,
            target_body_hashes: input.target_body_hashes,
            change_fact_ids: input.change_fact_ids,
            predecessor_mapping_ids: input.predecessor_mapping_ids,
            successor_ids,
            source_ids,
        })
        .and_then(|value| {
            bounded_event_dto(&value, MAX_M6_MAPPING_DTO_BYTES, "M6 mapping DTO bytes")?;
            Ok(value)
        })
    }

    pub(crate) fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_MAPPING_DTO_BYTES,
            "M6 mapping JSON bytes",
        )?;
        let wire: ProgramMappingWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.program_mapping.v5" {
            return Err(M6Error::InvalidWire("wrong mapping schema".to_owned()));
        }
        let expected = Self::from_parts(ProgramMappingPartsV5 {
            source_closure_id: wire.source_closure_id,
            source_snapshot_id: wire.source_snapshot_id,
            target_snapshot_id: wire.target_snapshot_id,
            object_kind: wire.object_kind,
            from_ids: wire.from_ids,
            to_ids: wire.to_ids,
            status: wire.status,
            candidate_key_kind: wire.candidate_key_kind,
            source_body_hashes: wire.source_body_hashes,
            target_body_hashes: wire.target_body_hashes,
            change_fact_ids: wire.change_fact_ids,
            predecessor_mapping_ids: wire.predecessor_mapping_ids,
        })?;
        if expected.id != wire.id
            || expected.successor_ids != wire.successor_ids
            || expected.source_ids != wire.source_ids
            || crate::canonical_json(&expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "mapping wire is not exact canonical derived content".to_owned(),
            ));
        }
        Ok(expected)
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn source_closure_id(&self) -> &StableId {
        &self.source_closure_id
    }
    #[must_use]
    pub fn source_snapshot_id(&self) -> &StableId {
        &self.source_snapshot_id
    }
    #[must_use]
    pub fn target_snapshot_id(&self) -> &StableId {
        &self.target_snapshot_id
    }
    #[must_use]
    pub fn from_ids(&self) -> &BTreeSet<StableId> {
        &self.from_ids
    }
    #[must_use]
    pub fn to_ids(&self) -> &BTreeSet<StableId> {
        &self.to_ids
    }
    #[must_use]
    pub fn status(&self) -> MappingStatusV5 {
        self.status
    }
    #[must_use]
    pub fn object_kind(&self) -> ProgramObjectKindV5 {
        self.object_kind
    }
    #[must_use]
    pub fn candidate_key_kind(&self) -> CandidateKeyKindV5 {
        self.candidate_key_kind
    }
    #[must_use]
    pub fn change_fact_ids(&self) -> &BTreeSet<StableId> {
        &self.change_fact_ids
    }
    #[must_use]
    pub fn predecessor_mapping_ids(&self) -> &BTreeSet<StableId> {
        &self.predecessor_mapping_ids
    }
    #[must_use]
    pub fn successor_ids(&self) -> &BTreeSet<StableId> {
        &self.successor_ids
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

fn preflight_event_line(payload_bytes: usize, envelope_bytes: usize) -> M6Result<usize> {
    let observed = payload_bytes
        .checked_add(envelope_bytes)
        .ok_or(M6Error::Incomplete {
            operation: "M6 event-line bytes",
            limit: MAX_M6_EVENT_LINE_BYTES,
            observed: usize::MAX,
        })?;
    bounded(observed, MAX_M6_EVENT_LINE_BYTES, "M6 event-line bytes")?;
    Ok(observed)
}

fn validate_component_shape(from: usize, to: usize, status: MappingStatusV5) -> M6Result<()> {
    let valid = match (from, to, status) {
        (0, 1, MappingStatusV5::Added) | (1, 0, MappingStatusV5::Removed) => true,
        (
            1,
            1,
            MappingStatusV5::Preserved | MappingStatusV5::Modified | MappingStatusV5::Unresolved,
        ) => true,
        (1, target, MappingStatusV5::Split) if target > 1 => true,
        (source, 1, MappingStatusV5::Merged) if source > 1 => true,
        (source, target, MappingStatusV5::Unresolved) if source > 1 && target > 1 => true,
        _ => false,
    };
    if !valid {
        return Err(M6Error::InvalidMapping(
            "status does not match the exclusive component cardinality",
        ));
    }
    Ok(())
}

fn candidate_key_matches_object(
    object_kind: ProgramObjectKindV5,
    candidate_key_kind: CandidateKeyKindV5,
) -> bool {
    match candidate_key_kind {
        CandidateKeyKindV5::NoCandidate => true,
        CandidateKeyKindV5::RepositoryIdentity => object_kind == ProgramObjectKindV5::Repository,
        CandidateKeyKindV5::SnapshotPair => object_kind == ProgramObjectKindV5::Snapshot,
        CandidateKeyKindV5::SamePath
        | CandidateKeyKindV5::GitRenameSameContent
        | CandidateKeyKindV5::RustSymbolAnchorV1
        | CandidateKeyKindV5::SameKindLabelLanguageLocation => {
            object_kind == ProgramObjectKindV5::Artifact
        }
        CandidateKeyKindV5::MappedDirectedEndpoints => object_kind == ProgramObjectKindV5::Relation,
        CandidateKeyKindV5::MappedMembers => object_kind == ProgramObjectKindV5::Context,
        CandidateKeyKindV5::MappedScope => object_kind == ProgramObjectKindV5::Invariant,
        CandidateKeyKindV5::MappedLimitationSources => {
            object_kind == ProgramObjectKindV5::Limitation
        }
    }
}

fn validate_body_hash_records(
    field: &'static str,
    ids: &BTreeSet<StableId>,
    hashes: &[IdBodyHashV5],
) -> M6Result<()> {
    if hashes.windows(2).any(|pair| pair[0].id >= pair[1].id)
        || hashes.iter().map(|record| &record.id).ne(ids.iter())
    {
        return Err(M6Error::InvalidMapping(
            "body-hash records must be sorted, unique, and exactly equal component IDs",
        ));
    }
    for record in hashes {
        full_sha256(field, &record.body_hash)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GitChangeKindV5 {
    Renamed,
    Copied,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitChangeFactV5 {
    fact_id: StableId,
    kind: GitChangeKindV5,
    source_artifact_id: StableId,
    target_artifact_id: StableId,
    source_path: String,
    target_path: String,
    equal_content_hash: Option<ContentHash>,
}

impl GitChangeFactV5 {
    fn from_accepted_artifact(
        fact: &crate::Artifact,
        source: &ProgramSpace,
        target: &ProgramSpace,
    ) -> M6Result<Option<Self>> {
        if fact.kind != "custom" || fact.provenance.extraction_method() != GIT_CHANGE_PROVENANCE_V1
        {
            return Ok(None);
        }
        let string_attribute = |name| {
            fact.attributes
                .get(name)
                .and_then(serde_json::Value::as_str)
        };
        let kind = match string_attribute("change_kind") {
            Some("renamed") => GitChangeKindV5::Renamed,
            Some("copied") => GitChangeKindV5::Copied,
            _ => return Ok(None),
        };
        let source_path = string_attribute("base_path").ok_or(M6Error::InvalidMapping(
            "accepted Git change fact is missing base_path",
        ))?;
        let target_path = string_attribute("target_path").ok_or(M6Error::InvalidMapping(
            "accepted Git change fact is missing target_path",
        ))?;
        if source_path == target_path
            || !normalized_mapping_path(source_path)
            || !normalized_mapping_path(target_path)
        {
            return Err(M6Error::InvalidMapping(
                "accepted Git change paths must be distinct normalized paths",
            ));
        }
        let source_file = unique_file_at_path(source, source_path)?;
        let target_file = unique_file_at_path(target, target_path)?;
        let equal_content_hash = match (&source_file.content_hash, &target_file.content_hash) {
            (Some(left), Some(right)) if left == right => Some(left.clone()),
            _ => None,
        };
        Ok(Some(Self {
            fact_id: fact.id.clone(),
            kind,
            source_artifact_id: source_file.id.clone(),
            target_artifact_id: target_file.id.clone(),
            source_path: source_path.to_owned(),
            target_path: target_path.to_owned(),
            equal_content_hash,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    // Unit-level algorithm fixture; production uses `from_accepted_artifact`.
    #[cfg(test)]
    fn fixture_from_parts(
        fact_id: StableId,
        kind: GitChangeKindV5,
        source_artifact_id: StableId,
        target_artifact_id: StableId,
        source_path: String,
        target_path: String,
        content_hash: ContentHash,
        provenance_descriptor_id: &str,
    ) -> M6Result<Self> {
        full_sha256("git change content_hash", &content_hash)?;
        if fact_id.kind() != "git-change-fact-v5" {
            return Err(M6Error::InvalidMapping(
                "Git change fact ID must use git-change-fact-v5",
            ));
        }
        if provenance_descriptor_id != GIT_CHANGE_PROVENANCE_V1 {
            return Err(M6Error::InvalidMapping(
                "Git change fact has untrusted extractor provenance",
            ));
        }
        if source_path == target_path
            || !normalized_mapping_path(&source_path)
            || !normalized_mapping_path(&target_path)
        {
            return Err(M6Error::InvalidMapping(
                "Git change paths must be distinct nonempty normalized paths",
            ));
        }
        Ok(Self {
            fact_id,
            kind,
            source_artifact_id,
            target_artifact_id,
            source_path,
            target_path,
            equal_content_hash: Some(content_hash),
        })
    }
}

fn normalized_mapping_path(path: &str) -> bool {
    !(path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || (path.len() >= 2
            && path.as_bytes()[0].is_ascii_alphabetic()
            && path.as_bytes()[1] == b':'))
        && path
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

fn unique_file_at_path<'a>(space: &'a ProgramSpace, path: &str) -> M6Result<&'a crate::Artifact> {
    let mut files = space.artifacts().iter().filter(|artifact| {
        artifact.kind == "file"
            && artifact
                .location
                .as_ref()
                .map(|location| location.path.as_str())
                == Some(path)
    });
    let file = files.next().ok_or(M6Error::InvalidMapping(
        "accepted Git change path has no file artifact",
    ))?;
    if files.next().is_some() {
        return Err(M6Error::InvalidMapping(
            "accepted Git change path has multiple file artifacts",
        ));
    }
    Ok(file)
}

#[derive(Clone, Debug)]
pub(crate) struct ValidatedIncrementalInputsV5 {
    source_snapshot_id: StableId,
    target_snapshot_id: StableId,
    rust_source_anchors: BTreeMap<StableId, RustSymbolAnchorV1>,
    rust_target_anchors: BTreeMap<StableId, RustSymbolAnchorV1>,
    source_relation_target_order: BTreeMap<StableId, Vec<StableId>>,
    target_relation_target_order: BTreeMap<StableId, Vec<StableId>>,
    git_change_facts: Vec<GitChangeFactV5>,
}

impl ValidatedIncrementalInputsV5 {
    fn derive_from_accepted_program_facts(
        source: &ProgramSpace,
        target: &ProgramSpace,
    ) -> M6Result<Self> {
        if source.extractor_set_hash() != target.extractor_set_hash() {
            return Err(M6Error::InvalidMapping(
                "source and target extractor sets must be identical",
            ));
        }
        let source_anchors = source.accepted_rust_symbol_anchors().ok_or_else(|| {
            M6Error::MissingAcceptedMappingFact {
                kind: "incremental Rust anchor set",
                object_id: source.snapshot_id().clone(),
            }
        })?;
        let target_anchors = target.accepted_rust_symbol_anchors().ok_or_else(|| {
            M6Error::MissingAcceptedMappingFact {
                kind: "incremental Rust anchor set",
                object_id: target.snapshot_id().clone(),
            }
        })?;
        let source_relation_order = source.accepted_relation_target_order().ok_or_else(|| {
            M6Error::MissingAcceptedMappingFact {
                kind: "incremental ordered relation set",
                object_id: source.snapshot_id().clone(),
            }
        })?;
        let target_relation_order = target.accepted_relation_target_order().ok_or_else(|| {
            M6Error::MissingAcceptedMappingFact {
                kind: "incremental ordered relation set",
                object_id: target.snapshot_id().clone(),
            }
        })?;
        let mut git_change_facts = target
            .artifacts()
            .iter()
            .filter_map(|artifact| {
                GitChangeFactV5::from_accepted_artifact(artifact, source, target).transpose()
            })
            .collect::<M6Result<Vec<_>>>()?;
        git_change_facts.sort_by(|left, right| left.fact_id.cmp(&right.fact_id));
        if git_change_facts
            .windows(2)
            .any(|pair| pair[0].fact_id == pair[1].fact_id)
        {
            return Err(M6Error::InvalidMapping("duplicate Git change fact ID"));
        }
        let mut seen_pairs = BTreeSet::new();
        let mut renamed_sources = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for fact in &git_change_facts {
            if !seen_pairs.insert((
                fact.source_artifact_id.clone(),
                fact.target_artifact_id.clone(),
            )) || !targets.insert(fact.target_artifact_id.clone())
                || (fact.kind == GitChangeKindV5::Renamed
                    && !renamed_sources.insert(fact.source_artifact_id.clone()))
            {
                return Err(M6Error::InvalidMapping(
                    "accepted Git change facts violate exact pair/source/target ownership",
                ));
            }
        }
        Ok(Self {
            source_snapshot_id: source.snapshot_id().clone(),
            target_snapshot_id: target.snapshot_id().clone(),
            rust_source_anchors: source_anchors.clone(),
            rust_target_anchors: target_anchors.clone(),
            source_relation_target_order: source_relation_order,
            target_relation_target_order: target_relation_order,
            git_change_facts,
        })
    }

    // Unit-level algorithm fixture; production always uses
    // `derive_from_accepted_program_facts` above.
    #[cfg(test)]
    fn fixture_from_parts(
        source: &ProgramSpace,
        target: &ProgramSpace,
        rust_source_anchors: BTreeMap<StableId, RustSymbolAnchorV1>,
        rust_target_anchors: BTreeMap<StableId, RustSymbolAnchorV1>,
        source_relation_target_order: BTreeMap<StableId, Vec<StableId>>,
        target_relation_target_order: BTreeMap<StableId, Vec<StableId>>,
        mut git_change_facts: Vec<GitChangeFactV5>,
    ) -> M6Result<Self> {
        if source.extractor_set_hash() != target.extractor_set_hash() {
            return Err(M6Error::InvalidMapping(
                "source and target extractor sets must be identical",
            ));
        }
        validate_anchor_domain(source, &rust_source_anchors)?;
        validate_anchor_domain(target, &rust_target_anchors)?;
        validate_relation_orders(source, &source_relation_target_order)?;
        validate_relation_orders(target, &target_relation_target_order)?;
        git_change_facts.sort_by(|left, right| left.fact_id.cmp(&right.fact_id));
        if git_change_facts
            .windows(2)
            .any(|pair| pair[0].fact_id == pair[1].fact_id)
        {
            return Err(M6Error::InvalidMapping("duplicate Git change fact ID"));
        }
        let mut seen_pairs = BTreeSet::new();
        let mut renamed_sources = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for fact in &git_change_facts {
            if !seen_pairs.insert((
                fact.source_artifact_id.clone(),
                fact.target_artifact_id.clone(),
            )) || !targets.insert(fact.target_artifact_id.clone())
                || (fact.kind == GitChangeKindV5::Renamed
                    && !renamed_sources.insert(fact.source_artifact_id.clone()))
            {
                return Err(M6Error::InvalidMapping(
                    "Git change facts violate exact pair/source/target ownership",
                ));
            }
        }
        let source_artifacts = source
            .artifacts()
            .iter()
            .map(|artifact| (&artifact.id, artifact))
            .collect::<BTreeMap<_, _>>();
        let target_artifacts = target
            .artifacts()
            .iter()
            .map(|artifact| (&artifact.id, artifact))
            .collect::<BTreeMap<_, _>>();
        for fact in &git_change_facts {
            let source_artifact = source_artifacts
                .get(&fact.source_artifact_id)
                .ok_or(M6Error::InvalidMapping("Git change source is outside S0"))?;
            let target_artifact = target_artifacts
                .get(&fact.target_artifact_id)
                .ok_or(M6Error::InvalidMapping("Git change target is outside S1"))?;
            let source_path = source_artifact
                .location
                .as_ref()
                .map(|value| value.path.as_str());
            let target_path = target_artifact
                .location
                .as_ref()
                .map(|value| value.path.as_str());
            if source_artifact.kind != "file"
                || target_artifact.kind != "file"
                || source_path != Some(fact.source_path.as_str())
                || target_path != Some(fact.target_path.as_str())
                || source_artifact.content_hash.as_ref() != fact.equal_content_hash.as_ref()
                || target_artifact.content_hash.as_ref() != fact.equal_content_hash.as_ref()
            {
                return Err(M6Error::InvalidMapping(
                    "Git change fact must bind exact equal-content file artifacts",
                ));
            }
        }
        Ok(Self {
            source_snapshot_id: source.snapshot_id().clone(),
            target_snapshot_id: target.snapshot_id().clone(),
            rust_source_anchors,
            rust_target_anchors,
            source_relation_target_order,
            target_relation_target_order,
            git_change_facts,
        })
    }
}

fn rust_symbol(artifact: &crate::Artifact) -> bool {
    artifact.language.as_deref() == Some("rust")
        && matches!(artifact.kind.as_str(), "function" | "method" | "type")
}

fn validate_anchor_domain(
    space: &ProgramSpace,
    anchors: &BTreeMap<StableId, RustSymbolAnchorV1>,
) -> M6Result<()> {
    let expected = space
        .artifacts()
        .iter()
        .filter(|artifact| rust_symbol(artifact))
        .map(|artifact| artifact.id.clone())
        .collect::<BTreeSet<_>>();
    if anchors.keys().ne(expected.iter()) {
        return Err(M6Error::InvalidMapping(
            "Rust anchor sidecar must exactly cover every Rust symbol",
        ));
    }
    Ok(())
}

fn validate_relation_orders(
    space: &ProgramSpace,
    orders: &BTreeMap<StableId, Vec<StableId>>,
) -> M6Result<()> {
    if orders.len() != space.relations().len() {
        return Err(M6Error::InvalidMapping(
            "ordered endpoint sidecar must cover every relation",
        ));
    }
    for relation in space.relations() {
        let ordered = orders.get(&relation.id).ok_or(M6Error::InvalidMapping(
            "ordered endpoint sidecar is missing a relation",
        ))?;
        let unique = ordered.iter().cloned().collect::<BTreeSet<_>>();
        if unique.len() != ordered.len() || unique != relation.target_ids {
            return Err(M6Error::InvalidMapping(
                "ordered endpoints must be a duplicate-free permutation of target_ids",
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
enum NodeDataV5 {
    Repository {
        identity: String,
    },
    Snapshot,
    Artifact {
        kind: String,
        label: String,
        language: Option<String>,
        path: Option<String>,
        location: Option<Box<crate::Location>>,
        content_hash: Option<ContentHash>,
        anchor: Option<RustSymbolAnchorV1>,
        base_hash: ContentHash,
        provenance: NormalizedProvenanceV5,
        change_fact_ids: BTreeSet<StableId>,
        symbol_ref: Option<StableId>,
    },
    Relation {
        kind: String,
        directed: bool,
        source_id: StableId,
        ordered_target_ids: Vec<StableId>,
        base_hash: ContentHash,
        provenance: NormalizedProvenanceV5,
    },
    Context {
        kind: String,
        members: BTreeSet<StableId>,
        base_hash: ContentHash,
        provenance: NormalizedProvenanceV5,
    },
    Invariant {
        property_id: String,
        scope: BTreeSet<StableId>,
        base_hash: ContentHash,
        provenance: NormalizedProvenanceV5,
    },
    Limitation {
        description: String,
        kind: crate::LimitationKind,
        severity: crate::Severity,
        sources: BTreeSet<StableId>,
        base_hash: ContentHash,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "scope", content = "value")]
enum NormalizedRevisionRefV5 {
    Absent,
    TargetSnapshot,
    Exact(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "scope", content = "value")]
enum NormalizedContentRefV5 {
    Absent,
    TargetTree,
    SelfArtifact,
    ProgramArtifact(StableId),
    Exact(ContentHash),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "scope", content = "value")]
enum NormalizedLocalRefV5 {
    Absent,
    SelfArtifact,
    ProgramArtifact(StableId),
    Exact(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct NormalizedProvenanceV5 {
    stable_hash: ContentHash,
    revision: NormalizedRevisionRefV5,
    content: NormalizedContentRefV5,
    local: NormalizedLocalRefV5,
}

impl NormalizedProvenanceV5 {
    fn allocated_bytes(&self) -> usize {
        fn revision(value: &NormalizedRevisionRefV5) -> usize {
            match value {
                NormalizedRevisionRefV5::Exact(value) => value.capacity(),
                NormalizedRevisionRefV5::Absent | NormalizedRevisionRefV5::TargetSnapshot => 0,
            }
        }
        fn content(value: &NormalizedContentRefV5) -> usize {
            match value {
                NormalizedContentRefV5::ProgramArtifact(id) => id.allocated_bytes(),
                NormalizedContentRefV5::Exact(hash) => hash.allocated_bytes(),
                NormalizedContentRefV5::Absent
                | NormalizedContentRefV5::TargetTree
                | NormalizedContentRefV5::SelfArtifact => 0,
            }
        }
        fn local(value: &NormalizedLocalRefV5) -> usize {
            match value {
                NormalizedLocalRefV5::ProgramArtifact(id) => id.allocated_bytes(),
                NormalizedLocalRefV5::Exact(value) => value.capacity(),
                NormalizedLocalRefV5::Absent | NormalizedLocalRefV5::SelfArtifact => 0,
            }
        }
        self.stable_hash
            .allocated_bytes()
            .saturating_add(revision(&self.revision))
            .saturating_add(content(&self.content))
            .saturating_add(local(&self.local))
    }
}

#[derive(Clone, Debug)]
struct SnapshotPathArtifactV5 {
    id: StableId,
    content_hash: Option<ContentHash>,
}

#[derive(Serialize)]
struct ProvenanceStableBodyV5<'a> {
    confidence: Option<f64>,
    extraction_method: &'a str,
    review_status: crate::ReviewStatus,
    source_kind: &'a str,
    source_locator: &'a str,
    tool_version: Option<&'a str>,
}

fn snapshot_path_artifacts(space: &ProgramSpace) -> BTreeMap<String, SnapshotPathArtifactV5> {
    let mut paths = BTreeMap::<String, Option<SnapshotPathArtifactV5>>::new();
    for artifact in space
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
    {
        let Some(location) = &artifact.location else {
            continue;
        };
        let value = SnapshotPathArtifactV5 {
            id: artifact.id.clone(),
            content_hash: artifact.content_hash.clone(),
        };
        paths
            .entry(location.path.clone())
            .and_modify(|entry| *entry = None)
            .or_insert(Some(value));
    }
    paths
        .into_iter()
        .filter_map(|(path, value)| value.map(|value| (path, value)))
        .collect()
}

fn normalize_provenance(
    space: &ProgramSpace,
    provenance: &crate::Provenance,
    self_path: Option<&str>,
    self_content_hash: Option<&ContentHash>,
    paths: &BTreeMap<String, SnapshotPathArtifactV5>,
) -> M6Result<NormalizedProvenanceV5> {
    let source = provenance.source();
    // Snapshot roles are recognized only for this exact Git repository. An
    // external source that happens to reuse an OID/hash remains semantically
    // exact and therefore cannot be laundered by replacement.
    let is_snapshot_git = source.kind() == "git" && source.locator() == space.repository_identity();
    let revision = match source.revision() {
        None => NormalizedRevisionRefV5::Absent,
        Some(value) if is_snapshot_git && value == space.target_revision() => {
            NormalizedRevisionRefV5::TargetSnapshot
        }
        Some(value) => NormalizedRevisionRefV5::Exact(value.to_owned()),
    };
    let local_path = source.source_local_id();
    let referenced_artifact = local_path.and_then(|path| paths.get(path));
    let content = match source.content_hash() {
        None => NormalizedContentRefV5::Absent,
        Some(value) if is_snapshot_git && value == space.incremental_tree_hash_for_store() => {
            NormalizedContentRefV5::TargetTree
        }
        Some(value)
            if is_snapshot_git
                && self_path == local_path
                && self_content_hash.is_some_and(|hash| hash == value) =>
        {
            NormalizedContentRefV5::SelfArtifact
        }
        Some(value)
            if is_snapshot_git
                && referenced_artifact
                    .and_then(|artifact| artifact.content_hash.as_ref())
                    .is_some_and(|hash| hash == value) =>
        {
            NormalizedContentRefV5::ProgramArtifact(referenced_artifact.unwrap().id.clone())
        }
        Some(value) => NormalizedContentRefV5::Exact(value.clone()),
    };
    let local = match local_path {
        None => NormalizedLocalRefV5::Absent,
        Some(_) if is_snapshot_git && self_path == local_path => NormalizedLocalRefV5::SelfArtifact,
        Some(_) if is_snapshot_git && referenced_artifact.is_some() => {
            NormalizedLocalRefV5::ProgramArtifact(referenced_artifact.unwrap().id.clone())
        }
        Some(value) => NormalizedLocalRefV5::Exact(value.to_owned()),
    };
    Ok(NormalizedProvenanceV5 {
        stable_hash: body_hash(&ProvenanceStableBodyV5 {
            confidence: provenance.confidence_for_mapping(),
            extraction_method: provenance.extraction_method(),
            review_status: provenance.review_status(),
            source_kind: source.kind(),
            source_locator: source.locator(),
            tool_version: provenance.tool_version_for_mapping(),
        })?,
        revision,
        content,
        local,
    })
}

fn normalized_program_ref_equal(
    source: &StableId,
    target: &StableId,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> bool {
    successors
        .get(source)
        .is_some_and(|ids| ids.len() == 1 && ids.contains(target))
}

fn normalized_provenance_equal(
    source: &NormalizedProvenanceV5,
    target: &NormalizedProvenanceV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> bool {
    if source.stable_hash != target.stable_hash || source.revision != target.revision {
        return false;
    }
    let content_equal = match (&source.content, &target.content) {
        (NormalizedContentRefV5::Absent, NormalizedContentRefV5::Absent)
        | (NormalizedContentRefV5::TargetTree, NormalizedContentRefV5::TargetTree)
        | (NormalizedContentRefV5::SelfArtifact, NormalizedContentRefV5::SelfArtifact) => true,
        (
            NormalizedContentRefV5::ProgramArtifact(source),
            NormalizedContentRefV5::ProgramArtifact(target),
        ) => normalized_program_ref_equal(source, target, successors),
        (NormalizedContentRefV5::Exact(source), NormalizedContentRefV5::Exact(target)) => {
            source == target
        }
        _ => false,
    };
    let local_equal = match (&source.local, &target.local) {
        (NormalizedLocalRefV5::Absent, NormalizedLocalRefV5::Absent)
        | (NormalizedLocalRefV5::SelfArtifact, NormalizedLocalRefV5::SelfArtifact) => true,
        (
            NormalizedLocalRefV5::ProgramArtifact(source),
            NormalizedLocalRefV5::ProgramArtifact(target),
        ) => normalized_program_ref_equal(source, target, successors),
        (NormalizedLocalRefV5::Exact(source), NormalizedLocalRefV5::Exact(target)) => {
            source == target
        }
        _ => false,
    };
    content_equal && local_equal
}

#[derive(Serialize)]
struct NormalizedProvenanceBodyV5<'a> {
    content: NormalizedScopedRefBodyV5<'a>,
    local: NormalizedScopedRefBodyV5<'a>,
    revision: &'a NormalizedRevisionRefV5,
    stable_hash: &'a ContentHash,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "scope", content = "value")]
enum NormalizedScopedRefBodyV5<'a> {
    Absent,
    TargetTree,
    SelfArtifact,
    ProgramTargets(BTreeSet<StableId>),
    ExactHash(&'a ContentHash),
    ExactString(&'a str),
}

fn normalized_provenance_body_hash(
    provenance: &NormalizedProvenanceV5,
    side: ProgramSideV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> M6Result<ContentHash> {
    let content = match &provenance.content {
        NormalizedContentRefV5::Absent => NormalizedScopedRefBodyV5::Absent,
        NormalizedContentRefV5::TargetTree => NormalizedScopedRefBodyV5::TargetTree,
        NormalizedContentRefV5::SelfArtifact => NormalizedScopedRefBodyV5::SelfArtifact,
        NormalizedContentRefV5::ProgramArtifact(id) => {
            NormalizedScopedRefBodyV5::ProgramTargets(normalized_targets(id, side, successors))
        }
        NormalizedContentRefV5::Exact(hash) => NormalizedScopedRefBodyV5::ExactHash(hash),
    };
    let local = match &provenance.local {
        NormalizedLocalRefV5::Absent => NormalizedScopedRefBodyV5::Absent,
        NormalizedLocalRefV5::SelfArtifact => NormalizedScopedRefBodyV5::SelfArtifact,
        NormalizedLocalRefV5::ProgramArtifact(id) => {
            NormalizedScopedRefBodyV5::ProgramTargets(normalized_targets(id, side, successors))
        }
        NormalizedLocalRefV5::Exact(value) => NormalizedScopedRefBodyV5::ExactString(value),
    };
    body_hash(&NormalizedProvenanceBodyV5 {
        content,
        local,
        revision: &provenance.revision,
        stable_hash: &provenance.stable_hash,
    })
}

#[derive(Clone, Debug)]
struct ProgramNodeV5 {
    id: StableId,
    object_kind: ProgramObjectKindV5,
    body_hash: ContentHash,
    data: NodeDataV5,
}

fn id_set_heap(ids: &BTreeSet<StableId>) -> usize {
    ids.len()
        .saturating_mul(std::mem::size_of::<StableId>())
        .saturating_add(ids.iter().map(StableId::allocated_bytes).sum::<usize>())
}

fn id_vec_heap(ids: &Vec<StableId>) -> usize {
    ids.capacity()
        .saturating_mul(std::mem::size_of::<StableId>())
        .saturating_add(ids.iter().map(StableId::allocated_bytes).sum::<usize>())
}

impl ProgramNodeV5 {
    fn allocated_bytes(&self) -> usize {
        let data = match &self.data {
            NodeDataV5::Repository { identity } => identity.capacity(),
            NodeDataV5::Snapshot => 0,
            NodeDataV5::Artifact {
                kind,
                label,
                language,
                path,
                location,
                content_hash,
                anchor,
                base_hash,
                provenance,
                change_fact_ids,
                symbol_ref,
            } => kind
                .capacity()
                .saturating_add(label.capacity())
                .saturating_add(language.as_ref().map_or(0, String::capacity))
                .saturating_add(path.as_ref().map_or(0, String::capacity))
                .saturating_add(location.as_ref().map_or(0, |location| {
                    std::mem::size_of::<crate::Location>()
                        + location.path.capacity()
                        + location
                            .symbol_id
                            .as_ref()
                            .map_or(0, StableId::allocated_bytes)
                }))
                .saturating_add(
                    content_hash
                        .as_ref()
                        .map_or(0, ContentHash::allocated_bytes),
                )
                .saturating_add(anchor.as_ref().map_or(0, |anchor| {
                    anchor.signature_shape_hash.allocated_bytes()
                        + anchor.normalized_body_hash.allocated_bytes()
                }))
                .saturating_add(base_hash.allocated_bytes())
                .saturating_add(provenance.allocated_bytes())
                .saturating_add(id_set_heap(change_fact_ids))
                .saturating_add(symbol_ref.as_ref().map_or(0, StableId::allocated_bytes)),
            NodeDataV5::Relation {
                kind,
                source_id,
                ordered_target_ids,
                base_hash,
                provenance,
                ..
            } => kind
                .capacity()
                .saturating_add(source_id.allocated_bytes())
                .saturating_add(id_vec_heap(ordered_target_ids))
                .saturating_add(base_hash.allocated_bytes())
                .saturating_add(provenance.allocated_bytes()),
            NodeDataV5::Context {
                kind,
                members,
                base_hash,
                provenance,
            } => kind
                .capacity()
                .saturating_add(id_set_heap(members))
                .saturating_add(base_hash.allocated_bytes())
                .saturating_add(provenance.allocated_bytes()),
            NodeDataV5::Invariant {
                property_id,
                scope,
                base_hash,
                provenance,
            } => property_id
                .capacity()
                .saturating_add(id_set_heap(scope))
                .saturating_add(base_hash.allocated_bytes())
                .saturating_add(provenance.allocated_bytes()),
            NodeDataV5::Limitation {
                description,
                sources,
                base_hash,
                ..
            } => description
                .capacity()
                .saturating_add(id_set_heap(sources))
                .saturating_add(base_hash.allocated_bytes()),
        };
        std::mem::size_of::<Self>()
            .saturating_add(self.id.allocated_bytes())
            .saturating_add(self.body_hash.allocated_bytes())
            .saturating_add(data)
    }
}

#[derive(Serialize)]
struct ArtifactNonReferenceBodyV5<'a> {
    attributes: &'a BTreeMap<String, serde_json::Value>,
    kind: &'a str,
    language: &'a Option<String>,
    provenance_stable_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct RelationNonReferenceBodyV5<'a> {
    attributes: &'a BTreeMap<String, serde_json::Value>,
    directed: bool,
    kind: &'a str,
    provenance_stable_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct ContextNonReferenceBodyV5<'a> {
    attributes: &'a BTreeMap<String, serde_json::Value>,
    kind: &'a str,
    label: &'a str,
    provenance_stable_hash: &'a ContentHash,
}

#[derive(Serialize)]
struct InvariantNonReferenceBodyV5<'a> {
    description: &'a str,
    property_id: &'a str,
    provenance_stable_hash: &'a ContentHash,
    severity: crate::Severity,
    verification_mode: &'a Option<String>,
}

#[derive(Serialize)]
struct LimitationNonReferenceBodyV5<'a> {
    description: &'a str,
    kind: crate::LimitationKind,
    related_capabilities: &'a BTreeSet<String>,
    severity: crate::Severity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProgramSideV5 {
    Source,
    Target,
}

#[derive(Serialize)]
struct ProgramSemanticBodyV5<'a> {
    accepted_program_body_hash: &'a ContentHash,
    change_fact_ids: &'a BTreeSet<StableId>,
    ordered_target_ids: Option<&'a Vec<StableId>>,
    rust_anchor: Option<&'a RustSymbolAnchorV1>,
}

fn semantic_program_body_hash(
    accepted_program_body_hash: &ContentHash,
    change_fact_ids: &BTreeSet<StableId>,
    ordered_target_ids: Option<&Vec<StableId>>,
    rust_anchor: Option<&RustSymbolAnchorV1>,
) -> M6Result<ContentHash> {
    body_hash(&ProgramSemanticBodyV5 {
        accepted_program_body_hash,
        change_fact_ids,
        ordered_target_ids,
        rust_anchor,
    })
}

fn program_nodes(
    space: &ProgramSpace,
    side: ProgramSideV5,
    anchors: &BTreeMap<StableId, RustSymbolAnchorV1>,
    relation_orders: &BTreeMap<StableId, Vec<StableId>>,
    git_change_facts: &[GitChangeFactV5],
) -> M6Result<(Vec<ProgramNodeV5>, usize)> {
    crate::canonical::canonical_json_count_bounded(
        &space.streaming_ref(),
        MAX_M6_MAPPING_WORKING_BYTES,
        "M6 ProgramSpace mapping preflight bytes",
    )?;
    let count = [
        2_usize,
        space.artifacts().len(),
        space.relations().len(),
        space.contexts().len(),
        space.invariants().len(),
        space.extraction().limitations.len(),
    ]
    .into_iter()
    .try_fold(0_usize, usize::checked_add)
    .ok_or(M6Error::Incomplete {
        operation: "M6 program domain",
        limit: MAX_M6_PROGRAM_DOMAIN_IDS,
        observed: usize::MAX,
    })?;
    bounded(count, MAX_M6_PROGRAM_DOMAIN_IDS, "M6 program domain")?;
    let mut nodes = Vec::new();
    nodes
        .try_reserve_exact(count)
        .map_err(|_| M6Error::Incomplete {
            operation: "M6 program node allocation",
            limit: MAX_M6_PROGRAM_DOMAIN_IDS,
            observed: usize::MAX,
        })?;
    nodes.push(ProgramNodeV5 {
        id: space.repository_id().clone(),
        object_kind: ProgramObjectKindV5::Repository,
        body_hash: space.m6_repository_body_hash()?,
        data: NodeDataV5::Repository {
            identity: space.repository_identity().to_owned(),
        },
    });
    nodes.push(ProgramNodeV5 {
        id: space.snapshot_id().clone(),
        object_kind: ProgramObjectKindV5::Snapshot,
        body_hash: space.m6_snapshot_body_hash()?,
        data: NodeDataV5::Snapshot,
    });
    let path_artifacts = snapshot_path_artifacts(space);
    for artifact in space.artifacts() {
        let accepted_body_hash = artifact.m6_body_hash()?;
        let change_fact_ids = git_change_facts
            .iter()
            .filter(|fact| match side {
                ProgramSideV5::Source => fact.source_artifact_id == artifact.id,
                ProgramSideV5::Target => fact.target_artifact_id == artifact.id,
            })
            .map(|fact| fact.fact_id.clone())
            .collect();
        let canonical_body_hash = semantic_program_body_hash(
            &accepted_body_hash,
            &change_fact_ids,
            None,
            anchors.get(&artifact.id),
        )?;
        let provenance = normalize_provenance(
            space,
            &artifact.provenance,
            artifact.location.as_ref().map(|value| value.path.as_str()),
            artifact.content_hash.as_ref(),
            &path_artifacts,
        )?;
        let base_hash = body_hash(&ArtifactNonReferenceBodyV5 {
            attributes: &artifact.attributes,
            kind: &artifact.kind,
            language: &artifact.language,
            provenance_stable_hash: &provenance.stable_hash,
        })?;
        nodes.push(ProgramNodeV5 {
            id: artifact.id.clone(),
            object_kind: ProgramObjectKindV5::Artifact,
            body_hash: canonical_body_hash,
            data: NodeDataV5::Artifact {
                kind: artifact.kind.clone(),
                label: artifact.label.clone(),
                language: artifact.language.clone(),
                path: artifact.location.as_ref().map(|value| value.path.clone()),
                location: artifact.location.clone().map(Box::new),
                content_hash: artifact.content_hash.clone(),
                anchor: anchors.get(&artifact.id).cloned(),
                base_hash,
                provenance,
                change_fact_ids,
                symbol_ref: artifact
                    .location
                    .as_ref()
                    .and_then(|value| value.symbol_id.clone()),
            },
        });
    }
    for relation in space.relations() {
        let accepted_body_hash = relation.m6_body_hash()?;
        let no_change_facts = BTreeSet::new();
        let provenance =
            normalize_provenance(space, &relation.provenance, None, None, &path_artifacts)?;
        nodes.push(ProgramNodeV5 {
            id: relation.id.clone(),
            object_kind: ProgramObjectKindV5::Relation,
            body_hash: semantic_program_body_hash(
                &accepted_body_hash,
                &no_change_facts,
                relation_orders.get(&relation.id),
                None,
            )?,
            data: NodeDataV5::Relation {
                kind: relation.kind.clone(),
                directed: relation.directed,
                source_id: relation.source_id.clone(),
                ordered_target_ids: relation_orders[&relation.id].clone(),
                base_hash: body_hash(&RelationNonReferenceBodyV5 {
                    attributes: &relation.attributes,
                    directed: relation.directed,
                    kind: &relation.kind,
                    provenance_stable_hash: &provenance.stable_hash,
                })?,
                provenance,
            },
        });
    }
    for context in space.contexts() {
        let provenance =
            normalize_provenance(space, &context.provenance, None, None, &path_artifacts)?;
        nodes.push(ProgramNodeV5 {
            id: context.id.clone(),
            object_kind: ProgramObjectKindV5::Context,
            body_hash: context.m6_body_hash()?,
            data: NodeDataV5::Context {
                kind: context.kind.clone(),
                members: context.member_ids.clone(),
                base_hash: body_hash(&ContextNonReferenceBodyV5 {
                    attributes: &context.attributes,
                    kind: &context.kind,
                    label: &context.label,
                    provenance_stable_hash: &provenance.stable_hash,
                })?,
                provenance,
            },
        });
    }
    for invariant in space.invariants() {
        let provenance =
            normalize_provenance(space, &invariant.provenance, None, None, &path_artifacts)?;
        nodes.push(ProgramNodeV5 {
            id: invariant.id.clone(),
            object_kind: ProgramObjectKindV5::Invariant,
            body_hash: invariant.m6_body_hash()?,
            data: NodeDataV5::Invariant {
                property_id: invariant.property_id.clone(),
                scope: invariant.scope_ids.clone(),
                base_hash: body_hash(&InvariantNonReferenceBodyV5 {
                    description: &invariant.description,
                    property_id: &invariant.property_id,
                    provenance_stable_hash: &provenance.stable_hash,
                    severity: invariant.severity,
                    verification_mode: &invariant.verification_mode,
                })?,
                provenance,
            },
        });
    }
    for limitation in &space.extraction().limitations {
        nodes.push(ProgramNodeV5 {
            id: limitation.id.clone(),
            object_kind: ProgramObjectKindV5::Limitation,
            body_hash: limitation.m6_body_hash()?,
            data: NodeDataV5::Limitation {
                description: limitation.description.clone(),
                kind: limitation.kind,
                severity: limitation.severity,
                sources: limitation.source_ids.clone(),
                base_hash: body_hash(&LimitationNonReferenceBodyV5 {
                    description: &limitation.description,
                    kind: limitation.kind,
                    related_capabilities: &limitation.related_capabilities,
                    severity: limitation.severity,
                })?,
            },
        });
    }
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    let retained_working = nodes.iter().try_fold(0_usize, |total, node| {
        checked_working_add(total, node.allocated_bytes())
    })?;
    Ok((nodes, retained_working))
}

#[derive(Clone, Debug)]
struct ComponentSeedV5 {
    object_kind: ProgramObjectKindV5,
    candidate_key_kind: CandidateKeyKindV5,
    from_ids: BTreeSet<StableId>,
    to_ids: BTreeSet<StableId>,
    stage: usize,
}

impl ComponentSeedV5 {
    fn allocated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(id_set_heap(&self.from_ids))
            .saturating_add(id_set_heap(&self.to_ids))
    }
}

#[allow(clippy::too_many_arguments)] // Each argument is one closed candidate-stage boundary.
fn collect_components<F>(
    source: &[&ProgramNodeV5],
    target: &[&ProgramNodeV5],
    consumed_source: &mut BTreeSet<StableId>,
    consumed_target: &mut BTreeSet<StableId>,
    object_kind: ProgramObjectKindV5,
    candidate_key_kind: CandidateKeyKindV5,
    stage: usize,
    base_working: usize,
    mut edge: F,
) -> M6Result<Vec<ComponentSeedV5>>
where
    F: FnMut(&ProgramNodeV5, &ProgramNodeV5) -> bool,
{
    let source = source
        .iter()
        .filter(|node| !consumed_source.contains(&node.id))
        .copied()
        .collect::<Vec<_>>();
    let target = target
        .iter()
        .filter(|node| !consumed_target.contains(&node.id))
        .copied()
        .collect::<Vec<_>>();
    let mut adjacency_source = vec![Vec::<usize>::new(); source.len()];
    let mut adjacency_target = vec![Vec::<usize>::new(); target.len()];
    let mut working = base_working;
    for (source_index, source_node) in source.iter().enumerate() {
        for (target_index, target_node) in target.iter().enumerate() {
            if edge(source_node, target_node) {
                bounded(
                    adjacency_source[source_index].len() + 1,
                    MAX_M6_MAPPING_SIDE_IDS,
                    "M6 candidate source degree",
                )?;
                bounded(
                    adjacency_target[target_index].len() + 1,
                    MAX_M6_MAPPING_SIDE_IDS,
                    "M6 candidate target degree",
                )?;
                working = checked_working_add(working, 2 * std::mem::size_of::<usize>())?;
                adjacency_source[source_index]
                    .try_reserve(1)
                    .map_err(|_| M6Error::Incomplete {
                        operation: "M6 candidate edge allocation",
                        limit: MAX_M6_MAPPING_WORKING_BYTES,
                        observed: usize::MAX,
                    })?;
                adjacency_target[target_index]
                    .try_reserve(1)
                    .map_err(|_| M6Error::Incomplete {
                        operation: "M6 candidate edge allocation",
                        limit: MAX_M6_MAPPING_WORKING_BYTES,
                        observed: usize::MAX,
                    })?;
                adjacency_source[source_index].push(target_index);
                adjacency_target[target_index].push(source_index);
            }
        }
    }
    let mut seen_source = BTreeSet::new();
    let mut seen_target = BTreeSet::new();
    let mut components = Vec::new();
    let mut max_queue_capacity = 0_usize;
    for start in 0..source.len() {
        if adjacency_source[start].is_empty() || seen_source.contains(&start) {
            continue;
        }
        let mut queue = VecDeque::from([(true, start)]);
        let mut from_ids = BTreeSet::new();
        let mut to_ids = BTreeSet::new();
        while let Some((is_source, index)) = queue.pop_front() {
            max_queue_capacity = max_queue_capacity.max(queue.capacity());
            if is_source {
                if !seen_source.insert(index) {
                    continue;
                }
                from_ids.insert(source[index].id.clone());
                queue.extend(adjacency_source[index].iter().map(|value| (false, *value)));
            } else {
                if !seen_target.insert(index) {
                    continue;
                }
                to_ids.insert(target[index].id.clone());
                queue.extend(adjacency_target[index].iter().map(|value| (true, *value)));
            }
        }
        bounded(
            from_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 mapping source component",
        )?;
        bounded(
            to_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 mapping target component",
        )?;
        consumed_source.extend(from_ids.iter().cloned());
        consumed_target.extend(to_ids.iter().cloned());
        components.push(ComponentSeedV5 {
            object_kind,
            candidate_key_kind,
            from_ids,
            to_ids,
            stage,
        });
    }
    components.sort_by(|left, right| {
        left.from_ids
            .iter()
            .next()
            .cmp(&right.from_ids.iter().next())
            .then_with(|| left.to_ids.iter().next().cmp(&right.to_ids.iter().next()))
    });
    let pointer_vectors = source.capacity() * std::mem::size_of::<&ProgramNodeV5>()
        + target.capacity() * std::mem::size_of::<&ProgramNodeV5>();
    let adjacency = adjacency_source.capacity() * std::mem::size_of::<Vec<usize>>()
        + adjacency_target.capacity() * std::mem::size_of::<Vec<usize>>()
        + adjacency_source
            .iter()
            .chain(&adjacency_target)
            .map(|values| values.capacity() * std::mem::size_of::<usize>())
            .sum::<usize>();
    let traversal = (seen_source.len() + seen_target.len()) * std::mem::size_of::<usize>()
        + max_queue_capacity * std::mem::size_of::<(bool, usize)>();
    let returned = components
        .iter()
        .map(ComponentSeedV5::allocated_bytes)
        .sum();
    [pointer_vectors, adjacency, traversal, returned]
        .into_iter()
        .try_fold(base_working, checked_working_add)?;
    Ok(components)
}

fn finish_unmatched(
    source: &[&ProgramNodeV5],
    target: &[&ProgramNodeV5],
    consumed_source: &mut BTreeSet<StableId>,
    consumed_target: &mut BTreeSet<StableId>,
    object_kind: ProgramObjectKindV5,
    stage: usize,
    components: &mut Vec<ComponentSeedV5>,
) {
    for node in source {
        if consumed_source.insert(node.id.clone()) {
            components.push(ComponentSeedV5 {
                object_kind,
                candidate_key_kind: CandidateKeyKindV5::NoCandidate,
                from_ids: BTreeSet::from([node.id.clone()]),
                to_ids: BTreeSet::new(),
                stage,
            });
        }
    }
    for node in target {
        if consumed_target.insert(node.id.clone()) {
            components.push(ComponentSeedV5 {
                object_kind,
                candidate_key_kind: CandidateKeyKindV5::NoCandidate,
                from_ids: BTreeSet::new(),
                to_ids: BTreeSet::from([node.id.clone()]),
                stage,
            });
        }
    }
}

fn successor_map(seeds: &[ComponentSeedV5]) -> BTreeMap<StableId, BTreeSet<StableId>> {
    let mut result = BTreeMap::new();
    for seed in seeds {
        for source in &seed.from_ids {
            result.insert(source.clone(), seed.to_ids.clone());
        }
    }
    result
}

fn id_ref_map_heap(values: &BTreeMap<StableId, &ProgramNodeV5>) -> usize {
    values.len() * std::mem::size_of::<(StableId, &ProgramNodeV5)>()
        + values.keys().map(StableId::allocated_bytes).sum::<usize>()
}

fn successor_map_heap(values: &BTreeMap<StableId, BTreeSet<StableId>>) -> usize {
    values.len() * std::mem::size_of::<(StableId, BTreeSet<StableId>)>()
        + values
            .iter()
            .map(|(id, targets)| id.allocated_bytes() + id_set_heap(targets))
            .sum::<usize>()
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum MappingSideV5 {
    Source,
    Target,
}

fn owner_map_heap(values: &BTreeMap<(MappingSideV5, StableId), (usize, StableId)>) -> usize {
    values.len() * std::mem::size_of::<((MappingSideV5, StableId), (usize, StableId))>()
        + values
            .iter()
            .map(|((_, id), (_, mapping_id))| id.allocated_bytes() + mapping_id.allocated_bytes())
            .sum::<usize>()
}

fn anchor_map_heap(values: &BTreeMap<StableId, RustSymbolAnchorV1>) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<(StableId, RustSymbolAnchorV1)>())
        .saturating_add(
            values
                .iter()
                .map(|(id, anchor)| {
                    id.allocated_bytes()
                        .saturating_add(anchor.signature_shape_hash.allocated_bytes())
                        .saturating_add(anchor.normalized_body_hash.allocated_bytes())
                })
                .sum(),
        )
}

fn relation_order_map_heap(values: &BTreeMap<StableId, Vec<StableId>>) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<(StableId, Vec<StableId>)>())
        .saturating_add(
            values
                .iter()
                .map(|(id, targets)| id.allocated_bytes().saturating_add(id_vec_heap(targets)))
                .sum(),
        )
}

fn git_change_facts_heap(values: &[GitChangeFactV5]) -> usize {
    std::mem::size_of_val(values).saturating_add(
        values
            .iter()
            .map(|fact| {
                fact.fact_id
                    .allocated_bytes()
                    .saturating_add(fact.source_artifact_id.allocated_bytes())
                    .saturating_add(fact.target_artifact_id.allocated_bytes())
                    .saturating_add(fact.source_path.capacity())
                    .saturating_add(fact.target_path.capacity())
                    .saturating_add(
                        fact.equal_content_hash
                            .as_ref()
                            .map_or(0, ContentHash::allocated_bytes),
                    )
            })
            .sum(),
    )
}

fn candidate_scratch_upper_bound(source_count: usize, target_count: usize) -> usize {
    let source_edges = source_count.saturating_mul(MAX_M6_MAPPING_SIDE_IDS);
    let target_edges = target_count.saturating_mul(MAX_M6_MAPPING_SIDE_IDS);
    let edges = source_edges.min(target_edges);
    source_count
        .saturating_add(target_count)
        .saturating_mul(std::mem::size_of::<&ProgramNodeV5>())
        .saturating_add(
            source_count
                .saturating_add(target_count)
                .saturating_mul(std::mem::size_of::<Vec<usize>>()),
        )
        .saturating_add(
            edges
                .saturating_mul(2)
                .saturating_mul(std::mem::size_of::<usize>()),
        )
        .saturating_add(
            source_count.saturating_add(target_count).saturating_mul(
                std::mem::size_of::<usize>() + std::mem::size_of::<(bool, usize)>(),
            ),
        )
}

fn mapped_set(
    source_ids: &BTreeSet<StableId>,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> Option<BTreeSet<StableId>> {
    let mut mapped = BTreeSet::new();
    for source_id in source_ids {
        mapped.extend(successors.get(source_id)?.iter().cloned());
    }
    Some(mapped)
}

fn uniquely_mapped_set(
    source_ids: &BTreeSet<StableId>,
    target_ids: &BTreeSet<StableId>,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> bool {
    let mut mapped = BTreeSet::new();
    for source_id in source_ids {
        let Some(targets) = successors.get(source_id) else {
            return false;
        };
        if targets.len() != 1 {
            return false;
        }
        let target_id = targets.iter().next().unwrap();
        if successors
            .values()
            .filter(|candidate_targets| candidate_targets.contains(target_id))
            .count()
            != 1
        {
            return false;
        }
        mapped.insert(target_id.clone());
    }
    mapped == *target_ids
}

fn dependent_references_are_unique(
    source: &ProgramNodeV5,
    target: &ProgramNodeV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> bool {
    match (&source.data, &target.data) {
        (
            NodeDataV5::Relation {
                source_id,
                ordered_target_ids,
                ..
            },
            NodeDataV5::Relation {
                source_id: target_source,
                ordered_target_ids: target_targets,
                ..
            },
        ) => {
            let source_refs = std::iter::once(source_id.clone())
                .chain(ordered_target_ids.iter().cloned())
                .collect();
            let target_refs = std::iter::once(target_source.clone())
                .chain(target_targets.iter().cloned())
                .collect();
            uniquely_mapped_set(&source_refs, &target_refs, successors)
        }
        (
            NodeDataV5::Context { members, .. },
            NodeDataV5::Context {
                members: targets, ..
            },
        ) => uniquely_mapped_set(members, targets, successors),
        (NodeDataV5::Invariant { scope, .. }, NodeDataV5::Invariant { scope: targets, .. }) => {
            uniquely_mapped_set(scope, targets, successors)
        }
        (
            NodeDataV5::Limitation { sources, .. },
            NodeDataV5::Limitation {
                sources: targets, ..
            },
        ) => uniquely_mapped_set(sources, targets, successors),
        _ => true,
    }
}

fn artifact_parts(node: &ProgramNodeV5) -> Option<(&str, &str, Option<&str>, Option<&str>)> {
    let NodeDataV5::Artifact {
        kind,
        label,
        language,
        path,
        ..
    } = &node.data
    else {
        return None;
    };
    Some((kind, label, language.as_deref(), path.as_deref()))
}

fn relation_candidate(
    source: &ProgramNodeV5,
    target: &ProgramNodeV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> bool {
    let NodeDataV5::Relation {
        kind: source_kind,
        directed: source_directed,
        source_id,
        ordered_target_ids,
        ..
    } = &source.data
    else {
        return false;
    };
    let NodeDataV5::Relation {
        kind: target_kind,
        directed: target_directed,
        source_id: target_source_id,
        ordered_target_ids: target_targets,
        ..
    } = &target.data
    else {
        return false;
    };
    source_kind == target_kind
        && source_directed == target_directed
        && successors
            .get(source_id)
            .is_some_and(|ids| ids.contains(target_source_id))
        && ordered_target_ids.len() == target_targets.len()
        && ordered_target_ids
            .iter()
            .zip(target_targets)
            .all(|(source_id, target_id)| {
                successors
                    .get(source_id)
                    .is_some_and(|ids| ids.contains(target_id))
            })
}

#[derive(Serialize)]
struct RelationTopologySignatureV5<'a> {
    directed: bool,
    kind: &'a str,
    scc_size: usize,
    source_targets: BTreeSet<StableId>,
    target_shapes: Vec<RelationTopologyRefV5<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "reference_kind", content = "value")]
enum RelationTopologyRefV5<'a> {
    External(BTreeSet<StableId>),
    Internal(&'a ContentHash),
}

/// Computes an ordered fixed-point topology color for every unresolved
/// relation. External references are normalized to target IDs; internal SCC
/// references use the previous round's color. Running one round per vertex
/// distinguishes finite ordered topology without guessing an ID pairing,
/// while truly symmetric/duplicate SCCs intentionally retain one ambiguous
/// color class.
fn relation_topology_signatures(
    nodes: &[&ProgramNodeV5],
    unresolved: &BTreeSet<StableId>,
    side: ProgramSideV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> M6Result<BTreeMap<StableId, ContentHash>> {
    #[derive(Serialize)]
    struct Initial<'a> {
        directed: bool,
        kind: &'a str,
        scc_size: usize,
        target_count: usize,
    }
    let scc_sizes = relation_scc_sizes(nodes, unresolved);
    let mut colors = BTreeMap::new();
    for node in nodes.iter().filter(|node| unresolved.contains(&node.id)) {
        let NodeDataV5::Relation {
            kind,
            directed,
            ordered_target_ids,
            ..
        } = &node.data
        else {
            continue;
        };
        colors.insert(
            node.id.clone(),
            body_hash(&Initial {
                directed: *directed,
                kind,
                scc_size: scc_sizes[&node.id],
                target_count: ordered_target_ids.len(),
            })?,
        );
    }
    for _ in 0..unresolved.len().max(1) {
        let mut next = BTreeMap::new();
        for node in nodes.iter().filter(|node| unresolved.contains(&node.id)) {
            let NodeDataV5::Relation {
                kind,
                directed,
                source_id,
                ordered_target_ids,
                ..
            } = &node.data
            else {
                continue;
            };
            let target_shapes = ordered_target_ids
                .iter()
                .map(|id| {
                    if unresolved.contains(id) {
                        RelationTopologyRefV5::Internal(&colors[id])
                    } else {
                        RelationTopologyRefV5::External(normalized_targets(id, side, successors))
                    }
                })
                .collect();
            next.insert(
                node.id.clone(),
                body_hash(&RelationTopologySignatureV5 {
                    directed: *directed,
                    kind,
                    scc_size: scc_sizes[&node.id],
                    source_targets: normalized_targets(source_id, side, successors),
                    target_shapes,
                })?,
            );
        }
        colors = next;
    }
    Ok(colors)
}

fn relation_scc_sizes(
    nodes: &[&ProgramNodeV5],
    unresolved: &BTreeSet<StableId>,
) -> BTreeMap<StableId, usize> {
    relation_scc_components(nodes, unresolved)
        .into_iter()
        .flat_map(|component| {
            let size = component.len();
            component.into_iter().map(move |id| (id, size))
        })
        .collect()
}

fn relation_scc_components(
    nodes: &[&ProgramNodeV5],
    unresolved: &BTreeSet<StableId>,
) -> Vec<BTreeSet<StableId>> {
    let ids = unresolved.iter().cloned().collect::<Vec<_>>();
    let index_by_id = ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let node_by_id = nodes
        .iter()
        .map(|node| (node.id.clone(), *node))
        .collect::<BTreeMap<_, _>>();
    let adjacency = ids
        .iter()
        .map(|id| match &node_by_id[id].data {
            NodeDataV5::Relation {
                ordered_target_ids, ..
            } => ordered_target_ids
                .iter()
                .filter_map(|target| index_by_id.get(target).copied())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect::<Vec<_>>();
    // Signature grouping is a quotient of the raw relation SCC graph. That
    // quotient can introduce a cycle even when the raw condensation is a DAG,
    // so collapse the final seed graph again instead of refusing legal input.
    struct Tarjan<'a> {
        adjacency: &'a [Vec<usize>],
        next_index: usize,
        indices: Vec<Option<usize>>,
        lowlink: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        components: Vec<Vec<usize>>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, vertex: usize) {
            let index = self.next_index;
            self.next_index += 1;
            self.indices[vertex] = Some(index);
            self.lowlink[vertex] = index;
            self.stack.push(vertex);
            self.on_stack[vertex] = true;
            for target in &self.adjacency[vertex] {
                if self.indices[*target].is_none() {
                    self.visit(*target);
                    self.lowlink[vertex] = self.lowlink[vertex].min(self.lowlink[*target]);
                } else if self.on_stack[*target] {
                    self.lowlink[vertex] = self.lowlink[vertex].min(self.indices[*target].unwrap());
                }
            }
            if self.lowlink[vertex] == self.indices[vertex].unwrap() {
                let mut component = Vec::new();
                loop {
                    let member = self.stack.pop().unwrap();
                    self.on_stack[member] = false;
                    component.push(member);
                    if member == vertex {
                        break;
                    }
                }
                self.components.push(component);
            }
        }
    }
    let len = ids.len();
    let mut tarjan = Tarjan {
        adjacency: &adjacency,
        next_index: 0,
        indices: vec![None; len],
        lowlink: vec![0; len],
        stack: Vec::new(),
        on_stack: vec![false; len],
        components: Vec::new(),
    };
    for vertex in 0..len {
        if tarjan.indices[vertex].is_none() {
            tarjan.visit(vertex);
        }
    }
    let mut components = tarjan
        .components
        .into_iter()
        .map(|component| {
            component
                .into_iter()
                .map(|index| ids[index].clone())
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    components.sort_by(|left, right| left.iter().next().cmp(&right.iter().next()));
    components
}

fn relation_scc_signature(
    component: &BTreeSet<StableId>,
    node_colors: &BTreeMap<StableId, ContentHash>,
) -> M6Result<ContentHash> {
    let mut colors = component
        .iter()
        .map(|id| node_colors[id].clone())
        .collect::<Vec<_>>();
    colors.sort();
    body_hash(&colors)
}

#[cfg(test)]
fn relation_scc_depths(
    nodes: &[&ProgramNodeV5],
    components: &[BTreeSet<StableId>],
) -> M6Result<Vec<usize>> {
    let membership = components
        .iter()
        .enumerate()
        .flat_map(|(index, component)| component.iter().cloned().map(move |id| (id, index)))
        .collect::<BTreeMap<_, _>>();
    let node_by_id = nodes
        .iter()
        .map(|node| (node.id.clone(), *node))
        .collect::<BTreeMap<_, _>>();
    let dependencies = components
        .iter()
        .enumerate()
        .map(|(index, component)| {
            component
                .iter()
                .flat_map(|id| match &node_by_id[id].data {
                    NodeDataV5::Relation {
                        ordered_target_ids, ..
                    } => ordered_target_ids.as_slice(),
                    _ => &[],
                })
                .filter_map(|target| membership.get(target).copied())
                .filter(|dependency| *dependency != index)
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    // Tarjan's component graph is acyclic by construction. Its depth is the
    // only stage offset used for the final grouped seeds.
    fn depth(
        index: usize,
        dependencies: &[BTreeSet<usize>],
        memo: &mut [Option<usize>],
    ) -> M6Result<usize> {
        if let Some(value) = memo[index] {
            return Ok(value);
        }
        let mut value = 0_usize;
        for dependency in &dependencies[index] {
            value = value.max(
                depth(*dependency, dependencies, memo)?
                    .checked_add(1)
                    .ok_or(M6Error::Incomplete {
                        operation: "M6 SCC condensation depth",
                        limit: usize::MAX,
                        observed: usize::MAX,
                    })?,
            );
        }
        memo[index] = Some(value);
        Ok(value)
    }
    let mut memo = vec![None; components.len()];
    for index in 0..components.len() {
        depth(index, &dependencies, &mut memo)?;
    }
    Ok(memo.into_iter().map(Option::unwrap).collect())
}

#[allow(clippy::too_many_arguments)]
fn collect_atomic_relation_sccs(
    source_nodes: &[&ProgramNodeV5],
    target_nodes: &[&ProgramNodeV5],
    unresolved_source: &BTreeSet<StableId>,
    unresolved_target: &BTreeSet<StableId>,
    source_colors: &BTreeMap<StableId, ContentHash>,
    target_colors: &BTreeMap<StableId, ContentHash>,
    consumed_source: &mut BTreeSet<StableId>,
    consumed_target: &mut BTreeSet<StableId>,
    stage: usize,
) -> M6Result<Vec<ComponentSeedV5>> {
    let mut by_signature =
        BTreeMap::<ContentHash, (Vec<BTreeSet<StableId>>, Vec<BTreeSet<StableId>>)>::new();
    let source_components = relation_scc_components(source_nodes, unresolved_source);
    for component in source_components {
        let signature = relation_scc_signature(&component, source_colors)?;
        by_signature.entry(signature).or_default().0.push(component);
    }
    let target_components = relation_scc_components(target_nodes, unresolved_target);
    for component in target_components {
        let signature = relation_scc_signature(&component, target_colors)?;
        by_signature.entry(signature).or_default().1.push(component);
    }
    let mut seeds = Vec::new();
    for (_signature, (source_components, target_components)) in by_signature {
        if source_components.is_empty() || target_components.is_empty() {
            continue;
        }
        let from_ids = source_components
            .into_iter()
            .flatten()
            .collect::<BTreeSet<_>>();
        let to_ids = target_components
            .into_iter()
            .flatten()
            .collect::<BTreeSet<_>>();
        bounded(
            from_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 atomic relation SCC source IDs",
        )?;
        bounded(
            to_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 atomic relation SCC target IDs",
        )?;
        consumed_source.extend(from_ids.iter().cloned());
        consumed_target.extend(to_ids.iter().cloned());
        seeds.push(ComponentSeedV5 {
            object_kind: ProgramObjectKindV5::Relation,
            candidate_key_kind: CandidateKeyKindV5::MappedDirectedEndpoints,
            from_ids,
            to_ids,
            stage,
        });
    }
    collapse_and_stage_grouped_relation_seeds(seeds, source_nodes, target_nodes, stage)
}

fn collapse_and_stage_grouped_relation_seeds(
    seeds: Vec<ComponentSeedV5>,
    source_nodes: &[&ProgramNodeV5],
    target_nodes: &[&ProgramNodeV5],
    base_stage: usize,
) -> M6Result<Vec<ComponentSeedV5>> {
    let source_owners = seeds
        .iter()
        .enumerate()
        .flat_map(|(index, seed)| seed.from_ids.iter().cloned().map(move |id| (id, index)))
        .collect::<BTreeMap<_, _>>();
    let target_owners = seeds
        .iter()
        .enumerate()
        .flat_map(|(index, seed)| seed.to_ids.iter().cloned().map(move |id| (id, index)))
        .collect::<BTreeMap<_, _>>();
    let source_by_id = source_nodes
        .iter()
        .map(|node| (node.id.clone(), *node))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_nodes
        .iter()
        .map(|node| (node.id.clone(), *node))
        .collect::<BTreeMap<_, _>>();
    let mut dependencies = vec![BTreeSet::new(); seeds.len()];
    for (index, seed) in seeds.iter().enumerate() {
        for (ids, nodes, owners) in [
            (&seed.from_ids, &source_by_id, &source_owners),
            (&seed.to_ids, &target_by_id, &target_owners),
        ] {
            for id in ids {
                let node = nodes.get(id).ok_or(M6Error::InvalidMapping(
                    "atomic relation seed references an unknown relation node",
                ))?;
                let NodeDataV5::Relation {
                    ordered_target_ids, ..
                } = &node.data
                else {
                    return Err(M6Error::InvalidMapping(
                        "atomic relation seed contains a non-relation node",
                    ));
                };
                dependencies[index].extend(
                    ordered_target_ids
                        .iter()
                        .filter_map(|target| owners.get(target).copied())
                        .filter(|owner| *owner != index),
                );
            }
        }
    }

    struct Tarjan<'a> {
        dependencies: &'a [BTreeSet<usize>],
        next_index: usize,
        indices: Vec<Option<usize>>,
        lowlink: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        components: Vec<Vec<usize>>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, vertex: usize) {
            let index = self.next_index;
            self.next_index += 1;
            self.indices[vertex] = Some(index);
            self.lowlink[vertex] = index;
            self.stack.push(vertex);
            self.on_stack[vertex] = true;
            for dependency in &self.dependencies[vertex] {
                if self.indices[*dependency].is_none() {
                    self.visit(*dependency);
                    self.lowlink[vertex] = self.lowlink[vertex].min(self.lowlink[*dependency]);
                } else if self.on_stack[*dependency] {
                    self.lowlink[vertex] = self.lowlink[vertex]
                        .min(self.indices[*dependency].expect("visited dependency has an index"));
                }
            }
            if self.lowlink[vertex] == self.indices[vertex].unwrap() {
                let mut component = Vec::new();
                loop {
                    let member = self.stack.pop().unwrap();
                    self.on_stack[member] = false;
                    component.push(member);
                    if member == vertex {
                        break;
                    }
                }
                component.sort_unstable();
                self.components.push(component);
            }
        }
    }
    let mut tarjan = Tarjan {
        dependencies: &dependencies,
        next_index: 0,
        indices: vec![None; seeds.len()],
        lowlink: vec![0; seeds.len()],
        stack: Vec::new(),
        on_stack: vec![false; seeds.len()],
        components: Vec::new(),
    };
    for index in 0..seeds.len() {
        if tarjan.indices[index].is_none() {
            tarjan.visit(index);
        }
    }
    tarjan
        .components
        .sort_by_key(|component| component.first().copied());
    let seed_component = tarjan
        .components
        .iter()
        .enumerate()
        .flat_map(|(component_index, component)| {
            component
                .iter()
                .copied()
                .map(move |seed_index| (seed_index, component_index))
        })
        .collect::<BTreeMap<_, _>>();
    let mut collapsed = Vec::with_capacity(tarjan.components.len());
    for component in &tarjan.components {
        let from_ids = component
            .iter()
            .flat_map(|index| seeds[*index].from_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let to_ids = component
            .iter()
            .flat_map(|index| seeds[*index].to_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        bounded(
            from_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 grouped relation condensation source IDs",
        )?;
        bounded(
            to_ids.len(),
            MAX_M6_MAPPING_SIDE_IDS,
            "M6 grouped relation condensation target IDs",
        )?;
        collapsed.push(ComponentSeedV5 {
            object_kind: ProgramObjectKindV5::Relation,
            candidate_key_kind: CandidateKeyKindV5::MappedDirectedEndpoints,
            from_ids,
            to_ids,
            stage: base_stage,
        });
    }
    let collapsed_dependencies = tarjan
        .components
        .iter()
        .enumerate()
        .map(|(component_index, component)| {
            component
                .iter()
                .flat_map(|seed_index| dependencies[*seed_index].iter())
                .map(|dependency| seed_component[dependency])
                .filter(|dependency| *dependency != component_index)
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();

    fn depth(
        index: usize,
        dependencies: &[BTreeSet<usize>],
        memo: &mut [Option<usize>],
    ) -> M6Result<usize> {
        if let Some(value) = memo[index] {
            return Ok(value);
        }
        let mut value = 0_usize;
        for dependency in &dependencies[index] {
            value = value.max(
                depth(*dependency, dependencies, memo)?
                    .checked_add(1)
                    .ok_or(M6Error::Incomplete {
                        operation: "M6 grouped relation SCC condensation depth",
                        limit: usize::MAX,
                        observed: usize::MAX,
                    })?,
            );
        }
        memo[index] = Some(value);
        Ok(value)
    }

    let mut memo = vec![None; collapsed.len()];
    for (index, seed) in collapsed.iter_mut().enumerate() {
        let seed_depth = depth(index, &collapsed_dependencies, &mut memo)?;
        seed.stage = base_stage
            .checked_add(seed_depth)
            .ok_or(M6Error::Incomplete {
                operation: "M6 grouped relation SCC condensation stage",
                limit: usize::MAX,
                observed: usize::MAX,
            })?;
    }
    Ok(collapsed)
}

fn node_references(node: &ProgramNodeV5) -> BTreeSet<StableId> {
    match &node.data {
        NodeDataV5::Artifact { symbol_ref, .. } => symbol_ref.iter().cloned().collect(),
        NodeDataV5::Relation {
            source_id,
            ordered_target_ids,
            ..
        } => std::iter::once(source_id.clone())
            .chain(ordered_target_ids.iter().cloned())
            .collect(),
        NodeDataV5::Context { members, .. } => members.clone(),
        NodeDataV5::Invariant { scope, .. } => scope.clone(),
        NodeDataV5::Limitation { sources, .. } => sources.clone(),
        NodeDataV5::Repository { .. } | NodeDataV5::Snapshot => BTreeSet::new(),
    }
}

#[derive(Serialize)]
struct LocationWithoutSymbolV5<'a> {
    end_column: Option<u64>,
    end_line: Option<u64>,
    path: &'a str,
    start_column: Option<u64>,
    start_line: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case", tag = "object_kind")]
enum NormalizedProgramBodyV5<'a> {
    Repository {
        canonical_body_hash: &'a ContentHash,
    },
    Snapshot {
        canonical_body_hash: &'a ContentHash,
    },
    Artifact {
        anchor: &'a Option<RustSymbolAnchorV1>,
        base_hash: &'a ContentHash,
        change_fact_ids: &'a BTreeSet<StableId>,
        content_hash: &'a Option<ContentHash>,
        label: &'a str,
        location: Option<LocationWithoutSymbolV5<'a>>,
        provenance_hash: ContentHash,
        symbol_targets: Option<BTreeSet<StableId>>,
    },
    Relation {
        base_hash: &'a ContentHash,
        provenance_hash: ContentHash,
        source_targets: BTreeSet<StableId>,
        target_targets: Vec<BTreeSet<StableId>>,
    },
    Context {
        base_hash: &'a ContentHash,
        member_targets: BTreeSet<StableId>,
        provenance_hash: ContentHash,
    },
    Invariant {
        base_hash: &'a ContentHash,
        provenance_hash: ContentHash,
        scope_targets: BTreeSet<StableId>,
    },
    Limitation {
        base_hash: &'a ContentHash,
        source_targets: BTreeSet<StableId>,
    },
}

fn normalized_targets(
    id: &StableId,
    side: ProgramSideV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> BTreeSet<StableId> {
    match side {
        ProgramSideV5::Source => successors.get(id).cloned().unwrap_or_default(),
        ProgramSideV5::Target => BTreeSet::from([id.clone()]),
    }
}

fn normalized_program_body_hash(
    node: &ProgramNodeV5,
    side: ProgramSideV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> M6Result<ContentHash> {
    let body = match &node.data {
        NodeDataV5::Repository { .. } => NormalizedProgramBodyV5::Repository {
            canonical_body_hash: &node.body_hash,
        },
        NodeDataV5::Snapshot => NormalizedProgramBodyV5::Snapshot {
            canonical_body_hash: &node.body_hash,
        },
        NodeDataV5::Artifact {
            label,
            location,
            content_hash,
            anchor,
            base_hash,
            provenance,
            change_fact_ids,
            symbol_ref,
            ..
        } => NormalizedProgramBodyV5::Artifact {
            anchor,
            base_hash,
            change_fact_ids,
            content_hash,
            label,
            location: location.as_ref().map(|location| LocationWithoutSymbolV5 {
                end_column: location.end_column,
                end_line: location.end_line,
                path: &location.path,
                start_column: location.start_column,
                start_line: location.start_line,
            }),
            provenance_hash: normalized_provenance_body_hash(provenance, side, successors)?,
            symbol_targets: symbol_ref
                .as_ref()
                .map(|id| normalized_targets(id, side, successors)),
        },
        NodeDataV5::Relation {
            source_id,
            ordered_target_ids,
            base_hash,
            provenance,
            ..
        } => NormalizedProgramBodyV5::Relation {
            base_hash,
            provenance_hash: normalized_provenance_body_hash(provenance, side, successors)?,
            source_targets: normalized_targets(source_id, side, successors),
            target_targets: ordered_target_ids
                .iter()
                .map(|id| normalized_targets(id, side, successors))
                .collect(),
        },
        NodeDataV5::Context {
            members,
            base_hash,
            provenance,
            ..
        } => NormalizedProgramBodyV5::Context {
            base_hash,
            member_targets: members
                .iter()
                .flat_map(|id| normalized_targets(id, side, successors))
                .collect(),
            provenance_hash: normalized_provenance_body_hash(provenance, side, successors)?,
        },
        NodeDataV5::Invariant {
            scope,
            base_hash,
            provenance,
            ..
        } => NormalizedProgramBodyV5::Invariant {
            base_hash,
            provenance_hash: normalized_provenance_body_hash(provenance, side, successors)?,
            scope_targets: scope
                .iter()
                .flat_map(|id| normalized_targets(id, side, successors))
                .collect(),
        },
        NodeDataV5::Limitation {
            sources, base_hash, ..
        } => NormalizedProgramBodyV5::Limitation {
            base_hash,
            source_targets: sources
                .iter()
                .flat_map(|id| normalized_targets(id, side, successors))
                .collect(),
        },
    };
    body_hash(&body)
}

fn location_equal_without_symbol(
    left: &Option<Box<crate::Location>>,
    right: &Option<Box<crate::Location>>,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.path == right.path
                && left.start_line == right.start_line
                && left.end_line == right.end_line
                && left.start_column == right.start_column
                && left.end_column == right.end_column
        }
        _ => false,
    }
}

fn git_rename_location_equal(
    left: &Option<Box<crate::Location>>,
    right: &Option<Box<crate::Location>>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.path != right.path
                && left.start_line == right.start_line
                && left.end_line == right.end_line
                && left.start_column == right.start_column
                && left.end_column == right.end_column
                && left.symbol_id == right.symbol_id
        }
        _ => false,
    }
}

fn path_derived_label(label: &str, location: &Option<Box<crate::Location>>) -> bool {
    location.as_ref().is_some_and(|location| {
        label == location.path
            || location
                .path
                .rsplit('/')
                .next()
                .is_some_and(|basename| label == basename)
    })
}

fn semantic_equal(
    source: &ProgramNodeV5,
    target: &ProgramNodeV5,
    candidate: CandidateKeyKindV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> bool {
    match (&source.data, &target.data) {
        (NodeDataV5::Repository { identity: left }, NodeDataV5::Repository { identity: right }) => {
            left == right
        }
        (NodeDataV5::Snapshot, NodeDataV5::Snapshot) => false,
        (
            NodeDataV5::Artifact {
                label: left_label,
                location: left_location,
                content_hash: left_content,
                anchor: left_anchor,
                base_hash: left_base,
                provenance: left_provenance,
                symbol_ref: left_symbol,
                ..
            },
            NodeDataV5::Artifact {
                label: right_label,
                location: right_location,
                content_hash: right_content,
                anchor: right_anchor,
                base_hash: right_base,
                provenance: right_provenance,
                symbol_ref: right_symbol,
                ..
            },
        ) => {
            if left_base != right_base
                || !normalized_provenance_equal(left_provenance, right_provenance, successors)
            {
                return false;
            }
            if left_anchor.is_some() || right_anchor.is_some() {
                if left_anchor != right_anchor {
                    return false;
                }
                if candidate == CandidateKeyKindV5::RustSymbolAnchorV1 {
                    return true;
                }
                let symbol_equal = match (left_symbol, right_symbol) {
                    (None, None) => true,
                    (Some(left), Some(right)) => successors
                        .get(left)
                        .is_some_and(|ids| ids.len() == 1 && ids.contains(right)),
                    _ => false,
                };
                return left_label == right_label
                    && location_equal_without_symbol(left_location, right_location)
                    && symbol_equal;
            }
            if left_content != right_content {
                return false;
            }
            if candidate == CandidateKeyKindV5::GitRenameSameContent {
                return git_rename_location_equal(left_location, right_location)
                    && path_derived_label(left_label, left_location)
                    && path_derived_label(right_label, right_location);
            }
            let symbol_equal = match (left_symbol, right_symbol) {
                (None, None) => true,
                (Some(left), Some(right)) => successors
                    .get(left)
                    .is_some_and(|ids| ids.len() == 1 && ids.contains(right)),
                _ => false,
            };
            left_label == right_label
                && location_equal_without_symbol(left_location, right_location)
                && symbol_equal
        }
        (
            NodeDataV5::Relation {
                source_id,
                ordered_target_ids,
                base_hash: left_base,
                provenance: left_provenance,
                ..
            },
            NodeDataV5::Relation {
                source_id: target_source,
                ordered_target_ids: target_ids,
                base_hash: right_base,
                provenance: right_provenance,
                ..
            },
        ) => {
            left_base == right_base
                && normalized_provenance_equal(left_provenance, right_provenance, successors)
                && successors
                    .get(source_id)
                    .is_some_and(|ids| ids.len() == 1 && ids.contains(target_source))
                && ordered_target_ids.len() == target_ids.len()
                && ordered_target_ids
                    .iter()
                    .zip(target_ids)
                    .all(|(left, right)| {
                        successors
                            .get(left)
                            .is_some_and(|ids| ids.len() == 1 && ids.contains(right))
                    })
        }
        (
            NodeDataV5::Context {
                members,
                base_hash: left,
                provenance: left_provenance,
                ..
            },
            NodeDataV5::Context {
                members: target_members,
                base_hash: right,
                provenance: right_provenance,
                ..
            },
        ) => {
            left == right
                && normalized_provenance_equal(left_provenance, right_provenance, successors)
                && mapped_set(members, successors).as_ref() == Some(target_members)
        }
        (
            NodeDataV5::Invariant {
                scope,
                base_hash: left,
                provenance: left_provenance,
                ..
            },
            NodeDataV5::Invariant {
                scope: target_scope,
                base_hash: right,
                provenance: right_provenance,
                ..
            },
        ) => {
            left == right
                && normalized_provenance_equal(left_provenance, right_provenance, successors)
                && mapped_set(scope, successors).as_ref() == Some(target_scope)
        }
        (
            NodeDataV5::Limitation {
                sources,
                base_hash: left,
                ..
            },
            NodeDataV5::Limitation {
                sources: target_sources,
                base_hash: right,
                ..
            },
        ) => left == right && mapped_set(sources, successors).as_ref() == Some(target_sources),
        _ => false,
    }
}

fn component_status(
    seed: &ComponentSeedV5,
    source_by_id: &BTreeMap<StableId, &ProgramNodeV5>,
    target_by_id: &BTreeMap<StableId, &ProgramNodeV5>,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> MappingStatusV5 {
    match (seed.from_ids.len(), seed.to_ids.len()) {
        (0, 1) => MappingStatusV5::Added,
        (1, 0) => MappingStatusV5::Removed,
        (1, 1) => {
            let source = source_by_id[seed.from_ids.iter().next().unwrap()];
            let target = target_by_id[seed.to_ids.iter().next().unwrap()];
            if !dependent_references_are_unique(source, target, successors) {
                MappingStatusV5::Unresolved
            } else if semantic_equal(source, target, seed.candidate_key_kind, successors) {
                MappingStatusV5::Preserved
            } else {
                MappingStatusV5::Modified
            }
        }
        (1, _) => MappingStatusV5::Split,
        (_, 1) => MappingStatusV5::Merged,
        _ => MappingStatusV5::Unresolved,
    }
}

#[derive(Clone, Debug)]
pub struct M6MappingPhaseV5 {
    mappings: Vec<ProgramMappingV5>,
    morphism: ChangeMorphismV5,
    working_peak_upper_bound_bytes: usize,
}

impl M6MappingPhaseV5 {
    pub fn mappings(&self) -> &[ProgramMappingV5] {
        &self.mappings
    }
    fn mappings_capacity(&self) -> usize {
        self.mappings.capacity()
    }
    pub fn morphism(&self) -> &ChangeMorphismV5 {
        &self.morphism
    }
    pub fn working_peak_upper_bound_bytes(&self) -> usize {
        self.working_peak_upper_bound_bytes
    }

    /// Compares replayed canonical DTOs against this freshly recomputed phase;
    /// replay bytes can never select or alter mappings.
    #[doc(hidden)]
    pub fn validate_replayed_canonical(
        &self,
        mapping_bytes: &[Vec<u8>],
        morphism_bytes: &[u8],
    ) -> M6Result<()> {
        if mapping_bytes.len() != self.mappings.len() {
            return Err(M6Error::InvalidWire(
                "replayed mapping count differs from recomputed phase".to_owned(),
            ));
        }
        for (bytes, expected) in mapping_bytes.iter().zip(&self.mappings) {
            let actual = ProgramMappingV5::from_json_bytes(bytes)?;
            if actual != *expected {
                return Err(M6Error::InvalidWire(
                    "replayed mapping differs from recomputed phase".to_owned(),
                ));
            }
        }
        let actual = ChangeMorphismV5::from_json_bytes(morphism_bytes, &self.morphism)?;
        if actual != self.morphism {
            return Err(M6Error::InvalidWire(
                "replayed morphism differs from recomputed phase".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct ChangeMorphismIdentityV5<'a> {
    source_closure_id: &'a StableId,
    repository_id: &'a StableId,
    source_snapshot_id: &'a StableId,
    target_snapshot_id: &'a StableId,
    mapping_policy_descriptor_id: &'static str,
    semantic_anchor_descriptor_id: &'static str,
    mapping_count: u64,
    mapping_set_digest: &'a ContentHash,
    source_domain_count: u64,
    source_domain_digest: &'a ContentHash,
    target_domain_count: u64,
    target_domain_digest: &'a ContentHash,
    status_counts: &'a MappingStatusCountsV5,
    source_ids: &'a BTreeSet<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChangeMorphismV5 {
    schema: &'static str,
    id: StableId,
    source_closure_id: StableId,
    repository_id: StableId,
    source_snapshot_id: StableId,
    target_snapshot_id: StableId,
    mapping_policy_descriptor_id: &'static str,
    semantic_anchor_descriptor_id: &'static str,
    mapping_count: u64,
    mapping_set_digest: ContentHash,
    source_domain_count: u64,
    source_domain_digest: ContentHash,
    target_domain_count: u64,
    target_domain_digest: ContentHash,
    status_counts: MappingStatusCountsV5,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeMorphismWireV5 {
    schema: String,
    id: StableId,
    source_closure_id: StableId,
    repository_id: StableId,
    source_snapshot_id: StableId,
    target_snapshot_id: StableId,
    mapping_policy_descriptor_id: String,
    semantic_anchor_descriptor_id: String,
    mapping_count: u64,
    mapping_set_digest: ContentHash,
    source_domain_count: u64,
    source_domain_digest: ContentHash,
    target_domain_count: u64,
    target_domain_digest: ContentHash,
    status_counts: MappingStatusCountsV5,
    source_ids: BTreeSet<StableId>,
}

impl ChangeMorphismV5 {
    /// Deterministic reservation admitted by Store before any mapping graph is
    /// allocated. The multiplier covers retained node copies, ownership maps,
    /// successor/predecessor IDs, mapping DTOs and seal domains; candidate
    /// adjacency has its separate exact cardinality bound.
    #[doc(hidden)]
    pub fn mapping_reservation_bytes_from_accepted_program_facts(
        source: &ProgramSpace,
        target: &ProgramSpace,
    ) -> M6Result<usize> {
        if source.accepted_git_revision_closure().is_none()
            || target.accepted_git_revision_closure().is_none()
            || source.accepted_rust_symbol_anchors().is_none()
            || target.accepted_rust_symbol_anchors().is_none()
            || source.accepted_relation_target_order().is_none()
            || target.accepted_relation_target_order().is_none()
        {
            return Err(M6Error::InvalidMapping(
                "mapping reservation requires accepted ProgramSpace v3 incremental facts",
            ));
        }
        Self::mapping_reservation_bytes(source, target)
    }

    fn mapping_reservation_bytes(source: &ProgramSpace, target: &ProgramSpace) -> M6Result<usize> {
        let domain_count = |space: &ProgramSpace| {
            [
                2_usize,
                space.artifacts().len(),
                space.relations().len(),
                space.contexts().len(),
                space.invariants().len(),
                space.extraction().limitations.len(),
            ]
            .into_iter()
            .try_fold(0_usize, usize::checked_add)
            .ok_or(M6Error::Incomplete {
                operation: "M6 mapping reservation domain arithmetic",
                limit: MAX_M6_PROGRAM_DOMAIN_IDS,
                observed: usize::MAX,
            })
        };
        let source_count = domain_count(source)?;
        let target_count = domain_count(target)?;
        bounded(source_count, MAX_M6_PROGRAM_DOMAIN_IDS, "M6 source domain")?;
        bounded(target_count, MAX_M6_PROGRAM_DOMAIN_IDS, "M6 target domain")?;
        crate::canonical::canonical_json_count_bounded(
            &source.streaming_ref(),
            MAX_M6_CANONICAL_BYTES,
            "M6 source ProgramSpace reservation bytes",
        )?;
        crate::canonical::canonical_json_count_bounded(
            &target.streaming_ref(),
            MAX_M6_CANONICAL_BYTES,
            "M6 target ProgramSpace reservation bytes",
        )?;
        let owned = source
            .allocated_bytes()
            .checked_add(target.allocated_bytes())
            .and_then(|bytes| bytes.checked_mul(MAX_M6_MAPPING_SIDE_IDS))
            .ok_or(M6Error::Incomplete {
                operation: "M6 mapping reservation arithmetic",
                limit: MAX_M6_MAPPING_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        let reservation = [
            owned,
            candidate_scratch_upper_bound(source_count, target_count),
            MAX_M6_CANONICAL_BYTES,
            MAX_M6_MAPPING_DTO_BYTES,
            MAX_M6_MORPHISM_DTO_BYTES,
        ]
        .into_iter()
        .try_fold(0_usize, checked_working_add)?;
        bounded(
            reservation,
            MAX_M6_MAPPING_WORKING_BYTES,
            "M6 mapping reservation bytes",
        )?;
        Ok(reservation)
    }

    /// Recomputes the complete deterministic mapping phase from accepted
    /// ProgramSpace facts. Missing anchor/order evidence is a typed refusal.
    #[doc(hidden)]
    pub fn derive_from_accepted_program_facts(
        closure: &IncrementalSourceClosureV5,
        source: &ProgramSpace,
        target: &ProgramSpace,
    ) -> M6Result<M6MappingPhaseV5> {
        let inputs =
            ValidatedIncrementalInputsV5::derive_from_accepted_program_facts(source, target)?;
        Self::build_with_inputs(closure, source, target, &inputs)
    }

    fn allocated_bytes(&self) -> usize {
        [
            std::mem::size_of::<Self>(),
            self.id.allocated_bytes(),
            self.source_closure_id.allocated_bytes(),
            self.repository_id.allocated_bytes(),
            self.source_snapshot_id.allocated_bytes(),
            self.target_snapshot_id.allocated_bytes(),
            self.mapping_set_digest.allocated_bytes(),
            self.source_domain_digest.allocated_bytes(),
            self.target_domain_digest.allocated_bytes(),
            id_set_heap(&self.source_ids),
        ]
        .into_iter()
        .fold(0_usize, usize::saturating_add)
    }

    fn build_with_inputs(
        closure: &IncrementalSourceClosureV5,
        source: &ProgramSpace,
        target: &ProgramSpace,
        inputs: &ValidatedIncrementalInputsV5,
    ) -> M6Result<M6MappingPhaseV5> {
        if source.snapshot_id() != &inputs.source_snapshot_id
            || target.snapshot_id() != &inputs.target_snapshot_id
            || source.snapshot_id() != &closure.input.source_snapshot_id
            || target.snapshot_id() != &closure.input.target_snapshot_id
            || source.repository_id() != target.repository_id()
            || source.repository_id() != &closure.input.repository_id
        {
            return Err(M6Error::InvalidMapping(
                "mapping proof, closure, and ProgramSpaces must bind the same snapshots",
            ));
        }
        let input_working = [
            source.allocated_bytes(),
            target.allocated_bytes(),
            anchor_map_heap(&inputs.rust_source_anchors),
            anchor_map_heap(&inputs.rust_target_anchors),
            relation_order_map_heap(&inputs.source_relation_target_order),
            relation_order_map_heap(&inputs.target_relation_target_order),
            git_change_facts_heap(&inputs.git_change_facts),
        ]
        .into_iter()
        .try_fold(0_usize, checked_working_add)?;
        let (source_nodes, source_working) = program_nodes(
            source,
            ProgramSideV5::Source,
            &inputs.rust_source_anchors,
            &inputs.source_relation_target_order,
            &inputs.git_change_facts,
        )?;
        let (target_nodes, target_working) = program_nodes(
            target,
            ProgramSideV5::Target,
            &inputs.rust_target_anchors,
            &inputs.target_relation_target_order,
            &inputs.git_change_facts,
        )?;
        let node_working = [source_working, target_working]
            .into_iter()
            .try_fold(input_working, checked_working_add)?;
        let source_by_id = source_nodes
            .iter()
            .map(|node| (node.id.clone(), node))
            .collect::<BTreeMap<_, _>>();
        let target_by_id = target_nodes
            .iter()
            .map(|node| (node.id.clone(), node))
            .collect::<BTreeMap<_, _>>();
        let source_kind = |kind| {
            source_nodes
                .iter()
                .filter(|node| node.object_kind == kind)
                .collect::<Vec<_>>()
        };
        let target_kind = |kind| {
            target_nodes
                .iter()
                .filter(|node| node.object_kind == kind)
                .collect::<Vec<_>>()
        };
        let mut consumed_source = BTreeSet::new();
        let mut consumed_target = BTreeSet::new();
        let mut seeds = Vec::new();

        let repositories_source = source_kind(ProgramObjectKindV5::Repository);
        let repositories_target = target_kind(ProgramObjectKindV5::Repository);
        seeds.extend(collect_components(
            &repositories_source, &repositories_target, &mut consumed_source, &mut consumed_target,
            ProgramObjectKindV5::Repository, CandidateKeyKindV5::RepositoryIdentity, 0, node_working,
            |left, right| matches!((&left.data, &right.data),
                (NodeDataV5::Repository { identity: a }, NodeDataV5::Repository { identity: b }) if a == b),
        )?);
        finish_unmatched(
            &repositories_source,
            &repositories_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Repository,
            0,
            &mut seeds,
        );

        let snapshots_source = source_kind(ProgramObjectKindV5::Snapshot);
        let snapshots_target = target_kind(ProgramObjectKindV5::Snapshot);
        seeds.extend(collect_components(
            &snapshots_source,
            &snapshots_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Snapshot,
            CandidateKeyKindV5::SnapshotPair,
            1,
            node_working,
            |_, _| true,
        )?);

        let artifacts_source = source_kind(ProgramObjectKindV5::Artifact);
        let artifacts_target = target_kind(ProgramObjectKindV5::Artifact);
        let copied_target_ids = inputs
            .git_change_facts
            .iter()
            .filter(|fact| fact.kind == GitChangeKindV5::Copied)
            .map(|fact| fact.target_artifact_id.clone())
            .collect::<BTreeSet<_>>();
        seeds.extend(collect_components(
            &artifacts_source,
            &artifacts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            CandidateKeyKindV5::SamePath,
            2,
            node_working,
            |left, right| {
                !copied_target_ids.contains(&right.id)
                    && matches!((artifact_parts(left), artifact_parts(right)),
                    (Some(("file", _, _, Some(a))), Some(("file", _, _, Some(b)))) if a == b)
            },
        )?);
        seeds.extend(collect_components(
            &artifacts_source,
            &artifacts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            CandidateKeyKindV5::GitRenameSameContent,
            3,
            node_working,
            |left, right| {
                inputs.git_change_facts.iter().any(|fact| {
                    fact.kind == GitChangeKindV5::Renamed
                        && fact.equal_content_hash.is_some()
                        && fact.source_artifact_id == left.id
                        && fact.target_artifact_id == right.id
                })
            },
        )?);
        seeds.extend(collect_components(
            &artifacts_source,
            &artifacts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            CandidateKeyKindV5::SamePath,
            4,
            node_working,
            |left, right| match (artifact_parts(left), artifact_parts(right)) {
                (
                    Some((left_kind, left_label, left_language, left_path)),
                    Some((right_kind, right_label, right_language, right_path)),
                ) => {
                    !copied_target_ids.contains(&right.id)
                        && matches!(left_kind, "function" | "method" | "type")
                        && left_language == Some("rust")
                        && right_language == Some("rust")
                        && left_kind == right_kind
                        && left_label == right_label
                        && left_path == right_path
                }
                _ => false,
            },
        )?);
        seeds.extend(collect_components(
            &artifacts_source,
            &artifacts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            CandidateKeyKindV5::RustSymbolAnchorV1,
            5,
            node_working,
            |left, right| match (&left.data, &right.data) {
                (
                    NodeDataV5::Artifact {
                        anchor: Some(a), ..
                    },
                    NodeDataV5::Artifact {
                        anchor: Some(b), ..
                    },
                ) => !copied_target_ids.contains(&right.id) && a == b,
                _ => false,
            },
        )?);
        seeds.extend(collect_components(
            &artifacts_source,
            &artifacts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            CandidateKeyKindV5::SameKindLabelLanguageLocation,
            6,
            node_working,
            |left, right| match (&left.data, &right.data) {
                (
                    NodeDataV5::Artifact {
                        kind: a_kind,
                        label: a_label,
                        language: a_language,
                        location: a_location,
                        anchor: None,
                        ..
                    },
                    NodeDataV5::Artifact {
                        kind: b_kind,
                        label: b_label,
                        language: b_language,
                        location: b_location,
                        anchor: None,
                        ..
                    },
                ) => {
                    !copied_target_ids.contains(&right.id)
                        && a_kind != "file"
                        && b_kind != "file"
                        && a_kind == b_kind
                        && a_label == b_label
                        && a_language == b_language
                        && a_location == b_location
                }
                _ => false,
            },
        )?);
        finish_unmatched(
            &artifacts_source,
            &artifacts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            7,
            &mut seeds,
        );

        let mut successors = successor_map(&seeds);
        let relations_source = source_kind(ProgramObjectKindV5::Relation);
        let relations_target = target_kind(ProgramObjectKindV5::Relation);
        let mut relation_stage = 8_usize;
        loop {
            let wave = collect_components(
                &relations_source,
                &relations_target,
                &mut consumed_source,
                &mut consumed_target,
                ProgramObjectKindV5::Relation,
                CandidateKeyKindV5::MappedDirectedEndpoints,
                relation_stage,
                node_working,
                |left, right| relation_candidate(left, right, &successors),
            )?;
            if wave.is_empty() {
                break;
            }
            seeds.extend(wave);
            successors = successor_map(&seeds);
            relation_stage = relation_stage.checked_add(1).ok_or(M6Error::Incomplete {
                operation: "M6 relation dependency stages",
                limit: usize::MAX,
                observed: usize::MAX,
            })?;
        }
        // A legal relation graph may contain relation-to-relation SCCs. No
        // member of such an SCC can be admitted by the acyclic wave above,
        // because each waits for another member's successor. Build the
        // remaining bipartite fixed-point graph while treating only remaining
        // relation references as unresolved variables. Connected ambiguity is
        // retained as one exact component; it is never degraded into unrelated
        // removed/added records.
        let unresolved_source_relations = relations_source
            .iter()
            .filter(|node| !consumed_source.contains(&node.id))
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        let unresolved_target_relations = relations_target
            .iter()
            .filter(|node| !consumed_target.contains(&node.id))
            .map(|node| node.id.clone())
            .collect::<BTreeSet<_>>();
        let source_topology = relation_topology_signatures(
            &relations_source,
            &unresolved_source_relations,
            ProgramSideV5::Source,
            &successors,
        )?;
        let target_topology = relation_topology_signatures(
            &relations_target,
            &unresolved_target_relations,
            ProgramSideV5::Target,
            &successors,
        )?;
        let cyclic_wave = collect_atomic_relation_sccs(
            &relations_source,
            &relations_target,
            &unresolved_source_relations,
            &unresolved_target_relations,
            &source_topology,
            &target_topology,
            &mut consumed_source,
            &mut consumed_target,
            relation_stage,
        )?;
        if !cyclic_wave.is_empty() {
            let last_cyclic_stage = cyclic_wave.iter().map(|seed| seed.stage).max().unwrap();
            seeds.extend(cyclic_wave);
            relation_stage = last_cyclic_stage
                .checked_add(1)
                .ok_or(M6Error::Incomplete {
                    operation: "M6 cyclic relation dependency stage",
                    limit: usize::MAX,
                    observed: usize::MAX,
                })?;
        }
        finish_unmatched(
            &relations_source,
            &relations_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Relation,
            relation_stage,
            &mut seeds,
        );
        successors = successor_map(&seeds);
        let context_stage = relation_stage.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 dependency stages",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;

        let contexts_source = source_kind(ProgramObjectKindV5::Context);
        let contexts_target = target_kind(ProgramObjectKindV5::Context);
        seeds.extend(collect_components(
            &contexts_source,
            &contexts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Context,
            CandidateKeyKindV5::MappedMembers,
            context_stage,
            node_working,
            |left, right| match (&left.data, &right.data) {
                (
                    NodeDataV5::Context {
                        kind: a, members, ..
                    },
                    NodeDataV5::Context {
                        kind: b,
                        members: target_members,
                        ..
                    },
                ) => a == b && mapped_set(members, &successors).as_ref() == Some(target_members),
                _ => false,
            },
        )?);
        finish_unmatched(
            &contexts_source,
            &contexts_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Context,
            context_stage,
            &mut seeds,
        );
        successors = successor_map(&seeds);
        let invariant_stage = context_stage.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 dependency stages",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;

        let invariants_source = source_kind(ProgramObjectKindV5::Invariant);
        let invariants_target = target_kind(ProgramObjectKindV5::Invariant);
        seeds.extend(collect_components(
            &invariants_source,
            &invariants_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Invariant,
            CandidateKeyKindV5::MappedScope,
            invariant_stage,
            node_working,
            |left, right| match (&left.data, &right.data) {
                (
                    NodeDataV5::Invariant {
                        property_id: a,
                        scope,
                        ..
                    },
                    NodeDataV5::Invariant {
                        property_id: b,
                        scope: target_scope,
                        ..
                    },
                ) => a == b && mapped_set(scope, &successors).as_ref() == Some(target_scope),
                _ => false,
            },
        )?);
        finish_unmatched(
            &invariants_source,
            &invariants_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Invariant,
            invariant_stage,
            &mut seeds,
        );
        successors = successor_map(&seeds);
        let limitation_base_stage = invariant_stage.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 dependency stages",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;

        let limitations_source = source_kind(ProgramObjectKindV5::Limitation);
        let limitations_target = target_kind(ProgramObjectKindV5::Limitation);
        let mut limitation_stage = limitation_base_stage;
        loop {
            let wave = collect_components(
                &limitations_source,
                &limitations_target,
                &mut consumed_source,
                &mut consumed_target,
                ProgramObjectKindV5::Limitation,
                CandidateKeyKindV5::MappedLimitationSources,
                limitation_stage,
                node_working,
                |left, right| match (&left.data, &right.data) {
                    (
                        NodeDataV5::Limitation {
                            base_hash: a,
                            sources,
                            ..
                        },
                        NodeDataV5::Limitation {
                            base_hash: b,
                            sources: target_sources,
                            ..
                        },
                    ) => {
                        a == b && mapped_set(sources, &successors).as_ref() == Some(target_sources)
                    }
                    _ => false,
                },
            )?;
            if wave.is_empty() {
                break;
            }
            seeds.extend(wave);
            successors = successor_map(&seeds);
            limitation_stage = limitation_stage.checked_add(1).ok_or(M6Error::Incomplete {
                operation: "M6 limitation dependency stages",
                limit: usize::MAX,
                observed: usize::MAX,
            })?;
        }
        finish_unmatched(
            &limitations_source,
            &limitations_target,
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Limitation,
            limitation_stage,
            &mut seeds,
        );
        successors = successor_map(&seeds);

        seeds.sort_by(|left, right| {
            left.stage
                .cmp(&right.stage)
                .then_with(|| {
                    left.from_ids
                        .iter()
                        .next()
                        .cmp(&right.from_ids.iter().next())
                })
                .then_with(|| left.to_ids.iter().next().cmp(&right.to_ids.iter().next()))
        });
        bounded(seeds.len(), MAX_M6_MAPPINGS, "M6 program mappings")?;
        let mut mappings = Vec::new();
        mappings
            .try_reserve_exact(seeds.len())
            .map_err(|_| M6Error::Incomplete {
                operation: "M6 mapping allocation",
                limit: MAX_M6_MAPPINGS,
                observed: usize::MAX,
            })?;
        let mut owner = BTreeMap::<(MappingSideV5, StableId), (usize, StableId)>::new();
        for seed in &seeds {
            let status = component_status(seed, &source_by_id, &target_by_id, &successors);
            let mut predecessor_mapping_ids = BTreeSet::new();
            for (side, _id, node) in seed
                .from_ids
                .iter()
                .map(|id| (MappingSideV5::Source, id, source_by_id[id]))
                .chain(
                    seed.to_ids
                        .iter()
                        .map(|id| (MappingSideV5::Target, id, target_by_id[id])),
                )
            {
                for reference in node_references(node) {
                    if let Some((stage, mapping_id)) = owner.get(&(side, reference))
                        && *stage < seed.stage
                    {
                        predecessor_mapping_ids.insert(mapping_id.clone());
                    }
                }
            }
            let change_fact_ids = inputs
                .git_change_facts
                .iter()
                .filter(|fact| {
                    seed.from_ids.contains(&fact.source_artifact_id)
                        || seed.to_ids.contains(&fact.target_artifact_id)
                })
                .map(|fact| fact.fact_id.clone())
                .collect();
            let source_body_hashes = seed
                .from_ids
                .iter()
                .map(|id| {
                    IdBodyHashV5::new(
                        id.clone(),
                        normalized_program_body_hash(
                            source_by_id[id],
                            ProgramSideV5::Source,
                            &successors,
                        )?,
                    )
                })
                .collect::<M6Result<Vec<_>>>()?;
            let target_body_hashes = seed
                .to_ids
                .iter()
                .map(|id| {
                    IdBodyHashV5::new(
                        id.clone(),
                        normalized_program_body_hash(
                            target_by_id[id],
                            ProgramSideV5::Target,
                            &successors,
                        )?,
                    )
                })
                .collect::<M6Result<Vec<_>>>()?;
            let mapping = ProgramMappingV5::from_parts(ProgramMappingPartsV5 {
                source_closure_id: closure.id.clone(),
                source_snapshot_id: source.snapshot_id().clone(),
                target_snapshot_id: target.snapshot_id().clone(),
                object_kind: seed.object_kind,
                from_ids: seed.from_ids.clone(),
                to_ids: seed.to_ids.clone(),
                status,
                candidate_key_kind: seed.candidate_key_kind,
                source_body_hashes,
                target_body_hashes,
                change_fact_ids,
                predecessor_mapping_ids,
            })?;
            for id in &seed.from_ids {
                owner.insert(
                    (MappingSideV5::Source, id.clone()),
                    (seed.stage, mapping.id.clone()),
                );
            }
            for id in &seed.to_ids {
                owner.insert(
                    (MappingSideV5::Target, id.clone()),
                    (seed.stage, mapping.id.clone()),
                );
            }
            mappings.push(mapping);
        }
        mappings.sort_by(|left, right| left.id.cmp(&right.id));
        let source_domain = source_nodes.iter().map(|node| node.id.clone()).collect();
        let target_domain = target_nodes.iter().map(|node| node.id.clone()).collect();
        let morphism = Self::seal_derived(closure, &mappings, &source_domain, &target_domain)?;
        // Recursive logical ownership accounting for every retained working
        // collection that is simultaneously live at the seal boundary. This
        // is deliberately based on the concrete nodes/maps/sets, not wire
        // size multipliers.
        let kind_vector_bytes = [
            repositories_source.capacity(),
            repositories_target.capacity(),
            snapshots_source.capacity(),
            snapshots_target.capacity(),
            artifacts_source.capacity(),
            artifacts_target.capacity(),
            relations_source.capacity(),
            relations_target.capacity(),
            contexts_source.capacity(),
            contexts_target.capacity(),
            invariants_source.capacity(),
            invariants_target.capacity(),
            limitations_source.capacity(),
            limitations_target.capacity(),
        ]
        .into_iter()
        .fold(0_usize, usize::saturating_add)
        .saturating_mul(std::mem::size_of::<&ProgramNodeV5>());
        let observed_working_peak_upper_bound_bytes = [
            node_working,
            id_ref_map_heap(&source_by_id),
            id_ref_map_heap(&target_by_id),
            id_set_heap(&consumed_source),
            id_set_heap(&consumed_target),
            seeds.iter().map(ComponentSeedV5::allocated_bytes).sum(),
            successor_map_heap(&successors),
            owner_map_heap(&owner),
            mappings.iter().map(ProgramMappingV5::allocated_bytes).sum(),
            id_set_heap(&source_domain),
            id_set_heap(&target_domain),
            morphism.allocated_bytes(),
            id_set_heap(&copied_target_ids),
            kind_vector_bytes,
            candidate_scratch_upper_bound(source_nodes.len(), target_nodes.len()),
            MAX_M6_CANONICAL_BYTES.max(MAX_M6_MAPPING_DTO_BYTES),
        ]
        .into_iter()
        .try_fold(0_usize, checked_working_add)?;
        let working_peak_upper_bound_bytes = Self::mapping_reservation_bytes(source, target)?;
        if observed_working_peak_upper_bound_bytes > working_peak_upper_bound_bytes {
            return Err(M6Error::Incomplete {
                operation: "M6 mapping reservation underflow",
                limit: working_peak_upper_bound_bytes,
                observed: observed_working_peak_upper_bound_bytes,
            });
        }
        Ok(M6MappingPhaseV5 {
            mappings,
            morphism,
            working_peak_upper_bound_bytes,
        })
    }

    fn seal_derived(
        closure: &IncrementalSourceClosureV5,
        mappings: &[ProgramMappingV5],
        source_domain_ids: &BTreeSet<StableId>,
        target_domain_ids: &BTreeSet<StableId>,
    ) -> M6Result<Self> {
        bounded(mappings.len(), MAX_M6_MAPPINGS, "M6 program mappings")?;
        bounded(
            source_domain_ids.len(),
            MAX_M6_PROGRAM_DOMAIN_IDS,
            "M6 source domain",
        )?;
        bounded(
            target_domain_ids.len(),
            MAX_M6_PROGRAM_DOMAIN_IDS,
            "M6 target domain",
        )?;
        if mappings.is_empty() || source_domain_ids.is_empty() || target_domain_ids.is_empty() {
            return Err(M6Error::InvalidMapping(
                "mapping seal and both program domains must be nonempty",
            ));
        }
        if mappings.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            return Err(M6Error::InvalidMapping(
                "mapping seal input must be strictly ID ordered",
            ));
        }
        let mut owned_source = BTreeSet::new();
        let mut owned_target = BTreeSet::new();
        let mut status_counts = MappingStatusCountsV5::default();
        for mapping in mappings {
            if mapping.source_closure_id != *closure.id()
                || mapping.source_snapshot_id != closure.input.source_snapshot_id
                || mapping.target_snapshot_id != closure.input.target_snapshot_id
            {
                return Err(M6Error::InvalidMapping("mapping closure/snapshot mismatch"));
            }
            if mapping
                .from_ids
                .iter()
                .any(|id| !owned_source.insert(id.clone()))
                || mapping
                    .to_ids
                    .iter()
                    .any(|id| !owned_target.insert(id.clone()))
            {
                return Err(M6Error::InvalidMapping(
                    "a program object is owned by more than one component",
                ));
            }
            status_counts.record(mapping.status);
        }
        if owned_source != *source_domain_ids || owned_target != *target_domain_ids {
            return Err(M6Error::InvalidMapping(
                "mapping components must exactly cover both program domains",
            ));
        }
        let mapping_set_digest = crate::canonical::compact_json_array_sha256_streaming(
            mappings.iter().map(|mapping| {
                mapping
                    .body_hash()
                    .and_then(|hash| IdBodyHashV5::new(mapping.id.clone(), hash))
                    .map_err(|error| DomainError::Validation(error.to_string()))
            }),
        )?;
        let source_domain_digest = digest_ids(source_domain_ids)?;
        let target_domain_digest = digest_ids(target_domain_ids)?;
        let source_ids = std::iter::once(closure.id.clone()).collect();
        let identity = ChangeMorphismIdentityV5 {
            source_closure_id: closure.id(),
            repository_id: &closure.input.repository_id,
            source_snapshot_id: &closure.input.source_snapshot_id,
            target_snapshot_id: &closure.input.target_snapshot_id,
            mapping_policy_descriptor_id: PROGRAM_MAPPING_POLICY_V5,
            semantic_anchor_descriptor_id: RUST_SYMBOL_ANCHOR_V1,
            mapping_count: mappings.len() as u64,
            mapping_set_digest: &mapping_set_digest,
            source_domain_count: source_domain_ids.len() as u64,
            source_domain_digest: &source_domain_digest,
            target_domain_count: target_domain_ids.len() as u64,
            target_domain_digest: &target_domain_digest,
            status_counts: &status_counts,
            source_ids: &source_ids,
        };
        let id = derive("change-morphism-v5", &identity)?;
        let value = Self {
            schema: "reviewgraphen.change_morphism.v5",
            id,
            source_closure_id: closure.id.clone(),
            repository_id: closure.input.repository_id.clone(),
            source_snapshot_id: closure.input.source_snapshot_id.clone(),
            target_snapshot_id: closure.input.target_snapshot_id.clone(),
            mapping_policy_descriptor_id: PROGRAM_MAPPING_POLICY_V5,
            semantic_anchor_descriptor_id: RUST_SYMBOL_ANCHOR_V1,
            mapping_count: mappings.len() as u64,
            mapping_set_digest,
            source_domain_count: source_domain_ids.len() as u64,
            source_domain_digest,
            target_domain_count: target_domain_ids.len() as u64,
            target_domain_digest,
            status_counts,
            source_ids,
        };
        bounded_event_dto(&value, MAX_M6_MORPHISM_DTO_BYTES, "M6 morphism DTO bytes")?;
        Ok(value)
    }

    pub(crate) fn from_json_bytes(input: &[u8], expected: &Self) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_MORPHISM_DTO_BYTES,
            "M6 morphism JSON bytes",
        )?;
        let wire: ChangeMorphismWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.change_morphism.v5"
            || wire.mapping_policy_descriptor_id != PROGRAM_MAPPING_POLICY_V5
            || wire.semantic_anchor_descriptor_id != RUST_SYMBOL_ANCHOR_V1
            || wire.id != expected.id
            || wire.source_closure_id != expected.source_closure_id
            || wire.repository_id != expected.repository_id
            || wire.source_snapshot_id != expected.source_snapshot_id
            || wire.target_snapshot_id != expected.target_snapshot_id
            || wire.mapping_count != expected.mapping_count
            || wire.mapping_set_digest != expected.mapping_set_digest
            || wire.source_domain_count != expected.source_domain_count
            || wire.source_domain_digest != expected.source_domain_digest
            || wire.target_domain_count != expected.target_domain_count
            || wire.target_domain_digest != expected.target_domain_digest
            || wire.status_counts != expected.status_counts
            || wire.source_ids != expected.source_ids
            || crate::canonical_json(expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "morphism wire is not exact canonical replay content".to_owned(),
            ));
        }
        Ok(expected.clone())
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn source_closure_id(&self) -> &StableId {
        &self.source_closure_id
    }
    #[must_use]
    pub fn source_snapshot_id(&self) -> &StableId {
        &self.source_snapshot_id
    }
    #[must_use]
    pub fn target_snapshot_id(&self) -> &StableId {
        &self.target_snapshot_id
    }
    #[must_use]
    pub fn mapping_count(&self) -> u64 {
        self.mapping_count
    }
    #[must_use]
    pub fn mapping_set_digest(&self) -> &ContentHash {
        &self.mapping_set_digest
    }
    #[must_use]
    pub fn source_domain_count(&self) -> u64 {
        self.source_domain_count
    }
    #[must_use]
    pub fn source_domain_digest(&self) -> &ContentHash {
        &self.source_domain_digest
    }
    #[must_use]
    pub fn target_domain_count(&self) -> u64 {
        self.target_domain_count
    }
    #[must_use]
    pub fn target_domain_digest(&self) -> &ContentHash {
        &self.target_domain_digest
    }
    #[must_use]
    pub fn status_counts(&self) -> &MappingStatusCountsV5 {
        &self.status_counts
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObligationCorrespondenceEntryPartsV5 {
    morphism_id: StableId,
    from_obligation_ids: BTreeSet<StableId>,
    to_obligation_ids: BTreeSet<StableId>,
    status: MappingStatusV5,
    source_mapping_ids: BTreeSet<StableId>,
    predecessor_entry_ids: BTreeSet<StableId>,
    source_body_hashes: Vec<IdBodyHashV5>,
    target_body_hashes: Vec<IdBodyHashV5>,
}

#[derive(Serialize)]
struct ObligationCorrespondenceEntryIdentityV5<'a> {
    morphism_id: &'a StableId,
    from_obligation_ids: &'a BTreeSet<StableId>,
    to_obligation_ids: &'a BTreeSet<StableId>,
    status: MappingStatusV5,
    source_mapping_ids: &'a BTreeSet<StableId>,
    predecessor_entry_ids: &'a BTreeSet<StableId>,
    source_body_hashes: &'a Vec<IdBodyHashV5>,
    target_body_hashes: &'a Vec<IdBodyHashV5>,
}

/// One deterministic, exclusive source/target obligation component.
/// Callers cannot mint accepted-looking entries from inferred JSON fields:
///
/// ```compile_fail
/// use reviewgraphen_core::ObligationCorrespondenceEntryV5;
/// let _forged = ObligationCorrespondenceEntryV5 {};
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObligationCorrespondenceEntryV5 {
    schema: &'static str,
    id: StableId,
    morphism_id: StableId,
    from_obligation_ids: BTreeSet<StableId>,
    to_obligation_ids: BTreeSet<StableId>,
    status: MappingStatusV5,
    source_mapping_ids: BTreeSet<StableId>,
    predecessor_entry_ids: BTreeSet<StableId>,
    successor_obligation_ids: BTreeSet<StableId>,
    source_body_hashes: Vec<IdBodyHashV5>,
    target_body_hashes: Vec<IdBodyHashV5>,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObligationCorrespondenceEntryWireV5 {
    schema: String,
    id: StableId,
    morphism_id: StableId,
    from_obligation_ids: BTreeSet<StableId>,
    to_obligation_ids: BTreeSet<StableId>,
    status: MappingStatusV5,
    source_mapping_ids: BTreeSet<StableId>,
    predecessor_entry_ids: BTreeSet<StableId>,
    successor_obligation_ids: BTreeSet<StableId>,
    source_body_hashes: Vec<IdBodyHashV5>,
    target_body_hashes: Vec<IdBodyHashV5>,
    source_ids: BTreeSet<StableId>,
}

fn validate_obligation_component_shape(
    from: usize,
    to: usize,
    status: MappingStatusV5,
) -> M6Result<()> {
    let valid = match (from, to, status) {
        (0, 1, MappingStatusV5::Added) | (1, 0, MappingStatusV5::Removed) => true,
        (
            1,
            1,
            MappingStatusV5::Preserved | MappingStatusV5::Modified | MappingStatusV5::Unresolved,
        ) => true,
        (1, target, MappingStatusV5::Split) if target > 1 => true,
        (source, 1, MappingStatusV5::Merged) if source > 1 => true,
        (source, target, MappingStatusV5::Unresolved) if source > 1 && target > 1 => true,
        _ => false,
    };
    if !valid {
        return Err(M6Error::InvalidObligationUniverse(
            "correspondence status does not match exclusive component cardinality",
        ));
    }
    Ok(())
}

impl ObligationCorrespondenceEntryV5 {
    fn from_parts(input: ObligationCorrespondenceEntryPartsV5) -> M6Result<Self> {
        require_kind(&input.morphism_id, "change-morphism-v5", "morphism_id")?;
        bounded(
            input.from_obligation_ids.len(),
            MAX_M6_CORRESPONDENCE_SIDE_IDS,
            "M6 correspondence source obligations",
        )?;
        bounded(
            input.to_obligation_ids.len(),
            MAX_M6_CORRESPONDENCE_SIDE_IDS,
            "M6 correspondence target obligations",
        )?;
        bounded(
            input.source_mapping_ids.len(),
            MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS,
            "M6 correspondence source mappings",
        )?;
        bounded(
            input.predecessor_entry_ids.len(),
            MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS,
            "M6 correspondence predecessor entries",
        )?;
        if input
            .from_obligation_ids
            .iter()
            .chain(&input.to_obligation_ids)
            .any(|id| id.kind() != "obligation")
            || input
                .source_mapping_ids
                .iter()
                .any(|id| id.kind() != "program-mapping-v5")
            || input
                .predecessor_entry_ids
                .iter()
                .any(|id| id.kind() != "obligation-correspondence-entry-v5")
        {
            return Err(M6Error::InvalidObligationUniverse(
                "correspondence member/source IDs use the wrong namespace",
            ));
        }
        validate_obligation_component_shape(
            input.from_obligation_ids.len(),
            input.to_obligation_ids.len(),
            input.status,
        )?;
        validate_body_hash_records(
            "source_body_hashes",
            &input.from_obligation_ids,
            &input.source_body_hashes,
        )?;
        validate_body_hash_records(
            "target_body_hashes",
            &input.to_obligation_ids,
            &input.target_body_hashes,
        )?;
        let identity = ObligationCorrespondenceEntryIdentityV5 {
            morphism_id: &input.morphism_id,
            from_obligation_ids: &input.from_obligation_ids,
            to_obligation_ids: &input.to_obligation_ids,
            status: input.status,
            source_mapping_ids: &input.source_mapping_ids,
            predecessor_entry_ids: &input.predecessor_entry_ids,
            source_body_hashes: &input.source_body_hashes,
            target_body_hashes: &input.target_body_hashes,
        };
        let id = derive("obligation-correspondence-entry-v5", &identity)?;
        let successor_obligation_ids = if input.from_obligation_ids.is_empty() {
            BTreeSet::new()
        } else {
            input.to_obligation_ids.clone()
        };
        let source_ids = std::iter::once(input.morphism_id.clone())
            .chain(input.source_mapping_ids.iter().cloned())
            .chain(input.predecessor_entry_ids.iter().cloned())
            .chain(input.from_obligation_ids.iter().cloned())
            .chain(input.to_obligation_ids.iter().cloned())
            .collect();
        let value = Self {
            schema: "reviewgraphen.obligation_correspondence_entry.v5",
            id,
            morphism_id: input.morphism_id,
            from_obligation_ids: input.from_obligation_ids,
            to_obligation_ids: input.to_obligation_ids,
            status: input.status,
            source_mapping_ids: input.source_mapping_ids,
            predecessor_entry_ids: input.predecessor_entry_ids,
            successor_obligation_ids,
            source_body_hashes: input.source_body_hashes,
            target_body_hashes: input.target_body_hashes,
            source_ids,
        };
        bounded_event_dto(
            &value,
            MAX_M6_CORRESPONDENCE_DTO_BYTES,
            "M6 correspondence entry DTO bytes",
        )?;
        Ok(value)
    }

    fn from_json_bytes(input: &[u8]) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_CORRESPONDENCE_DTO_BYTES,
            "M6 correspondence entry JSON bytes",
        )?;
        let wire: ObligationCorrespondenceEntryWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.obligation_correspondence_entry.v5" {
            return Err(M6Error::InvalidWire(
                "wrong obligation correspondence entry schema".to_owned(),
            ));
        }
        let expected = Self::from_parts(ObligationCorrespondenceEntryPartsV5 {
            morphism_id: wire.morphism_id,
            from_obligation_ids: wire.from_obligation_ids,
            to_obligation_ids: wire.to_obligation_ids,
            status: wire.status,
            source_mapping_ids: wire.source_mapping_ids,
            predecessor_entry_ids: wire.predecessor_entry_ids,
            source_body_hashes: wire.source_body_hashes,
            target_body_hashes: wire.target_body_hashes,
        })?;
        if expected.id != wire.id
            || expected.successor_obligation_ids != wire.successor_obligation_ids
            || expected.source_ids != wire.source_ids
            || crate::canonical_json(&expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "correspondence entry wire is not exact canonical derived content".to_owned(),
            ));
        }
        Ok(expected)
    }

    fn allocated_bytes(&self) -> usize {
        let records = |values: &Vec<IdBodyHashV5>| {
            values
                .capacity()
                .saturating_mul(std::mem::size_of::<IdBodyHashV5>())
                .saturating_add(
                    values
                        .iter()
                        .map(|record| {
                            record
                                .id
                                .allocated_bytes()
                                .saturating_add(record.body_hash.allocated_bytes())
                        })
                        .sum::<usize>(),
                )
        };
        [
            std::mem::size_of::<Self>(),
            self.id.allocated_bytes(),
            self.morphism_id.allocated_bytes(),
            id_set_heap(&self.from_obligation_ids),
            id_set_heap(&self.to_obligation_ids),
            id_set_heap(&self.source_mapping_ids),
            id_set_heap(&self.predecessor_entry_ids),
            id_set_heap(&self.successor_obligation_ids),
            records(&self.source_body_hashes),
            records(&self.target_body_hashes),
            id_set_heap(&self.source_ids),
        ]
        .into_iter()
        .fold(0_usize, usize::saturating_add)
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn morphism_id(&self) -> &StableId {
        &self.morphism_id
    }
    #[must_use]
    pub fn from_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.from_obligation_ids
    }
    #[must_use]
    pub fn to_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.to_obligation_ids
    }
    #[must_use]
    pub fn status(&self) -> MappingStatusV5 {
        self.status
    }
    #[must_use]
    pub fn source_mapping_ids(&self) -> &BTreeSet<StableId> {
        &self.source_mapping_ids
    }
    #[must_use]
    pub fn predecessor_entry_ids(&self) -> &BTreeSet<StableId> {
        &self.predecessor_entry_ids
    }
    #[must_use]
    pub fn successor_obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.successor_obligation_ids
    }
    #[must_use]
    pub fn source_body_hashes(&self) -> &[IdBodyHashV5] {
        &self.source_body_hashes
    }
    #[must_use]
    pub fn target_body_hashes(&self) -> &[IdBodyHashV5] {
        &self.target_body_hashes
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

#[derive(Serialize)]
struct ObligationCorrespondenceIdentityV5<'a> {
    morphism_id: &'a StableId,
    source_universe_id: &'a StableId,
    target_universe_id: &'a StableId,
    policy_descriptor_id: &'static str,
    entry_count: u64,
    entry_set_digest: &'a ContentHash,
    source_domain_count: u64,
    source_domain_digest: &'a ContentHash,
    target_domain_count: u64,
    target_domain_digest: &'a ContentHash,
    status_counts: &'a MappingStatusCountsV5,
    source_ids: &'a BTreeSet<StableId>,
}

/// Seal over the complete, exclusively owned obligation correspondence phase.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObligationCorrespondenceV5 {
    schema: &'static str,
    id: StableId,
    morphism_id: StableId,
    source_universe_id: StableId,
    target_universe_id: StableId,
    policy_descriptor_id: &'static str,
    entry_count: u64,
    entry_set_digest: ContentHash,
    source_domain_count: u64,
    source_domain_digest: ContentHash,
    target_domain_count: u64,
    target_domain_digest: ContentHash,
    status_counts: MappingStatusCountsV5,
    source_ids: BTreeSet<StableId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObligationCorrespondenceWireV5 {
    schema: String,
    id: StableId,
    morphism_id: StableId,
    source_universe_id: StableId,
    target_universe_id: StableId,
    policy_descriptor_id: String,
    entry_count: u64,
    entry_set_digest: ContentHash,
    source_domain_count: u64,
    source_domain_digest: ContentHash,
    target_domain_count: u64,
    target_domain_digest: ContentHash,
    status_counts: MappingStatusCountsV5,
    source_ids: BTreeSet<StableId>,
}

impl ObligationCorrespondenceV5 {
    fn seal_derived(
        morphism: &ChangeMorphismV5,
        source_universe_id: &StableId,
        target_universe_id: &StableId,
        entries: &[ObligationCorrespondenceEntryV5],
        source_domain: &BTreeSet<StableId>,
        target_domain: &BTreeSet<StableId>,
    ) -> M6Result<Self> {
        if source_universe_id.kind() != "universe" || target_universe_id.kind() != "universe" {
            return Err(M6Error::InvalidObligationUniverse(
                "correspondence seal universe IDs use the wrong namespace",
            ));
        }
        bounded(
            entries.len(),
            MAX_M6_CORRESPONDENCE_ENTRIES,
            "M6 correspondence entries",
        )?;
        bounded(
            source_domain.len(),
            MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            "M6 source obligations",
        )?;
        bounded(
            target_domain.len(),
            MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            "M6 target obligations",
        )?;
        if entries.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            return Err(M6Error::InvalidObligationUniverse(
                "correspondence seal entries must be strictly ID ordered",
            ));
        }
        let mut owned_source = BTreeSet::new();
        let mut owned_target = BTreeSet::new();
        let mut status_counts = MappingStatusCountsV5::default();
        for entry in entries {
            if entry.morphism_id != *morphism.id()
                || entry
                    .from_obligation_ids
                    .iter()
                    .any(|id| !owned_source.insert(id.clone()))
                || entry
                    .to_obligation_ids
                    .iter()
                    .any(|id| !owned_target.insert(id.clone()))
            {
                return Err(M6Error::InvalidObligationUniverse(
                    "correspondence entry morphism/ownership mismatch",
                ));
            }
            status_counts.record(entry.status);
        }
        if owned_source != *source_domain || owned_target != *target_domain {
            return Err(M6Error::InvalidObligationUniverse(
                "correspondence entries must exactly cover both obligation domains",
            ));
        }
        let entry_set_digest =
            crate::canonical::compact_json_array_sha256_streaming(entries.iter().map(|entry| {
                entry
                    .body_hash()
                    .and_then(|hash| IdBodyHashV5::new(entry.id.clone(), hash))
                    .map_err(|error| DomainError::Validation(error.to_string()))
            }))?;
        let source_domain_digest = digest_ids(source_domain)?;
        let target_domain_digest = digest_ids(target_domain)?;
        let source_ids = BTreeSet::from([
            morphism.id().clone(),
            source_universe_id.clone(),
            target_universe_id.clone(),
        ]);
        let identity = ObligationCorrespondenceIdentityV5 {
            morphism_id: morphism.id(),
            source_universe_id,
            target_universe_id,
            policy_descriptor_id: OBLIGATION_CORRESPONDENCE_POLICY_V5,
            entry_count: entries.len() as u64,
            entry_set_digest: &entry_set_digest,
            source_domain_count: source_domain.len() as u64,
            source_domain_digest: &source_domain_digest,
            target_domain_count: target_domain.len() as u64,
            target_domain_digest: &target_domain_digest,
            status_counts: &status_counts,
            source_ids: &source_ids,
        };
        let id = derive("obligation-correspondence-v5", &identity)?;
        let value = Self {
            schema: "reviewgraphen.obligation_correspondence.v5",
            id,
            morphism_id: morphism.id().clone(),
            source_universe_id: source_universe_id.clone(),
            target_universe_id: target_universe_id.clone(),
            policy_descriptor_id: OBLIGATION_CORRESPONDENCE_POLICY_V5,
            entry_count: entries.len() as u64,
            entry_set_digest,
            source_domain_count: source_domain.len() as u64,
            source_domain_digest,
            target_domain_count: target_domain.len() as u64,
            target_domain_digest,
            status_counts,
            source_ids,
        };
        bounded_event_dto(
            &value,
            MAX_M6_CORRESPONDENCE_DTO_BYTES,
            "M6 correspondence seal DTO bytes",
        )?;
        Ok(value)
    }

    fn from_json_bytes(input: &[u8], expected: &Self) -> M6Result<Self> {
        preflight_event_line(input.len(), 1)?;
        bounded(
            input.len(),
            MAX_M6_CORRESPONDENCE_DTO_BYTES,
            "M6 correspondence seal JSON bytes",
        )?;
        let wire: ObligationCorrespondenceWireV5 = serde_json::from_slice(input)
            .map_err(|error| M6Error::InvalidWire(error.to_string()))?;
        if wire.schema != "reviewgraphen.obligation_correspondence.v5"
            || wire.policy_descriptor_id != OBLIGATION_CORRESPONDENCE_POLICY_V5
            || wire.id != expected.id
            || wire.morphism_id != expected.morphism_id
            || wire.source_universe_id != expected.source_universe_id
            || wire.target_universe_id != expected.target_universe_id
            || wire.entry_count != expected.entry_count
            || wire.entry_set_digest != expected.entry_set_digest
            || wire.source_domain_count != expected.source_domain_count
            || wire.source_domain_digest != expected.source_domain_digest
            || wire.target_domain_count != expected.target_domain_count
            || wire.target_domain_digest != expected.target_domain_digest
            || wire.status_counts != expected.status_counts
            || wire.source_ids != expected.source_ids
            || crate::canonical_json(expected)? != input
        {
            return Err(M6Error::InvalidWire(
                "correspondence seal wire is not exact canonical replay content".to_owned(),
            ));
        }
        Ok(expected.clone())
    }

    fn allocated_bytes(&self) -> usize {
        [
            std::mem::size_of::<Self>(),
            self.id.allocated_bytes(),
            self.morphism_id.allocated_bytes(),
            self.source_universe_id.allocated_bytes(),
            self.target_universe_id.allocated_bytes(),
            self.entry_set_digest.allocated_bytes(),
            self.source_domain_digest.allocated_bytes(),
            self.target_domain_digest.allocated_bytes(),
            id_set_heap(&self.source_ids),
        ]
        .into_iter()
        .fold(0_usize, usize::saturating_add)
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn morphism_id(&self) -> &StableId {
        &self.morphism_id
    }
    #[must_use]
    pub fn source_universe_id(&self) -> &StableId {
        &self.source_universe_id
    }
    #[must_use]
    pub fn target_universe_id(&self) -> &StableId {
        &self.target_universe_id
    }
    #[must_use]
    pub fn entry_count(&self) -> u64 {
        self.entry_count
    }
    #[must_use]
    pub fn entry_set_digest(&self) -> &ContentHash {
        &self.entry_set_digest
    }
    #[must_use]
    pub fn source_domain_count(&self) -> u64 {
        self.source_domain_count
    }
    #[must_use]
    pub fn source_domain_digest(&self) -> &ContentHash {
        &self.source_domain_digest
    }
    #[must_use]
    pub fn target_domain_count(&self) -> u64 {
        self.target_domain_count
    }
    #[must_use]
    pub fn target_domain_digest(&self) -> &ContentHash {
        &self.target_domain_digest
    }
    #[must_use]
    pub fn status_counts(&self) -> &MappingStatusCountsV5 {
        &self.status_counts
    }
    #[must_use]
    pub fn source_ids(&self) -> &BTreeSet<StableId> {
        &self.source_ids
    }
    pub fn body_hash(&self) -> M6Result<ContentHash> {
        body_hash(self)
    }
}

/// A `(kind, id)` key is required because claim assessments deliberately use
/// their claim ID; collapsing it with a claim would lose an ADR 0023 source
/// record.  This is input-only and carries no event or append authority.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct HistoricalRecordKeyV5<'a> {
    pub(crate) kind: HistoricalSourceRecordKindV4,
    pub(crate) id: &'a StableId,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct OwnedHistoricalRecordKeyV5 {
    kind: HistoricalSourceRecordKindV4,
    id: StableId,
}

/// Typed source body retained for an actual-successor predicate.  The reducer
/// must compare this concrete DTO, never a generic JSON rendering, prose, or
/// inferred StableId set.
#[derive(Clone)]
pub(crate) struct HistoricalRecordDescriptorV5<'a> {
    key: HistoricalRecordKeyV5<'a>,
    body_hash: Option<&'a ContentHash>,
    pinned_active_or_current: bool,
    value: HistoricalSourceRecordValueV4<'a>,
}

/// Inventory-wide, typed reverse indexes over the one pinned historical
/// prefix.  This is deliberately not a generic `StableId -> JSON` map: every
/// entry records the source record kind that owns the referenced ID, which is
/// essential for the claim/claim-assessment same-ID case.
struct HistoricalSourceInventoryV5 {
    program_ids: BTreeSet<StableId>,
    record_keys: BTreeSet<OwnedHistoricalRecordKeyV5>,
    plans_for_obligation: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    claims_for_evidence: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    reproducing_claims_for_evidence: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    bindings_for_evidence: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    v4_registrations_for_descriptor: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    section_traces_for_descriptor: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    coverage_contributors: BTreeMap<StableId, BTreeSet<OwnedHistoricalRecordKeyV5>>,
    coverage_obligation_for_claim: BTreeMap<StableId, StableId>,
}

/// Borrow-only lookup surface shared by the real retained inventory and the
/// allocation-free admission scanner.  It keeps the descriptor's explicit
/// predecessor vocabulary in one place while ensuring the first pass never
/// clones a StableId merely to count it.
trait HistoricalInventoryViewV5 {
    fn is_program_id(&self, id: &StableId) -> bool;
    fn visit_plans_for_obligations(
        &self,
        ids: &BTreeSet<StableId>,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
    fn visit_claims_for_evidence(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
    fn visit_bindings_for_evidence(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
    fn visit_v4_registrations_for_descriptor(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
    fn visit_section_traces_for_descriptor(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
    fn visit_typed_trace_sources(
        &self,
        ids: &[StableId],
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
    fn visit_coverage_contributors(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    );
}

fn target_metadata_inventory_reservation_v5(
    target: &TargetActualRecordInventoryV5<'_>,
    target_program: &ProgramSpace,
) -> M6Result<usize> {
    let mut occurrences = 0_usize;
    let mut id_bytes = 0_usize;
    let mut add_id = |id: &StableId| -> M6Result<()> {
        occurrences = occurrences.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 target metadata inventory occurrences",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        })?;
        id_bytes = id_bytes
            .checked_add(id.allocated_bytes())
            .ok_or(M6Error::Incomplete {
                operation: "M6 target metadata inventory ID bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        Ok(())
    };
    let mut error = None;
    target_program.visit_known_ids(|id| {
        if error.is_none() {
            error = add_id(id).err();
        }
    });
    if let Some(error) = error.take() {
        return Err(error);
    }
    target.try_visit_records(|record| {
        let mut visit = |id: &StableId| {
            if error.is_none() {
                error = add_id(id).err();
            }
        };
        visit(record.id()); // one canonical record key
        match record.value().historical_value() {
            HistoricalSourceRecordValueV4::ReviewPlan(plan) => {
                for wave in plan.waves() {
                    for id in wave.obligation_ids() {
                        visit(id);
                    }
                }
            }
            HistoricalSourceRecordValueV4::Claim(claim) if claim.obligation_ids().len() == 1 => {
                visit(claim.id()); // claim-to-obligation owner
                visit(claim.id()); // possible coverage contributor
            }
            HistoricalSourceRecordValueV4::ClaimAssessment(value) => visit(value.claim_id()),
            HistoricalSourceRecordValueV4::Evidence(value) => visit(value.id()),
            HistoricalSourceRecordValueV4::EvidenceBinding(binding) => {
                // all-relation claim/evidence reverse indexes plus the
                // reproduces-only coverage reverse index upper image.
                for id in [
                    binding.evidence_id(),
                    binding.claim_id(),
                    binding.evidence_id(),
                    binding.id(),
                    binding.evidence_id(),
                    binding.claim_id(),
                ] {
                    visit(id);
                }
            }
            HistoricalSourceRecordValueV4::Verification(value) => visit(value.claim_id()),
            HistoricalSourceRecordValueV4::Decision(value) => visit(value.claim_id()),
            HistoricalSourceRecordValueV4::Finding(value) => visit(value.claim_id()),
            HistoricalSourceRecordValueV4::ArtifactRegistrationV4(registration) => {
                if let ArtifactSourceV4::GluingInput { descriptor_id, .. } = registration.source() {
                    visit(descriptor_id);
                    visit(registration.id());
                }
            }
            HistoricalSourceRecordValueV4::Section(section) => {
                visit(section.projection_input_descriptor_id());
                visit(section.projection_claim_assessment_id());
                for ids in [
                    section.projection_binding_ids(),
                    section.projection_evidence_ids(),
                    section.projection_verification_ids(),
                    section.projection_decision_ids(),
                    section.projection_finding_ids(),
                ] {
                    for id in ids {
                        visit(id);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    if let Some(error) = error {
        return Err(error);
    }
    occurrences
        .checked_mul(1_024)
        .and_then(|bytes| bytes.checked_add(id_bytes.saturating_mul(4)))
        // Only one record's unique-union scratch is live at once.
        .and_then(|bytes| bytes.checked_add(MAX_M6_RECORD_METADATA_IDS * 1_024))
        .ok_or(M6Error::Incomplete {
            operation: "M6 target metadata inventory bytes",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        })
}

impl HistoricalSourceInventoryV5 {
    fn new(
        source: &HistoricalPrefixProjectionV4<'_>,
        source_program: &ProgramSpace,
    ) -> M6Result<Self> {
        let mut value = Self {
            program_ids: source_program.known_ids(),
            record_keys: BTreeSet::new(),
            plans_for_obligation: BTreeMap::new(),
            claims_for_evidence: BTreeMap::new(),
            reproducing_claims_for_evidence: BTreeMap::new(),
            bindings_for_evidence: BTreeMap::new(),
            v4_registrations_for_descriptor: BTreeMap::new(),
            section_traces_for_descriptor: BTreeMap::new(),
            coverage_contributors: BTreeMap::new(),
            coverage_obligation_for_claim: BTreeMap::new(),
        };
        source.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_projection(record);
            let key = OwnedHistoricalRecordKeyV5 {
                kind: descriptor.key().kind,
                id: descriptor.key().id.clone(),
            };
            if !value.record_keys.insert(key.clone()) {
                return Err(M6Error::InvalidSourceClosure(
                    "duplicate historical source (kind,id) inventory key",
                ));
            }
            match descriptor.typed_body() {
                HistoricalSourceRecordValueV4::ReviewPlan(plan) => {
                    for wave in plan.waves() {
                        for id in wave.obligation_ids() {
                            value
                                .plans_for_obligation
                                .entry(id.clone())
                                .or_default()
                                .insert(key.clone());
                        }
                    }
                }
                HistoricalSourceRecordValueV4::Claim(claim)
                    if claim.obligation_ids().len() == 1 =>
                {
                    // The coverage reducer accepts only one-obligation claims
                    // in the selected plan closure.  Index no claim outside
                    // an actual numerator; its plan/envelope/execution chain
                    // must not contaminate a coverage source closure.
                    let id = claim
                        .obligation_ids()
                        .iter()
                        .next()
                        .expect("one obligation checked above");
                    let coverage = source.coverage();
                    let contributes = coverage.evidence_supported_obligation_ids().contains(id)
                        || coverage.verified_obligation_ids().contains(id)
                        || coverage.fresh_obligation_ids().contains(id)
                        || coverage.human_accepted_obligation_ids().contains(id);
                    if contributes {
                        value
                            .coverage_obligation_for_claim
                            .insert(claim.id().clone(), id.clone());
                        value
                            .coverage_contributors
                            .entry(id.clone())
                            .or_default()
                            .insert(key.clone());
                    }
                }
                HistoricalSourceRecordValueV4::ClaimAssessment(assessment) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(assessment.claim_id())
                        .filter(|id| {
                            source
                                .coverage()
                                .human_accepted_obligation_ids()
                                .contains(*id)
                        })
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key.clone());
                    }
                }
                HistoricalSourceRecordValueV4::EvidenceBinding(binding) => {
                    let contributes = value
                        .coverage_obligation_for_claim
                        .get(binding.claim_id())
                        .is_some_and(|id| {
                            source
                                .coverage()
                                .evidence_supported_obligation_ids()
                                .contains(id)
                        })
                        && binding.relation() == crate::EvidenceRelationV3::Reproduces;
                    value
                        .claims_for_evidence
                        .entry(binding.evidence_id().clone())
                        .or_default()
                        .insert(OwnedHistoricalRecordKeyV5 {
                            kind: HistoricalSourceRecordKindV4::Claim,
                            id: binding.claim_id().clone(),
                        });
                    value
                        .bindings_for_evidence
                        .entry(binding.evidence_id().clone())
                        .or_default()
                        .insert(key.clone());
                    if contributes
                        && let Some(obligation_id) =
                            value.coverage_obligation_for_claim.get(binding.claim_id())
                    {
                        value
                            .reproducing_claims_for_evidence
                            .entry(binding.evidence_id().clone())
                            .or_default()
                            .insert(OwnedHistoricalRecordKeyV5 {
                                kind: HistoricalSourceRecordKindV4::Claim,
                                id: binding.claim_id().clone(),
                            });
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key.clone());
                    }
                }
                HistoricalSourceRecordValueV4::Evidence(_) => {}
                HistoricalSourceRecordValueV4::Verification(verification) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(verification.claim_id())
                        .filter(|id| source.coverage().verified_obligation_ids().contains(*id))
                        .filter(|_| verification.outcome() == VerificationOutcomeV3::Passed)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key.clone());
                    }
                }
                HistoricalSourceRecordValueV4::Decision(decision) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(decision.claim_id())
                        .filter(|id| {
                            source
                                .coverage()
                                .human_accepted_obligation_ids()
                                .contains(*id)
                        })
                        .filter(|_| decision.outcome() == DecisionOutcomeV3::Accept)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key.clone());
                    }
                }
                HistoricalSourceRecordValueV4::Finding(finding) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(finding.claim_id())
                        .filter(|id| {
                            source
                                .coverage()
                                .human_accepted_obligation_ids()
                                .contains(*id)
                        })
                        .filter(|_| finding.status() == FindingStatusV3::Accepted)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key.clone());
                    }
                }
                HistoricalSourceRecordValueV4::ArtifactRegistrationV4(registration) => {
                    if let ArtifactSourceV4::GluingInput { descriptor_id, .. } =
                        registration.source()
                    {
                        value
                            .v4_registrations_for_descriptor
                            .entry(descriptor_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::Section(section) => {
                    let traces = value
                        .section_traces_for_descriptor
                        .entry(section.projection_input_descriptor_id().clone())
                        .or_default();
                    traces.insert(OwnedHistoricalRecordKeyV5 {
                        kind: HistoricalSourceRecordKindV4::ClaimAssessment,
                        id: section.projection_claim_assessment_id().clone(),
                    });
                    for (kind, ids) in [
                        (
                            HistoricalSourceRecordKindV4::EvidenceBinding,
                            section.projection_binding_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Evidence,
                            section.projection_evidence_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Verification,
                            section.projection_verification_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Decision,
                            section.projection_decision_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Finding,
                            section.projection_finding_ids(),
                        ),
                    ] {
                        for id in ids {
                            traces.insert(OwnedHistoricalRecordKeyV5 {
                                kind,
                                id: id.clone(),
                            });
                        }
                    }
                }
                _ => {}
            }
            Ok::<(), M6Error>(())
        })?;
        // Evidence is listed before bindings in the canonical inventory.
        // Resolve its coverage ownership only after the complete binding
        // reverse index exists; this is a second borrow-only scan, not an
        // inferred StableId relation.
        source.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_projection(record);
            if let HistoricalSourceRecordValueV4::Evidence(evidence) = descriptor.typed_body()
                && let Some(claims) = value.reproducing_claims_for_evidence.get(evidence.id())
            {
                for claim_key in claims {
                    if let Some(obligation_id) =
                        value.coverage_obligation_for_claim.get(&claim_key.id)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(OwnedHistoricalRecordKeyV5 {
                                kind: descriptor.key().kind,
                                id: descriptor.key().id.clone(),
                            });
                    }
                }
            }
            Ok::<(), M6Error>(())
        })?;
        Ok(value)
    }

    fn new_target(
        target: &TargetActualRecordInventoryV5<'_>,
        target_program: &ProgramSpace,
    ) -> M6Result<Self> {
        let metadata_reservation =
            target_metadata_inventory_reservation_v5(target, target_program)?;
        let combined_reservation = usize::try_from(target.working_reservation_bytes())
            .unwrap_or(usize::MAX)
            .checked_add(metadata_reservation)
            .ok_or(M6Error::Incomplete {
                operation: "M6 target metadata combined working bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        if combined_reservation > MAX_M6_STALENESS_WORKING_BYTES {
            return Err(M6Error::Incomplete {
                operation: "M6 target metadata combined working bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: combined_reservation,
            });
        }
        let mut value = Self {
            program_ids: target_program.known_ids(),
            record_keys: BTreeSet::new(),
            plans_for_obligation: BTreeMap::new(),
            claims_for_evidence: BTreeMap::new(),
            reproducing_claims_for_evidence: BTreeMap::new(),
            bindings_for_evidence: BTreeMap::new(),
            v4_registrations_for_descriptor: BTreeMap::new(),
            section_traces_for_descriptor: BTreeMap::new(),
            coverage_contributors: BTreeMap::new(),
            coverage_obligation_for_claim: BTreeMap::new(),
        };
        target.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_target(&record);
            let key = OwnedHistoricalRecordKeyV5 {
                kind: descriptor.key().kind,
                id: descriptor.key().id.clone(),
            };
            if !value.record_keys.insert(key.clone()) {
                return Err(DomainError::HistoricalPrefixMismatch(
                    "duplicate target actual (kind,id) inventory key",
                ));
            }
            match descriptor.typed_body() {
                HistoricalSourceRecordValueV4::ReviewPlan(plan) => {
                    for wave in plan.waves() {
                        for id in wave.obligation_ids() {
                            value
                                .plans_for_obligation
                                .entry(id.clone())
                                .or_default()
                                .insert(key.clone());
                        }
                    }
                }
                HistoricalSourceRecordValueV4::Claim(claim)
                    if claim.obligation_ids().len() == 1 =>
                {
                    value.coverage_obligation_for_claim.insert(
                        claim.id().clone(),
                        claim
                            .obligation_ids()
                            .iter()
                            .next()
                            .expect("one obligation checked")
                            .clone(),
                    );
                }
                HistoricalSourceRecordValueV4::ArtifactRegistrationV4(registration) => {
                    if let ArtifactSourceV4::GluingInput { descriptor_id, .. } =
                        registration.source()
                    {
                        value
                            .v4_registrations_for_descriptor
                            .entry(descriptor_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::Section(section) => {
                    let traces = value
                        .section_traces_for_descriptor
                        .entry(section.projection_input_descriptor_id().clone())
                        .or_default();
                    traces.insert(OwnedHistoricalRecordKeyV5 {
                        kind: HistoricalSourceRecordKindV4::ClaimAssessment,
                        id: section.projection_claim_assessment_id().clone(),
                    });
                    for (kind, ids) in [
                        (
                            HistoricalSourceRecordKindV4::EvidenceBinding,
                            section.projection_binding_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Evidence,
                            section.projection_evidence_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Verification,
                            section.projection_verification_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Decision,
                            section.projection_decision_ids(),
                        ),
                        (
                            HistoricalSourceRecordKindV4::Finding,
                            section.projection_finding_ids(),
                        ),
                    ] {
                        for id in ids {
                            traces.insert(OwnedHistoricalRecordKeyV5 {
                                kind,
                                id: id.clone(),
                            });
                        }
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        let coverage = target.coverage();
        target.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_target(&record);
            let key = OwnedHistoricalRecordKeyV5 {
                kind: descriptor.key().kind,
                id: descriptor.key().id.clone(),
            };
            match descriptor.typed_body() {
                HistoricalSourceRecordValueV4::Claim(claim)
                    if claim.obligation_ids().len() == 1 =>
                {
                    let obligation_id = value
                        .coverage_obligation_for_claim
                        .get(claim.id())
                        .expect("one-obligation claim indexed in first pass");
                    if coverage
                        .evidence_supported_obligation_ids()
                        .contains(obligation_id)
                        || coverage.verified_obligation_ids().contains(obligation_id)
                        || coverage.fresh_obligation_ids().contains(obligation_id)
                        || coverage
                            .human_accepted_obligation_ids()
                            .contains(obligation_id)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::ClaimAssessment(assessment) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(assessment.claim_id())
                        .filter(|id| coverage.human_accepted_obligation_ids().contains(*id))
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::EvidenceBinding(binding) => {
                    value
                        .claims_for_evidence
                        .entry(binding.evidence_id().clone())
                        .or_default()
                        .insert(OwnedHistoricalRecordKeyV5 {
                            kind: HistoricalSourceRecordKindV4::Claim,
                            id: binding.claim_id().clone(),
                        });
                    value
                        .bindings_for_evidence
                        .entry(binding.evidence_id().clone())
                        .or_default()
                        .insert(key.clone());
                    if binding.relation() == crate::EvidenceRelationV3::Reproduces
                        && let Some(obligation_id) = value
                            .coverage_obligation_for_claim
                            .get(binding.claim_id())
                            .filter(|id| coverage.evidence_supported_obligation_ids().contains(*id))
                    {
                        value
                            .reproducing_claims_for_evidence
                            .entry(binding.evidence_id().clone())
                            .or_default()
                            .insert(OwnedHistoricalRecordKeyV5 {
                                kind: HistoricalSourceRecordKindV4::Claim,
                                id: binding.claim_id().clone(),
                            });
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::Verification(verification) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(verification.claim_id())
                        .filter(|id| coverage.verified_obligation_ids().contains(*id))
                        .filter(|_| verification.outcome() == VerificationOutcomeV3::Passed)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::Decision(decision) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(decision.claim_id())
                        .filter(|id| coverage.human_accepted_obligation_ids().contains(*id))
                        .filter(|_| decision.outcome() == DecisionOutcomeV3::Accept)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                HistoricalSourceRecordValueV4::Finding(finding) => {
                    if let Some(obligation_id) = value
                        .coverage_obligation_for_claim
                        .get(finding.claim_id())
                        .filter(|id| coverage.human_accepted_obligation_ids().contains(*id))
                        .filter(|_| finding.status() == FindingStatusV3::Accepted)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(key);
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        target.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_target(&record);
            if let HistoricalSourceRecordValueV4::Evidence(evidence) = descriptor.typed_body()
                && let Some(claims) = value.reproducing_claims_for_evidence.get(evidence.id())
            {
                for claim_key in claims {
                    if let Some(obligation_id) =
                        value.coverage_obligation_for_claim.get(&claim_key.id)
                        && coverage
                            .evidence_supported_obligation_ids()
                            .contains(obligation_id)
                    {
                        value
                            .coverage_contributors
                            .entry(obligation_id.clone())
                            .or_default()
                            .insert(OwnedHistoricalRecordKeyV5 {
                                kind: descriptor.key().kind,
                                id: descriptor.key().id.clone(),
                            });
                    }
                }
            }
            Ok(())
        })?;
        let realized = value.target_retained_bytes();
        if realized > metadata_reservation {
            return Err(M6Error::Incomplete {
                operation: "M6 target metadata realized retained bytes",
                limit: metadata_reservation,
                observed: realized,
            });
        }
        Ok(value)
    }

    fn target_retained_bytes(&self) -> usize {
        fn id_bytes(id: &StableId) -> usize {
            std::mem::size_of::<StableId>().saturating_add(id.allocated_bytes())
        }
        fn key_bytes(key: &OwnedHistoricalRecordKeyV5) -> usize {
            std::mem::size_of::<OwnedHistoricalRecordKeyV5>()
                .saturating_add(key.id.allocated_bytes())
                .saturating_add(128)
        }
        let mut total = std::mem::size_of::<Self>();
        for id in &self.program_ids {
            total = total.saturating_add(id_bytes(id)).saturating_add(128);
        }
        for key in &self.record_keys {
            total = total.saturating_add(key_bytes(key));
        }
        for map in [
            &self.plans_for_obligation,
            &self.claims_for_evidence,
            &self.reproducing_claims_for_evidence,
            &self.bindings_for_evidence,
            &self.v4_registrations_for_descriptor,
            &self.section_traces_for_descriptor,
            &self.coverage_contributors,
        ] {
            for (id, keys) in map {
                total = total.saturating_add(id_bytes(id)).saturating_add(128);
                for key in keys {
                    total = total.saturating_add(key_bytes(key));
                }
            }
        }
        for (claim, obligation) in &self.coverage_obligation_for_claim {
            total = total
                .saturating_add(id_bytes(claim))
                .saturating_add(id_bytes(obligation))
                .saturating_add(128);
        }
        total
    }

    fn is_program_id(&self, id: &StableId) -> bool {
        self.program_ids.contains(id)
    }

    fn visit_plans_for_obligations(
        &self,
        ids: &BTreeSet<StableId>,
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        for id in ids {
            if let Some(keys) = self.plans_for_obligation.get(id) {
                for key in keys {
                    visit(key);
                }
            }
        }
    }

    fn visit_claims_for_evidence(
        &self,
        id: &StableId,
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        if let Some(keys) = self.claims_for_evidence.get(id) {
            for key in keys {
                visit(key);
            }
        }
    }

    fn visit_bindings_for_evidence(
        &self,
        id: &StableId,
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        if let Some(keys) = self.bindings_for_evidence.get(id) {
            for key in keys {
                visit(key);
            }
        }
    }

    fn visit_v4_registrations_for_descriptor(
        &self,
        id: &StableId,
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        if let Some(keys) = self.v4_registrations_for_descriptor.get(id) {
            for key in keys {
                visit(key);
            }
        }
    }

    fn visit_section_traces_for_descriptor(
        &self,
        id: &StableId,
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        if let Some(keys) = self.section_traces_for_descriptor.get(id) {
            for key in keys {
                visit(key);
            }
        }
    }

    fn visit_typed_trace_sources(
        &self,
        ids: &[StableId],
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        // A decision's `source_ids` is audit provenance, not a generic
        // dependency list.  Only actual inventory members of the explicitly
        // allowed M4 trace kinds may enter this closure.
        for id in ids {
            for kind in [
                HistoricalSourceRecordKindV4::Claim,
                HistoricalSourceRecordKindV4::EvidenceBinding,
                HistoricalSourceRecordKindV4::Evidence,
                HistoricalSourceRecordKindV4::Verification,
            ] {
                let key = OwnedHistoricalRecordKeyV5 {
                    kind,
                    id: id.clone(),
                };
                if self.record_keys.contains(&key) {
                    // `key` is only a transient lookup.  Visit the canonical
                    // owned key in the inventory so no reverse-lookup Vec or
                    // StableId clone survives this traversal.
                    if let Some(existing) = self.record_keys.get(&key) {
                        visit(existing);
                    }
                }
            }
        }
    }

    fn visit_coverage_contributors(
        &self,
        id: &StableId,
        mut visit: impl FnMut(&OwnedHistoricalRecordKeyV5),
    ) {
        if let Some(keys) = self.coverage_contributors.get(id) {
            for key in keys {
                visit(key);
            }
        }
    }
}

impl HistoricalInventoryViewV5 for HistoricalSourceInventoryV5 {
    fn is_program_id(&self, id: &StableId) -> bool {
        self.is_program_id(id)
    }
    fn visit_plans_for_obligations(
        &self,
        ids: &BTreeSet<StableId>,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_plans_for_obligations(ids, |key| visit(key.kind, &key.id));
    }
    fn visit_claims_for_evidence(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_claims_for_evidence(id, |key| visit(key.kind, &key.id));
    }
    fn visit_bindings_for_evidence(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_bindings_for_evidence(id, |key| visit(key.kind, &key.id));
    }
    fn visit_v4_registrations_for_descriptor(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_v4_registrations_for_descriptor(id, |key| visit(key.kind, &key.id));
    }
    fn visit_section_traces_for_descriptor(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_section_traces_for_descriptor(id, |key| visit(key.kind, &key.id));
    }
    fn visit_typed_trace_sources(
        &self,
        ids: &[StableId],
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_typed_trace_sources(ids, |key| visit(key.kind, &key.id));
    }
    fn visit_coverage_contributors(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_coverage_contributors(id, |key| visit(key.kind, &key.id));
    }
}

/// Allocation-free counterpart of the retained inventory.  Its lookup
/// methods rescan the immutable V4 replay values instead of building maps;
/// admission is cold-path work and must prove the real sparse reference count
/// before the HPP/vector allocation is allowed.
struct HistoricalAdmissionInventoryV5<'a> {
    source: &'a HistoricalPrefixAdmissionV4<'a>,
}

impl HistoricalAdmissionInventoryV5<'_> {
    fn visit_records(
        &self,
        mut visit: impl FnMut(crate::event::HistoricalSourceRecordAdmissionV4<'_>),
    ) {
        match self.source.try_visit_replay_records(|record| {
            visit(record);
            Ok::<(), std::convert::Infallible>(())
        }) {
            Ok(()) => {}
            Err(never) => match never {},
        }
    }
}

impl HistoricalInventoryViewV5 for HistoricalAdmissionInventoryV5<'_> {
    fn is_program_id(&self, id: &StableId) -> bool {
        let mut found = false;
        self.source
            .program_space()
            .visit_known_ids(|candidate| found |= candidate == id);
        found
    }

    fn visit_plans_for_obligations(
        &self,
        ids: &BTreeSet<StableId>,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_records(|record| {
            if let HistoricalSourceRecordValueV4::ReviewPlan(plan) = record.value()
                && plan
                    .waves()
                    .iter()
                    .flat_map(|wave| wave.obligation_ids())
                    .any(|id| ids.contains(id))
            {
                visit(HistoricalSourceRecordKindV4::ReviewPlan, plan.id());
            }
        });
    }

    fn visit_claims_for_evidence(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_records(|record| {
            if let HistoricalSourceRecordValueV4::EvidenceBinding(binding) = record.value()
                && binding.evidence_id() == id
            {
                visit(HistoricalSourceRecordKindV4::Claim, binding.claim_id());
            }
        });
    }

    fn visit_bindings_for_evidence(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_records(|record| {
            if let HistoricalSourceRecordValueV4::EvidenceBinding(binding) = record.value()
                && binding.evidence_id() == id
            {
                visit(HistoricalSourceRecordKindV4::EvidenceBinding, binding.id());
            }
        });
    }

    fn visit_v4_registrations_for_descriptor(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_records(|record| {
            if let HistoricalSourceRecordValueV4::ArtifactRegistrationV4(registration) = record.value()
                && matches!(registration.source(), ArtifactSourceV4::GluingInput { descriptor_id, .. } if descriptor_id == id)
            {
                visit(HistoricalSourceRecordKindV4::ArtifactRegistrationV4, registration.id());
            }
        });
    }

    fn visit_section_traces_for_descriptor(
        &self,
        id: &StableId,
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        self.visit_records(|record| {
            let HistoricalSourceRecordValueV4::Section(section) = record.value() else {
                return;
            };
            if section.projection_input_descriptor_id() != id {
                return;
            }
            visit(
                HistoricalSourceRecordKindV4::ClaimAssessment,
                section.projection_claim_assessment_id(),
            );
            for (kind, ids) in [
                (
                    HistoricalSourceRecordKindV4::EvidenceBinding,
                    section.projection_binding_ids(),
                ),
                (
                    HistoricalSourceRecordKindV4::Evidence,
                    section.projection_evidence_ids(),
                ),
                (
                    HistoricalSourceRecordKindV4::Verification,
                    section.projection_verification_ids(),
                ),
                (
                    HistoricalSourceRecordKindV4::Decision,
                    section.projection_decision_ids(),
                ),
                (
                    HistoricalSourceRecordKindV4::Finding,
                    section.projection_finding_ids(),
                ),
            ] {
                for member in ids {
                    visit(kind, member);
                }
            }
        });
    }

    fn visit_typed_trace_sources(
        &self,
        ids: &[StableId],
        visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        for id in ids {
            self.visit_records(|record| {
                if record.id() != id {
                    return;
                }
                if matches!(
                    record.kind(),
                    HistoricalSourceRecordKindV4::Claim
                        | HistoricalSourceRecordKindV4::EvidenceBinding
                        | HistoricalSourceRecordKindV4::Evidence
                        | HistoricalSourceRecordKindV4::Verification
                ) {
                    visit(record.kind(), record.id());
                }
            });
        }
    }

    fn visit_coverage_contributors(
        &self,
        _id: &StableId,
        _visit: &mut dyn FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        // Coverage is handled as one bounded aggregate below; invoking this
        // scan per denominator member would turn an O(records) source into an
        // artificial O(records*denominator) admission cost.
    }
}

impl<'a> HistoricalRecordDescriptorV5<'a> {
    fn from_target(value: &'a TargetActualRecordV5<'_>) -> Self {
        Self {
            key: HistoricalRecordKeyV5 {
                kind: value.kind(),
                id: value.id(),
            },
            body_hash: Some(value.body_hash()),
            pinned_active_or_current: false,
            // Reuse the exact source-history typed descriptor vocabulary. A
            // target actual record therefore cannot acquire dependencies via
            // a second, drifting target-only match statement.
            value: value.value().historical_value(),
        }
    }
    fn from_admission(value: crate::event::HistoricalSourceRecordAdmissionV4<'a>) -> Self {
        Self {
            key: HistoricalRecordKeyV5 {
                kind: value.kind(),
                id: value.id(),
            },
            body_hash: None,
            pinned_active_or_current: value.pinned_active_or_current(),
            value: *value.value(),
        }
    }
    fn from_projection(value: HistoricalSourceRecordProjectionV4<'a>) -> Self {
        Self {
            key: HistoricalRecordKeyV5 {
                kind: value.kind(),
                id: value.id(),
            },
            // The hash is borrowed from the one pinned projection.  ContentHash
            // owns its canonical string; copying it here would allocate once
            // for the projection and again for each descriptor traversal.
            body_hash: Some(value.body_hash()),
            pinned_active_or_current: value.pinned_active_or_current(),
            value: *value.value(),
        }
    }
    pub(crate) const fn key(&self) -> HistoricalRecordKeyV5<'a> {
        self.key
    }
    pub(crate) const fn body_hash(&self) -> &ContentHash {
        match self.body_hash {
            Some(value) => value,
            None => panic!("historical admission descriptor has no body hash"),
        }
    }
    pub(crate) const fn pinned_active_or_current(&self) -> bool {
        self.pinned_active_or_current
    }
    pub(crate) const fn typed_body(&self) -> HistoricalSourceRecordValueV4<'a> {
        self.value
    }

    /// Visits direct ProgramSpace dependencies only.  Each source DTO kind is
    /// intentionally listed here; `source_ids` are not treated as a generic
    /// substitute for semantic dependencies.
    fn visit_direct_program_ids(
        &self,
        inventory: &impl HistoricalInventoryViewV5,
        mut visit: impl FnMut(&'a StableId),
    ) {
        // StableId kinds are namespace labels, not an M6 semantic classifier:
        // accepted ProgramSpace artifacts intentionally include file, test,
        // repository, snapshot and language-specific IDs.  An explicit field
        // becomes a Program dependency only if it is a member of this exact
        // pinned ProgramSpace inventory.
        let program = |id: &'a StableId, visit: &mut dyn FnMut(&'a StableId)| {
            if inventory.is_program_id(id) {
                visit(id);
            }
        };
        match self.value {
            HistoricalSourceRecordValueV4::Obligation(v) => {
                for id in v
                    .normalized_target_refs()
                    .iter()
                    .chain(v.normalized_context_ids())
                    .chain(v.normalized_source_ids())
                    .chain(v.generator_ids())
                {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::ContextEnvelope(v) => {
                for id in v
                    .candidate_source_ids()
                    .iter()
                    .chain(v.normalized_included_source_ids())
                {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::Claim(v) => {
                for id in v.target_refs().iter().chain(v.source_ids()) {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::Evidence(v) => {
                for id in v.subject_ids() {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::ArtifactRegistrationV3(v) => match v.source() {
                ArtifactSourceV3::SnapshotIngest { snapshot_id, .. } => {
                    program(snapshot_id, &mut visit);
                }
                ArtifactSourceV3::ExternalHarnessWitness {
                    repository_id,
                    snapshot_id,
                    test_artifact_id,
                    ..
                } => {
                    for id in [repository_id, snapshot_id, test_artifact_id] {
                        program(id, &mut visit);
                    }
                }
                ArtifactSourceV3::RunGenesis { .. }
                | ArtifactSourceV3::ReviewerExecution { .. }
                | ArtifactSourceV3::VerifierArtifact { .. } => {}
            },
            HistoricalSourceRecordValueV4::ArtifactRegistrationV4(v) => match v.source() {
                ArtifactSourceV4::SnapshotIngest { snapshot_id, .. } => {
                    program(snapshot_id, &mut visit);
                }
                ArtifactSourceV4::ExternalHarnessWitness {
                    repository_id,
                    snapshot_id,
                    test_artifact_id,
                    ..
                } => {
                    for id in [repository_id, snapshot_id, test_artifact_id] {
                        program(id, &mut visit);
                    }
                }
                ArtifactSourceV4::GluingInput {
                    context_id,
                    repository_id,
                    snapshot_id,
                    ..
                } => {
                    for id in [context_id, repository_id, snapshot_id] {
                        program(id, &mut visit);
                    }
                }
                ArtifactSourceV4::RunGenesis { .. }
                | ArtifactSourceV4::ReviewerExecution { .. }
                | ArtifactSourceV4::VerifierArtifact { .. } => {}
            },
            HistoricalSourceRecordValueV4::GluingInputDescriptor(v) => {
                for id in std::iter::once(v.context_id()).chain(v.qualification_source_ids()) {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::ContextCover(v) => {
                for id in v
                    .cover_domain_ids()
                    .iter()
                    .chain(v.projection_required_context_ids())
                {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::Section(v) => {
                for id in [v.context_id(), v.projection_invariant_id()] {
                    program(id, &mut visit);
                }
                for id in v.projection_qualification_source_ids() {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::Restriction(v) => {
                for id in v
                    .projection_context_pair()
                    .iter()
                    .chain(v.projection_overlap_member_ids())
                    .chain(v.projection_qualification_source_ids())
                {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::GlobalCandidate(v) => {
                program(v.projection_invariant_id(), &mut visit);
                for id in v.projection_qualification_source_ids() {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::GluingAttempt(v) => {
                program(v.projection_invariant_id(), &mut visit);
            }
            HistoricalSourceRecordValueV4::GluingObstruction(v) => {
                for id in v
                    .projection_conflicting_context_ids()
                    .iter()
                    .chain(v.projection_overlap_member_ids())
                    .chain(std::iter::once(v.projection_affected_invariant_id()))
                    .chain(v.projection_blocks())
                {
                    program(id, &mut visit);
                }
            }
            HistoricalSourceRecordValueV4::ReviewPlan(_)
            | HistoricalSourceRecordValueV4::Execution(_)
            | HistoricalSourceRecordValueV4::ClaimAssessment(_)
            | HistoricalSourceRecordValueV4::EvidenceBinding(_)
            | HistoricalSourceRecordValueV4::Verification(_)
            | HistoricalSourceRecordValueV4::Decision(_)
            | HistoricalSourceRecordValueV4::Finding(_)
            | HistoricalSourceRecordValueV4::Coverage(_) => {}
        }
    }

    /// Required predecessor records, expressed by explicit DTO fields.
    /// Ownership back-references in the M5 bundle are intentionally omitted.
    fn visit_required_records(
        &self,
        inventory: &impl HistoricalInventoryViewV5,
        mut visit: impl FnMut(HistoricalSourceRecordKindV4, &StableId),
    ) {
        let mut emit = |kind, id: &StableId| visit(kind, id);
        match self.value {
            HistoricalSourceRecordValueV4::Obligation(v) => {
                for id in v.normalized_depends_on() {
                    emit(HistoricalSourceRecordKindV4::Obligation, id);
                }
            }
            HistoricalSourceRecordValueV4::ReviewPlan(v) => {
                for wave in v.waves() {
                    for id in wave.obligation_ids() {
                        emit(HistoricalSourceRecordKindV4::Obligation, id);
                    }
                }
            }
            HistoricalSourceRecordValueV4::ContextEnvelope(v) => {
                for id in v.obligation_ids() {
                    emit(HistoricalSourceRecordKindV4::Obligation, id);
                }
                // Envelopes have no plan field.  The only legal association is
                // the inventory's actual plan/wave membership, never an ID
                // spelling or an inferred source_ids relation.
                inventory.visit_plans_for_obligations(v.obligation_ids(), &mut |kind, id| {
                    emit(kind, id);
                });
            }
            HistoricalSourceRecordValueV4::ArtifactRegistrationV3(v) => match v.source() {
                // Reviewer raw registration is owned by Execution.  Keeping
                // Execution -> registration is sufficient; the reverse edge
                // would make an artificial ownership cycle.
                ArtifactSourceV3::ReviewerExecution { .. } => {}
                ArtifactSourceV3::VerifierArtifact { claim_id, .. }
                | ArtifactSourceV3::ExternalHarnessWitness { claim_id, .. } => {
                    emit(HistoricalSourceRecordKindV4::Claim, claim_id);
                }
                ArtifactSourceV3::RunGenesis { .. } | ArtifactSourceV3::SnapshotIngest { .. } => {}
            },
            HistoricalSourceRecordValueV4::ArtifactRegistrationV4(v) => match v.source() {
                ArtifactSourceV4::ReviewerExecution { execution_id, .. } => {
                    emit(HistoricalSourceRecordKindV4::Execution, execution_id);
                }
                ArtifactSourceV4::VerifierArtifact { claim_id, .. }
                | ArtifactSourceV4::ExternalHarnessWitness { claim_id, .. } => {
                    emit(HistoricalSourceRecordKindV4::Claim, claim_id);
                }
                ArtifactSourceV4::GluingInput { plan_id, .. } => {
                    // Descriptor owns its materialized V4 registration. The
                    // reverse descriptor edge is excluded to retain a DAG.
                    emit(HistoricalSourceRecordKindV4::ReviewPlan, plan_id);
                }
                ArtifactSourceV4::RunGenesis { .. } | ArtifactSourceV4::SnapshotIngest { .. } => {}
            },
            HistoricalSourceRecordValueV4::Execution(v) => {
                emit(HistoricalSourceRecordKindV4::ReviewPlan, v.plan_id());
                emit(
                    HistoricalSourceRecordKindV4::ContextEnvelope,
                    v.envelope_id(),
                );
                for id in v.obligation_ids() {
                    emit(HistoricalSourceRecordKindV4::Obligation, id);
                }
                emit(
                    HistoricalSourceRecordKindV4::ArtifactRegistrationV3,
                    v.raw_artifact_registration_id(),
                );
            }
            HistoricalSourceRecordValueV4::Claim(v) => {
                emit(HistoricalSourceRecordKindV4::Execution, v.execution_id());
                for id in v.obligation_ids() {
                    emit(HistoricalSourceRecordKindV4::Obligation, id);
                }
            }
            HistoricalSourceRecordValueV4::ClaimAssessment(v) => {
                emit(HistoricalSourceRecordKindV4::Claim, v.claim_id());
                for id in v.binding_ids() {
                    emit(HistoricalSourceRecordKindV4::EvidenceBinding, id);
                }
                for id in v.evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                for id in v.decision_ids() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.finding_ids() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::Evidence(v) => {
                emit(
                    HistoricalSourceRecordKindV4::ArtifactRegistrationV3,
                    v.input_registration_id(),
                );
                emit(
                    HistoricalSourceRecordKindV4::ArtifactRegistrationV3,
                    v.output_registration_id(),
                );
                // A binding points back to evidence.  Resolve its real claim
                // through the inventory instead of adding a binding edge and
                // creating Evidence <-> EvidenceBinding ownership cycles.
                inventory.visit_claims_for_evidence(v.id(), &mut |kind, id| emit(kind, id));
            }
            HistoricalSourceRecordValueV4::EvidenceBinding(v) => {
                emit(HistoricalSourceRecordKindV4::Claim, v.claim_id());
                emit(HistoricalSourceRecordKindV4::Evidence, v.evidence_id());
            }
            HistoricalSourceRecordValueV4::Verification(v) => {
                emit(HistoricalSourceRecordKindV4::Claim, v.claim_id());
                for id in v.evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                emit(
                    HistoricalSourceRecordKindV4::ArtifactRegistrationV3,
                    v.input_registration_id(),
                );
                emit(
                    HistoricalSourceRecordKindV4::ArtifactRegistrationV3,
                    v.output_registration_id(),
                );
                for id in v.evidence_ids() {
                    inventory.visit_bindings_for_evidence(id, &mut |kind, id| emit(kind, id));
                }
            }
            HistoricalSourceRecordValueV4::Decision(v) => {
                emit(HistoricalSourceRecordKindV4::Claim, v.claim_id());
                inventory.visit_typed_trace_sources(v.source_ids(), &mut |kind, id| {
                    emit(kind, id);
                });
            }
            HistoricalSourceRecordValueV4::Finding(v) => {
                emit(HistoricalSourceRecordKindV4::Claim, v.claim_id());
                if let Some(id) = v.decision_id() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                if let Some(id) = v.supersedes_finding_id() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::GluingInputDescriptor(v) => {
                emit(HistoricalSourceRecordKindV4::ReviewPlan, v.plan_id());
                // Registration is the reverse owner of this descriptor.  It
                // is indexed rather than guessed from its StableId.
                inventory.visit_v4_registrations_for_descriptor(v.id(), &mut |kind, id| {
                    emit(kind, id);
                });
                inventory.visit_section_traces_for_descriptor(v.id(), &mut |kind, id| {
                    emit(kind, id);
                });
            }
            HistoricalSourceRecordValueV4::ContextCover(v) => {
                emit(HistoricalSourceRecordKindV4::ReviewPlan, v.plan_id());
                for id in v.selected_obligation_ids() {
                    emit(HistoricalSourceRecordKindV4::Obligation, id);
                }
            }
            HistoricalSourceRecordValueV4::Section(v) => {
                // Do not classify naked IDs by equality or StableId kind:
                // ClaimAssessment intentionally shares Claim's ID. Context is
                // a Program fact and consequently never a historical record.
                emit(
                    HistoricalSourceRecordKindV4::ContextCover,
                    v.projection_cover_id(),
                );
                emit(
                    HistoricalSourceRecordKindV4::Obligation,
                    v.projection_obligation_id(),
                );
                emit(HistoricalSourceRecordKindV4::Claim, v.projection_claim_id());
                emit(
                    HistoricalSourceRecordKindV4::ClaimAssessment,
                    v.projection_claim_assessment_id(),
                );
                emit(
                    HistoricalSourceRecordKindV4::GluingInputDescriptor,
                    v.projection_input_descriptor_id(),
                );
                emit(
                    HistoricalSourceRecordKindV4::ArtifactRegistrationV4,
                    v.projection_input_registration_id(),
                );
                for id in v.projection_binding_ids() {
                    emit(HistoricalSourceRecordKindV4::EvidenceBinding, id);
                }
                for id in v.projection_evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.projection_verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                for id in v.projection_decision_ids() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.projection_finding_ids() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::Restriction(v) => {
                emit(
                    HistoricalSourceRecordKindV4::Section,
                    v.projection_section_id(),
                );
                for id in v.projection_claim_ids() {
                    emit(HistoricalSourceRecordKindV4::Claim, id);
                }
                for id in v.projection_evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.projection_verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                for id in v.projection_decision_ids() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.projection_finding_ids() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::GluingAttempt(v) => {
                emit(
                    HistoricalSourceRecordKindV4::ContextCover,
                    v.projection_cover_id(),
                );
                for id in v.projection_input_descriptor_ids() {
                    emit(HistoricalSourceRecordKindV4::GluingInputDescriptor, id);
                }
                for id in v.projection_section_ids() {
                    emit(HistoricalSourceRecordKindV4::Section, id);
                }
                for id in v.projection_restriction_ids() {
                    emit(HistoricalSourceRecordKindV4::Restriction, id);
                }
                if let Some(id) = v.projection_global_candidate_id() {
                    emit(HistoricalSourceRecordKindV4::GlobalCandidate, id);
                }
                // Obstruction points to its owner attempt.  The reverse option
                // is an explicit ownership back-reference and is omitted.
                for id in v.projection_claim_ids() {
                    emit(HistoricalSourceRecordKindV4::Claim, id);
                }
                for id in v.projection_evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.projection_verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                for id in v.projection_decision_ids() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.projection_finding_ids() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::GlobalCandidate(v) => {
                emit(
                    HistoricalSourceRecordKindV4::ContextCover,
                    v.projection_cover_id(),
                );
                for id in v.projection_required_section_ids() {
                    emit(HistoricalSourceRecordKindV4::Section, id);
                }
                for id in v.projection_restriction_ids() {
                    emit(HistoricalSourceRecordKindV4::Restriction, id);
                }
                for id in v.projection_claim_ids() {
                    emit(HistoricalSourceRecordKindV4::Claim, id);
                }
                for id in v.projection_evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.projection_verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                for id in v.projection_decision_ids() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.projection_finding_ids() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::GluingObstruction(v) => {
                emit(
                    HistoricalSourceRecordKindV4::GluingAttempt,
                    v.projection_attempt_id(),
                );
                for id in v.projection_section_ids() {
                    emit(HistoricalSourceRecordKindV4::Section, id);
                }
                for id in v.projection_claim_ids() {
                    emit(HistoricalSourceRecordKindV4::Claim, id);
                }
                for id in v.projection_evidence_ids() {
                    emit(HistoricalSourceRecordKindV4::Evidence, id);
                }
                for id in v.projection_verification_ids() {
                    emit(HistoricalSourceRecordKindV4::Verification, id);
                }
                for id in v.projection_decision_ids() {
                    emit(HistoricalSourceRecordKindV4::Decision, id);
                }
                for id in v.projection_finding_ids() {
                    emit(HistoricalSourceRecordKindV4::Finding, id);
                }
            }
            HistoricalSourceRecordValueV4::Coverage(v) => {
                // The denominator is an obligation universe.  Numerators are
                // expanded from the aggregate's actual current M4 ownership
                // through inventory reverse indexes, never from an implied
                // state-axis relation.
                for id in v.denominator_obligation_ids() {
                    emit(HistoricalSourceRecordKindV4::Obligation, id);
                    inventory.visit_coverage_contributors(id, &mut |kind, id| emit(kind, id));
                }
            }
        }
    }
}

/// M6-facing target descriptor stream. The descriptor and its body hash are
/// callback-scoped because coverage is freshly reduced by Core and is never
/// retained as a second target topology.
impl TargetActualRecordProjectionV5<'_, '_> {
    pub(crate) fn try_visit_historical_descriptors(
        &self,
        mut visitor: impl for<'record> FnMut(HistoricalRecordDescriptorV5<'record>) -> M6Result<()>,
    ) -> M6Result<()> {
        let target = self.materialize_inventory_v5()?;
        let mut callback_error = None;
        let result = target.try_visit_records(|record| {
            if let Err(error) = visitor(HistoricalRecordDescriptorV5::from_target(&record)) {
                callback_error = Some(error);
                return Err(DomainError::HistoricalPrefixMismatch(
                    "M6 target descriptor visitor aborted",
                ));
            }
            Ok(())
        });
        callback_error.map_or_else(|| result.map_err(M6Error::from), Err)
    }

    /// Visits each target body and the exact Program/predecessor metadata
    /// defined by the source-history descriptor. Reverse edges are resolved
    /// only by scanning actual target DTO fields; mappings and
    /// correspondences never participate in this inventory.
    pub(crate) fn try_visit_historical_metadata(
        &self,
        mut visit_record: impl for<'record> FnMut(
            &HistoricalRecordDescriptorV5<'record>,
        ) -> M6Result<()>,
        mut visit_program_dependency: impl FnMut(HistoricalRecordKeyV5<'_>, &StableId) -> M6Result<()>,
        mut visit_required_record: impl FnMut(
            HistoricalRecordKeyV5<'_>,
            HistoricalSourceRecordKindV4,
            &StableId,
        ) -> M6Result<()>,
    ) -> M6Result<()> {
        let target = self.materialize_inventory_v5()?;
        let inventory = HistoricalSourceInventoryV5::new_target(&target, self.program_space())?;
        let mut callback_error = None;
        let result = target.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_target(&record);
            if let Err(error) = visit_record(&descriptor) {
                callback_error = Some(error);
                return Err(DomainError::HistoricalPrefixMismatch(
                    "M6 target metadata visitor aborted",
                ));
            }
            let key = descriptor.key();
            // Check membership and the exact +1 boundary before cloning each
            // new StableId. Thus an over-limit input never allocates storage
            // for its 513th unique dependency, while duplicates remain free.
            let mut dependency_ids = BTreeSet::new();
            let mut program_ids = BTreeSet::new();
            let mut required_keys = BTreeSet::new();
            let mut error = None;
            descriptor.visit_direct_program_ids(&inventory, |id| {
                if error.is_some() {
                    return;
                }
                if !dependency_ids.contains(id) {
                    if dependency_ids.len() == MAX_M6_RECORD_METADATA_IDS {
                        error = Some(M6Error::Incomplete {
                            operation: "M6 target record metadata IDs",
                            limit: MAX_M6_RECORD_METADATA_IDS,
                            observed: MAX_M6_RECORD_METADATA_IDS + 1,
                        });
                        return;
                    }
                    dependency_ids.insert(id.clone());
                }
                program_ids.insert(id.clone());
            });
            descriptor.visit_required_records(&inventory, |kind, id| {
                if error.is_some() {
                    return;
                }
                if !dependency_ids.contains(id) {
                    if dependency_ids.len() == MAX_M6_RECORD_METADATA_IDS {
                        error = Some(M6Error::Incomplete {
                            operation: "M6 target record metadata IDs",
                            limit: MAX_M6_RECORD_METADATA_IDS,
                            observed: MAX_M6_RECORD_METADATA_IDS + 1,
                        });
                        return;
                    }
                    dependency_ids.insert(id.clone());
                }
                required_keys.insert((kind, id.clone()));
            });
            for id in &program_ids {
                if error.is_none()
                    && let Err(value) = visit_program_dependency(key, id)
                {
                    error = Some(value);
                }
            }
            for (kind, id) in &required_keys {
                if error.is_none()
                    && let Err(value) = visit_required_record(key, *kind, id)
                {
                    error = Some(value);
                }
            }
            if let Some(value) = error {
                callback_error = Some(value);
            }
            if callback_error.is_some() {
                Err(DomainError::HistoricalPrefixMismatch(
                    "M6 target metadata visitor aborted",
                ))
            } else {
                Ok(())
            }
        });
        callback_error.map_or_else(|| result.map_err(M6Error::from), Err)
    }
}

/// The sole authority-free handoff to the later reducer.  It borrows both
/// tails and all accepted phases; callers cannot pass record lists or mint a
/// replacement source topology.
pub(crate) struct IncrementalStalenessInputV5<'a> {
    source: HistoricalPrefixProjectionV4<'a>,
    closure: &'a IncrementalSourceClosureV5,
    mapping: &'a M6MappingPhaseV5,
    correspondence: &'a M6ObligationCorrespondencePhaseV5,
    target: &'a V5PreIncrementalStructuralPrefixProjection<'a>,
    inventory: HistoricalSourceInventoryV5,
}

#[cfg(test)]
thread_local! {
    static STALENESS_INPUT_MATERIALIZATIONS_V5: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_staleness_input_materializations_v5_for_test() {
    STALENESS_INPUT_MATERIALIZATIONS_V5.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn staleness_input_materializations_v5_for_test() -> usize {
    STALENESS_INPUT_MATERIALIZATIONS_V5.with(std::cell::Cell::get)
}

#[derive(Clone, Copy, Debug, Default)]
struct HistoricalAdmissionReferenceStatsV5 {
    occurrences: usize,
    id_bytes: usize,
}

fn historical_admission_reference_stats_v5(
    source: &HistoricalPrefixAdmissionV4<'_>,
) -> M6Result<HistoricalAdmissionReferenceStatsV5> {
    fn add(stats: &mut HistoricalAdmissionReferenceStatsV5, id: &StableId) -> M6Result<()> {
        stats.occurrences = stats
            .occurrences
            .checked_add(1)
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness historical reference count",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        stats.id_bytes =
            stats
                .id_bytes
                .checked_add(id.allocated_bytes())
                .ok_or(M6Error::Incomplete {
                    operation: "M6 staleness historical reference ID bytes",
                    limit: MAX_M6_STALENESS_WORKING_BYTES,
                    observed: usize::MAX,
                })?;
        Ok(())
    }

    let inventory = HistoricalAdmissionInventoryV5 { source };
    let mut stats = HistoricalAdmissionReferenceStatsV5::default();
    let mut failed = None;
    source.try_visit_replay_records(|record| {
        let descriptor = HistoricalRecordDescriptorV5::from_admission(record);
        descriptor.visit_required_records(&inventory, |_, id| {
            if failed.is_none() {
                failed = add(&mut stats, id).err();
            }
        });
        Ok::<(), M6Error>(())
    })?;
    if let Some(error) = failed {
        return Err(error);
    }
    // HPP's synthetic coverage member has one denominator predecessor for
    // each actual universe obligation.  A coverage contributor is a typed
    // historical record key, so no source record can contribute more than
    // once to its unique obligation axis; charge every replay record once as
    // the tight allocation-free upper image without deriving coverage sets.
    for id in source.universe_obligation_ids() {
        add(&mut stats, id)?;
    }
    source.try_visit_replay_records(|record| {
        add(&mut stats, record.id())?;
        Ok::<(), M6Error>(())
    })?;
    Ok(stats)
}

/// Allocation-free combined resident-set admission for the historical reducer.
/// Mapping/correspondence *construction* peaks are intentionally not added:
/// those phases are already sealed.  The source projection's own oracle is
/// charged separately; this function covers only currently retained sealed
/// DTO ownership, target predecessor, and every inventory/topology collection
/// that this constructor can materialize.
fn staleness_input_external_reservation_bytes(
    source: &HistoricalPrefixAdmissionV4<'_>,
    mapping: &M6MappingPhaseV5,
    correspondence: &M6ObligationCorrespondencePhaseV5,
    target: &V5PreIncrementalStructuralPrefixProjection<'_>,
) -> M6Result<usize> {
    fn add(total: usize, value: usize) -> M6Result<usize> {
        total.checked_add(value).ok_or(M6Error::Incomplete {
            operation: "M6 staleness combined working bytes",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        })
    }
    fn mul(left: usize, right: usize) -> M6Result<usize> {
        left.checked_mul(right).ok_or(M6Error::Incomplete {
            operation: "M6 staleness combined working bytes",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        })
    }

    let record_count = source.record_count();
    let mut program_id_count = 0_usize;
    let mut program_id_bytes = 0_usize;
    let mut program_id_overflow = false;
    source.program_space().visit_known_ids(|id| {
        program_id_count = program_id_count.checked_add(1).unwrap_or_else(|| {
            program_id_overflow = true;
            usize::MAX
        });
        program_id_bytes = program_id_bytes
            .checked_add(id.allocated_bytes())
            .unwrap_or_else(|| {
                program_id_overflow = true;
                usize::MAX
            });
    });
    if program_id_overflow {
        return Err(M6Error::Incomplete {
            operation: "M6 staleness Program ID accounting",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        });
    }
    // The no-allocation pass walks the actual typed predecessor vocabulary,
    // so sparse legal histories reserve their observed references rather
    // than a record-count-times-maximum fiction.
    let reference_stats = historical_admission_reference_stats_v5(source)?;
    let key_slot = std::mem::size_of::<OwnedHistoricalRecordKeyV5>();
    let inventory_bytes = [
        mul(record_count, key_slot)?,
        source.record_id_bytes(),
        // Reverse maps retain one owner key and one referenced key per
        // appearance.  Topology retains the same reference in `edges`,
        // `dependents`, and Kahn's indegree/frontier.  Each is charged with a
        // concrete key slot; dynamic StableId storage is charged below.
        mul(
            reference_stats.occurrences,
            key_slot.checked_mul(5).ok_or(M6Error::Incomplete {
                operation: "M6 staleness inventory slot bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?,
        )?,
        mul(reference_stats.id_bytes, 5)?,
        mul(program_id_count, std::mem::size_of::<StableId>())?,
        program_id_bytes,
        // The identity map, indegree table and ready frontier each own at
        // most one key per historical record.
        mul(
            record_count,
            key_slot.checked_mul(3).ok_or(M6Error::Incomplete {
                operation: "M6 staleness topology slot bytes",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?,
        )?,
    ]
    .into_iter()
    .try_fold(0_usize, add)?;
    let (mapping_retained, correspondence_retained) =
        staleness_mapping_correspondence_retained_bytes(mapping, correspondence)?;
    // Event owns the replay backing type, so it supplies one capacity-aware
    // retained oracle rather than letting this reducer accidentally omit the
    // duplicate plan, registration Vec spare slots, or run/tail scalars.
    let target_retained = target.retained_bytes_for_m6().map_err(M6Error::from)?;
    let total = [
        mapping_retained,
        correspondence_retained,
        target_retained,
        inventory_bytes,
        MAX_M6_CANONICAL_BYTES,
    ]
    .into_iter()
    .try_fold(0_usize, add)?;
    bounded(
        total,
        MAX_M6_STALENESS_WORKING_BYTES,
        "M6 staleness external working bytes",
    )?;
    Ok(total)
}

fn staleness_mapping_correspondence_retained_bytes(
    mapping: &M6MappingPhaseV5,
    correspondence: &M6ObligationCorrespondencePhaseV5,
) -> M6Result<(usize, usize)> {
    fn add(total: usize, value: usize) -> M6Result<usize> {
        total.checked_add(value).ok_or(M6Error::Incomplete {
            operation: "M6 staleness phase retained bytes",
            limit: MAX_M6_STALENESS_WORKING_BYTES,
            observed: usize::MAX,
        })
    }
    let mapping_retained = mapping
        .mappings()
        .iter()
        .try_fold(mapping.morphism().allocated_bytes(), |total, item| {
            add(total, item.allocated_bytes())
        })?;
    let mapping_retained = add(
        mapping_retained,
        mapping
            .mappings_capacity()
            .checked_mul(std::mem::size_of::<ProgramMappingV5>())
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness mapping vector slots",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?,
    )?;
    let correspondence_retained = correspondence.entries().iter().try_fold(
        correspondence.correspondence().allocated_bytes(),
        |total, item| add(total, item.allocated_bytes()),
    )?;
    let correspondence_retained = add(
        correspondence_retained,
        correspondence
            .entries_capacity()
            .checked_mul(std::mem::size_of::<ObligationCorrespondenceEntryV5>())
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness correspondence vector slots",
                limit: MAX_M6_STALENESS_WORKING_BYTES,
                observed: usize::MAX,
            })?,
    )?;
    Ok((mapping_retained, correspondence_retained))
}

/// Splits the fixed M6 process budget into the already-live external inputs
/// and the source-only historical projection.  The source projection oracle
/// includes source replay ownership; this function never charges it again.
fn staleness_source_working_limit(
    working_limit: usize,
    source_reservation: usize,
    external_reservation: usize,
) -> M6Result<usize> {
    let source_limit =
        working_limit
            .checked_sub(external_reservation)
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness combined working bytes",
                limit: working_limit,
                observed: usize::MAX,
            })?;
    let combined_reservation =
        source_reservation
            .checked_add(external_reservation)
            .ok_or(M6Error::Incomplete {
                operation: "M6 staleness combined working bytes",
                limit: working_limit,
                observed: usize::MAX,
            })?;
    bounded(
        combined_reservation,
        working_limit,
        "M6 staleness combined working bytes",
    )?;
    Ok(source_limit)
}

/// Checks the sealed mapping's complete ProgramSpace domains against the two
/// actual pinned spaces.  This runs before HPP materialization; a matching
/// snapshot ID is deliberately insufficient because it does not commit to the
/// accepted fact set.
fn validate_mapping_program_domains(
    mapping: &M6MappingPhaseV5,
    source: &ProgramSpace,
    target: &ProgramSpace,
) -> M6Result<()> {
    let (source_domain_count, source_domain_digest) = source.m6_known_id_domain()?;
    let (target_domain_count, target_domain_digest) = target.m6_known_id_domain()?;
    let expected_source_count =
        usize::try_from(mapping.morphism().source_domain_count()).map_err(|_| {
            M6Error::InvalidHistoricalTopology(
                "mapping source ProgramSpace domain count is not representable",
            )
        })?;
    let expected_target_count =
        usize::try_from(mapping.morphism().target_domain_count()).map_err(|_| {
            M6Error::InvalidHistoricalTopology(
                "mapping target ProgramSpace domain count is not representable",
            )
        })?;
    if source_domain_count != expected_source_count
        || &source_domain_digest != mapping.morphism().source_domain_digest()
        || target_domain_count != expected_target_count
        || &target_domain_digest != mapping.morphism().target_domain_digest()
    {
        return Err(M6Error::InvalidHistoricalTopology(
            "mapping ProgramSpace domain does not equal the pinned source/target facts",
        ));
    }
    Ok(())
}

impl<'a> IncrementalStalenessInputV5<'a> {
    /// Creates the only reducer handoff from exact replay-owned inputs. No
    /// generic record list, raw JSON, or caller-provided provenance set can
    /// replace the pinned source topology.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        source_log: &'a EventLogV4,
        closure: &'a IncrementalSourceClosureV5,
        mapping: &'a M6MappingPhaseV5,
        correspondence: &'a M6ObligationCorrespondencePhaseV5,
        target: &'a V5PreIncrementalStructuralPrefixProjection<'a>,
    ) -> M6Result<Self> {
        Self::new_with_working_limit(
            source_log,
            closure,
            mapping,
            correspondence,
            target,
            MAX_M6_STALENESS_WORKING_BYTES,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_with_working_limit(
        source_log: &'a EventLogV4,
        closure: &'a IncrementalSourceClosureV5,
        mapping: &'a M6MappingPhaseV5,
        correspondence: &'a M6ObligationCorrespondencePhaseV5,
        target: &'a V5PreIncrementalStructuralPrefixProjection<'a>,
        working_limit: usize,
    ) -> M6Result<Self> {
        // This must remain the first operation: HPP generation, `known_ids`,
        // inventory maps, topology edges and reducer scratch all allocate.
        // The V4 admission and every retained external input are observed only
        // through allocation-free accessors.  The source oracle already
        // includes replay ownership, so it is added exactly once here and is
        // deliberately excluded from the external reservation below.
        let source_admission = source_log.historical_prefix_admission_v4()?;
        if source_log.tail_hash() != closure.source_tail_hash()
            || source_log.run_id() != &closure.input.source_run_id
            || source_admission.program_space().snapshot_id() != closure.source_snapshot_id()
            || target.tail_hash() != closure.target_predecessor_tail_hash()
            || target.run_id() != &closure.input.target_run_id
            || target.genesis_hash() != &closure.input.target_genesis_hash
            || target.predecessor_offset() != closure.input.target_predecessor_offset
            || target.predecessor_event_count() != closure.input.target_predecessor_event_count
            || target.program_space().snapshot_id() != closure.target_snapshot_id()
            || target.universe().id() != closure.target_universe_id()
        {
            return Err(M6Error::InvalidHistoricalTopology(
                "source/target replay coordinates or snapshot do not equal closure",
            ));
        }
        let accounting = target.accounting();
        if accounting.canonical_prefix_bytes() != target.predecessor_offset()
            || accounting.event_count() != target.predecessor_event_count()
            || accounting.retained_envelope_bytes() == 0
        {
            return Err(M6Error::InvalidHistoricalTopology(
                "target structural replay accounting does not bind its predecessor coordinates",
            ));
        }
        if mapping.morphism().source_closure_id() != closure.id()
            || mapping.morphism().source_snapshot_id() != closure.source_snapshot_id()
            || mapping.morphism().target_snapshot_id() != closure.target_snapshot_id()
            || correspondence.correspondence().morphism_id() != mapping.morphism().id()
            || correspondence.correspondence().source_universe_id() != closure.source_universe_id()
            || correspondence.correspondence().target_universe_id() != closure.target_universe_id()
        {
            return Err(M6Error::InvalidHistoricalTopology(
                "mapping/correspondence phase is not bound to the closure",
            ));
        }
        // Snapshot IDs alone are not a ProgramSpace domain binding: two
        // independently admitted extractions can legitimately carry the same
        // snapshot ID while exposing different accepted fact sets.  Recompute
        // the exact sorted fact-ID domain in a streaming, allocation-free pass
        // before asking the source log to materialize HPP.
        validate_mapping_program_domains(
            mapping,
            source_admission.program_space(),
            target.program_space(),
        )?;
        let external_reservation = staleness_input_external_reservation_bytes(
            &source_admission,
            mapping,
            correspondence,
            target,
        )?;
        let source_limit = staleness_source_working_limit(
            working_limit,
            source_admission.working_reservation_bytes(),
            external_reservation,
        )?;
        let source = source_log.historical_prefix_projection_v4_with_limit(
            u64::try_from(source_limit).map_err(|_| M6Error::Incomplete {
                operation: "M6 staleness source working limit",
                limit: working_limit,
                observed: usize::MAX,
            })?,
        )?;
        #[cfg(test)]
        STALENESS_INPUT_MATERIALIZATIONS_V5.with(|count| count.set(count.get() + 1));
        let inventory = HistoricalSourceInventoryV5::new(&source, source.program_space())?;
        Self::validate_topology(&source, &inventory)?;
        Ok(Self {
            source,
            closure,
            mapping,
            correspondence,
            target,
            inventory,
        })
    }

    fn validate_topology(
        source: &HistoricalPrefixProjectionV4<'_>,
        inventory: &HistoricalSourceInventoryV5,
    ) -> M6Result<()> {
        let mut edges =
            BTreeMap::<OwnedHistoricalRecordKeyV5, BTreeSet<OwnedHistoricalRecordKeyV5>>::new();
        source.try_visit_records(|record| {
            let descriptor = HistoricalRecordDescriptorV5::from_projection(record);
            let key = OwnedHistoricalRecordKeyV5 {
                kind: descriptor.key().kind,
                id: descriptor.key().id.clone(),
            };
            let mut dependencies = BTreeSet::new();
            let mut self_edge = false;
            descriptor.visit_required_records(inventory, |kind, id| {
                let dependency = OwnedHistoricalRecordKeyV5 {
                    kind,
                    id: id.clone(),
                };
                if dependency == key {
                    // Ownership backreferences are omitted in the individual
                    // record arms. Any remaining self reference is an actual
                    // malformed predecessor, never a harmless duplicate.
                    self_edge = true;
                    return;
                }
                dependencies.insert(dependency);
            });
            if self_edge {
                return Err(M6Error::InvalidHistoricalTopology(
                    "historical required-record graph contains a self edge",
                ));
            }
            if edges.insert(key, dependencies).is_some() {
                return Err(M6Error::InvalidHistoricalTopology(
                    "duplicate source record key",
                ));
            }
            Ok(())
        })?;
        for dependencies in edges.values() {
            if dependencies.iter().any(|key| !edges.contains_key(key)) {
                return Err(M6Error::InvalidHistoricalTopology(
                    "required source record is external to the pinned inventory",
                ));
            }
        }
        let mut indegree = edges
            .iter()
            .map(|(key, dependencies)| (key.clone(), dependencies.len()))
            .collect::<BTreeMap<_, _>>();
        let mut dependents =
            BTreeMap::<OwnedHistoricalRecordKeyV5, BTreeSet<OwnedHistoricalRecordKeyV5>>::new();
        for (record, dependencies) in &edges {
            for dependency in dependencies {
                dependents
                    .entry(dependency.clone())
                    .or_default()
                    .insert(record.clone());
            }
        }
        let mut ready = indegree
            .iter()
            .filter_map(|(key, count)| (*count == 0).then_some(key.clone()))
            .collect::<BTreeSet<_>>();
        let mut visited = 0_usize;
        while let Some(key) = ready.pop_first() {
            visited += 1;
            for dependent in dependents.get(&key).into_iter().flatten() {
                let count =
                    indegree
                        .get_mut(dependent)
                        .ok_or(M6Error::InvalidHistoricalTopology(
                            "dependent edge has no source owner",
                        ))?;
                *count = count
                    .checked_sub(1)
                    .ok_or(M6Error::InvalidHistoricalTopology(
                        "historical dependency indegree underflow",
                    ))?;
                if *count == 0 {
                    ready.insert(dependent.clone());
                }
            }
        }
        if visited != edges.len() {
            return Err(M6Error::InvalidHistoricalTopology(
                "historical required-record graph contains a cycle",
            ));
        }
        Ok(())
    }

    pub(crate) fn visit_source_descriptors(
        &self,
        mut visitor: impl for<'b> FnMut(HistoricalRecordDescriptorV5<'b>) -> M6Result<()>,
    ) -> M6Result<()> {
        self.source.try_visit_records(|record| {
            visitor(HistoricalRecordDescriptorV5::from_projection(record))
        })
    }
}

/// Complete in-memory correspondence phase. Event/Store persistence is owned
/// by a later slice; this value grants no append or acceptance authority.
#[derive(Clone, Debug)]
pub struct M6ObligationCorrespondencePhaseV5 {
    entries: Vec<ObligationCorrespondenceEntryV5>,
    correspondence: ObligationCorrespondenceV5,
    working_peak_upper_bound_bytes: usize,
}

impl M6ObligationCorrespondencePhaseV5 {
    #[must_use]
    pub fn entries(&self) -> &[ObligationCorrespondenceEntryV5] {
        &self.entries
    }
    fn entries_capacity(&self) -> usize {
        self.entries.capacity()
    }
    #[must_use]
    pub fn correspondence(&self) -> &ObligationCorrespondenceV5 {
        &self.correspondence
    }
    #[must_use]
    pub fn working_peak_upper_bound_bytes(&self) -> usize {
        self.working_peak_upper_bound_bytes
    }

    #[doc(hidden)]
    pub fn validate_replayed_canonical(
        &self,
        entry_bytes: &[Vec<u8>],
        correspondence_bytes: &[u8],
    ) -> M6Result<()> {
        if entry_bytes.len() != self.entries.len() {
            return Err(M6Error::InvalidWire(
                "replayed correspondence entry count differs from recomputed phase".to_owned(),
            ));
        }
        for (bytes, expected) in entry_bytes.iter().zip(&self.entries) {
            if ObligationCorrespondenceEntryV5::from_json_bytes(bytes)? != *expected {
                return Err(M6Error::InvalidWire(
                    "replayed correspondence entry differs from recomputed phase".to_owned(),
                ));
            }
        }
        if ObligationCorrespondenceV5::from_json_bytes(correspondence_bytes, &self.correspondence)?
            != self.correspondence
        {
            return Err(M6Error::InvalidWire(
                "replayed correspondence seal differs from recomputed phase".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ObligationCandidateKeyV5 {
    rule_id: String,
    property_id: String,
    property_version: String,
    target_kind: String,
    semantic_key: String,
    target_ids: BTreeSet<StableId>,
    context_ids: BTreeSet<StableId>,
    generator_program_ids: BTreeSet<StableId>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ObligationSideV5 {
    Source,
    Target,
}

#[derive(Clone, Debug)]
struct ObligationComponentSeedV5 {
    from_ids: BTreeSet<StableId>,
    to_ids: BTreeSet<StableId>,
    stage: usize,
}

impl ObligationComponentSeedV5 {
    fn allocated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(id_set_heap(&self.from_ids))
            .saturating_add(id_set_heap(&self.to_ids))
    }
}

fn id_obligation_ref_map_heap(values: &BTreeMap<StableId, &Obligation>) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<(StableId, &Obligation)>())
        .saturating_add(values.keys().map(StableId::allocated_bytes).sum::<usize>())
}

fn id_mapping_ref_map_heap(values: &BTreeMap<StableId, &ProgramMappingV5>) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<(StableId, &ProgramMappingV5)>())
        .saturating_add(values.keys().map(StableId::allocated_bytes).sum::<usize>())
}

fn id_id_map_heap(values: &BTreeMap<StableId, StableId>) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<(StableId, StableId)>())
        .saturating_add(
            values
                .iter()
                .map(|(left, right)| {
                    left.allocated_bytes()
                        .saturating_add(right.allocated_bytes())
                })
                .sum::<usize>(),
        )
}

fn obligation_entry_owner_map_heap(
    values: &BTreeMap<StableId, (usize, StableId, MappingStatusV5)>,
) -> usize {
    values
        .len()
        .saturating_mul(std::mem::size_of::<(
            StableId,
            (usize, StableId, MappingStatusV5),
        )>())
        .saturating_add(
            values
                .iter()
                .map(|(id, (_, entry_id, _))| {
                    id.allocated_bytes()
                        .saturating_add(entry_id.allocated_bytes())
                })
                .sum::<usize>(),
        )
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case", tag = "reference_kind", content = "id")]
enum NormalizedObligationReferenceV5 {
    TargetProgram(StableId),
    UnmappedSourceProgram(StableId),
    TargetObligation(StableId),
    UnmappedSourceObligation(StableId),
}

#[derive(Serialize)]
struct NormalizedObligationBodyV5<'a> {
    target_kind: &'a str,
    target_refs: BTreeSet<NormalizedObligationReferenceV5>,
    semantic_key: String,
    property_id: &'a str,
    property_version: &'a str,
    context_ids: BTreeSet<NormalizedObligationReferenceV5>,
    required_capabilities: &'a BTreeSet<String>,
    evidence_required: bool,
    accepted_evidence_modes: &'a BTreeSet<String>,
    applicability_status: &'a str,
    applicability_reasons: &'a BTreeSet<String>,
    qualification_ids: BTreeSet<NormalizedObligationReferenceV5>,
    weight: f64,
    version_profile: &'a str,
    version_rule: &'a str,
    version_extractor_set: &'a ContentHash,
    version_snapshot: NormalizedObligationReferenceV5,
    depends_on: Vec<NormalizedObligationReferenceV5>,
    generator_ids: BTreeSet<NormalizedObligationReferenceV5>,
    source_ids: BTreeSet<NormalizedObligationReferenceV5>,
}

/// Immutable authority body used only to prove that an accepted aggregate
/// still contains the exact obligation definition independently synthesized
/// from its accepted ProgramSpace.  Ordered fields deliberately remain
/// ordered here: path target order is part of obligation identity and must not
/// be laundered through a normalized set before cross-snapshot comparison.
#[derive(Serialize)]
struct AcceptedObligationDefinitionV5<'a> {
    id: &'a StableId,
    target_kind: &'a str,
    target_refs: &'a [StableId],
    normalized_target_refs: &'a BTreeSet<StableId>,
    semantic_key: &'a str,
    property_id: &'a str,
    property_version: &'a str,
    context_ids: &'a [StableId],
    normalized_context_ids: &'a BTreeSet<StableId>,
    required_capabilities: &'a BTreeSet<String>,
    evidence_required: bool,
    accepted_evidence_modes: &'a BTreeSet<String>,
    applicability_status: &'a str,
    applicability_reasons: &'a BTreeSet<String>,
    qualification_ids: &'a BTreeSet<StableId>,
    weight: f64,
    version: &'a crate::VersionTuple,
    depends_on: &'a [StableId],
    normalized_depends_on: &'a BTreeSet<StableId>,
    generator_ids: &'a BTreeSet<StableId>,
    source_ids: &'a [StableId],
    normalized_source_ids: &'a BTreeSet<StableId>,
}

fn accepted_obligation_definition(obligation: &Obligation) -> AcceptedObligationDefinitionV5<'_> {
    AcceptedObligationDefinitionV5 {
        id: obligation.id(),
        target_kind: obligation.target_kind(),
        target_refs: obligation.target_refs(),
        normalized_target_refs: obligation.normalized_target_refs(),
        semantic_key: obligation.semantic_key(),
        property_id: obligation.property_id(),
        property_version: obligation.property_version(),
        context_ids: obligation.context_ids(),
        normalized_context_ids: obligation.normalized_context_ids(),
        required_capabilities: obligation.required_capabilities(),
        evidence_required: obligation.evidence_required(),
        accepted_evidence_modes: obligation.accepted_evidence_modes(),
        applicability_status: obligation.applicability_status(),
        applicability_reasons: obligation.applicability_reasons(),
        qualification_ids: obligation.qualification_ids(),
        weight: obligation.weight(),
        version: obligation.version(),
        depends_on: obligation.depends_on(),
        normalized_depends_on: obligation.normalized_depends_on(),
        generator_ids: obligation.generator_ids(),
        source_ids: obligation.source_ids(),
        normalized_source_ids: obligation.normalized_source_ids(),
    }
}

fn obligation_program_ids(
    obligation: &Obligation,
    program_domain: &BTreeSet<StableId>,
) -> BTreeSet<StableId> {
    obligation
        .normalized_target_refs()
        .iter()
        .chain(obligation.normalized_source_ids())
        .chain(obligation.normalized_context_ids())
        .chain(obligation.qualification_ids())
        .chain(obligation.generator_ids())
        .filter(|id| program_domain.contains(*id))
        .cloned()
        .chain(std::iter::once(obligation.version().snapshot().clone()))
        .collect()
}

fn validate_obligation_dependency_dag(
    obligations: &BTreeMap<StableId, &Obligation>,
) -> M6Result<BTreeMap<StableId, usize>> {
    let mut indegree = BTreeMap::new();
    let mut dependents = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    let mut depths = obligations
        .keys()
        .cloned()
        .map(|id| (id, 0_usize))
        .collect::<BTreeMap<_, _>>();
    for (id, obligation) in obligations {
        for dependency in obligation.normalized_depends_on() {
            if !obligations.contains_key(dependency) {
                return Err(M6Error::InvalidObligationUniverse(
                    "obligation dependency is outside its accepted universe",
                ));
            }
            dependents
                .entry(dependency.clone())
                .or_default()
                .insert(id.clone());
        }
        indegree.insert(id.clone(), obligation.normalized_depends_on().len());
    }
    let mut ready = indegree
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut visited = 0_usize;
    while let Some(id) = ready.pop_first() {
        visited = visited.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 obligation topological count",
            limit: MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            observed: usize::MAX,
        })?;
        let next_depth = depths[&id].checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 obligation dependency depth",
            limit: usize::MAX,
            observed: usize::MAX,
        })?;
        for dependent in dependents.get(&id).into_iter().flatten() {
            depths
                .entry(dependent.clone())
                .and_modify(|depth| *depth = (*depth).max(next_depth));
            let count = indegree.get_mut(dependent).unwrap();
            *count -= 1;
            if *count == 0 {
                ready.insert(dependent.clone());
            }
        }
    }
    if visited != obligations.len() {
        return Err(M6Error::InvalidObligationUniverse(
            "obligation dependency graph contains a cycle",
        ));
    }
    Ok(depths)
}

fn mapped_candidate_program_set(
    ids: impl IntoIterator<Item = StableId>,
    side: ObligationSideV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> Option<BTreeSet<StableId>> {
    let ids = ids.into_iter().collect::<BTreeSet<_>>();
    match side {
        ObligationSideV5::Target => Some(ids),
        ObligationSideV5::Source => {
            let mut mapped = BTreeSet::new();
            for id in ids {
                let targets = successors.get(&id)?;
                if targets.is_empty() {
                    return None;
                }
                mapped.extend(targets.iter().cloned());
            }
            Some(mapped)
        }
    }
}

fn obligation_candidate_key(
    obligation: &Obligation,
    side: ObligationSideV5,
    program_domain: &BTreeSet<StableId>,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> Option<ObligationCandidateKeyV5> {
    let generator_program_ids = obligation
        .generator_ids()
        .iter()
        .filter(|id| program_domain.contains(*id))
        .cloned();
    Some(ObligationCandidateKeyV5 {
        rule_id: obligation.version().rule().to_owned(),
        property_id: obligation.property_id().to_owned(),
        property_version: obligation.property_version().to_owned(),
        target_kind: obligation.target_kind().to_owned(),
        semantic_key: normalized_obligation_semantic_key(obligation, side, successors),
        target_ids: mapped_candidate_program_set(
            obligation.normalized_target_refs().iter().cloned(),
            side,
            successors,
        )?,
        context_ids: mapped_candidate_program_set(
            obligation.normalized_context_ids().iter().cloned(),
            side,
            successors,
        )?,
        generator_program_ids: mapped_candidate_program_set(
            generator_program_ids,
            side,
            successors,
        )?,
    })
}

fn normalized_obligation_semantic_key(
    obligation: &Obligation,
    side: ObligationSideV5,
    successors: &BTreeMap<StableId, BTreeSet<StableId>>,
) -> String {
    let replacement = match side {
        ObligationSideV5::Target => None,
        ObligationSideV5::Source => successors
            .get(obligation.version().snapshot())
            .filter(|targets| targets.len() == 1)
            .and_then(|targets| targets.iter().next()),
    };
    obligation
        .semantic_key()
        .split('|')
        .map(|part| {
            if part == obligation.version().snapshot().as_str() {
                replacement.map_or(part, StableId::as_str)
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn collapse_and_stage_obligation_components(
    seeds: Vec<ObligationComponentSeedV5>,
    source: &BTreeMap<StableId, &Obligation>,
    target: &BTreeMap<StableId, &Obligation>,
) -> M6Result<Vec<ObligationComponentSeedV5>> {
    let source_owner = seeds
        .iter()
        .enumerate()
        .flat_map(|(index, seed)| seed.from_ids.iter().cloned().map(move |id| (id, index)))
        .collect::<BTreeMap<_, _>>();
    let target_owner = seeds
        .iter()
        .enumerate()
        .flat_map(|(index, seed)| seed.to_ids.iter().cloned().map(move |id| (id, index)))
        .collect::<BTreeMap<_, _>>();
    let mut dependencies = vec![BTreeSet::new(); seeds.len()];
    for (index, seed) in seeds.iter().enumerate() {
        for (ids, obligations, owners) in [
            (&seed.from_ids, source, &source_owner),
            (&seed.to_ids, target, &target_owner),
        ] {
            for id in ids {
                for dependency in obligations[id].normalized_depends_on() {
                    let owner = owners[dependency];
                    if owner != index {
                        dependencies[index].insert(owner);
                    }
                }
            }
        }
    }

    struct Tarjan<'a> {
        edges: &'a [BTreeSet<usize>],
        next: usize,
        indices: Vec<Option<usize>>,
        lowlink: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        components: Vec<Vec<usize>>,
    }
    impl Tarjan<'_> {
        fn visit(&mut self, vertex: usize) {
            let index = self.next;
            self.next += 1;
            self.indices[vertex] = Some(index);
            self.lowlink[vertex] = index;
            self.stack.push(vertex);
            self.on_stack[vertex] = true;
            for next in &self.edges[vertex] {
                if self.indices[*next].is_none() {
                    self.visit(*next);
                    self.lowlink[vertex] = self.lowlink[vertex].min(self.lowlink[*next]);
                } else if self.on_stack[*next] {
                    self.lowlink[vertex] = self.lowlink[vertex].min(self.indices[*next].unwrap());
                }
            }
            if self.lowlink[vertex] == self.indices[vertex].unwrap() {
                let mut component = Vec::new();
                loop {
                    let member = self.stack.pop().unwrap();
                    self.on_stack[member] = false;
                    component.push(member);
                    if member == vertex {
                        break;
                    }
                }
                component.sort_unstable();
                self.components.push(component);
            }
        }
    }
    let mut tarjan = Tarjan {
        edges: &dependencies,
        next: 0,
        indices: vec![None; seeds.len()],
        lowlink: vec![0; seeds.len()],
        stack: Vec::new(),
        on_stack: vec![false; seeds.len()],
        components: Vec::new(),
    };
    for index in 0..seeds.len() {
        if tarjan.indices[index].is_none() {
            tarjan.visit(index);
        }
    }
    tarjan
        .components
        .sort_by_key(|component| component.first().copied());
    let membership = tarjan
        .components
        .iter()
        .enumerate()
        .flat_map(|(component, members)| {
            members
                .iter()
                .copied()
                .map(move |member| (member, component))
        })
        .collect::<BTreeMap<_, _>>();
    let mut collapsed = Vec::with_capacity(tarjan.components.len());
    for members in &tarjan.components {
        let from_ids = members
            .iter()
            .flat_map(|index| seeds[*index].from_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let to_ids = members
            .iter()
            .flat_map(|index| seeds[*index].to_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        bounded(
            from_ids.len(),
            MAX_M6_CORRESPONDENCE_SIDE_IDS,
            "M6 grouped correspondence source obligations",
        )?;
        bounded(
            to_ids.len(),
            MAX_M6_CORRESPONDENCE_SIDE_IDS,
            "M6 grouped correspondence target obligations",
        )?;
        collapsed.push(ObligationComponentSeedV5 {
            from_ids,
            to_ids,
            stage: 0,
        });
    }
    let condensed = tarjan
        .components
        .iter()
        .enumerate()
        .map(|(component, members)| {
            members
                .iter()
                .flat_map(|member| dependencies[*member].iter())
                .map(|dependency| membership[dependency])
                .filter(|dependency| *dependency != component)
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    fn depth(
        index: usize,
        dependencies: &[BTreeSet<usize>],
        memo: &mut [Option<usize>],
    ) -> M6Result<usize> {
        if let Some(value) = memo[index] {
            return Ok(value);
        }
        let mut value = 0_usize;
        for dependency in &dependencies[index] {
            value = value.max(
                depth(*dependency, dependencies, memo)?
                    .checked_add(1)
                    .ok_or(M6Error::Incomplete {
                        operation: "M6 correspondence dependency depth",
                        limit: usize::MAX,
                        observed: usize::MAX,
                    })?,
            );
        }
        memo[index] = Some(value);
        Ok(value)
    }
    let mut memo = vec![None; collapsed.len()];
    for (index, seed) in collapsed.iter_mut().enumerate() {
        seed.stage = depth(index, &condensed, &mut memo)?;
    }
    collapsed.sort_by(|left, right| {
        left.stage
            .cmp(&right.stage)
            .then_with(|| {
                left.from_ids
                    .iter()
                    .next()
                    .cmp(&right.from_ids.iter().next())
            })
            .then_with(|| left.to_ids.iter().next().cmp(&right.to_ids.iter().next()))
    });
    Ok(collapsed)
}

fn normalized_program_reference(
    id: &StableId,
    side: ObligationSideV5,
    preserved_successors: &BTreeMap<StableId, StableId>,
) -> NormalizedObligationReferenceV5 {
    match side {
        ObligationSideV5::Target => NormalizedObligationReferenceV5::TargetProgram(id.clone()),
        ObligationSideV5::Source => preserved_successors.get(id).map_or_else(
            || NormalizedObligationReferenceV5::UnmappedSourceProgram(id.clone()),
            |target| NormalizedObligationReferenceV5::TargetProgram(target.clone()),
        ),
    }
}

fn normalized_obligation_reference(
    id: &StableId,
    side: ObligationSideV5,
    preserved_successors: &BTreeMap<StableId, StableId>,
) -> NormalizedObligationReferenceV5 {
    match side {
        ObligationSideV5::Target => NormalizedObligationReferenceV5::TargetObligation(id.clone()),
        ObligationSideV5::Source => preserved_successors.get(id).map_or_else(
            || NormalizedObligationReferenceV5::UnmappedSourceObligation(id.clone()),
            |target| NormalizedObligationReferenceV5::TargetObligation(target.clone()),
        ),
    }
}

fn normalized_obligation_body_hash(
    obligation: &Obligation,
    side: ObligationSideV5,
    program_domain: &BTreeSet<StableId>,
    obligation_domain: &BTreeSet<StableId>,
    preserved_program_successors: &BTreeMap<StableId, StableId>,
    preserved_obligation_successors: &BTreeMap<StableId, StableId>,
) -> M6Result<ContentHash> {
    let normalize = |id: &StableId| {
        if program_domain.contains(id) {
            normalized_program_reference(id, side, preserved_program_successors)
        } else if obligation_domain.contains(id) {
            normalized_obligation_reference(id, side, preserved_obligation_successors)
        } else {
            NormalizedObligationReferenceV5::UnmappedSourceObligation(id.clone())
        }
    };
    body_hash(&NormalizedObligationBodyV5 {
        target_kind: obligation.target_kind(),
        target_refs: obligation
            .normalized_target_refs()
            .iter()
            .map(&normalize)
            .collect(),
        semantic_key: normalized_obligation_semantic_key(
            obligation,
            side,
            &preserved_program_successors
                .iter()
                .map(|(source, target)| (source.clone(), BTreeSet::from([target.clone()])))
                .collect(),
        ),
        property_id: obligation.property_id(),
        property_version: obligation.property_version(),
        context_ids: obligation
            .normalized_context_ids()
            .iter()
            .map(&normalize)
            .collect(),
        required_capabilities: obligation.required_capabilities(),
        evidence_required: obligation.evidence_required(),
        accepted_evidence_modes: obligation.accepted_evidence_modes(),
        applicability_status: obligation.applicability_status(),
        applicability_reasons: obligation.applicability_reasons(),
        qualification_ids: obligation
            .qualification_ids()
            .iter()
            .map(&normalize)
            .collect(),
        weight: obligation.weight(),
        version_profile: obligation.version().profile(),
        version_rule: obligation.version().rule(),
        version_extractor_set: obligation.version().extractor_set(),
        version_snapshot: normalize(obligation.version().snapshot()),
        depends_on: obligation.depends_on().iter().map(&normalize).collect(),
        generator_ids: obligation.generator_ids().iter().map(&normalize).collect(),
        source_ids: obligation
            .normalized_source_ids()
            .iter()
            .map(&normalize)
            .collect(),
    })
}

fn validate_resynthesized_accepted_universe(aggregate: &ReviewAggregate) -> M6Result<()> {
    let expected = crate::MvpRulePack::synthesize(aggregate.program()).map_err(|_| {
        M6Error::InvalidObligationUniverse(
            "accepted ProgramSpace cannot reproduce its obligation universe",
        )
    })?;
    if expected.universe() != aggregate.universe() {
        return Err(M6Error::InvalidObligationUniverse(
            "accepted universe differs from deterministic rule-pack synthesis",
        ));
    }
    let actual = aggregate
        .obligations()
        .map(|obligation| (obligation.id().clone(), obligation))
        .collect::<BTreeMap<_, _>>();
    let expected = expected
        .obligations()
        .iter()
        .map(|obligation| (obligation.id().clone(), obligation))
        .collect::<BTreeMap<_, _>>();
    if actual.keys().ne(expected.keys()) {
        return Err(M6Error::InvalidObligationUniverse(
            "accepted obligation domain differs from deterministic rule-pack synthesis",
        ));
    }
    for id in actual.keys() {
        let actual_definition = accepted_obligation_definition(actual[id]);
        let expected_definition = accepted_obligation_definition(expected[id]);
        bounded_serialized(
            &actual_definition,
            MAX_M6_CANONICAL_BYTES,
            "M6 accepted obligation definition bytes",
        )?;
        bounded_serialized(
            &expected_definition,
            MAX_M6_CANONICAL_BYTES,
            "M6 synthesized obligation definition bytes",
        )?;
        if crate::canonical_json(&actual_definition)?
            != crate::canonical_json(&expected_definition)?
        {
            return Err(M6Error::InvalidObligationUniverse(
                "accepted obligation body differs from deterministic rule-pack synthesis",
            ));
        }
    }
    Ok(())
}

fn checked_correspondence_working_add(total: usize, addition: usize) -> M6Result<usize> {
    let observed = total.checked_add(addition).ok_or(M6Error::Incomplete {
        operation: "M6 correspondence retained working bytes",
        limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
        observed: usize::MAX,
    })?;
    bounded(
        observed,
        MAX_M6_CORRESPONDENCE_WORKING_BYTES,
        "M6 correspondence retained working bytes",
    )?;
    Ok(observed)
}

fn one_to_one_obligation_status(
    program_refs_are_unique: bool,
    dependency_refs_are_unique: bool,
    source_body_hash: &ContentHash,
    target_body_hash: &ContentHash,
) -> MappingStatusV5 {
    if !program_refs_are_unique || !dependency_refs_are_unique {
        MappingStatusV5::Unresolved
    } else if source_body_hash == target_body_hash {
        MappingStatusV5::Preserved
    } else {
        MappingStatusV5::Modified
    }
}

fn checked_correspondence_working_mul(value: usize, multiplier: usize) -> M6Result<usize> {
    let observed = value.checked_mul(multiplier).ok_or(M6Error::Incomplete {
        operation: "M6 correspondence reservation arithmetic",
        limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
        observed: usize::MAX,
    })?;
    bounded(
        observed,
        MAX_M6_CORRESPONDENCE_WORKING_BYTES,
        "M6 correspondence reservation arithmetic",
    )?;
    Ok(observed)
}

/// Deterministic allocation oracle evaluated before any correspondence-owned
/// map, domain, candidate, component, entry, or seal is constructed.  Each
/// field names a simultaneously chargeable ownership class rather than hiding
/// allocations behind one post-hoc multiplier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CorrespondenceAllocationOracleV5 {
    accepted_aggregate_bytes: usize,
    aggregate_validation_scratch_bytes: usize,
    mapping_phase_bytes: usize,
    obligation_domain_bytes: usize,
    owner_successor_bytes: usize,
    normalized_key_body_bytes: usize,
    dependency_component_bytes: usize,
    entry_seal_bytes: usize,
    serialization_scratch_bytes: usize,
}

impl CorrespondenceAllocationOracleV5 {
    fn reservation_bytes(self) -> M6Result<usize> {
        [
            self.accepted_aggregate_bytes,
            self.aggregate_validation_scratch_bytes,
            self.mapping_phase_bytes,
            self.obligation_domain_bytes,
            self.owner_successor_bytes,
            self.normalized_key_body_bytes,
            self.dependency_component_bytes,
            self.entry_seal_bytes,
            self.serialization_scratch_bytes,
        ]
        .into_iter()
        .try_fold(0_usize, checked_correspondence_working_add)
    }
}

fn correspondence_allocation_oracle(
    mapping_phase: &M6MappingPhaseV5,
    source: &ReviewAggregate,
    target: &ReviewAggregate,
) -> M6Result<CorrespondenceAllocationOracleV5> {
    let retained = |aggregate: &ReviewAggregate| {
        usize::try_from(aggregate.retained_bytes_v3()?).map_err(|_| M6Error::Incomplete {
            operation: "M6 correspondence accepted aggregate bytes",
            limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
            observed: usize::MAX,
        })
    };
    let accepted_aggregate_bytes =
        checked_correspondence_working_add(retained(source)?, retained(target)?)?;
    // ReviewAggregate::validate materializes obligation_ids, program_ids and
    // all_ids, and can retain an additional evidence/decision/key projection
    // while checking later records. Six full retained-aggregate ownership
    // copies are a deterministic allocation-free upper bound over those ID
    // clones and portable B-tree slots; it is charged separately because the
    // validation happens before correspondence collections exist.
    let aggregate_validation_scratch_bytes =
        checked_correspondence_working_mul(accepted_aggregate_bytes, 6)?;

    let mapping_records_bytes = mapping_phase.mappings().iter().try_fold(
        mapping_phase
            .mappings()
            .len()
            .checked_mul(std::mem::size_of::<ProgramMappingV5>())
            .ok_or(M6Error::Incomplete {
                operation: "M6 correspondence mapping phase arithmetic",
                limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                observed: usize::MAX,
            })?,
        |total, mapping| {
            total
                .checked_add(mapping.allocated_bytes())
                .ok_or(M6Error::Incomplete {
                    operation: "M6 correspondence mapping phase arithmetic",
                    limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                    observed: usize::MAX,
                })
        },
    )?;
    let mapping_phase_bytes = checked_correspondence_working_add(
        mapping_records_bytes,
        mapping_phase.morphism().allocated_bytes(),
    )?;

    let obligations = source.obligations().chain(target.obligations());
    let mut obligation_count = 0_usize;
    let mut obligation_owned_bytes = 0_usize;
    let mut obligation_id_bytes = 0_usize;
    let mut dependency_edges = 0_usize;
    let mut program_reference_slots = 0_usize;
    let mut max_id_bytes = 0_usize;
    for obligation in obligations {
        obligation_count = obligation_count.checked_add(1).ok_or(M6Error::Incomplete {
            operation: "M6 correspondence obligation count arithmetic",
            limit: MAX_M6_CORRESPONDENCE_ENTRIES,
            observed: usize::MAX,
        })?;
        obligation_owned_bytes = obligation_owned_bytes
            .checked_add(obligation.allocated_bytes())
            .ok_or(M6Error::Incomplete {
                operation: "M6 correspondence obligation ownership arithmetic",
                limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        let id_bytes = obligation.id().allocated_bytes();
        obligation_id_bytes =
            obligation_id_bytes
                .checked_add(id_bytes)
                .ok_or(M6Error::Incomplete {
                    operation: "M6 correspondence obligation ID arithmetic",
                    limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                    observed: usize::MAX,
                })?;
        max_id_bytes = max_id_bytes.max(id_bytes);
        dependency_edges = dependency_edges
            .checked_add(obligation.normalized_depends_on().len())
            .ok_or(M6Error::Incomplete {
                operation: "M6 correspondence dependency arithmetic",
                limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                observed: usize::MAX,
            })?;
        program_reference_slots = [
            obligation.normalized_target_refs().len(),
            obligation.normalized_source_ids().len(),
            obligation.normalized_context_ids().len(),
            obligation.qualification_ids().len(),
            obligation.generator_ids().len(),
            1,
        ]
        .into_iter()
        .try_fold(program_reference_slots, usize::checked_add)
        .ok_or(M6Error::Incomplete {
            operation: "M6 correspondence Program reference arithmetic",
            limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
            observed: usize::MAX,
        })?;
    }
    bounded(
        obligation_count,
        MAX_M6_CORRESPONDENCE_ENTRIES,
        "M6 correspondence total obligation count",
    )?;

    for mapping in mapping_phase.mappings() {
        max_id_bytes = max_id_bytes.max(mapping.id().allocated_bytes());
        for id in mapping.from_ids().iter().chain(mapping.to_ids()) {
            max_id_bytes = max_id_bytes.max(id.allocated_bytes());
        }
    }
    let id_slot_bytes = std::mem::size_of::<StableId>()
        .checked_add(max_id_bytes)
        .ok_or(M6Error::Incomplete {
            operation: "M6 correspondence ID slot arithmetic",
            limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
            observed: usize::MAX,
        })?;
    let program_domain_count = usize::try_from(
        mapping_phase
            .morphism()
            .source_domain_count()
            .checked_add(mapping_phase.morphism().target_domain_count())
            .ok_or(M6Error::Incomplete {
                operation: "M6 correspondence Program domain arithmetic",
                limit: MAX_M6_PROGRAM_DOMAIN_IDS * 2,
                observed: usize::MAX,
            })?,
    )
    .map_err(|_| M6Error::Incomplete {
        operation: "M6 correspondence Program domain arithmetic",
        limit: MAX_M6_PROGRAM_DOMAIN_IDS * 2,
        observed: usize::MAX,
    })?;

    // Domain/index charge: two obligation reference maps, two obligation ID
    // sets and both Program domain sets. Dynamic ID storage is charged for
    // every clone in addition to the portable map/set slot contract.
    let domain_slots = obligation_count
        .checked_mul(
            2 * std::mem::size_of::<(StableId, &Obligation)>()
                + 2 * std::mem::size_of::<StableId>(),
        )
        .and_then(|bytes| {
            program_domain_count
                .checked_mul(std::mem::size_of::<StableId>())
                .and_then(|program| bytes.checked_add(program))
        })
        .ok_or(M6Error::Incomplete {
            operation: "M6 correspondence domain reservation arithmetic",
            limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
            observed: usize::MAX,
        })?;
    let obligation_domain_bytes = [
        domain_slots,
        checked_correspondence_working_mul(obligation_id_bytes, 4)?,
        checked_correspondence_working_mul(mapping_phase_bytes, 2)?,
    ]
    .into_iter()
    .try_fold(0_usize, checked_correspondence_working_add)?;

    // Successor/owner maps clone mapping-domain IDs and retain mapping refs.
    // The factor also covers preserved-only projections and both source/target
    // ownership indexes.
    let owner_successor_bytes = [
        checked_correspondence_working_mul(mapping_phase_bytes, 5)?,
        checked_correspondence_working_mul(
            program_domain_count,
            id_slot_bytes.checked_mul(4).ok_or(M6Error::Incomplete {
                operation: "M6 correspondence owner slot arithmetic",
                limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                observed: usize::MAX,
            })?,
        )?,
    ]
    .into_iter()
    .try_fold(0_usize, checked_correspondence_working_add)?;

    // One candidate key per obligation owns all key strings and mapped
    // target/context/generator sets. One normalized body and canonical buffer
    // may coexist while an entry is materialized.
    let normalized_key_body_bytes = [
        checked_correspondence_working_mul(obligation_owned_bytes, 4)?,
        checked_correspondence_working_mul(
            program_reference_slots,
            id_slot_bytes.checked_mul(2).ok_or(M6Error::Incomplete {
                operation: "M6 correspondence normalized reference slot arithmetic",
                limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                observed: usize::MAX,
            })?,
        )?,
        MAX_M6_CANONICAL_BYTES,
    ]
    .into_iter()
    .try_fold(0_usize, checked_correspondence_working_add)?;

    // Raw DAG validation and quotient staging can coexist with owner maps,
    // dependency sets, Tarjan frontiers/components, membership and collapsed
    // seeds. Obligation bytes provide a checked upper bound for every cloned
    // dependency ID/string; slot charges cover the index-only vectors/maps.
    let dependency_component_bytes = [
        checked_correspondence_working_mul(obligation_owned_bytes, 8)?,
        checked_correspondence_working_mul(
            dependency_edges,
            id_slot_bytes
                .checked_add(std::mem::size_of::<usize>())
                .and_then(|bytes| bytes.checked_mul(6))
                .ok_or(M6Error::Incomplete {
                    operation: "M6 correspondence dependency slot arithmetic",
                    limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                    observed: usize::MAX,
                })?,
        )?,
        checked_correspondence_working_mul(
            obligation_count,
            std::mem::size_of::<usize>()
                .checked_mul(24)
                .ok_or(M6Error::Incomplete {
                    operation: "M6 correspondence component slot arithmetic",
                    limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                    observed: usize::MAX,
                })?,
        )?,
    ]
    .into_iter()
    .try_fold(0_usize, checked_correspondence_working_add)?;

    // Exclusive coverage means from/to and body-hash members total no more
    // than the two obligation domains. Source-mapping/predecessor sets retain
    // at most their declared 64 IDs for every possible component.
    let link_slots = obligation_count
        .checked_mul(
            2_usize
                .checked_mul(MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS)
                .and_then(|count| count.checked_add(6))
                .ok_or(M6Error::Incomplete {
                    operation: "M6 correspondence entry link arithmetic",
                    limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                    observed: usize::MAX,
                })?,
        )
        .ok_or(M6Error::Incomplete {
            operation: "M6 correspondence entry link arithmetic",
            limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
            observed: usize::MAX,
        })?;
    let entry_seal_bytes = [
        checked_correspondence_working_mul(
            obligation_count,
            std::mem::size_of::<ObligationCorrespondenceEntryV5>(),
        )?,
        checked_correspondence_working_mul(
            link_slots,
            id_slot_bytes
                .checked_add(std::mem::size_of::<ContentHash>())
                .ok_or(M6Error::Incomplete {
                    operation: "M6 correspondence entry slot arithmetic",
                    limit: MAX_M6_CORRESPONDENCE_WORKING_BYTES,
                    observed: usize::MAX,
                })?,
        )?,
        checked_correspondence_working_mul(obligation_owned_bytes, 4)?,
        checked_correspondence_working_mul(mapping_phase_bytes, 2)?,
        std::mem::size_of::<ObligationCorrespondenceV5>(),
    ]
    .into_iter()
    .try_fold(0_usize, checked_correspondence_working_add)?;

    let serialization_scratch_bytes = checked_correspondence_working_add(
        MAX_M6_CANONICAL_BYTES,
        MAX_M6_CORRESPONDENCE_DTO_BYTES,
    )?;
    Ok(CorrespondenceAllocationOracleV5 {
        accepted_aggregate_bytes,
        aggregate_validation_scratch_bytes,
        mapping_phase_bytes,
        obligation_domain_bytes,
        owner_successor_bytes,
        normalized_key_body_bytes,
        dependency_component_bytes,
        entry_seal_bytes,
        serialization_scratch_bytes,
    })
}

#[cfg(test)]
std::thread_local! {
    static CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

fn validate_correspondence_aggregate(aggregate: &ReviewAggregate) -> Result<(), DomainError> {
    #[cfg(test)]
    CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS.with(|calls| calls.set(calls.get() + 1));
    aggregate.validate()
}

impl ObligationCorrespondenceV5 {
    /// Computes and admits the complete correspondence reservation before any
    /// correspondence-owned retained collection is built.
    #[doc(hidden)]
    pub fn correspondence_reservation_bytes_from_accepted_universes(
        mapping_phase: &M6MappingPhaseV5,
        source: &ReviewAggregate,
        target: &ReviewAggregate,
    ) -> M6Result<usize> {
        correspondence_allocation_oracle(mapping_phase, source, target)?.reservation_bytes()
    }

    /// Recomputes correspondence only from validated accepted aggregates, the
    /// exact accepted closure, and its already sealed Program mapping phase.
    #[doc(hidden)]
    pub fn derive_from_accepted_universes(
        closure: &IncrementalSourceClosureV5,
        mapping_phase: &M6MappingPhaseV5,
        source: &ReviewAggregate,
        target: &ReviewAggregate,
    ) -> M6Result<M6ObligationCorrespondencePhaseV5> {
        Self::derive_with_working_limit(
            closure,
            mapping_phase,
            source,
            target,
            MAX_M6_CORRESPONDENCE_WORKING_BYTES,
        )
    }

    fn derive_with_working_limit(
        closure: &IncrementalSourceClosureV5,
        mapping_phase: &M6MappingPhaseV5,
        source: &ReviewAggregate,
        target: &ReviewAggregate,
        working_limit: usize,
    ) -> M6Result<M6ObligationCorrespondencePhaseV5> {
        // The allocation-free oracle and its admission are the first
        // operations. In particular ReviewAggregate::validate allocates
        // obligation/program/all-ID scratch and may not run before this gate.
        let allocation_oracle = correspondence_allocation_oracle(mapping_phase, source, target)?;
        let working_peak_upper_bound_bytes = allocation_oracle.reservation_bytes()?;
        bounded(
            working_peak_upper_bound_bytes,
            working_limit,
            "M6 correspondence preflight working bytes",
        )?;
        validate_correspondence_aggregate(source).map_err(|_| {
            M6Error::InvalidObligationUniverse("source aggregate/universe is not valid")
        })?;
        validate_correspondence_aggregate(target).map_err(|_| {
            M6Error::InvalidObligationUniverse("target aggregate/universe is not valid")
        })?;
        validate_resynthesized_accepted_universe(source)?;
        validate_resynthesized_accepted_universe(target)?;
        let morphism = mapping_phase.morphism();
        if morphism.source_closure_id() != closure.id()
            || source.program().repository_id() != target.program().repository_id()
            || source.program().snapshot_id() != morphism.source_snapshot_id()
            || target.program().snapshot_id() != morphism.target_snapshot_id()
            || source.universe().id() != &closure.input.source_universe_id
            || target.universe().id() != &closure.input.target_universe_id
        {
            return Err(M6Error::InvalidObligationUniverse(
                "accepted universes do not bind the closure and morphism snapshots",
            ));
        }
        let source_obligations = source
            .obligations()
            .map(|obligation| (obligation.id().clone(), obligation))
            .collect::<BTreeMap<_, _>>();
        let target_obligations = target
            .obligations()
            .map(|obligation| (obligation.id().clone(), obligation))
            .collect::<BTreeMap<_, _>>();
        bounded(
            source_obligations.len(),
            MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            "M6 source obligations",
        )?;
        bounded(
            target_obligations.len(),
            MAX_M6_OBLIGATIONS_PER_UNIVERSE,
            "M6 target obligations",
        )?;
        if source_obligations.keys().cloned().collect::<BTreeSet<_>>()
            != *source.universe().obligation_ids()
            || target_obligations.keys().cloned().collect::<BTreeSet<_>>()
                != *target.universe().obligation_ids()
        {
            return Err(M6Error::InvalidObligationUniverse(
                "accepted universe denominator does not equal its obligation records",
            ));
        }
        validate_obligation_dependency_dag(&source_obligations)?;
        validate_obligation_dependency_dag(&target_obligations)?;

        let mut program_successors = BTreeMap::<StableId, BTreeSet<StableId>>::new();
        let mut preserved_program_successors = BTreeMap::<StableId, StableId>::new();
        let mut source_mapping_owner = BTreeMap::<StableId, &ProgramMappingV5>::new();
        let mut target_mapping_owner = BTreeMap::<StableId, &ProgramMappingV5>::new();
        for mapping in mapping_phase.mappings() {
            for id in mapping.from_ids() {
                program_successors.insert(id.clone(), mapping.to_ids().clone());
                source_mapping_owner.insert(id.clone(), mapping);
                if (mapping.status() == MappingStatusV5::Preserved
                    || mapping.object_kind() == ProgramObjectKindV5::Snapshot)
                    && mapping.to_ids().len() == 1
                {
                    preserved_program_successors
                        .insert(id.clone(), mapping.to_ids().iter().next().unwrap().clone());
                }
            }
            for id in mapping.to_ids() {
                target_mapping_owner.insert(id.clone(), mapping);
            }
        }
        let source_program_domain = source.program().known_ids();
        let target_program_domain = target.program().known_ids();
        if source_mapping_owner
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            != source_program_domain
            || target_mapping_owner
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                != target_program_domain
            || morphism.source_domain_count() != source_program_domain.len() as u64
            || morphism.target_domain_count() != target_program_domain.len() as u64
            || morphism.source_domain_digest() != &digest_ids(&source_program_domain)?
            || morphism.target_domain_digest() != &digest_ids(&target_program_domain)?
        {
            return Err(M6Error::InvalidObligationUniverse(
                "accepted morphism does not exactly cover both aggregate Program domains",
            ));
        }
        let source_obligation_domain = source_obligations.keys().cloned().collect::<BTreeSet<_>>();
        let target_obligation_domain = target_obligations.keys().cloned().collect::<BTreeSet<_>>();

        let mut source_by_key = BTreeMap::<ObligationCandidateKeyV5, BTreeSet<StableId>>::new();
        let mut target_by_key = BTreeMap::<ObligationCandidateKeyV5, BTreeSet<StableId>>::new();
        let mut unmatched_source = BTreeSet::new();
        for obligation in source_obligations.values() {
            if let Some(key) = obligation_candidate_key(
                obligation,
                ObligationSideV5::Source,
                &source_program_domain,
                &program_successors,
            ) {
                source_by_key
                    .entry(key)
                    .or_default()
                    .insert(obligation.id().clone());
            } else {
                unmatched_source.insert(obligation.id().clone());
            }
        }
        for obligation in target_obligations.values() {
            let key = obligation_candidate_key(
                obligation,
                ObligationSideV5::Target,
                &target_program_domain,
                &program_successors,
            )
            .ok_or(M6Error::InvalidObligationUniverse(
                "target obligation candidate key could not be constructed",
            ))?;
            target_by_key
                .entry(key)
                .or_default()
                .insert(obligation.id().clone());
        }
        let keys = source_by_key
            .keys()
            .chain(target_by_key.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut seeds = Vec::new();
        for key in keys {
            let from_ids = source_by_key.remove(&key).unwrap_or_default();
            let to_ids = target_by_key.remove(&key).unwrap_or_default();
            if from_ids.is_empty() {
                seeds.extend(to_ids.into_iter().map(|id| ObligationComponentSeedV5 {
                    from_ids: BTreeSet::new(),
                    to_ids: BTreeSet::from([id]),
                    stage: 0,
                }));
            } else if to_ids.is_empty() {
                seeds.extend(from_ids.into_iter().map(|id| ObligationComponentSeedV5 {
                    from_ids: BTreeSet::from([id]),
                    to_ids: BTreeSet::new(),
                    stage: 0,
                }));
            } else {
                seeds.push(ObligationComponentSeedV5 {
                    from_ids,
                    to_ids,
                    stage: 0,
                });
            }
        }
        seeds.extend(
            unmatched_source
                .into_iter()
                .map(|id| ObligationComponentSeedV5 {
                    from_ids: BTreeSet::from([id]),
                    to_ids: BTreeSet::new(),
                    stage: 0,
                }),
        );
        bounded(
            seeds.len(),
            MAX_M6_CORRESPONDENCE_ENTRIES,
            "M6 correspondence entries",
        )?;
        let seeds = collapse_and_stage_obligation_components(
            seeds,
            &source_obligations,
            &target_obligations,
        )?;

        let all_obligation_successors = seeds
            .iter()
            .flat_map(|seed| {
                seed.from_ids
                    .iter()
                    .cloned()
                    .map(move |id| (id, seed.to_ids.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let mut preserved_obligation_successors = BTreeMap::<StableId, StableId>::new();
        let mut source_entry_owner =
            BTreeMap::<StableId, (usize, StableId, MappingStatusV5)>::new();
        let mut target_entry_owner =
            BTreeMap::<StableId, (usize, StableId, MappingStatusV5)>::new();
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(seeds.len())
            .map_err(|_| M6Error::Incomplete {
                operation: "M6 correspondence entry allocation",
                limit: MAX_M6_CORRESPONDENCE_ENTRIES,
                observed: usize::MAX,
            })?;
        for seed in &seeds {
            let mut predecessor_entry_ids = BTreeSet::new();
            for (side, id) in seed
                .from_ids
                .iter()
                .map(|id| (ObligationSideV5::Source, id))
                .chain(seed.to_ids.iter().map(|id| (ObligationSideV5::Target, id)))
            {
                let obligation = match side {
                    ObligationSideV5::Source => source_obligations[id],
                    ObligationSideV5::Target => target_obligations[id],
                };
                let owners = match side {
                    ObligationSideV5::Source => &source_entry_owner,
                    ObligationSideV5::Target => &target_entry_owner,
                };
                for dependency in obligation.normalized_depends_on() {
                    if let Some((stage, entry_id, _)) = owners.get(dependency)
                        && *stage < seed.stage
                    {
                        predecessor_entry_ids.insert(entry_id.clone());
                    }
                }
            }
            bounded(
                predecessor_entry_ids.len(),
                MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS,
                "M6 correspondence predecessor entries",
            )?;

            let mut source_mapping_ids = BTreeSet::new();
            for id in &seed.from_ids {
                for program_id in
                    obligation_program_ids(source_obligations[id], &source_program_domain)
                {
                    source_mapping_ids.insert(source_mapping_owner[&program_id].id().clone());
                }
            }
            for id in &seed.to_ids {
                for program_id in
                    obligation_program_ids(target_obligations[id], &target_program_domain)
                {
                    source_mapping_ids.insert(target_mapping_owner[&program_id].id().clone());
                }
            }
            bounded(
                source_mapping_ids.len(),
                MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS,
                "M6 correspondence source mappings",
            )?;

            let source_body_hashes = seed
                .from_ids
                .iter()
                .map(|id| {
                    IdBodyHashV5::new(
                        id.clone(),
                        normalized_obligation_body_hash(
                            source_obligations[id],
                            ObligationSideV5::Source,
                            &source_program_domain,
                            &source_obligation_domain,
                            &preserved_program_successors,
                            &preserved_obligation_successors,
                        )?,
                    )
                })
                .collect::<M6Result<Vec<_>>>()?;
            let target_body_hashes = seed
                .to_ids
                .iter()
                .map(|id| {
                    IdBodyHashV5::new(
                        id.clone(),
                        normalized_obligation_body_hash(
                            target_obligations[id],
                            ObligationSideV5::Target,
                            &target_program_domain,
                            &target_obligation_domain,
                            &preserved_program_successors,
                            &preserved_obligation_successors,
                        )?,
                    )
                })
                .collect::<M6Result<Vec<_>>>()?;

            let status = match (seed.from_ids.len(), seed.to_ids.len()) {
                (0, 1) => MappingStatusV5::Added,
                (1, 0) => MappingStatusV5::Removed,
                (1, 1) => {
                    let source_id = seed.from_ids.iter().next().unwrap();
                    let source_obligation = source_obligations[source_id];
                    let program_refs_are_unique =
                        obligation_program_ids(source_obligation, &source_program_domain)
                            .iter()
                            .all(|id| {
                                let mapping = source_mapping_owner[id];
                                mapping.to_ids().len() == 1
                                    && matches!(
                                        mapping.status(),
                                        MappingStatusV5::Preserved | MappingStatusV5::Modified
                                    )
                            });
                    let dependency_refs_are_unique = source_obligation
                        .normalized_depends_on()
                        .iter()
                        .all(|dependency| {
                            all_obligation_successors
                                .get(dependency)
                                .is_some_and(|targets| targets.len() == 1)
                                && source_entry_owner.get(dependency).is_some_and(
                                    |(_, _, status)| {
                                        matches!(
                                            status,
                                            MappingStatusV5::Preserved | MappingStatusV5::Modified
                                        )
                                    },
                                )
                        });
                    one_to_one_obligation_status(
                        program_refs_are_unique,
                        dependency_refs_are_unique,
                        &source_body_hashes[0].body_hash,
                        &target_body_hashes[0].body_hash,
                    )
                }
                (1, _) => MappingStatusV5::Split,
                (_, 1) => MappingStatusV5::Merged,
                _ => MappingStatusV5::Unresolved,
            };
            let entry = ObligationCorrespondenceEntryV5::from_parts(
                ObligationCorrespondenceEntryPartsV5 {
                    morphism_id: morphism.id().clone(),
                    from_obligation_ids: seed.from_ids.clone(),
                    to_obligation_ids: seed.to_ids.clone(),
                    status,
                    source_mapping_ids,
                    predecessor_entry_ids,
                    source_body_hashes,
                    target_body_hashes,
                },
            )?;
            if status == MappingStatusV5::Preserved
                && seed.from_ids.len() == 1
                && seed.to_ids.len() == 1
            {
                preserved_obligation_successors.insert(
                    seed.from_ids.iter().next().unwrap().clone(),
                    seed.to_ids.iter().next().unwrap().clone(),
                );
            }
            for id in &seed.from_ids {
                source_entry_owner.insert(id.clone(), (seed.stage, entry.id.clone(), status));
            }
            for id in &seed.to_ids {
                target_entry_owner.insert(id.clone(), (seed.stage, entry.id.clone(), status));
            }
            entries.push(entry);
        }
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        let correspondence = Self::seal_derived(
            morphism,
            source.universe().id(),
            target.universe().id(),
            &entries,
            &source_obligation_domain,
            &target_obligation_domain,
        )?;
        // Allocation realization at the seal boundary.  Every still-live
        // correspondence collection is charged recursively; transient DAG,
        // candidate, normalized-body and component scratch were separately
        // reserved by the preflight oracle above.  A reservation bug is a
        // typed refusal rather than an unaccounted allocation.
        let realized_bytes = [
            allocation_oracle.accepted_aggregate_bytes,
            allocation_oracle.mapping_phase_bytes,
            id_obligation_ref_map_heap(&source_obligations),
            id_obligation_ref_map_heap(&target_obligations),
            successor_map_heap(&program_successors),
            id_id_map_heap(&preserved_program_successors),
            id_mapping_ref_map_heap(&source_mapping_owner),
            id_mapping_ref_map_heap(&target_mapping_owner),
            id_set_heap(&source_program_domain),
            id_set_heap(&target_program_domain),
            id_set_heap(&source_obligation_domain),
            id_set_heap(&target_obligation_domain),
            seeds
                .iter()
                .map(ObligationComponentSeedV5::allocated_bytes)
                .sum(),
            successor_map_heap(&all_obligation_successors),
            id_id_map_heap(&preserved_obligation_successors),
            obligation_entry_owner_map_heap(&source_entry_owner),
            obligation_entry_owner_map_heap(&target_entry_owner),
            entries
                .iter()
                .map(ObligationCorrespondenceEntryV5::allocated_bytes)
                .sum(),
            correspondence.allocated_bytes(),
            MAX_M6_CANONICAL_BYTES,
            MAX_M6_CORRESPONDENCE_DTO_BYTES,
        ]
        .into_iter()
        .try_fold(0_usize, checked_correspondence_working_add)?;
        if realized_bytes > working_peak_upper_bound_bytes {
            return Err(M6Error::Incomplete {
                operation: "M6 correspondence reservation underflow",
                limit: working_peak_upper_bound_bytes,
                observed: realized_bytes,
            });
        }
        Ok(M6ObligationCorrespondencePhaseV5 {
            entries,
            correspondence,
            working_peak_upper_bound_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn id(value: &str) -> StableId {
        StableId::parse(value).unwrap()
    }

    fn sha(value: usize) -> ContentHash {
        ContentHash::parse(format!("sha256:{value:064x}")).unwrap()
    }

    fn oid(value: usize) -> String {
        format!("{value:040x}")
    }

    fn tree(value: usize) -> ContentHash {
        ContentHash::parse(format!("git:{value:040x}")).unwrap()
    }

    fn replace_string(value: &mut Value, from: &str, to: &str) {
        match value {
            Value::String(value) if value == from => *value = to.to_owned(),
            Value::Array(values) => {
                for value in values {
                    replace_string(value, from, to);
                }
            }
            Value::Object(values) => {
                for value in values.values_mut() {
                    replace_string(value, from, to);
                }
            }
            _ => {}
        }
    }

    fn spaces() -> (ProgramSpace, ProgramSpace) {
        let mut source: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        source["snapshot"]["base_revision"] = Value::String(oid(9));
        source["snapshot"]["target_revision"] = Value::String(oid(10));
        source["snapshot"]["tree_hash"] = Value::String(tree(11).to_string());
        for artifact in source["artifacts"].as_array_mut().unwrap() {
            if artifact["id"] == "file:checkout-controller" {
                artifact["content_hash"] = Value::String(sha(44).to_string());
            }
        }
        let mut target = source.clone();
        replace_string(
            &mut target,
            "snapshot:double-submit-v1",
            "snapshot:double-submit-v2",
        );
        target["snapshot"]["base_revision"] = Value::String(oid(10));
        target["snapshot"]["target_revision"] = Value::String(oid(12));
        target["snapshot"]["tree_hash"] = Value::String(tree(13).to_string());
        (
            ProgramSpace::from_json_slice(&serde_json::to_vec(&source).unwrap()).unwrap(),
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target).unwrap()).unwrap(),
        )
    }

    fn spaces_with_relation_chain(length: usize) -> (ProgramSpace, ProgramSpace) {
        let (source, target) = spaces();
        let mut source_value: Value =
            serde_json::from_slice(&crate::canonical_json(&source.streaming_ref()).unwrap())
                .unwrap();
        let mut target_value: Value =
            serde_json::from_slice(&crate::canonical_json(&target.streaming_ref()).unwrap())
                .unwrap();
        let template = source_value["relations"][0].clone();
        for index in 0..length {
            let relation_id = format!("relation:chain-{index:04}");
            let target_id = if index == 0 {
                "file:checkout-controller".to_owned()
            } else {
                format!("relation:chain-{:04}", index - 1)
            };
            let mut relation = template.clone();
            relation["id"] = Value::String(relation_id);
            relation["kind"] = Value::String("dependency_chain".to_owned());
            relation["source_id"] = Value::String("file:checkout-controller".to_owned());
            relation["target_ids"] = serde_json::json!([target_id]);
            relation["directed"] = Value::Bool(true);
            relation["attributes"] = serde_json::json!({});
            source_value["relations"]
                .as_array_mut()
                .unwrap()
                .push(relation.clone());
            target_value["relations"]
                .as_array_mut()
                .unwrap()
                .push(relation);
        }
        (
            ProgramSpace::from_json_slice(&serde_json::to_vec(&source_value).unwrap()).unwrap(),
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap(),
        )
    }

    fn spaces_with_mutual_relation_scc() -> (ProgramSpace, ProgramSpace) {
        let (source, target) = spaces();
        let mut source_value: Value =
            serde_json::from_slice(&crate::canonical_json(&source.streaming_ref()).unwrap())
                .unwrap();
        let mut target_value: Value =
            serde_json::from_slice(&crate::canonical_json(&target.streaming_ref()).unwrap())
                .unwrap();
        let template = source_value["relations"][0].clone();
        for (id_value, target_id) in [
            ("relation:cycle-a", "relation:cycle-b"),
            ("relation:cycle-b", "relation:cycle-a"),
        ] {
            let mut relation = template.clone();
            relation["id"] = Value::String(id_value.to_owned());
            relation["kind"] = Value::String("mutual_dependency".to_owned());
            relation["source_id"] = Value::String("file:checkout-controller".to_owned());
            relation["target_ids"] = serde_json::json!([target_id]);
            relation["directed"] = Value::Bool(true);
            relation["attributes"] = serde_json::json!({});
            source_value["relations"]
                .as_array_mut()
                .unwrap()
                .push(relation.clone());
            target_value["relations"]
                .as_array_mut()
                .unwrap()
                .push(relation);
        }
        (
            ProgramSpace::from_json_slice(&serde_json::to_vec(&source_value).unwrap()).unwrap(),
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap(),
        )
    }

    fn spaces_with_two_disjoint_relation_sccs() -> (ProgramSpace, ProgramSpace) {
        let (source, target) = spaces_with_mutual_relation_scc();
        let mut source_value: Value =
            serde_json::from_slice(&crate::canonical_json(&source.streaming_ref()).unwrap())
                .unwrap();
        let mut target_value: Value =
            serde_json::from_slice(&crate::canonical_json(&target.streaming_ref()).unwrap())
                .unwrap();
        let template = source_value["relations"][0].clone();
        for (id_value, target_id) in [
            ("relation:cycle-c", "relation:cycle-d"),
            ("relation:cycle-d", "relation:cycle-c"),
        ] {
            let mut relation = template.clone();
            relation["id"] = Value::String(id_value.to_owned());
            relation["kind"] = Value::String("mutual_dependency".to_owned());
            relation["source_id"] = Value::String("file:payment-repository".to_owned());
            relation["target_ids"] = serde_json::json!([target_id]);
            relation["directed"] = Value::Bool(true);
            relation["attributes"] = serde_json::json!({});
            source_value["relations"]
                .as_array_mut()
                .unwrap()
                .push(relation.clone());
            target_value["relations"]
                .as_array_mut()
                .unwrap()
                .push(relation);
        }
        (
            ProgramSpace::from_json_slice(&serde_json::to_vec(&source_value).unwrap()).unwrap(),
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap(),
        )
    }

    fn closure_input(source: &ProgramSpace, target: &ProgramSpace) -> IncrementalStructuralInputV5 {
        IncrementalStructuralInputV5 {
            repository_id: source.repository_id().clone(),
            repository_identity_hash: sha(1),
            source_run_id: id("run:source"),
            source_genesis_hash: sha(2),
            source_confirmed_offset: 100,
            source_tail_hash: sha(3),
            source_event_count: 10,
            source_snapshot_id: source.snapshot_id().clone(),
            source_universe_id: id("universe:source"),
            source_index_snapshot_hash: sha(4),
            source_authority_policy_revision_hash: sha(5),
            source_authority_replay_basis_digest: sha(6),
            source_resolved_target_commit_oid: oid(10),
            source_target_tree_hash: tree(11),
            source_gluing_bundle_id: id("event:source-gluing-bundle"),
            target_run_id: id("run:target"),
            target_genesis_hash: sha(7),
            target_predecessor_offset: 200,
            target_predecessor_tail_hash: sha(8),
            target_predecessor_event_count: 20,
            target_snapshot_id: target.snapshot_id().clone(),
            target_universe_id: id("universe:target"),
            target_predecessor_index_snapshot_hash: sha(9),
            target_authority_policy_revision_hash: sha(10),
            target_pre_incremental_authority_replay_basis_digest: sha(11),
            target_resolved_base_commit_oid: oid(10),
            target_base_tree_hash: tree(11),
            target_resolved_target_commit_oid: oid(12),
            target_target_tree_hash: tree(13),
        }
    }

    fn closure(source: &ProgramSpace, target: &ProgramSpace) -> IncrementalSourceClosureV5 {
        let proof = ValidatedIncrementalStructureV5::validate_store_projection(
            source,
            target,
            closure_input(source, target),
        )
        .unwrap();
        IncrementalSourceClosureV5::from_validated_structure(proof).unwrap()
    }

    fn anchors(space: &ProgramSpace) -> BTreeMap<StableId, RustSymbolAnchorV1> {
        space
            .artifacts()
            .iter()
            .filter(|artifact| rust_symbol(artifact))
            .enumerate()
            .map(|(index, artifact)| {
                (
                    artifact.id.clone(),
                    RustSymbolAnchorV1::new(
                        match artifact.kind.as_str() {
                            "method" => RustSymbolKindV1::Method,
                            "type" => RustSymbolKindV1::Type,
                            _ => RustSymbolKindV1::Function,
                        },
                        sha(100 + index),
                        sha(200 + index),
                    )
                    .unwrap(),
                )
            })
            .collect()
    }

    fn orders(space: &ProgramSpace) -> BTreeMap<StableId, Vec<StableId>> {
        space
            .relations()
            .iter()
            .map(|relation| {
                (
                    relation.id.clone(),
                    relation.target_ids.iter().cloned().collect(),
                )
            })
            .collect()
    }

    fn inputs(source: &ProgramSpace, target: &ProgramSpace) -> ValidatedIncrementalInputsV5 {
        ValidatedIncrementalInputsV5::fixture_from_parts(
            source,
            target,
            anchors(source),
            anchors(target),
            orders(source),
            orders(target),
            vec![],
        )
        .unwrap()
    }

    fn phase() -> (IncrementalSourceClosureV5, M6MappingPhaseV5) {
        let (source, target) = spaces();
        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        (closure, phase)
    }

    fn accepted_aggregate(program: ProgramSpace) -> ReviewAggregate {
        let (universe, obligations) = crate::MvpRulePack::synthesize(&program)
            .unwrap()
            .into_parts();
        ReviewAggregate::new(program, universe, obligations).unwrap()
    }

    fn correspondence_phase() -> (
        IncrementalSourceClosureV5,
        M6MappingPhaseV5,
        ReviewAggregate,
        ReviewAggregate,
        M6ObligationCorrespondencePhaseV5,
    ) {
        let (source_program, target_program) = spaces();
        let source = accepted_aggregate(source_program);
        let target = accepted_aggregate(target_program);
        let mut input = closure_input(source.program(), target.program());
        input.source_universe_id = source.universe().id().clone();
        input.target_universe_id = target.universe().id().clone();
        let proof = ValidatedIncrementalStructureV5::validate_store_projection(
            source.program(),
            target.program(),
            input,
        )
        .unwrap();
        let closure = IncrementalSourceClosureV5::from_validated_structure(proof).unwrap();
        let mappings = ChangeMorphismV5::build_with_inputs(
            &closure,
            source.program(),
            target.program(),
            &inputs(source.program(), target.program()),
        )
        .unwrap();
        let correspondence = ObligationCorrespondenceV5::derive_from_accepted_universes(
            &closure, &mappings, &source, &target,
        )
        .unwrap();
        (closure, mappings, source, target, correspondence)
    }

    #[test]
    fn structural_projection_has_no_boolean_authority_and_fails_closed() {
        let (source, target) = spaces();
        assert!(
            ValidatedIncrementalStructureV5::validate_store_projection(
                &source,
                &target,
                closure_input(&source, &target),
            )
            .is_ok()
        );
        let mut input = closure_input(&source, &target);
        input.source_gluing_bundle_id = id("gluing-attempt-v4:wrong");
        assert!(matches!(
            ValidatedIncrementalStructureV5::validate_store_projection(&source, &target, input),
            Err(M6Error::InvalidSourceClosure(_))
        ));
        let closure = closure(&source, &target);
        assert_eq!(closure.id().kind(), "incremental-source-closure-v5");
        assert_eq!(
            closure.input().source_target_tree_hash,
            closure.input().target_base_tree_hash
        );
    }

    #[test]
    fn builder_derives_complete_domains_statuses_and_stable_ids() {
        let (source, target) = spaces();
        let closure = closure(&source, &target);
        assert!(matches!(
            ChangeMorphismV5::derive_from_accepted_program_facts(&closure, &source, &target),
            Err(M6Error::MissingAcceptedMappingFact { .. })
        ));
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        assert_eq!(
            phase.morphism().source_domain_count(),
            source.known_ids().len() as u64
        );
        assert_eq!(
            phase.morphism().target_domain_count(),
            target.known_ids().len() as u64
        );
        assert_eq!(
            phase.morphism().mapping_count(),
            source.known_ids().len() as u64
        );
        assert_eq!(phase.morphism().status_counts().modified, 1);
        assert_eq!(
            phase.morphism().status_counts().preserved + 1,
            source.known_ids().len() as u64
        );
        assert_eq!(
            phase.morphism().source_ids(),
            &BTreeSet::from([closure.id().clone()])
        );
        assert_eq!(phase.morphism().id().kind(), "change-morphism-v5");
        assert!(phase.working_peak_upper_bound_bytes() > 0);
        assert!(phase.working_peak_upper_bound_bytes() <= MAX_M6_MAPPING_WORKING_BYTES);
        assert_eq!(
            phase
                .mappings()
                .iter()
                .find(|mapping| mapping.object_kind() == ProgramObjectKindV5::Snapshot)
                .unwrap()
                .status(),
            MappingStatusV5::Modified
        );
    }

    #[test]
    fn mapping_domain_binding_uses_streaming_program_fact_set_not_snapshot_id() {
        let (source, target) = spaces();
        let closure = closure(&source, &target);
        let mapping = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        let (count, digest) = target.m6_known_id_domain().unwrap();
        assert_eq!(count, target.known_ids().len());
        assert_eq!(digest, *mapping.morphism().target_domain_digest());

        // Keep the accepted snapshot fields byte-for-byte identical while
        // changing one accepted fact ID.  This is the mixed-domain case that
        // snapshot-only phase validation would silently admit.
        let mut foreign: Value =
            serde_json::from_slice(&crate::canonical_json(&target.streaming_ref()).unwrap())
                .unwrap();
        foreign["relations"][0]["id"] = Value::String("relation:foreign-domain".to_owned());
        let foreign =
            ProgramSpace::from_json_slice(&serde_json::to_vec(&foreign).unwrap()).unwrap();
        assert_eq!(foreign.snapshot_id(), target.snapshot_id());
        assert!(matches!(
            validate_mapping_program_domains(&mapping, &source, &foreign),
            Err(M6Error::InvalidHistoricalTopology(
                "mapping ProgramSpace domain does not equal the pinned source/target facts"
            ))
        ));
    }

    #[test]
    fn staleness_phase_retained_oracle_charges_outer_and_body_vector_capacity() {
        let (_closure, mut mappings, _source, _target, mut correspondence) = correspondence_phase();
        let before_mapping_capacity = mappings.mappings.capacity();
        let before_entry_capacity = correspondence.entries.capacity();
        let before =
            staleness_mapping_correspondence_retained_bytes(&mappings, &correspondence).unwrap();
        mappings.mappings.reserve(32);
        correspondence.entries.reserve(32);
        let after_mapping_capacity = mappings.mappings.capacity();
        let after_entry_capacity = correspondence.entries.capacity();
        let after =
            staleness_mapping_correspondence_retained_bytes(&mappings, &correspondence).unwrap();
        assert!(after_mapping_capacity > before_mapping_capacity);
        assert!(after_entry_capacity > before_entry_capacity);
        assert_eq!(
            after.0 - before.0,
            (after_mapping_capacity - before_mapping_capacity)
                * std::mem::size_of::<ProgramMappingV5>()
        );
        assert_eq!(
            after.1 - before.1,
            (after_entry_capacity - before_entry_capacity)
                * std::mem::size_of::<ObligationCorrespondenceEntryV5>()
        );

        let mapping = mappings.mappings.first_mut().unwrap();
        let before = mapping.allocated_bytes();
        let before_capacity = mapping.source_body_hashes.capacity();
        mapping.source_body_hashes.reserve(32);
        let after_capacity = mapping.source_body_hashes.capacity();
        let after = mapping.allocated_bytes();
        assert!(after_capacity > before_capacity);
        assert_eq!(
            after - before,
            (after_capacity - before_capacity) * std::mem::size_of::<IdBodyHashV5>()
        );

        let entry = correspondence.entries.first_mut().unwrap();
        let before = entry.allocated_bytes();
        let before_capacity = entry.target_body_hashes.capacity();
        entry.target_body_hashes.reserve(32);
        let after_capacity = entry.target_body_hashes.capacity();
        let after = entry.allocated_bytes();
        assert!(after_capacity > before_capacity);
        assert_eq!(
            after - before,
            (after_capacity - before_capacity) * std::mem::size_of::<IdBodyHashV5>()
        );

        correspondence.entries = Vec::with_capacity(17);
        let (_, empty_correspondence) =
            staleness_mapping_correspondence_retained_bytes(&mappings, &correspondence).unwrap();
        assert_eq!(
            empty_correspondence,
            correspondence.correspondence.allocated_bytes()
                + correspondence.entries.capacity()
                    * std::mem::size_of::<ObligationCorrespondenceEntryV5>()
        );
    }

    #[test]
    fn correspondence_derives_complete_preserved_domains_and_exact_traces() {
        let (_closure, mappings, source, target, phase) = correspondence_phase();
        assert_eq!(
            phase.correspondence().source_domain_count(),
            source.universe().raw_denominator() as u64
        );
        assert_eq!(
            phase.correspondence().target_domain_count(),
            target.universe().raw_denominator() as u64
        );
        assert_eq!(
            phase.correspondence().entry_count(),
            phase.entries().len() as u64
        );
        assert_eq!(
            phase.correspondence().status_counts().preserved,
            source.universe().raw_denominator() as u64
        );
        assert_eq!(
            phase.correspondence().source_ids(),
            &BTreeSet::from([
                mappings.morphism().id().clone(),
                source.universe().id().clone(),
                target.universe().id().clone(),
            ])
        );
        let source_domain = phase
            .entries()
            .iter()
            .flat_map(|entry| entry.from_obligation_ids().iter().cloned())
            .collect::<BTreeSet<_>>();
        let target_domain = phase
            .entries()
            .iter()
            .flat_map(|entry| entry.to_obligation_ids().iter().cloned())
            .collect::<BTreeSet<_>>();
        assert_eq!(source_domain, *source.universe().obligation_ids());
        assert_eq!(target_domain, *target.universe().obligation_ids());
        for entry in phase.entries() {
            let expected_sources = std::iter::once(mappings.morphism().id().clone())
                .chain(entry.source_mapping_ids().iter().cloned())
                .chain(entry.predecessor_entry_ids().iter().cloned())
                .chain(entry.from_obligation_ids().iter().cloned())
                .chain(entry.to_obligation_ids().iter().cloned())
                .collect::<BTreeSet<_>>();
            assert_eq!(entry.source_ids(), &expected_sources);
            assert_eq!(entry.successor_obligation_ids(), entry.to_obligation_ids());
        }
        assert!(phase.working_peak_upper_bound_bytes() <= MAX_M6_CORRESPONDENCE_WORKING_BYTES);
    }

    #[test]
    fn correspondence_wire_is_strict_canonical_and_recomputed_byte_for_byte() {
        let (closure, mappings, source, target, phase) = correspondence_phase();
        let entry_bytes = phase
            .entries()
            .iter()
            .map(|entry| crate::canonical_json(entry).unwrap())
            .collect::<Vec<_>>();
        let seal_bytes = crate::canonical_json(phase.correspondence()).unwrap();
        phase
            .validate_replayed_canonical(&entry_bytes, &seal_bytes)
            .unwrap();
        let repeated = ObligationCorrespondenceV5::derive_from_accepted_universes(
            &closure, &mappings, &source, &target,
        )
        .unwrap();
        assert_eq!(repeated.entries(), phase.entries());
        assert_eq!(repeated.correspondence(), phase.correspondence());

        let mut tampered: Value = serde_json::from_slice(&entry_bytes[0]).unwrap();
        tampered["successor_obligation_ids"] = serde_json::json!([]);
        assert!(
            ObligationCorrespondenceEntryV5::from_json_bytes(
                &crate::canonical_json(&tampered).unwrap()
            )
            .is_err()
        );
        let mut unknown: Value = serde_json::from_slice(&entry_bytes[0]).unwrap();
        unknown["caller_authority"] = Value::Bool(true);
        assert!(
            ObligationCorrespondenceEntryV5::from_json_bytes(
                &crate::canonical_json(&unknown).unwrap()
            )
            .is_err()
        );
        let mut seal_tamper: Value = serde_json::from_slice(&seal_bytes).unwrap();
        seal_tamper["entry_count"] = serde_json::json!(999);
        assert!(
            ObligationCorrespondenceV5::from_json_bytes(
                &crate::canonical_json(&seal_tamper).unwrap(),
                phase.correspondence(),
            )
            .is_err()
        );
        let original = &phase.entries()[0];
        let mut trace_only_tamper = original.clone();
        trace_only_tamper.successor_obligation_ids.clear();
        assert_eq!(trace_only_tamper.id(), original.id());
        assert_ne!(
            trace_only_tamper.body_hash().unwrap(),
            original.body_hash().unwrap(),
            "successors are excluded from identity but protected by the complete body hash"
        );
        let mut identity_change = ObligationCorrespondenceEntryPartsV5 {
            morphism_id: original.morphism_id.clone(),
            from_obligation_ids: original.from_obligation_ids.clone(),
            to_obligation_ids: original.to_obligation_ids.clone(),
            status: MappingStatusV5::Modified,
            source_mapping_ids: original.source_mapping_ids.clone(),
            predecessor_entry_ids: original.predecessor_entry_ids.clone(),
            source_body_hashes: original.source_body_hashes.clone(),
            target_body_hashes: original.target_body_hashes.clone(),
        };
        if original.status() == MappingStatusV5::Modified {
            identity_change.status = MappingStatusV5::Preserved;
        }
        let changed_identity =
            ObligationCorrespondenceEntryV5::from_parts(identity_change).unwrap();
        assert_ne!(changed_identity.id(), original.id());
    }

    #[test]
    fn obligation_dependency_cycles_and_external_ids_are_typed_invalid_universe() {
        let (_closure, _mappings, source, _target, _phase) = correspondence_phase();
        let template = source.obligations().next().unwrap();
        let make = |id_value: &str, dependencies: &[&str]| {
            let mut value = serde_json::to_value(template).unwrap();
            value["id"] = Value::String(id_value.to_owned());
            value["depends_on"] = serde_json::json!(dependencies);
            value["normalized_depends_on"] = serde_json::json!(dependencies);
            serde_json::from_value::<Obligation>(value).unwrap()
        };
        let cyclic = [
            make("obligation:cycle-a", &["obligation:cycle-b"]),
            make("obligation:cycle-b", &["obligation:cycle-a"]),
        ];
        let cyclic_map = cyclic
            .iter()
            .map(|obligation| (obligation.id().clone(), obligation))
            .collect::<BTreeMap<_, _>>();
        assert!(matches!(
            validate_obligation_dependency_dag(&cyclic_map),
            Err(M6Error::InvalidObligationUniverse(
                "obligation dependency graph contains a cycle"
            ))
        ));

        let external = [make(
            "obligation:external-owner",
            &["obligation:outside-universe"],
        )];
        let external_map = external
            .iter()
            .map(|obligation| (obligation.id().clone(), obligation))
            .collect::<BTreeMap<_, _>>();
        assert!(matches!(
            validate_obligation_dependency_dag(&external_map),
            Err(M6Error::InvalidObligationUniverse(
                "obligation dependency is outside its accepted universe"
            ))
        ));
    }

    #[test]
    fn correspondence_rejects_constructor_valid_but_non_synthesized_obligation_bodies() {
        let (closure, mappings, source, target, _phase) = correspondence_phase();
        let mut obligations = target.obligations().cloned().collect::<Vec<_>>();
        let mut value = serde_json::to_value(&obligations[0]).unwrap();
        value["weight"] = serde_json::json!(99.0);
        obligations[0] = serde_json::from_value(value).unwrap();
        let forged = ReviewAggregate::new(
            target.program().clone(),
            target.universe().clone(),
            obligations,
        )
        .unwrap();
        assert!(matches!(
            ObligationCorrespondenceV5::derive_from_accepted_universes(
                &closure, &mappings, &source, &forged,
            ),
            Err(M6Error::InvalidObligationUniverse(
                "accepted obligation body differs from deterministic rule-pack synthesis"
            ))
        ));
    }

    #[test]
    fn correspondence_rejects_reordered_path_targets_before_cross_snapshot_normalization() {
        let (closure, mappings, source, target, _phase) = correspondence_phase();
        let mut obligations = target.obligations().cloned().collect::<Vec<_>>();
        let path_index = obligations
            .iter()
            .position(|obligation| obligation.target_refs().len() > 1)
            .expect("reference scenario must contain an ordered path obligation");
        let original_id = obligations[path_index].id().clone();
        let mut value = serde_json::to_value(&obligations[path_index]).unwrap();
        value["target_refs"].as_array_mut().unwrap().reverse();
        obligations[path_index] = serde_json::from_value(value).unwrap();
        assert_eq!(obligations[path_index].id(), &original_id);
        assert_eq!(
            obligations[path_index].normalized_target_refs(),
            target
                .obligations()
                .find(|obligation| obligation.id() == &original_id)
                .unwrap()
                .normalized_target_refs()
        );
        let forged = ReviewAggregate::new(
            target.program().clone(),
            target.universe().clone(),
            obligations,
        )
        .unwrap();
        assert!(matches!(
            ObligationCorrespondenceV5::derive_from_accepted_universes(
                &closure, &mappings, &source, &forged,
            ),
            Err(M6Error::InvalidObligationUniverse(
                "accepted obligation body differs from deterministic rule-pack synthesis"
            ))
        ));
    }

    #[test]
    fn deterministic_program_body_change_is_modified_and_policy_change_rebinds_seal() {
        let (_baseline_closure, _baseline_mappings, _source, _target, baseline) =
            correspondence_phase();
        let (source_program, target_program) = spaces();
        let mut target_value: Value = serde_json::from_slice(
            &crate::canonical_json(&target_program.streaming_ref()).unwrap(),
        )
        .unwrap();
        target_value["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["id"] == "function:checkout-submit")
            .unwrap()["attributes"]["body_revision"] = Value::String("2".to_owned());
        target_value["profile"]["policy_version"] = Value::String("default@2".to_owned());
        let target_program =
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap();
        let source = accepted_aggregate(source_program);
        let target = accepted_aggregate(target_program);
        let mut input = closure_input(source.program(), target.program());
        input.source_universe_id = source.universe().id().clone();
        input.target_universe_id = target.universe().id().clone();
        let proof = ValidatedIncrementalStructureV5::validate_store_projection(
            source.program(),
            target.program(),
            input,
        )
        .unwrap();
        let closure = IncrementalSourceClosureV5::from_validated_structure(proof).unwrap();
        let mappings = ChangeMorphismV5::build_with_inputs(
            &closure,
            source.program(),
            target.program(),
            &inputs(source.program(), target.program()),
        )
        .unwrap();
        let changed = ObligationCorrespondenceV5::derive_from_accepted_universes(
            &closure, &mappings, &source, &target,
        )
        .unwrap();
        assert!(changed.correspondence().status_counts().modified > 0);
        assert_ne!(
            changed.correspondence().target_universe_id(),
            baseline.correspondence().target_universe_id()
        );
        assert_ne!(
            changed.correspondence().id(),
            baseline.correspondence().id()
        );
    }

    #[test]
    fn correspondence_dependency_stages_cross_255_without_truncation() {
        let (_closure, _mappings, source, _target, _phase) = correspondence_phase();
        let template = source.obligations().next().unwrap();
        let obligations = (0..300)
            .map(|index| {
                let mut value = serde_json::to_value(template).unwrap();
                let obligation_id = format!("obligation:chain-{index:04}");
                let dependencies = if index == 0 {
                    Vec::<String>::new()
                } else {
                    vec![format!("obligation:chain-{:04}", index - 1)]
                };
                value["id"] = Value::String(obligation_id);
                value["depends_on"] = serde_json::json!(dependencies);
                value["normalized_depends_on"] = serde_json::json!(dependencies);
                serde_json::from_value::<Obligation>(value).unwrap()
            })
            .collect::<Vec<_>>();
        let obligation_map = obligations
            .iter()
            .map(|obligation| (obligation.id().clone(), obligation))
            .collect::<BTreeMap<_, _>>();
        let depths = validate_obligation_dependency_dag(&obligation_map).unwrap();
        assert_eq!(depths[&id("obligation:chain-0299")], 299);
        let seeds = obligations
            .iter()
            .map(|obligation| ObligationComponentSeedV5 {
                from_ids: BTreeSet::from([obligation.id().clone()]),
                to_ids: BTreeSet::from([obligation.id().clone()]),
                stage: 0,
            })
            .collect();
        let staged =
            collapse_and_stage_obligation_components(seeds, &obligation_map, &obligation_map)
                .unwrap();
        assert_eq!(staged.last().unwrap().stage, 299);
    }

    #[test]
    fn correspondence_entries_close_split_merge_ambiguity_and_all_status_counts() {
        let (_closure, mappings, source, target, _phase) = correspondence_phase();
        let mut serial = 1_usize;
        fn next_ids(side: &str, count: usize, serial: &mut usize) -> BTreeSet<StableId> {
            (0..count)
                .map(|_| {
                    let value = id(&format!("obligation:{side}-{:02}", *serial));
                    *serial += 1;
                    value
                })
                .collect::<BTreeSet<_>>()
        }
        let shapes = [
            (MappingStatusV5::Preserved, 1, 1),
            (MappingStatusV5::Modified, 1, 1),
            (MappingStatusV5::Added, 0, 1),
            (MappingStatusV5::Removed, 1, 0),
            (MappingStatusV5::Split, 1, 2),
            (MappingStatusV5::Merged, 2, 1),
            (MappingStatusV5::Unresolved, 2, 2),
        ];
        let mut entries = shapes
            .into_iter()
            .map(|(status, from_count, to_count)| {
                let from_ids = next_ids("source", from_count, &mut serial);
                let to_ids = next_ids("target", to_count, &mut serial);
                let source_body_hashes = from_ids
                    .iter()
                    .map(|id| IdBodyHashV5::new(id.clone(), sha(serial)).unwrap())
                    .collect();
                serial += 1;
                let target_body_hashes = to_ids
                    .iter()
                    .map(|id| IdBodyHashV5::new(id.clone(), sha(serial)).unwrap())
                    .collect();
                serial += 1;
                ObligationCorrespondenceEntryV5::from_parts(ObligationCorrespondenceEntryPartsV5 {
                    morphism_id: mappings.morphism().id().clone(),
                    from_obligation_ids: from_ids,
                    to_obligation_ids: to_ids,
                    status,
                    source_mapping_ids: BTreeSet::new(),
                    predecessor_entry_ids: BTreeSet::new(),
                    source_body_hashes,
                    target_body_hashes,
                })
                .unwrap()
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.id().cmp(right.id()));
        let source_domain = entries
            .iter()
            .flat_map(|entry| entry.from_obligation_ids().iter().cloned())
            .collect::<BTreeSet<_>>();
        let target_domain = entries
            .iter()
            .flat_map(|entry| entry.to_obligation_ids().iter().cloned())
            .collect::<BTreeSet<_>>();
        let seal = ObligationCorrespondenceV5::seal_derived(
            mappings.morphism(),
            source.universe().id(),
            target.universe().id(),
            &entries,
            &source_domain,
            &target_domain,
        )
        .unwrap();
        assert_eq!(
            seal.status_counts(),
            &MappingStatusCountsV5 {
                preserved: 1,
                modified: 1,
                added: 1,
                removed: 1,
                split: 1,
                merged: 1,
                unresolved: 1,
            }
        );
        let mut incomplete_source_domain = source_domain.clone();
        incomplete_source_domain.pop_first();
        assert!(matches!(
            ObligationCorrespondenceV5::seal_derived(
                mappings.morphism(),
                source.universe().id(),
                target.universe().id(),
                &entries,
                &incomplete_source_domain,
                &target_domain,
            ),
            Err(M6Error::InvalidObligationUniverse(_))
        ));
    }

    #[test]
    fn obligation_candidate_key_is_sensitive_to_every_adr_field() {
        let (_closure, mappings, _source, target, _phase) = correspondence_phase();
        let obligation = target
            .obligations()
            .find(|obligation| !obligation.normalized_context_ids().is_empty())
            .unwrap();
        let successors = mappings
            .mappings()
            .iter()
            .flat_map(|mapping| {
                mapping
                    .from_ids()
                    .iter()
                    .cloned()
                    .map(move |id| (id, mapping.to_ids().clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let target_domain = target.program().known_ids();
        let base = obligation_candidate_key(
            obligation,
            ObligationSideV5::Target,
            &target_domain,
            &successors,
        )
        .unwrap();
        for mutation in [
            "rule",
            "property_id",
            "property_version",
            "target_kind",
            "semantic_key",
            "target_refs",
            "context_ids",
            "generator_ids",
        ] {
            let mut value = serde_json::to_value(obligation).unwrap();
            match mutation {
                "rule" => value["version"]["rule"] = Value::String("changed.rule@2".to_owned()),
                "property_id" => {
                    value["property_id"] = Value::String("changed.property".to_owned())
                }
                "property_version" => value["property_version"] = Value::String("2".to_owned()),
                "target_kind" => value["target_kind"] = Value::String("changed_kind".to_owned()),
                "semantic_key" => value["semantic_key"] = Value::String("changed|key".to_owned()),
                "target_refs" => {
                    value["target_refs"] = serde_json::json!(["file:checkout-controller"]);
                    value["normalized_target_refs"] =
                        serde_json::json!(["file:checkout-controller"]);
                }
                "context_ids" => {
                    value["context_ids"] = serde_json::json!([]);
                    value["normalized_context_ids"] = serde_json::json!([]);
                }
                "generator_ids" => {
                    value["generator_ids"] = serde_json::json!(["file:checkout-controller"]);
                }
                _ => unreachable!(),
            }
            let changed: Obligation = serde_json::from_value(value).unwrap();
            let key = obligation_candidate_key(
                &changed,
                ObligationSideV5::Target,
                &target_domain,
                &successors,
            )
            .unwrap();
            assert_ne!(base, key, "candidate field {mutation} must be semantic");
        }
    }

    #[test]
    fn source_only_change_stays_one_candidate_and_is_modified_by_complete_body() {
        let (_closure, mappings, _source, target, _phase) = correspondence_phase();
        let obligation = target.obligations().next().unwrap();
        let target_domain = target.program().known_ids();
        let obligation_domain = target
            .obligations()
            .map(|obligation| obligation.id().clone())
            .collect::<BTreeSet<_>>();
        let successors = mappings
            .mappings()
            .iter()
            .flat_map(|mapping| {
                mapping
                    .from_ids()
                    .iter()
                    .cloned()
                    .map(move |id| (id, mapping.to_ids().clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let replacement = target_domain
            .iter()
            .find(|id| !obligation.normalized_source_ids().contains(*id))
            .unwrap()
            .clone();
        let mut value = serde_json::to_value(obligation).unwrap();
        value["source_ids"] = serde_json::json!([replacement]);
        value["normalized_source_ids"] = value["source_ids"].clone();
        let changed: Obligation = serde_json::from_value(value).unwrap();

        let base_key = obligation_candidate_key(
            obligation,
            ObligationSideV5::Target,
            &target_domain,
            &successors,
        )
        .unwrap();
        let changed_key = obligation_candidate_key(
            &changed,
            ObligationSideV5::Target,
            &target_domain,
            &successors,
        )
        .unwrap();
        assert_eq!(
            base_key, changed_key,
            "source IDs are a complete-body equality field, not a quotient candidate key"
        );
        let base_hash = normalized_obligation_body_hash(
            obligation,
            ObligationSideV5::Target,
            &target_domain,
            &obligation_domain,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        let changed_hash = normalized_obligation_body_hash(
            &changed,
            ObligationSideV5::Target,
            &target_domain,
            &obligation_domain,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert_ne!(
            base_hash, changed_hash,
            "a one-to-one source-only change must classify Modified, never Preserved"
        );
        assert_eq!(
            one_to_one_obligation_status(true, true, &base_hash, &changed_hash),
            MappingStatusV5::Modified
        );
    }

    #[test]
    fn correspondence_count_byte_and_memory_bounds_are_exact_and_overflow_closed() {
        assert!(
            bounded(
                MAX_M6_OBLIGATIONS_PER_UNIVERSE,
                MAX_M6_OBLIGATIONS_PER_UNIVERSE,
                "fixture obligation bound",
            )
            .is_ok()
        );
        assert!(matches!(
            bounded(
                MAX_M6_OBLIGATIONS_PER_UNIVERSE + 1,
                MAX_M6_OBLIGATIONS_PER_UNIVERSE,
                "fixture obligation bound",
            ),
            Err(M6Error::Incomplete { .. })
        ));
        assert!(
            bounded(
                MAX_M6_CORRESPONDENCE_ENTRIES,
                MAX_M6_CORRESPONDENCE_ENTRIES,
                "fixture correspondence entry bound",
            )
            .is_ok()
        );
        assert!(matches!(
            bounded(
                MAX_M6_CORRESPONDENCE_ENTRIES + 1,
                MAX_M6_CORRESPONDENCE_ENTRIES,
                "fixture correspondence entry bound",
            ),
            Err(M6Error::Incomplete { .. })
        ));
        assert_eq!(
            preflight_event_line(MAX_M6_CORRESPONDENCE_DTO_BYTES - 1, 1).unwrap(),
            MAX_M6_CORRESPONDENCE_DTO_BYTES
        );
        assert!(matches!(
            preflight_event_line(MAX_M6_CORRESPONDENCE_DTO_BYTES, 1),
            Err(M6Error::Incomplete { .. })
        ));
        assert_eq!(
            checked_correspondence_working_add(MAX_M6_CORRESPONDENCE_WORKING_BYTES - 1, 1).unwrap(),
            MAX_M6_CORRESPONDENCE_WORKING_BYTES
        );
        assert!(matches!(
            checked_correspondence_working_add(MAX_M6_CORRESPONDENCE_WORKING_BYTES, 1),
            Err(M6Error::Incomplete { .. })
        ));
        assert!(matches!(
            checked_correspondence_working_add(usize::MAX, 1),
            Err(M6Error::Incomplete {
                observed: usize::MAX,
                ..
            })
        ));

        let (closure, mappings, source, target, phase) = correspondence_phase();
        let actual_oracle = correspondence_allocation_oracle(&mappings, &source, &target).unwrap();
        let actual_reservation = actual_oracle.reservation_bytes().unwrap();
        assert_eq!(
            phase.working_peak_upper_bound_bytes(),
            actual_reservation,
            "the phase exposes the preflight oracle reservation, not a post-hoc subset"
        );
        CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS.with(|calls| calls.set(0));
        assert!(matches!(
            ObligationCorrespondenceV5::derive_with_working_limit(
                &closure,
                &mappings,
                &source,
                &target,
                actual_reservation - 1,
            ),
            Err(M6Error::Incomplete {
                operation: "M6 correspondence preflight working bytes",
                ..
            })
        ));
        CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS.with(|calls| assert_eq!(calls.get(), 0));

        let exact_phase = ObligationCorrespondenceV5::derive_with_working_limit(
            &closure,
            &mappings,
            &source,
            &target,
            actual_reservation,
        )
        .unwrap();
        assert_eq!(
            exact_phase.correspondence(),
            phase.correspondence(),
            "the actual production reservation is inclusive"
        );
        CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS.with(|calls| assert_eq!(calls.get(), 2));

        CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS.with(|calls| calls.set(0));
        ObligationCorrespondenceV5::derive_from_accepted_universes(
            &closure, &mappings, &source, &target,
        )
        .unwrap();
        CORRESPONDENCE_AGGREGATE_VALIDATION_CALLS.with(|calls| assert_eq!(calls.get(), 2));
        let from_ids = (0..MAX_M6_CORRESPONDENCE_SIDE_IDS)
            .map(|index| id(&format!("obligation:bound-source-{index:02}")))
            .collect::<BTreeSet<_>>();
        let to_ids = (0..MAX_M6_CORRESPONDENCE_SIDE_IDS)
            .map(|index| id(&format!("obligation:bound-target-{index:02}")))
            .collect::<BTreeSet<_>>();
        let hashes = |ids: &BTreeSet<StableId>| {
            ids.iter()
                .enumerate()
                .map(|(index, id)| IdBodyHashV5::new(id.clone(), sha(900 + index)).unwrap())
                .collect::<Vec<_>>()
        };
        let exact =
            ObligationCorrespondenceEntryV5::from_parts(ObligationCorrespondenceEntryPartsV5 {
                morphism_id: mappings.morphism().id().clone(),
                from_obligation_ids: from_ids.clone(),
                to_obligation_ids: to_ids.clone(),
                status: MappingStatusV5::Unresolved,
                source_mapping_ids: BTreeSet::new(),
                predecessor_entry_ids: BTreeSet::new(),
                source_body_hashes: hashes(&from_ids),
                target_body_hashes: hashes(&to_ids),
            })
            .unwrap();
        assert_eq!(
            exact.from_obligation_ids().len(),
            MAX_M6_CORRESPONDENCE_SIDE_IDS
        );
        let mut over_from = from_ids;
        over_from.insert(id("obligation:bound-source-over"));
        assert!(matches!(
            ObligationCorrespondenceEntryV5::from_parts(ObligationCorrespondenceEntryPartsV5 {
                morphism_id: mappings.morphism().id().clone(),
                from_obligation_ids: over_from.clone(),
                to_obligation_ids: to_ids,
                status: MappingStatusV5::Unresolved,
                source_mapping_ids: BTreeSet::new(),
                predecessor_entry_ids: BTreeSet::new(),
                source_body_hashes: hashes(&over_from),
                target_body_hashes: Vec::new(),
            },),
            Err(M6Error::Incomplete { .. })
        ));

        let predecessor_ids = (0..MAX_M6_CORRESPONDENCE_PREDECESSOR_IDS)
            .map(|index| {
                id(&format!(
                    "obligation-correspondence-entry-v5:predecessor-{index:02}"
                ))
            })
            .collect::<BTreeSet<_>>();
        let one_source = BTreeSet::from([id("obligation:predecessor-bound-source")]);
        let one_target = BTreeSet::from([id("obligation:predecessor-bound-target")]);
        assert!(
            ObligationCorrespondenceEntryV5::from_parts(ObligationCorrespondenceEntryPartsV5 {
                morphism_id: mappings.morphism().id().clone(),
                from_obligation_ids: one_source.clone(),
                to_obligation_ids: one_target.clone(),
                status: MappingStatusV5::Modified,
                source_mapping_ids: BTreeSet::new(),
                predecessor_entry_ids: predecessor_ids.clone(),
                source_body_hashes: hashes(&one_source),
                target_body_hashes: hashes(&one_target),
            },)
            .is_ok()
        );
        let mut over_predecessors = predecessor_ids;
        over_predecessors.insert(id("obligation-correspondence-entry-v5:predecessor-over"));
        assert!(matches!(
            ObligationCorrespondenceEntryV5::from_parts(ObligationCorrespondenceEntryPartsV5 {
                morphism_id: mappings.morphism().id().clone(),
                from_obligation_ids: one_source.clone(),
                to_obligation_ids: one_target.clone(),
                status: MappingStatusV5::Modified,
                source_mapping_ids: BTreeSet::new(),
                predecessor_entry_ids: over_predecessors,
                source_body_hashes: hashes(&one_source),
                target_body_hashes: hashes(&one_target),
            },),
            Err(M6Error::Incomplete { .. })
        ));
    }

    #[test]
    fn component_reduction_and_count_object_close_all_seven_statuses() {
        let source_repository = ProgramNodeV5 {
            id: id("repository:source"),
            object_kind: ProgramObjectKindV5::Repository,
            body_hash: sha(1),
            data: NodeDataV5::Repository {
                identity: "same".to_owned(),
            },
        };
        let target_repository = ProgramNodeV5 {
            id: id("repository:target"),
            object_kind: ProgramObjectKindV5::Repository,
            body_hash: sha(2),
            data: NodeDataV5::Repository {
                identity: "same".to_owned(),
            },
        };
        let source_snapshot = ProgramNodeV5 {
            id: id("snapshot:source"),
            object_kind: ProgramObjectKindV5::Snapshot,
            body_hash: sha(3),
            data: NodeDataV5::Snapshot,
        };
        let target_snapshot = ProgramNodeV5 {
            id: id("snapshot:target"),
            object_kind: ProgramObjectKindV5::Snapshot,
            body_hash: sha(4),
            data: NodeDataV5::Snapshot,
        };
        let source_nodes = BTreeMap::from([
            (source_repository.id.clone(), &source_repository),
            (source_snapshot.id.clone(), &source_snapshot),
        ]);
        let target_nodes = BTreeMap::from([
            (target_repository.id.clone(), &target_repository),
            (target_snapshot.id.clone(), &target_snapshot),
        ]);
        let successors = BTreeMap::from([
            (
                source_repository.id.clone(),
                BTreeSet::from([target_repository.id.clone()]),
            ),
            (
                source_snapshot.id.clone(),
                BTreeSet::from([target_snapshot.id.clone()]),
            ),
        ]);
        let seed = |from_ids, to_ids, object_kind, candidate_key_kind| ComponentSeedV5 {
            object_kind,
            candidate_key_kind,
            from_ids,
            to_ids,
            stage: 0,
        };
        let statuses = [
            component_status(
                &seed(
                    BTreeSet::from([source_repository.id.clone()]),
                    BTreeSet::from([target_repository.id.clone()]),
                    ProgramObjectKindV5::Repository,
                    CandidateKeyKindV5::RepositoryIdentity,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
            component_status(
                &seed(
                    BTreeSet::from([source_snapshot.id.clone()]),
                    BTreeSet::from([target_snapshot.id.clone()]),
                    ProgramObjectKindV5::Snapshot,
                    CandidateKeyKindV5::SnapshotPair,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
            component_status(
                &seed(
                    BTreeSet::new(),
                    BTreeSet::from([id("artifact:added")]),
                    ProgramObjectKindV5::Artifact,
                    CandidateKeyKindV5::NoCandidate,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
            component_status(
                &seed(
                    BTreeSet::from([id("artifact:removed")]),
                    BTreeSet::new(),
                    ProgramObjectKindV5::Artifact,
                    CandidateKeyKindV5::NoCandidate,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
            component_status(
                &seed(
                    BTreeSet::from([id("artifact:s")]),
                    BTreeSet::from([id("artifact:t1"), id("artifact:t2")]),
                    ProgramObjectKindV5::Artifact,
                    CandidateKeyKindV5::SamePath,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
            component_status(
                &seed(
                    BTreeSet::from([id("artifact:s1"), id("artifact:s2")]),
                    BTreeSet::from([id("artifact:t")]),
                    ProgramObjectKindV5::Artifact,
                    CandidateKeyKindV5::SamePath,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
            component_status(
                &seed(
                    BTreeSet::from([id("artifact:s1"), id("artifact:s2")]),
                    BTreeSet::from([id("artifact:t1"), id("artifact:t2")]),
                    ProgramObjectKindV5::Artifact,
                    CandidateKeyKindV5::SamePath,
                ),
                &source_nodes,
                &target_nodes,
                &successors,
            ),
        ];
        assert_eq!(
            statuses,
            [
                MappingStatusV5::Preserved,
                MappingStatusV5::Modified,
                MappingStatusV5::Added,
                MappingStatusV5::Removed,
                MappingStatusV5::Split,
                MappingStatusV5::Merged,
                MappingStatusV5::Unresolved,
            ]
        );
        let mut counts = MappingStatusCountsV5::default();
        for status in statuses {
            counts.record(status);
        }
        assert_eq!(
            serde_json::to_value(counts).unwrap(),
            serde_json::json!({
                "added": 1, "merged": 1, "modified": 1, "preserved": 1,
                "removed": 1, "split": 1, "unresolved": 1
            })
        );
    }

    #[test]
    fn ambiguous_endpoint_correspondence_propagates_unresolved_to_one_to_one_relation() {
        let source_relation = ProgramNodeV5 {
            id: id("relation:source-ambiguous"),
            object_kind: ProgramObjectKindV5::Relation,
            body_hash: sha(10),
            data: NodeDataV5::Relation {
                kind: "calls".to_owned(),
                directed: true,
                source_id: id("function:source"),
                ordered_target_ids: vec![id("function:callee-source")],
                base_hash: sha(11),
                provenance: NormalizedProvenanceV5 {
                    stable_hash: sha(20),
                    revision: NormalizedRevisionRefV5::Absent,
                    content: NormalizedContentRefV5::Absent,
                    local: NormalizedLocalRefV5::Absent,
                },
            },
        };
        let target_relation = ProgramNodeV5 {
            id: id("relation:target-ambiguous"),
            object_kind: ProgramObjectKindV5::Relation,
            body_hash: sha(12),
            data: NodeDataV5::Relation {
                kind: "calls".to_owned(),
                directed: true,
                source_id: id("function:target-a"),
                ordered_target_ids: vec![id("function:callee-target")],
                base_hash: sha(11),
                provenance: NormalizedProvenanceV5 {
                    stable_hash: sha(20),
                    revision: NormalizedRevisionRefV5::Absent,
                    content: NormalizedContentRefV5::Absent,
                    local: NormalizedLocalRefV5::Absent,
                },
            },
        };
        let seed = ComponentSeedV5 {
            object_kind: ProgramObjectKindV5::Relation,
            candidate_key_kind: CandidateKeyKindV5::MappedDirectedEndpoints,
            from_ids: BTreeSet::from([source_relation.id.clone()]),
            to_ids: BTreeSet::from([target_relation.id.clone()]),
            stage: 10,
        };
        let source_by_id = BTreeMap::from([(source_relation.id.clone(), &source_relation)]);
        let target_by_id = BTreeMap::from([(target_relation.id.clone(), &target_relation)]);
        let successors = BTreeMap::from([
            (
                id("function:source"),
                BTreeSet::from([id("function:target-a"), id("function:target-b")]),
            ),
            (
                id("function:callee-source"),
                BTreeSet::from([id("function:callee-target")]),
            ),
        ]);
        assert!(relation_candidate(
            &source_relation,
            &target_relation,
            &successors
        ));
        assert_eq!(
            component_status(&seed, &source_by_id, &target_by_id, &successors),
            MappingStatusV5::Unresolved
        );
    }

    #[test]
    fn same_stable_id_without_candidate_remains_side_qualified_removed_and_added() {
        let source = ProgramNodeV5 {
            id: id("custom:same-id"),
            object_kind: ProgramObjectKindV5::Artifact,
            body_hash: sha(1),
            data: NodeDataV5::Artifact {
                kind: "custom".to_owned(),
                label: "source".to_owned(),
                language: None,
                path: None,
                location: None,
                content_hash: None,
                anchor: None,
                base_hash: sha(2),
                provenance: NormalizedProvenanceV5 {
                    stable_hash: sha(20),
                    revision: NormalizedRevisionRefV5::Absent,
                    content: NormalizedContentRefV5::Absent,
                    local: NormalizedLocalRefV5::Absent,
                },
                change_fact_ids: BTreeSet::new(),
                symbol_ref: None,
            },
        };
        let target = ProgramNodeV5 {
            id: source.id.clone(),
            object_kind: ProgramObjectKindV5::Artifact,
            body_hash: sha(3),
            data: NodeDataV5::Artifact {
                kind: "custom".to_owned(),
                label: "target".to_owned(),
                language: None,
                path: None,
                location: None,
                content_hash: None,
                anchor: None,
                base_hash: sha(4),
                provenance: NormalizedProvenanceV5 {
                    stable_hash: sha(20),
                    revision: NormalizedRevisionRefV5::Absent,
                    content: NormalizedContentRefV5::Absent,
                    local: NormalizedLocalRefV5::Absent,
                },
                change_fact_ids: BTreeSet::new(),
                symbol_ref: None,
            },
        };
        let mut consumed_source = BTreeSet::new();
        let mut consumed_target = BTreeSet::new();
        let mut seeds = Vec::new();
        finish_unmatched(
            &[&source],
            &[&target],
            &mut consumed_source,
            &mut consumed_target,
            ProgramObjectKindV5::Artifact,
            1,
            &mut seeds,
        );
        assert_eq!(seeds.len(), 2);
        assert!(seeds.iter().any(|seed| {
            seed.from_ids == BTreeSet::from([source.id.clone()]) && seed.to_ids.is_empty()
        }));
        assert!(seeds.iter().any(|seed| {
            seed.from_ids.is_empty() && seed.to_ids == BTreeSet::from([target.id.clone()])
        }));
    }

    #[test]
    fn snapshot_scoped_git_provenance_normalizes_but_extractor_identity_does_not() {
        let (source_space, target_space) = spaces();
        let path = "src/checkout_controller.rs";
        let source_blob = sha(700);
        let target_blob = sha(701);
        let source_paths = BTreeMap::from([(
            path.to_owned(),
            SnapshotPathArtifactV5 {
                id: id("file:source"),
                content_hash: Some(source_blob.clone()),
            },
        )]);
        let target_paths = BTreeMap::from([(
            path.to_owned(),
            SnapshotPathArtifactV5 {
                id: id("file:target"),
                content_hash: Some(target_blob.clone()),
            },
        )]);
        let provenance = |space: &ProgramSpace, blob: ContentHash, extractor: &str| {
            crate::Provenance::accepted_deterministic(
                crate::SourceRef::new(
                    "git",
                    space.repository_identity(),
                    Some(space.target_revision().to_owned()),
                    Some(blob),
                    Some(path.to_owned()),
                )
                .unwrap(),
                extractor,
                Some("1.0.0".to_owned()),
                None,
            )
            .unwrap()
        };
        let source_provenance = normalize_provenance(
            &source_space,
            &provenance(&source_space, source_blob.clone(), "rust-syn@1"),
            Some(path),
            Some(&source_blob),
            &source_paths,
        )
        .unwrap();
        let target_provenance = normalize_provenance(
            &target_space,
            &provenance(&target_space, target_blob.clone(), "rust-syn@1"),
            Some(path),
            Some(&target_blob),
            &target_paths,
        )
        .unwrap();
        let successors = BTreeMap::from([(id("file:source"), BTreeSet::from([id("file:target")]))]);
        assert!(normalized_provenance_equal(
            &source_provenance,
            &target_provenance,
            &successors
        ));

        let mutated = normalize_provenance(
            &target_space,
            &provenance(&target_space, target_blob.clone(), "rust-syn@2"),
            Some(path),
            Some(&target_blob),
            &target_paths,
        )
        .unwrap();
        assert!(!normalized_provenance_equal(
            &source_provenance,
            &mutated,
            &successors
        ));

        let external = crate::Provenance::accepted_deterministic(
            crate::SourceRef::new(
                "external",
                "https://evidence.invalid/source",
                Some(source_space.target_revision().to_owned()),
                Some(source_space.incremental_tree_hash_for_store().clone()),
                Some(path.to_owned()),
            )
            .unwrap(),
            "external-parser@1",
            None,
            None,
        )
        .unwrap();
        let external = normalize_provenance(
            &source_space,
            &external,
            Some(path),
            Some(&source_blob),
            &source_paths,
        )
        .unwrap();
        assert!(matches!(
            external.revision,
            NormalizedRevisionRefV5::Exact(_)
        ));
        assert!(matches!(external.content, NormalizedContentRefV5::Exact(_)));
        assert!(matches!(external.local, NormalizedLocalRefV5::Exact(_)));
    }

    #[test]
    fn rust_equal_key_ambiguity_is_split_without_canonical_first_choice() {
        let (source, _) = spaces();
        let mut target_value: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        target_value["snapshot"]["base_revision"] = Value::String(oid(10));
        target_value["snapshot"]["target_revision"] = Value::String(oid(12));
        target_value["snapshot"]["tree_hash"] = Value::String(tree(13).to_string());
        replace_string(
            &mut target_value,
            "snapshot:double-submit-v1",
            "snapshot:double-submit-v2",
        );
        let artifacts = target_value["artifacts"].as_array_mut().unwrap();
        let mut duplicate = artifacts
            .iter()
            .find(|value| value["id"] == "function:checkout-submit")
            .unwrap()
            .clone();
        duplicate["id"] = Value::String("function:checkout-submit-duplicate".to_owned());
        artifacts.push(duplicate);
        let target =
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap();
        let closure = closure(&source, &target);
        let mut target_anchors = anchors(&target);
        let original = target_anchors[&id("function:checkout-submit")].clone();
        target_anchors.insert(id("function:checkout-submit-duplicate"), original);
        let proof = ValidatedIncrementalInputsV5::fixture_from_parts(
            &source,
            &target,
            anchors(&source),
            target_anchors,
            orders(&source),
            orders(&target),
            vec![],
        )
        .unwrap();
        let phase =
            ChangeMorphismV5::build_with_inputs(&closure, &source, &target, &proof).unwrap();
        let split = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.status() == MappingStatusV5::Split)
            .unwrap();
        assert_eq!(split.from_ids().len(), 1);
        assert_eq!(split.to_ids().len(), 2);
        assert_eq!(split.successor_ids(), split.to_ids());
    }

    #[test]
    fn relation_sidecar_order_is_semantic_and_never_set_order() {
        let (source, target) = spaces();
        let closure = closure(&source, &target);
        let source_orders = orders(&source);
        let mut target_orders = orders(&target);
        let relation_id = target
            .relations()
            .iter()
            .find(|relation| relation.target_ids.len() > 1)
            .unwrap()
            .id
            .clone();
        target_orders.get_mut(&relation_id).unwrap().reverse();
        let proof = ValidatedIncrementalInputsV5::fixture_from_parts(
            &source,
            &target,
            anchors(&source),
            anchors(&target),
            source_orders,
            target_orders,
            vec![],
        )
        .unwrap();
        let phase =
            ChangeMorphismV5::build_with_inputs(&closure, &source, &target, &proof).unwrap();
        assert!(phase.mappings().iter().any(|mapping| mapping.object_kind()
            == ProgramObjectKindV5::Relation
            && mapping.status() == MappingStatusV5::Removed));
        assert!(phase.mappings().iter().any(|mapping| mapping.object_kind()
            == ProgramObjectKindV5::Relation
            && mapping.status() == MappingStatusV5::Added));
    }

    #[test]
    fn relation_dependency_waves_cross_legacy_32_and_255_without_stage_collision() {
        for length in [25_usize, 256] {
            let (source, target) = spaces_with_relation_chain(length);
            let closure = closure(&source, &target);
            let phase = ChangeMorphismV5::build_with_inputs(
                &closure,
                &source,
                &target,
                &inputs(&source, &target),
            )
            .unwrap();
            let last_id = id(&format!("relation:chain-{:04}", length - 1));
            let previous_id = id(&format!("relation:chain-{:04}", length - 2));
            let last = phase
                .mappings()
                .iter()
                .find(|mapping| mapping.from_ids().contains(&last_id))
                .unwrap();
            let previous_mapping_id = phase
                .mappings()
                .iter()
                .find(|mapping| mapping.from_ids().contains(&previous_id))
                .unwrap()
                .id()
                .clone();
            let file_mapping_id = phase
                .mappings()
                .iter()
                .find(|mapping| mapping.from_ids().contains(&id("file:checkout-controller")))
                .unwrap()
                .id()
                .clone();
            assert_eq!(last.status(), MappingStatusV5::Preserved);
            assert_eq!(
                last.predecessor_mapping_ids(),
                &BTreeSet::from([file_mapping_id, previous_mapping_id])
            );
        }
    }

    #[test]
    fn mutually_referential_relation_scc_is_one_unresolved_component() {
        let (source, target) = spaces_with_mutual_relation_scc();
        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        let cycle_ids = BTreeSet::from([id("relation:cycle-a"), id("relation:cycle-b")]);
        let cycle = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids() == &cycle_ids)
            .expect("the cyclic fixed point must not split into removed/added records");
        assert_eq!(cycle.to_ids(), &cycle_ids);
        assert_eq!(cycle.status(), MappingStatusV5::Unresolved);
        let file_mapping_id = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids().contains(&id("file:checkout-controller")))
            .unwrap()
            .id()
            .clone();
        assert_eq!(
            cycle.predecessor_mapping_ids(),
            &BTreeSet::from([file_mapping_id])
        );
    }

    #[test]
    fn asymmetric_relation_scc_is_atomic_with_all_external_predecessors() {
        let (source, target) = spaces_with_mutual_relation_scc();
        let make_asymmetric = |space: ProgramSpace| {
            let mut value: Value =
                serde_json::from_slice(&crate::canonical_json(&space.streaming_ref()).unwrap())
                    .unwrap();
            value["relations"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|relation| relation["id"] == "relation:cycle-b")
                .unwrap()["source_id"] = Value::String("file:payment-repository".to_owned());
            ProgramSpace::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap()
        };
        let source = make_asymmetric(source);
        let target = make_asymmetric(target);
        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        let cycle_ids = BTreeSet::from([id("relation:cycle-a"), id("relation:cycle-b")]);
        let cycle = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids() == &cycle_ids)
            .expect("Tarjan SCC membership must be the atomic mapping unit");
        assert_eq!(cycle.to_ids(), &cycle_ids);
        assert_eq!(cycle.status(), MappingStatusV5::Unresolved);
        let external_predecessors = ["file:checkout-controller", "file:payment-repository"]
            .into_iter()
            .map(|file_id| {
                phase
                    .mappings()
                    .iter()
                    .find(|mapping| mapping.from_ids().contains(&id(file_id)))
                    .unwrap()
                    .id()
                    .clone()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(cycle.predecessor_mapping_ids(), &external_predecessors);
    }

    #[test]
    fn disjoint_same_kind_sccs_with_distinct_external_topology_do_not_merge() {
        let (source, target) = spaces_with_two_disjoint_relation_sccs();
        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        let first = BTreeSet::from([id("relation:cycle-a"), id("relation:cycle-b")]);
        let second = BTreeSet::from([id("relation:cycle-c"), id("relation:cycle-d")]);
        assert!(phase.mappings().iter().any(|mapping| {
            mapping.from_ids() == &first
                && mapping.to_ids() == &first
                && mapping.status() == MappingStatusV5::Unresolved
        }));
        assert!(phase.mappings().iter().any(|mapping| {
            mapping.from_ids() == &second
                && mapping.to_ids() == &second
                && mapping.status() == MappingStatusV5::Unresolved
        }));
        assert!(!phase.mappings().iter().any(|mapping| {
            mapping.from_ids().is_superset(&first) && mapping.from_ids().is_superset(&second)
        }));
    }

    #[test]
    fn scc_condensation_stages_dependencies_before_dependents() {
        let (source, target) = spaces_with_two_disjoint_relation_sccs();
        let add_condensation_edge = |space: ProgramSpace| {
            let mut value: Value =
                serde_json::from_slice(&crate::canonical_json(&space.streaming_ref()).unwrap())
                    .unwrap();
            let relation = value["relations"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|relation| relation["id"] == "relation:cycle-c")
                .unwrap();
            relation["target_ids"] = serde_json::json!(["relation:cycle-d", "relation:cycle-a"]);
            ProgramSpace::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap()
        };
        let source = add_condensation_edge(source);
        let target = add_condensation_edge(target);
        let source_nodes = program_nodes(
            &source,
            ProgramSideV5::Source,
            &anchors(&source),
            &orders(&source),
            &[],
        )
        .unwrap()
        .0;
        let relation_nodes = source_nodes
            .iter()
            .filter(|node| node.object_kind == ProgramObjectKindV5::Relation)
            .collect::<Vec<_>>();
        let cycle_ids = BTreeSet::from([
            id("relation:cycle-a"),
            id("relation:cycle-b"),
            id("relation:cycle-c"),
            id("relation:cycle-d"),
        ]);
        let components = relation_scc_components(&relation_nodes, &cycle_ids);
        let depths = relation_scc_depths(&relation_nodes, &components).unwrap();
        let depth_by_id = components
            .iter()
            .zip(depths)
            .flat_map(|(component, depth)| component.iter().cloned().map(move |id| (id, depth)))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(depth_by_id[&id("relation:cycle-a")], 0);
        assert_eq!(depth_by_id[&id("relation:cycle-c")], 1);

        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        let dependency_ids = BTreeSet::from([id("relation:cycle-a"), id("relation:cycle-b")]);
        let dependent_ids = BTreeSet::from([id("relation:cycle-c"), id("relation:cycle-d")]);
        let dependency = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids() == &dependency_ids)
            .unwrap();
        let dependent = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids() == &dependent_ids)
            .unwrap();
        assert_eq!(dependency.status(), MappingStatusV5::Unresolved);
        assert_eq!(dependent.status(), MappingStatusV5::Unresolved);
        assert!(
            dependent
                .predecessor_mapping_ids()
                .contains(dependency.id()),
            "dependent SCC must retain the dependency SCC mapping as source trace"
        );
    }

    #[test]
    fn ambiguity_grouping_recomputes_condensation_depth_before_source_trace() {
        let (source, target) = spaces();
        let add_relations = |space: ProgramSpace| {
            let mut value: Value =
                serde_json::from_slice(&crate::canonical_json(&space.streaming_ref()).unwrap())
                    .unwrap();
            let template = value["relations"][0].clone();
            for (relation_id, kind, target_ids) in [
                (
                    "relation:group-d1",
                    "ambiguity_group",
                    serde_json::json!(["relation:group-d1"]),
                ),
                (
                    "relation:group-e",
                    "ambiguity_group",
                    serde_json::json!(["relation:group-e"]),
                ),
                (
                    "relation:group-d2",
                    "ambiguity_group",
                    serde_json::json!(["relation:group-e"]),
                ),
                (
                    "relation:group-a",
                    "ambiguity_dependent",
                    serde_json::json!(["relation:group-a", "relation:group-d1"]),
                ),
            ] {
                let mut relation = template.clone();
                relation["id"] = Value::String(relation_id.to_owned());
                relation["kind"] = Value::String(kind.to_owned());
                relation["source_id"] = Value::String("file:checkout-controller".to_owned());
                relation["target_ids"] = target_ids;
                relation["directed"] = Value::Bool(true);
                relation["attributes"] = serde_json::json!({});
                value["relations"].as_array_mut().unwrap().push(relation);
            }
            ProgramSpace::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap()
        };
        let source = add_relations(source);
        let target = add_relations(target);
        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();

        let dependency_ids = BTreeSet::from([
            id("relation:group-d1"),
            id("relation:group-d2"),
            id("relation:group-e"),
        ]);
        let dependent_ids = BTreeSet::from([id("relation:group-a")]);
        let dependency = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids() == &dependency_ids)
            .expect("equal-signature raw SCCs must remain one ambiguity group");
        let dependent = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids() == &dependent_ids)
            .expect("dependent relation must retain its own mapping component");

        assert_eq!(dependency.to_ids(), &dependency_ids);
        assert_eq!(dependency.status(), MappingStatusV5::Unresolved);
        assert!(
            dependent
                .predecessor_mapping_ids()
                .contains(dependency.id()),
            "the final grouped condensation DAG must stage A strictly after its D1 dependency and retain that mapping as source trace"
        );
    }

    #[test]
    fn grouping_induced_seed_cycle_collapses_to_one_atomic_unresolved_component() {
        let (source, target) = spaces();
        let add_relations = |space: ProgramSpace| {
            let mut value: Value =
                serde_json::from_slice(&crate::canonical_json(&space.streaming_ref()).unwrap())
                    .unwrap();
            let template = value["relations"][0].clone();
            for (relation_id, target_id) in [
                ("relation:quotient-x1", "relation:quotient-y1"),
                ("relation:quotient-x2", "file:checkout-controller"),
                ("relation:quotient-y1", "file:checkout-controller"),
                ("relation:quotient-y2", "relation:quotient-x2"),
            ] {
                let mut relation = template.clone();
                relation["id"] = Value::String(relation_id.to_owned());
                relation["kind"] = Value::String("quotient_cycle_fixture".to_owned());
                relation["source_id"] = Value::String("file:checkout-controller".to_owned());
                relation["target_ids"] = serde_json::json!([target_id]);
                relation["directed"] = Value::Bool(true);
                relation["attributes"] = serde_json::json!({});
                value["relations"].as_array_mut().unwrap().push(relation);
            }
            ProgramSpace::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap()
        };
        let source = add_relations(source);
        let target = add_relations(target);
        let source_program_nodes = program_nodes(
            &source,
            ProgramSideV5::Source,
            &anchors(&source),
            &orders(&source),
            &[],
        )
        .unwrap()
        .0;
        let target_program_nodes = program_nodes(
            &target,
            ProgramSideV5::Target,
            &anchors(&target),
            &orders(&target),
            &[],
        )
        .unwrap()
        .0;
        let source_relations = source_program_nodes
            .iter()
            .filter(|node| node.object_kind == ProgramObjectKindV5::Relation)
            .collect::<Vec<_>>();
        let target_relations = target_program_nodes
            .iter()
            .filter(|node| node.object_kind == ProgramObjectKindV5::Relation)
            .collect::<Vec<_>>();
        let x_ids = BTreeSet::from([id("relation:quotient-x1"), id("relation:quotient-x2")]);
        let y_ids = BTreeSet::from([id("relation:quotient-y1"), id("relation:quotient-y2")]);
        let make_seed = |ids: BTreeSet<StableId>| ComponentSeedV5 {
            object_kind: ProgramObjectKindV5::Relation,
            candidate_key_kind: CandidateKeyKindV5::MappedDirectedEndpoints,
            from_ids: ids.clone(),
            to_ids: ids,
            stage: 0,
        };

        let collapsed = collapse_and_stage_grouped_relation_seeds(
            vec![make_seed(x_ids.clone()), make_seed(y_ids.clone())],
            &source_relations,
            &target_relations,
            17,
        )
        .unwrap();

        assert_eq!(collapsed.len(), 1);
        assert_eq!(
            collapsed[0].from_ids,
            x_ids.union(&y_ids).cloned().collect()
        );
        assert_eq!(collapsed[0].to_ids, collapsed[0].from_ids);
        assert_eq!(collapsed[0].stage, 17);
    }

    #[test]
    fn mismatched_relation_scc_topology_is_not_cross_connected() {
        let (source, target) = spaces_with_mutual_relation_scc();
        let mut target_value: Value =
            serde_json::from_slice(&crate::canonical_json(&target.streaming_ref()).unwrap())
                .unwrap();
        let relations = target_value["relations"].as_array_mut().unwrap();
        let template = relations
            .iter()
            .find(|relation| relation["id"] == "relation:cycle-a")
            .unwrap()
            .clone();
        relations
            .iter_mut()
            .find(|relation| relation["id"] == "relation:cycle-b")
            .unwrap()["target_ids"] = serde_json::json!(["relation:cycle-c"]);
        let mut third = template;
        third["id"] = Value::String("relation:cycle-c".to_owned());
        third["target_ids"] = serde_json::json!(["relation:cycle-a"]);
        relations.push(third);
        let target =
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap();
        let closure = closure(&source, &target);
        let phase = ChangeMorphismV5::build_with_inputs(
            &closure,
            &source,
            &target,
            &inputs(&source, &target),
        )
        .unwrap();
        for relation_id in [id("relation:cycle-a"), id("relation:cycle-b")] {
            assert!(phase.mappings().iter().any(|mapping| {
                mapping.from_ids() == &BTreeSet::from([relation_id.clone()])
                    && mapping.to_ids().is_empty()
                    && mapping.status() == MappingStatusV5::Removed
            }));
        }
        assert!(!phase.mappings().iter().any(|mapping| {
            !mapping.from_ids().is_empty()
                && !mapping.to_ids().is_empty()
                && (mapping.from_ids().contains(&id("relation:cycle-a"))
                    || mapping.from_ids().contains(&id("relation:cycle-b")))
        }));
    }

    #[test]
    fn cyclic_relation_endpoint_order_is_part_of_topology_signature() {
        let (source, target) = spaces_with_mutual_relation_scc();
        let add_external_endpoint = |space: ProgramSpace| {
            let mut value: Value =
                serde_json::from_slice(&crate::canonical_json(&space.streaming_ref()).unwrap())
                    .unwrap();
            for relation in value["relations"].as_array_mut().unwrap().iter_mut() {
                if matches!(
                    relation["id"].as_str(),
                    Some("relation:cycle-a" | "relation:cycle-b")
                ) {
                    relation["target_ids"] = serde_json::json!([
                        relation["target_ids"][0].clone(),
                        "file:payment-repository"
                    ]);
                }
            }
            ProgramSpace::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap()
        };
        let source = add_external_endpoint(source);
        let target = add_external_endpoint(target);
        let closure = closure(&source, &target);
        let source_orders = orders(&source);
        let mut target_orders = orders(&target);
        for relation_id in [id("relation:cycle-a"), id("relation:cycle-b")] {
            target_orders.get_mut(&relation_id).unwrap().reverse();
        }
        let proof = ValidatedIncrementalInputsV5::fixture_from_parts(
            &source,
            &target,
            anchors(&source),
            anchors(&target),
            source_orders,
            target_orders,
            vec![],
        )
        .unwrap();
        let phase =
            ChangeMorphismV5::build_with_inputs(&closure, &source, &target, &proof).unwrap();
        assert!(!phase.mappings().iter().any(|mapping| {
            !mapping.from_ids().is_empty()
                && !mapping.to_ids().is_empty()
                && mapping.from_ids().contains(&id("relation:cycle-a"))
        }));
    }

    #[test]
    fn copied_git_fact_is_traced_but_never_becomes_a_rename_edge() {
        let (source, _) = spaces();
        let mut target_value: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        target_value["snapshot"]["base_revision"] = Value::String(oid(10));
        target_value["snapshot"]["target_revision"] = Value::String(oid(12));
        target_value["snapshot"]["tree_hash"] = Value::String(tree(13).to_string());
        replace_string(
            &mut target_value,
            "snapshot:double-submit-v1",
            "snapshot:double-submit-v2",
        );
        let artifacts = target_value["artifacts"].as_array_mut().unwrap();
        let original = artifacts
            .iter_mut()
            .find(|value| value["id"] == "file:checkout-controller")
            .unwrap();
        original["content_hash"] = Value::String(sha(44).to_string());
        let mut copied = original.clone();
        copied["id"] = Value::String("file:checkout-controller-copy".to_owned());
        copied["label"] = Value::String("src/checkout_controller_copy.rs".to_owned());
        copied["location"]["path"] = Value::String("src/checkout_controller_copy.rs".to_owned());
        artifacts.push(copied);
        let target =
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap();
        let copy_fact = GitChangeFactV5::fixture_from_parts(
            id("git-change-fact-v5:copy"),
            GitChangeKindV5::Copied,
            id("file:checkout-controller"),
            id("file:checkout-controller-copy"),
            "src/checkout_controller.rs".to_owned(),
            "src/checkout_controller_copy.rs".to_owned(),
            sha(44),
            GIT_CHANGE_PROVENANCE_V1,
        )
        .unwrap();
        let _rename_shape = GitChangeFactV5::fixture_from_parts(
            id("git-change-fact-v5:rename-shape"),
            GitChangeKindV5::Renamed,
            id("file:checkout-controller"),
            id("file:checkout-controller-copy"),
            "src/checkout_controller.rs".to_owned(),
            "src/checkout_controller_copy.rs".to_owned(),
            sha(44),
            GIT_CHANGE_PROVENANCE_V1,
        )
        .unwrap();
        let proof = ValidatedIncrementalInputsV5::fixture_from_parts(
            &source,
            &target,
            anchors(&source),
            anchors(&target),
            orders(&source),
            orders(&target),
            vec![copy_fact],
        )
        .unwrap();
        let closure = closure(&source, &target);
        let phase =
            ChangeMorphismV5::build_with_inputs(&closure, &source, &target, &proof).unwrap();
        let added = phase
            .mappings()
            .iter()
            .find(|mapping| {
                mapping
                    .to_ids()
                    .contains(&id("file:checkout-controller-copy"))
            })
            .unwrap();
        assert_eq!(added.status(), MappingStatusV5::Added);
        assert!(added.successor_ids().is_empty());
        assert!(added.source_ids().contains(&id("git-change-fact-v5:copy")));
    }

    #[test]
    fn exact_git_rename_fact_forms_the_only_rename_edge() {
        let (source, _) = spaces();
        let mut target_value: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        target_value["snapshot"]["base_revision"] = Value::String(oid(10));
        target_value["snapshot"]["target_revision"] = Value::String(oid(12));
        target_value["snapshot"]["tree_hash"] = Value::String(tree(13).to_string());
        replace_string(
            &mut target_value,
            "snapshot:double-submit-v1",
            "snapshot:double-submit-v2",
        );
        let artifacts = target_value["artifacts"].as_array_mut().unwrap();
        let artifact = artifacts
            .iter_mut()
            .find(|value| value["id"] == "file:checkout-controller")
            .unwrap();
        artifact["content_hash"] = Value::String(sha(44).to_string());
        artifact["label"] = Value::String("src/renamed_checkout_controller.rs".to_owned());
        artifact["location"]["path"] =
            Value::String("src/renamed_checkout_controller.rs".to_owned());
        let mut provenance = artifact["provenance"].clone();
        provenance["extraction_method"] = Value::String(GIT_CHANGE_PROVENANCE_V1.to_owned());
        artifacts.push(serde_json::json!({
            "attributes": {
                "base_path": "src/checkout_controller.rs",
                "change_kind": "renamed",
                "target_path": "src/renamed_checkout_controller.rs"
            },
            "id": "change:accepted-rename",
            "kind": "custom",
            "label": "renamed src/renamed_checkout_controller.rs",
            "provenance": provenance
        }));
        let target =
            ProgramSpace::from_json_slice(&serde_json::to_vec(&target_value).unwrap()).unwrap();
        let rename = target
            .artifacts()
            .iter()
            .find(|artifact| artifact.id == id("change:accepted-rename"))
            .and_then(|artifact| {
                GitChangeFactV5::from_accepted_artifact(artifact, &source, &target).unwrap()
            })
            .unwrap();
        let proof = ValidatedIncrementalInputsV5::fixture_from_parts(
            &source,
            &target,
            anchors(&source),
            anchors(&target),
            orders(&source),
            orders(&target),
            vec![rename],
        )
        .unwrap();
        let closure = closure(&source, &target);
        let phase =
            ChangeMorphismV5::build_with_inputs(&closure, &source, &target, &proof).unwrap();
        let renamed = phase
            .mappings()
            .iter()
            .find(|mapping| mapping.from_ids().contains(&id("file:checkout-controller")))
            .unwrap();
        assert_eq!(
            renamed.candidate_key_kind(),
            CandidateKeyKindV5::GitRenameSameContent
        );
        assert_eq!(renamed.status(), MappingStatusV5::Preserved);
        assert_eq!(
            renamed.change_fact_ids(),
            &BTreeSet::from([id("change:accepted-rename")])
        );
    }

    #[test]
    fn strict_wire_decode_rejects_unknown_order_duplicate_and_identity_tamper() {
        let (closure, phase) = phase();
        let closure_bytes = crate::canonical_json(&closure).unwrap();
        assert_eq!(
            IncrementalSourceClosureV5::from_json_bytes(&closure_bytes, &closure).unwrap(),
            closure
        );
        let mapping = phase.mappings().first().unwrap();
        let bytes = crate::canonical_json(mapping).unwrap();
        assert_eq!(ProgramMappingV5::from_json_bytes(&bytes).unwrap(), *mapping);
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unknown".to_owned(), Value::Bool(true));
        assert!(
            ProgramMappingV5::from_json_bytes(&crate::canonical_json(&value).unwrap()).is_err()
        );
        let morphism_bytes = crate::canonical_json(phase.morphism()).unwrap();
        let mapping_bytes = phase
            .mappings()
            .iter()
            .map(|mapping| crate::canonical_json(mapping).unwrap())
            .collect::<Vec<_>>();
        phase
            .validate_replayed_canonical(&mapping_bytes, &morphism_bytes)
            .unwrap();
        let mut reordered = mapping_bytes.clone();
        reordered.swap(0, 1);
        assert!(
            phase
                .validate_replayed_canonical(&reordered, &morphism_bytes)
                .is_err()
        );
        assert_eq!(
            ChangeMorphismV5::from_json_bytes(&morphism_bytes, phase.morphism()).unwrap(),
            *phase.morphism()
        );
        let mut value: Value = serde_json::from_slice(&morphism_bytes).unwrap();
        value["mapping_count"] = Value::from(0);
        assert!(
            ChangeMorphismV5::from_json_bytes(
                &crate::canonical_json(&value).unwrap(),
                phase.morphism()
            )
            .is_err()
        );

        let split_hashes = vec![
            IdBodyHashV5::new(id("artifact:b"), sha(2)).unwrap(),
            IdBodyHashV5::new(id("artifact:a"), sha(1)).unwrap(),
        ];
        assert!(
            validate_body_hash_records(
                "test",
                &BTreeSet::from([id("artifact:a"), id("artifact:b")]),
                &split_hashes,
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<IdBodyHashV5>(serde_json::json!({
                "body_hash": tree(1),
                "id": "artifact:a"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<IdBodyHashV5>(serde_json::json!({
                "body_hash": sha(1),
                "id": "artifact:a",
                "unknown": true
            }))
            .is_err()
        );
    }

    #[test]
    fn allocation_and_event_line_bounds_are_exact_and_overflow_closed() {
        #[derive(Serialize)]
        struct ExactDto {
            a: String,
        }
        for (limit, operation) in [
            (MAX_M6_CLOSURE_DTO_BYTES, "closure"),
            (MAX_M6_MAPPING_DTO_BYTES, "mapping"),
            (MAX_M6_MORPHISM_DTO_BYTES, "morphism"),
        ] {
            bounded(limit, limit, operation).unwrap();
            assert_eq!(
                bounded(limit + 1, limit, operation).unwrap_err(),
                M6Error::Incomplete {
                    operation,
                    limit,
                    observed: limit + 1,
                }
            );
        }
        assert_eq!(
            preflight_event_line(MAX_M6_EVENT_LINE_BYTES - 1, 1).unwrap(),
            MAX_M6_EVENT_LINE_BYTES
        );
        assert_eq!(
            preflight_event_line(MAX_M6_EVENT_LINE_BYTES, 1).unwrap_err(),
            M6Error::Incomplete {
                operation: "M6 event-line bytes",
                limit: MAX_M6_EVENT_LINE_BYTES,
                observed: MAX_M6_EVENT_LINE_BYTES + 1,
            }
        );
        assert_eq!(
            preflight_event_line(usize::MAX, 1).unwrap_err(),
            M6Error::Incomplete {
                operation: "M6 event-line bytes",
                limit: MAX_M6_EVENT_LINE_BYTES,
                observed: usize::MAX,
            }
        );
        assert_eq!(
            checked_working_add(MAX_M6_MAPPING_WORKING_BYTES, 0).unwrap(),
            MAX_M6_MAPPING_WORKING_BYTES
        );
        assert!(checked_working_add(MAX_M6_MAPPING_WORKING_BYTES, 1).is_err());
        assert!(checked_working_add(usize::MAX, 1).is_err());
        let exact = ExactDto {
            a: "x".repeat(MAX_M6_EVENT_LINE_BYTES - 9),
        };
        assert_eq!(
            crate::canonical_json(&exact).unwrap().len() + 1,
            MAX_M6_EVENT_LINE_BYTES
        );
        bounded_event_dto(&exact, MAX_M6_MAPPING_DTO_BYTES, "exact DTO plus LF").unwrap();
        let plus_one = ExactDto {
            a: "x".repeat(MAX_M6_EVENT_LINE_BYTES - 8),
        };
        assert!(
            bounded_event_dto(&plus_one, MAX_M6_MAPPING_DTO_BYTES, "exact DTO plus LF").is_err()
        );
    }

    #[test]
    fn staleness_combined_budget_is_inclusive_and_overflow_closed() {
        assert_eq!(staleness_source_working_limit(19, 7, 12).unwrap(), 7);
        assert!(matches!(
            staleness_source_working_limit(18, 7, 12),
            Err(M6Error::Incomplete {
                operation: "M6 staleness combined working bytes",
                limit: 18,
                observed: 19,
            })
        ));
        assert!(matches!(
            staleness_source_working_limit(usize::MAX, usize::MAX, 1),
            Err(M6Error::Incomplete {
                operation: "M6 staleness combined working bytes",
                observed: usize::MAX,
                ..
            })
        ));
        assert!(matches!(
            staleness_source_working_limit(0, 0, 1),
            Err(M6Error::Incomplete {
                operation: "M6 staleness combined working bytes",
                observed: usize::MAX,
                ..
            })
        ));
    }

    #[test]
    fn staleness_phase_retention_uses_actual_sealed_mapping_and_correspondence() {
        let (_closure, mapping, _source, _target, correspondence) = correspondence_phase();
        let (mapping_bytes, correspondence_bytes) =
            staleness_mapping_correspondence_retained_bytes(&mapping, &correspondence).unwrap();
        let expected_mapping = mapping
            .mappings()
            .iter()
            .fold(mapping.morphism().allocated_bytes(), |total, item| {
                total.checked_add(item.allocated_bytes()).unwrap()
            })
            + mapping.mappings.capacity() * std::mem::size_of::<ProgramMappingV5>();
        let expected_correspondence = correspondence.entries().iter().fold(
            correspondence.correspondence().allocated_bytes(),
            |total, item| total.checked_add(item.allocated_bytes()).unwrap(),
        ) + correspondence.entries.capacity()
            * std::mem::size_of::<ObligationCorrespondenceEntryV5>();
        assert_eq!(mapping_bytes, expected_mapping);
        assert_eq!(correspondence_bytes, expected_correspondence);
        assert!(mapping_bytes > 0 && correspondence_bytes > 0);
    }

    #[test]
    fn seal_accepts_exact_8192_mappings_and_4096_domains_with_streamed_digest() {
        let (source, target) = spaces();
        let closure = closure(&source, &target);
        let source_domain = (0..MAX_M6_PROGRAM_DOMAIN_IDS)
            .map(|index| id(&format!("custom:source-{index:04}")))
            .collect::<BTreeSet<_>>();
        let target_domain = (0..MAX_M6_PROGRAM_DOMAIN_IDS)
            .map(|index| id(&format!("custom:target-{index:04}")))
            .collect::<BTreeSet<_>>();
        let mut mappings = Vec::with_capacity(MAX_M6_MAPPINGS);
        for source_id in &source_domain {
            mappings.push(
                ProgramMappingV5::from_parts(ProgramMappingPartsV5 {
                    source_closure_id: closure.id().clone(),
                    source_snapshot_id: source.snapshot_id().clone(),
                    target_snapshot_id: target.snapshot_id().clone(),
                    object_kind: ProgramObjectKindV5::Artifact,
                    from_ids: BTreeSet::from([source_id.clone()]),
                    to_ids: BTreeSet::new(),
                    status: MappingStatusV5::Removed,
                    candidate_key_kind: CandidateKeyKindV5::NoCandidate,
                    source_body_hashes: vec![IdBodyHashV5::new(source_id.clone(), sha(1)).unwrap()],
                    target_body_hashes: Vec::new(),
                    change_fact_ids: BTreeSet::new(),
                    predecessor_mapping_ids: BTreeSet::new(),
                })
                .unwrap(),
            );
        }
        for target_id in &target_domain {
            mappings.push(
                ProgramMappingV5::from_parts(ProgramMappingPartsV5 {
                    source_closure_id: closure.id().clone(),
                    source_snapshot_id: source.snapshot_id().clone(),
                    target_snapshot_id: target.snapshot_id().clone(),
                    object_kind: ProgramObjectKindV5::Artifact,
                    from_ids: BTreeSet::new(),
                    to_ids: BTreeSet::from([target_id.clone()]),
                    status: MappingStatusV5::Added,
                    candidate_key_kind: CandidateKeyKindV5::NoCandidate,
                    source_body_hashes: Vec::new(),
                    target_body_hashes: vec![IdBodyHashV5::new(target_id.clone(), sha(2)).unwrap()],
                    change_fact_ids: BTreeSet::new(),
                    predecessor_mapping_ids: BTreeSet::new(),
                })
                .unwrap(),
            );
        }
        mappings.sort_by(|left, right| left.id().cmp(right.id()));
        let morphism =
            ChangeMorphismV5::seal_derived(&closure, &mappings, &source_domain, &target_domain)
                .unwrap();
        assert_eq!(morphism.mapping_count(), MAX_M6_MAPPINGS as u64);
        assert_eq!(
            morphism.source_domain_count(),
            MAX_M6_PROGRAM_DOMAIN_IDS as u64
        );
        assert_eq!(
            morphism.target_domain_count(),
            MAX_M6_PROGRAM_DOMAIN_IDS as u64
        );
        assert!(matches!(
            bounded(MAX_M6_MAPPINGS + 1, MAX_M6_MAPPINGS, "M6 program mappings"),
            Err(M6Error::Incomplete { observed, .. }) if observed == MAX_M6_MAPPINGS + 1
        ));
    }

    #[test]
    fn hard_canonical_ids_are_independent_fixtures() {
        let (closure, phase) = phase();
        assert_eq!(
            closure.id().as_str(),
            "incremental-source-closure-v5:sha256:44cc90ad0a8a09c4ecb4a6d547ea7039495f6739bdfa11f4490ec0572148af2b"
        );
        assert_eq!(
            phase.morphism().id().as_str(),
            "change-morphism-v5:sha256:ffdd345adc7dcc87d448f0ae8d2e9b1bf770fd2364aefbb104f2f003359f5ef2"
        );
        assert_eq!(
            phase.mappings().first().unwrap().id().as_str(),
            "program-mapping-v5:sha256:04a21071bd5b70720917396e1ccd4024a62982e84d3ec1892c6ad671129ee459"
        );
    }
}
