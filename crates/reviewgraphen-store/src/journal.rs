//! Locked, durably appended JSONL event journals.
//!
//! The journal deliberately does not apply domain events.  It only admits a
//! complete, core-validated chain while holding the OS lock that protects the
//! bytes.  Projection and authority reconciliation remain core/index work.

use super::{
    CasHash, CasReader, CasReceipt, CasStore, StoreError, StoreRoot, StoreRootIdentity,
    open_or_create_dir, verify_fd_kind_mode,
};
use reviewgraphen_core::{
    ArtifactRegisteredV3, ArtifactRegistrationReceiptV4, ArtifactRegistrationV3AtV4Admission,
    ArtifactRegistrationV3AtV4Receipt, ArtifactSensitivity, AuthorityArtifactResolverV3,
    AuthorityArtifactResolverV4, AuthorityReplayBasisV3, AuthorityReplayBasisV4,
    AuthorityTrustRootsV3, AuthorityTrustRootsV4, BuiltContextProjection, ChangeMorphismV5,
    ContentHash, DecisionInputV3, EventAdmissions, EventCommand, EventContractVersion,
    EventEnvelope, EventLog, EventLogV4, EventLogV5, EventReplayLimits, EventStreamGenesis,
    ExpectedVerificationAttemptV3, ExternalWitnessAdmissionV3, FixtureExecutionReceiptV1,
    FixtureRegistrationResumeAuthorityV3, GLUING_INPUT_MEDIA_TYPE_V4, GluingBundleReceiptV4,
    GluingInputDescriptorV4, IncrementalSourceClosureV5, InheritedD2EventReceiptV4,
    M5CompletedGluingProfileV4, M5DoubleSubmitAssignmentsV4, M5GluingProfileInputV4, M6Error,
    M6MappingPhaseV5, MAX_M5_DESCRIPTOR_CANONICAL_BYTES, ObligationLifecycle,
    OpaqueSessionIdentityV4, PreparedInheritedD2EventV4,
    RecoveredM4BundleV4Session as CoreRecoveredM4BundleV4Session, ReviewPlan, RunGenesisSnapshot,
    SnapshotSourceBundle, SnapshotSourcesRecorded, StableId, StaticFactEvaluationV1,
    StaticVerificationAttemptInspectionV3, TrustedGluingInputAdmissionV4,
    TrustedGluingInputSourceV4, UntrustedIncrementalMappingProposalV5,
    ValidatedArtifactRegistrationV3, ValidatedDecisionV3, ValidatedExecutionBundle,
    ValidatedFindingV3, ValidatedGluingBundleV4, ValidatedVerificationBundleV3,
    ValidatedVerificationBundleV4, VerificationAttemptStageV3, VerificationBundleReceiptV3,
    VerificationBundleReceiptV4, VerificationBundleRecoveryV4, VerificationBundleRequestV4,
    VerificationBundleResumeAuthorityV3, VerificationBundleResumeAuthorityV4,
    VerifierArtifactRoleV3, canonical_json, derive_untrusted_incremental_mapping_proposal_v5,
};
use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, Dir, FileType, FlockOperation, Mode, OFlags},
    io::{Errno, dup},
    rand::{GetRandomFlags, getrandom},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::sync::Mutex;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::fd::AsFd,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

const RUNS_DIR: &str = "runs";
const JOURNAL_FILE: &str = "logs.jsonl";
const RECOVERY_DIR: &str = "recovery";
const INTENTS_DIR: &str = "intents";
const COMPLETIONS_DIR: &str = "completions";
/// A durable, create-only write-ahead marker.  It is deliberately a fixed
/// name: while it exists no reader or writer may trust the log, and only the
/// explicit recovery path may decide whether the attempted append reached the
/// file.  This closes the interval between a failed append and publication of
/// a recovery receipt.
const APPEND_PENDING_MARKER: &str = "append.pending";
const APPEND_PENDING_BYTES: &[u8] = b"reviewgraphen.append-pending.v1\n";
const BUNDLE_PENDING_MARKER: &str = "verification-bundle.pending.json";
const BUNDLE_PENDING_STAGE: &str = "verification-bundle.pending.stage";
const BUNDLE_MARKER_SCHEMA: &str = "reviewgraphen.verification_bundle_append.v1";
const RECOVERY_RECEIPT_SCHEMA_V4: &str = "reviewgraphen.recovery_receipt.v4";
const EVENT_CONTRACT_SCHEMA_V4: &str = "reviewgraphen.review_event.v4";
const MAX_INCREMENTAL_JOURNAL_PREFIX_BYTES: u64 = 67_108_864;
const MAX_INCREMENTAL_SESSION_WORKING_BYTES: u64 = 536_870_912;

fn admit_incremental_source_journal_bytes(observed: u64) -> Result<u64, JournalError> {
    if observed > MAX_INCREMENTAL_JOURNAL_PREFIX_BYTES {
        return Err(JournalError::Incomplete {
            limit: MAX_INCREMENTAL_JOURNAL_PREFIX_BYTES,
            observed,
        });
    }
    Ok(observed)
}

/// Immutable material which determines the event-chain genesis sentinel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JournalGenesis {
    /// Historical V1 stream identity.
    V1(ContentHash),
    /// Input form for exact canonical `RunGenesisSnapshot` bytes.  `JournalIdentity::new`
    /// consumes this vector and normalizes it to the shared representation
    /// below, so a durable identity never retains two full genesis buffers.
    V2(Vec<u8>),
    /// Shared, immutable exact V2 genesis backing. This variant is primarily
    /// useful when identities are cloned for concurrent readers.
    V2Shared(Arc<[u8]>),
    /// Input form for exact canonical V3 `RunGenesisSnapshot` bytes.
    V3(Vec<u8>),
    /// Shared, immutable exact V3 genesis backing retained by a verified
    /// journal identity.
    V3Shared(Arc<[u8]>),
    /// Input form for exact canonical V4 `RunGenesisSnapshot` bytes.
    V4(Vec<u8>),
    /// Shared, immutable exact V4 genesis backing retained by a verified
    /// journal identity.
    V4Shared(Arc<[u8]>),
    /// Input form for an exact canonical V5 target genesis snapshot.
    V5(Vec<u8>),
    /// Shared immutable V5 genesis backing retained by the journal identity.
    V5Shared(Arc<[u8]>),
}

impl JournalGenesis {
    fn version(&self) -> EventContractVersion {
        match self {
            Self::V1(_) => EventContractVersion::V1,
            Self::V2(_) | Self::V2Shared(_) => EventContractVersion::V2,
            Self::V3(_) | Self::V3Shared(_) => EventContractVersion::V3,
            Self::V4(_) | Self::V4Shared(_) => EventContractVersion::V4,
            Self::V5(_) | Self::V5Shared(_) => EventContractVersion::V5,
        }
    }
    fn hash(&self) -> ContentHash {
        match self {
            Self::V1(hash) => hash.clone(),
            Self::V2(bytes) => ContentHash::sha256(bytes),
            Self::V2Shared(bytes) => ContentHash::sha256(bytes),
            Self::V3(bytes) => ContentHash::sha256(bytes),
            Self::V3Shared(bytes) => ContentHash::sha256(bytes),
            Self::V4(bytes) => ContentHash::sha256(bytes),
            Self::V4Shared(bytes) => ContentHash::sha256(bytes),
            Self::V5(bytes) => ContentHash::sha256(bytes),
            Self::V5Shared(bytes) => ContentHash::sha256(bytes),
        }
    }
    fn core_genesis(&self) -> EventStreamGenesis<'_> {
        match self {
            Self::V1(hash) => EventStreamGenesis::V1(hash),
            Self::V2(bytes) => EventStreamGenesis::V2(bytes),
            Self::V2Shared(bytes) => EventStreamGenesis::V2(bytes),
            Self::V3(bytes) => EventStreamGenesis::V3(bytes),
            Self::V3Shared(bytes) => EventStreamGenesis::V3(bytes),
            Self::V4(bytes) => EventStreamGenesis::V4(bytes),
            Self::V4Shared(bytes) => EventStreamGenesis::V4(bytes),
            Self::V5(bytes) => EventStreamGenesis::V5(bytes),
            Self::V5Shared(bytes) => EventStreamGenesis::V5(bytes),
        }
    }
}

/// The one logical event stream owned by a durable JSONL file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalIdentity {
    pub run_id: StableId,
    pub genesis: JournalGenesis,
    verified_v2: Option<Arc<reviewgraphen_core::VerifiedV2Genesis>>,
    verified_v3: Option<Arc<VerifiedV3GenesisIdentity>>,
    verified_v4: Option<Arc<VerifiedV4GenesisIdentity>>,
    verified_v5: Option<Arc<VerifiedV5GenesisIdentity>>,
}

#[derive(Debug, Eq, PartialEq)]
struct VerifiedV3GenesisIdentity {
    run_id: StableId,
    genesis_hash: ContentHash,
}

#[derive(Debug, Eq, PartialEq)]
struct VerifiedV4GenesisIdentity {
    run_id: StableId,
    genesis_hash: ContentHash,
}

#[derive(Debug, Eq, PartialEq)]
struct VerifiedV5GenesisIdentity {
    run_id: StableId,
    genesis_hash: ContentHash,
}

impl JournalIdentity {
    pub fn new(run_id: StableId, genesis: JournalGenesis) -> Result<Self, JournalError> {
        if run_id.kind() != "run" {
            return Err(JournalError::Identity("journal requires a run StableId"));
        }
        // The public Vec input is intentionally consumed here. Subsequent
        // reader/writer identity clones share exactly one immutable backing.
        let genesis = match genesis {
            JournalGenesis::V2(bytes) => JournalGenesis::V2Shared(Arc::from(bytes)),
            JournalGenesis::V3(bytes) => JournalGenesis::V3Shared(Arc::from(bytes)),
            JournalGenesis::V4(bytes) => JournalGenesis::V4Shared(Arc::from(bytes)),
            JournalGenesis::V5(bytes) => JournalGenesis::V5Shared(Arc::from(bytes)),
            other => other,
        };
        let verified_v2 = if let JournalGenesis::V2Shared(bytes) = &genesis {
            // Strict decoding here means no path can initialize a V2 journal
            // with merely hash-shaped, noncanonical genesis bytes.
            Some(Arc::new(
                reviewgraphen_core::VerifiedV2Genesis::from_canonical_bytes(&run_id, bytes)
                    .map_err(map_bounded_domain_error)?,
            ))
        } else {
            None
        };
        let verified_v3 = if let JournalGenesis::V3Shared(bytes) = &genesis {
            // V3 has no public reusable certificate type. Strictly decode its
            // canonical snapshot now and retain its exact immutable hash.
            EventEnvelope::validated_view(
                EventContractVersion::V3,
                &run_id,
                EventStreamGenesis::V3(bytes),
                &[],
            )
            .map_err(map_bounded_domain_error)?;
            Some(Arc::new(VerifiedV3GenesisIdentity {
                run_id: run_id.clone(),
                genesis_hash: ContentHash::sha256(bytes),
            }))
        } else {
            None
        };
        let verified_v4 = if let JournalGenesis::V4Shared(bytes) = &genesis {
            // Event-v4 has a separate discriminator but the same strict
            // canonical baseline shape.  The Core bridge validates that a
            // V4 journal cannot be initialized from merely hash-shaped bytes.
            EventEnvelope::validate_v4_stream(&run_id, bytes, &[])
                .map_err(map_bounded_domain_error)?;
            Some(Arc::new(VerifiedV4GenesisIdentity {
                run_id: run_id.clone(),
                genesis_hash: ContentHash::sha256(bytes),
            }))
        } else {
            None
        };
        let verified_v5 = if let JournalGenesis::V5Shared(bytes) = &genesis {
            let snapshot = RunGenesisSnapshot::from_canonical_v4_bytes_for_store(bytes)
                .map_err(map_bounded_domain_error)?;
            let _ = snapshot
                .rebuild_aggregate()
                .map_err(map_bounded_domain_error)?;
            Some(Arc::new(VerifiedV5GenesisIdentity {
                run_id: run_id.clone(),
                genesis_hash: ContentHash::sha256(bytes),
            }))
        } else {
            None
        };
        Ok(Self {
            run_id,
            genesis,
            verified_v2,
            verified_v3,
            verified_v4,
            verified_v5,
        })
    }
    #[must_use]
    pub fn version(&self) -> EventContractVersion {
        self.genesis.version()
    }
    #[must_use]
    pub fn genesis_hash(&self) -> ContentHash {
        self.genesis.hash()
    }
    #[allow(dead_code)] // Consumed by the following derived-index C-3 unit.
    pub(crate) fn core_genesis(&self) -> EventStreamGenesis<'_> {
        self.genesis.core_genesis()
    }

    pub(crate) fn verified_core_genesis(&self) -> Result<EventStreamGenesis<'_>, JournalError> {
        match (
            &self.genesis,
            &self.verified_v2,
            &self.verified_v3,
            &self.verified_v4,
            &self.verified_v5,
        ) {
            (JournalGenesis::V1(hash), None, None, None, None) => Ok(EventStreamGenesis::V1(hash)),
            (JournalGenesis::V2Shared(bytes), Some(verified), None, None, None)
                if verified.run_id() == &self.run_id
                    && verified.genesis_hash() == &ContentHash::sha256(bytes) =>
            {
                Ok(EventStreamGenesis::V2Verified(verified.as_ref()))
            }
            (JournalGenesis::V3Shared(bytes), None, Some(verified), None, None)
                if verified.run_id == self.run_id
                    && verified.genesis_hash == ContentHash::sha256(bytes) =>
            {
                Ok(EventStreamGenesis::V3(bytes))
            }
            (JournalGenesis::V4Shared(bytes), None, None, Some(verified), None)
                if verified.run_id == self.run_id
                    && verified.genesis_hash == ContentHash::sha256(bytes) =>
            {
                Ok(EventStreamGenesis::V4(bytes))
            }
            (JournalGenesis::V5Shared(bytes), None, None, None, Some(verified))
                if verified.run_id == self.run_id
                    && verified.genesis_hash == ContentHash::sha256(bytes) =>
            {
                Ok(EventStreamGenesis::V5(bytes))
            }
            _ => Err(JournalError::Identity(
                "journal identity no longer matches its verified genesis",
            )),
        }
    }

    /// The V2 canonical genesis shares one immutable allocation among all
    /// lock-held readers and writers. V1 deliberately has no byte backing.
    #[must_use]
    #[allow(dead_code)] // The index migration consumes this shared backing.
    pub(crate) fn v2_genesis_backing(&self) -> Option<&Arc<[u8]>> {
        match &self.genesis {
            JournalGenesis::V2Shared(bytes) => Some(bytes),
            JournalGenesis::V1(_)
            | JournalGenesis::V2(_)
            | JournalGenesis::V3(_)
            | JournalGenesis::V3Shared(_)
            | JournalGenesis::V4(_)
            | JournalGenesis::V4Shared(_)
            | JournalGenesis::V5(_)
            | JournalGenesis::V5Shared(_) => None,
        }
    }

    fn local_metadata_capacity(&self) -> usize {
        self.run_id.allocated_bytes()
            + match &self.genesis {
                JournalGenesis::V1(hash) => hash.allocated_bytes(),
                JournalGenesis::V2(_)
                | JournalGenesis::V2Shared(_)
                | JournalGenesis::V3(_)
                | JournalGenesis::V3Shared(_)
                | JournalGenesis::V4(_)
                | JournalGenesis::V4Shared(_)
                | JournalGenesis::V5(_)
                | JournalGenesis::V5Shared(_) => 0,
            }
    }

    fn shared_certificate_capacity(&self) -> usize {
        let v2 = self.verified_v2.as_ref().map_or(0, |verified| {
            verified
                .allocated_bytes()
                .saturating_add(std::mem::size_of::<usize>() * 2)
        });
        v2.saturating_add(self.verified_v3.as_ref().map_or(0, |verified| {
            verified
                .run_id
                .allocated_bytes()
                .saturating_add(verified.genesis_hash.allocated_bytes())
                .saturating_add(std::mem::size_of::<usize>() * 2)
        }))
        .saturating_add(self.verified_v4.as_ref().map_or(0, |verified| {
            verified
                .run_id
                .allocated_bytes()
                .saturating_add(verified.genesis_hash.allocated_bytes())
                .saturating_add(std::mem::size_of::<usize>() * 2)
        }))
        .saturating_add(self.verified_v5.as_ref().map_or(0, |verified| {
            verified
                .run_id
                .allocated_bytes()
                .saturating_add(verified.genesis_hash.allocated_bytes())
                .saturating_add(std::mem::size_of::<usize>() * 2)
        }))
    }

    pub(crate) fn cloned_local_metadata_capacity(&self) -> Result<u64, JournalError> {
        u64::try_from(self.local_metadata_capacity()).map_err(|_| JournalError::Incomplete {
            limit: u64::MAX,
            observed: u64::MAX,
        })
    }
}

/// Independent bounds for the journal protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JournalLimits {
    pub max_event_line_bytes: u64,
    pub max_events: u64,
    pub max_replay_bytes: u64,
    /// Maximum receipt files across each recovery directory.
    pub max_receipt_files: u64,
    /// Maximum total apparent receipt bytes scanned before trusting a log.
    pub max_receipt_scan_bytes: u64,
    /// Maximum one canonical receipt file.
    pub max_receipt_bytes: u64,
}
impl JournalLimits {
    #[must_use]
    pub const fn from_store(limits: super::StoreLimits) -> Self {
        Self {
            max_event_line_bytes: limits.max_event_line_bytes,
            max_events: limits.max_events,
            max_replay_bytes: limits.max_replay_bytes,
            max_receipt_files: 10_000,
            max_receipt_scan_bytes: 64 * 1024 * 1024,
            max_receipt_bytes: 64 * 1024,
        }
    }
}

/// Typed journal failures; corrupted bytes are never represented as a valid prefix.
#[derive(Debug, Error)]
pub enum JournalError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Domain(#[from] reviewgraphen_core::DomainError),
    #[error("journal I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid journal identity: {0}")]
    Identity(&'static str),
    #[error("journal run has not been initialized")]
    Missing,
    #[error("V1 journals are read-only and cannot be initialized or appended")]
    V1ReadOnly,
    #[error("V3 journals are writable only through a roots-bound replay session")]
    V3ReplaySessionRequired,
    #[error("V4 journals are writable only through a roots-bound replay session")]
    V4ReplaySessionRequired,
    #[error("V5 journals are writable only through a dual-run M6 replay session")]
    V5ReplaySessionRequired,
    #[error("V5 genesis was not committed; stopped at {stage}")]
    GenesisNotCommittedV5 { stage: &'static str },
    #[error("V5 genesis durability is uncertain and requires recovery")]
    GenesisSessionUncertainV5,
    #[error("verification bundle append stopped after {durable_stage:?}")]
    BundleAppendInterrupted {
        durable_stage: VerificationBundleDurableStageV3,
    },
    #[error("verification bundle resume authority does not match this session or durable prefix")]
    BundleResumeAuthorityMismatch,
    #[error("journal has a partially durable verification bundle and permits resume only")]
    SessionResumeRequired,
    #[error(
        "journal needs recovery at byte offset {good_offset}; auto_recoverable={auto_recoverable}"
    )]
    CorruptNeedsRecovery {
        good_offset: u64,
        auto_recoverable: bool,
    },
    #[error("V2/V3 durable logs require a sequence-one RunGenesisManifest")]
    V2GenesisRequired,
    #[error("journal writer is poisoned after failed durable rollback")]
    Poisoned,
    #[error("replayed session cannot expose state after uncertain durable append acknowledgement")]
    SessionUncertain,
    #[error("journal receipt collision or invalid receipt at {name}")]
    ReceiptCorruption { name: String },
    #[error("journal operation exceeded {limit} bytes/events after observing {observed}")]
    Incomplete { limit: u64, observed: u64 },
    #[error("CAS object {hash} exists without its exact durable V3 registration")]
    OrphanCasObjectV3 { hash: ContentHash },
    #[error("canonical gluing-input object {hash} is not the exact next adoptable V4 input")]
    OrphanCanonicalInput { hash: ContentHash },
    #[error("event-v4 gluing-input publication stopped at {stage}")]
    GluingInputPublicationInterruptedV4 { stage: &'static str },
    #[error(
        "event-v4 recovery key is stale, foreign, or does not describe the current durable state"
    )]
    RecoveryKeyMismatchV4,
    #[error("event-v4 genesis publication is uncertain; inspect and recover before retrying")]
    GenesisSessionUncertainV4,
    #[error("event-v4 genesis was not committed at {stage}")]
    GenesisNotCommittedV4 { stage: &'static str },
    #[error("event-v4 recovery durability is uncertain after {outcome:?}")]
    RecoveryDurabilityUncertainV4 { outcome: RecoveryOutcomeV4 },
    #[error("the exact M5 profile has no completed atomic bundle")]
    M5ProfileIncompleteV4,
}

/// Confirmation returned only after the appended line has reached `sync_data`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalAppendReceipt {
    pub sequence: u64,
    pub event_hash: ContentHash,
    pub tail_offset: u64,
}

/// The fixed recovery operation requested for an event-v4 run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryKindV4 {
    GenesisBootstrap,
    CanonicalTail,
    /// Reserved for the later M4-at-V4 bundle unit.  This bootstrap unit
    /// deliberately refuses it rather than guessing an authority plan.
    M4BundleResume,
}

/// Closed request used to inspect one event-v4 durable state.  This is not an
/// append/replay capability; the returned opaque key binds the observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryInspectionV4 {
    run_id: StableId,
    genesis_hash: ContentHash,
    event_contract_version: &'static str,
    expected_kind: RecoveryKindV4,
}

impl RecoveryInspectionV4 {
    #[must_use]
    pub fn new(run_id: StableId, genesis_hash: ContentHash, expected_kind: RecoveryKindV4) -> Self {
        Self {
            run_id,
            genesis_hash,
            event_contract_version: EVENT_CONTRACT_SCHEMA_V4,
            expected_kind,
        }
    }
}

/// An opaque one-shot description of a lock-held recovery inspection.  Its
/// private fields and absence of Clone/serde prevent callers from minting,
/// editing, serializing, or replaying recovery authority.
#[derive(Debug)]
pub struct RecoveryKeyV4 {
    run_id: StableId,
    genesis_hash: ContentHash,
    event_contract_version: &'static str,
    expected_kind: RecoveryKindV4,
    pre_recovery_offset: u64,
    pre_recovery_tail_hash: ContentHash,
    pre_recovery_file_hash: Option<ContentHash>,
    pending_digest: Option<ContentHash>,
}

impl RecoveryKeyV4 {
    #[must_use]
    pub fn expected_kind(&self) -> RecoveryKindV4 {
        self.expected_kind
    }
}

/// Exhaustive M4 marker-prefix classification reserved for the later
/// authority-aware M4-at-V4 recovery implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum M4BundlePrefixClassificationV4 {
    Stage0,
    StrictInterior,
    AlreadyComplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct M4BundlePrefixStageV4 {
    classification: M4BundlePrefixClassificationV4,
    confirmed_events: u64,
    expected_events: u64,
}

impl M4BundlePrefixStageV4 {
    #[must_use]
    pub const fn classification(&self) -> M4BundlePrefixClassificationV4 {
        self.classification
    }

    #[must_use]
    pub const fn confirmed_events(&self) -> u64 {
        self.confirmed_events
    }

    #[must_use]
    pub const fn expected_events(&self) -> u64 {
        self.expected_events
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum M4BundleMarkerActionV4 {
    ClearedAndSynced,
    RetainedForResume,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct M4BundleMarkerRecoveryReceiptV4 {
    pub prefix_stage: M4BundlePrefixStageV4,
    pub action: M4BundleMarkerActionV4,
    pub pre_marker_hash: ContentHash,
    pub post_marker_hash: Option<ContentHash>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryOutcomeV4 {
    GenesisNotCommitted,
    GenesisCommitted {
        event_id: StableId,
        event_hash: ContentHash,
        confirmed_offset: u64,
    },
    TailRecovered {
        good_offset: u64,
        discarded_hash: ContentHash,
    },
    M4BundleCleanupOrdinary {
        prefix_stage: M4BundlePrefixStageV4,
    },
    M4BundleResumeRequired {
        prefix_stage: M4BundlePrefixStageV4,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryProvenanceV4 {
    actor: String,
    tool_version: String,
}

impl RecoveryProvenanceV4 {
    pub fn new(
        actor: impl Into<String>,
        tool_version: impl Into<String>,
    ) -> Result<Self, JournalError> {
        let value = Self {
            actor: actor.into(),
            tool_version: tool_version.into(),
        };
        validate_recovery_actor(&value.actor, &value.tool_version)?;
        Ok(value)
    }

    #[must_use]
    pub fn actor(&self) -> &str {
        &self.actor
    }

    #[must_use]
    pub fn tool_version(&self) -> &str {
        &self.tool_version
    }
}

/// Attribution for a recovery mutation.  The opaque key is retained rather
/// than re-exposed as an append capability.
#[derive(Debug)]
pub struct RecoveryReceiptV4 {
    schema: &'static str,
    key: RecoveryKeyV4,
    kind: RecoveryKindV4,
    outcome: RecoveryOutcomeV4,
    provenance: RecoveryProvenanceV4,
    timestamp_unix_seconds: u64,
    pre_file_hash: Option<ContentHash>,
    post_file_hash: Option<ContentHash>,
    marker_recovery: Option<M4BundleMarkerRecoveryReceiptV4>,
}

impl RecoveryReceiptV4 {
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        self.schema
    }
    #[must_use]
    pub fn kind(&self) -> RecoveryKindV4 {
        self.kind
    }
    #[must_use]
    pub fn outcome(&self) -> &RecoveryOutcomeV4 {
        &self.outcome
    }
    #[must_use]
    pub fn pre_file_hash(&self) -> Option<&ContentHash> {
        self.pre_file_hash.as_ref()
    }
    #[must_use]
    pub fn post_file_hash(&self) -> Option<&ContentHash> {
        self.post_file_hash.as_ref()
    }
    #[must_use]
    pub fn marker_recovery(&self) -> Option<&M4BundleMarkerRecoveryReceiptV4> {
        self.marker_recovery.as_ref()
    }
    #[must_use]
    pub fn recovery_key_kind(&self) -> RecoveryKindV4 {
        self.key.expected_kind
    }
    #[must_use]
    pub fn run_id(&self) -> &StableId {
        &self.key.run_id
    }
    #[must_use]
    pub fn genesis_hash(&self) -> &ContentHash {
        &self.key.genesis_hash
    }
    #[must_use]
    pub const fn event_contract_version(&self) -> &'static str {
        self.key.event_contract_version
    }
    #[must_use]
    pub const fn pre_recovery_offset(&self) -> u64 {
        self.key.pre_recovery_offset
    }
    #[must_use]
    pub fn pre_recovery_tail_hash(&self) -> &ContentHash {
        &self.key.pre_recovery_tail_hash
    }
    #[must_use]
    pub fn pending_digest(&self) -> Option<&ContentHash> {
        self.key.pending_digest.as_ref()
    }
    #[must_use]
    pub fn provenance(&self) -> &RecoveryProvenanceV4 {
        &self.provenance
    }
    #[must_use]
    pub const fn timestamp_unix_seconds(&self) -> u64 {
        self.timestamp_unix_seconds
    }
}

/// Confirmation of the sole V4 bootstrap event after CAS and journal bytes
/// are both durable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenesisCommitReceiptV4 {
    pub run_id: StableId,
    pub genesis_hash: ContentHash,
    pub event_id: StableId,
    pub event_hash: ContentHash,
    pub confirmed_offset: u64,
}

/// Confirmation of the sole V5 bootstrap event after CAS and journal bytes
/// are both durable. This is descriptive durability, not M6 session authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenesisCommitReceiptV5 {
    pub run_id: StableId,
    pub genesis_hash: ContentHash,
    pub event_id: StableId,
    pub event_hash: ContentHash,
    pub confirmed_offset: u64,
}

pub enum GenesisRecoveryV4<'a> {
    NotCommitted {
        receipt: RecoveryReceiptV4,
    },
    Committed {
        receipt: RecoveryReceiptV4,
        journal: EventJournal<'a>,
    },
}

/// Exact count of ordered bundle events known durable at an interrupted
/// append boundary. It is descriptive state, never append authority.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationBundleDurableStageV3 {
    confirmed_events: u64,
    expected_events: u64,
}

impl VerificationBundleDurableStageV3 {
    #[must_use]
    pub const fn confirmed_events(self) -> u64 {
        self.confirmed_events
    }

    #[must_use]
    pub const fn expected_events(self) -> u64 {
        self.expected_events
    }
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct VerificationBundlePendingMarkerV3 {
    schema: String,
    run_id: StableId,
    genesis_hash: ContentHash,
    pre_tail_hash: ContentHash,
    pre_offset: u64,
    bundle_digest: ContentHash,
    expected_event_ids: Vec<StableId>,
    expected_event_hashes: Vec<ContentHash>,
    expected_envelopes: Vec<serde_json::Value>,
    expected_count: u64,
}

impl VerificationBundlePendingMarkerV3 {
    fn new(
        identity: &JournalIdentity,
        state: &ScanState,
        envelopes: &[EventEnvelope],
    ) -> Result<Self, JournalError> {
        let expected_envelopes = envelopes
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| reviewgraphen_core::DomainError::Json(error.to_string()))?;
        let bundle_digest = ContentHash::sha256(&canonical_json(&expected_envelopes)?);
        Ok(Self {
            schema: BUNDLE_MARKER_SCHEMA.to_owned(),
            run_id: identity.run_id.clone(),
            genesis_hash: identity.genesis_hash(),
            pre_tail_hash: state.tail_hash.clone(),
            pre_offset: state.confirmed_offset,
            bundle_digest,
            expected_event_ids: envelopes
                .iter()
                .map(|envelope| envelope.id().clone())
                .collect(),
            expected_event_hashes: envelopes
                .iter()
                .map(|envelope| envelope.event_hash().clone())
                .collect(),
            expected_envelopes,
            expected_count: u64::try_from(envelopes.len()).map_err(|_| {
                JournalError::Incomplete {
                    limit: 3,
                    observed: u64::MAX,
                }
            })?,
        })
    }

    fn validate(
        &self,
        identity: &JournalIdentity,
        limits: JournalLimits,
    ) -> Result<(), JournalError> {
        let expected =
            usize::try_from(self.expected_count).map_err(|_| JournalError::ReceiptCorruption {
                name: BUNDLE_PENDING_MARKER.to_owned(),
            })?;
        if self.schema != BUNDLE_MARKER_SCHEMA
            || self.run_id != identity.run_id
            || self.genesis_hash != identity.genesis_hash()
            || expected == 0
            || expected > 3
            || self.expected_event_ids.len() != expected
            || self.expected_event_hashes.len() != expected
            || self.expected_envelopes.len() != expected
            || self.pre_offset > limits.max_replay_bytes
            || self.bundle_digest != ContentHash::sha256(&canonical_json(&self.expected_envelopes)?)
        {
            return Err(JournalError::ReceiptCorruption {
                name: BUNDLE_PENDING_MARKER.to_owned(),
            });
        }
        Ok(())
    }

    fn envelopes(&self, limits: JournalLimits) -> Result<Vec<EventEnvelope>, JournalError> {
        self.expected_envelopes
            .iter()
            .zip(&self.expected_event_ids)
            .zip(&self.expected_event_hashes)
            .map(|((value, id), hash)| {
                let bytes = canonical_json(value)?;
                limit(
                    u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                    limits.max_event_line_bytes.saturating_sub(1),
                )?;
                let envelope = EventEnvelope::from_json_slice(&bytes)?;
                if envelope.id() != id || envelope.event_hash() != hash {
                    return Err(JournalError::ReceiptCorruption {
                        name: BUNDLE_PENDING_MARKER.to_owned(),
                    });
                }
                Ok(envelope)
            })
            .collect()
    }
}

/// Intent written before mutating a torn tail.  The serialized form is
/// canonical and create-only, so a recovery operation itself is auditable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryIntent {
    pub recovery_id: StableId,
    pub run_id: StableId,
    pub genesis_hash: ContentHash,
    /// Generic recovery uses a kernel nonce. Bundle recovery uses a
    /// domain-separated deterministic digest so retries name the same pair.
    pub nonce: String,
    pub good_offset: u64,
    pub discarded_hash: ContentHash,
    /// Present for bundle-tail recovery. Historical V1/V2 receipts omit these
    /// fields byte-for-byte, preserving their canonical receipt schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discarded_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discarded_len: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_digest: Option<ContentHash>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_pre_offset: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_recovery_kind: Option<String>,
    pub pre_tail_hash: ContentHash,
    pub actor: String,
    pub tool_version: String,
    pub timestamp_unix_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCompletion {
    pub recovery_id: StableId,
    pub post_file_hash: ContentHash,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalRecoveryReceipt {
    pub intent: RecoveryIntent,
    pub completion: RecoveryCompletion,
    pub resumed: bool,
}

/// Descriptor-relative journal entry point.  The journal path is never rebuilt
/// from a caller-controlled workspace path.
pub struct EventJournal<'a> {
    #[allow(dead_code)] // Retains the StoreRoot lifetime and test display path.
    root: &'a StoreRoot,
    identity: JournalIdentity,
    limits: JournalLimits,
    run: OwnedFd,
    #[cfg(test)]
    recovery_faults: Mutex<std::collections::VecDeque<RecoveryFault>>,
}

pub struct JournalReader {
    #[allow(dead_code)] // Owns the shared lock for the reader's entire lifetime.
    file: File,
    identity: JournalIdentity,
    limits: JournalLimits,
    state: ScanState,
}

/// Compact, lock-bound summary of one fully confirmed journal prefix.  It
/// contains no envelopes, decoded payloads, raw JSONL bytes, or torn suffix.
#[allow(dead_code)] // Wired by the in-flight index streaming migration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PrefixCertificate {
    pub(crate) confirmed_offset: u64,
    pub(crate) tail_hash: ContentHash,
    pub(crate) event_count: u64,
    pub(crate) line_buffer_capacity: u64,
    pub(crate) canonical_scratch_capacity: u64,
}

/// A shared-lock journal reader reserved for derived-index construction.  It
/// intentionally has no `events()` API: callers must consume each envelope
/// in the bounded streaming callback and retain only their own compact
/// projection state.
#[allow(dead_code)] // Wired by the in-flight index streaming migration.
pub(crate) struct IndexJournalReader {
    #[allow(dead_code)] // Holds the shared journal lock for the replay.
    file: File,
    identity: JournalIdentity,
    limits: JournalLimits,
    working_limit: u64,
    completed_receipts: Vec<(RecoveryIntent, RecoveryCompletion)>,
}

/// Distinguishes a corrupt/bounded durable input from a projection callback
/// failure without forcing the journal module to depend on the index error
/// type.
#[allow(dead_code)] // Wired by the in-flight index streaming migration.
#[derive(Debug)]
pub(crate) enum IndexReplayError<E> {
    Journal(JournalError),
    Visitor(E),
}
pub struct ReplayedV2RunSession {
    writer: JournalWriter,
    log: EventLog,
    store_root_identity: StoreRootIdentity,
    state: ReplayedV2RunSessionState,
}

/// A lock-held V3 log whose authority state was rebuilt exclusively from the
/// canonical journal prefix, exact FD-relative CAS bytes, and host trust
/// roots.  Its fields are private so neither the mutable log nor its writer
/// can escape this authority boundary.
pub struct ReplayedV3RunSession<'root, 'roots> {
    writer: JournalWriter,
    log: EventLog,
    resolver: JournalAuthorityResolverV3<'root>,
    roots: &'roots AuthorityTrustRootsV3,
    store_root_identity: StoreRootIdentity,
    state: ReplayedV3RunSessionState,
}

/// A lock-held homogeneous V4 log whose authority basis was rebuilt from the
/// complete canonical prefix, exact CAS bytes, and the caller's V4 trust
/// roots. Private fields prevent Store state or Core authority from escaping
/// the session boundary.
#[allow(dead_code)] // Subsequent V4 append units consume this closed session state.
pub struct ReplayedV4RunSession<'root, 'roots> {
    _root_lock: V4RootLock,
    _run_lock: OwnedFd,
    writer: JournalWriter,
    log: EventLogV4,
    index_genesis: RunGenesisSnapshot,
    resolver: JournalAuthorityResolverV4<'root>,
    roots: &'roots AuthorityTrustRootsV4,
    session_identity: OpaqueSessionIdentityV4,
    state: ReplayedV4RunSessionState,
    index_projection: Option<crate::index::ReplayProjectionChargeV5>,
}

/// Lock-held structural V5 target prefix. It contains no M6 append authority;
/// only the dual-run admission module may consume it into a session proof.
pub(crate) struct ReplayedV5RunSession {
    _run_lock: OwnedFd,
    writer: JournalWriter,
    log: EventLogV5,
    store_root_identity: StoreRootIdentity,
}

/// Narrow callback surface for one lock-contiguous M5 profile operation.
/// Core-issued input capabilities remain private and cannot escape through
/// the callback result; callers can only consume them in canonical order.
pub struct M5GluingProfileSessionV4<'session, 'root, 'roots> {
    session: &'session mut ReplayedV4RunSession<'root, 'roots>,
    basis: &'session mut AuthorityReplayBasisV4,
    remaining_inputs: Vec<M5GluingProfileInputV4>,
    source_ids: std::collections::BTreeSet<StableId>,
}

/// Opaque completed-profile authority used by Report V4. The augmented trust
/// roots remain private while the caller can request one fully revalidated
/// schema-v5 snapshot.
pub struct CompletedM5ReportAuthorityV4 {
    roots: AuthorityTrustRootsV4,
    completed: M5CompletedGluingProfileV4,
}

pub enum M5ReportAuthorityInspectionV4 {
    Complete(Box<CompletedM5ReportAuthorityV4>),
    Incomplete { registered_inputs: u64 },
}

#[derive(Debug, Error)]
pub enum IncrementalSessionError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Index(#[from] crate::IndexError),
    #[error(transparent)]
    Core(#[from] M6Error),
    #[error("incremental session authority refused: {0}")]
    Authority(&'static str),
    #[error("incremental session working set exceeds {limit} bytes: observed {observed}")]
    Incomplete { limit: u64, observed: u64 },
}

/// Store-owned acceptance of one M6 mapping while both journal prefixes,
/// both index projections, and their CAS bindings remain locked and live.
/// The Core DTOs exposed by the accessors are borrowed deterministic views;
/// they carry no authority without this non-cloneable, non-serializable owner.
///
/// External callers cannot construct the accepted owner:
///
/// ```compile_fail
/// use reviewgraphen_store::AcceptedIncrementalMappingV5;
/// let _ = AcceptedIncrementalMappingV5 {};
/// ```
///
/// It cannot be cloned, serialized, or split into appendable raw parts:
///
/// ```compile_fail
/// use reviewgraphen_store::AcceptedIncrementalMappingV5;
/// fn requires_clone<T: Clone>(_: &T) {}
/// fn rejected(value: &AcceptedIncrementalMappingV5<'_, '_, '_>) {
///     requires_clone(value);
///     let _ = serde_json::to_vec(value);
///     let _ = value.into_parts();
/// }
/// ```
pub struct AcceptedIncrementalMappingV5<'root, 'roots, 'index> {
    proof: IncrementalSessionProofV5<'root, 'roots, 'index>,
    proposal: UntrustedIncrementalMappingProposalV5,
    accounting: DualSessionAccountingV5,
}

impl AcceptedIncrementalMappingV5<'_, '_, '_> {
    #[must_use]
    pub const fn closure(&self) -> &IncrementalSourceClosureV5 {
        let _ = &self.proof;
        self.proposal.closure()
    }

    #[must_use]
    pub const fn mapping_phase(&self) -> &M6MappingPhaseV5 {
        self.proposal.mapping_phase()
    }

    #[must_use]
    pub fn morphism(&self) -> &reviewgraphen_core::ChangeMorphismV5 {
        self.proposal.morphism()
    }

    #[must_use]
    pub const fn working_peak_bytes(&self) -> u64 {
        self.accounting.peak_bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DualSessionAccountingV5 {
    source_journal_bytes: u64,
    target_journal_bytes: u64,
    source_index_bytes: u64,
    target_index_bytes: u64,
    source_index_owned_bytes: u64,
    target_index_owned_bytes: u64,
    source_cas_buffer_bytes: u64,
    target_cas_buffer_bytes: u64,
    source_event_line_bytes: u64,
    target_event_line_bytes: u64,
    mapping_reservation_bytes: u64,
    peak_bytes: u64,
}

impl DualSessionAccountingV5 {
    fn checked(parts: [u64; 11]) -> Result<Self, IncrementalSessionError> {
        let mut peak_bytes = 0_u64;
        for part in parts {
            peak_bytes =
                peak_bytes
                    .checked_add(part)
                    .ok_or(IncrementalSessionError::Incomplete {
                        limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                        observed: u64::MAX,
                    })?;
            if peak_bytes > MAX_INCREMENTAL_SESSION_WORKING_BYTES {
                return Err(IncrementalSessionError::Incomplete {
                    limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                    observed: peak_bytes,
                });
            }
        }
        let value = Self {
            source_journal_bytes: parts[0],
            target_journal_bytes: parts[1],
            source_index_bytes: parts[2],
            target_index_bytes: parts[3],
            source_index_owned_bytes: parts[4],
            target_index_owned_bytes: parts[5],
            source_cas_buffer_bytes: parts[6],
            target_cas_buffer_bytes: parts[7],
            source_event_line_bytes: parts[8],
            target_event_line_bytes: parts[9],
            mapping_reservation_bytes: parts[10],
            peak_bytes,
        };
        debug_assert_eq!(value.recomputed_peak(), peak_bytes);
        Ok(value)
    }

    fn recomputed_peak(&self) -> u64 {
        [
            self.source_journal_bytes,
            self.target_journal_bytes,
            self.source_index_bytes,
            self.target_index_bytes,
            self.source_index_owned_bytes,
            self.target_index_owned_bytes,
            self.source_cas_buffer_bytes,
            self.target_cas_buffer_bytes,
            self.source_event_line_bytes,
            self.target_event_line_bytes,
            self.mapping_reservation_bytes,
        ]
        .into_iter()
        .sum()
    }
}

struct DualSessionLiveBuffersV5 {
    source_cas: Vec<u8>,
    source_event_line: Vec<u8>,
    target_event_line: Vec<u8>,
}

impl DualSessionLiveBuffersV5 {
    fn observed_lengths(&self, target_cas: &[u8]) -> [u64; 4] {
        [
            u64::try_from(self.source_cas.len()).unwrap_or(u64::MAX),
            u64::try_from(target_cas.len()).unwrap_or(u64::MAX),
            u64::try_from(self.source_event_line.len()).unwrap_or(u64::MAX),
            u64::try_from(self.target_event_line.len()).unwrap_or(u64::MAX),
        ]
    }
}

fn checked_incremental_component_add(
    left: u64,
    right: u64,
) -> Result<u64, IncrementalSessionError> {
    left.checked_add(right)
        .ok_or(IncrementalSessionError::Incomplete {
            limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
            observed: u64::MAX,
        })
}

fn retain_largest_canonical_event_line(
    prefix: &[u8],
    expected: u64,
) -> Result<Vec<u8>, IncrementalSessionError> {
    let largest = prefix
        .split_inclusive(|byte| *byte == b'\n')
        .max_by_key(|line| line.len())
        .unwrap_or_default();
    if !largest.ends_with(b"\n") || u64::try_from(largest.len()).ok() != Some(expected) {
        return Err(IncrementalSessionError::Authority(
            "retained canonical event-line bytes differ from replay accounting",
        ));
    }
    let mut retained = Vec::new();
    retained
        .try_reserve_exact(largest.len())
        .map_err(|_| IncrementalSessionError::Incomplete {
            limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
            observed: expected,
        })?;
    retained.extend_from_slice(largest);
    Ok(retained)
}

/// Store-owned dual-prefix authority.  It is intentionally private, neither
/// cloneable nor serializable, and can only be consumed into deterministic M6
/// values while its source and target journal locks remain held.
struct IncrementalSessionProofV5<'source_root, 'roots, 'index> {
    root: &'source_root StoreRoot,
    _source_index: crate::ValidatedIndexSnapshotV5<'index, 'source_root, 'roots>,
    _source_session: ReplayedV4RunSession<'source_root, 'roots>,
    source_basis: AuthorityReplayBasisV4,
    completed: M5CompletedGluingProfileV4,
    target_index: crate::ValidatedIndexSnapshotV6,
    source_index_canonical_bytes: Vec<u8>,
    source_journal_bytes: Vec<u8>,
    target_journal_bytes: Vec<u8>,
    _live_buffers: Option<DualSessionLiveBuffersV5>,
}

impl<'root, 'roots, 'index> IncrementalSessionProofV5<'root, 'roots, 'index> {
    fn retain_largest_source_cas(&self, expected: u64) -> Result<Vec<u8>, IncrementalSessionError> {
        if expected == 0 {
            return Ok(Vec::new());
        }
        let snapshot = self._source_index.snapshot();
        let candidate = snapshot
            .artifact_registrations
            .iter()
            .filter(|row| row.size == expected)
            .map(|row| (&row.cas_hash, row.size))
            .chain(
                snapshot
                    .artifact_registrations_v4
                    .iter()
                    .filter(|row| row.size == expected)
                    .map(|row| (&row.cas_hash, row.size)),
            )
            .next();
        let (hash, size) = if let Some(candidate) = candidate {
            candidate
        } else {
            let genesis = self._source_session.index_v5_genesis()?;
            if u64::try_from(
                genesis
                    .canonical_bytes()
                    .map_err(JournalError::Domain)?
                    .len(),
            )
            .ok()
                != Some(expected)
            {
                return Err(IncrementalSessionError::Authority(
                    "largest source CAS observation has no retained registration",
                ));
            }
            let hash = self._source_session.log.genesis_hash();
            let cas_hash = CasHash::parse(hash.to_string()).map_err(JournalError::Store)?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(usize::try_from(expected).map_err(|_| {
                    IncrementalSessionError::Incomplete {
                        limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                        observed: expected,
                    }
                })?)
                .map_err(|_| IncrementalSessionError::Incomplete {
                    limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                    observed: expected,
                })?;
            CasReader::open_existing(self.root)
                .map_err(JournalError::Store)?
                .read_into(&cas_hash, Some(expected), &mut bytes)
                .map_err(JournalError::Store)?;
            return Ok(bytes);
        };
        let cas_hash = CasHash::parse(hash.to_string()).map_err(JournalError::Store)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(usize::try_from(size).map_err(|_| {
                IncrementalSessionError::Incomplete {
                    limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                    observed: size,
                }
            })?)
            .map_err(|_| IncrementalSessionError::Incomplete {
                limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                observed: size,
            })?;
        CasReader::open_existing(self.root)
            .map_err(JournalError::Store)?
            .read_into(&cas_hash, Some(size), &mut bytes)
            .map_err(JournalError::Store)?;
        Ok(bytes)
    }

    fn accept_mapping(
        mut self,
    ) -> Result<AcceptedIncrementalMappingV5<'root, 'roots, 'index>, IncrementalSessionError> {
        self.target_index.revalidate_cas(self.root)?;
        let (
            mapping_reservation_bytes,
            source_cas_bytes,
            target_cas_bytes,
            source_line_bytes,
            target_line_bytes,
            source_event_bytes,
            target_event_bytes,
        ) = {
            let source_program = self
                ._source_session
                .index_v5_genesis()?
                .program_space_for_store();
            let target_program = &self.target_index.snapshot().program_space;
            let mapping_reservation_bytes = u64::try_from(
                ChangeMorphismV5::mapping_reservation_bytes_from_accepted_program_facts(
                    source_program,
                    target_program,
                )?,
            )
            .map_err(|_| IncrementalSessionError::Incomplete {
                limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                observed: u64::MAX,
            })?;
            let source_projection = self._source_session.index_v5_replay_projection()?;
            let source_line_bytes = source_projection.max_event_line_bytes();
            let target_line_bytes = self.target_index.max_event_line_bytes();
            (
                mapping_reservation_bytes,
                source_projection.max_cas_bytes(),
                self.target_index.max_cas_bytes(),
                source_line_bytes,
                target_line_bytes,
                checked_incremental_component_add(
                    self._source_session.retained_event_bytes()?,
                    source_line_bytes,
                )?,
                checked_incremental_component_add(
                    self.target_index.retained_event_bytes()?,
                    target_line_bytes,
                )?,
            )
        };
        let accounting = DualSessionAccountingV5::checked([
            self._source_session.writer.state.confirmed_offset,
            self.target_index.confirmed_journal_bytes(),
            u64::try_from(self.source_index_canonical_bytes.len()).map_err(|_| {
                IncrementalSessionError::Incomplete {
                    limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                    observed: u64::MAX,
                }
            })?,
            u64::try_from(self.target_index.canonical_snapshot_bytes().len()).map_err(|_| {
                IncrementalSessionError::Incomplete {
                    limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                    observed: u64::MAX,
                }
            })?,
            self._source_index.recursive_owned_bytes()?,
            self.target_index.decoded_owned_bytes(),
            source_cas_bytes,
            target_cas_bytes,
            source_event_bytes,
            target_event_bytes,
            mapping_reservation_bytes,
        ])?;

        // Real confirmed prefix/CAS/event-line bytes are retained only after
        // every simultaneous term, including Core's exact mapping
        // reservation, has been admitted.
        self.source_journal_bytes = self._source_session.retain_confirmed_prefix_bytes()?;
        self.target_journal_bytes = self.target_index.retain_confirmed_journal_prefix()?;
        if u64::try_from(self.source_journal_bytes.len()).ok()
            != Some(accounting.source_journal_bytes)
            || u64::try_from(self.target_journal_bytes.len()).ok()
                != Some(accounting.target_journal_bytes)
        {
            return Err(IncrementalSessionError::Authority(
                "retained journal prefix length changed after dual-session admission",
            ));
        }
        let live_buffers = DualSessionLiveBuffersV5 {
            source_cas: self.retain_largest_source_cas(source_cas_bytes)?,
            source_event_line: retain_largest_canonical_event_line(
                &self.source_journal_bytes,
                source_line_bytes,
            )?,
            target_event_line: retain_largest_canonical_event_line(
                &self.target_journal_bytes,
                target_line_bytes,
            )?,
        };
        if live_buffers.observed_lengths(self.target_index.largest_verified_cas_bytes())
            != [
                source_cas_bytes,
                target_cas_bytes,
                source_line_bytes,
                target_line_bytes,
            ]
        {
            return Err(IncrementalSessionError::Authority(
                "retained CAS/event buffers differ from admitted observations",
            ));
        }
        self._live_buffers = Some(live_buffers);
        let proposal = derive_untrusted_incremental_mapping_proposal_v5(
            &self._source_session.log,
            &self.source_basis,
            &self.completed,
            &ContentHash::sha256(&self.source_index_canonical_bytes),
            self.target_index.event_log(),
            self.target_index.snapshot_hash(),
        )?;
        if u64::try_from(proposal.mapping_phase().working_peak_upper_bound_bytes()).ok()
            != Some(accounting.mapping_reservation_bytes)
        {
            return Err(IncrementalSessionError::Authority(
                "Core mapping realization differs from its admitted reservation",
            ));
        }
        Ok(AcceptedIncrementalMappingV5 {
            proof: self,
            proposal,
            accounting,
        })
    }
}

impl CompletedM5ReportAuthorityV4 {
    pub fn rebuild_v5(
        &self,
        index: &crate::DerivedIndexV5<'_>,
        journal: &EventJournal<'_>,
    ) -> Result<crate::IndexRebuildReceiptV5, crate::IndexError> {
        index.rebuild_v5(journal, &self.roots)
    }

    pub fn validated_snapshot_v5<'index, 'root>(
        &self,
        index: &'index crate::DerivedIndexV5<'root>,
        journal: &EventJournal<'_>,
    ) -> Result<crate::ValidatedIndexSnapshotV5<'index, 'root, '_>, crate::IndexError> {
        index.validated_snapshot_current_v5(journal, &self.roots)
    }

    /// Replays and binds the exact completed M5 source, current source index,
    /// complete V5 target predecessor, and its CAS-backed V6 projection.  No
    /// caller-provided OID, tree, prefix, index hash, or completion flag is
    /// accepted by this seam.
    pub fn derive_incremental_mapping_v5<'authority, 'root, 'index>(
        &'authority self,
        source_journal: &EventJournal<'root>,
        source_index: &'index crate::DerivedIndexV5<'root>,
        target_journal: &EventJournal<'root>,
        target_index: &crate::DerivedIndexV6<'root>,
    ) -> Result<AcceptedIncrementalMappingV5<'root, 'authority, 'index>, IncrementalSessionError>
    {
        self.mint_incremental_session_v5(
            source_journal,
            source_index,
            target_journal,
            target_index,
        )?
        .accept_mapping()
    }

    fn mint_incremental_session_v5<'root, 'index>(
        &self,
        source_journal: &EventJournal<'root>,
        source_index: &'index crate::DerivedIndexV5<'root>,
        target_journal: &EventJournal<'root>,
        target_index: &crate::DerivedIndexV6<'root>,
    ) -> Result<IncrementalSessionProofV5<'root, '_, 'index>, IncrementalSessionError> {
        if source_journal.root.identity() != target_journal.root.identity()
            || !source_index.matches_store_root(source_journal.root)
            || !target_index.matches_store_root(source_journal.root)
        {
            return Err(IncrementalSessionError::Authority(
                "source journal/index and target journal/index must share one admitted StoreRoot",
            ));
        }

        // Rebuild the complete source projection first.  The following source
        // replay lock then freezes that exact prefix before the target lock is
        // acquired, preserving source -> target lock order.
        let source_view = self.validated_snapshot_v5(source_index, source_journal)?;
        let source_snapshot = source_view.snapshot();
        let source_index_canonical_bytes = source_view.canonical_snapshot_bytes()?;

        if source_snapshot.marker.tail_hash != *self.completed.confirmed_tail_hash()
            || source_snapshot.marker.event_count != self.completed.confirmed_event_count()
            || source_snapshot.gluing_attempts.len() != 1
            || source_snapshot.gluing_attempts[0].event_id != *self.completed.event_id()
            || source_snapshot.gluing_input_descriptors.len() != 2
            || source_snapshot.artifact_registrations_v4.len() != 2
        {
            return Err(IncrementalSessionError::Authority(
                "source index is not the exact completed M5 gluing prefix",
            ));
        }
        let result_event_matches = source_snapshot
            .global_candidates
            .iter()
            .any(|row| row.event_id == *self.completed.event_id())
            || source_snapshot
                .gluing_obstructions
                .iter()
                .any(|row| row.event_id == *self.completed.event_id());
        if !result_event_matches {
            return Err(IncrementalSessionError::Authority(
                "source gluing result is not bound to the completed bundle event",
            ));
        }

        let (source_session, source_basis, target_session) =
            source_journal.replayed_incremental_pair_v5(target_journal, &self.roots)?;
        if source_basis.confirmed_tail_hash() != &source_snapshot.marker.tail_hash
            || source_basis.confirmed_event_count() != source_snapshot.marker.event_count
            || source_basis.policy_revision_hash() != &source_snapshot.marker.policy_revision_hash
            || source_basis.basis_digest() != &source_snapshot.marker.authority_replay_basis_digest
        {
            return Err(IncrementalSessionError::Authority(
                "source replay and source index prefix bindings differ",
            ));
        }
        let source_genesis = source_session.index_v5_genesis()?;
        let source_program = source_genesis.program_space_for_store();

        let target_view = target_index.validated_snapshot_from_session_v6(target_session)?;
        if target_view.store_root_identity() != source_journal.root.identity() {
            return Err(IncrementalSessionError::Authority(
                "target projection changed StoreRoot identity",
            ));
        }
        target_view.revalidate_cas(source_journal.root)?;
        let target = target_view.snapshot();
        let source_repository_identity_hash =
            crate::index::repository_identity_hash_v6(source_program)?;
        if source_program.repository_id() != &target.repository_id
            || source_repository_identity_hash != target.repository_identity_hash
        {
            return Err(IncrementalSessionError::Authority(
                "source and target repository identities differ",
            ));
        }

        let source_git = source_program.accepted_git_revision_closure().ok_or(
            IncrementalSessionError::Authority(
                "source ProgramSpace lacks accepted Git revision closure",
            ),
        )?;
        let target_git = target.program_space.accepted_git_revision_closure().ok_or(
            IncrementalSessionError::Authority(
                "target ProgramSpace lacks accepted Git revision closure",
            ),
        )?;
        if source_git.target_commit_oid() != target_git.base_commit_oid()
            || source_git.target_tree_hash() != target_git.base_tree_hash()
        {
            return Err(IncrementalSessionError::Authority(
                "accepted source target and target base Git revisions differ",
            ));
        }

        Ok(IncrementalSessionProofV5 {
            root: source_journal.root,
            _source_index: source_view,
            _source_session: source_session,
            source_basis,
            completed: self.completed.clone(),
            target_index: target_view,
            source_index_canonical_bytes,
            source_journal_bytes: Vec::new(),
            target_journal_bytes: Vec::new(),
            _live_buffers: None,
        })
    }
}

/// Result of one profile-specific canonical-tail recovery. The complete
/// branch proves that the uncertain bundle was already durable through the
/// same full roots-bound replay; no callback or second append is attempted.
pub enum M5GluingProfileRecoveryV4<T> {
    Continued {
        recovery: RecoveryReceiptV4,
        value: T,
    },
    AlreadyComplete {
        recovery: RecoveryReceiptV4,
        completed: M5CompletedGluingProfileV4,
    },
}

impl M5GluingProfileSessionV4<'_, '_, '_> {
    #[must_use]
    pub fn remaining_input_count(&self) -> usize {
        self.remaining_inputs.len()
    }

    #[must_use]
    pub fn source_ids(&self) -> &std::collections::BTreeSet<StableId> {
        &self.source_ids
    }

    /// Arms the next ordinary append acknowledgement boundary for an
    /// integration-test-only post-sync uncertainty. This method exists only
    /// behind the opt-in `test-support` feature and cannot mint authority or
    /// alter the bytes selected by Core.
    #[cfg(feature = "test-support")]
    pub fn inject_next_append_post_sync_uncertainty_for_test_support(&mut self) {
        self.session
            .writer
            .inject_faults([AppendFault::ClearMarkerDirectorySync]);
    }

    /// Consumes the next Core-issued suffix element (payment then UI-event).
    pub fn publish_next_gluing_input(
        &mut self,
    ) -> Result<Option<GluingInputPublicationV4>, JournalError> {
        if self.remaining_inputs.is_empty() {
            return Ok(None);
        }
        let input = self.remaining_inputs.remove(0);
        let descriptor = GluingInputDescriptorV4::from_json_bytes(input.descriptor_bytes())
            .map_err(|error| {
                JournalError::Domain(reviewgraphen_core::DomainError::Validation(
                    error.to_string(),
                ))
            })?;
        let source = input.into_trusted_source();
        self.session
            .publish_gluing_input(source, descriptor, self.basis)
            .map(Some)
    }

    /// Seals and appends the atomic bundle only after the complete suffix was
    /// consumed inside this same lock-held callback.
    pub fn append_gluing_bundle(&mut self) -> Result<V4GluingBundleAppendReceipt, JournalError> {
        if !self.remaining_inputs.is_empty() {
            return Err(JournalError::Identity(
                "M5 bundle requires all profile inputs to be published",
            ));
        }
        let bundle = self.session.mint_gluing_bundle(self.basis)?;
        self.session.append_gluing_bundle(bundle, self.basis)
    }
}

enum LockedM5ProfileSessionResult<T> {
    Continued(T),
    AlreadyComplete {
        completed: Box<M5CompletedGluingProfileV4>,
        roots: AuthorityTrustRootsV4,
    },
}

fn run_locked_m5_profile_session<'root, T, F>(
    root: &'root StoreRoot,
    root_lock: V4RootLock,
    run_lock: OwnedFd,
    writer: JournalWriter,
    base_roots: AuthorityTrustRootsV4,
    assignments: M5DoubleSubmitAssignmentsV4,
    operation: F,
) -> Result<LockedM5ProfileSessionResult<T>, JournalError>
where
    F: for<'session, 'roots> FnOnce(
        &mut M5GluingProfileSessionV4<'session, 'root, 'roots>,
    ) -> Result<T, JournalError>,
{
    let JournalGenesis::V4Shared(genesis) = &writer.identity.genesis else {
        return Err(JournalError::Identity(
            "M5 profile inspection requires verified genesis bytes",
        ));
    };
    let resolver = JournalAuthorityResolverV4 {
        reader: CasReader::open_existing(root)?,
    };
    let inspection = EventLogV4::inspect_m5_gluing_profile_v4(
        writer.identity.run_id.clone(),
        genesis,
        &writer.state.events,
        &resolver,
        base_roots,
        EventReplayLimits::new(writer.limits.max_events, writer.limits.max_replay_bytes),
        assignments,
    )?;
    let source_ids = inspection.source_ids().clone();
    let (augmented_roots, remaining_inputs, completed) = inspection.into_recovery_parts();
    if let Some(completed) = completed {
        return Ok(LockedM5ProfileSessionResult::AlreadyComplete {
            completed: Box::new(completed),
            roots: augmented_roots,
        });
    }
    let session_identity = OpaqueSessionIdentityV4::fresh();
    let (log, mut basis) = EventLogV4::replay_confirmed_v4_prefix_for_session(
        writer.identity.run_id.clone(),
        genesis,
        &writer.state.events,
        &resolver,
        &augmented_roots,
        EventReplayLimits::new(writer.limits.max_events, writer.limits.max_replay_bytes),
        &session_identity,
    )?;
    let index_projection = index_v5_projection_charge_from_log(
        &log,
        writer.limits.max_replay_bytes,
        writer.state.confirmed_offset,
    )?;
    let index_genesis = decode_index_v5_genesis(genesis)?;
    let mut session = ReplayedV4RunSession {
        _root_lock: root_lock,
        _run_lock: run_lock,
        writer,
        log,
        index_genesis,
        resolver,
        roots: &augmented_roots,
        session_identity,
        state: ReplayedV4RunSessionState::Healthy,
        index_projection: Some(index_projection),
    };
    let mut profile = M5GluingProfileSessionV4 {
        session: &mut session,
        basis: &mut basis,
        remaining_inputs,
        source_ids,
    };
    operation(&mut profile).map(LockedM5ProfileSessionResult::Continued)
}

/// Authority-replayed historical prefix used only to validate a disposable
/// derived-index image before it may be classified as stale.
pub(crate) struct IndexV4ReplayedPrefix {
    log: EventLog,
    basis: AuthorityReplayBasisV3,
    confirmed_offset: u64,
}

/// Roots-bound historical V4 prefix exposed only to the disposable v5 index
/// projector. The typed genesis is descriptive input, while `log`/`basis`
/// remain the Core replay authority used to validate every projected row.
pub(crate) struct IndexV5ReplayedPrefix {
    log: EventLogV4,
    basis: AuthorityReplayBasisV4,
    index_genesis: RunGenesisSnapshot,
    confirmed_offset: u64,
    projection: crate::index::ReplayProjectionChargeV5,
}

fn index_v5_projection_charge_from_log(
    log: &EventLogV4,
    max_replay_bytes: u64,
    expected_confirmed_offset: u64,
) -> Result<crate::index::ReplayProjectionChargeV5, JournalError> {
    let mut projection = crate::index::ReplayProjectionChargeV5::default();
    let mut projection_error = None;
    let visitor: &mut dyn for<'event> FnMut(
        reviewgraphen_core::BorrowedV4EventMetadata<'event>,
        reviewgraphen_core::BorrowedProjectionPayloadV4<'event>,
    ) = &mut |metadata: reviewgraphen_core::BorrowedV4EventMetadata<'_>,
              payload: reviewgraphen_core::BorrowedProjectionPayloadV4<'_>| {
        if projection_error.is_none()
            && let Err(error) = projection.observe(metadata, payload)
        {
            projection_error = Some(error);
        }
    };
    log.visit_confirmed_projection_v4(visitor)
        .map_err(JournalError::Domain)?;
    if projection_error.is_some() {
        return Err(JournalError::Incomplete {
            limit: max_replay_bytes,
            observed: u64::MAX,
        });
    }
    if projection.confirmed_offset != expected_confirmed_offset {
        return Err(JournalError::Identity(
            "V5 projection offset does not match the recovered journal",
        ));
    }
    Ok(projection)
}

impl IndexV4ReplayedPrefix {
    pub(crate) fn initial(&self) -> &reviewgraphen_core::ReviewAggregate {
        self.log.initial()
    }

    pub(crate) fn current(&self) -> &reviewgraphen_core::ReviewAggregate {
        self.log.aggregate()
    }

    pub(crate) fn envelopes(
        &self,
    ) -> impl ExactSizeIterator<Item = &reviewgraphen_core::EventEnvelope> {
        self.log.envelopes()
    }

    pub(crate) fn claim_assessments(
        &self,
    ) -> impl Iterator<Item = &reviewgraphen_core::ClaimAssessmentV3> {
        self.log.claim_assessments_v3()
    }

    pub(crate) const fn confirmed_offset(&self) -> u64 {
        self.confirmed_offset
    }

    pub(crate) const fn basis(&self) -> &AuthorityReplayBasisV3 {
        &self.basis
    }
}

impl IndexV5ReplayedPrefix {
    pub(crate) const fn genesis(&self) -> &RunGenesisSnapshot {
        &self.index_genesis
    }

    pub(crate) fn obligation_lifecycle(
        &self,
        obligation_id: &StableId,
    ) -> Option<ObligationLifecycle> {
        self.log.obligation_lifecycle_v4(obligation_id)
    }

    pub(crate) fn for_each_claim_assessment(
        &self,
        visitor: &mut dyn FnMut(reviewgraphen_core::BorrowedClaimAssessmentProjectionV4<'_>),
    ) {
        for assessment in self.log.claim_assessments_v3() {
            visitor(assessment);
        }
    }

    pub(crate) const fn confirmed_offset(&self) -> u64 {
        self.confirmed_offset
    }

    pub(crate) const fn basis(&self) -> &AuthorityReplayBasisV4 {
        &self.basis
    }

    pub(crate) const fn projection(&self) -> &crate::index::ReplayProjectionChargeV5 {
        &self.projection
    }

    pub(crate) fn visit_projection(
        &self,
        visitor: &mut dyn for<'event> FnMut(
            reviewgraphen_core::BorrowedV4EventMetadata<'event>,
            reviewgraphen_core::BorrowedProjectionPayloadV4<'event>,
        ),
    ) -> Result<(), JournalError> {
        self.log
            .visit_confirmed_projection_v4(visitor)
            .map_err(JournalError::Domain)
    }
}

/// A lock-held V3 session recovered at a partially durable verification
/// bundle. It deliberately exposes no read, mint, registration, decision,
/// finding, or ordinary append surface: the only legal transition is the
/// exact sealed resume operation.
pub struct RecoveredVerificationBundleV3Session<'root, 'roots> {
    session: ReplayedV3RunSession<'root, 'roots>,
    marker: VerificationBundlePendingMarkerV3,
    confirmed_events: usize,
}

/// Resume-only V4 session returned solely for a strict-interior M4 bundle.
/// It exposes no ordinary append or read API; consuming the exact Core
/// authority is its only transition back to an editable session.
pub struct RecoveredM4BundleV4Session<'root, 'roots> {
    root_lock: V4RootLock,
    run_lock: OwnedFd,
    writer: JournalWriter,
    resolver: JournalAuthorityResolverV4<'root>,
    roots: &'roots AuthorityTrustRootsV4,
    session_identity: OpaqueSessionIdentityV4,
    core_session: CoreRecoveredM4BundleV4Session,
    marker: VerificationBundlePendingMarkerV3,
    confirmed_events: usize,
}

// Keep both closed recovery branches inline: the strict-interior branch
// exposes neither an editable log nor a replay basis until its sealed suffix
// has been durably confirmed.
#[allow(clippy::large_enum_variant)]
pub enum RecoveredV4Session<'root, 'roots> {
    Editable {
        session: ReplayedV4RunSession<'root, 'roots>,
        basis: AuthorityReplayBasisV4,
    },
    M4BundleResumeRequired {
        session: RecoveredM4BundleV4Session<'root, 'roots>,
        resume_authority: VerificationBundleResumeAuthorityV4,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplayedV3RunSessionState {
    Healthy,
    ResumeOnly,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Later V4 append units consume the uncertainty state.
enum ReplayedV4RunSessionState {
    Healthy,
    Uncertain,
}

/// Durable receipt for one atomic, possibly multi-event verification bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct V3VerificationBundleAppendReceipt {
    authority: VerificationBundleReceiptV3,
    journal: Vec<JournalAppendReceipt>,
}

pub struct V4VerificationBundleAppendReceipt {
    authority: VerificationBundleReceiptV4,
    journal: Vec<JournalAppendReceipt>,
}

/// Store durability plus Core replay confirmation for one exact V4 event.
/// The Core receipt is descriptive; neither field grants another append.
pub struct V4EventAppendReceipt<T> {
    core: T,
    journal: JournalAppendReceipt,
}

impl<T> V4EventAppendReceipt<T> {
    #[must_use]
    pub fn core(&self) -> &T {
        &self.core
    }

    #[must_use]
    pub fn journal(&self) -> &JournalAppendReceipt {
        &self.journal
    }
}

/// Exact result of publishing or adopting one canonical M5 input. An
/// already-confirmed pair is reconstructed from replay and never appended a
/// second time.
pub enum GluingInputPublicationV4 {
    Confirmed {
        cas: CasReceipt,
        append: V4EventAppendReceipt<ArtifactRegistrationReceiptV4>,
    },
    AlreadyRegistered {
        context_id: StableId,
        descriptor_id: StableId,
        registration_id: StableId,
    },
}

pub type V4GluingBundleAppendReceipt = V4EventAppendReceipt<GluingBundleReceiptV4>;

impl V4VerificationBundleAppendReceipt {
    #[must_use]
    pub fn authority(&self) -> &VerificationBundleReceiptV4 {
        &self.authority
    }

    #[must_use]
    pub fn journal(&self) -> &[JournalAppendReceipt] {
        &self.journal
    }
}

impl V3VerificationBundleAppendReceipt {
    #[must_use]
    pub fn authority(&self) -> &VerificationBundleReceiptV3 {
        &self.authority
    }

    #[must_use]
    pub fn journal(&self) -> &[JournalAppendReceipt] {
        &self.journal
    }
}

struct JournalAuthorityResolverV3<'a> {
    reader: CasReader<'a>,
}

impl AuthorityArtifactResolverV3 for JournalAuthorityResolverV3<'_> {
    fn read_exact(
        &self,
        cas_hash: &ContentHash,
        destination: &mut [u8],
    ) -> reviewgraphen_core::Result<()> {
        let hash = CasHash::parse(cas_hash.as_str().to_owned()).map_err(|error| {
            reviewgraphen_core::DomainError::Validation(format!(
                "authority CAS hash is not admissible: {error}"
            ))
        })?;
        self.reader
            .read_exact_slice(&hash, destination)
            .map_err(|error| {
                reviewgraphen_core::DomainError::Validation(format!(
                    "authority CAS resolution failed: {error}"
                ))
            })
    }
}

struct JournalAuthorityResolverV4<'a> {
    reader: CasReader<'a>,
}

impl AuthorityArtifactResolverV4 for JournalAuthorityResolverV4<'_> {
    fn read_exact(
        &self,
        cas_hash: &ContentHash,
        destination: &mut [u8],
    ) -> reviewgraphen_core::Result<()> {
        let hash = CasHash::parse(cas_hash.as_str().to_owned()).map_err(|error| {
            reviewgraphen_core::DomainError::Validation(format!(
                "event-v4 authority CAS hash is not admissible: {error}"
            ))
        })?;
        self.reader
            .read_exact_slice(&hash, destination)
            .map_err(|error| {
                reviewgraphen_core::DomainError::Validation(format!(
                    "event-v4 authority CAS resolution failed: {error}"
                ))
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplayedV2RunSessionState {
    Healthy,
    Uncertain,
}

impl ReplayedV2RunSession {
    pub fn run_id(&self) -> Result<&StableId, JournalError> {
        self.require_healthy()?;
        Ok(self.log.run_id())
    }
    pub fn aggregate(&self) -> Result<&reviewgraphen_core::ReviewAggregate, JournalError> {
        self.require_healthy()?;
        Ok(self.log.aggregate())
    }
    pub fn tail_hash(&self) -> Result<&ContentHash, JournalError> {
        self.require_healthy()?;
        Ok(self.log.tail_hash())
    }
    pub fn event_count(&self) -> Result<usize, JournalError> {
        self.require_healthy()?;
        Ok(self.log.events().len())
    }
    /// Opaque identity of the admitted store root that owns this session.
    pub fn store_root_identity(&self) -> Result<&StoreRootIdentity, JournalError> {
        self.require_healthy()?;
        Ok(&self.store_root_identity)
    }
    /// Tests whether a caller-supplied root is the exact admitted directory
    /// descriptor identity for this session.
    pub fn matches_store_root(&self, root: &StoreRoot) -> Result<bool, JournalError> {
        self.require_healthy()?;
        Ok(self.store_root_identity == *root.identity())
    }

    /// Stages core validation first, appends the exact staged envelope while
    /// the journal lock remains held, then commits the staged in-memory log.
    pub fn append_command(
        &mut self,
        command: EventCommand,
    ) -> Result<JournalAppendReceipt, JournalError> {
        self.require_healthy()?;
        let mut staged = self.log.clone();
        staged.append(command)?;
        let envelope = staged
            .events()
            .last()
            .ok_or(JournalError::Identity("staged command produced no event"))?
            .envelope()
            .clone();
        let receipt = match self.writer.append(envelope) {
            Ok(receipt) => receipt,
            Err(error) if self.writer.append_durability == AppendDurability::Uncertain => {
                let _ = error;
                self.state = ReplayedV2RunSessionState::Uncertain;
                return Err(JournalError::SessionUncertain);
            }
            Err(error) => return Err(error),
        };
        self.log = staged;
        Ok(receipt)
    }

    fn require_healthy(&self) -> Result<(), JournalError> {
        match self.state {
            ReplayedV2RunSessionState::Healthy => Ok(()),
            ReplayedV2RunSessionState::Uncertain => Err(JournalError::SessionUncertain),
        }
    }
}

impl ReplayedV3RunSession<'_, '_> {
    pub fn run_id(&self) -> Result<&StableId, JournalError> {
        self.require_healthy()?;
        Ok(self.log.run_id())
    }

    pub fn aggregate(&self) -> Result<&reviewgraphen_core::ReviewAggregate, JournalError> {
        self.require_healthy()?;
        Ok(self.log.aggregate())
    }

    pub fn tail_hash(&self) -> Result<&ContentHash, JournalError> {
        self.require_healthy()?;
        Ok(self.log.tail_hash())
    }

    pub fn event_count(&self) -> Result<usize, JournalError> {
        self.require_healthy()?;
        Ok(self.log.events().len())
    }

    pub fn store_root_identity(&self) -> Result<&StoreRootIdentity, JournalError> {
        self.require_healthy()?;
        Ok(&self.store_root_identity)
    }

    pub fn matches_store_root(&self, root: &StoreRoot) -> Result<bool, JournalError> {
        self.require_healthy()?;
        Ok(self.store_root_identity == *root.identity())
    }

    pub fn claim_assessment(
        &self,
        claim_id: &StableId,
    ) -> Result<Option<&reviewgraphen_core::ClaimAssessmentV3>, JournalError> {
        self.require_healthy()?;
        Ok(self.log.claim_assessment_v3(claim_id))
    }

    pub fn evidence_count(&self) -> Result<usize, JournalError> {
        self.require_healthy()?;
        Ok(self.log.evidence_v3().count())
    }

    pub fn evidence_binding_count(&self) -> Result<usize, JournalError> {
        self.require_healthy()?;
        Ok(self.log.evidence_bindings_v3().count())
    }

    pub fn verification_count(&self) -> Result<usize, JournalError> {
        self.require_healthy()?;
        Ok(self.log.verifications_v3().count())
    }

    /// Read-only, crate-internal source for the authority-bound index-v4
    /// projection.  Keeping these borrows on the replay session ensures the
    /// index cannot accidentally combine an authority-verified aggregate
    /// with envelopes read from a later journal prefix.
    pub(crate) fn index_v4_initial(
        &self,
    ) -> Result<&reviewgraphen_core::ReviewAggregate, JournalError> {
        self.require_healthy()?;
        Ok(self.log.initial())
    }

    pub(crate) fn index_v4_current(
        &self,
    ) -> Result<&reviewgraphen_core::ReviewAggregate, JournalError> {
        self.require_healthy()?;
        Ok(self.log.aggregate())
    }

    pub(crate) fn index_v4_envelopes(
        &self,
    ) -> Result<impl ExactSizeIterator<Item = &reviewgraphen_core::EventEnvelope>, JournalError>
    {
        self.require_healthy()?;
        Ok(self.log.envelopes())
    }

    pub(crate) fn index_v4_claim_assessments(
        &self,
    ) -> Result<impl Iterator<Item = &reviewgraphen_core::ClaimAssessmentV3>, JournalError> {
        self.require_healthy()?;
        Ok(self.log.claim_assessments_v3())
    }

    pub(crate) fn index_v4_confirmed_offset(&self) -> Result<u64, JournalError> {
        self.require_healthy()?;
        Ok(self.writer.state.confirmed_offset)
    }

    pub(crate) fn index_v4_replay_prefix(
        &self,
        event_count: u64,
        confirmed_offset: u64,
    ) -> Result<IndexV4ReplayedPrefix, JournalError> {
        self.require_healthy()?;
        let count = usize::try_from(event_count).map_err(|_| JournalError::Incomplete {
            limit: self.writer.limits.max_events,
            observed: event_count,
        })?;
        let prefix = self
            .writer
            .state
            .events
            .get(..count)
            .ok_or(JournalError::Identity(
                "index prefix event count exceeds journal",
            ))?;
        let mut observed_offset = 0_u64;
        for envelope in prefix {
            let line = canonical_json(envelope)?;
            observed_offset = observed_offset
                .checked_add(
                    u64::try_from(line.len())
                        .map_err(|_| JournalError::Incomplete {
                            limit: self.writer.limits.max_replay_bytes,
                            observed: u64::MAX,
                        })?
                        .checked_add(1)
                        .ok_or(JournalError::Incomplete {
                            limit: self.writer.limits.max_replay_bytes,
                            observed: u64::MAX,
                        })?,
                )
                .ok_or(JournalError::Incomplete {
                    limit: self.writer.limits.max_replay_bytes,
                    observed: u64::MAX,
                })?;
        }
        if observed_offset != confirmed_offset {
            return Err(JournalError::Identity(
                "index prefix offset does not match canonical journal prefix",
            ));
        }
        let JournalGenesis::V3Shared(genesis) = &self.writer.identity.genesis else {
            return Err(JournalError::Identity(
                "V3 session lost its verified genesis",
            ));
        };
        let (log, basis) = EventLog::replay_validated_v3_prefix(
            self.writer.identity.run_id.clone(),
            genesis,
            prefix,
            &self.resolver,
            self.roots,
            EventReplayLimits::new(
                self.writer.limits.max_events,
                self.writer.limits.max_replay_bytes,
            ),
        )?;
        Ok(IndexV4ReplayedPrefix {
            log,
            basis,
            confirmed_offset,
        })
    }

    /// Revalidates that an operation basis is the exact roots/CAS-bound
    /// certificate for the session's current durable prefix.  It performs no
    /// journal or CAS mutation and never accepts a caller-composed tuple.
    pub fn validate_v3_operation_basis(
        &self,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<(), JournalError> {
        self.require_healthy()?;
        let (_, replayed) = self.replay_candidate()?;
        if !same_authority_basis(basis, &replayed) {
            return Err(reviewgraphen_core::DomainError::AuthorityReplayBasisMismatch.into());
        }
        Ok(())
    }

    /// Descriptor-relative, read-only classification of one exact CAS tuple.
    /// `false` means no object exists; every malformed, wrong-size, or
    /// wrong-hash object is a typed refusal rather than an approximate miss.
    pub fn cas_contains_exact_v3(
        &self,
        hash: &ContentHash,
        size: u64,
    ) -> Result<bool, JournalError> {
        self.require_healthy()?;
        if size > self.resolver.reader.root.limits().max_object_bytes {
            return Err(JournalError::Incomplete {
                limit: self.resolver.reader.root.limits().max_object_bytes,
                observed: size,
            });
        }
        let length = usize::try_from(size).map_err(|_| JournalError::Incomplete {
            limit: self.resolver.reader.root.limits().max_object_bytes,
            observed: size,
        })?;
        let cas_hash = CasHash::parse(hash.to_string())?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| JournalError::Incomplete {
                limit: size,
                observed: size,
            })?;
        bytes.resize(length, 0);
        match self.resolver.reader.read_exact_slice(&cas_hash, &mut bytes) {
            Ok(()) => Ok(true),
            Err(StoreError::MissingArtifact) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    pub fn cas_contains_exact(&self, hash: &ContentHash, size: u64) -> Result<bool, JournalError> {
        self.cas_contains_exact_v3(hash, size)
    }

    /// Durably records one snapshot-source projection in a V3 stream.  This
    /// intentionally exposes no generic command or authority payload seam.
    pub fn append_snapshot_sources_v3(
        &mut self,
        sources: SnapshotSourcesRecorded,
        basis: &mut AuthorityReplayBasisV3,
    ) -> Result<JournalAppendReceipt, JournalError> {
        self.append_nonauthority_v3(EventCommand::snapshot_sources_recorded(sources), basis)
    }

    /// Durably applies one ordinary obligation lifecycle transition.
    pub fn append_obligation_transition_v3(
        &mut self,
        obligation_id: StableId,
        next: ObligationLifecycle,
        basis: &mut AuthorityReplayBasisV3,
    ) -> Result<JournalAppendReceipt, JournalError> {
        self.append_nonauthority_v3(
            EventCommand::obligation_transition(obligation_id, next),
            basis,
        )
    }

    /// Durably records one deterministic D2 review plan in a V3 stream.
    pub fn append_review_plan_v3(
        &mut self,
        plan: ReviewPlan,
        basis: &mut AuthorityReplayBasisV3,
    ) -> Result<JournalAppendReceipt, JournalError> {
        self.append_nonauthority_v3(EventCommand::review_plan_recorded(plan), basis)
    }

    /// Durably records a context projection together with its private,
    /// byte-reverified core admission.
    pub fn append_context_projection_v3(
        &mut self,
        projection: BuiltContextProjection,
        basis: &mut AuthorityReplayBasisV3,
    ) -> Result<JournalAppendReceipt, JournalError> {
        self.append_nonauthority_v3(EventCommand::context_envelope_projected(projection), basis)
    }

    /// Durably records a non-authority V3 registration.  Core rejects the
    /// verifier/external-witness source variants at this ordinary append seam.
    pub fn append_nonauthority_registration_v3(
        &mut self,
        registration: ArtifactRegisteredV3,
        basis: &mut AuthorityReplayBasisV3,
    ) -> Result<JournalAppendReceipt, JournalError> {
        self.append_nonauthority_v3(EventCommand::artifact_registered_v3(registration), basis)
    }

    /// Durably records one validated D2 execution and all of its claims as a
    /// single event.  The bundle carries the private raw-reviewer closure.
    pub fn append_review_execution_v3(
        &mut self,
        bundle: ValidatedExecutionBundle,
        basis: &mut AuthorityReplayBasisV3,
    ) -> Result<JournalAppendReceipt, JournalError> {
        self.append_nonauthority_v3(EventCommand::review_execution_recorded(bundle), basis)
    }

    fn append_nonauthority_v3(
        &mut self,
        command: EventCommand,
        basis: &mut AuthorityReplayBasisV3,
    ) -> Result<JournalAppendReceipt, JournalError> {
        self.require_healthy()?;
        let (mut candidate, _) = self.checked_candidate(basis)?;
        let first = candidate.events().len();
        candidate.append(command)?;
        let next_basis = self.replay_candidate_log(&candidate)?.1;
        let mut receipts = self.commit_candidate(candidate, next_basis, first, basis)?;
        receipts.pop().ok_or(JournalError::Identity(
            "non-authority V3 append produced no event",
        ))
    }

    fn replay_candidate_log(
        &self,
        candidate: &EventLog,
    ) -> Result<(EventLog, AuthorityReplayBasisV3), JournalError> {
        let JournalGenesis::V3Shared(genesis) = &self.writer.identity.genesis else {
            return Err(JournalError::Identity(
                "V3 session lost its verified genesis",
            ));
        };
        let envelopes = candidate
            .events()
            .iter()
            .map(|event| event.envelope().clone())
            .collect::<Vec<_>>();
        Ok(EventLog::replay_validated_v3_prefix(
            self.writer.identity.run_id.clone(),
            genesis,
            &envelopes,
            &self.resolver,
            self.roots,
            EventReplayLimits::new(
                self.writer.limits.max_events,
                self.writer.limits.max_replay_bytes,
            ),
        )?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare_static_verifier_artifact_registration(
        &self,
        claim_id: StableId,
        role: VerifierArtifactRoleV3,
        cas_hash: ContentHash,
        size: u64,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<ValidatedArtifactRegistrationV3, JournalError> {
        self.require_healthy()?;
        Ok(self.log.prepare_static_verifier_artifact_registration_v3(
            claim_id,
            role,
            cas_hash,
            size,
            &self.resolver,
            self.roots,
            basis,
        )?)
    }

    pub fn append_authority_registration(
        &mut self,
        validated: ValidatedArtifactRegistrationV3,
        basis: &mut AuthorityReplayBasisV3,
    ) -> Result<JournalAppendReceipt, JournalError> {
        self.require_healthy()?;
        let (mut candidate, mut next_basis) = self.checked_candidate(basis)?;
        let first = candidate.events().len();
        candidate.append_authority_registration_v3(validated, &mut next_basis)?;
        let mut receipts = self.commit_candidate(candidate, next_basis, first, basis)?;
        receipts
            .pop()
            .ok_or(JournalError::Identity("authority append produced no event"))
    }

    pub fn execute_fixture_harness(
        &self,
        claim_id: &StableId,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<FixtureExecutionReceiptV1, JournalError> {
        self.require_healthy()?;
        Ok(self
            .log
            .execute_fixture_harness_v1(claim_id, self.roots, basis)?)
    }

    /// Seals a fresh caller-executed static evaluation against the exact
    /// claim, program and current durable-prefix basis held by Core.
    pub fn seal_static_verification_attempt_v3(
        &self,
        claim_id: &StableId,
        evaluation: &StaticFactEvaluationV1,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<ExpectedVerificationAttemptV3, JournalError> {
        self.require_healthy()?;
        Ok(self
            .log
            .seal_static_verification_attempt_v3(claim_id, evaluation, basis)?)
    }

    /// Inspects durable static-verifier state without invoking the evaluator.
    pub fn inspect_static_verification_attempt_v3(
        &self,
        claim_id: &StableId,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<StaticVerificationAttemptInspectionV3, JournalError> {
        self.require_healthy()?;
        self.validate_v3_operation_basis(basis)?;
        Ok(self.log.inspect_static_verification_attempt_v3(
            claim_id,
            &self.resolver,
            self.roots,
            basis,
        )?)
    }

    /// Builds Core's opaque description of the one admitted fixed-fixture
    /// attempt. The returned value carries no harness execution authority.
    pub fn expect_fixture_verification_attempt_v3(
        &self,
        claim_id: &StableId,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<ExpectedVerificationAttemptV3, JournalError> {
        self.require_healthy()?;
        Ok(self
            .log
            .expect_fixture_verification_attempt_v3(claim_id, self.roots, basis)?)
    }

    /// Classifies the exact durable retry stage and rejects CAS objects that
    /// exist before their corresponding registration becomes authoritative.
    pub fn inspect_m4_verification_attempt_v3(
        &self,
        expected: &ExpectedVerificationAttemptV3,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<VerificationAttemptStageV3, JournalError> {
        self.require_healthy()?;
        self.validate_v3_operation_basis(basis)?;
        let stage = self.log.inspect_verification_attempt_v3(
            expected,
            &self.resolver,
            self.roots,
            basis,
        )?;
        if stage == VerificationAttemptStageV3::Ready
            && self.cas_contains_exact_v3(expected.input_hash(), expected.input_size())?
        {
            return Err(JournalError::OrphanCasObjectV3 {
                hash: expected.input_hash().clone(),
            });
        }
        if matches!(
            stage,
            VerificationAttemptStageV3::Ready
                | VerificationAttemptStageV3::InputRegistered
                | VerificationAttemptStageV3::WitnessRegistered
        ) && self.cas_contains_exact_v3(expected.output_hash(), expected.output_size())?
        {
            return Err(JournalError::OrphanCasObjectV3 {
                hash: expected.output_hash().clone(),
            });
        }
        Ok(stage)
    }

    /// Recovers the narrowly scoped, non-executable fixture authority from
    /// an exact durable registration prefix.
    pub fn recover_fixture_registration_resume_authority(
        &self,
        claim_id: &StableId,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<FixtureRegistrationResumeAuthorityV3, JournalError> {
        self.require_healthy()?;
        Ok(self.log.recover_fixture_registration_resume_authority_v3(
            claim_id,
            &self.resolver,
            self.roots,
            basis,
        )?)
    }

    pub fn prepare_fixture_output_from_resume(
        &self,
        authority: &mut FixtureRegistrationResumeAuthorityV3,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<ValidatedArtifactRegistrationV3, JournalError> {
        self.require_healthy()?;
        Ok(self
            .log
            .prepare_fixture_output_from_resume_v3(authority, &self.resolver, basis)?)
    }

    pub fn mint_fixture_verification_bundle_from_resume(
        &self,
        authority: FixtureRegistrationResumeAuthorityV3,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<ValidatedVerificationBundleV3, JournalError> {
        self.require_healthy()?;
        Ok(self.log.mint_fixture_verification_bundle_from_resume_v3(
            authority,
            &self.resolver,
            basis,
        )?)
    }

    pub fn prepare_external_fixture_witness_registration(
        &self,
        receipt: &mut FixtureExecutionReceiptV1,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<ValidatedArtifactRegistrationV3, JournalError> {
        self.require_healthy()?;
        Ok(self.log.prepare_external_fixture_witness_registration_v3(
            receipt,
            &self.resolver,
            basis,
        )?)
    }

    pub fn prepare_fixture_verifier_output_registration(
        &self,
        receipt: &mut FixtureExecutionReceiptV1,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<ValidatedArtifactRegistrationV3, JournalError> {
        self.require_healthy()?;
        Ok(self.log.prepare_fixture_verifier_output_registration_v3(
            receipt,
            &self.resolver,
            basis,
        )?)
    }

    pub fn admit_external_fixture_witness(
        &self,
        receipt: &mut FixtureExecutionReceiptV1,
        witness_registration_id: &StableId,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<ExternalWitnessAdmissionV3, JournalError> {
        self.require_healthy()?;
        Ok(self.log.admit_external_witness_v3(
            receipt,
            witness_registration_id,
            &self.resolver,
            basis,
        )?)
    }

    pub fn mint_fixture_verification_bundle(
        &self,
        admission: ExternalWitnessAdmissionV3,
        output_registration_id: &StableId,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<ValidatedVerificationBundleV3, JournalError> {
        self.require_healthy()?;
        Ok(self.log.mint_fixture_verification_bundle_v3(
            admission,
            output_registration_id,
            &self.resolver,
            basis,
        )?)
    }

    pub fn mint_static_verification_bundle(
        &self,
        claim_id: &StableId,
        input_registration_id: &StableId,
        output_registration_id: &StableId,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<ValidatedVerificationBundleV3, JournalError> {
        self.require_healthy()?;
        Ok(self.log.mint_static_verification_bundle_v3(
            claim_id,
            input_registration_id,
            output_registration_id,
            &self.resolver,
            self.roots,
            basis,
        )?)
    }

    pub fn append_verification_bundle(
        &mut self,
        bundle: ValidatedVerificationBundleV3,
        basis: &mut AuthorityReplayBasisV3,
    ) -> Result<V3VerificationBundleAppendReceipt, JournalError> {
        self.require_healthy()?;
        let (mut candidate, mut next_basis) = self.checked_candidate(basis)?;
        let first = candidate.events().len();
        let authority = candidate.append_verification_bundle_v3(bundle, &mut next_basis)?;
        let suffix = candidate.events()[first..]
            .iter()
            .map(|event| event.envelope().clone())
            .collect::<Vec<_>>();
        let journal = match self.writer.append_verification_bundle_suffix(&suffix) {
            Ok(receipts) => receipts,
            Err(error @ JournalError::BundleAppendInterrupted { .. }) => {
                self.state = ReplayedV3RunSessionState::ResumeOnly;
                return Err(error);
            }
            Err(_) if self.writer.append_durability == AppendDurability::Uncertain => {
                self.state = ReplayedV3RunSessionState::Uncertain;
                return Err(JournalError::SessionUncertain);
            }
            Err(error) => return Err(error),
        };
        self.log = candidate;
        *basis = next_basis;
        Ok(V3VerificationBundleAppendReceipt { authority, journal })
    }

    pub fn mint_decision(
        &self,
        claim_id: &StableId,
        input: DecisionInputV3,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<ValidatedDecisionV3, JournalError> {
        self.require_healthy()?;
        Ok(self
            .log
            .mint_decision_v3(claim_id, input, self.roots, basis)?)
    }

    pub fn append_decision(
        &mut self,
        validated: ValidatedDecisionV3,
        basis: &mut AuthorityReplayBasisV3,
    ) -> Result<JournalAppendReceipt, JournalError> {
        self.require_healthy()?;
        let (mut candidate, mut next_basis) = self.checked_candidate(basis)?;
        let first = candidate.events().len();
        candidate.append_decision_v3(validated, &mut next_basis)?;
        let mut receipts = self.commit_candidate(candidate, next_basis, first, basis)?;
        receipts
            .pop()
            .ok_or(JournalError::Identity("decision append produced no event"))
    }

    pub fn mint_finding(
        &self,
        claim_id: &StableId,
        projection_descriptor_id: &str,
        basis: &AuthorityReplayBasisV3,
    ) -> Result<ValidatedFindingV3, JournalError> {
        self.require_healthy()?;
        Ok(self
            .log
            .mint_finding_v3(claim_id, projection_descriptor_id, basis)?)
    }

    pub fn append_finding(
        &mut self,
        validated: ValidatedFindingV3,
        basis: &mut AuthorityReplayBasisV3,
    ) -> Result<JournalAppendReceipt, JournalError> {
        self.require_healthy()?;
        let (mut candidate, mut next_basis) = self.checked_candidate(basis)?;
        let first = candidate.events().len();
        candidate.append_finding_v3(validated, &mut next_basis)?;
        let mut receipts = self.commit_candidate(candidate, next_basis, first, basis)?;
        receipts
            .pop()
            .ok_or(JournalError::Identity("finding append produced no event"))
    }

    fn checked_candidate(
        &self,
        supplied: &AuthorityReplayBasisV3,
    ) -> Result<(EventLog, AuthorityReplayBasisV3), JournalError> {
        let (candidate, replayed) = self.replay_candidate()?;
        if !same_authority_basis(supplied, &replayed) {
            return Err(reviewgraphen_core::DomainError::AuthorityReplayBasisMismatch.into());
        }
        Ok((candidate, replayed))
    }

    fn replay_candidate(&self) -> Result<(EventLog, AuthorityReplayBasisV3), JournalError> {
        let JournalGenesis::V3Shared(genesis) = &self.writer.identity.genesis else {
            return Err(JournalError::Identity(
                "V3 session lost its verified genesis",
            ));
        };
        Ok(EventLog::replay_validated_v3_prefix(
            self.writer.identity.run_id.clone(),
            genesis,
            &self.writer.state.events,
            &self.resolver,
            self.roots,
            EventReplayLimits::new(
                self.writer.limits.max_events,
                self.writer.limits.max_replay_bytes,
            ),
        )?)
    }

    fn commit_candidate(
        &mut self,
        candidate: EventLog,
        next_basis: AuthorityReplayBasisV3,
        first: usize,
        supplied: &mut AuthorityReplayBasisV3,
    ) -> Result<Vec<JournalAppendReceipt>, JournalError> {
        self.writer.refuse_bundle_resume_gate()?;
        let suffix = candidate.events()[first..]
            .iter()
            .map(|event| event.envelope().clone())
            .collect::<Vec<_>>();
        let receipts = match self.writer.append_batch(&suffix) {
            Ok(receipts) => receipts,
            Err(_) if self.writer.append_durability == AppendDurability::Uncertain => {
                self.state = ReplayedV3RunSessionState::Uncertain;
                return Err(JournalError::SessionUncertain);
            }
            Err(error) => return Err(error),
        };
        self.log = candidate;
        *supplied = next_basis;
        Ok(receipts)
    }

    fn require_healthy(&self) -> Result<(), JournalError> {
        match self.state {
            ReplayedV3RunSessionState::Healthy => self.writer.refuse_bundle_resume_gate(),
            ReplayedV3RunSessionState::ResumeOnly => Err(JournalError::SessionResumeRequired),
            ReplayedV3RunSessionState::Uncertain => Err(JournalError::SessionUncertain),
        }
    }
}

impl ReplayedV5RunSession {
    pub(crate) fn log(&self) -> &EventLogV5 {
        &self.log
    }

    pub(crate) fn confirmed_offset(&self) -> u64 {
        self.writer.state.confirmed_offset
    }

    pub(crate) fn store_root_identity(&self) -> &StoreRootIdentity {
        &self.store_root_identity
    }

    pub(crate) fn retain_confirmed_prefix_bytes(&mut self) -> Result<Vec<u8>, JournalError> {
        read_prefix(&mut self.writer.file, self.writer.state.confirmed_offset)
    }

    pub(crate) fn retained_event_bytes(&self) -> Result<u64, JournalError> {
        self.writer
            .state
            .retained_envelope_bytes()?
            .checked_add(
                self.log
                    .retained_envelope_bytes_for_store()
                    .map_err(JournalError::Domain)?,
            )
            .ok_or(JournalError::Incomplete {
                limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                observed: u64::MAX,
            })
    }
}

fn replay_v5_locked(
    root: &StoreRoot,
    run_lock: OwnedFd,
    mut writer: JournalWriter,
) -> Result<ReplayedV5RunSession, JournalError> {
    let JournalGenesis::V5Shared(genesis) = &writer.identity.genesis else {
        return Err(JournalError::Identity(
            "V5 replay requires verified genesis bytes",
        ));
    };
    let genesis_cas = CasHash::parse(writer.identity.genesis_hash().to_string())?;
    CasStore::open(root)?.verify_exact_bytes_streaming(&genesis_cas, genesis)?;
    let envelopes = std::mem::take(&mut writer.state.events);
    let log = EventLogV5::replay_confirmed_prefix_for_store(
        writer.identity.run_id.clone(),
        genesis.to_vec(),
        envelopes,
    )?;
    Ok(ReplayedV5RunSession {
        _run_lock: run_lock,
        writer,
        log,
        store_root_identity: root.identity().clone(),
    })
}

impl<'root, 'roots> RecoveredVerificationBundleV3Session<'root, 'roots> {
    #[must_use]
    pub fn durable_stage(&self) -> VerificationBundleDurableStageV3 {
        VerificationBundleDurableStageV3 {
            confirmed_events: u64::try_from(self.confirmed_events).unwrap_or(u64::MAX),
            expected_events: self.marker.expected_count,
        }
    }

    /// Consumes this resume-only session and the one-shot core authority. A
    /// healthy ordinary session is returned only after the exact missing
    /// suffix and marker cleanup are durably confirmed.
    pub fn resume_verification_bundle(
        mut self,
        authority: VerificationBundleResumeAuthorityV3,
        basis: &mut AuthorityReplayBasisV3,
    ) -> Result<
        (
            ReplayedV3RunSession<'root, 'roots>,
            V3VerificationBundleAppendReceipt,
        ),
        JournalError,
    > {
        let (mut candidate, mut next_basis) = self.session.checked_candidate(basis)?;
        let first = candidate.events().len();
        let authority_receipt = candidate
            .resume_verification_bundle_v3(&mut next_basis, authority)
            .map_err(map_bundle_resume_domain_error)?;
        let suffix = candidate.events()[first..]
            .iter()
            .map(|event| event.envelope().clone())
            .collect::<Vec<_>>();
        let journal = match self.session.writer.resume_verification_bundle_suffix(
            &self.marker,
            &suffix,
            self.confirmed_events,
        ) {
            Ok(receipts) => receipts,
            Err(error @ JournalError::BundleAppendInterrupted { .. }) => return Err(error),
            Err(_) if self.session.writer.append_durability == AppendDurability::Uncertain => {
                return Err(JournalError::SessionUncertain);
            }
            Err(error) => return Err(error),
        };
        self.session.log = candidate;
        *basis = next_basis;
        Ok((
            self.session,
            V3VerificationBundleAppendReceipt {
                authority: authority_receipt,
                journal,
            },
        ))
    }
}

impl<'root, 'roots> ReplayedV4RunSession<'root, 'roots> {
    /// Seals one authority-free inherited D2 command against this exact
    /// lock-held V4 tail. No durable state changes during preparation.
    pub fn prepare_inherited_d2_event(
        &self,
        command: EventCommand,
        basis: &AuthorityReplayBasisV4,
    ) -> Result<PreparedInheritedD2EventV4, JournalError> {
        self.require_healthy()?;
        Ok(self.log.prepare_inherited_d2_event_v4(command, basis)?)
    }

    /// Durably appends one previously sealed D2 event, then rebuilds and
    /// confirms the complete prefix under this same session identity.
    pub fn append_inherited_d2_event(
        &mut self,
        prepared: PreparedInheritedD2EventV4,
        basis: &mut AuthorityReplayBasisV4,
    ) -> Result<V4EventAppendReceipt<InheritedD2EventReceiptV4>, JournalError> {
        self.require_healthy()?;
        let envelope = prepared
            .envelope(&self.log, basis, &self.session_identity)?
            .clone();
        let journal = self.append_one_v4(envelope)?;
        let (next_log, next_basis) = self.replay_candidate_v4_after_durable()?;
        let core = prepared
            .confirm_replayed(&next_log, &next_basis, &self.session_identity)
            .map_err(|error| {
                self.state = ReplayedV4RunSessionState::Uncertain;
                JournalError::Domain(error)
            })?;
        self.log = next_log;
        *basis = next_basis;
        Ok(V4EventAppendReceipt { core, journal })
    }

    pub fn prepare_snapshot_artifact_registration(
        &self,
        registration: ArtifactRegisteredV3,
        source_bundle: &SnapshotSourceBundle,
        artifact_id: &StableId,
        basis: &AuthorityReplayBasisV4,
    ) -> Result<ArtifactRegistrationV3AtV4Admission, JournalError> {
        self.require_healthy()?;
        Ok(self.log.prepare_snapshot_artifact_registration_v3_at_v4(
            registration,
            source_bundle,
            artifact_id,
            basis,
        )?)
    }

    pub fn prepare_reviewer_raw_artifact_registration(
        &self,
        registration: ArtifactRegisteredV3,
        execution_bundle: &ValidatedExecutionBundle,
        basis: &AuthorityReplayBasisV4,
    ) -> Result<ArtifactRegistrationV3AtV4Admission, JournalError> {
        self.require_healthy()?;
        Ok(self
            .log
            .prepare_reviewer_raw_artifact_registration_v3_at_v4(
                registration,
                execution_bundle,
                basis,
            )?)
    }

    pub fn prepare_static_verifier_input_registration(
        &self,
        claim_id: &StableId,
        cas_hash: ContentHash,
        size: u64,
        basis: &AuthorityReplayBasisV4,
    ) -> Result<ArtifactRegistrationV3AtV4Admission, JournalError> {
        self.require_healthy()?;
        Ok(self
            .log
            .prepare_static_verifier_input_registration_v3_at_v4(
                claim_id,
                cas_hash,
                size,
                &self.resolver,
                self.roots,
                basis,
            )?)
    }

    pub fn prepare_static_verifier_output_registration(
        &self,
        claim_id: &StableId,
        cas_hash: ContentHash,
        size: u64,
        basis: &AuthorityReplayBasisV4,
    ) -> Result<ArtifactRegistrationV3AtV4Admission, JournalError> {
        self.require_healthy()?;
        Ok(self
            .log
            .prepare_static_verifier_output_registration_v3_at_v4(
                claim_id,
                cas_hash,
                size,
                &self.resolver,
                self.roots,
                basis,
            )?)
    }

    pub fn append_inherited_artifact_registration(
        &mut self,
        prepared: ArtifactRegistrationV3AtV4Admission,
        basis: &mut AuthorityReplayBasisV4,
    ) -> Result<V4EventAppendReceipt<ArtifactRegistrationV3AtV4Receipt>, JournalError> {
        self.require_healthy()?;
        let envelope = prepared
            .envelope(&self.log, basis, &self.session_identity)?
            .clone();
        let journal = self.append_one_v4(envelope)?;
        let (next_log, next_basis) = self.replay_candidate_v4_after_durable()?;
        let core = prepared
            .confirm_replayed(&next_log, &next_basis, &self.session_identity)
            .map_err(|error| {
                self.state = ReplayedV4RunSessionState::Uncertain;
                JournalError::Domain(error)
            })?;
        self.log = next_log;
        *basis = next_basis;
        Ok(V4EventAppendReceipt { core, journal })
    }

    pub fn mint_verification_bundle(
        &self,
        request: VerificationBundleRequestV4,
        witness: Option<reviewgraphen_core::ExternalWitnessAdmissionV4>,
        basis: &AuthorityReplayBasisV4,
    ) -> Result<ValidatedVerificationBundleV4, JournalError> {
        self.require_healthy()?;
        Ok(self.log.mint_verification_bundle_v4(
            request,
            witness,
            &self.resolver,
            self.roots,
            basis,
        )?)
    }

    pub fn append_verification_bundle(
        &mut self,
        prepared: ValidatedVerificationBundleV4,
        basis: &mut AuthorityReplayBasisV4,
    ) -> Result<V4VerificationBundleAppendReceipt, JournalError> {
        self.require_healthy()?;
        let envelopes = prepared
            .envelopes(&self.log, basis, &self.session_identity)?
            .to_vec();
        let journal = match self.writer.append_verification_bundle_suffix(&envelopes) {
            Ok(receipts) => receipts,
            Err(error @ JournalError::BundleAppendInterrupted { .. }) => {
                self.state = ReplayedV4RunSessionState::Uncertain;
                return Err(error);
            }
            Err(_) if self.writer.append_durability == AppendDurability::Uncertain => {
                self.state = ReplayedV4RunSessionState::Uncertain;
                return Err(JournalError::SessionUncertain);
            }
            Err(error) => return Err(error),
        };
        let (next_log, next_basis) = self.replay_candidate_v4_after_durable()?;
        let authority = prepared
            .confirm_replayed(&next_log, &next_basis, &self.session_identity)
            .map_err(|error| {
                self.state = ReplayedV4RunSessionState::Uncertain;
                JournalError::Domain(error)
            })?;
        self.log = next_log;
        *basis = next_basis;
        Ok(V4VerificationBundleAppendReceipt { authority, journal })
    }

    /// Canonicalizes and durably publishes one trusted descriptor before
    /// sealing its exact registration. The CAS receipt remains storage-only;
    /// Core admission is independently required before any event append.
    pub fn publish_gluing_input(
        &mut self,
        trusted: TrustedGluingInputSourceV4,
        descriptor: GluingInputDescriptorV4,
        basis: &mut AuthorityReplayBasisV4,
    ) -> Result<GluingInputPublicationV4, JournalError> {
        self.require_healthy()?;
        let bytes = canonical_json(&descriptor)?;
        if bytes.len() > MAX_M5_DESCRIPTOR_CANONICAL_BYTES {
            return Err(JournalError::Incomplete {
                limit: u64::try_from(MAX_M5_DESCRIPTOR_CANONICAL_BYTES).unwrap_or(u64::MAX),
                observed: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            });
        }
        self.validate_gluing_source_bytes(&trusted, &descriptor, &bytes)?;
        if let Some(existing) = self.reconstruct_registered_gluing_input(&trusted, &descriptor)? {
            return Ok(existing);
        }
        let descriptor_hash = trusted.descriptor_hash().clone();
        let descriptor_size = trusted.descriptor_size();
        let admission = self.log.admit_gluing_input_registration_v4(
            trusted,
            &bytes,
            self.roots,
            basis,
            &self.session_identity,
        )?;
        #[cfg(test)]
        if take_v4_gluing_fault(V4GluingFault::BeforeCasPublication) {
            return Err(JournalError::GluingInputPublicationInterruptedV4 {
                stage: "before CAS publication",
            });
        }
        let hash = CasHash::parse(descriptor_hash.to_string())?;
        let cas = CasStore::open(self.resolver.reader.root)?.put(
            &hash,
            Some(descriptor_size),
            std::io::Cursor::new(&bytes),
        )?;
        #[cfg(test)]
        if take_v4_gluing_fault(V4GluingFault::AfterCasPublication) {
            return Err(JournalError::GluingInputPublicationInterruptedV4 {
                stage: "after CAS publication",
            });
        }
        let append = self.append_gluing_input_registration(admission, basis)?;
        Ok(GluingInputPublicationV4::Confirmed { cas, append })
    }

    /// Adopts only the immutable object named by one trusted binding. The
    /// object is read through the admitted CAS descriptor, strictly decoded,
    /// and then independently admitted by Core at the exact next context.
    pub fn adopt_gluing_input(
        &mut self,
        trusted: TrustedGluingInputSourceV4,
        basis: &mut AuthorityReplayBasisV4,
    ) -> Result<GluingInputPublicationV4, JournalError> {
        self.require_healthy()?;
        let hash_value = trusted.descriptor_hash().clone();
        if let Some(matches) = self
            .log
            .registered_gluing_input_matches_trusted_v4(&trusted)
        {
            if matches {
                return Ok(GluingInputPublicationV4::AlreadyRegistered {
                    context_id: trusted.context_id().clone(),
                    descriptor_id: trusted.descriptor_id().clone(),
                    registration_id: trusted.registration_id().clone(),
                });
            }
            return Err(JournalError::OrphanCanonicalInput { hash: hash_value });
        }
        let size = usize::try_from(trusted.descriptor_size()).map_err(|_| {
            JournalError::OrphanCanonicalInput {
                hash: hash_value.clone(),
            }
        })?;
        if size > MAX_M5_DESCRIPTOR_CANONICAL_BYTES {
            return Err(JournalError::OrphanCanonicalInput { hash: hash_value });
        }
        let cas_hash = CasHash::parse(hash_value.to_string()).map_err(|_| {
            JournalError::OrphanCanonicalInput {
                hash: hash_value.clone(),
            }
        })?;
        let mut bytes = vec![0_u8; size];
        self.resolver
            .reader
            .read_exact_slice(&cas_hash, &mut bytes)
            .map_err(|_| JournalError::OrphanCanonicalInput {
                hash: hash_value.clone(),
            })?;
        let descriptor = GluingInputDescriptorV4::from_json_bytes(&bytes).map_err(|_| {
            JournalError::OrphanCanonicalInput {
                hash: hash_value.clone(),
            }
        })?;
        self.validate_gluing_source_bytes(&trusted, &descriptor, &bytes)
            .map_err(|_| JournalError::OrphanCanonicalInput {
                hash: hash_value.clone(),
            })?;
        let admission = self
            .log
            .admit_gluing_input_registration_v4(
                trusted,
                &bytes,
                self.roots,
                basis,
                &self.session_identity,
            )
            .map_err(|_| JournalError::OrphanCanonicalInput {
                hash: hash_value.clone(),
            })?;
        let append = self.append_gluing_input_registration(admission, basis)?;
        Ok(GluingInputPublicationV4::Confirmed {
            cas: CasReceipt {
                hash: cas_hash,
                size: u64::try_from(size).unwrap_or(u64::MAX),
                existed: true,
            },
            append,
        })
    }

    pub fn mint_gluing_bundle(
        &self,
        basis: &AuthorityReplayBasisV4,
    ) -> Result<ValidatedGluingBundleV4, JournalError> {
        self.require_healthy()?;
        Ok(self
            .log
            .mint_gluing_bundle_v4(basis, &self.session_identity)?)
    }

    /// Appends the whole M5 topology as one ordinary V4 line. It deliberately
    /// uses `append.pending`, not the M4 multi-event marker, so a partial M5
    /// topology is impossible and CanonicalTail is the sole uncertainty path.
    pub fn append_gluing_bundle(
        &mut self,
        prepared: ValidatedGluingBundleV4,
        basis: &mut AuthorityReplayBasisV4,
    ) -> Result<V4GluingBundleAppendReceipt, JournalError> {
        self.require_healthy()?;
        let envelope = prepared
            .envelope(&self.log, basis, &self.session_identity)?
            .clone();
        let journal = self.append_one_v4(envelope)?;
        let (next_log, next_basis) = self.replay_candidate_v4_after_durable()?;
        let core = prepared
            .confirm_replayed(&next_log, &next_basis, &self.session_identity)
            .map_err(|error| {
                self.state = ReplayedV4RunSessionState::Uncertain;
                JournalError::Domain(error)
            })?;
        self.log = next_log;
        *basis = next_basis;
        Ok(V4EventAppendReceipt { core, journal })
    }

    fn append_gluing_input_registration(
        &mut self,
        prepared: TrustedGluingInputAdmissionV4,
        basis: &mut AuthorityReplayBasisV4,
    ) -> Result<V4EventAppendReceipt<ArtifactRegistrationReceiptV4>, JournalError> {
        let envelope = prepared
            .envelope(&self.log, basis, &self.session_identity)?
            .clone();
        let journal = self.append_one_v4(envelope)?;
        let (next_log, next_basis) = self.replay_candidate_v4_after_durable()?;
        let core = prepared
            .confirm_replayed(&next_log, &next_basis, &self.session_identity)
            .map_err(|error| {
                self.state = ReplayedV4RunSessionState::Uncertain;
                JournalError::Domain(error)
            })?;
        self.log = next_log;
        *basis = next_basis;
        Ok(V4EventAppendReceipt { core, journal })
    }

    fn validate_gluing_source_bytes(
        &self,
        trusted: &TrustedGluingInputSourceV4,
        descriptor: &GluingInputDescriptorV4,
        bytes: &[u8],
    ) -> Result<(), JournalError> {
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if trusted.descriptor_hash() != &ContentHash::sha256(bytes)
            || trusted.descriptor_size() != size
            || trusted.descriptor_media_type() != GLUING_INPUT_MEDIA_TYPE_V4
            || trusted.descriptor_sensitivity() != ArtifactSensitivity::CanonicalState
            || trusted.descriptor_id() != descriptor.id()
            || trusted.context_id() != descriptor.context_id()
        {
            return Err(reviewgraphen_core::DomainError::GluingInputAdmissionMismatch.into());
        }
        Ok(())
    }

    fn reconstruct_registered_gluing_input(
        &self,
        trusted: &TrustedGluingInputSourceV4,
        descriptor: &GluingInputDescriptorV4,
    ) -> Result<Option<GluingInputPublicationV4>, JournalError> {
        let Some(matches) = self
            .log
            .registered_gluing_input_matches_v4(trusted, descriptor)
        else {
            return Ok(None);
        };
        if !matches {
            return Err(JournalError::OrphanCanonicalInput {
                hash: trusted.descriptor_hash().clone(),
            });
        }
        Ok(Some(GluingInputPublicationV4::AlreadyRegistered {
            context_id: trusted.context_id().clone(),
            descriptor_id: trusted.descriptor_id().clone(),
            registration_id: trusted.registration_id().clone(),
        }))
    }

    fn append_one_v4(
        &mut self,
        envelope: EventEnvelope,
    ) -> Result<JournalAppendReceipt, JournalError> {
        match self.writer.append(envelope) {
            Ok(receipt) => Ok(receipt),
            Err(_) if self.writer.append_durability == AppendDurability::Uncertain => {
                self.state = ReplayedV4RunSessionState::Uncertain;
                Err(JournalError::SessionUncertain)
            }
            Err(error) => Err(error),
        }
    }

    fn replay_candidate_v4_after_durable(
        &mut self,
    ) -> Result<(EventLogV4, AuthorityReplayBasisV4), JournalError> {
        let JournalGenesis::V4Shared(genesis) = &self.writer.identity.genesis else {
            self.state = ReplayedV4RunSessionState::Uncertain;
            return Err(JournalError::Identity(
                "V4 session lost its verified genesis",
            ));
        };
        let mut projection = crate::index::ReplayProjectionChargeV5::default();
        let mut projection_error = None;
        let replayed = EventLogV4::replay_confirmed_v4_prefix_for_session_with_projection_visitor(
            self.writer.identity.run_id.clone(),
            genesis,
            &self.writer.state.events,
            &self.resolver,
            self.roots,
            EventReplayLimits::new(
                self.writer.limits.max_events,
                self.writer.limits.max_replay_bytes,
            ),
            &self.session_identity,
            |metadata, payload| {
                if projection_error.is_none()
                    && let Err(error) = projection.observe(metadata, payload)
                {
                    projection_error = Some(error);
                }
            },
        )
        .map_err(|error| {
            self.state = ReplayedV4RunSessionState::Uncertain;
            JournalError::Domain(error)
        })?;
        if projection_error.is_some() {
            self.state = ReplayedV4RunSessionState::Uncertain;
            return Err(JournalError::Incomplete {
                limit: self.writer.limits.max_replay_bytes,
                observed: u64::MAX,
            });
        }
        self.index_projection = Some(projection);
        Ok(replayed)
    }

    pub(crate) fn index_v5_genesis(&self) -> Result<&RunGenesisSnapshot, JournalError> {
        self.require_healthy()?;
        Ok(&self.index_genesis)
    }

    fn retain_confirmed_prefix_bytes(&mut self) -> Result<Vec<u8>, JournalError> {
        self.require_healthy()?;
        read_prefix(&mut self.writer.file, self.writer.state.confirmed_offset)
    }

    fn retained_event_bytes(&self) -> Result<u64, JournalError> {
        self.require_healthy()?;
        self.writer
            .state
            .retained_envelope_bytes()?
            .checked_add(
                self.log
                    .retained_envelope_bytes_for_store()
                    .map_err(JournalError::Domain)?,
            )
            .ok_or(JournalError::Incomplete {
                limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                observed: u64::MAX,
            })
    }

    pub(crate) fn index_v5_replay_projection(
        &self,
    ) -> Result<&crate::index::ReplayProjectionChargeV5, JournalError> {
        self.require_healthy()?;
        self.index_projection.as_ref().ok_or(JournalError::Identity(
            "V5 projection is unavailable for this recovered session",
        ))
    }

    pub(crate) fn index_v5_visit_projection(
        &self,
        visitor: &mut dyn for<'event> FnMut(
            reviewgraphen_core::BorrowedV4EventMetadata<'event>,
            reviewgraphen_core::BorrowedProjectionPayloadV4<'event>,
        ),
    ) -> Result<(), JournalError> {
        self.require_healthy()?;
        self.log
            .visit_confirmed_projection_v4(visitor)
            .map_err(JournalError::Domain)
    }

    pub(crate) fn index_v5_obligation_lifecycle(
        &self,
        obligation_id: &StableId,
    ) -> Result<Option<ObligationLifecycle>, JournalError> {
        self.require_healthy()?;
        Ok(self.log.obligation_lifecycle_v4(obligation_id))
    }

    pub(crate) fn index_v5_for_each_claim_assessment(
        &self,
        visitor: &mut dyn FnMut(reviewgraphen_core::BorrowedClaimAssessmentProjectionV4<'_>),
    ) -> Result<(), JournalError> {
        self.require_healthy()?;
        for assessment in self.log.claim_assessments_v3() {
            visitor(assessment);
        }
        Ok(())
    }

    pub(crate) fn index_v5_confirmed_offset(&self) -> Result<u64, JournalError> {
        self.require_healthy()?;
        Ok(self.writer.state.confirmed_offset)
    }

    pub(crate) fn index_v5_replay_prefix(
        &self,
        event_count: u64,
        confirmed_offset: u64,
    ) -> Result<IndexV5ReplayedPrefix, JournalError> {
        self.require_healthy()?;
        let count = usize::try_from(event_count).map_err(|_| JournalError::Incomplete {
            limit: self.writer.limits.max_events,
            observed: event_count,
        })?;
        let prefix = self
            .writer
            .state
            .events
            .get(..count)
            .ok_or(JournalError::Identity(
                "index prefix event count exceeds journal",
            ))?;
        let JournalGenesis::V4Shared(genesis) = &self.writer.identity.genesis else {
            return Err(JournalError::Identity(
                "V4 session lost its verified genesis",
            ));
        };
        let session_identity = OpaqueSessionIdentityV4::fresh();
        let mut projection = crate::index::ReplayProjectionChargeV5::default();
        let mut projection_error = None;
        let (log, basis) =
            EventLogV4::replay_confirmed_v4_prefix_for_session_with_projection_visitor(
                self.writer.identity.run_id.clone(),
                genesis,
                prefix,
                &self.resolver,
                self.roots,
                EventReplayLimits::new(
                    self.writer.limits.max_events,
                    self.writer.limits.max_replay_bytes,
                ),
                &session_identity,
                |envelope, payload| {
                    if projection_error.is_none()
                        && let Err(error) = projection.observe(envelope, payload)
                    {
                        projection_error = Some(error);
                    }
                },
            )?;
        if projection_error.is_some() {
            return Err(JournalError::Incomplete {
                limit: self.writer.limits.max_replay_bytes,
                observed: u64::MAX,
            });
        }
        if projection.confirmed_offset != confirmed_offset {
            return Err(JournalError::Identity(
                "index prefix offset does not match opaque replay metadata",
            ));
        }
        Ok(IndexV5ReplayedPrefix {
            log,
            basis,
            index_genesis: decode_index_v5_genesis(genesis)?,
            confirmed_offset,
            projection,
        })
    }

    fn require_healthy(&self) -> Result<(), JournalError> {
        match self.state {
            ReplayedV4RunSessionState::Healthy => self.writer.refuse_bundle_resume_gate(),
            ReplayedV4RunSessionState::Uncertain => Err(JournalError::SessionUncertain),
        }
    }
}

impl<'root, 'roots> RecoveredM4BundleV4Session<'root, 'roots> {
    #[must_use]
    pub fn durable_stage(&self) -> M4BundlePrefixStageV4 {
        M4BundlePrefixStageV4 {
            classification: M4BundlePrefixClassificationV4::StrictInterior,
            confirmed_events: u64::try_from(self.confirmed_events).unwrap_or(u64::MAX),
            expected_events: self.marker.expected_count,
        }
    }

    pub fn resume_verification_bundle(
        mut self,
        authority: VerificationBundleResumeAuthorityV4,
    ) -> Result<
        (
            ReplayedV4RunSession<'root, 'roots>,
            AuthorityReplayBasisV4,
            V4VerificationBundleAppendReceipt,
        ),
        JournalError,
    > {
        let prepared = self
            .core_session
            .prepare_resume(authority, &self.session_identity)?;
        let suffix = prepared.envelopes().to_vec();
        let journal = match self.writer.resume_verification_bundle_suffix(
            &self.marker,
            &suffix,
            self.confirmed_events,
        ) {
            Ok(receipts) => receipts,
            Err(error @ JournalError::BundleAppendInterrupted { .. }) => return Err(error),
            Err(_) if self.writer.append_durability == AppendDurability::Uncertain => {
                return Err(JournalError::SessionUncertain);
            }
            Err(error) => return Err(error),
        };
        let (log, next_basis, authority_receipt) = prepared.confirm_replayed(
            &self.writer.state.events,
            &self.resolver,
            self.roots,
            EventReplayLimits::new(
                self.writer.limits.max_events,
                self.writer.limits.max_replay_bytes,
            ),
            &self.session_identity,
        )?;
        let JournalGenesis::V4Shared(genesis) = &self.writer.identity.genesis else {
            return Err(JournalError::Identity(
                "V4 session lost its verified genesis",
            ));
        };
        let index_projection = index_v5_projection_charge_from_log(
            &log,
            self.writer.limits.max_replay_bytes,
            self.writer.state.confirmed_offset,
        )?;
        let index_genesis = decode_index_v5_genesis(genesis)?;
        Ok((
            ReplayedV4RunSession {
                _root_lock: self.root_lock,
                _run_lock: self.run_lock,
                writer: self.writer,
                log,
                index_genesis,
                resolver: self.resolver,
                roots: self.roots,
                session_identity: self.session_identity,
                state: ReplayedV4RunSessionState::Healthy,
                index_projection: Some(index_projection),
            },
            next_basis,
            V4VerificationBundleAppendReceipt {
                authority: authority_receipt,
                journal,
            },
        ))
    }
}

fn same_authority_basis(left: &AuthorityReplayBasisV3, right: &AuthorityReplayBasisV3) -> bool {
    left.basis_digest() == right.basis_digest()
        && left.confirmed_tail_hash() == right.confirmed_tail_hash()
        && left.confirmed_event_count() == right.confirmed_event_count()
        && left.run_id() == right.run_id()
        && left.genesis_hash() == right.genesis_hash()
        && left.policy_revision_hash() == right.policy_revision_hash()
        && left.authority_entry_count() == right.authority_entry_count()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppendDurability {
    Confirmed,
    Uncertain,
}

pub struct JournalWriter {
    file: File,
    identity: JournalIdentity,
    limits: JournalLimits,
    state: ScanState,
    intents: OwnedFd,
    completions: OwnedFd,
    run: OwnedFd,
    poisoned: bool,
    append_durability: AppendDurability,
    #[cfg(any(test, feature = "test-support"))]
    faults: std::collections::VecDeque<AppendFault>,
}

/// A deterministic, per-writer failure point used only by the journal's
/// contract tests.  Keeping the injector on the writer rather than in global
/// state makes concurrent tests independent and documents the exact durable
/// boundary being exercised.
#[cfg(any(test, feature = "test-support"))]
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppendFault {
    PartialWrite,
    Flush,
    SyncData,
    Truncate,
    RollbackSync,
    IntentPublish,
    ClearMarkerDirectorySync,
    BundleAfterDurableLine1,
    BundleAfterDurableLine2,
    BundleStageSync,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryFault {
    MarkerLogSync,
    ClearMarkerDirectorySync,
    BundleAfterIntent,
    BundleAfterTruncateSync,
    BundleAfterCompletion,
    BundleAfterMarkerUnlink,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum V4BootstrapFault {
    BeforeDirectorySetup,
    BeforeLineWrite,
    BeforeFileSync,
    AfterFileSync,
    AfterLink,
    AfterDirectorySync,
    BeforeSessionOpen,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum V4RecoveryFault {
    GenesisDirectory,
    TailFile,
    TailDirectory,
    M4Directory,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum V4GluingFault {
    BeforeCasPublication,
    AfterCasPublication,
}

#[cfg(test)]
thread_local! {
    static V4_BOOTSTRAP_FAULT: std::cell::Cell<Option<V4BootstrapFault>> = const { std::cell::Cell::new(None) };
    static V4_RECOVERY_FAULT: std::cell::Cell<Option<V4RecoveryFault>> = const { std::cell::Cell::new(None) };
    static V4_GLUING_FAULT: std::cell::Cell<Option<V4GluingFault>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn inject_v4_bootstrap_fault(fault: V4BootstrapFault) {
    V4_BOOTSTRAP_FAULT.set(Some(fault));
}

#[cfg(test)]
fn take_v4_bootstrap_fault(expected: V4BootstrapFault) -> bool {
    V4_BOOTSTRAP_FAULT.with(|fault| {
        if fault.get() == Some(expected) {
            fault.set(None);
            true
        } else {
            false
        }
    })
}

#[cfg(test)]
fn inject_v4_recovery_fault(fault: V4RecoveryFault) {
    V4_RECOVERY_FAULT.set(Some(fault));
}

#[cfg(test)]
fn take_v4_recovery_fault(expected: V4RecoveryFault) -> bool {
    V4_RECOVERY_FAULT.with(|fault| {
        if fault.get() == Some(expected) {
            fault.set(None);
            true
        } else {
            false
        }
    })
}

#[cfg(test)]
fn inject_v4_gluing_fault(fault: V4GluingFault) {
    V4_GLUING_FAULT.set(Some(fault));
}

#[cfg(test)]
fn take_v4_gluing_fault(expected: V4GluingFault) -> bool {
    V4_GLUING_FAULT.with(|fault| {
        if fault.get() == Some(expected) {
            fault.set(None);
            true
        } else {
            false
        }
    })
}

#[derive(Clone, Debug)]
struct ScanState {
    events: Vec<EventEnvelope>,
    confirmed_offset: u64,
    tail_hash: ContentHash,
    torn: Option<TornTail>,
}

impl ScanState {
    fn retained_envelope_bytes(&self) -> Result<u64, JournalError> {
        let slots = self
            .events
            .capacity()
            .checked_mul(std::mem::size_of::<EventEnvelope>())
            .and_then(|value| u64::try_from(value).ok())
            .ok_or(JournalError::Incomplete {
                limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                observed: u64::MAX,
            })?;
        self.events.iter().try_fold(slots, |total, event| {
            total
                .checked_add(u64::try_from(event.allocated_bytes()).unwrap_or(u64::MAX))
                .ok_or(JournalError::Incomplete {
                    limit: MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                    observed: u64::MAX,
                })
        })
    }
}
#[derive(Clone, Debug)]
struct TornTail {
    good_offset: u64,
    discarded: Vec<u8>,
    pre_tail_hash: ContentHash,
}

struct RecoveryAudit {
    pending: Option<RecoveryIntent>,
    completed: Vec<(RecoveryIntent, RecoveryCompletion)>,
    receipt_files: u64,
    receipt_scan_bytes: u64,
}

struct BundlePendingInspection {
    marker: VerificationBundlePendingMarkerV3,
    marker_hash: ContentHash,
    planned: Vec<EventEnvelope>,
    pre_state: ScanState,
    current_state: ScanState,
    confirmed_events: usize,
    discarded: Vec<u8>,
}

impl<'a> EventJournal<'a> {
    pub(crate) fn matches_store_root(&self, root: &StoreRoot) -> bool {
        self.root.identity() == root.identity()
    }

    /// Durably publishes the only fresh V4 journal prefix.  The opaque core
    /// log has already derived the nested registration, manifest, and exact
    /// envelope; Store only persists those sealed bytes.  Lock ordering is
    /// root then run for the whole CAS-to-journal publication boundary.
    pub fn publish_new_v4(
        root: &'a StoreRoot,
        log: EventLogV4,
    ) -> Result<(Self, GenesisCommitReceiptV4), JournalError> {
        let limits = JournalLimits::from_store(root.limits());
        validate_limits(limits)?;
        if log.envelopes().len() != 1 {
            return Err(JournalError::Identity(
                "V4 bootstrap requires exactly one sealed genesis envelope",
            ));
        }
        let identity = JournalIdentity::new(
            log.run_id().clone(),
            JournalGenesis::V4(log.canonical_genesis_bytes().to_vec()),
        )?;
        let envelope = &log.envelopes()[0];
        validate_prefix(&identity, std::slice::from_ref(envelope))?;
        let mut line = envelope.canonical_bytes()?;
        line.push(b'\n');
        let line_len = u64::try_from(line.len()).map_err(|_| JournalError::Incomplete {
            limit: limits.max_event_line_bytes,
            observed: u64::MAX,
        })?;
        limit(line_len, limits.max_event_line_bytes)?;
        limit(1, limits.max_events)?;
        limit(line_len, limits.max_replay_bytes)?;

        // Open the root again to obtain a distinct open-file description;
        // dropping this guard releases the bootstrap lock without retaining
        // flock state on StoreRoot's anchor descriptor.
        let root_lock =
            acquire_v4_root_lock(root).map_err(|_| JournalError::GenesisNotCommittedV4 {
                stage: "root lock acquisition",
            })?;
        let cas_hash = CasHash::parse(log.genesis_hash().to_string())?;
        let genesis_size = u64::try_from(log.canonical_genesis_bytes().len()).map_err(|_| {
            JournalError::Incomplete {
                limit: root.limits().max_object_bytes,
                observed: u64::MAX,
            }
        })?;
        // CAS publication is intentionally before the first line.  An error
        // later in this function can therefore leave only an authority-free
        // orphan, which bootstrap recovery never treats as a run.
        CasStore::open(root)
            .and_then(|cas| {
                cas.put(
                    &cas_hash,
                    Some(genesis_size),
                    std::io::Cursor::new(log.canonical_genesis_bytes()),
                )
            })
            .map_err(|_| JournalError::GenesisNotCommittedV4 {
                stage: "genesis CAS publication",
            })?;

        #[cfg(test)]
        if take_v4_bootstrap_fault(V4BootstrapFault::BeforeDirectorySetup) {
            return Err(JournalError::GenesisNotCommittedV4 {
                stage: "before directory setup",
            });
        }
        let runs = open_or_create_dir(root.fd(), RUNS_DIR, "runs directory").map_err(|_| {
            JournalError::GenesisNotCommittedV4 {
                stage: "runs directory setup",
            }
        })?;
        let run = open_or_create_dir(&runs, &run_dir_name(&identity.run_id), "run directory")
            .map_err(|_| JournalError::GenesisNotCommittedV4 {
                stage: "run directory setup",
            })?;
        let run_lock = dup(&run).map_err(|_| JournalError::GenesisNotCommittedV4 {
            stage: "run lock open",
        })?;
        fs::flock(&run_lock, FlockOperation::LockExclusive).map_err(|_| {
            JournalError::GenesisNotCommittedV4 {
                stage: "run lock acquisition",
            }
        })?;
        let recovery =
            open_or_create_dir(&run, RECOVERY_DIR, "run recovery directory").map_err(|_| {
                JournalError::GenesisNotCommittedV4 {
                    stage: "recovery directory setup",
                }
            })?;
        let _ = open_or_create_dir(&recovery, INTENTS_DIR, "recovery intent directory").map_err(
            |_| JournalError::GenesisNotCommittedV4 {
                stage: "recovery intents directory setup",
            },
        )?;
        let _ = open_or_create_dir(&recovery, COMPLETIONS_DIR, "recovery completion directory")
            .map_err(|_| JournalError::GenesisNotCommittedV4 {
                stage: "recovery completions directory setup",
            })?;
        publish_initial_log_v4(&run, &line)?;
        drop(run_lock);
        drop(root_lock);
        #[cfg(test)]
        if take_v4_bootstrap_fault(V4BootstrapFault::BeforeSessionOpen) {
            return Err(JournalError::GenesisSessionUncertainV4);
        }
        let journal = Self::open_with_limits(root, identity, limits)
            .map_err(|_| JournalError::GenesisSessionUncertainV4)?;
        Ok((
            journal,
            GenesisCommitReceiptV4 {
                run_id: log.run_id().clone(),
                genesis_hash: log.genesis_hash().clone(),
                event_id: envelope.id().clone(),
                event_hash: envelope.event_hash().clone(),
                confirmed_offset: line_len,
            },
        ))
    }

    /// Durably publishes a complete homogeneous V5 target predecessor. The
    /// prefix must end at its deterministic review plan; genesis-only and
    /// partially planned targets are refused.
    pub fn publish_new_v5(
        root: &'a StoreRoot,
        log: EventLogV5,
    ) -> Result<(Self, GenesisCommitReceiptV5), JournalError> {
        let limits = JournalLimits::from_store(root.limits());
        validate_limits(limits)?;
        let target_state = log.replay_pre_incremental_state_for_store()?;
        let target_projection = target_state.projection();
        let identity = JournalIdentity::new(
            log.run_id().clone(),
            JournalGenesis::V5(log.canonical_genesis_bytes().to_vec()),
        )?;
        validate_prefix(&identity, log.envelopes())?;
        let mut lines = Vec::new();
        for envelope in log.envelopes() {
            lines.extend_from_slice(&envelope.canonical_bytes()?);
            lines.push(b'\n');
        }
        let line_len = u64::try_from(lines.len()).map_err(|_| JournalError::Incomplete {
            limit: limits.max_event_line_bytes,
            observed: u64::MAX,
        })?;
        for envelope in log.envelopes() {
            let envelope_len = u64::try_from(envelope.canonical_bytes()?.len())
                .unwrap_or(u64::MAX)
                .saturating_add(1);
            limit(envelope_len, limits.max_event_line_bytes)?;
        }
        limit(
            u64::try_from(log.envelopes().len()).unwrap_or(u64::MAX),
            limits.max_events,
        )?;
        limit(line_len, limits.max_replay_bytes)?;

        let root_lock =
            acquire_v4_root_lock(root).map_err(|_| JournalError::GenesisNotCommittedV5 {
                stage: "root lock acquisition",
            })?;
        let cas_hash = CasHash::parse(log.genesis_hash().to_string())?;
        let genesis_size = u64::try_from(log.canonical_genesis_bytes().len()).map_err(|_| {
            JournalError::Incomplete {
                limit: root.limits().max_object_bytes,
                observed: u64::MAX,
            }
        })?;
        CasStore::open(root)
            .and_then(|cas| {
                cas.put(
                    &cas_hash,
                    Some(genesis_size),
                    std::io::Cursor::new(log.canonical_genesis_bytes()),
                )
            })
            .map_err(|_| JournalError::GenesisNotCommittedV5 {
                stage: "genesis CAS publication",
            })?;
        let reader =
            CasReader::open_existing(root).map_err(|_| JournalError::GenesisNotCommittedV5 {
                stage: "target source CAS admission",
            })?;
        for registration in target_projection.registrations() {
            let hash = CasHash::parse(registration.cas_hash().to_string()).map_err(|_| {
                JournalError::GenesisNotCommittedV5 {
                    stage: "target source CAS identity",
                }
            })?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(usize::try_from(registration.size()).map_err(|_| {
                    JournalError::GenesisNotCommittedV5 {
                        stage: "target source CAS size admission",
                    }
                })?)
                .map_err(|_| JournalError::GenesisNotCommittedV5 {
                    stage: "target source CAS buffer admission",
                })?;
            reader
                .read_into(&hash, Some(registration.size()), &mut bytes)
                .map_err(|_| JournalError::GenesisNotCommittedV5 {
                    stage: "target source CAS verification",
                })?;
        }
        let runs = open_or_create_dir(root.fd(), RUNS_DIR, "runs directory").map_err(|_| {
            JournalError::GenesisNotCommittedV5 {
                stage: "runs directory setup",
            }
        })?;
        let run = open_or_create_dir(&runs, &run_dir_name(&identity.run_id), "run directory")
            .map_err(|_| JournalError::GenesisNotCommittedV5 {
                stage: "run directory setup",
            })?;
        let run_lock =
            acquire_v4_run_lock(&run).map_err(|_| JournalError::GenesisNotCommittedV5 {
                stage: "run lock acquisition",
            })?;
        let recovery =
            open_or_create_dir(&run, RECOVERY_DIR, "run recovery directory").map_err(|_| {
                JournalError::GenesisNotCommittedV5 {
                    stage: "recovery directory setup",
                }
            })?;
        let _ = open_or_create_dir(&recovery, INTENTS_DIR, "recovery intent directory")?;
        let _ = open_or_create_dir(&recovery, COMPLETIONS_DIR, "recovery completion directory")?;
        publish_initial_log_v5(&run, &lines)?;
        drop(run_lock);
        drop(root_lock);
        let journal = Self::open_with_limits(root, identity, limits)
            .map_err(|_| JournalError::GenesisSessionUncertainV5)?;
        Ok((
            journal,
            GenesisCommitReceiptV5 {
                run_id: log.run_id().clone(),
                genesis_hash: log.genesis_hash().clone(),
                event_id: log.envelopes()[0].id().clone(),
                event_hash: log.envelopes()[0].event_hash().clone(),
                confirmed_offset: line_len,
            },
        ))
    }

    /// Reads V4 recovery state under the same root -> run -> journal lock
    /// ordering used by mutation, but makes no filesystem change. The
    /// returned key seals the observed complete-file hash, prefix cursor,
    /// tail, and marker digest.
    pub fn inspect_recovery_v4(
        root: &StoreRoot,
        inspection: RecoveryInspectionV4,
    ) -> Result<RecoveryKeyV4, JournalError> {
        if inspection.run_id.kind() != "run"
            || inspection.event_contract_version != EVENT_CONTRACT_SCHEMA_V4
        {
            return Err(JournalError::Identity(
                "V4 recovery requires its exact event contract and a run ID",
            ));
        }
        let root_lock = acquire_v4_root_lock(root)?;
        let limits = JournalLimits::from_store(root.limits());
        let result = (|| {
            let runs = match open_existing_dir(root.fd(), RUNS_DIR, "runs directory") {
                Ok(value) => value,
                Err(JournalError::Missing)
                    if inspection.expected_kind == RecoveryKindV4::GenesisBootstrap =>
                {
                    return Ok(v4_absent_recovery_key(inspection));
                }
                Err(JournalError::Missing) => return Err(JournalError::RecoveryKeyMismatchV4),
                Err(error) => return Err(error),
            };
            let run = match open_existing_dir(
                &runs,
                &run_dir_name(&inspection.run_id),
                "run directory",
            ) {
                Ok(value) => value,
                Err(JournalError::Missing)
                    if inspection.expected_kind == RecoveryKindV4::GenesisBootstrap =>
                {
                    return Ok(v4_absent_recovery_key(inspection));
                }
                Err(JournalError::Missing) => return Err(JournalError::RecoveryKeyMismatchV4),
                Err(error) => return Err(error),
            };
            let _run_lock = acquire_v4_run_lock(&run)?;
            let append_pending = marker_exists(&run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES)?;
            let bundle_pending = bundle_file_exists(&run, BUNDLE_PENDING_MARKER)?
                || bundle_file_exists(&run, BUNDLE_PENDING_STAGE)?;
            let mut file = match open_verified_log(&run, false) {
                Ok(fd) => File::from(fd),
                Err(JournalError::Missing)
                    if inspection.expected_kind == RecoveryKindV4::GenesisBootstrap
                        && !append_pending
                        && !bundle_pending =>
                {
                    return Ok(v4_absent_recovery_key(inspection));
                }
                Err(JournalError::Missing) => return Err(JournalError::RecoveryKeyMismatchV4),
                Err(error) => return Err(error),
            };
            fs::flock(file.as_fd(), FlockOperation::LockExclusive).map_err(StoreError::Io)?;
            let genesis = CasStore::open(root)?
                .read(&CasHash::parse(inspection.genesis_hash.to_string())?)?;
            let observed = inspect_v4_file(
                file.try_clone()?,
                &inspection.run_id,
                &inspection.genesis_hash,
                &genesis,
                limits,
            )?;
            if inspection.expected_kind == RecoveryKindV4::M4BundleResume {
                if !bundle_pending || append_pending {
                    return Err(JournalError::RecoveryKeyMismatchV4);
                }
                let identity =
                    JournalIdentity::new(inspection.run_id.clone(), JournalGenesis::V4(genesis))?;
                let Some(pending) =
                    inspect_bundle_pending_file_locked(&run, &identity, limits, &mut file)?
                else {
                    return Err(JournalError::RecoveryKeyMismatchV4);
                };
                if !pending.discarded.is_empty() {
                    return Err(JournalError::CorruptNeedsRecovery {
                        good_offset: pending.current_state.confirmed_offset,
                        auto_recoverable: false,
                    });
                }
                let _ = read_bundle_stage(&run, pending.marker.expected_count)?;
                let _ = v4_bundle_prefix_stage(&pending)?;
                return Ok(RecoveryKeyV4 {
                    run_id: inspection.run_id,
                    genesis_hash: inspection.genesis_hash,
                    event_contract_version: inspection.event_contract_version,
                    expected_kind: inspection.expected_kind,
                    pre_recovery_offset: observed.good_offset,
                    pre_recovery_tail_hash: observed.tail_hash,
                    pre_recovery_file_hash: Some(observed.file_hash),
                    pending_digest: Some(pending.marker_hash),
                });
            }
            if bundle_pending {
                return Err(JournalError::RecoveryKeyMismatchV4);
            }
            if inspection.expected_kind == RecoveryKindV4::GenesisBootstrap
                && (append_pending
                    || observed.event_count > 1
                    || (observed.event_count == 1 && observed.torn))
            {
                // A durable first event is already a committed run. Never let
                // bootstrap recovery remove it because a later append tore.
                return Err(JournalError::RecoveryKeyMismatchV4);
            }
            if inspection.expected_kind == RecoveryKindV4::CanonicalTail
                && (!observed.torn && !append_pending || observed.event_count == 0)
            {
                return Err(JournalError::RecoveryKeyMismatchV4);
            }
            Ok(RecoveryKeyV4 {
                run_id: inspection.run_id,
                genesis_hash: inspection.genesis_hash,
                event_contract_version: inspection.event_contract_version,
                expected_kind: inspection.expected_kind,
                pre_recovery_offset: observed.good_offset,
                pre_recovery_tail_hash: observed.tail_hash,
                pre_recovery_file_hash: Some(observed.file_hash),
                pending_digest: append_pending.then(|| ContentHash::sha256(APPEND_PENDING_BYTES)),
            })
        })();
        drop(root_lock);
        result
    }

    /// Performs the genesis-only recovery branch.  It consumes the inspection
    /// key and rechecks every sealed field while holding root then run locks.
    /// It never constructs a session or authority basis.
    pub fn recover_new_v4(
        root: &'a StoreRoot,
        key: RecoveryKeyV4,
        provenance: RecoveryProvenanceV4,
    ) -> Result<GenesisRecoveryV4<'a>, JournalError> {
        if key.expected_kind != RecoveryKindV4::GenesisBootstrap
            || key.event_contract_version != EVENT_CONTRACT_SCHEMA_V4
        {
            return Err(JournalError::RecoveryKeyMismatchV4);
        }
        let root_lock = acquire_v4_root_lock(root)?;
        let result = recover_new_v4_locked(root, key, provenance);
        drop(root_lock);
        result
    }

    /// Storage-only helper retained for low-level recovery fault tests. The
    /// public M5 path is `recover_replayed_v4_session`, which does not release
    /// any lock between this mutation and roots-bound replay.
    #[cfg(test)]
    fn recover_canonical_tail_v4(
        root: &StoreRoot,
        key: RecoveryKeyV4,
        provenance: RecoveryProvenanceV4,
    ) -> Result<RecoveryReceiptV4, JournalError> {
        if key.expected_kind != RecoveryKindV4::CanonicalTail
            || key.event_contract_version != EVENT_CONTRACT_SCHEMA_V4
        {
            return Err(JournalError::RecoveryKeyMismatchV4);
        }
        let root_lock = acquire_v4_root_lock(root)?;
        let result = recover_canonical_tail_v4_locked(root, key, provenance);
        drop(root_lock);
        result
    }

    /// Rechecks and mutates one inspected V4 recovery under root -> run ->
    /// journal locks, then performs the complete roots-bound Core replay while
    /// retaining those same locks. Canonical-tail recovery always returns an
    /// editable session; only a strict-interior M4 bundle is resume-only.
    pub fn recover_replayed_v4_session<'roots>(
        &self,
        roots: &'roots AuthorityTrustRootsV4,
        key: RecoveryKeyV4,
        provenance: RecoveryProvenanceV4,
    ) -> Result<(RecoveryReceiptV4, RecoveredV4Session<'a, 'roots>), JournalError> {
        if self.identity.version() != EventContractVersion::V4
            || !matches!(
                key.expected_kind,
                RecoveryKindV4::CanonicalTail | RecoveryKindV4::M4BundleResume
            )
            || key.event_contract_version != EVENT_CONTRACT_SCHEMA_V4
            || key.run_id != self.identity.run_id
            || key.genesis_hash != self.identity.genesis_hash()
        {
            return Err(JournalError::RecoveryKeyMismatchV4);
        }
        let timestamp_unix_seconds = v4_timestamp()?;
        let root_lock = acquire_v4_root_lock(self.root)?;
        let run_lock = acquire_v4_run_lock(&self.run)?;
        if key.expected_kind == RecoveryKindV4::CanonicalTail {
            return (|| {
                let fd = open_verified_log(&self.run, true)?;
                let mut file = File::from(fd);
                fs::flock(file.as_fd(), FlockOperation::LockExclusive).map_err(StoreError::Io)?;
                let JournalGenesis::V4Shared(genesis) = &self.identity.genesis else {
                    return Err(JournalError::Identity(
                        "V4 canonical-tail recovery requires verified genesis bytes",
                    ));
                };
                let observed = inspect_v4_file(
                    file.try_clone()?,
                    &key.run_id,
                    &key.genesis_hash,
                    genesis,
                    self.limits,
                )?;
                let append_pending =
                    marker_exists(&self.run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES)?;
                let pending_digest =
                    append_pending.then(|| ContentHash::sha256(APPEND_PENDING_BYTES));
                if bundle_file_exists(&self.run, BUNDLE_PENDING_MARKER)?
                    || bundle_file_exists(&self.run, BUNDLE_PENDING_STAGE)?
                    || !v4_key_matches(&key, &observed, pending_digest.as_ref())
                    || observed.event_count == 0
                {
                    return Err(JournalError::RecoveryKeyMismatchV4);
                }
                if !append_pending {
                    ensure_v4_append_pending(&self.run)?;
                }
                let discarded_hash = if observed.torn {
                    let discarded = read_suffix(
                        &mut file,
                        observed.good_offset,
                        self.limits.max_replay_bytes,
                    )?;
                    let outcome = RecoveryOutcomeV4::TailRecovered {
                        good_offset: observed.good_offset,
                        discarded_hash: ContentHash::sha256(&discarded),
                    };
                    file.set_len(observed.good_offset)?;
                    #[cfg(test)]
                    let injected = take_v4_recovery_fault(V4RecoveryFault::TailFile);
                    #[cfg(not(test))]
                    let injected = false;
                    if injected || file.sync_all().is_err() {
                        return Err(JournalError::RecoveryDurabilityUncertainV4 { outcome });
                    }
                    ContentHash::sha256(&discarded)
                } else {
                    ContentHash::sha256(&[])
                };
                let outcome = RecoveryOutcomeV4::TailRecovered {
                    good_offset: observed.good_offset,
                    discarded_hash,
                };
                fs::unlinkat(&self.run, APPEND_PENDING_MARKER, AtFlags::empty())
                    .map_err(StoreError::Io)?;
                #[cfg(test)]
                let injected = take_v4_recovery_fault(V4RecoveryFault::TailDirectory);
                #[cfg(not(test))]
                let injected = false;
                if injected || fs::fsync(&self.run).is_err() {
                    return Err(JournalError::RecoveryDurabilityUncertainV4 { outcome });
                }

                let state = scan(&mut file, &self.identity, self.limits, true)?;
                validate_prefix(&self.identity, &state.events)?;
                let (intents, completions) = self.recovery_dirs()?;
                let audit = recovery_audit(&intents, &completions, &self.identity, self.limits)?;
                sync_recovery_dirs(&intents, &completions)?;
                validate_completed_receipts(
                    &mut file,
                    &self.identity,
                    self.limits,
                    &audit.completed,
                )?;
                if let Some(intent) = audit.pending {
                    return Err(JournalError::CorruptNeedsRecovery {
                        good_offset: intent.good_offset,
                        auto_recoverable: true,
                    });
                }
                let resolver = JournalAuthorityResolverV4 {
                    reader: CasReader::open_existing(self.root)?,
                };
                let session_identity = OpaqueSessionIdentityV4::fresh();
                let (log, basis) = EventLogV4::replay_confirmed_v4_prefix_for_session(
                    self.identity.run_id.clone(),
                    genesis,
                    &state.events,
                    &resolver,
                    roots,
                    EventReplayLimits::new(self.limits.max_events, self.limits.max_replay_bytes),
                    &session_identity,
                )?;
                let index_projection = index_v5_projection_charge_from_log(
                    &log,
                    self.limits.max_replay_bytes,
                    state.confirmed_offset,
                )?;
                let post_file_hash =
                    ContentHash::sha256(&read_prefix(&mut file, state.confirmed_offset)?);
                let receipt = RecoveryReceiptV4 {
                    schema: RECOVERY_RECEIPT_SCHEMA_V4,
                    kind: key.expected_kind,
                    outcome,
                    pre_file_hash: Some(observed.file_hash),
                    post_file_hash: Some(post_file_hash),
                    key,
                    provenance,
                    timestamp_unix_seconds,
                    marker_recovery: None,
                };
                let writer = JournalWriter {
                    file,
                    identity: self.identity.clone(),
                    limits: self.limits,
                    state,
                    intents,
                    completions,
                    run: dup(&run_lock).map_err(StoreError::Io)?,
                    poisoned: false,
                    append_durability: AppendDurability::Confirmed,
                    #[cfg(any(test, feature = "test-support"))]
                    faults: std::collections::VecDeque::new(),
                };
                Ok((
                    receipt,
                    RecoveredV4Session::Editable {
                        session: ReplayedV4RunSession {
                            _root_lock: root_lock,
                            _run_lock: run_lock,
                            writer,
                            log,
                            index_genesis: decode_index_v5_genesis(genesis)?,
                            resolver,
                            roots,
                            session_identity,
                            state: ReplayedV4RunSessionState::Healthy,
                            index_projection: Some(index_projection),
                        },
                        basis,
                    },
                ))
            })();
        }
        (|| {
            if marker_exists(&self.run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES)? {
                return Err(JournalError::RecoveryKeyMismatchV4);
            }
            let fd = open_verified_log(&self.run, true)?;
            let mut file = File::from(fd);
            fs::flock(file.as_fd(), FlockOperation::LockExclusive).map_err(StoreError::Io)?;
            let JournalGenesis::V4Shared(genesis) = &self.identity.genesis else {
                return Err(JournalError::Identity(
                    "V4 M4 recovery requires verified genesis bytes",
                ));
            };
            let observed = inspect_v4_file(
                file.try_clone()?,
                &key.run_id,
                &key.genesis_hash,
                genesis,
                self.limits,
            )?;
            let pending = inspect_bundle_pending_file_locked(
                &self.run,
                &self.identity,
                self.limits,
                &mut file,
            )?
            .ok_or(JournalError::RecoveryKeyMismatchV4)?;
            if !pending.discarded.is_empty()
                || !v4_key_matches(&key, &observed, Some(&pending.marker_hash))
            {
                return Err(JournalError::RecoveryKeyMismatchV4);
            }
            let _ = read_bundle_stage(&self.run, pending.marker.expected_count)?;
            let store_stage = v4_bundle_prefix_stage(&pending)?;
            let (intents, completions) = self.recovery_dirs()?;
            let audit = recovery_audit(&intents, &completions, &self.identity, self.limits)?;
            sync_recovery_dirs(&intents, &completions)?;
            validate_completed_receipts(&mut file, &self.identity, self.limits, &audit.completed)?;
            if let Some(intent) = audit.pending {
                return Err(JournalError::CorruptNeedsRecovery {
                    good_offset: intent.good_offset,
                    auto_recoverable: true,
                });
            }
            let resolver = JournalAuthorityResolverV4 {
                reader: CasReader::open_existing(self.root)?,
            };
            let session_identity = OpaqueSessionIdentityV4::fresh();
            let recovery = EventLogV4::recover_verification_bundle_v4_for_session(
                self.identity.run_id.clone(),
                genesis,
                &pending.current_state.events,
                &pending.planned,
                &resolver,
                roots,
                EventReplayLimits::new(self.limits.max_events, self.limits.max_replay_bytes),
                &session_identity,
            )
            .map_err(map_bundle_resume_domain_error)?;
            if recovery.confirmed_bundle_events() != store_stage.confirmed_events
                || recovery.expected_bundle_events() != store_stage.expected_events
            {
                return Err(JournalError::BundleResumeAuthorityMismatch);
            }
            let pre_file_hash = observed.file_hash;
            let marker_hash = pending.marker_hash;
            let writer = JournalWriter {
                file,
                identity: self.identity.clone(),
                limits: self.limits,
                state: pending.current_state,
                intents,
                completions,
                run: dup(&run_lock).map_err(StoreError::Io)?,
                poisoned: false,
                append_durability: AppendDurability::Confirmed,
                #[cfg(any(test, feature = "test-support"))]
                faults: std::collections::VecDeque::new(),
            };
            match recovery {
                VerificationBundleRecoveryV4::Stage0 { log, basis, .. }
                | VerificationBundleRecoveryV4::AlreadyComplete { log, basis, .. } => {
                    if !matches!(
                        store_stage.classification,
                        M4BundlePrefixClassificationV4::Stage0
                            | M4BundlePrefixClassificationV4::AlreadyComplete
                    ) {
                        return Err(JournalError::BundleResumeAuthorityMismatch);
                    }
                    let outcome = RecoveryOutcomeV4::M4BundleCleanupOrdinary {
                        prefix_stage: store_stage,
                    };
                    clear_v4_bundle_marker(&self.run, &outcome)?;
                    let receipt = RecoveryReceiptV4 {
                        schema: RECOVERY_RECEIPT_SCHEMA_V4,
                        kind: key.expected_kind,
                        outcome,
                        pre_file_hash: Some(pre_file_hash.clone()),
                        post_file_hash: Some(pre_file_hash),
                        key,
                        provenance,
                        timestamp_unix_seconds,
                        marker_recovery: Some(M4BundleMarkerRecoveryReceiptV4 {
                            prefix_stage: store_stage,
                            action: M4BundleMarkerActionV4::ClearedAndSynced,
                            pre_marker_hash: marker_hash,
                            post_marker_hash: None,
                        }),
                    };
                    let index_projection = index_v5_projection_charge_from_log(
                        &log,
                        self.limits.max_replay_bytes,
                        writer.state.confirmed_offset,
                    )?;
                    Ok((
                        receipt,
                        RecoveredV4Session::Editable {
                            session: ReplayedV4RunSession {
                                _root_lock: root_lock,
                                _run_lock: run_lock,
                                writer,
                                log,
                                index_genesis: decode_index_v5_genesis(genesis)?,
                                resolver,
                                roots,
                                session_identity,
                                state: ReplayedV4RunSessionState::Healthy,
                                index_projection: Some(index_projection),
                            },
                            basis,
                        },
                    ))
                }
                VerificationBundleRecoveryV4::StrictInterior {
                    session: core_session,
                    authority,
                    ..
                } => {
                    if store_stage.classification != M4BundlePrefixClassificationV4::StrictInterior
                    {
                        return Err(JournalError::BundleResumeAuthorityMismatch);
                    }
                    let outcome = RecoveryOutcomeV4::M4BundleResumeRequired {
                        prefix_stage: store_stage,
                    };
                    let receipt = RecoveryReceiptV4 {
                        schema: RECOVERY_RECEIPT_SCHEMA_V4,
                        kind: key.expected_kind,
                        outcome,
                        pre_file_hash: Some(pre_file_hash.clone()),
                        post_file_hash: Some(pre_file_hash),
                        key,
                        provenance,
                        timestamp_unix_seconds,
                        marker_recovery: Some(M4BundleMarkerRecoveryReceiptV4 {
                            prefix_stage: store_stage,
                            action: M4BundleMarkerActionV4::RetainedForResume,
                            pre_marker_hash: marker_hash.clone(),
                            post_marker_hash: Some(marker_hash),
                        }),
                    };
                    Ok((
                        receipt,
                        RecoveredV4Session::M4BundleResumeRequired {
                            session: RecoveredM4BundleV4Session {
                                root_lock,
                                run_lock,
                                writer,
                                resolver,
                                roots,
                                session_identity,
                                core_session,
                                marker: pending.marker,
                                confirmed_events: pending.confirmed_events,
                            },
                            resume_authority: authority,
                        },
                    ))
                }
            }
        })()
    }

    fn inspect_bundle_pending_locked(
        &self,
        file: &mut File,
    ) -> Result<Option<BundlePendingInspection>, JournalError> {
        inspect_bundle_pending_file_locked(&self.run, &self.identity, self.limits, file)
    }

    fn clear_bundle_pending_after_receipt(
        &self,
        _marker: Option<&VerificationBundlePendingMarkerV3>,
        _confirmed: u64,
    ) -> Result<(), JournalError> {
        for name in [BUNDLE_PENDING_STAGE, BUNDLE_PENDING_MARKER] {
            if bundle_file_exists(&self.run, name)? {
                fs::unlinkat(&self.run, name, AtFlags::empty()).map_err(StoreError::Io)?;
            }
        }
        #[cfg(test)]
        if self.take_recovery_fault(RecoveryFault::BundleAfterMarkerUnlink) {
            // Recreate the exact gate before reporting the simulated lost
            // directory-fsync acknowledgement. The next recovery therefore
            // exercises the same durable receipt instead of silently
            // treating an unacknowledged cleanup as complete.
            if let Some(marker) = _marker {
                publish_bundle_file(&self.run, BUNDLE_PENDING_MARKER, &canonical_json(marker)?)?;
            }
            publish_bundle_file(
                &self.run,
                BUNDLE_PENDING_STAGE,
                format!("{_confirmed}\n").as_bytes(),
            )?;
            fs::fsync(&self.run).map_err(StoreError::Io)?;
            return Err(JournalError::Io(injected_io_error(
                "bundle recovery after marker unlink",
            )));
        }
        fs::fsync(&self.run).map_err(StoreError::Io)?;
        Ok(())
    }

    fn validate_pending_bundle_recovery_context(
        &self,
        file: &mut File,
        intent: &RecoveryIntent,
    ) -> Result<(), JournalError> {
        let name = receipt_name(&intent.recovery_id);
        let kind = intent
            .bundle_recovery_kind
            .as_deref()
            .ok_or_else(|| JournalError::ReceiptCorruption { name: name.clone() })?;
        if kind == "stage-only" {
            let expected_digest = ContentHash::sha256(b"reviewgraphen.bundle-stage-only.v1");
            let marker_absent =
                read_bundle_pending(&self.run, &self.identity, self.limits)?.is_none();
            let stage_present = bundle_file_exists(&self.run, BUNDLE_PENDING_STAGE)?;
            let actual_len = file.metadata()?.len();
            if !marker_absent
                || !stage_present
                || read_bundle_stage(&self.run, 3)? != 0
                || intent.bundle_digest.as_ref() != Some(&expected_digest)
                || intent.bundle_pre_offset != Some(intent.good_offset)
                || intent.discarded_offset.is_some()
                || intent.discarded_len.is_some()
                || intent.pre_size.is_some()
                || intent.discarded_hash != ContentHash::sha256(b"")
                || actual_len != intent.good_offset
            {
                return Err(JournalError::ReceiptCorruption { name });
            }
            let state = scan_with_torn(file, &self.identity, self.limits, true)?;
            if state.torn.is_some()
                || state.confirmed_offset != intent.good_offset
                || state.tail_hash != intent.pre_tail_hash
            {
                return Err(JournalError::ReceiptCorruption { name });
            }
            return Ok(());
        }

        let inspection = self
            .inspect_bundle_pending_locked(file)?
            .ok_or_else(|| JournalError::ReceiptCorruption { name: name.clone() })?;
        if intent.bundle_digest.as_ref() != Some(&inspection.marker.bundle_digest)
            || intent.bundle_pre_offset != Some(inspection.marker.pre_offset)
            || intent.good_offset != inspection.current_state.confirmed_offset
            || intent.pre_tail_hash != inspection.current_state.tail_hash
        {
            return Err(JournalError::ReceiptCorruption { name });
        }
        let actual_len = file.metadata()?.len();
        let expected_kind = if intent.discarded_len.is_some() {
            "torn"
        } else if inspection.confirmed_events == 0 {
            "confirmed-zero-cleanup"
        } else if inspection.confirmed_events == inspection.planned.len() {
            "already-complete-cleanup"
        } else {
            return Err(JournalError::ReceiptCorruption { name });
        };
        if kind != expected_kind {
            return Err(JournalError::ReceiptCorruption { name });
        }
        match (
            intent.discarded_offset,
            intent.discarded_len,
            intent.pre_size,
        ) {
            (Some(offset), Some(len), Some(pre_size)) => {
                if kind != "torn"
                    || offset != intent.good_offset
                    || len == 0
                    || offset.checked_add(len) != Some(pre_size)
                    || (actual_len != pre_size && actual_len != intent.good_offset)
                {
                    return Err(JournalError::ReceiptCorruption { name });
                }
                if actual_len == pre_size {
                    let suffix =
                        read_suffix(file, intent.good_offset, self.limits.max_replay_bytes)?;
                    if suffix.len() as u64 != len
                        || ContentHash::sha256(&suffix) != intent.discarded_hash
                    {
                        return Err(JournalError::ReceiptCorruption { name });
                    }
                }
            }
            (None, None, None) => {
                if kind == "torn"
                    || !inspection.discarded.is_empty()
                    || intent.discarded_hash != ContentHash::sha256(b"")
                    || actual_len != intent.good_offset
                {
                    return Err(JournalError::ReceiptCorruption { name });
                }
            }
            _ => return Err(JournalError::ReceiptCorruption { name }),
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn publish_bundle_recovery_receipt_locked(
        &self,
        file: &mut File,
        intents: &OwnedFd,
        completions: &OwnedFd,
        audit: &RecoveryAudit,
        state: &ScanState,
        discarded: &[u8],
        marker: Option<&VerificationBundlePendingMarkerV3>,
        recovery_kind: &'static str,
    ) -> Result<JournalRecoveryReceipt, JournalError> {
        let discarded_hash = ContentHash::sha256(discarded);
        let discarded_len =
            u64::try_from(discarded.len()).map_err(|_| JournalError::Incomplete {
                limit: self.limits.max_replay_bytes,
                observed: u64::MAX,
            })?;
        let bundle_digest = marker.map_or_else(
            || ContentHash::sha256(b"reviewgraphen.bundle-stage-only.v1"),
            |marker| marker.bundle_digest.clone(),
        );
        let bundle_pre_offset = marker.map_or(state.confirmed_offset, |marker| marker.pre_offset);
        let nonce = bundle_recovery_nonce(
            &self.identity,
            &bundle_digest,
            bundle_pre_offset,
            state.confirmed_offset,
            &discarded_hash,
            discarded_len,
            recovery_kind,
        )?;
        let recovery_id = recovery_id(
            &self.identity.run_id,
            &self.identity.genesis_hash(),
            state.confirmed_offset,
            &discarded_hash,
            &nonce,
        )?;
        let bundle_range = (discarded_len != 0).then_some((
            state.confirmed_offset,
            discarded_len,
            state
                .confirmed_offset
                .checked_add(discarded_len)
                .ok_or(JournalError::Incomplete {
                    limit: self.limits.max_replay_bytes,
                    observed: u64::MAX,
                })?,
        ));
        let intent = RecoveryIntent {
            recovery_id: recovery_id.clone(),
            run_id: self.identity.run_id.clone(),
            genesis_hash: self.identity.genesis_hash(),
            nonce,
            good_offset: state.confirmed_offset,
            discarded_hash,
            discarded_offset: bundle_range.map(|range| range.0),
            discarded_len: bundle_range.map(|range| range.1),
            pre_size: bundle_range.map(|range| range.2),
            bundle_digest: Some(bundle_digest),
            bundle_pre_offset: Some(bundle_pre_offset),
            bundle_recovery_kind: Some(recovery_kind.to_owned()),
            pre_tail_hash: state.tail_hash.clone(),
            actor: "reviewgraphen-store".to_owned(),
            tool_version: format!("bundle-recovery:{recovery_kind}"),
            timestamp_unix_seconds: 0,
        };
        let completion = RecoveryCompletion {
            recovery_id,
            post_file_hash: ContentHash::sha256(&read_prefix(file, state.confirmed_offset)?),
        };
        validate_proposed_recovery_lineage(audit, &intent)?;
        for (known_intent, known_completion) in &audit.completed {
            if same_bundle_recovery_lineage(known_intent, &intent)
                && known_completion != &completion
            {
                return Err(JournalError::ReceiptCorruption {
                    name: receipt_name(&known_intent.recovery_id),
                });
            }
        }
        let name = receipt_name(&intent.recovery_id);
        let existing_intent = read_receipt::<RecoveryIntent>(intents, &name, self.limits)?;
        let existing_completion =
            read_receipt::<RecoveryCompletion>(completions, &name, self.limits)?;
        if existing_completion.is_some() && existing_intent.is_none() {
            return Err(JournalError::ReceiptCorruption { name });
        }
        if existing_intent
            .as_ref()
            .is_some_and(|existing| existing != &intent)
            || existing_completion
                .as_ref()
                .is_some_and(|existing| existing != &completion)
        {
            return Err(JournalError::ReceiptCorruption { name });
        }
        let mut added = Vec::with_capacity(2);
        if existing_intent.is_none() {
            added.push(receipt_bytes(&intent)?);
        }
        if existing_completion.is_none() {
            added.push(receipt_bytes(&completion)?);
        }
        reserve_recovery_capacity(audit, self.limits, &added)?;
        if existing_intent.is_none() {
            publish_receipt_with_limits(intents, &name, &intent, self.limits)?;
            #[cfg(test)]
            if self.take_recovery_fault(RecoveryFault::BundleAfterIntent) {
                return Err(JournalError::Io(injected_io_error(
                    "bundle recovery after intent",
                )));
            }
        }
        // The immutable intent, including the exact discarded range and
        // pre-truncate size, is durable before the first log mutation.
        if let Some(pre_size) = intent.pre_size {
            let actual_len = file.metadata()?.len();
            if actual_len != pre_size && actual_len != state.confirmed_offset {
                return Err(JournalError::ReceiptCorruption { name });
            }
            if actual_len == pre_size {
                let suffix =
                    read_suffix(file, state.confirmed_offset, self.limits.max_replay_bytes)?;
                if suffix.len() as u64 != discarded_len
                    || ContentHash::sha256(&suffix) != intent.discarded_hash
                {
                    return Err(JournalError::ReceiptCorruption { name });
                }
                file.set_len(state.confirmed_offset)?;
                file.seek(SeekFrom::Start(state.confirmed_offset))?;
                file.sync_data()?;
                #[cfg(test)]
                if self.take_recovery_fault(RecoveryFault::BundleAfterTruncateSync) {
                    return Err(JournalError::Io(injected_io_error(
                        "bundle recovery after truncate sync",
                    )));
                }
            } else {
                file.sync_data()?;
            }
        }
        if existing_completion.is_none() {
            publish_receipt_with_limits(completions, &name, &completion, self.limits)?;
            #[cfg(test)]
            if self.take_recovery_fault(RecoveryFault::BundleAfterCompletion) {
                return Err(JournalError::Io(injected_io_error(
                    "bundle recovery after completion",
                )));
            }
        }
        Ok(JournalRecoveryReceipt {
            intent,
            completion,
            resumed: existing_intent.is_some(),
        })
    }

    /// Acquires the exclusive writer lock and returns a narrow, staged V2
    /// command session. No mutable EventLog or unchecked envelope escapes.
    pub fn replayed_v2_session(
        &self,
        admissions: &EventAdmissions,
    ) -> Result<ReplayedV2RunSession, JournalError> {
        let writer = self.writer()?;
        let JournalGenesis::V2Shared(genesis) = &writer.identity.genesis else {
            return Err(JournalError::V1ReadOnly);
        };
        let log = EventLog::replay_validated_v2_prefix(
            writer.identity.run_id.clone(),
            genesis,
            &writer.state.events,
            admissions,
            EventReplayLimits::new(writer.limits.max_events, writer.limits.max_replay_bytes),
        )?;
        Ok(ReplayedV2RunSession {
            writer,
            log,
            store_root_identity: self.root.identity().clone(),
            state: ReplayedV2RunSessionState::Healthy,
        })
    }

    /// Acquires the exclusive journal lock, replays the complete V3 prefix
    /// against exact CAS bytes and these host roots, and returns the opaque
    /// replay basis alongside the only writable V3 session surface.
    pub fn replayed_v3_session<'roots>(
        &self,
        roots: &'roots AuthorityTrustRootsV3,
    ) -> Result<(ReplayedV3RunSession<'a, 'roots>, AuthorityReplayBasisV3), JournalError> {
        if self.identity.version() != EventContractVersion::V3 {
            return Err(JournalError::Identity("V3 replay requires a V3 journal"));
        }
        let writer = self.writer_v3()?;
        let JournalGenesis::V3Shared(genesis) = &writer.identity.genesis else {
            return Err(JournalError::Identity(
                "V3 replay requires verified genesis bytes",
            ));
        };
        let resolver = JournalAuthorityResolverV3 {
            reader: CasReader::open_existing(self.root)?,
        };
        let (log, basis) = EventLog::replay_validated_v3_prefix(
            writer.identity.run_id.clone(),
            genesis,
            &writer.state.events,
            &resolver,
            roots,
            EventReplayLimits::new(writer.limits.max_events, writer.limits.max_replay_bytes),
        )?;
        Ok((
            ReplayedV3RunSession {
                writer,
                log,
                resolver,
                roots,
                store_root_identity: self.root.identity().clone(),
                state: ReplayedV3RunSessionState::Healthy,
            },
            basis,
        ))
    }

    /// Acquires the target journal lock and structurally replays the complete
    /// homogeneous V5 prefix. The result is crate-private and carries no M6
    /// append authority until paired with the locked source/index closure.
    pub(crate) fn replayed_v5_target_session(&self) -> Result<ReplayedV5RunSession, JournalError> {
        if self.identity.version() != EventContractVersion::V5 {
            return Err(JournalError::Identity("V5 replay requires a V5 journal"));
        }
        let run_lock = acquire_v4_run_lock(&self.run)?;
        let writer = self.writer_v5()?;
        replay_v5_locked(self.root, run_lock, writer)
    }

    fn replayed_incremental_pair_v5<'roots>(
        &self,
        target: &EventJournal<'a>,
        roots: &'roots AuthorityTrustRootsV4,
    ) -> Result<
        (
            ReplayedV4RunSession<'a, 'roots>,
            AuthorityReplayBasisV4,
            ReplayedV5RunSession,
        ),
        JournalError,
    > {
        if self.identity.version() != EventContractVersion::V4
            || target.identity.version() != EventContractVersion::V5
            || self.root.identity() != target.root.identity()
            || self.identity.run_id == target.identity.run_id
        {
            return Err(JournalError::Identity(
                "incremental pair requires distinct V4 source and V5 target in one StoreRoot",
            ));
        }
        let root_lock = acquire_v4_root_lock(self.root)?;
        let (source_run_lock, source_writer, target_run_lock, target_writer) =
            if self.identity.run_id < target.identity.run_id {
                let source_run_lock = acquire_v4_run_lock(&self.run)?;
                let source_writer = self.writer_v4()?;
                let target_run_lock = acquire_v4_run_lock(&target.run)?;
                let target_writer = target.writer_v5()?;
                (
                    source_run_lock,
                    source_writer,
                    target_run_lock,
                    target_writer,
                )
            } else {
                let target_run_lock = acquire_v4_run_lock(&target.run)?;
                let target_writer = target.writer_v5()?;
                let source_run_lock = acquire_v4_run_lock(&self.run)?;
                let source_writer = self.writer_v4()?;
                (
                    source_run_lock,
                    source_writer,
                    target_run_lock,
                    target_writer,
                )
            };

        let JournalGenesis::V4Shared(source_genesis) = &source_writer.identity.genesis else {
            return Err(JournalError::Identity(
                "incremental source requires verified V4 genesis bytes",
            ));
        };
        admit_incremental_source_journal_bytes(source_writer.state.confirmed_offset)?;
        let resolver = JournalAuthorityResolverV4 {
            reader: CasReader::open_existing(self.root)?,
        };
        let session_identity = OpaqueSessionIdentityV4::fresh();
        let mut index_projection = crate::index::ReplayProjectionChargeV5::default();
        let mut projection_error = None;
        let (source_log, source_basis) =
            EventLogV4::replay_confirmed_v4_prefix_for_session_with_projection_visitor(
                source_writer.identity.run_id.clone(),
                source_genesis,
                &source_writer.state.events,
                &resolver,
                roots,
                EventReplayLimits::new(
                    source_writer.limits.max_events,
                    source_writer.limits.max_replay_bytes,
                ),
                &session_identity,
                |metadata, payload| {
                    if projection_error.is_none()
                        && let Err(error) = index_projection.observe(metadata, payload)
                    {
                        projection_error = Some(error);
                    }
                },
            )?;
        if projection_error.is_some() {
            return Err(JournalError::Incomplete {
                limit: source_writer.limits.max_replay_bytes,
                observed: u64::MAX,
            });
        }
        let index_genesis = decode_index_v5_genesis(source_genesis)?;
        let source_session = ReplayedV4RunSession {
            _root_lock: root_lock,
            _run_lock: source_run_lock,
            writer: source_writer,
            log: source_log,
            index_genesis,
            resolver,
            roots,
            session_identity,
            state: ReplayedV4RunSessionState::Healthy,
            index_projection: Some(index_projection),
        };
        let target_session = replay_v5_locked(self.root, target_run_lock, target_writer)?;
        Ok((source_session, source_basis, target_session))
    }

    /// Runs Core's inspection-only M5 profile derivation and the augmented
    /// full replay under one root -> run -> journal exclusive lock interval.
    /// The callback receives no raw roots or trusted-source values; those
    /// capabilities can only be consumed through the narrow operation seam.
    pub fn with_m5_gluing_profile_session<T, F>(
        &self,
        base_roots: AuthorityTrustRootsV4,
        assignments: M5DoubleSubmitAssignmentsV4,
        operation: F,
    ) -> Result<T, JournalError>
    where
        F: for<'session, 'roots> FnOnce(
            &mut M5GluingProfileSessionV4<'session, 'a, 'roots>,
        ) -> Result<T, JournalError>,
    {
        if self.identity.version() != EventContractVersion::V4 {
            return Err(JournalError::Identity(
                "M5 profile inspection requires a V4 journal",
            ));
        }
        let root_lock = acquire_v4_root_lock(self.root)?;
        let run_lock = acquire_v4_run_lock(&self.run)?;
        let writer = self.writer_v4()?;
        match run_locked_m5_profile_session(
            self.root,
            root_lock,
            run_lock,
            writer,
            base_roots,
            assignments,
            operation,
        )? {
            LockedM5ProfileSessionResult::Continued(value) => Ok(value),
            LockedM5ProfileSessionResult::AlreadyComplete { .. } => Err(JournalError::Domain(
                reviewgraphen_core::DomainError::AlreadyComplete,
            )),
        }
    }

    /// Revalidates one already-complete fixed profile without exposing the
    /// augmented roots, replay basis, or editable session used internally.
    /// The returned value is descriptive and source-bound to the confirmed
    /// journal/CAS prefix.
    pub fn inspect_completed_m5_gluing_profile_v4(
        &self,
        base_roots: AuthorityTrustRootsV4,
        assignments: M5DoubleSubmitAssignmentsV4,
    ) -> Result<M5CompletedGluingProfileV4, JournalError> {
        if self.identity.version() != EventContractVersion::V4 {
            return Err(JournalError::Identity(
                "M5 profile inspection requires a V4 journal",
            ));
        }
        let root_lock = acquire_v4_root_lock(self.root)?;
        let run_lock = acquire_v4_run_lock(&self.run)?;
        let writer = self.writer_v4()?;
        match run_locked_m5_profile_session(
            self.root,
            root_lock,
            run_lock,
            writer,
            base_roots,
            assignments,
            |_| Ok(()),
        )? {
            LockedM5ProfileSessionResult::AlreadyComplete { completed, .. } => Ok(*completed),
            LockedM5ProfileSessionResult::Continued(()) => Err(JournalError::M5ProfileIncompleteV4),
        }
    }

    /// Replays the fixed M5 profile from base host roots and returns either an
    /// opaque completed authority for Report V4 or the exact legal durable
    /// 0/1/2 gluing-input prefix count. Augmented roots never escape.
    pub fn inspect_m5_report_authority_v4(
        &self,
        base_roots: AuthorityTrustRootsV4,
        assignments: M5DoubleSubmitAssignmentsV4,
    ) -> Result<M5ReportAuthorityInspectionV4, JournalError> {
        if self.identity.version() != EventContractVersion::V4 {
            return Err(JournalError::Identity(
                "M5 report inspection requires a V4 journal",
            ));
        }
        let root_lock = acquire_v4_root_lock(self.root)?;
        let run_lock = acquire_v4_run_lock(&self.run)?;
        let writer = self.writer_v4()?;
        match run_locked_m5_profile_session(
            self.root,
            root_lock,
            run_lock,
            writer,
            base_roots,
            assignments,
            |profile| {
                let remaining = u64::try_from(profile.remaining_input_count()).map_err(|_| {
                    JournalError::Identity("M5 report input count does not fit u64")
                })?;
                2_u64.checked_sub(remaining).ok_or(JournalError::Identity(
                    "M5 report input count exceeds the closed two-input profile",
                ))
            },
        )? {
            LockedM5ProfileSessionResult::AlreadyComplete {
                completed, roots, ..
            } => Ok(M5ReportAuthorityInspectionV4::Complete(Box::new(
                CompletedM5ReportAuthorityV4 {
                    roots,
                    completed: *completed,
                },
            ))),
            LockedM5ProfileSessionResult::Continued(registered_inputs) => {
                Ok(M5ReportAuthorityInspectionV4::Incomplete { registered_inputs })
            }
        }
    }

    /// Rebuilds and re-reads V5 from the same exact completed profile inputs.
    /// This cross-crate assertion seam is intentionally available only to
    /// integration tests; augmented roots remain private.
    #[cfg(feature = "test-support")]
    pub fn completed_m5_v5_snapshot_for_test_support(
        &self,
        base_roots: AuthorityTrustRootsV4,
        assignments: M5DoubleSubmitAssignmentsV4,
    ) -> Result<(M5CompletedGluingProfileV4, crate::IndexSnapshotV5), crate::IndexError> {
        if self.identity.version() != EventContractVersion::V4 {
            return Err(
                JournalError::Identity("M5 profile inspection requires a V4 journal").into(),
            );
        }
        let root_lock = acquire_v4_root_lock(self.root)?;
        let run_lock = acquire_v4_run_lock(&self.run)?;
        let writer = self.writer_v4()?;
        let (completed, roots) = match run_locked_m5_profile_session(
            self.root,
            root_lock,
            run_lock,
            writer,
            base_roots,
            assignments,
            |_| Ok(()),
        )? {
            LockedM5ProfileSessionResult::AlreadyComplete { completed, roots } => {
                (*completed, roots)
            }
            LockedM5ProfileSessionResult::Continued(()) => {
                return Err(JournalError::M5ProfileIncompleteV4.into());
            }
        };
        let index = crate::DerivedIndexV5::open(self.root)?;
        index.rebuild_v5(self, &roots)?;
        let snapshot = index.snapshot_current_v5(self, &roots)?;
        Ok((completed, snapshot))
    }

    /// Recovers one keyed canonical tail and, without releasing any lock,
    /// derives fresh profile roots, fully replays, and runs the narrow M5
    /// callback. A failed durability sync remains recovery-required.
    pub fn recover_with_m5_gluing_profile_session<T, F>(
        &self,
        base_roots: AuthorityTrustRootsV4,
        assignments: M5DoubleSubmitAssignmentsV4,
        key: RecoveryKeyV4,
        provenance: RecoveryProvenanceV4,
        operation: F,
    ) -> Result<M5GluingProfileRecoveryV4<T>, JournalError>
    where
        F: for<'session, 'roots> FnOnce(
            &mut M5GluingProfileSessionV4<'session, 'a, 'roots>,
        ) -> Result<T, JournalError>,
    {
        if self.identity.version() != EventContractVersion::V4
            || key.expected_kind != RecoveryKindV4::CanonicalTail
            || key.event_contract_version != EVENT_CONTRACT_SCHEMA_V4
            || key.run_id != self.identity.run_id
            || key.genesis_hash != self.identity.genesis_hash()
        {
            return Err(JournalError::RecoveryKeyMismatchV4);
        }
        let timestamp_unix_seconds = v4_timestamp()?;
        let root_lock = acquire_v4_root_lock(self.root)?;
        let run_lock = acquire_v4_run_lock(&self.run)?;
        let fd = open_verified_log(&self.run, true)?;
        let mut file = File::from(fd);
        fs::flock(file.as_fd(), FlockOperation::LockExclusive).map_err(StoreError::Io)?;
        let JournalGenesis::V4Shared(genesis) = &self.identity.genesis else {
            return Err(JournalError::Identity(
                "V4 canonical-tail recovery requires verified genesis bytes",
            ));
        };
        let observed = inspect_v4_file(
            file.try_clone()?,
            &key.run_id,
            &key.genesis_hash,
            genesis,
            self.limits,
        )?;
        let append_pending = marker_exists(&self.run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES)?;
        let pending_digest = append_pending.then(|| ContentHash::sha256(APPEND_PENDING_BYTES));
        if bundle_file_exists(&self.run, BUNDLE_PENDING_MARKER)?
            || bundle_file_exists(&self.run, BUNDLE_PENDING_STAGE)?
            || !v4_key_matches(&key, &observed, pending_digest.as_ref())
            || observed.event_count == 0
        {
            return Err(JournalError::RecoveryKeyMismatchV4);
        }
        if !append_pending {
            ensure_v4_append_pending(&self.run)?;
        }
        let discarded_hash = if observed.torn {
            let discarded = read_suffix(
                &mut file,
                observed.good_offset,
                self.limits.max_replay_bytes,
            )?;
            let outcome = RecoveryOutcomeV4::TailRecovered {
                good_offset: observed.good_offset,
                discarded_hash: ContentHash::sha256(&discarded),
            };
            file.set_len(observed.good_offset)?;
            #[cfg(test)]
            let injected = take_v4_recovery_fault(V4RecoveryFault::TailFile);
            #[cfg(not(test))]
            let injected = false;
            if injected || file.sync_all().is_err() {
                return Err(JournalError::RecoveryDurabilityUncertainV4 { outcome });
            }
            ContentHash::sha256(&discarded)
        } else {
            ContentHash::sha256(&[])
        };
        let outcome = RecoveryOutcomeV4::TailRecovered {
            good_offset: observed.good_offset,
            discarded_hash,
        };
        fs::unlinkat(&self.run, APPEND_PENDING_MARKER, AtFlags::empty()).map_err(StoreError::Io)?;
        #[cfg(test)]
        let injected = take_v4_recovery_fault(V4RecoveryFault::TailDirectory);
        #[cfg(not(test))]
        let injected = false;
        if injected || fs::fsync(&self.run).is_err() {
            return Err(JournalError::RecoveryDurabilityUncertainV4 { outcome });
        }
        let state = scan(&mut file, &self.identity, self.limits, true)?;
        validate_prefix(&self.identity, &state.events)?;
        let (intents, completions) = self.recovery_dirs()?;
        let audit = recovery_audit(&intents, &completions, &self.identity, self.limits)?;
        sync_recovery_dirs(&intents, &completions)?;
        validate_completed_receipts(&mut file, &self.identity, self.limits, &audit.completed)?;
        if let Some(intent) = audit.pending {
            return Err(JournalError::CorruptNeedsRecovery {
                good_offset: intent.good_offset,
                auto_recoverable: true,
            });
        }
        let post_file_hash = ContentHash::sha256(&read_prefix(&mut file, state.confirmed_offset)?);
        let receipt = RecoveryReceiptV4 {
            schema: RECOVERY_RECEIPT_SCHEMA_V4,
            kind: key.expected_kind,
            outcome,
            pre_file_hash: Some(observed.file_hash),
            post_file_hash: Some(post_file_hash),
            key,
            provenance,
            timestamp_unix_seconds,
            marker_recovery: None,
        };
        let writer = JournalWriter {
            file,
            identity: self.identity.clone(),
            limits: self.limits,
            state,
            intents,
            completions,
            run: dup(&run_lock).map_err(StoreError::Io)?,
            poisoned: false,
            append_durability: AppendDurability::Confirmed,
            #[cfg(any(test, feature = "test-support"))]
            faults: std::collections::VecDeque::new(),
        };
        let value = run_locked_m5_profile_session(
            self.root,
            root_lock,
            run_lock,
            writer,
            base_roots,
            assignments,
            operation,
        )?;
        Ok(match value {
            LockedM5ProfileSessionResult::Continued(value) => {
                M5GluingProfileRecoveryV4::Continued {
                    recovery: receipt,
                    value,
                }
            }
            LockedM5ProfileSessionResult::AlreadyComplete { completed, .. } => {
                M5GluingProfileRecoveryV4::AlreadyComplete {
                    recovery: receipt,
                    completed: *completed,
                }
            }
        })
    }

    /// Acquires the exclusive journal lock and rebuilds one V4 replay basis
    /// from the complete confirmed prefix, CAS objects, and exact host roots.
    /// A pending bundle is recovery-only and is never admitted here.
    pub fn replayed_v4_session<'roots>(
        &self,
        roots: &'roots AuthorityTrustRootsV4,
    ) -> Result<(ReplayedV4RunSession<'a, 'roots>, AuthorityReplayBasisV4), JournalError> {
        if self.identity.version() != EventContractVersion::V4 {
            return Err(JournalError::Identity("V4 replay requires a V4 journal"));
        }
        let root_lock = acquire_v4_root_lock(self.root)?;
        let run_lock = acquire_v4_run_lock(&self.run)?;
        let writer = self.writer_v4()?;
        let JournalGenesis::V4Shared(genesis) = &writer.identity.genesis else {
            return Err(JournalError::Identity(
                "V4 replay requires verified genesis bytes",
            ));
        };
        let resolver = JournalAuthorityResolverV4 {
            reader: CasReader::open_existing(self.root)?,
        };
        let session_identity = OpaqueSessionIdentityV4::fresh();
        let mut index_projection = crate::index::ReplayProjectionChargeV5::default();
        let mut projection_error = None;
        let (log, basis) =
            EventLogV4::replay_confirmed_v4_prefix_for_session_with_projection_visitor(
                writer.identity.run_id.clone(),
                genesis,
                &writer.state.events,
                &resolver,
                roots,
                EventReplayLimits::new(writer.limits.max_events, writer.limits.max_replay_bytes),
                &session_identity,
                |metadata, payload| {
                    if projection_error.is_none()
                        && let Err(error) = index_projection.observe(metadata, payload)
                    {
                        projection_error = Some(error);
                    }
                },
            )?;
        if projection_error.is_some() {
            return Err(JournalError::Incomplete {
                limit: writer.limits.max_replay_bytes,
                observed: u64::MAX,
            });
        }
        let index_genesis = decode_index_v5_genesis(genesis)?;
        Ok((
            ReplayedV4RunSession {
                _root_lock: root_lock,
                _run_lock: run_lock,
                writer,
                log,
                index_genesis,
                resolver,
                roots,
                session_identity,
                state: ReplayedV4RunSessionState::Healthy,
                index_projection: Some(index_projection),
            },
            basis,
        ))
    }

    /// Recovers a partially durable verification bundle under one exclusive
    /// lock and asks core to seal the exact missing suffix. Stage zero and an
    /// already-complete plan never produce resume authority.
    pub fn recover_verification_bundle_resume<'roots>(
        &self,
        roots: &'roots AuthorityTrustRootsV3,
    ) -> Result<
        (
            RecoveredVerificationBundleV3Session<'a, 'roots>,
            AuthorityReplayBasisV3,
            VerificationBundleResumeAuthorityV3,
        ),
        JournalError,
    > {
        if self.identity.version() != EventContractVersion::V3 {
            return Err(JournalError::Identity("V3 recovery requires a V3 journal"));
        }
        let fd = self.open_file(true)?;
        fs::flock(&fd, FlockOperation::LockExclusive).map_err(StoreError::Io)?;
        let mut file = File::from(fd);
        if marker_exists(&self.run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES)? {
            return Err(JournalError::SessionUncertain);
        }
        let (intents, completions) = self.recovery_dirs()?;
        let audit = recovery_audit(&intents, &completions, &self.identity, self.limits)?;
        sync_recovery_dirs(&intents, &completions)?;
        validate_completed_receipts(&mut file, &self.identity, self.limits, &audit.completed)?;
        if let Some(intent) = audit.pending {
            return Err(JournalError::CorruptNeedsRecovery {
                good_offset: intent.good_offset,
                auto_recoverable: true,
            });
        }
        let inspection = self
            .inspect_bundle_pending_locked(&mut file)?
            .ok_or(JournalError::BundleResumeAuthorityMismatch)?;
        if inspection.confirmed_events == 0
            || inspection.confirmed_events >= inspection.planned.len()
        {
            return Err(JournalError::BundleResumeAuthorityMismatch);
        }
        let confirmed = u64::try_from(inspection.confirmed_events)
            .map_err(|_| JournalError::BundleResumeAuthorityMismatch)?;
        if !inspection.discarded.is_empty() {
            let _ = self.publish_bundle_recovery_receipt_locked(
                &mut file,
                &intents,
                &completions,
                &audit,
                &inspection.current_state,
                &inspection.discarded,
                Some(&inspection.marker),
                "torn",
            )?;
        }
        persist_bundle_stage_file(&self.run, confirmed)?;

        let JournalGenesis::V3Shared(genesis) = &self.identity.genesis else {
            return Err(JournalError::Identity(
                "V3 replay requires verified genesis bytes",
            ));
        };
        let resolver = JournalAuthorityResolverV3 {
            reader: CasReader::open_existing(self.root)?,
        };
        let replay_limits =
            EventReplayLimits::new(self.limits.max_events, self.limits.max_replay_bytes);
        let (pre_log, pre_basis) = EventLog::replay_validated_v3_prefix(
            self.identity.run_id.clone(),
            genesis,
            &inspection.pre_state.events,
            &resolver,
            roots,
            replay_limits,
        )?;
        let (current_log, current_basis) = EventLog::replay_validated_v3_prefix(
            self.identity.run_id.clone(),
            genesis,
            &inspection.current_state.events,
            &resolver,
            roots,
            replay_limits,
        )?;
        let authority = EventLog::recover_verification_bundle_resume_authority_v3(
            &pre_log,
            &pre_basis,
            &current_log,
            &current_basis,
            &inspection.planned,
            &resolver,
            roots,
        )
        .map_err(map_bundle_resume_domain_error)?;
        let writer = JournalWriter {
            file,
            identity: self.identity.clone(),
            limits: self.limits,
            state: inspection.current_state,
            intents,
            completions,
            run: dup(&self.run).map_err(StoreError::Io)?,
            poisoned: false,
            append_durability: AppendDurability::Confirmed,
            #[cfg(any(test, feature = "test-support"))]
            faults: std::collections::VecDeque::new(),
        };
        Ok((
            RecoveredVerificationBundleV3Session {
                session: ReplayedV3RunSession {
                    writer,
                    log: current_log,
                    resolver,
                    roots,
                    store_root_identity: self.root.identity().clone(),
                    state: ReplayedV3RunSessionState::Healthy,
                },
                marker: inspection.marker,
                confirmed_events: inspection.confirmed_events,
            },
            current_basis,
            authority,
        ))
    }
    pub fn open(root: &'a StoreRoot, identity: JournalIdentity) -> Result<Self, JournalError> {
        Self::open_with_limits(root, identity, JournalLimits::from_store(root.limits()))
    }
    pub fn open_with_limits(
        root: &'a StoreRoot,
        identity: JournalIdentity,
        limits: JournalLimits,
    ) -> Result<Self, JournalError> {
        validate_limits(limits)?;
        let runs = open_existing_dir(root.fd(), RUNS_DIR, "runs directory")?;
        let run_name = run_dir_name(&identity.run_id);
        let run = open_existing_dir(&runs, &run_name, "run directory")?;
        Ok(Self {
            root,
            identity,
            limits,
            run,
            #[cfg(test)]
            recovery_faults: Mutex::new(std::collections::VecDeque::new()),
        })
    }

    /// The only create path for a durable V2 stream.  It creates every
    /// component one-at-a-time, validates the first envelope as the complete
    /// one-event chain, and only then makes a non-empty log durable.
    pub fn initialize_v2(
        root: &'a StoreRoot,
        identity: JournalIdentity,
        first_manifest_envelope: EventEnvelope,
    ) -> Result<Self, JournalError> {
        Self::initialize_v2_with_limits(
            root,
            identity,
            first_manifest_envelope,
            JournalLimits::from_store(root.limits()),
        )
    }

    pub fn initialize_v2_with_limits(
        root: &'a StoreRoot,
        identity: JournalIdentity,
        first_manifest_envelope: EventEnvelope,
        limits: JournalLimits,
    ) -> Result<Self, JournalError> {
        if identity.version() != EventContractVersion::V2 {
            return Err(JournalError::V1ReadOnly);
        }
        validate_limits(limits)?;
        validate_prefix(&identity, std::slice::from_ref(&first_manifest_envelope))?;
        let mut line = first_manifest_envelope.canonical_bytes()?;
        line.push(b'\n');
        limit(line.len() as u64, limits.max_event_line_bytes)?;
        limit(1, limits.max_events)?;
        limit(line.len() as u64, limits.max_replay_bytes)?;

        let runs = open_or_create_dir(root.fd(), RUNS_DIR, "runs directory")?;
        // A run identity is a validated StableId, and is used only as this
        // one descriptor-relative component (never a path expression).
        // Stable IDs permit '/' in their payload.  Never make that input a
        // filesystem component: a full SHA-256 digest is the sole run-dir
        // name; the manifest event remains the authoritative run binding.
        let run_name = run_dir_name(&identity.run_id);
        let run = open_or_create_dir(&runs, &run_name, "run directory")?;
        let recovery = open_or_create_dir(&run, RECOVERY_DIR, "run recovery directory")?;
        let _ = open_or_create_dir(&recovery, INTENTS_DIR, "recovery intent directory")?;
        let _ = open_or_create_dir(&recovery, COMPLETIONS_DIR, "recovery completion directory")?;
        publish_initial_log(&run, &line, &identity, limits)?;
        Self::open_with_limits(root, identity, limits)
    }

    /// Creates a V3 journal only after strict canonical genesis validation and
    /// validation of the complete sequence-one manifest prefix.
    pub fn initialize_v3(
        root: &'a StoreRoot,
        identity: JournalIdentity,
        first_manifest_envelope: EventEnvelope,
    ) -> Result<Self, JournalError> {
        Self::initialize_v3_with_limits(
            root,
            identity,
            first_manifest_envelope,
            JournalLimits::from_store(root.limits()),
        )
    }

    pub fn initialize_v3_with_limits(
        root: &'a StoreRoot,
        identity: JournalIdentity,
        first_manifest_envelope: EventEnvelope,
        limits: JournalLimits,
    ) -> Result<Self, JournalError> {
        if identity.version() != EventContractVersion::V3 {
            return Err(JournalError::Identity(
                "V3 initialization requires V3 genesis",
            ));
        }
        validate_limits(limits)?;
        validate_prefix(&identity, std::slice::from_ref(&first_manifest_envelope))?;
        let mut line = first_manifest_envelope.canonical_bytes()?;
        line.push(b'\n');
        limit(
            u64::try_from(line.len()).map_err(|_| JournalError::Incomplete {
                limit: limits.max_event_line_bytes,
                observed: u64::MAX,
            })?,
            limits.max_event_line_bytes,
        )?;
        limit(1, limits.max_events)?;
        limit(
            u64::try_from(line.len()).map_err(|_| JournalError::Incomplete {
                limit: limits.max_replay_bytes,
                observed: u64::MAX,
            })?,
            limits.max_replay_bytes,
        )?;
        let runs = open_or_create_dir(root.fd(), RUNS_DIR, "runs directory")?;
        let run = open_or_create_dir(&runs, &run_dir_name(&identity.run_id), "run directory")?;
        let recovery = open_or_create_dir(&run, RECOVERY_DIR, "run recovery directory")?;
        let _ = open_or_create_dir(&recovery, INTENTS_DIR, "recovery intent directory")?;
        let _ = open_or_create_dir(&recovery, COMPLETIONS_DIR, "recovery completion directory")?;
        publish_initial_log(&run, &line, &identity, limits)?;
        Self::open_with_limits(root, identity, limits)
    }

    /// Constructs a validated immutable V1 fixture only for store contract
    /// tests. Production V1 streams are imported/read-only; this helper never
    /// exposes an append path and validates the complete supplied prefix
    /// before publishing the fixed JSONL bytes.
    #[cfg(test)]
    pub(crate) fn initialize_v1_for_test(
        root: &'a StoreRoot,
        identity: JournalIdentity,
        events: &[EventEnvelope],
    ) -> Result<Self, JournalError> {
        if identity.version() != EventContractVersion::V1 {
            return Err(JournalError::V1ReadOnly);
        }
        let limits = JournalLimits::from_store(root.limits());
        validate_limits(limits)?;
        validate_prefix(&identity, events)?;
        let mut bytes = Vec::new();
        for event in events {
            let mut line = event.canonical_bytes()?;
            line.push(b'\n');
            limit(
                u64::try_from(line.len()).map_err(|_| JournalError::Incomplete {
                    limit: limits.max_event_line_bytes,
                    observed: u64::MAX,
                })?,
                limits.max_event_line_bytes,
            )?;
            bytes.extend_from_slice(&line);
        }
        limit(
            u64::try_from(events.len()).map_err(|_| JournalError::Incomplete {
                limit: limits.max_events,
                observed: u64::MAX,
            })?,
            limits.max_events,
        )?;
        limit(
            u64::try_from(bytes.len()).map_err(|_| JournalError::Incomplete {
                limit: limits.max_replay_bytes,
                observed: u64::MAX,
            })?,
            limits.max_replay_bytes,
        )?;
        let runs = open_or_create_dir(root.fd(), RUNS_DIR, "runs directory")?;
        let run = open_or_create_dir(&runs, &run_dir_name(&identity.run_id), "run directory")?;
        let recovery = open_or_create_dir(&run, RECOVERY_DIR, "run recovery directory")?;
        let _ = open_or_create_dir(&recovery, INTENTS_DIR, "recovery intent directory")?;
        let _ = open_or_create_dir(&recovery, COMPLETIONS_DIR, "recovery completion directory")?;
        publish_initial_log(&run, &bytes, &identity, limits)?;
        Self::open_with_limits(root, identity, limits)
    }

    pub fn reader(&self) -> Result<JournalReader, JournalError> {
        self.reader_with_working_limit(None)
    }

    /// Opens the index-only streaming reader. Unlike the public materialized
    /// reader, this
    /// path never materializes the raw file, an envelope vector, a start
    /// offset vector, a `ValidatedEventView`, or a torn suffix. New derived
    /// index code must use this entry point.
    #[allow(dead_code)] // Kept beside legacy reader_for_index until index migration lands.
    pub(crate) fn index_reader(
        &self,
        working_limit: u64,
    ) -> Result<IndexJournalReader, JournalError> {
        if let Some(bytes) = self.identity.v2_genesis_backing() {
            reviewgraphen_core::preflight_index_genesis_json_structure(bytes)
                .map_err(map_bounded_domain_error)?;
        }
        let fd = self.open_file(false)?;
        fs::flock(&fd, FlockOperation::LockShared).map_err(StoreError::Io)?;
        let file = File::from(fd);
        self.refuse_pending_markers()?;
        let (intents, completions) = self.recovery_dirs()?;
        let genesis = self
            .identity
            .v2_genesis_backing()
            .map_or(0, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        let original_identity = self.identity.cloned_local_metadata_capacity()?;
        let future_index_identity = original_identity;
        let reader_identity = u64::try_from(
            self.identity
                .local_metadata_capacity()
                .checked_add(self.identity.shared_certificate_capacity())
                .ok_or(JournalError::Incomplete {
                    limit: working_limit,
                    observed: u64::MAX,
                })?,
        )
        .map_err(|_| JournalError::Incomplete {
            limit: working_limit,
            observed: u64::MAX,
        })?;
        let canonical_scratch = self.limits.max_event_line_bytes.saturating_sub(1);
        let fixed = genesis
            .checked_add(original_identity)
            .and_then(|value| value.checked_add(future_index_identity))
            .and_then(|value| value.checked_add(reader_identity))
            .and_then(|value| value.checked_add(self.limits.max_event_line_bytes))
            .and_then(|value| value.checked_add(canonical_scratch))
            .ok_or(JournalError::Incomplete {
                limit: working_limit,
                observed: u64::MAX,
            })?;
        limit(fixed, working_limit)?;
        let mut receipt_count = 0;
        let mut receipt_bytes = 0;
        let (intent_count, intent_max) = index_receipt_inventory(
            &intents,
            self.limits,
            &mut receipt_count,
            &mut receipt_bytes,
        )?;
        let (completion_count, completion_max) = index_receipt_inventory(
            &completions,
            self.limits,
            &mut receipt_count,
            &mut receipt_bytes,
        )?;
        preflight_index_recovery_audit(
            intent_count,
            completion_count,
            intent_max.max(completion_max),
            receipt_bytes,
            fixed,
            working_limit,
        )?;
        // Directory stream offsets belong to the open file description and
        // are shared by dup(2). Reopen both directories before the exact-name
        // pass so a non-empty, already inventoried receipt set cannot appear
        // empty merely because the first bounded scan reached EOF.
        drop(intents);
        drop(completions);
        let (intents, completions) = self.recovery_dirs()?;
        let intent_names = receipt_names_exact(&intents, self.limits, intent_count)?;
        let completion_names = receipt_names_exact(&completions, self.limits, completion_count)?;
        let audit = recovery_audit_from_names(
            &intents,
            &completions,
            &self.identity,
            self.limits,
            intent_names,
            completion_names,
            receipt_count,
            receipt_bytes,
        )?;
        sync_recovery_dirs(&intents, &completions)?;
        if let Some(intent) = audit.pending {
            if self.identity.version() != EventContractVersion::V1 && intent.good_offset == 0 {
                return Err(JournalError::V2GenesisRequired);
            }
            return Err(JournalError::CorruptNeedsRecovery {
                good_offset: intent.good_offset,
                auto_recoverable: true,
            });
        }
        let reader = IndexJournalReader {
            file,
            identity: self.identity.clone(),
            limits: self.limits,
            working_limit,
            completed_receipts: audit.completed,
        };
        let requested = reader
            .retained_metadata_capacity()?
            .checked_add(original_identity)
            .and_then(|value| value.checked_add(future_index_identity))
            .and_then(|value| value.checked_add(genesis))
            .and_then(|value| value.checked_add(self.limits.max_event_line_bytes))
            .and_then(|value| value.checked_add(canonical_scratch))
            .ok_or(JournalError::Incomplete {
                limit: working_limit,
                observed: u64::MAX,
            })?;
        limit(requested, working_limit)?;
        Ok(reader)
    }

    fn reader_with_working_limit(
        &self,
        working_limit: Option<u64>,
    ) -> Result<JournalReader, JournalError> {
        let fd = self.open_file(false)?;
        fs::flock(&fd, FlockOperation::LockShared).map_err(StoreError::Io)?;
        let mut file = File::from(fd);
        self.refuse_pending_markers()?;
        let (intents, completions) = self.recovery_dirs()?;
        let audit = recovery_audit(&intents, &completions, &self.identity, self.limits)?;
        sync_recovery_dirs(&intents, &completions)?;
        if let Some(intent) = audit.pending.clone() {
            if self.identity.version() != EventContractVersion::V1 && intent.good_offset == 0 {
                return Err(JournalError::V2GenesisRequired);
            }
            return Err(JournalError::CorruptNeedsRecovery {
                good_offset: intent.good_offset,
                auto_recoverable: true,
            });
        }
        validate_completed_receipts(&mut file, &self.identity, self.limits, &audit.completed)?;
        let state =
            scan_with_working_limit(&mut file, &self.identity, self.limits, true, working_limit)?;
        Ok(JournalReader {
            file,
            identity: self.identity.clone(),
            limits: self.limits,
            state,
        })
    }
    pub fn writer(&self) -> Result<JournalWriter, JournalError> {
        match self.identity.version() {
            EventContractVersion::V1 => return Err(JournalError::V1ReadOnly),
            EventContractVersion::V3 => return Err(JournalError::V3ReplaySessionRequired),
            EventContractVersion::V4 => return Err(JournalError::V4ReplaySessionRequired),
            EventContractVersion::V5 => return Err(JournalError::V5ReplaySessionRequired),
            EventContractVersion::V2 => {}
        }
        self.writer_locked()
    }

    fn writer_v3(&self) -> Result<JournalWriter, JournalError> {
        if self.identity.version() != EventContractVersion::V3 {
            return Err(JournalError::Identity("V3 writer requires a V3 journal"));
        }
        self.writer_locked()
    }

    fn writer_v4(&self) -> Result<JournalWriter, JournalError> {
        if self.identity.version() != EventContractVersion::V4 {
            return Err(JournalError::Identity("V4 writer requires a V4 journal"));
        }
        self.writer_locked()
    }

    fn writer_v5(&self) -> Result<JournalWriter, JournalError> {
        if self.identity.version() != EventContractVersion::V5 {
            return Err(JournalError::Identity("V5 writer requires a V5 journal"));
        }
        self.writer_locked()
    }

    fn writer_locked(&self) -> Result<JournalWriter, JournalError> {
        let fd = self.open_file(true)?;
        fs::flock(&fd, FlockOperation::LockExclusive).map_err(StoreError::Io)?;
        let mut file = File::from(fd);
        self.refuse_pending_markers()?;
        let (intents, completions) = self.recovery_dirs()?;
        let audit = recovery_audit(&intents, &completions, &self.identity, self.limits)?;
        sync_recovery_dirs(&intents, &completions)?;
        validate_completed_receipts(&mut file, &self.identity, self.limits, &audit.completed)?;
        if let Some(intent) = audit.pending.clone() {
            return Err(JournalError::CorruptNeedsRecovery {
                good_offset: intent.good_offset,
                auto_recoverable: true,
            });
        }
        let state = scan(&mut file, &self.identity, self.limits, true)?;
        Ok(JournalWriter {
            file,
            identity: self.identity.clone(),
            limits: self.limits,
            state,
            intents,
            completions,
            run: dup(&self.run).map_err(StoreError::Io)?,
            poisoned: false,
            append_durability: AppendDurability::Confirmed,
            #[cfg(any(test, feature = "test-support"))]
            faults: std::collections::VecDeque::new(),
        })
    }

    /// Runs canonical tail recovery and then reopens a roots-bound V3
    /// session. The reopen necessarily revalidates the entire recovered
    /// prefix and all authority CAS objects before returning state.
    pub fn recover_replayed_v3_session<'roots>(
        &self,
        actor: impl Into<String>,
        tool_version: impl Into<String>,
        roots: &'roots AuthorityTrustRootsV3,
    ) -> Result<
        (
            JournalRecoveryReceipt,
            ReplayedV3RunSession<'a, 'roots>,
            AuthorityReplayBasisV3,
        ),
        JournalError,
    > {
        if self.identity.version() != EventContractVersion::V3 {
            return Err(JournalError::Identity("V3 recovery requires a V3 journal"));
        }
        let actor = actor.into();
        let tool_version = tool_version.into();
        validate_recovery_actor(&actor, &tool_version)?;
        let fd = self.open_file(true)?;
        fs::flock(&fd, FlockOperation::LockExclusive).map_err(StoreError::Io)?;
        let mut file = File::from(fd);
        let (intents, completions) = self.recovery_dirs()?;
        let receipt =
            self.recover_with_locked_file(&mut file, &intents, &completions, actor, tool_version)?;

        // The same open-file-description lock remains held through the
        // recovered-prefix audit, roots/CAS replay, and session construction.
        self.refuse_pending_markers()?;
        let audit = recovery_audit(&intents, &completions, &self.identity, self.limits)?;
        sync_recovery_dirs(&intents, &completions)?;
        validate_completed_receipts(&mut file, &self.identity, self.limits, &audit.completed)?;
        if let Some(intent) = audit.pending {
            return Err(JournalError::CorruptNeedsRecovery {
                good_offset: intent.good_offset,
                auto_recoverable: true,
            });
        }
        let state = scan(&mut file, &self.identity, self.limits, true)?;
        let writer = JournalWriter {
            file,
            identity: self.identity.clone(),
            limits: self.limits,
            state,
            intents,
            completions,
            run: dup(&self.run).map_err(StoreError::Io)?,
            poisoned: false,
            append_durability: AppendDurability::Confirmed,
            #[cfg(any(test, feature = "test-support"))]
            faults: std::collections::VecDeque::new(),
        };
        let JournalGenesis::V3Shared(genesis) = &writer.identity.genesis else {
            return Err(JournalError::Identity(
                "V3 replay requires verified genesis bytes",
            ));
        };
        let resolver = JournalAuthorityResolverV3 {
            reader: CasReader::open_existing(self.root)?,
        };
        let (log, basis) = EventLog::replay_validated_v3_prefix(
            writer.identity.run_id.clone(),
            genesis,
            &writer.state.events,
            &resolver,
            roots,
            EventReplayLimits::new(writer.limits.max_events, writer.limits.max_replay_bytes),
        )?;
        Ok((
            receipt,
            ReplayedV3RunSession {
                writer,
                log,
                resolver,
                roots,
                store_root_identity: self.root.identity().clone(),
                state: ReplayedV3RunSessionState::Healthy,
            },
            basis,
        ))
    }
    pub fn recover(
        &self,
        actor: impl Into<String>,
        tool_version: impl Into<String>,
    ) -> Result<JournalRecoveryReceipt, JournalError> {
        match self.identity.version() {
            EventContractVersion::V1 => return Err(JournalError::V1ReadOnly),
            EventContractVersion::V4 => return Err(JournalError::V4ReplaySessionRequired),
            EventContractVersion::V5 => return Err(JournalError::V5ReplaySessionRequired),
            EventContractVersion::V2 | EventContractVersion::V3 => {}
        }
        let actor = actor.into();
        let tool_version = tool_version.into();
        validate_recovery_actor(&actor, &tool_version)?;
        let fd = self.open_file(true)?;
        fs::flock(&fd, FlockOperation::LockExclusive).map_err(StoreError::Io)?;
        let mut file = File::from(fd);
        let (intents, completions) = self.recovery_dirs()?;
        self.recover_with_locked_file(&mut file, &intents, &completions, actor, tool_version)
    }

    #[allow(clippy::needless_borrow)] // Preserves the moved legacy recovery body verbatim.
    fn recover_with_locked_file(
        &self,
        mut file: &mut File,
        intents: &OwnedFd,
        completions: &OwnedFd,
        actor: String,
        tool_version: String,
    ) -> Result<JournalRecoveryReceipt, JournalError> {
        let audit = recovery_audit(&intents, &completions, &self.identity, self.limits)?;
        sync_recovery_dirs(&intents, &completions)?;
        validate_completed_receipts(&mut file, &self.identity, self.limits, &audit.completed)?;
        if let Some(intent) = audit.pending.clone() {
            if intent.run_id != self.identity.run_id
                || intent.genesis_hash != self.identity.genesis_hash()
            {
                return Err(JournalError::ReceiptCorruption {
                    name: receipt_name(&intent.recovery_id),
                });
            }
            if intent.bundle_recovery_kind.is_none() {
                let active_bundle_good =
                    if let Some(inspection) = self.inspect_bundle_pending_locked(file)? {
                        Some((
                            inspection.current_state.confirmed_offset,
                            inspection.marker.bundle_digest,
                        ))
                    } else if bundle_file_exists(&self.run, BUNDLE_PENDING_STAGE)? {
                        let state = scan_with_torn(file, &self.identity, self.limits, true)?;
                        Some((
                            state.confirmed_offset,
                            ContentHash::sha256(b"reviewgraphen.bundle-stage-only.v1"),
                        ))
                    } else {
                        None
                    };
                if let Some((good_offset, digest)) = active_bundle_good {
                    let mut active_lineage = intent.clone();
                    active_lineage.good_offset = good_offset;
                    active_lineage.bundle_digest = Some(digest);
                    if recovery_inventory_conflicts(&intent, &active_lineage) {
                        return Err(JournalError::ReceiptCorruption {
                            name: receipt_name(&intent.recovery_id),
                        });
                    }
                }
            }
            if intent.bundle_recovery_kind.is_some() {
                self.validate_pending_bundle_recovery_context(file, &intent)?;
            }
            let actual_len = file.metadata()?.len();
            limit(actual_len, self.limits.max_replay_bytes)?;
            if actual_len < intent.good_offset {
                return Err(JournalError::ReceiptCorruption {
                    name: receipt_name(&intent.recovery_id),
                });
            }
            if let Some(pre_size) = intent.pre_size
                && actual_len != pre_size
                && actual_len != intent.good_offset
            {
                return Err(JournalError::ReceiptCorruption {
                    name: receipt_name(&intent.recovery_id),
                });
            }
            let prefix = read_prefix(&mut file, intent.good_offset)?;
            let prefix_state = scan_bytes(&prefix, &self.identity, self.limits, true)?;
            if prefix_state.torn.is_some()
                || prefix_state.confirmed_offset != intent.good_offset
                || prefix_state.tail_hash != intent.pre_tail_hash
            {
                return Err(JournalError::ReceiptCorruption {
                    name: receipt_name(&intent.recovery_id),
                });
            }
            let completion = RecoveryCompletion {
                recovery_id: intent.recovery_id.clone(),
                post_file_hash: ContentHash::sha256(&prefix),
            };
            reserve_recovery_capacity(&audit, self.limits, &[receipt_bytes(&completion)?])?;
            if actual_len > intent.good_offset {
                let suffix =
                    read_suffix(&mut file, intent.good_offset, self.limits.max_replay_bytes)?;
                if ContentHash::sha256(&suffix) != intent.discarded_hash
                    || intent
                        .discarded_len
                        .is_some_and(|expected| expected != suffix.len() as u64)
                {
                    return Err(JournalError::ReceiptCorruption {
                        name: receipt_name(&intent.recovery_id),
                    });
                }
                file.set_len(intent.good_offset)?;
                file.seek(SeekFrom::Start(intent.good_offset))?;
                #[cfg(test)]
                if self.take_recovery_fault(RecoveryFault::MarkerLogSync) {
                    return Err(JournalError::Io(injected_io_error(
                        "marker recovery log sync",
                    )));
                }
                file.sync_data()?;
            } else {
                // A failed rollback can already have shortened the log while
                // losing its durability acknowledgement. Its receipt binds
                // the original discarded bytes, not an imaginary empty tail.
                file.sync_data()?;
            }
            publish_receipt_with_limits(
                &completions,
                &receipt_name(&completion.recovery_id),
                &completion,
                self.limits,
            )?;
            #[cfg(test)]
            if intent.bundle_recovery_kind.is_some()
                && self.take_recovery_fault(RecoveryFault::BundleAfterCompletion)
            {
                return Err(JournalError::Io(injected_io_error(
                    "bundle recovery after completion",
                )));
            }
            match intent.bundle_recovery_kind.as_deref() {
                Some("stage-only") => {
                    if read_bundle_pending(&self.run, &self.identity, self.limits)?.is_some()
                        || read_bundle_stage(&self.run, 3)? != 0
                    {
                        return Err(JournalError::ReceiptCorruption {
                            name: BUNDLE_PENDING_STAGE.to_owned(),
                        });
                    }
                    self.clear_bundle_pending_after_receipt(None, 0)?;
                }
                Some(kind @ ("torn" | "confirmed-zero-cleanup" | "already-complete-cleanup")) => {
                    let inspection =
                        self.inspect_bundle_pending_locked(file)?.ok_or_else(|| {
                            JournalError::ReceiptCorruption {
                                name: BUNDLE_PENDING_MARKER.to_owned(),
                            }
                        })?;
                    if intent.bundle_digest.as_ref() != Some(&inspection.marker.bundle_digest)
                        || intent.bundle_pre_offset != Some(inspection.marker.pre_offset)
                    {
                        return Err(JournalError::ReceiptCorruption {
                            name: receipt_name(&intent.recovery_id),
                        });
                    }
                    let confirmed = u64::try_from(inspection.confirmed_events)
                        .map_err(|_| JournalError::BundleResumeAuthorityMismatch)?;
                    persist_bundle_stage_file(&self.run, confirmed)?;
                    let cleanup_expected = match kind {
                        "confirmed-zero-cleanup" => inspection.confirmed_events == 0,
                        "already-complete-cleanup" => {
                            inspection.confirmed_events == inspection.planned.len()
                        }
                        "torn" => {
                            inspection.confirmed_events == 0
                                || inspection.confirmed_events == inspection.planned.len()
                        }
                        _ => unreachable!(),
                    };
                    if cleanup_expected {
                        self.clear_bundle_pending_after_receipt(
                            Some(&inspection.marker),
                            confirmed,
                        )?;
                    } else if kind != "torn" {
                        return Err(JournalError::ReceiptCorruption {
                            name: receipt_name(&intent.recovery_id),
                        });
                    }
                }
                Some(_) => {
                    return Err(JournalError::ReceiptCorruption {
                        name: receipt_name(&intent.recovery_id),
                    });
                }
                None => {}
            }
            self.clear_append_marker_if_present()?;
            return Ok(JournalRecoveryReceipt {
                intent,
                completion,
                resumed: true,
            });
        }
        if read_bundle_pending(&self.run, &self.identity, self.limits)?.is_none()
            && bundle_file_exists(&self.run, BUNDLE_PENDING_STAGE)?
        {
            if read_bundle_stage(&self.run, 3)? != 0 {
                return Err(JournalError::ReceiptCorruption {
                    name: BUNDLE_PENDING_STAGE.to_owned(),
                });
            }
            let stage_only = scan_with_torn(file, &self.identity, self.limits, true)?;
            if stage_only.torn.is_some() {
                return Err(JournalError::ReceiptCorruption {
                    name: BUNDLE_PENDING_STAGE.to_owned(),
                });
            }
            let receipt = self.publish_bundle_recovery_receipt_locked(
                file,
                intents,
                completions,
                &audit,
                &stage_only,
                &[],
                None,
                "stage-only",
            )?;
            self.clear_bundle_pending_after_receipt(None, 0)?;
            return Ok(receipt);
        }
        if let Some(inspection) = self.inspect_bundle_pending_locked(file)? {
            let confirmed = u64::try_from(inspection.confirmed_events).unwrap_or(u64::MAX);
            let partial = inspection.confirmed_events > 0
                && inspection.confirmed_events < inspection.planned.len();
            if partial && inspection.discarded.is_empty() {
                // The authoritative journal already ends on a confirmed
                // event boundary. Re-deriving an advisory stage performs no
                // log mutation and must not mint a new recovery receipt on
                // every operator retry.
                persist_bundle_stage_file(&self.run, confirmed)?;
                return Err(JournalError::BundleAppendInterrupted {
                    durable_stage: VerificationBundleDurableStageV3 {
                        confirmed_events: confirmed,
                        expected_events: inspection.marker.expected_count,
                    },
                });
            }
            let recovery_kind = if !inspection.discarded.is_empty() {
                "torn"
            } else if inspection.confirmed_events == 0 {
                "confirmed-zero-cleanup"
            } else if inspection.confirmed_events == inspection.planned.len() {
                "already-complete-cleanup"
            } else {
                return Err(JournalError::ReceiptCorruption {
                    name: BUNDLE_PENDING_MARKER.to_owned(),
                });
            };
            let receipt = self.publish_bundle_recovery_receipt_locked(
                file,
                intents,
                completions,
                &audit,
                &inspection.current_state,
                &inspection.discarded,
                Some(&inspection.marker),
                recovery_kind,
            )?;
            // A torn suffix reaches this point only after its exact intent,
            // truncate sync, and completion are durable.
            persist_bundle_stage_file(&self.run, confirmed)?;
            if inspection.confirmed_events == 0
                || inspection.confirmed_events == inspection.planned.len()
            {
                self.clear_bundle_pending_after_receipt(Some(&inspection.marker), confirmed)?;
                return Ok(receipt);
            }
            return Err(JournalError::BundleAppendInterrupted {
                durable_stage: VerificationBundleDurableStageV3 {
                    confirmed_events: confirmed,
                    expected_events: inspection.marker.expected_count,
                },
            });
        }
        // If a process died after durable append but before unlinking the
        // pre-marker, the log itself is a complete valid prefix.  Receipt and
        // completion make clearing that gate just as auditable as a truncate.
        if marker_exists(&self.run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES)? {
            let marker_state = match scan_with_torn(&mut file, &self.identity, self.limits, true) {
                Ok(state) => state,
                Err(error @ JournalError::CorruptNeedsRecovery { .. }) => return Err(error),
                Err(error) => return Err(error),
            };
            if marker_state.torn.is_none() {
                #[cfg(test)]
                if self.take_recovery_fault(RecoveryFault::MarkerLogSync) {
                    return Err(JournalError::Io(injected_io_error(
                        "marker recovery log sync",
                    )));
                }
                file.sync_data()?;
                let nonce = recovery_nonce()?;
                let discarded_hash = ContentHash::sha256(b"");
                let recovery_id = recovery_id(
                    &self.identity.run_id,
                    &self.identity.genesis_hash(),
                    marker_state.confirmed_offset,
                    &discarded_hash,
                    &nonce,
                )?;
                let intent = RecoveryIntent {
                    recovery_id: recovery_id.clone(),
                    run_id: self.identity.run_id.clone(),
                    genesis_hash: self.identity.genesis_hash(),
                    nonce,
                    good_offset: marker_state.confirmed_offset,
                    discarded_hash,
                    discarded_offset: None,
                    discarded_len: None,
                    pre_size: None,
                    bundle_digest: None,
                    bundle_pre_offset: None,
                    bundle_recovery_kind: None,
                    pre_tail_hash: marker_state.tail_hash,
                    actor: actor.clone(),
                    tool_version: tool_version.clone(),
                    timestamp_unix_seconds: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| JournalError::Identity("system time before Unix epoch"))?
                        .as_secs(),
                };
                validate_proposed_recovery_lineage(&audit, &intent)?;
                let completion = RecoveryCompletion {
                    recovery_id: intent.recovery_id.clone(),
                    post_file_hash: ContentHash::sha256(&read_prefix(
                        &mut file,
                        intent.good_offset,
                    )?),
                };
                reserve_recovery_capacity(
                    &audit,
                    self.limits,
                    &[receipt_bytes(&intent)?, receipt_bytes(&completion)?],
                )?;
                publish_receipt_with_limits(
                    &intents,
                    &receipt_name(&intent.recovery_id),
                    &intent,
                    self.limits,
                )?;
                publish_receipt_with_limits(
                    &completions,
                    &receipt_name(&completion.recovery_id),
                    &completion,
                    self.limits,
                )?;
                self.clear_append_marker_if_present()?;
                return Ok(JournalRecoveryReceipt {
                    intent,
                    completion,
                    resumed: false,
                });
            }
        }
        let state = match scan_with_torn(&mut file, &self.identity, self.limits, true) {
            Ok(state) => state,
            Err(error @ JournalError::CorruptNeedsRecovery { .. }) => return Err(error),
            Err(error) => return Err(error),
        };
        let Some(torn) = state.torn else {
            return Err(JournalError::CorruptNeedsRecovery {
                good_offset: state.confirmed_offset,
                auto_recoverable: false,
            });
        };
        let discarded_hash = ContentHash::sha256(&torn.discarded);
        let nonce = recovery_nonce()?;
        let recovery_id = recovery_id(
            &self.identity.run_id,
            &self.identity.genesis_hash(),
            torn.good_offset,
            &discarded_hash,
            &nonce,
        )?;
        let proposed_intent = RecoveryIntent {
            recovery_id: recovery_id.clone(),
            run_id: self.identity.run_id.clone(),
            genesis_hash: self.identity.genesis_hash(),
            nonce,
            good_offset: torn.good_offset,
            discarded_hash,
            discarded_offset: None,
            discarded_len: None,
            pre_size: None,
            bundle_digest: None,
            bundle_pre_offset: None,
            bundle_recovery_kind: None,
            pre_tail_hash: torn.pre_tail_hash,
            actor,
            tool_version,
            timestamp_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| JournalError::Identity("system time before Unix epoch"))?
                .as_secs(),
        };
        validate_proposed_recovery_lineage(&audit, &proposed_intent)?;
        let proposed_completion = RecoveryCompletion {
            recovery_id,
            post_file_hash: ContentHash::sha256(&read_prefix(&mut file, torn.good_offset)?),
        };
        reserve_recovery_capacity(
            &audit,
            self.limits,
            &[
                receipt_bytes(&proposed_intent)?,
                receipt_bytes(&proposed_completion)?,
            ],
        )?;
        let intent_name = receipt_name(&proposed_intent.recovery_id);
        let completion_name = receipt_name(&proposed_completion.recovery_id);
        let (intent, resumed) =
            match read_receipt::<RecoveryIntent>(&intents, &intent_name, self.limits)? {
                Some(existing) => {
                    if existing.recovery_id != proposed_intent.recovery_id
                        || existing.good_offset != proposed_intent.good_offset
                        || existing.discarded_hash != proposed_intent.discarded_hash
                        || existing.pre_tail_hash != proposed_intent.pre_tail_hash
                    {
                        return Err(JournalError::ReceiptCorruption { name: intent_name });
                    }
                    (existing, true)
                }
                None => {
                    publish_receipt_with_limits(
                        &intents,
                        &intent_name,
                        &proposed_intent,
                        self.limits,
                    )?;
                    (proposed_intent, false)
                }
            };
        file.set_len(torn.good_offset)?;
        file.seek(SeekFrom::Start(torn.good_offset))?;
        file.sync_data()?;
        let completion = match read_receipt::<RecoveryCompletion>(
            &completions,
            &completion_name,
            self.limits,
        )? {
            Some(existing) if existing == proposed_completion => existing,
            Some(_) => {
                return Err(JournalError::ReceiptCorruption {
                    name: completion_name,
                });
            }
            None => {
                publish_receipt_with_limits(
                    &completions,
                    &completion_name,
                    &proposed_completion,
                    self.limits,
                )?;
                proposed_completion
            }
        };
        self.clear_append_marker_if_present()?;
        Ok(JournalRecoveryReceipt {
            intent,
            completion,
            resumed,
        })
    }
    fn open_file(&self, writable: bool) -> Result<OwnedFd, JournalError> {
        open_verified_log(&self.run, writable)
    }
    fn refuse_pending_markers(&self) -> Result<(), JournalError> {
        if read_bundle_pending(&self.run, &self.identity, self.limits)?.is_some()
            || bundle_file_exists(&self.run, BUNDLE_PENDING_STAGE)?
        {
            return Err(JournalError::SessionResumeRequired);
        }
        if marker_exists(&self.run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES)? {
            return Err(JournalError::CorruptNeedsRecovery {
                good_offset: 0,
                auto_recoverable: true,
            });
        }
        Ok(())
    }
    fn clear_append_marker_if_present(&self) -> Result<(), JournalError> {
        if marker_exists(&self.run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES)? {
            fs::unlinkat(&self.run, APPEND_PENDING_MARKER, AtFlags::empty())
                .map_err(StoreError::Io)?;
            #[cfg(test)]
            if self.take_recovery_fault(RecoveryFault::ClearMarkerDirectorySync) {
                // Restore the durable gate before reporting a directory-sync
                // acknowledgement failure; no new reader/writer may mistake
                // an uncertain unlink for a committed state transition.
                restore_append_marker(&self.run)?;
                return Err(JournalError::Io(injected_io_error(
                    "marker recovery directory sync",
                )));
            }
            fs::fsync(&self.run).map_err(StoreError::Io)?;
        }
        Ok(())
    }
    fn recovery_dirs(&self) -> Result<(OwnedFd, OwnedFd), JournalError> {
        let recovery = open_existing_dir(&self.run, RECOVERY_DIR, "run recovery directory")?;
        Ok((
            open_existing_dir(&recovery, INTENTS_DIR, "recovery intent directory")?,
            open_existing_dir(&recovery, COMPLETIONS_DIR, "recovery completion directory")?,
        ))
    }
    #[cfg(test)]
    fn inject_recovery_faults(&self, faults: impl IntoIterator<Item = RecoveryFault>) {
        self.recovery_faults.lock().unwrap().extend(faults);
    }
    #[cfg(test)]
    fn take_recovery_fault(&self, expected: RecoveryFault) -> bool {
        let mut faults = self.recovery_faults.lock().unwrap();
        faults.front().copied() == Some(expected) && faults.pop_front().is_some()
    }
}

/// Owns a distinct open-file description for the store root.  `dup` is not
/// sufficient here because flock state is shared by duplicated descriptors.
/// Dropping this guard closes the distinct description and releases the lock.
struct V4RootLock {
    _fd: OwnedFd,
}

fn open_v4_root_lock_fd(root: &StoreRoot) -> Result<OwnedFd, JournalError> {
    let fd = fs::openat(
        root.fd(),
        ".",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(StoreError::Io)?;
    verify_fd_kind_mode(&fd, "event-v4 root lock", FileType::Directory, 0o700)?;
    let anchor = fs::fstat(root.fd()).map_err(StoreError::Io)?;
    let opened = fs::fstat(&fd).map_err(StoreError::Io)?;
    if anchor.st_dev != opened.st_dev || anchor.st_ino != opened.st_ino {
        return Err(JournalError::Identity(
            "event-v4 root lock identity changed",
        ));
    }
    Ok(fd)
}

fn acquire_v4_root_lock(root: &StoreRoot) -> Result<V4RootLock, JournalError> {
    let fd = open_v4_root_lock_fd(root)?;
    fs::flock(&fd, FlockOperation::LockExclusive).map_err(StoreError::Io)?;
    Ok(V4RootLock { _fd: fd })
}

fn acquire_v4_run_lock(run: &OwnedFd) -> Result<OwnedFd, JournalError> {
    let fd = fs::openat(
        run,
        ".",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(StoreError::Io)?;
    verify_fd_kind_mode(&fd, "event-v4 run lock", FileType::Directory, 0o700)?;
    let anchor = fs::fstat(run).map_err(StoreError::Io)?;
    let opened = fs::fstat(&fd).map_err(StoreError::Io)?;
    if anchor.st_dev != opened.st_dev || anchor.st_ino != opened.st_ino {
        return Err(JournalError::Identity("event-v4 run lock identity changed"));
    }
    fs::flock(&fd, FlockOperation::LockExclusive).map_err(StoreError::Io)?;
    Ok(fd)
}

#[cfg(test)]
fn try_acquire_v4_root_lock(root: &StoreRoot) -> Result<Option<V4RootLock>, JournalError> {
    let fd = open_v4_root_lock_fd(root)?;
    match fs::flock(&fd, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(Some(V4RootLock { _fd: fd })),
        Err(Errno::WOULDBLOCK) => Ok(None),
        Err(error) => Err(JournalError::Store(StoreError::Io(error))),
    }
}

struct V4RecoveryObservation {
    good_offset: u64,
    tail_hash: ContentHash,
    file_hash: ContentHash,
    torn: bool,
    event_count: usize,
    first_event_id: Option<StableId>,
    first_event_hash: Option<ContentHash>,
}

fn v4_absent_recovery_key(inspection: RecoveryInspectionV4) -> RecoveryKeyV4 {
    RecoveryKeyV4 {
        pre_recovery_tail_hash: v4_chain_genesis_hash(&inspection.run_id, &inspection.genesis_hash),
        run_id: inspection.run_id,
        genesis_hash: inspection.genesis_hash,
        event_contract_version: inspection.event_contract_version,
        expected_kind: inspection.expected_kind,
        pre_recovery_offset: 0,
        pre_recovery_file_hash: None,
        pending_digest: None,
    }
}

fn v4_chain_genesis_hash(run_id: &StableId, genesis_hash: &ContentHash) -> ContentHash {
    let bindings = std::collections::BTreeMap::from([
        (
            "domain".to_owned(),
            serde_json::Value::String("reviewgraphen.event_chain_genesis.v1".to_owned()),
        ),
        (
            "genesis_hash".to_owned(),
            serde_json::Value::String(genesis_hash.to_string()),
        ),
        (
            "run".to_owned(),
            serde_json::Value::String(run_id.to_string()),
        ),
    ]);
    ContentHash::sha256(
        &canonical_json(&bindings).expect("fixed V4 chain genesis serializes canonically"),
    )
}

fn inspect_v4_file(
    file: File,
    run_id: &StableId,
    genesis_hash: &ContentHash,
    canonical_genesis_bytes: &[u8],
    limits: JournalLimits,
) -> Result<V4RecoveryObservation, JournalError> {
    let mut bytes = Vec::new();
    file.take(limits.max_replay_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    limit(
        u64::try_from(bytes.len()).map_err(|_| JournalError::Incomplete {
            limit: limits.max_replay_bytes,
            observed: u64::MAX,
        })?,
        limits.max_replay_bytes,
    )?;
    let file_hash = ContentHash::sha256(&bytes);
    let mut events = Vec::new();
    let mut start = 0usize;
    let mut good_offset = 0u64;
    while let Some(relative) = bytes[start..].iter().position(|byte| *byte == b'\n') {
        let end = start + relative;
        let line_len = end
            .checked_sub(start)
            .and_then(|value| value.checked_add(1))
            .ok_or(JournalError::Incomplete {
                limit: limits.max_event_line_bytes,
                observed: u64::MAX,
            })?;
        limit(
            u64::try_from(line_len).map_err(|_| JournalError::Incomplete {
                limit: limits.max_event_line_bytes,
                observed: u64::MAX,
            })?,
            limits.max_event_line_bytes,
        )?;
        let next_count = u64::try_from(events.len())
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or(JournalError::Incomplete {
                limit: limits.max_events,
                observed: u64::MAX,
            })?;
        limit(next_count, limits.max_events)?;
        let event = EventEnvelope::from_json_slice(&bytes[start..end]).map_err(|_| {
            JournalError::CorruptNeedsRecovery {
                good_offset,
                auto_recoverable: false,
            }
        })?;
        if event.contract_version()? != EventContractVersion::V4
            || event.run_id() != run_id
            || event.genesis_hash() != genesis_hash
            || event.canonical_bytes()? != bytes[start..end]
        {
            return Err(JournalError::CorruptNeedsRecovery {
                good_offset,
                auto_recoverable: false,
            });
        }
        events.push(event);
        start = end + 1;
        good_offset = u64::try_from(start).map_err(|_| JournalError::Incomplete {
            limit: limits.max_replay_bytes,
            observed: u64::MAX,
        })?;
    }
    if start < bytes.len() {
        limit(
            u64::try_from(bytes.len() - start).map_err(|_| JournalError::Incomplete {
                limit: limits.max_event_line_bytes,
                observed: u64::MAX,
            })?,
            limits.max_event_line_bytes,
        )?;
    }
    if EventEnvelope::validate_v4_stream(run_id, canonical_genesis_bytes, &events).is_err() {
        return Err(JournalError::CorruptNeedsRecovery {
            good_offset: 0,
            auto_recoverable: false,
        });
    }
    let tail_hash = events.last().map_or_else(
        || v4_chain_genesis_hash(run_id, genesis_hash),
        |event| event.event_hash().clone(),
    );
    Ok(V4RecoveryObservation {
        good_offset,
        tail_hash,
        file_hash,
        torn: start < bytes.len(),
        event_count: events.len(),
        first_event_id: events.first().map(|event| event.id().clone()),
        first_event_hash: events.first().map(|event| event.event_hash().clone()),
    })
}

fn v4_key_matches(
    key: &RecoveryKeyV4,
    observed: &V4RecoveryObservation,
    pending_digest: Option<&ContentHash>,
) -> bool {
    key.event_contract_version == EVENT_CONTRACT_SCHEMA_V4
        && key.pre_recovery_offset == observed.good_offset
        && key.pre_recovery_tail_hash == observed.tail_hash
        && key.pre_recovery_file_hash.as_ref() == Some(&observed.file_hash)
        && key.pending_digest.as_ref() == pending_digest
}

fn v4_bundle_prefix_stage(
    pending: &BundlePendingInspection,
) -> Result<M4BundlePrefixStageV4, JournalError> {
    let confirmed_events = u64::try_from(pending.confirmed_events)
        .map_err(|_| JournalError::BundleResumeAuthorityMismatch)?;
    let expected_events = u64::try_from(pending.planned.len())
        .map_err(|_| JournalError::BundleResumeAuthorityMismatch)?;
    if expected_events == 0
        || expected_events != pending.marker.expected_count
        || confirmed_events > expected_events
    {
        return Err(JournalError::BundleResumeAuthorityMismatch);
    }
    let classification = if confirmed_events == 0 {
        M4BundlePrefixClassificationV4::Stage0
    } else if confirmed_events == expected_events {
        M4BundlePrefixClassificationV4::AlreadyComplete
    } else {
        M4BundlePrefixClassificationV4::StrictInterior
    };
    Ok(M4BundlePrefixStageV4 {
        classification,
        confirmed_events,
        expected_events,
    })
}

fn v4_timestamp() -> Result<u64, JournalError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| JournalError::Identity("system time before Unix epoch"))
        .map(|value| value.as_secs())
}

fn decode_index_v5_genesis(bytes: &[u8]) -> Result<RunGenesisSnapshot, JournalError> {
    let snapshot: RunGenesisSnapshot = serde_json::from_slice(bytes)
        .map_err(|error| reviewgraphen_core::DomainError::Json(error.to_string()))?;
    snapshot.rebuild_aggregate()?;
    Ok(snapshot)
}

fn open_v4_run_locked(
    root: &StoreRoot,
    key: &RecoveryKeyV4,
) -> Result<(OwnedFd, OwnedFd), JournalError> {
    let runs = open_existing_dir(root.fd(), RUNS_DIR, "runs directory")?;
    let run = open_existing_dir(&runs, &run_dir_name(&key.run_id), "run directory")?;
    let run_lock = dup(&run).map_err(StoreError::Io)?;
    fs::flock(&run_lock, FlockOperation::LockExclusive).map_err(StoreError::Io)?;
    Ok((run, run_lock))
}

fn recover_new_v4_locked<'a>(
    root: &'a StoreRoot,
    key: RecoveryKeyV4,
    provenance: RecoveryProvenanceV4,
) -> Result<GenesisRecoveryV4<'a>, JournalError> {
    let limits = JournalLimits::from_store(root.limits());
    let timestamp_unix_seconds = v4_timestamp()?;
    let (run, run_lock) = match open_v4_run_locked(root, &key) {
        Ok(value) => value,
        Err(JournalError::Missing) if key.pre_recovery_file_hash.is_none() => {
            let receipt = RecoveryReceiptV4 {
                schema: RECOVERY_RECEIPT_SCHEMA_V4,
                kind: key.expected_kind,
                outcome: RecoveryOutcomeV4::GenesisNotCommitted,
                pre_file_hash: None,
                post_file_hash: None,
                key,
                provenance,
                timestamp_unix_seconds,
                marker_recovery: None,
            };
            return Ok(GenesisRecoveryV4::NotCommitted { receipt });
        }
        Err(error) => return Err(error),
    };
    let result = (|| {
        let fd = match open_verified_log(&run, true) {
            Ok(fd) => fd,
            Err(JournalError::Missing) if key.pre_recovery_file_hash.is_none() => {
                let append_pending =
                    marker_exists(&run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES)?;
                let bundle_pending = bundle_file_exists(&run, BUNDLE_PENDING_MARKER)?
                    || bundle_file_exists(&run, BUNDLE_PENDING_STAGE)?;
                if append_pending || bundle_pending || key.pending_digest.is_some() {
                    return Err(JournalError::RecoveryKeyMismatchV4);
                }
                return Ok(GenesisRecoveryV4::NotCommitted {
                    receipt: RecoveryReceiptV4 {
                        schema: RECOVERY_RECEIPT_SCHEMA_V4,
                        kind: key.expected_kind,
                        outcome: RecoveryOutcomeV4::GenesisNotCommitted,
                        pre_file_hash: None,
                        post_file_hash: None,
                        key,
                        provenance,
                        timestamp_unix_seconds,
                        marker_recovery: None,
                    },
                });
            }
            Err(error) => return Err(error),
        };
        let mut file = File::from(fd);
        fs::flock(file.as_fd(), FlockOperation::LockExclusive).map_err(StoreError::Io)?;
        let genesis = CasStore::open(root)?.read(&CasHash::parse(key.genesis_hash.to_string())?)?;
        let observed = inspect_v4_file(
            file.try_clone()?,
            &key.run_id,
            &key.genesis_hash,
            &genesis,
            limits,
        )?;
        let append_pending = marker_exists(&run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES)?;
        let pending_digest = append_pending.then(|| ContentHash::sha256(APPEND_PENDING_BYTES));
        if bundle_file_exists(&run, BUNDLE_PENDING_MARKER)?
            || bundle_file_exists(&run, BUNDLE_PENDING_STAGE)?
            || !v4_key_matches(&key, &observed, pending_digest.as_ref())
        {
            return Err(JournalError::RecoveryKeyMismatchV4);
        }
        if observed.event_count == 0 {
            // A partial line has never become a confirmed genesis event.
            fs::unlinkat(&run, JOURNAL_FILE, AtFlags::empty()).map_err(StoreError::Io)?;
            #[cfg(test)]
            let injected = take_v4_recovery_fault(V4RecoveryFault::GenesisDirectory);
            #[cfg(not(test))]
            let injected = false;
            if injected || fs::fsync(&run).is_err() {
                return Err(JournalError::RecoveryDurabilityUncertainV4 {
                    outcome: RecoveryOutcomeV4::GenesisNotCommitted,
                });
            }
            return Ok(GenesisRecoveryV4::NotCommitted {
                receipt: RecoveryReceiptV4 {
                    schema: RECOVERY_RECEIPT_SCHEMA_V4,
                    kind: key.expected_kind,
                    outcome: RecoveryOutcomeV4::GenesisNotCommitted,
                    pre_file_hash: Some(observed.file_hash),
                    post_file_hash: None,
                    key,
                    provenance,
                    timestamp_unix_seconds,
                    marker_recovery: None,
                },
            });
        }
        if observed.torn
            || observed.event_count != 1
            || observed.first_event_id.is_none()
            || observed.first_event_hash.is_none()
        {
            return Err(JournalError::RecoveryKeyMismatchV4);
        }
        let identity = JournalIdentity::new(key.run_id.clone(), JournalGenesis::V4(genesis))?;
        if identity.genesis_hash() != key.genesis_hash {
            return Err(JournalError::RecoveryKeyMismatchV4);
        }
        validate_prefix(&identity, &scan(&mut file, &identity, limits, true)?.events)?;
        let journal = EventJournal::open_with_limits(root, identity, limits)?;
        Ok(GenesisRecoveryV4::Committed {
            receipt: RecoveryReceiptV4 {
                schema: RECOVERY_RECEIPT_SCHEMA_V4,
                kind: key.expected_kind,
                outcome: RecoveryOutcomeV4::GenesisCommitted {
                    event_id: observed.first_event_id.expect("checked"),
                    event_hash: observed.first_event_hash.expect("checked"),
                    confirmed_offset: observed.good_offset,
                },
                pre_file_hash: Some(observed.file_hash.clone()),
                post_file_hash: Some(observed.file_hash),
                key,
                provenance,
                timestamp_unix_seconds,
                marker_recovery: None,
            },
            journal,
        })
    })();
    drop(run_lock);
    result
}

#[cfg(test)]
fn recover_canonical_tail_v4_locked(
    root: &StoreRoot,
    key: RecoveryKeyV4,
    provenance: RecoveryProvenanceV4,
) -> Result<RecoveryReceiptV4, JournalError> {
    let limits = JournalLimits::from_store(root.limits());
    let timestamp_unix_seconds = v4_timestamp()?;
    let (run, run_lock) = open_v4_run_locked(root, &key)?;
    let result = (|| {
        let fd = open_verified_log(&run, true)?;
        let mut file = File::from(fd);
        fs::flock(file.as_fd(), FlockOperation::LockExclusive).map_err(StoreError::Io)?;
        let genesis = CasStore::open(root)?.read(&CasHash::parse(key.genesis_hash.to_string())?)?;
        let observed = inspect_v4_file(
            file.try_clone()?,
            &key.run_id,
            &key.genesis_hash,
            &genesis,
            limits,
        )?;
        let mut append_pending = marker_exists(&run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES)?;
        let pending_digest = append_pending.then(|| ContentHash::sha256(APPEND_PENDING_BYTES));
        if bundle_file_exists(&run, BUNDLE_PENDING_MARKER)?
            || bundle_file_exists(&run, BUNDLE_PENDING_STAGE)?
            || !v4_key_matches(&key, &observed, pending_digest.as_ref())
            || observed.event_count == 0
        {
            return Err(JournalError::RecoveryKeyMismatchV4);
        }
        if !append_pending {
            ensure_v4_append_pending(&run)?;
            append_pending = true;
        }
        let discarded_hash = if observed.torn {
            let discarded = read_suffix(&mut file, observed.good_offset, limits.max_replay_bytes)?;
            let outcome = RecoveryOutcomeV4::TailRecovered {
                good_offset: observed.good_offset,
                discarded_hash: ContentHash::sha256(&discarded),
            };
            file.set_len(observed.good_offset)?;
            #[cfg(test)]
            let injected = take_v4_recovery_fault(V4RecoveryFault::TailFile);
            #[cfg(not(test))]
            let injected = false;
            if injected || file.sync_all().is_err() {
                return Err(JournalError::RecoveryDurabilityUncertainV4 { outcome });
            }
            ContentHash::sha256(&discarded)
        } else {
            ContentHash::sha256(&[])
        };
        let outcome = RecoveryOutcomeV4::TailRecovered {
            good_offset: observed.good_offset,
            discarded_hash: discarded_hash.clone(),
        };
        if append_pending {
            fs::unlinkat(&run, APPEND_PENDING_MARKER, AtFlags::empty()).map_err(StoreError::Io)?;
            #[cfg(test)]
            let injected = take_v4_recovery_fault(V4RecoveryFault::TailDirectory);
            #[cfg(not(test))]
            let injected = false;
            if injected || fs::fsync(&run).is_err() {
                return Err(JournalError::RecoveryDurabilityUncertainV4 { outcome });
            }
        }
        let post_bytes = read_prefix(&mut file, observed.good_offset)?;
        let post_file_hash = ContentHash::sha256(&post_bytes);
        Ok(RecoveryReceiptV4 {
            schema: RECOVERY_RECEIPT_SCHEMA_V4,
            kind: key.expected_kind,
            outcome,
            pre_file_hash: Some(observed.file_hash),
            post_file_hash: Some(post_file_hash),
            key,
            provenance,
            timestamp_unix_seconds,
            marker_recovery: None,
        })
    })();
    drop(run_lock);
    result
}

fn clear_v4_bundle_marker(run: &OwnedFd, outcome: &RecoveryOutcomeV4) -> Result<(), JournalError> {
    let mut namespace_mutated = false;
    if bundle_file_exists(run, BUNDLE_PENDING_STAGE)? {
        fs::unlinkat(run, BUNDLE_PENDING_STAGE, AtFlags::empty()).map_err(StoreError::Io)?;
        namespace_mutated = true;
    }
    if let Err(error) = fs::unlinkat(run, BUNDLE_PENDING_MARKER, AtFlags::empty()) {
        if namespace_mutated {
            return Err(JournalError::RecoveryDurabilityUncertainV4 {
                outcome: outcome.clone(),
            });
        }
        return Err(StoreError::Io(error).into());
    }
    #[cfg(test)]
    let injected = take_v4_recovery_fault(V4RecoveryFault::M4Directory);
    #[cfg(not(test))]
    let injected = false;
    if injected || fs::fsync(run).is_err() {
        return Err(JournalError::RecoveryDurabilityUncertainV4 {
            outcome: outcome.clone(),
        });
    }
    Ok(())
}

impl JournalReader {
    #[must_use]
    pub fn events(&self) -> &[EventEnvelope] {
        &self.state.events
    }
    #[must_use]
    pub fn confirmed_offset(&self) -> u64 {
        self.state.confirmed_offset
    }
    #[must_use]
    pub fn tail_hash(&self) -> &ContentHash {
        &self.state.tail_hash
    }
    #[must_use]
    pub fn identity(&self) -> &JournalIdentity {
        &self.identity
    }
    #[must_use]
    pub const fn limits(&self) -> JournalLimits {
        self.limits
    }
    /// Executes `operation` while the shared journal lock is still held.
    /// Derived-index readers use this to keep a tail check and its projection
    /// query in one lock-held snapshot rather than opening an append TOCTOU.
    pub fn with_locked_snapshot<T>(
        &self,
        operation: impl FnOnce(&[EventEnvelope], u64, &ContentHash) -> T,
    ) -> T {
        operation(
            &self.state.events,
            self.state.confirmed_offset,
            &self.state.tail_hash,
        )
    }
}

impl IndexJournalReader {
    #[must_use]
    #[allow(dead_code)] // Used by the index migration.
    pub(crate) fn identity(&self) -> &JournalIdentity {
        &self.identity
    }

    /// Exact requested capacities retained by the lock-held receipt metadata.
    /// The shared V2 genesis backing is reported separately by
    /// `v2_genesis_backing` and must be charged only once by the caller.
    pub(crate) fn retained_metadata_capacity(&self) -> Result<u64, JournalError> {
        let vector = self
            .completed_receipts
            .capacity()
            .checked_mul(std::mem::size_of::<(RecoveryIntent, RecoveryCompletion)>())
            .ok_or(JournalError::Incomplete {
                limit: self.working_limit,
                observed: u64::MAX,
            })?;
        let heap =
            self.completed_receipts
                .iter()
                .try_fold(0usize, |used, (intent, completion)| {
                    let next = intent
                        .recovery_id
                        .allocated_bytes()
                        .checked_add(intent.run_id.allocated_bytes())
                        .and_then(|value| value.checked_add(intent.genesis_hash.allocated_bytes()))
                        .and_then(|value| value.checked_add(intent.nonce.capacity()))
                        .and_then(|value| {
                            value.checked_add(intent.discarded_hash.allocated_bytes())
                        })
                        .and_then(|value| {
                            value.checked_add(
                                intent
                                    .bundle_digest
                                    .as_ref()
                                    .map_or(0, ContentHash::allocated_bytes),
                            )
                        })
                        .and_then(|value| {
                            value.checked_add(
                                intent
                                    .bundle_recovery_kind
                                    .as_ref()
                                    .map_or(0, String::capacity),
                            )
                        })
                        .and_then(|value| value.checked_add(intent.pre_tail_hash.allocated_bytes()))
                        .and_then(|value| value.checked_add(intent.actor.capacity()))
                        .and_then(|value| value.checked_add(intent.tool_version.capacity()))
                        .and_then(|value| {
                            value.checked_add(completion.recovery_id.allocated_bytes())
                        })
                        .and_then(|value| {
                            value.checked_add(completion.post_file_hash.allocated_bytes())
                        })
                        .ok_or(JournalError::Incomplete {
                            limit: self.working_limit,
                            observed: u64::MAX,
                        })?;
                    used.checked_add(next).ok_or(JournalError::Incomplete {
                        limit: self.working_limit,
                        observed: u64::MAX,
                    })
                })?;
        let identity = self
            .identity
            .local_metadata_capacity()
            .checked_add(self.identity.shared_certificate_capacity())
            .ok_or(JournalError::Incomplete {
                limit: self.working_limit,
                observed: u64::MAX,
            })?;
        u64::try_from(
            vector
                .checked_add(heap)
                .and_then(|value| value.checked_add(identity))
                .ok_or(JournalError::Incomplete {
                    limit: self.working_limit,
                    observed: u64::MAX,
                })?,
        )
        .map_err(|_| JournalError::Incomplete {
            limit: self.working_limit,
            observed: u64::MAX,
        })
    }

    /// Replays the confirmed JSONL prefix once while its shared lock remains
    /// held. The callback receives one locally validated, canonical envelope
    /// at a time and must not retain it. A complete compact certificate is
    /// returned only after the physical EOF, hash chain, V2 genesis manifest,
    /// and every durable recovery checkpoint agree.
    #[allow(dead_code)] // Used by the index migration.
    pub(crate) fn with_locked_prefix<E>(
        &mut self,
        visitor: impl FnMut(&EventEnvelope, u64) -> Result<(), E>,
    ) -> Result<PrefixCertificate, IndexReplayError<E>> {
        scan_index_prefix(
            &mut self.file,
            &self.identity,
            self.limits,
            self.working_limit,
            &self.completed_receipts,
            visitor,
        )
    }
}

impl JournalWriter {
    fn refuse_bundle_resume_gate(&self) -> Result<(), JournalError> {
        if read_bundle_pending(&self.run, &self.identity, self.limits)?.is_some()
            || bundle_file_exists(&self.run, BUNDLE_PENDING_STAGE)?
        {
            return Err(JournalError::SessionResumeRequired);
        }
        Ok(())
    }

    /// Appends exactly one canonical JSON object and newline after validating
    /// the entire candidate prefix through core.  A failure before durable
    /// confirmation rolls back to the previously scanned byte offset.
    pub fn append(
        &mut self,
        envelope: EventEnvelope,
    ) -> Result<JournalAppendReceipt, JournalError> {
        self.append_batch(std::slice::from_ref(&envelope))?
            .pop()
            .ok_or(JournalError::Identity("append batch produced no event"))
    }

    fn append_verification_bundle_suffix(
        &mut self,
        envelopes: &[EventEnvelope],
    ) -> Result<Vec<JournalAppendReceipt>, JournalError> {
        if self.poisoned || self.append_durability == AppendDurability::Uncertain {
            return Err(JournalError::Poisoned);
        }
        if envelopes.is_empty() || envelopes.len() > 3 {
            return Err(JournalError::Identity(
                "verification bundle must contain one to three events",
            ));
        }
        self.refuse_bundle_resume_gate()?;
        let mut candidate = self.state.events.clone();
        candidate.extend(envelopes.iter().cloned());
        limit(
            u64::try_from(candidate.len()).map_err(|_| JournalError::Incomplete {
                limit: self.limits.max_events,
                observed: u64::MAX,
            })?,
            self.limits.max_events,
        )?;
        let mut lines = Vec::new();
        let mut candidate_end = self.state.confirmed_offset;
        for envelope in envelopes {
            let mut line = envelope.canonical_bytes()?;
            line.push(b'\n');
            let line_len = u64::try_from(line.len()).map_err(|_| JournalError::Incomplete {
                limit: self.limits.max_event_line_bytes,
                observed: u64::MAX,
            })?;
            limit(line_len, self.limits.max_event_line_bytes)?;
            candidate_end =
                candidate_end
                    .checked_add(line_len)
                    .ok_or(JournalError::Incomplete {
                        limit: self.limits.max_replay_bytes,
                        observed: u64::MAX,
                    })?;
            limit(candidate_end, self.limits.max_replay_bytes)?;
            lines.push(line);
        }
        validate_prefix(&self.identity, &candidate)?;
        let marker =
            VerificationBundlePendingMarkerV3::new(&self.identity, &self.state, envelopes)?;
        self.publish_bundle_pending(&marker)?;

        self.append_bundle_lines(envelopes, lines, &marker, 0)
    }

    fn resume_verification_bundle_suffix(
        &mut self,
        marker: &VerificationBundlePendingMarkerV3,
        envelopes: &[EventEnvelope],
        already_confirmed: usize,
    ) -> Result<Vec<JournalAppendReceipt>, JournalError> {
        if self.poisoned || self.append_durability == AppendDurability::Uncertain {
            return Err(JournalError::Poisoned);
        }
        let planned = marker.envelopes(self.limits)?;
        let suffix_matches = planned.get(already_confirmed..).is_some_and(|expected| {
            expected.len() == envelopes.len()
                && expected.iter().zip(envelopes).all(|(left, right)| {
                    left.id() == right.id()
                        && left.event_hash() == right.event_hash()
                        && left.canonical_bytes().ok() == right.canonical_bytes().ok()
                })
        });
        if already_confirmed == 0 || already_confirmed >= planned.len() || !suffix_matches {
            return Err(JournalError::BundleResumeAuthorityMismatch);
        }
        let mut candidate = self.state.events.clone();
        candidate.extend(envelopes.iter().cloned());
        validate_prefix(&self.identity, &candidate)?;
        let mut lines = Vec::new();
        let mut candidate_end = self.state.confirmed_offset;
        for envelope in envelopes {
            let mut line = envelope.canonical_bytes()?;
            line.push(b'\n');
            let line_len = u64::try_from(line.len()).map_err(|_| JournalError::Incomplete {
                limit: self.limits.max_event_line_bytes,
                observed: u64::MAX,
            })?;
            limit(line_len, self.limits.max_event_line_bytes)?;
            candidate_end =
                candidate_end
                    .checked_add(line_len)
                    .ok_or(JournalError::Incomplete {
                        limit: self.limits.max_replay_bytes,
                        observed: u64::MAX,
                    })?;
            limit(candidate_end, self.limits.max_replay_bytes)?;
            lines.push(line);
        }
        self.append_bundle_lines(envelopes, lines, marker, already_confirmed)
    }

    fn append_bundle_lines(
        &mut self,
        envelopes: &[EventEnvelope],
        lines: Vec<Vec<u8>>,
        marker: &VerificationBundlePendingMarkerV3,
        already_confirmed: usize,
    ) -> Result<Vec<JournalAppendReceipt>, JournalError> {
        let mut receipts = Vec::new();
        for (index, (envelope, line)) in envelopes.iter().zip(lines).enumerate() {
            let pre = self.state.confirmed_offset;
            let audit = recovery_audit(
                &self.intents,
                &self.completions,
                &self.identity,
                self.limits,
            )?;
            sync_recovery_dirs(&self.intents, &self.completions)?;
            let rollback_sizes =
                rollback_receipt_size_bound(&self.identity, pre, &self.state.tail_hash)?;
            reserve_recovery_capacity(&audit, self.limits, &rollback_sizes)?;
            self.file.seek(SeekFrom::Start(pre))?;
            if let Err(error) = self.write_candidate(&line) {
                let intent = match self.record_rollback_intent(pre) {
                    Ok(intent) => intent,
                    Err(_) => {
                        self.poisoned = true;
                        self.append_durability = AppendDurability::Uncertain;
                        return Err(JournalError::SessionUncertain);
                    }
                };
                if self.rollback(pre).is_err() || self.complete_rollback_intent(&intent).is_err() {
                    self.poisoned = true;
                    self.append_durability = AppendDurability::Uncertain;
                    return Err(JournalError::SessionUncertain);
                }
                if already_confirmed == 0 && index == 0 {
                    if self.clear_bundle_pending().is_err() {
                        self.poisoned = true;
                        self.append_durability = AppendDurability::Uncertain;
                        return Err(JournalError::SessionUncertain);
                    }
                    return Err(JournalError::Io(error));
                }
                return Err(JournalError::BundleAppendInterrupted {
                    durable_stage: VerificationBundleDurableStageV3 {
                        confirmed_events: u64::try_from(already_confirmed + index)
                            .unwrap_or(u64::MAX),
                        expected_events: marker.expected_count,
                    },
                });
            }

            self.state.confirmed_offset = pre
                .checked_add(u64::try_from(line.len()).unwrap_or(u64::MAX))
                .ok_or(JournalError::Incomplete {
                    limit: self.limits.max_replay_bytes,
                    observed: u64::MAX,
                })?;
            self.state.tail_hash = envelope.event_hash().clone();
            self.state.events.push(envelope.clone());
            let confirmed = u64::try_from(already_confirmed + index + 1).unwrap_or(u64::MAX);
            if let Err(error) = self.persist_bundle_stage(confirmed) {
                self.poisoned = true;
                self.append_durability = AppendDurability::Uncertain;
                return Err(error);
            }
            receipts.push(JournalAppendReceipt {
                sequence: envelope.sequence(),
                event_hash: envelope.event_hash().clone(),
                tail_offset: self.state.confirmed_offset,
            });
            #[cfg(test)]
            if (confirmed == 1 && self.take_fault(AppendFault::BundleAfterDurableLine1))
                || (confirmed == 2 && self.take_fault(AppendFault::BundleAfterDurableLine2))
            {
                return Err(JournalError::BundleAppendInterrupted {
                    durable_stage: VerificationBundleDurableStageV3 {
                        confirmed_events: confirmed,
                        expected_events: marker.expected_count,
                    },
                });
            }
        }
        if let Err(error) = self.clear_bundle_pending() {
            self.poisoned = true;
            self.append_durability = AppendDurability::Uncertain;
            return Err(error);
        }
        Ok(receipts)
    }

    fn publish_bundle_pending(
        &mut self,
        marker: &VerificationBundlePendingMarkerV3,
    ) -> Result<(), JournalError> {
        let bytes = canonical_json(marker)?;
        let expected_bound = self
            .limits
            .max_event_line_bytes
            .checked_mul(marker.expected_count)
            .and_then(|value| value.checked_add(64 * 1024))
            .ok_or(JournalError::Incomplete {
                limit: self.limits.max_replay_bytes,
                observed: u64::MAX,
            })?;
        limit(
            u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            expected_bound.min(self.limits.max_replay_bytes),
        )?;
        if let Err(error) = publish_bundle_file(&self.run, BUNDLE_PENDING_MARKER, &bytes) {
            self.poisoned = true;
            self.append_durability = AppendDurability::Uncertain;
            return Err(error);
        }
        if let Err(error) = publish_bundle_file(&self.run, BUNDLE_PENDING_STAGE, b"0\n") {
            self.poisoned = true;
            self.append_durability = AppendDurability::Uncertain;
            return Err(error);
        }
        if let Err(error) = fs::fsync(&self.run).map_err(StoreError::Io) {
            self.poisoned = true;
            self.append_durability = AppendDurability::Uncertain;
            return Err(error.into());
        }
        Ok(())
    }

    fn persist_bundle_stage(&mut self, confirmed: u64) -> Result<(), JournalError> {
        #[cfg(test)]
        if self.take_fault(AppendFault::BundleStageSync) {
            return Err(JournalError::Io(injected_io_error("bundle stage sync")));
        }
        persist_bundle_stage_file(&self.run, confirmed)
    }

    fn clear_bundle_pending(&mut self) -> Result<(), JournalError> {
        #[cfg(any(test, feature = "test-support"))]
        if self.take_fault(AppendFault::ClearMarkerDirectorySync) {
            return Err(JournalError::Io(injected_io_error(
                "bundle marker directory sync",
            )));
        }
        for name in [BUNDLE_PENDING_STAGE, BUNDLE_PENDING_MARKER] {
            fs::unlinkat(&self.run, name, AtFlags::empty()).map_err(StoreError::Io)?;
        }
        fs::fsync(&self.run).map_err(StoreError::Io)?;
        Ok(())
    }

    /// Atomically appends one closed suffix under one pending marker and one
    /// `sync_data`. Kept private so V3 callers cannot bypass the sealed core
    /// transaction APIs with arbitrary envelopes.
    fn append_batch(
        &mut self,
        envelopes: &[EventEnvelope],
    ) -> Result<Vec<JournalAppendReceipt>, JournalError> {
        if self.poisoned {
            return Err(JournalError::Poisoned);
        }
        if self.append_durability == AppendDurability::Uncertain {
            return Err(JournalError::Poisoned);
        }
        self.refuse_bundle_resume_gate()?;
        if envelopes.is_empty() {
            return Err(JournalError::Identity("append batch must not be empty"));
        }
        let candidate_count = self.state.events.len().checked_add(envelopes.len()).ok_or(
            JournalError::Incomplete {
                limit: self.limits.max_events,
                observed: u64::MAX,
            },
        )?;
        limit(
            u64::try_from(candidate_count).map_err(|_| JournalError::Incomplete {
                limit: self.limits.max_events,
                observed: u64::MAX,
            })?,
            self.limits.max_events,
        )?;
        let mut bytes = Vec::new();
        let mut relative_ends = Vec::new();
        relative_ends
            .try_reserve_exact(envelopes.len())
            .map_err(|_| JournalError::Incomplete {
                limit: self.limits.max_replay_bytes,
                observed: u64::MAX,
            })?;
        for envelope in envelopes {
            let mut line = envelope.canonical_bytes()?;
            line.push(b'\n');
            let line_len = u64::try_from(line.len()).map_err(|_| JournalError::Incomplete {
                limit: self.limits.max_event_line_bytes,
                observed: u64::MAX,
            })?;
            limit(line_len, self.limits.max_event_line_bytes)?;
            let admitted_total = u64::try_from(bytes.len())
                .ok()
                .and_then(|current| current.checked_add(line_len))
                .and_then(|relative| self.state.confirmed_offset.checked_add(relative))
                .ok_or(JournalError::Incomplete {
                    limit: self.limits.max_replay_bytes,
                    observed: u64::MAX,
                })?;
            limit(admitted_total, self.limits.max_replay_bytes)?;
            bytes
                .try_reserve_exact(line.len())
                .map_err(|_| JournalError::Incomplete {
                    limit: self.limits.max_replay_bytes,
                    observed: admitted_total,
                })?;
            bytes.extend_from_slice(&line);
            relative_ends.push(u64::try_from(bytes.len()).map_err(|_| {
                JournalError::Incomplete {
                    limit: self.limits.max_replay_bytes,
                    observed: u64::MAX,
                }
            })?);
        }
        let mut candidate = self.state.events.clone();
        candidate.extend(envelopes.iter().cloned());
        let candidate_end = self
            .state
            .confirmed_offset
            .checked_add(
                u64::try_from(bytes.len()).map_err(|_| JournalError::Incomplete {
                    limit: self.limits.max_replay_bytes,
                    observed: u64::MAX,
                })?,
            )
            .ok_or(JournalError::Incomplete {
                limit: self.limits.max_replay_bytes,
                observed: u64::MAX,
            })?;
        limit(candidate_end, self.limits.max_replay_bytes)?;
        validate_prefix(&self.identity, &candidate)?;
        let pre = self.state.confirmed_offset;
        // A writer may be reused after a failed append, which leaves two
        // immutable receipts behind.  Re-audit for every append rather than
        // relying on the capacity seen when this lock was acquired.
        let audit = recovery_audit(
            &self.intents,
            &self.completions,
            &self.identity,
            self.limits,
        )?;
        sync_recovery_dirs(&self.intents, &self.completions)?;
        let rollback_sizes =
            rollback_receipt_size_bound(&self.identity, pre, &self.state.tail_hash)?;
        reserve_recovery_capacity(&audit, self.limits, &rollback_sizes)?;
        self.record_append_marker()?;
        self.file.seek(SeekFrom::Start(pre))?;
        let write_result = self.write_candidate(&bytes);
        if let Err(error) = write_result {
            // Publish the exact suffix *before* attempting the destructive
            // rollback.  Thus a crash after any unacknowledged write/flush/
            // sync boundary is discoverable on the next open.
            let intent = match self.record_rollback_intent(pre) {
                Ok(intent) => intent,
                Err(_) => {
                    self.poisoned = true;
                    self.append_durability = AppendDurability::Uncertain;
                    return Err(JournalError::Poisoned);
                }
            };
            if self.rollback(pre).is_err() {
                self.poisoned = true;
                self.append_durability = AppendDurability::Uncertain;
                return Err(JournalError::Poisoned);
            }
            if self.complete_rollback_intent(&intent).is_err() {
                self.poisoned = true;
                self.append_durability = AppendDurability::Uncertain;
                return Err(JournalError::Poisoned);
            }
            if let Err(error) = self.clear_append_marker() {
                self.poisoned = true;
                self.append_durability = AppendDurability::Uncertain;
                return Err(error);
            }
            return Err(JournalError::Io(error));
        }
        self.state.confirmed_offset = candidate_end;
        self.state.tail_hash = envelopes
            .last()
            .ok_or(JournalError::Identity("append batch must not be empty"))?
            .event_hash()
            .clone();
        self.state.events = candidate;
        if let Err(error) = self.clear_append_marker() {
            // The event reached sync_data. Never leave this writer pointing
            // at the old offset if marker cleanup durability is unknown.
            self.poisoned = true;
            self.append_durability = AppendDurability::Uncertain;
            return Err(error);
        }
        envelopes
            .iter()
            .zip(relative_ends)
            .map(|(envelope, relative_end)| {
                Ok(JournalAppendReceipt {
                    sequence: envelope.sequence(),
                    event_hash: envelope.event_hash().clone(),
                    tail_offset: pre
                        .checked_add(relative_end)
                        .ok_or(JournalError::Incomplete {
                            limit: self.limits.max_replay_bytes,
                            observed: u64::MAX,
                        })?,
                })
            })
            .collect()
    }
    #[must_use]
    pub fn events(&self) -> &[EventEnvelope] {
        &self.state.events
    }
    #[must_use]
    pub fn tail_hash(&self) -> &ContentHash {
        &self.state.tail_hash
    }

    fn write_candidate(&mut self, line: &[u8]) -> Result<(), std::io::Error> {
        #[cfg(test)]
        if self.take_fault(AppendFault::PartialWrite) {
            let partial = line.len().max(2) / 2;
            self.file.write_all(&line[..partial])?;
            return Err(injected_io_error("partial write"));
        }
        self.file.write_all(line)?;
        #[cfg(test)]
        if self.take_fault(AppendFault::Flush) {
            return Err(injected_io_error("flush"));
        }
        self.file.flush()?;
        #[cfg(test)]
        if self.take_fault(AppendFault::SyncData) {
            return Err(injected_io_error("sync_data"));
        }
        self.file.sync_data()
    }

    fn rollback(&mut self, pre: u64) -> Result<(), std::io::Error> {
        #[cfg(test)]
        if self.take_fault(AppendFault::Truncate) {
            return Err(injected_io_error("truncate"));
        }
        self.file.set_len(pre)?;
        self.file.seek(SeekFrom::Start(pre))?;
        #[cfg(test)]
        if self.take_fault(AppendFault::RollbackSync) {
            return Err(injected_io_error("rollback sync_data"));
        }
        self.file.sync_data()
    }

    /// Durable rollback uncertainty is itself a recovery transaction.  The
    /// marker persists even if the truncate happened but its `sync_data`
    /// acknowledgement failed, so a fresh writer cannot mistake an
    /// unacknowledged rollback for a safe log.
    fn record_rollback_intent(&mut self, pre: u64) -> Result<RecoveryIntent, JournalError> {
        let actual_len = self.file.metadata()?.len();
        if actual_len < pre {
            return Err(JournalError::ReceiptCorruption {
                name: "rollback shortened a confirmed prefix".to_owned(),
            });
        }
        self.file.seek(SeekFrom::Start(pre))?;
        let mut discarded = Vec::new();
        self.file.read_to_end(&mut discarded)?;
        let discarded_hash = ContentHash::sha256(&discarded);
        let nonce = recovery_nonce()?;
        let recovery_id = recovery_id(
            &self.identity.run_id,
            &self.identity.genesis_hash(),
            pre,
            &discarded_hash,
            &nonce,
        )?;
        let intent = RecoveryIntent {
            recovery_id: recovery_id.clone(),
            run_id: self.identity.run_id.clone(),
            genesis_hash: self.identity.genesis_hash(),
            nonce,
            good_offset: pre,
            discarded_hash,
            discarded_offset: None,
            discarded_len: None,
            pre_size: None,
            bundle_digest: None,
            bundle_pre_offset: None,
            bundle_recovery_kind: None,
            pre_tail_hash: self.state.tail_hash.clone(),
            actor: "reviewgraphen-store".to_owned(),
            tool_version: "append-rollback".to_owned(),
            timestamp_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| JournalError::Identity("system time before Unix epoch"))?
                .as_secs(),
        };
        let audit = recovery_audit(
            &self.intents,
            &self.completions,
            &self.identity,
            self.limits,
        )?;
        validate_proposed_recovery_lineage(&audit, &intent)?;
        let name = receipt_name(&recovery_id);
        match read_receipt::<RecoveryIntent>(&self.intents, &name, self.limits)? {
            Some(existing)
                if existing.recovery_id == intent.recovery_id
                    && existing.good_offset == intent.good_offset
                    && existing.discarded_hash == intent.discarded_hash
                    && existing.pre_tail_hash == intent.pre_tail_hash =>
            {
                Ok(existing)
            }
            Some(_) => Err(JournalError::ReceiptCorruption { name }),
            None => {
                #[cfg(test)]
                if self.take_fault(AppendFault::IntentPublish) {
                    return Err(JournalError::Io(injected_io_error(
                        "rollback intent publish",
                    )));
                }
                publish_receipt_with_limits(&self.intents, &name, &intent, self.limits)?;
                Ok(intent)
            }
        }
    }

    fn complete_rollback_intent(&mut self, intent: &RecoveryIntent) -> Result<(), JournalError> {
        let audit = recovery_audit(
            &self.intents,
            &self.completions,
            &self.identity,
            self.limits,
        )?;
        validate_proposed_recovery_lineage(&audit, intent)?;
        if self.file.metadata()?.len() != intent.good_offset {
            return Err(JournalError::ReceiptCorruption {
                name: receipt_name(&intent.recovery_id),
            });
        }
        let prefix = read_prefix(&mut self.file, intent.good_offset)?;
        let state = scan_bytes(&prefix, &self.identity, self.limits, false)?;
        if state.torn.is_some()
            || state.confirmed_offset != intent.good_offset
            || state.tail_hash != intent.pre_tail_hash
        {
            return Err(JournalError::ReceiptCorruption {
                name: receipt_name(&intent.recovery_id),
            });
        }
        let completion = RecoveryCompletion {
            recovery_id: intent.recovery_id.clone(),
            post_file_hash: ContentHash::sha256(&prefix),
        };
        publish_receipt_with_limits(
            &self.completions,
            &receipt_name(&completion.recovery_id),
            &completion,
            self.limits,
        )
    }

    fn record_append_marker(&mut self) -> Result<(), JournalError> {
        let fd = match fs::openat(
            &self.run,
            APPEND_PENDING_MARKER,
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::from_raw_mode(0o600),
        ) {
            Ok(fd) => fd,
            Err(Errno::EXIST) => return Err(JournalError::Poisoned),
            Err(error) => return Err(JournalError::Store(StoreError::Io(error))),
        };
        verify_fd_kind_mode(&fd, "append pending marker", FileType::RegularFile, 0o600)?;
        let mut marker = File::from(fd);
        marker.write_all(APPEND_PENDING_BYTES)?;
        marker.sync_all()?;
        fs::fsync(&self.run).map_err(StoreError::Io)?;
        Ok(())
    }

    fn clear_append_marker(&mut self) -> Result<(), JournalError> {
        fs::unlinkat(&self.run, APPEND_PENDING_MARKER, AtFlags::empty()).map_err(StoreError::Io)?;
        #[cfg(any(test, feature = "test-support"))]
        if self.take_fault(AppendFault::ClearMarkerDirectorySync) {
            restore_append_marker(&self.run).map_err(|error| match error {
                JournalError::Io(error) => error,
                _ => injected_io_error("marker restoration"),
            })?;
            return Err(JournalError::Io(injected_io_error(
                "append marker directory sync",
            )));
        }
        fs::fsync(&self.run).map_err(StoreError::Io)?;
        Ok(())
    }

    #[cfg(any(test, feature = "test-support"))]
    fn inject_faults(&mut self, faults: impl IntoIterator<Item = AppendFault>) {
        self.faults.extend(faults);
    }

    #[cfg(any(test, feature = "test-support"))]
    fn take_fault(&mut self, expected: AppendFault) -> bool {
        self.faults.front().copied() == Some(expected) && self.faults.pop_front().is_some()
    }
}

#[cfg(any(test, feature = "test-support"))]
fn injected_io_error(operation: &'static str) -> std::io::Error {
    std::io::Error::other(format!("test-only injected {operation} failure"))
}

fn publish_bundle_file(run: &OwnedFd, name: &str, bytes: &[u8]) -> Result<(), JournalError> {
    let fd = fs::openat(
        run,
        name,
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )
    .map_err(StoreError::Io)?;
    verify_fd_kind_mode(
        &fd,
        "verification bundle marker",
        FileType::RegularFile,
        0o600,
    )?;
    let mut file = File::from(fd);
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn persist_bundle_stage_file(run: &OwnedFd, confirmed: u64) -> Result<(), JournalError> {
    if confirmed > 3 {
        return Err(JournalError::ReceiptCorruption {
            name: BUNDLE_PENDING_STAGE.to_owned(),
        });
    }
    let fd = fs::openat(
        run,
        BUNDLE_PENDING_STAGE,
        OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )
    .map_err(StoreError::Io)?;
    verify_fd_kind_mode(&fd, "bundle append stage", FileType::RegularFile, 0o600)?;
    let mut file = File::from(fd);
    file.seek(SeekFrom::Start(0))?;
    file.write_all(format!("{confirmed}\n").as_bytes())?;
    file.set_len(2)?;
    file.sync_all()?;
    fs::fsync(run).map_err(StoreError::Io)?;
    Ok(())
}

fn bundle_file_exists(run: &OwnedFd, name: &str) -> Result<bool, JournalError> {
    match fs::statat(run, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => {
            if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
                || stat.st_mode & 0o7777 != 0o600
            {
                return Err(JournalError::ReceiptCorruption {
                    name: name.to_owned(),
                });
            }
            Ok(true)
        }
        Err(Errno::NOENT) => Ok(false),
        Err(error) => Err(StoreError::Io(error).into()),
    }
}

fn read_bundle_pending(
    run: &OwnedFd,
    identity: &JournalIdentity,
    limits: JournalLimits,
) -> Result<Option<VerificationBundlePendingMarkerV3>, JournalError> {
    Ok(read_bundle_pending_with_hash(run, identity, limits)?.map(|(marker, _)| marker))
}

fn read_bundle_pending_with_hash(
    run: &OwnedFd,
    identity: &JournalIdentity,
    limits: JournalLimits,
) -> Result<Option<(VerificationBundlePendingMarkerV3, ContentHash)>, JournalError> {
    let marker_exists = bundle_file_exists(run, BUNDLE_PENDING_MARKER)?;
    if !marker_exists {
        return Ok(None);
    }
    let fd = fs::openat(
        run,
        BUNDLE_PENDING_MARKER,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(StoreError::Io)?;
    verify_fd_kind_mode(
        &fd,
        "verification bundle marker",
        FileType::RegularFile,
        0o600,
    )?;
    let size = u64::try_from(fs::fstat(&fd).map_err(StoreError::Io)?.st_size).map_err(|_| {
        JournalError::ReceiptCorruption {
            name: BUNDLE_PENDING_MARKER.to_owned(),
        }
    })?;
    let marker_limit = limits
        .max_event_line_bytes
        .checked_mul(3)
        .and_then(|value| value.checked_add(64 * 1024))
        .unwrap_or(u64::MAX)
        .min(limits.max_replay_bytes);
    limit(size, marker_limit)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(usize::try_from(size).map_err(|_| JournalError::Incomplete {
            limit: marker_limit,
            observed: size,
        })?)
        .map_err(|_| JournalError::Incomplete {
            limit: marker_limit,
            observed: size,
        })?;
    File::from(fd).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).ok() != Some(size) {
        return Err(JournalError::ReceiptCorruption {
            name: BUNDLE_PENDING_MARKER.to_owned(),
        });
    }
    let marker: VerificationBundlePendingMarkerV3 =
        serde_json::from_slice(&bytes).map_err(|_| JournalError::ReceiptCorruption {
            name: BUNDLE_PENDING_MARKER.to_owned(),
        })?;
    if canonical_json(&marker)? != bytes {
        return Err(JournalError::ReceiptCorruption {
            name: BUNDLE_PENDING_MARKER.to_owned(),
        });
    }
    marker.validate(identity, limits)?;
    let marker_hash = ContentHash::sha256(&bytes);
    Ok(Some((marker, marker_hash)))
}

fn inspect_bundle_pending_file_locked(
    run: &OwnedFd,
    identity: &JournalIdentity,
    limits: JournalLimits,
    file: &mut File,
) -> Result<Option<BundlePendingInspection>, JournalError> {
    let Some((marker, marker_hash)) = read_bundle_pending_with_hash(run, identity, limits)? else {
        return Ok(None);
    };
    let planned = marker.envelopes(limits)?;
    let mut current_state = scan_with_torn(file, identity, limits, true)?;
    let mut discarded = Vec::new();
    if let Some(torn) = current_state.torn.take() {
        if torn.good_offset < marker.pre_offset {
            return Err(JournalError::ReceiptCorruption {
                name: BUNDLE_PENDING_MARKER.to_owned(),
            });
        }
        current_state.confirmed_offset = torn.good_offset;
        discarded = torn.discarded;
    }
    let pre_bytes = read_prefix(file, marker.pre_offset)?;
    let pre_state = scan_bytes(&pre_bytes, identity, limits, true)?;
    if pre_state.torn.is_some()
        || pre_state.confirmed_offset != marker.pre_offset
        || pre_state.tail_hash != marker.pre_tail_hash
        || current_state.events.len() < pre_state.events.len()
    {
        return Err(JournalError::ReceiptCorruption {
            name: BUNDLE_PENDING_MARKER.to_owned(),
        });
    }
    let confirmed = current_state.events.len() - pre_state.events.len();
    if confirmed > planned.len() {
        return Err(JournalError::ReceiptCorruption {
            name: BUNDLE_PENDING_MARKER.to_owned(),
        });
    }
    for (actual, expected) in current_state.events[pre_state.events.len()..]
        .iter()
        .zip(&planned)
    {
        if actual.id() != expected.id()
            || actual.event_hash() != expected.event_hash()
            || actual.canonical_bytes()? != expected.canonical_bytes()?
        {
            return Err(JournalError::BundleResumeAuthorityMismatch);
        }
    }
    let mut complete_candidate = pre_state.events.clone();
    complete_candidate.extend(planned.iter().cloned());
    validate_prefix(identity, &complete_candidate)?;
    Ok(Some(BundlePendingInspection {
        marker,
        marker_hash,
        planned,
        pre_state,
        current_state,
        confirmed_events: confirmed,
        discarded,
    }))
}

fn read_bundle_stage(run: &OwnedFd, expected: u64) -> Result<u64, JournalError> {
    if !bundle_file_exists(run, BUNDLE_PENDING_STAGE)? {
        // The immutable plan marker is published first and removed last. A
        // missing stage file means stage-zero publication or interrupted
        // cleanup; recovery derives the authoritative stage from the log.
        return Ok(0);
    }
    let fd = fs::openat(
        run,
        BUNDLE_PENDING_STAGE,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(StoreError::Io)?;
    verify_fd_kind_mode(
        &fd,
        "verification bundle stage",
        FileType::RegularFile,
        0o600,
    )?;
    let mut bytes = Vec::new();
    File::from(fd).take(4).read_to_end(&mut bytes)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| JournalError::ReceiptCorruption {
        name: BUNDLE_PENDING_STAGE.to_owned(),
    })?;
    let confirmed = text
        .strip_suffix('\n')
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| JournalError::ReceiptCorruption {
            name: BUNDLE_PENDING_STAGE.to_owned(),
        })?;
    if confirmed > expected {
        return Err(JournalError::ReceiptCorruption {
            name: BUNDLE_PENDING_STAGE.to_owned(),
        });
    }
    Ok(confirmed)
}

fn map_bounded_domain_error(error: reviewgraphen_core::DomainError) -> JournalError {
    match error {
        reviewgraphen_core::DomainError::Incomplete {
            limit, observed, ..
        } => JournalError::Incomplete {
            limit: u64::try_from(limit).unwrap_or(u64::MAX),
            observed: u64::try_from(observed).unwrap_or(u64::MAX),
        },
        other => JournalError::Domain(other),
    }
}

fn map_bundle_resume_domain_error(error: reviewgraphen_core::DomainError) -> JournalError {
    match error {
        reviewgraphen_core::DomainError::BundleResumeAuthorityMismatch => {
            JournalError::BundleResumeAuthorityMismatch
        }
        other => map_bounded_domain_error(other),
    }
}

fn validate_recovery_actor(actor: &str, tool_version: &str) -> Result<(), JournalError> {
    if actor.trim().is_empty()
        || tool_version.trim().is_empty()
        || actor.len() > 1024
        || tool_version.len() > 1024
    {
        return Err(JournalError::Identity(
            "recovery actor and tool version must be non-empty",
        ));
    }
    Ok(())
}

fn map_event_decode_error(
    error: reviewgraphen_core::DomainError,
    good_offset: u64,
) -> JournalError {
    match error {
        reviewgraphen_core::DomainError::Incomplete { .. } => map_bounded_domain_error(error),
        _ => JournalError::CorruptNeedsRecovery {
            good_offset,
            auto_recoverable: false,
        },
    }
}

fn map_index_event_decode_error<E>(
    error: reviewgraphen_core::DomainError,
    good_offset: u64,
) -> IndexReplayError<E> {
    IndexReplayError::Journal(map_event_decode_error(error, good_offset))
}

/// One-pass, fixed-buffer journal validation for the derived-index path.
/// This intentionally has no recovery/torn-tail output: an incomplete final
/// line is an obstruction, never a copied suffix for this read-only caller.
#[allow(dead_code)] // Reached from IndexJournalReader during index migration.
fn scan_index_prefix<E>(
    file: &mut File,
    identity: &JournalIdentity,
    limits: JournalLimits,
    working_limit: u64,
    completed_receipts: &[(RecoveryIntent, RecoveryCompletion)],
    mut visitor: impl FnMut(&EventEnvelope, u64) -> Result<(), E>,
) -> Result<PrefixCertificate, IndexReplayError<E>> {
    // The index path owns exactly one fixed physical-line buffer and one
    // explicitly pre-reserved canonical destination. Core's
    // closed decoder applies its independent structural limits; its transient
    // allocations are not a store-owned retained buffer. The certificate
    // reports the exact maximum canonical writer capacity to the index so it
    // can include that scratch in its own stage accounting.
    let canonical_scratch = limits.max_event_line_bytes.saturating_sub(1);
    let fixed_buffers = limits
        .max_event_line_bytes
        .checked_add(canonical_scratch)
        .ok_or_else(|| {
            IndexReplayError::Journal(JournalError::Incomplete {
                limit: working_limit,
                observed: u64::MAX,
            })
        })?;
    if fixed_buffers > working_limit {
        return Err(IndexReplayError::Journal(JournalError::Incomplete {
            limit: working_limit,
            observed: fixed_buffers,
        }));
    }
    let line_capacity = usize::try_from(limits.max_event_line_bytes).map_err(|_| {
        IndexReplayError::Journal(JournalError::Incomplete {
            limit: limits.max_event_line_bytes,
            observed: u64::MAX,
        })
    })?;
    let mut line = Vec::new();
    line.try_reserve_exact(line_capacity).map_err(|_| {
        IndexReplayError::Journal(JournalError::Incomplete {
            limit: working_limit,
            observed: limits.max_event_line_bytes,
        })
    })?;

    // The audit canonicalizes by (good_offset, recovery_id). A single cursor
    // validates contiguous same-offset groups without another heap-backed
    // map/vector projection in the index replay working set.
    let mut receipt_cursor = 0_usize;
    let mut digest = Sha256::new();
    let mut offset = 0_u64;
    let mut count = 0_u64;
    let mut tail = chain_genesis(identity);
    let canonical_scratch_capacity = canonical_scratch;
    while let Some((intent, completion)) = completed_receipts.get(receipt_cursor) {
        if intent.good_offset != 0 {
            break;
        }
        if intent.pre_tail_hash != tail || completion.post_file_hash != ContentHash::sha256(b"") {
            return Err(IndexReplayError::Journal(JournalError::ReceiptCorruption {
                name: receipt_name(&intent.recovery_id),
            }));
        }
        receipt_cursor += 1;
    }

    file.seek(SeekFrom::Start(0))
        .map_err(JournalError::from)
        .map_err(IndexReplayError::Journal)?;
    let mut read_buf = [0_u8; 8192];
    loop {
        let read = file
            .read(&mut read_buf)
            .map_err(JournalError::from)
            .map_err(IndexReplayError::Journal)?;
        if read == 0 {
            break;
        }
        for byte in &read_buf[..read] {
            offset = offset.checked_add(1).ok_or_else(|| {
                IndexReplayError::Journal(JournalError::Incomplete {
                    limit: limits.max_replay_bytes,
                    observed: u64::MAX,
                })
            })?;
            limit(offset, limits.max_replay_bytes).map_err(IndexReplayError::Journal)?;
            if *byte != b'\n' {
                let next = line.len().checked_add(1).ok_or_else(|| {
                    IndexReplayError::Journal(JournalError::Incomplete {
                        limit: limits.max_event_line_bytes,
                        observed: u64::MAX,
                    })
                })?;
                let next_u64 = u64::try_from(next).map_err(|_| {
                    IndexReplayError::Journal(JournalError::Incomplete {
                        limit: limits.max_event_line_bytes,
                        observed: u64::MAX,
                    })
                })?;
                limit(next_u64.saturating_add(1), limits.max_event_line_bytes)
                    .map_err(IndexReplayError::Journal)?;
                // `line` was pre-reserved at the physical maximum; this
                // cannot grow and so cannot allocate after admission.
                line.push(*byte);
                continue;
            }

            let physical_len = u64::try_from(line.len())
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| {
                    IndexReplayError::Journal(JournalError::Incomplete {
                        limit: limits.max_event_line_bytes,
                        observed: u64::MAX,
                    })
                })?;
            limit(physical_len, limits.max_event_line_bytes).map_err(IndexReplayError::Journal)?;
            count = count.checked_add(1).ok_or_else(|| {
                IndexReplayError::Journal(JournalError::Incomplete {
                    limit: limits.max_events,
                    observed: u64::MAX,
                })
            })?;
            limit(count, limits.max_events).map_err(IndexReplayError::Journal)?;

            let event_start = offset.checked_sub(physical_len).ok_or_else(|| {
                IndexReplayError::Journal(JournalError::Incomplete {
                    limit: limits.max_replay_bytes,
                    observed: u64::MAX,
                })
            })?;
            let event = EventEnvelope::from_json_slice_for_index(&line)
                .map_err(|error| map_index_event_decode_error(error, event_start))?;
            let canonical = event
                .canonical_bytes_for_index(usize::try_from(canonical_scratch).map_err(|_| {
                    IndexReplayError::Journal(JournalError::Incomplete {
                        limit: working_limit,
                        observed: u64::MAX,
                    })
                })?)
                .map_err(JournalError::from)
                .map_err(IndexReplayError::Journal)?;
            if canonical != line {
                return Err(IndexReplayError::Journal(
                    JournalError::CorruptNeedsRecovery {
                        good_offset: event_start,
                        auto_recoverable: false,
                    },
                ));
            }
            event.validate().map_err(|_| {
                IndexReplayError::Journal(JournalError::CorruptNeedsRecovery {
                    good_offset: event_start,
                    auto_recoverable: false,
                })
            })?;
            if event
                .contract_version()
                .map_err(JournalError::from)
                .map_err(IndexReplayError::Journal)?
                != identity.version()
                || event.run_id() != &identity.run_id
                || event.genesis_hash() != &identity.genesis_hash()
                || event.sequence() != count
                || event.previous_event_hash() != &tail
            {
                return Err(IndexReplayError::Journal(
                    JournalError::CorruptNeedsRecovery {
                        good_offset: event_start,
                        auto_recoverable: false,
                    },
                ));
            }
            if count == 1 && identity.version() == EventContractVersion::V2 {
                EventEnvelope::validated_view(
                    identity.version(),
                    &identity.run_id,
                    identity
                        .verified_core_genesis()
                        .map_err(IndexReplayError::Journal)?,
                    std::slice::from_ref(&event),
                )
                .map_err(|_| {
                    IndexReplayError::Journal(JournalError::CorruptNeedsRecovery {
                        good_offset: event_start,
                        auto_recoverable: false,
                    })
                })?;
            }
            digest.update(&line);
            digest.update(b"\n");
            tail = event.event_hash().clone();
            let boundary = event_start.checked_add(physical_len).ok_or_else(|| {
                IndexReplayError::Journal(JournalError::Incomplete {
                    limit: limits.max_replay_bytes,
                    observed: u64::MAX,
                })
            })?;
            if completed_receipts
                .get(receipt_cursor)
                .is_some_and(|(intent, _)| intent.good_offset < boundary)
            {
                let (intent, _) = &completed_receipts[receipt_cursor];
                return Err(IndexReplayError::Journal(JournalError::ReceiptCorruption {
                    name: receipt_name(&intent.recovery_id),
                }));
            }
            if completed_receipts
                .get(receipt_cursor)
                .is_some_and(|(intent, _)| intent.good_offset == boundary)
            {
                let actual = ContentHash::parse(format!("sha256:{:x}", digest.clone().finalize()))
                    .map_err(JournalError::from)
                    .map_err(IndexReplayError::Journal)?;
                while let Some((intent, completion)) = completed_receipts.get(receipt_cursor) {
                    if intent.good_offset != boundary {
                        break;
                    }
                    if intent.pre_tail_hash != tail || completion.post_file_hash != actual {
                        return Err(IndexReplayError::Journal(JournalError::ReceiptCorruption {
                            name: receipt_name(&intent.recovery_id),
                        }));
                    }
                    receipt_cursor += 1;
                }
            }
            visitor(&event, boundary).map_err(IndexReplayError::Visitor)?;
            line.clear();
        }
    }
    if !line.is_empty() {
        // The index never keeps a torn suffix. It only reports its exact
        // confirmed boundary for the explicit recovery workflow.
        return Err(IndexReplayError::Journal(
            JournalError::CorruptNeedsRecovery {
                good_offset: offset.saturating_sub(u64::try_from(line.len()).unwrap_or(u64::MAX)),
                auto_recoverable: true,
            },
        ));
    }
    if identity.version() != EventContractVersion::V1 && count == 0 {
        return Err(IndexReplayError::Journal(JournalError::V2GenesisRequired));
    }
    if let Some((intent, _)) = completed_receipts.get(receipt_cursor) {
        return Err(IndexReplayError::Journal(JournalError::ReceiptCorruption {
            name: receipt_name(&intent.recovery_id),
        }));
    }
    Ok(PrefixCertificate {
        confirmed_offset: offset,
        tail_hash: tail,
        event_count: count,
        line_buffer_capacity: u64::try_from(line.capacity()).unwrap_or(u64::MAX),
        canonical_scratch_capacity,
    })
}

fn scan(
    file: &mut File,
    identity: &JournalIdentity,
    limits: JournalLimits,
    reject_empty_v2: bool,
) -> Result<ScanState, JournalError> {
    scan_with_working_limit(file, identity, limits, reject_empty_v2, None)
}

fn scan_with_working_limit(
    file: &mut File,
    identity: &JournalIdentity,
    limits: JournalLimits,
    reject_empty_v2: bool,
    working_limit: Option<u64>,
) -> Result<ScanState, JournalError> {
    let state =
        scan_with_torn_and_working_limit(file, identity, limits, reject_empty_v2, working_limit)?;
    if let Some(torn) = &state.torn {
        return Err(JournalError::CorruptNeedsRecovery {
            good_offset: torn.good_offset,
            auto_recoverable: true,
        });
    }
    Ok(state)
}

fn scan_with_torn(
    file: &mut File,
    identity: &JournalIdentity,
    limits: JournalLimits,
    reject_empty_v2: bool,
) -> Result<ScanState, JournalError> {
    scan_with_torn_and_working_limit(file, identity, limits, reject_empty_v2, None)
}

fn scan_with_torn_and_working_limit(
    file: &mut File,
    identity: &JournalIdentity,
    limits: JournalLimits,
    reject_empty_v2: bool,
    working_limit: Option<u64>,
) -> Result<ScanState, JournalError> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    let mut buf = [0u8; 8192];
    let mut observed = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        observed = observed
            .checked_add(u64::try_from(n).map_err(|_| JournalError::Incomplete {
                limit: limits.max_replay_bytes,
                observed: u64::MAX,
            })?)
            .ok_or(JournalError::Incomplete {
                limit: limits.max_replay_bytes,
                observed: u64::MAX,
            })?;
        limit(observed, limits.max_replay_bytes)?;
        let needed = bytes.len().checked_add(n).ok_or(JournalError::Incomplete {
            limit: working_limit.unwrap_or(limits.max_replay_bytes),
            observed: u64::MAX,
        })?;
        if needed > bytes.capacity() {
            let prospective = u64::try_from(needed).map_err(|_| JournalError::Incomplete {
                limit: working_limit.unwrap_or(limits.max_replay_bytes),
                observed: u64::MAX,
            })?;
            if working_limit.is_some_and(|limit| prospective > limit) {
                return Err(JournalError::Incomplete {
                    limit: working_limit.expect("checked Some"),
                    observed: prospective,
                });
            }
            bytes
                .try_reserve_exact(needed - bytes.capacity())
                .map_err(|_| JournalError::Incomplete {
                    limit: working_limit.unwrap_or(limits.max_replay_bytes),
                    observed: prospective,
                })?;
        }
        bytes.extend_from_slice(&buf[..n]);
    }
    let raw_capacity = u64::try_from(bytes.capacity()).map_err(|_| JournalError::Incomplete {
        limit: limits.max_replay_bytes,
        observed: u64::MAX,
    })?;
    scan_bytes_with_accounting(
        &bytes,
        raw_capacity,
        identity,
        limits,
        reject_empty_v2,
        working_limit,
    )
}

fn scan_bytes(
    bytes: &[u8],
    identity: &JournalIdentity,
    limits: JournalLimits,
    reject_empty_v2: bool,
) -> Result<ScanState, JournalError> {
    let raw_capacity = u64::try_from(bytes.len()).map_err(|_| JournalError::Incomplete {
        limit: limits.max_replay_bytes,
        observed: u64::MAX,
    })?;
    scan_bytes_with_accounting(bytes, raw_capacity, identity, limits, reject_empty_v2, None)
}

fn scan_bytes_with_accounting(
    bytes: &[u8],
    raw_capacity: u64,
    identity: &JournalIdentity,
    limits: JournalLimits,
    reject_empty_v2: bool,
    working_limit: Option<u64>,
) -> Result<ScanState, JournalError> {
    let mut events = Vec::new();
    let mut starts = Vec::new();
    let mut good = 0u64;
    let mut start = 0usize;
    let mut canonical_scratch_capacity = 0_u64;
    let mut event_heap_capacity = 0_u64;
    while let Some(relative) = bytes[start..].iter().position(|byte| *byte == b'\n') {
        let end = start + relative;
        let next = end + 1;
        limit(
            u64::try_from(next - start).map_err(|_| JournalError::Incomplete {
                limit: limits.max_event_line_bytes,
                observed: u64::MAX,
            })?,
            limits.max_event_line_bytes,
        )?;
        let next_count = u64::try_from(events.len())
            .ok()
            .and_then(|count| count.checked_add(1))
            .ok_or(JournalError::Incomplete {
                limit: limits.max_events,
                observed: u64::MAX,
            })?;
        // Refuse the +1 event before parsing/allocating its decoded payload.
        limit(next_count, limits.max_events)?;
        if events.len() == events.capacity() {
            let prospective_vector = events
                .len()
                .checked_add(1)
                .and_then(|count| count.checked_mul(std::mem::size_of::<EventEnvelope>()))
                .and_then(|bytes| u64::try_from(bytes).ok())
                .ok_or(JournalError::Incomplete {
                    limit: working_limit.unwrap_or(limits.max_replay_bytes),
                    observed: u64::MAX,
                })?;
            let line = u64::try_from(end - start).map_err(|_| JournalError::Incomplete {
                limit: working_limit.unwrap_or(limits.max_replay_bytes),
                observed: u64::MAX,
            })?;
            let reservation = raw_capacity
                .checked_add(event_heap_capacity)
                .and_then(|value| value.checked_add(prospective_vector))
                .and_then(|value| value.checked_add(line))
                .and_then(|value| value.checked_add(line.checked_mul(2)?))
                .ok_or(JournalError::Incomplete {
                    limit: working_limit.unwrap_or(limits.max_replay_bytes),
                    observed: u64::MAX,
                })?;
            if working_limit.is_some_and(|limit| reservation > limit) {
                return Err(JournalError::Incomplete {
                    limit: working_limit.expect("checked Some"),
                    observed: reservation,
                });
            }
            events
                .try_reserve_exact(1)
                .map_err(|_| JournalError::Incomplete {
                    limit: working_limit.unwrap_or(limits.max_replay_bytes),
                    observed: reservation,
                })?;
        }
        let retained_vector = events
            .capacity()
            .checked_mul(std::mem::size_of::<EventEnvelope>())
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or(JournalError::Incomplete {
                limit: working_limit.unwrap_or(limits.max_replay_bytes),
                observed: u64::MAX,
            })?;
        let line = u64::try_from(end - start).map_err(|_| JournalError::Incomplete {
            limit: working_limit.unwrap_or(limits.max_replay_bytes),
            observed: u64::MAX,
        })?;
        let reservation = raw_capacity
            .checked_add(event_heap_capacity)
            .and_then(|value| value.checked_add(retained_vector))
            .and_then(|value| value.checked_add(line))
            .and_then(|value| value.checked_add(line.checked_mul(2)?))
            .ok_or(JournalError::Incomplete {
                limit: working_limit.unwrap_or(limits.max_replay_bytes),
                observed: u64::MAX,
            })?;
        if working_limit.is_some_and(|limit| reservation > limit) {
            return Err(JournalError::Incomplete {
                limit: working_limit.expect("checked Some"),
                observed: reservation,
            });
        }
        let event = EventEnvelope::from_json_slice(&bytes[start..end])
            .map_err(|error| map_event_decode_error(error, good))?;
        let canonical = event.canonical_bytes().map_err(JournalError::Domain)?;
        canonical_scratch_capacity =
            canonical_scratch_capacity.max(u64::try_from(canonical.capacity()).map_err(|_| {
                JournalError::Incomplete {
                    limit: limits.max_replay_bytes,
                    observed: u64::MAX,
                }
            })?);
        if canonical != bytes[start..end] {
            return Err(JournalError::CorruptNeedsRecovery {
                good_offset: good,
                auto_recoverable: false,
            });
        }
        event_heap_capacity = event_heap_capacity
            .checked_add(u64::try_from(event.allocated_bytes()).map_err(|_| {
                JournalError::Incomplete {
                    limit: working_limit.unwrap_or(limits.max_replay_bytes),
                    observed: u64::MAX,
                }
            })?)
            .ok_or(JournalError::Incomplete {
                limit: working_limit.unwrap_or(limits.max_replay_bytes),
                observed: u64::MAX,
            })?;
        starts.push(u64::try_from(start).map_err(|_| JournalError::Incomplete {
            limit: limits.max_replay_bytes,
            observed: u64::MAX,
        })?);
        events.push(event);
        good = u64::try_from(next).map_err(|_| JournalError::Incomplete {
            limit: limits.max_replay_bytes,
            observed: u64::MAX,
        })?;
        start = next;
    }
    if start < bytes.len() {
        limit(
            u64::try_from(bytes.len() - start).map_err(|_| JournalError::Incomplete {
                limit: limits.max_event_line_bytes,
                observed: u64::MAX,
            })?,
            limits.max_event_line_bytes,
        )?;
    }
    if validate_prefix(identity, &events).is_err() {
        // Locate the first bad chain/sequence/link without quadratic prefix
        // replay. Validation is monotone: once a prefix fails, extensions do.
        let mut low = 0usize;
        let mut high = events.len();
        while low < high {
            let mid = low + (high - low) / 2;
            if validate_prefix(identity, &events[..=mid]).is_ok() {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        return Err(JournalError::CorruptNeedsRecovery {
            good_offset: starts.get(low).copied().unwrap_or(0),
            auto_recoverable: false,
        });
    }
    let tail = if start < bytes.len() {
        let prefix = events.last().map_or_else(
            || chain_genesis(identity),
            |event| event.event_hash().clone(),
        );
        Some(TornTail {
            good_offset: good,
            discarded: bytes[start..].to_vec(),
            pre_tail_hash: prefix,
        })
    } else {
        None
    };
    if tail.is_some() {
        if reject_empty_v2 && identity.version() != EventContractVersion::V1 && events.is_empty() {
            return Err(JournalError::V2GenesisRequired);
        }
        let tail_hash = events.last().map_or_else(
            || chain_genesis(identity),
            |event| event.event_hash().clone(),
        );
        return Ok(ScanState {
            events,
            confirmed_offset: good,
            tail_hash,
            torn: tail,
        });
    }
    if reject_empty_v2 && identity.version() != EventContractVersion::V1 && events.is_empty() {
        return Err(JournalError::V2GenesisRequired);
    }
    let tail_hash = events.last().map_or_else(
        || chain_genesis(identity),
        |event| event.event_hash().clone(),
    );
    let offset = u64::try_from(bytes.len()).map_err(|_| JournalError::Incomplete {
        limit: limits.max_replay_bytes,
        observed: u64::MAX,
    })?;
    Ok(ScanState {
        events,
        confirmed_offset: offset,
        tail_hash,
        torn: None,
    })
}

fn validate_prefix(
    identity: &JournalIdentity,
    events: &[EventEnvelope],
) -> Result<(), JournalError> {
    if identity.version() == EventContractVersion::V4 {
        let JournalGenesis::V4Shared(bytes) = &identity.genesis else {
            return Err(JournalError::Identity(
                "V4 journal identity requires verified canonical genesis bytes",
            ));
        };
        EventEnvelope::validate_v4_stream(&identity.run_id, bytes, events)?;
        return Ok(());
    }
    if identity.version() == EventContractVersion::V5 {
        let JournalGenesis::V5Shared(bytes) = &identity.genesis else {
            return Err(JournalError::Identity(
                "V5 journal identity requires verified canonical genesis bytes",
            ));
        };
        EventEnvelope::validate_v5_stream_for_store(&identity.run_id, bytes, events)?;
        return Ok(());
    }
    let _ = EventEnvelope::validated_view(
        identity.version(),
        &identity.run_id,
        identity.verified_core_genesis()?,
        events,
    )?;
    Ok(())
}

fn chain_genesis(identity: &JournalIdentity) -> ContentHash {
    // This is the exact core sentinel construction, kept in canonical data
    // form because core intentionally does not expose a mutable chain cursor.
    let bindings = std::collections::BTreeMap::from([
        (
            "domain".to_owned(),
            serde_json::Value::String("reviewgraphen.event_chain_genesis.v1".to_owned()),
        ),
        (
            "genesis_hash".to_owned(),
            serde_json::Value::String(identity.genesis_hash().to_string()),
        ),
        (
            "run".to_owned(),
            serde_json::Value::String(identity.run_id.to_string()),
        ),
    ]);
    ContentHash::sha256(
        &canonical_json(&bindings).expect("fixed canonical sentinel map serializes"),
    )
}
fn limit(observed: u64, maximum: u64) -> Result<(), JournalError> {
    if observed > maximum {
        Err(JournalError::Incomplete {
            limit: maximum,
            observed,
        })
    } else {
        Ok(())
    }
}

fn validate_limits(limits: JournalLimits) -> Result<(), JournalError> {
    if limits.max_event_line_bytes == 0
        || limits.max_events == 0
        || limits.max_replay_bytes == 0
        || limits.max_receipt_files == 0
        || limits.max_receipt_scan_bytes == 0
        || limits.max_receipt_bytes == 0
    {
        Err(JournalError::Identity("journal limits must be nonzero"))
    } else {
        Ok(())
    }
}

/// The filesystem name for a run is deliberately not the StableId text: a
/// StableId payload may contain '/' and must never alter descriptor-relative
/// traversal.  The complete SHA-256 digest has no collision truncation.
fn run_dir_name(run_id: &StableId) -> String {
    ContentHash::sha256(run_id.to_string().as_bytes()).to_string()[7..].to_owned()
}

fn open_verified_regular(
    parent: &OwnedFd,
    name: &str,
    label: &'static str,
    writable: bool,
) -> Result<OwnedFd, JournalError> {
    let before = fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
        if error == Errno::NOENT {
            JournalError::Missing
        } else {
            JournalError::Store(StoreError::Io(error))
        }
    })?;
    if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile || before.st_size < 0 {
        return Err(JournalError::ReceiptCorruption {
            name: name.to_owned(),
        });
    }
    let fd = fs::openat(
        parent,
        name,
        (if writable {
            OFlags::RDWR
        } else {
            OFlags::RDONLY
        }) | OFlags::CLOEXEC
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|error| {
        if error == Errno::NOENT {
            JournalError::Missing
        } else {
            JournalError::Store(StoreError::Io(error))
        }
    })?;
    verify_fd_kind_mode(&fd, label, FileType::RegularFile, 0o600)?;
    let after = fs::fstat(&fd).map_err(StoreError::Io)?;
    if before.st_dev != after.st_dev
        || before.st_ino != after.st_ino
        || before.st_size != after.st_size
    {
        return Err(JournalError::ReceiptCorruption {
            name: name.to_owned(),
        });
    }
    Ok(fd)
}

/// The main log is intentionally mutable while another process waits on the
/// flock.  Bind identity/type before opening, but do not compare a stale size
/// observed before the lock; the lock holder reads the authoritative size.
fn open_verified_log(parent: &OwnedFd, writable: bool) -> Result<OwnedFd, JournalError> {
    let before = fs::statat(parent, JOURNAL_FILE, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
        if error == Errno::NOENT {
            JournalError::Missing
        } else {
            JournalError::Store(StoreError::Io(error))
        }
    })?;
    if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile || before.st_size < 0 {
        return Err(JournalError::ReceiptCorruption {
            name: JOURNAL_FILE.to_owned(),
        });
    }
    let fd = fs::openat(
        parent,
        JOURNAL_FILE,
        (if writable {
            OFlags::RDWR
        } else {
            OFlags::RDONLY
        }) | OFlags::CLOEXEC
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(StoreError::Io)?;
    verify_fd_kind_mode(&fd, "journal event log", FileType::RegularFile, 0o600)?;
    let after = fs::fstat(&fd).map_err(StoreError::Io)?;
    if before.st_dev != after.st_dev || before.st_ino != after.st_ino {
        return Err(JournalError::ReceiptCorruption {
            name: JOURNAL_FILE.to_owned(),
        });
    }
    Ok(fd)
}

fn marker_exists(run: &OwnedFd, name: &str, expected: &[u8]) -> Result<bool, JournalError> {
    match open_verified_regular(run, name, "journal pending marker", false) {
        Ok(fd) => {
            let mut bytes = Vec::with_capacity(expected.len().saturating_add(1));
            File::from(fd)
                .take((expected.len() as u64).saturating_add(1))
                .read_to_end(&mut bytes)?;
            if bytes != expected {
                return Err(JournalError::ReceiptCorruption {
                    name: name.to_owned(),
                });
            }
            Ok(true)
        }
        Err(JournalError::Missing) => Ok(false),
        Err(error) => Err(error),
    }
}

fn ensure_v4_append_pending(run: &OwnedFd) -> Result<(), JournalError> {
    if marker_exists(run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES)? {
        return Ok(());
    }
    let fd = fs::openat(
        run,
        APPEND_PENDING_MARKER,
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )
    .map_err(StoreError::Io)?;
    let mut marker = File::from(fd);
    marker.write_all(APPEND_PENDING_BYTES)?;
    marker.sync_all()?;
    fs::fsync(run).map_err(StoreError::Io)?;
    Ok(())
}

#[cfg(any(test, feature = "test-support"))]
fn restore_append_marker(run: &OwnedFd) -> Result<(), JournalError> {
    let fd = fs::openat(
        run,
        APPEND_PENDING_MARKER,
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_raw_mode(0o600),
    )
    .map_err(StoreError::Io)?;
    let mut marker = File::from(fd);
    marker.write_all(APPEND_PENDING_BYTES)?;
    marker.sync_all()?;
    fs::fsync(run).map_err(StoreError::Io)?;
    Ok(())
}

fn publish_initial_log(
    run: &OwnedFd,
    line: &[u8],
    identity: &JournalIdentity,
    limits: JournalLimits,
) -> Result<(), JournalError> {
    // The inode cannot be observed by a reader until it is fully initialized.
    let fd = fs::openat(
        run,
        ".",
        OFlags::TMPFILE | OFlags::RDWR | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map_err(|_| JournalError::Store(StoreError::UnsupportedPlatform))?;
    verify_fd_kind_mode(
        &fd,
        "journal event log temporary",
        FileType::RegularFile,
        0o600,
    )?;
    let mut file = File::from(fd);
    file.write_all(line)?;
    file.sync_all()?;
    match fs::linkat(&file, "", run, JOURNAL_FILE, AtFlags::EMPTY_PATH) {
        Ok(()) => fs::fsync(run)
            .map_err(StoreError::Io)
            .map_err(JournalError::from),
        Err(Errno::EXIST) => {
            drop(file);
            let fd = open_verified_log(run, false)?;
            let mut existing = File::from(fd);
            // A retry must observe one stable log.  Holding the shared lock
            // through the exact first-record comparison and directory fsync
            // prevents a concurrent append from changing the basis midway.
            fs::flock(existing.as_fd(), FlockOperation::LockShared).map_err(StoreError::Io)?;
            let state = scan(&mut existing, identity, limits, true)?;
            if state.events.first().is_some_and(|first| {
                first.canonical_bytes().ok().as_deref() == Some(&line[..line.len() - 1])
            }) {
                fs::fsync(run)
                    .map_err(StoreError::Io)
                    .map_err(JournalError::from)
            } else {
                Err(JournalError::ReceiptCorruption {
                    name: JOURNAL_FILE.to_owned(),
                })
            }
        }
        Err(error) => Err(JournalError::Store(StoreError::Io(error))),
    }
}

/// Create-only event-v4 genesis publisher.  Before link publication every
/// failure is known-not-committed; after link publication any missing durable
/// acknowledgement is uncertain and must be resolved by keyed recovery.
fn publish_initial_log_v4(run: &OwnedFd, line: &[u8]) -> Result<(), JournalError> {
    let fd = fs::openat(
        run,
        ".",
        OFlags::TMPFILE | OFlags::RDWR | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map_err(|_| JournalError::GenesisNotCommittedV4 {
        stage: "temporary create",
    })?;
    verify_fd_kind_mode(
        &fd,
        "event-v4 genesis temporary",
        FileType::RegularFile,
        0o600,
    )
    .map_err(|_| JournalError::GenesisNotCommittedV4 {
        stage: "temporary validation",
    })?;
    let mut file = File::from(fd);
    #[cfg(test)]
    if take_v4_bootstrap_fault(V4BootstrapFault::BeforeLineWrite) {
        return Err(JournalError::GenesisNotCommittedV4 {
            stage: "before line write",
        });
    }
    file.write_all(line)
        .map_err(|_| JournalError::GenesisNotCommittedV4 {
            stage: "line write",
        })?;
    #[cfg(test)]
    if take_v4_bootstrap_fault(V4BootstrapFault::BeforeFileSync) {
        return Err(JournalError::GenesisNotCommittedV4 {
            stage: "before file sync",
        });
    }
    file.sync_all()
        .map_err(|_| JournalError::GenesisNotCommittedV4 { stage: "file sync" })?;
    #[cfg(test)]
    if take_v4_bootstrap_fault(V4BootstrapFault::AfterFileSync) {
        return Err(JournalError::GenesisNotCommittedV4 {
            stage: "after file sync",
        });
    }
    match fs::linkat(&file, "", run, JOURNAL_FILE, AtFlags::EMPTY_PATH) {
        Ok(()) => {}
        Err(Errno::EXIST) => {
            return Err(JournalError::ReceiptCorruption {
                name: JOURNAL_FILE.to_owned(),
            });
        }
        Err(_) => {
            return Err(JournalError::GenesisNotCommittedV4 {
                stage: "create-only link",
            });
        }
    }
    #[cfg(test)]
    if take_v4_bootstrap_fault(V4BootstrapFault::AfterLink) {
        return Err(JournalError::GenesisSessionUncertainV4);
    }
    fs::fsync(run).map_err(|_| JournalError::GenesisSessionUncertainV4)?;
    #[cfg(test)]
    if take_v4_bootstrap_fault(V4BootstrapFault::AfterDirectorySync) {
        return Err(JournalError::GenesisSessionUncertainV4);
    }
    Ok(())
}

fn publish_initial_log_v5(run: &OwnedFd, line: &[u8]) -> Result<(), JournalError> {
    let fd = fs::openat(
        run,
        ".",
        OFlags::TMPFILE | OFlags::RDWR | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map_err(|_| JournalError::GenesisNotCommittedV5 {
        stage: "temporary create",
    })?;
    verify_fd_kind_mode(
        &fd,
        "event-v5 genesis temporary",
        FileType::RegularFile,
        0o600,
    )
    .map_err(|_| JournalError::GenesisNotCommittedV5 {
        stage: "temporary validation",
    })?;
    let mut file = File::from(fd);
    file.write_all(line)
        .map_err(|_| JournalError::GenesisNotCommittedV5 {
            stage: "line write",
        })?;
    file.sync_all()
        .map_err(|_| JournalError::GenesisNotCommittedV5 { stage: "file sync" })?;
    match fs::linkat(&file, "", run, JOURNAL_FILE, AtFlags::EMPTY_PATH) {
        Ok(()) => {}
        Err(Errno::EXIST) => {
            return Err(JournalError::ReceiptCorruption {
                name: JOURNAL_FILE.to_owned(),
            });
        }
        Err(_) => {
            return Err(JournalError::GenesisNotCommittedV5 {
                stage: "create-only link",
            });
        }
    }
    fs::fsync(run).map_err(|_| JournalError::GenesisSessionUncertainV5)
}

/// Open one already-existing, independently verified component.  This is
/// deliberately separate from the creator: reader/recovery paths must never
/// manufacture a durable namespace while diagnosing a missing/corrupt run.
fn open_existing_dir(
    parent: &OwnedFd,
    name: &str,
    label: &'static str,
) -> Result<OwnedFd, JournalError> {
    let fd = fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| {
        if error == Errno::NOENT {
            JournalError::Missing
        } else {
            JournalError::Store(StoreError::Io(error))
        }
    })?;
    verify_fd_kind_mode(&fd, label, FileType::Directory, 0o700)?;
    Ok(fd)
}
fn read_prefix(file: &mut File, end: u64) -> Result<Vec<u8>, JournalError> {
    file.seek(SeekFrom::Start(0))?;
    let cap = usize::try_from(end).map_err(|_| JournalError::Incomplete {
        limit: end,
        observed: end,
    })?;
    let mut bytes = Vec::with_capacity(cap);
    let mut prefix = file.take(end);
    prefix.read_to_end(&mut bytes)?;
    Ok(bytes)
}
fn read_suffix(file: &mut File, start: u64, maximum: u64) -> Result<Vec<u8>, JournalError> {
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    let mut limited = file.take(maximum.saturating_add(1));
    limited.read_to_end(&mut bytes)?;
    limit(bytes.len() as u64, maximum)?;
    Ok(bytes)
}
fn receipt_name(id: &StableId) -> String {
    format!(
        "{}.json",
        ContentHash::sha256(id.to_string().as_bytes())
            .to_string()
            .trim_start_matches("sha256:")
    )
}

fn recovery_id(
    run_id: &StableId,
    genesis_hash: &ContentHash,
    good_offset: u64,
    discarded_hash: &ContentHash,
    nonce: &str,
) -> Result<StableId, JournalError> {
    let bindings = std::collections::BTreeMap::from([
        (
            "discarded_hash".to_owned(),
            serde_json::Value::String(discarded_hash.to_string()),
        ),
        (
            "good_offset".to_owned(),
            serde_json::Value::Number(good_offset.into()),
        ),
        (
            "genesis_hash".to_owned(),
            serde_json::Value::String(genesis_hash.to_string()),
        ),
        (
            "nonce".to_owned(),
            serde_json::Value::String(nonce.to_owned()),
        ),
        (
            "run_id".to_owned(),
            serde_json::Value::String(run_id.to_string()),
        ),
    ]);
    Ok(StableId::derived("recovery", &bindings)?)
}

fn recovery_nonce() -> Result<String, JournalError> {
    let mut bytes = [0u8; 32];
    getrandom(&mut bytes, GetRandomFlags::empty()).map_err(StoreError::Io)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn bundle_recovery_nonce(
    identity: &JournalIdentity,
    bundle_digest: &ContentHash,
    bundle_pre_offset: u64,
    good_offset: u64,
    discarded_hash: &ContentHash,
    discarded_len: u64,
    recovery_kind: &str,
) -> Result<String, JournalError> {
    let bindings = std::collections::BTreeMap::from([
        (
            "domain".to_owned(),
            serde_json::Value::String("reviewgraphen.bundle-recovery.v1".to_owned()),
        ),
        (
            "run_id".to_owned(),
            serde_json::Value::String(identity.run_id.to_string()),
        ),
        (
            "genesis_hash".to_owned(),
            serde_json::Value::String(identity.genesis_hash().to_string()),
        ),
        (
            "bundle_digest".to_owned(),
            serde_json::Value::String(bundle_digest.to_string()),
        ),
        (
            "bundle_pre_offset".to_owned(),
            serde_json::Value::Number(bundle_pre_offset.into()),
        ),
        (
            "good_offset".to_owned(),
            serde_json::Value::Number(good_offset.into()),
        ),
        (
            "discarded_offset".to_owned(),
            serde_json::Value::Number(good_offset.into()),
        ),
        (
            "discarded_len".to_owned(),
            serde_json::Value::Number(discarded_len.into()),
        ),
        (
            "discarded_hash".to_owned(),
            serde_json::Value::String(discarded_hash.to_string()),
        ),
        (
            "cleanup_kind".to_owned(),
            serde_json::Value::String(recovery_kind.to_owned()),
        ),
    ]);
    Ok(ContentHash::sha256(&canonical_json(&bindings)?)
        .as_str()
        .trim_start_matches("sha256:")
        .to_owned())
}

fn same_bundle_recovery_lineage(left: &RecoveryIntent, right: &RecoveryIntent) -> bool {
    left.bundle_digest.is_some()
        && right.bundle_digest.is_some()
        && left.run_id == right.run_id
        && left.genesis_hash == right.genesis_hash
        && left.bundle_digest == right.bundle_digest
        && left.bundle_pre_offset == right.bundle_pre_offset
        && left.good_offset == right.good_offset
        && left.discarded_offset == right.discarded_offset
        && left.discarded_len == right.discarded_len
        && left.pre_size == right.pre_size
        && left.bundle_recovery_kind == right.bundle_recovery_kind
}

fn bundle_discard_ranges_overlap(left: &RecoveryIntent, right: &RecoveryIntent) -> bool {
    match (
        left.discarded_offset,
        left.pre_size,
        right.discarded_offset,
        right.pre_size,
    ) {
        (Some(left_start), Some(left_end), Some(right_start), Some(right_end)) => {
            left_start < right_end && right_start < left_end
        }
        _ => false,
    }
}

/// Pure inventory predicate shared by audit, recovery, and rollback paths.
/// Exact bundle intents are idempotent; every other legacy/mixed duplicate,
/// same-lineage mismatch, or overlapping destructive range conflicts.
fn recovery_inventory_conflicts(existing: &RecoveryIntent, proposed: &RecoveryIntent) -> bool {
    if existing.good_offset != proposed.good_offset
        && !bundle_discard_ranges_overlap(existing, proposed)
    {
        return false;
    }
    let both_bundle = existing.bundle_digest.is_some() && proposed.bundle_digest.is_some();
    if !both_bundle {
        return existing.good_offset == proposed.good_offset;
    }
    if same_bundle_recovery_lineage(existing, proposed) {
        return existing != proposed;
    }
    bundle_discard_ranges_overlap(existing, proposed)
}

fn validate_proposed_recovery_lineage(
    audit: &RecoveryAudit,
    proposed: &RecoveryIntent,
) -> Result<(), JournalError> {
    if let Some(existing) = audit
        .pending
        .iter()
        .chain(audit.completed.iter().map(|(intent, _)| intent))
        .find(|existing| {
            existing.recovery_id != proposed.recovery_id
                && recovery_inventory_conflicts(existing, proposed)
        })
    {
        return Err(JournalError::ReceiptCorruption {
            name: receipt_name(&existing.recovery_id),
        });
    }
    Ok(())
}

fn read_receipt<T: for<'de> Deserialize<'de> + Serialize>(
    dir: &OwnedFd,
    name: &str,
    limits: JournalLimits,
) -> Result<Option<T>, JournalError> {
    if !valid_receipt_name(name) {
        return Err(JournalError::ReceiptCorruption {
            name: name.to_owned(),
        });
    }
    let before = match fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(Errno::NOENT) => return Ok(None),
        Err(error) => return Err(JournalError::Store(StoreError::Io(error))),
    };
    if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile
        || before.st_size < 0
        || u64::try_from(before.st_size)
            .ok()
            .is_none_or(|size| size > limits.max_receipt_bytes)
    {
        return Err(JournalError::ReceiptCorruption {
            name: name.to_owned(),
        });
    }
    let fd = match fs::openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(error) => return Err(JournalError::Store(StoreError::Io(error))),
    };
    verify_fd_kind_mode(&fd, "recovery receipt", FileType::RegularFile, 0o600)?;
    let after = fs::fstat(&fd).map_err(StoreError::Io)?;
    if after.st_ino != before.st_ino
        || after.st_dev != before.st_dev
        || after.st_size != before.st_size
    {
        return Err(JournalError::ReceiptCorruption {
            name: name.to_owned(),
        });
    }
    let mut actual = Vec::new();
    File::from(fd)
        .take(limits.max_receipt_bytes.saturating_add(1))
        .read_to_end(&mut actual)?;
    if actual.len() as u64 > limits.max_receipt_bytes {
        return Err(JournalError::ReceiptCorruption {
            name: name.to_owned(),
        });
    }
    let parsed: T =
        serde_json::from_slice(&actual).map_err(|_| JournalError::ReceiptCorruption {
            name: name.to_owned(),
        })?;
    if canonical_json(&parsed)? != actual {
        return Err(JournalError::ReceiptCorruption {
            name: name.to_owned(),
        });
    }
    Ok(Some(parsed))
}

fn recovery_audit(
    intents: &OwnedFd,
    completions: &OwnedFd,
    identity: &JournalIdentity,
    limits: JournalLimits,
) -> Result<RecoveryAudit, JournalError> {
    let mut count = 0u64;
    let mut scanned = 0u64;
    let intent_names = receipt_names(intents, limits, &mut count, &mut scanned)?;
    let completion_names = receipt_names(completions, limits, &mut count, &mut scanned)?;
    recovery_audit_from_names(
        intents,
        completions,
        identity,
        limits,
        intent_names,
        completion_names,
        count,
        scanned,
    )
}

#[allow(clippy::too_many_arguments)]
fn recovery_audit_from_names(
    intents: &OwnedFd,
    completions: &OwnedFd,
    identity: &JournalIdentity,
    limits: JournalLimits,
    intent_names: Vec<String>,
    completion_names: Vec<String>,
    count: u64,
    scanned: u64,
) -> Result<RecoveryAudit, JournalError> {
    let mut intents_by_id = Vec::new();
    intents_by_id
        .try_reserve_exact(intent_names.len())
        .map_err(|_| JournalError::Incomplete {
            limit: limits.max_receipt_scan_bytes,
            observed: u64::MAX,
        })?;
    for name in intent_names {
        let intent = read_receipt::<RecoveryIntent>(intents, &name, limits)?
            .ok_or_else(|| JournalError::ReceiptCorruption { name: name.clone() })?;
        validate_intent_shape(&intent, &name, identity, limits)?;
        if name != receipt_name(&intent.recovery_id)
            || intents_by_id
                .iter()
                .any(|known: &RecoveryIntent| known.recovery_id == intent.recovery_id)
        {
            return Err(JournalError::ReceiptCorruption { name });
        }
        intents_by_id.push(intent);
    }
    let mut completed = Vec::new();
    completed
        .try_reserve_exact(completion_names.len())
        .map_err(|_| JournalError::Incomplete {
            limit: limits.max_receipt_scan_bytes,
            observed: u64::MAX,
        })?;
    for name in completion_names {
        let completion = read_receipt::<RecoveryCompletion>(completions, &name, limits)?
            .ok_or_else(|| JournalError::ReceiptCorruption { name: name.clone() })?;
        if name != receipt_name(&completion.recovery_id)
            || !intents_by_id
                .iter()
                .any(|intent| intent.recovery_id == completion.recovery_id)
            || completed
                .iter()
                .any(|(_, known): &(RecoveryIntent, RecoveryCompletion)| {
                    known.recovery_id == completion.recovery_id
                })
        {
            return Err(JournalError::ReceiptCorruption { name });
        }
        let intent = intents_by_id
            .iter()
            .find(|intent| intent.recovery_id == completion.recovery_id)
            .expect("completion intent existence was checked")
            .clone();
        completed.push((intent, completion));
    }
    for (index, (intent, _)) in completed.iter().enumerate() {
        for (other, _) in &completed[index + 1..] {
            if recovery_inventory_conflicts(intent, other)
                || recovery_inventory_conflicts(other, intent)
            {
                return Err(JournalError::ReceiptCorruption {
                    name: "duplicate or overlapping recovery lineage".to_owned(),
                });
            }
        }
    }
    let mut pending = None;
    for intent in intents_by_id {
        if !completed
            .iter()
            .any(|(_, completion)| completion.recovery_id == intent.recovery_id)
            && pending.replace(intent).is_some()
        {
            return Err(JournalError::ReceiptCorruption {
                name: "multiple pending recovery intents".to_owned(),
            });
        }
    }
    if let Some(pending_intent) = &pending {
        for (completed_intent, _) in &completed {
            if recovery_inventory_conflicts(completed_intent, pending_intent)
                || recovery_inventory_conflicts(pending_intent, completed_intent)
            {
                return Err(JournalError::ReceiptCorruption {
                    name: "pending recovery conflicts with completed lineage".to_owned(),
                });
            }
        }
    }
    completed.sort_by(|left, right| {
        left.0
            .good_offset
            .cmp(&right.0.good_offset)
            .then_with(|| left.0.recovery_id.cmp(&right.0.recovery_id))
    });
    Ok(RecoveryAudit {
        pending,
        completed,
        receipt_files: count,
        receipt_scan_bytes: scanned,
    })
}

/// Admit the largest store-visible recovery-audit shape before any receipt is
/// read or deserialized. The final retained-metadata check remains exact; this
/// conservative pass accounts for temporary receipt bytes, canonical scratch,
/// both audit vectors, the cloned completed intent, and discovered names.
fn preflight_index_recovery_audit(
    intent_count: usize,
    completion_count: usize,
    max_receipt: u64,
    receipt_bytes: u64,
    fixed: u64,
    working_limit: u64,
) -> Result<(), JournalError> {
    let names = intent_count.saturating_add(completion_count);
    let name_heap = u64::try_from(names)
        .unwrap_or(u64::MAX)
        .checked_mul(69)
        .ok_or(JournalError::Incomplete {
            limit: working_limit,
            observed: u64::MAX,
        })?;
    let intent_layout = u64::try_from(intent_count)
        .unwrap_or(u64::MAX)
        .checked_mul(u64::try_from(std::mem::size_of::<RecoveryIntent>()).unwrap_or(u64::MAX))
        .ok_or(JournalError::Incomplete {
            limit: working_limit,
            observed: u64::MAX,
        })?;
    let completed_layout = u64::try_from(completion_count)
        .unwrap_or(u64::MAX)
        .checked_mul(
            u64::try_from(std::mem::size_of::<(RecoveryIntent, RecoveryCompletion)>())
                .unwrap_or(u64::MAX),
        )
        .ok_or(JournalError::Incomplete {
            limit: working_limit,
            observed: u64::MAX,
        })?;
    let name_layout = u64::try_from(names)
        .unwrap_or(u64::MAX)
        .checked_mul(u64::try_from(std::mem::size_of::<String>()).unwrap_or(u64::MAX))
        .ok_or(JournalError::Incomplete {
            limit: working_limit,
            observed: u64::MAX,
        })?;
    let requested = fixed
        // Parsed fields are a subset of their canonical receipt bytes; a
        // completed pair additionally clones its corresponding intent.
        .checked_add(receipt_bytes.saturating_mul(2))
        // One raw receipt and one canonicalization destination coexist.
        .and_then(|value| value.checked_add(max_receipt.saturating_mul(2)))
        .and_then(|value| value.checked_add(intent_layout))
        .and_then(|value| value.checked_add(completed_layout))
        .and_then(|value| value.checked_add(name_layout))
        .and_then(|value| value.checked_add(name_heap))
        .ok_or(JournalError::Incomplete {
            limit: working_limit,
            observed: u64::MAX,
        })?;
    limit(requested, working_limit)
}

/// A receipt discovered by an audit is not trusted merely because a directory
/// lookup can observe it: a crash between link and parent-directory fsync must
/// not let a resumed recovery make an orphan completion durable.  The lock
/// holder establishes that durability barrier before any log mutation.
fn sync_recovery_dirs(intents: &OwnedFd, completions: &OwnedFd) -> Result<(), JournalError> {
    fs::fsync(intents).map_err(StoreError::Io)?;
    fs::fsync(completions).map_err(StoreError::Io)?;
    Ok(())
}

/// Reserve the two immutable receipts which an append rollback or recovery
/// transaction may need before it mutates the log or publishes either one.
/// The reservation is deliberately worst-case (the configured one-receipt
/// maximum), so a later serialization detail can never turn an admitted
/// mutation into an over-limit receipt directory.
fn reserve_recovery_capacity(
    audit: &RecoveryAudit,
    limits: JournalLimits,
    receipt_sizes: &[u64],
) -> Result<(), JournalError> {
    let receipts = u64::try_from(receipt_sizes.len()).map_err(|_| JournalError::Incomplete {
        limit: limits.max_receipt_files,
        observed: u64::MAX,
    })?;
    let files = audit
        .receipt_files
        .checked_add(receipts)
        .ok_or(JournalError::Incomplete {
            limit: limits.max_receipt_files,
            observed: u64::MAX,
        })?;
    limit(files, limits.max_receipt_files)?;
    let receipt_bytes = receipt_sizes.iter().try_fold(0u64, |total, size| {
        limit(*size, limits.max_receipt_bytes)?;
        total.checked_add(*size).ok_or(JournalError::Incomplete {
            limit: limits.max_receipt_scan_bytes,
            observed: u64::MAX,
        })
    })?;
    let scanned =
        audit
            .receipt_scan_bytes
            .checked_add(receipt_bytes)
            .ok_or(JournalError::Incomplete {
                limit: limits.max_receipt_scan_bytes,
                observed: u64::MAX,
            })?;
    limit(scanned, limits.max_receipt_scan_bytes)
}

fn receipt_bytes<T: Serialize>(receipt: &T) -> Result<u64, JournalError> {
    u64::try_from(canonical_json(receipt)?.len()).map_err(|_| JournalError::Incomplete {
        limit: u64::MAX,
        observed: u64::MAX,
    })
}

/// A failed append has not yet observed the discarded suffix, but all
/// variable-width receipt fields are bounded before the pre-marker exists.
/// Hashes, IDs, and nonces are fixed-width; using the largest decimal offset
/// and timestamp gives a conservative canonical-size bound without allowing a
/// too-small per-receipt limit to be discovered after the log was touched.
fn rollback_receipt_size_bound(
    identity: &JournalIdentity,
    pre_tail_offset: u64,
    pre_tail_hash: &ContentHash,
) -> Result<[u64; 2], JournalError> {
    let nonce = "0".repeat(64);
    let discarded_hash = ContentHash::sha256(b"");
    let recovery_id = recovery_id(
        &identity.run_id,
        &identity.genesis_hash(),
        u64::MAX,
        &discarded_hash,
        &nonce,
    )?;
    let intent = RecoveryIntent {
        recovery_id: recovery_id.clone(),
        run_id: identity.run_id.clone(),
        genesis_hash: identity.genesis_hash(),
        nonce,
        // The concrete value is immaterial to the size bound; its decimal
        // width is no greater than u64::MAX above.
        good_offset: pre_tail_offset,
        discarded_hash,
        discarded_offset: None,
        discarded_len: None,
        pre_size: None,
        bundle_digest: None,
        bundle_pre_offset: None,
        bundle_recovery_kind: None,
        pre_tail_hash: pre_tail_hash.clone(),
        actor: "reviewgraphen-store".to_owned(),
        tool_version: "append-rollback".to_owned(),
        timestamp_unix_seconds: u64::MAX,
    };
    let completion = RecoveryCompletion {
        recovery_id,
        post_file_hash: ContentHash::sha256(b""),
    };
    Ok([receipt_bytes(&intent)?, receipt_bytes(&completion)?])
}

fn validate_completed_receipts(
    file: &mut File,
    identity: &JournalIdentity,
    limits: JournalLimits,
    completed: &[(RecoveryIntent, RecoveryCompletion)],
) -> Result<(), JournalError> {
    if completed.is_empty() {
        return Ok(());
    }
    let len = file.metadata()?.len();
    limit(len, limits.max_replay_bytes)?;
    let bytes = read_prefix(file, len)?;
    // Exactly one bounded log pass validates the chain.  Checkpoints below
    // reuse that byte buffer and never reopen/re-read a prefix per receipt.
    let full = scan_bytes(&bytes, identity, limits, false)?;
    let wanted = completed
        .iter()
        .map(|(intent, _)| intent.good_offset)
        .collect::<std::collections::BTreeSet<_>>();
    let mut checkpoints = std::collections::BTreeMap::new();
    let mut digest = Sha256::new();
    if wanted.contains(&0) {
        checkpoints.insert(0, (chain_genesis(identity), ContentHash::sha256(b"")));
    }
    let mut offset = 0usize;
    let mut event_index = 0usize;
    while let Some(relative) = bytes[offset..].iter().position(|byte| *byte == b'\n') {
        let end = offset + relative + 1;
        digest.update(&bytes[offset..end]);
        let boundary = u64::try_from(end).map_err(|_| JournalError::Incomplete {
            limit: limits.max_replay_bytes,
            observed: u64::MAX,
        })?;
        if wanted.contains(&boundary) {
            let tail = full
                .events
                .get(event_index)
                .ok_or_else(|| JournalError::ReceiptCorruption {
                    name: "receipt boundary".to_owned(),
                })?
                .event_hash()
                .clone();
            let hash = ContentHash::parse(format!("sha256:{:x}", digest.clone().finalize()))?;
            checkpoints.insert(boundary, (tail, hash));
        }
        event_index += 1;
        offset = end;
    }
    for (intent, completion) in completed {
        let Some((tail, hash)) = checkpoints.get(&intent.good_offset) else {
            return Err(JournalError::ReceiptCorruption {
                name: receipt_name(&intent.recovery_id),
            });
        };
        if tail != &intent.pre_tail_hash || hash != &completion.post_file_hash {
            return Err(JournalError::ReceiptCorruption {
                name: receipt_name(&intent.recovery_id),
            });
        }
    }
    Ok(())
}

fn receipt_names(
    dir: &OwnedFd,
    limits: JournalLimits,
    count: &mut u64,
    scanned: &mut u64,
) -> Result<Vec<String>, JournalError> {
    let mut entries = Dir::new(dup(dir).map_err(StoreError::Io)?).map_err(StoreError::Io)?;
    let mut names = Vec::new();
    for entry in &mut entries {
        let entry = entry.map_err(StoreError::Io)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != "." && name != ".." {
            if !valid_receipt_name(&name) {
                return Err(JournalError::ReceiptCorruption { name });
            }
            *count = count.checked_add(1).ok_or(JournalError::Incomplete {
                limit: limits.max_receipt_files,
                observed: u64::MAX,
            })?;
            limit(*count, limits.max_receipt_files)?;
            let stat = fs::statat(dir, &name, AtFlags::SYMLINK_NOFOLLOW).map_err(StoreError::Io)?;
            if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile || stat.st_size < 0 {
                return Err(JournalError::ReceiptCorruption { name });
            }
            let size = u64::try_from(stat.st_size)
                .map_err(|_| JournalError::ReceiptCorruption { name: name.clone() })?;
            if size > limits.max_receipt_bytes {
                return Err(JournalError::ReceiptCorruption { name });
            }
            *scanned = scanned.checked_add(size).ok_or(JournalError::Incomplete {
                limit: limits.max_receipt_scan_bytes,
                observed: u64::MAX,
            })?;
            limit(*scanned, limits.max_receipt_scan_bytes)?;
            names.push(name);
        }
    }
    names.sort_unstable();
    Ok(names)
}

fn index_receipt_inventory(
    dir: &OwnedFd,
    limits: JournalLimits,
    count: &mut u64,
    scanned: &mut u64,
) -> Result<(usize, u64), JournalError> {
    let mut entries = Dir::new(dup(dir).map_err(StoreError::Io)?).map_err(StoreError::Io)?;
    let mut local_count = 0_usize;
    let mut maximum = 0_u64;
    for entry in &mut entries {
        let entry = entry.map_err(StoreError::Io)?;
        let name = entry.file_name().to_string_lossy();
        if name == "." || name == ".." {
            continue;
        }
        if !valid_receipt_name(&name) {
            return Err(JournalError::ReceiptCorruption {
                name: name.into_owned(),
            });
        }
        *count = count.checked_add(1).ok_or(JournalError::Incomplete {
            limit: limits.max_receipt_files,
            observed: u64::MAX,
        })?;
        limit(*count, limits.max_receipt_files)?;
        local_count = local_count.checked_add(1).ok_or(JournalError::Incomplete {
            limit: limits.max_receipt_files,
            observed: u64::MAX,
        })?;
        let stat =
            fs::statat(dir, name.as_ref(), AtFlags::SYMLINK_NOFOLLOW).map_err(StoreError::Io)?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile || stat.st_size < 0 {
            return Err(JournalError::ReceiptCorruption {
                name: name.into_owned(),
            });
        }
        let size = u64::try_from(stat.st_size).map_err(|_| JournalError::ReceiptCorruption {
            name: name.into_owned(),
        })?;
        limit(size, limits.max_receipt_bytes)?;
        *scanned = scanned.checked_add(size).ok_or(JournalError::Incomplete {
            limit: limits.max_receipt_scan_bytes,
            observed: u64::MAX,
        })?;
        limit(*scanned, limits.max_receipt_scan_bytes)?;
        maximum = maximum.max(size);
    }
    Ok((local_count, maximum))
}

fn receipt_names_exact(
    dir: &OwnedFd,
    limits: JournalLimits,
    expected: usize,
) -> Result<Vec<String>, JournalError> {
    let mut entries = Dir::new(dup(dir).map_err(StoreError::Io)?).map_err(StoreError::Io)?;
    let mut names = Vec::new();
    names
        .try_reserve_exact(expected)
        .map_err(|_| JournalError::Incomplete {
            limit: limits.max_receipt_files,
            observed: u64::MAX,
        })?;
    for entry in &mut entries {
        let entry = entry.map_err(StoreError::Io)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != "." && name != ".." {
            if !valid_receipt_name(&name) || names.len() == expected {
                return Err(JournalError::ReceiptCorruption { name });
            }
            names.push(name);
        }
    }
    if names.len() != expected {
        return Err(JournalError::ReceiptCorruption {
            name: "recovery directory changed during index admission".to_owned(),
        });
    }
    names.sort_unstable();
    Ok(names)
}

fn valid_receipt_name(name: &str) -> bool {
    name.len() == 69
        && name.ends_with(".json")
        && name[..64]
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn validate_intent_shape(
    intent: &RecoveryIntent,
    name: &str,
    identity: &JournalIdentity,
    limits: JournalLimits,
) -> Result<(), JournalError> {
    let bundle_range_valid = match (
        intent.discarded_offset,
        intent.discarded_len,
        intent.pre_size,
    ) {
        (None, None, None) => true,
        (Some(offset), Some(len), Some(pre_size)) => {
            offset == intent.good_offset
                && len > 0
                && offset.checked_add(len) == Some(pre_size)
                && pre_size <= limits.max_replay_bytes
        }
        _ => false,
    };
    let bundle_identity_valid = match (
        &intent.bundle_digest,
        intent.bundle_pre_offset,
        intent.bundle_recovery_kind.as_deref(),
    ) {
        (None, None, None) => true,
        (Some(digest), Some(pre_offset), Some(kind)) => {
            let discarded_len = intent.discarded_len.unwrap_or(0);
            let expected_nonce = bundle_recovery_nonce(
                identity,
                digest,
                pre_offset,
                intent.good_offset,
                &intent.discarded_hash,
                discarded_len,
                kind,
            )?;
            let range_matches_kind = if kind == "torn" {
                intent.discarded_offset == Some(intent.good_offset)
                    && intent.discarded_len.is_some_and(|len| len > 0)
                    && intent.pre_size
                        == intent
                            .discarded_len
                            .and_then(|len| intent.good_offset.checked_add(len))
            } else {
                intent.discarded_offset.is_none()
                    && intent.discarded_len.is_none()
                    && intent.pre_size.is_none()
                    && intent.discarded_hash == ContentHash::sha256(b"")
            };
            let stage_only_matches = kind != "stage-only"
                || (digest == &ContentHash::sha256(b"reviewgraphen.bundle-stage-only.v1")
                    && pre_offset == intent.good_offset);
            let provenance_matches = intent.actor == "reviewgraphen-store"
                && intent.tool_version == format!("bundle-recovery:{kind}")
                && intent.timestamp_unix_seconds == 0;
            pre_offset <= intent.good_offset
                && matches!(
                    kind,
                    "torn" | "stage-only" | "confirmed-zero-cleanup" | "already-complete-cleanup"
                )
                && range_matches_kind
                && stage_only_matches
                && provenance_matches
                && intent.nonce == expected_nonce
        }
        _ => false,
    };
    if intent.recovery_id.kind() != "recovery"
        || intent.run_id != identity.run_id
        || intent.genesis_hash != identity.genesis_hash()
        || (identity.version() != EventContractVersion::V1 && intent.good_offset == 0)
        || intent.nonce.len() != 64
        || !intent.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        || intent.actor.is_empty()
        || intent.actor.len() > 1024
        || intent.tool_version.is_empty()
        || intent.tool_version.len() > 1024
        || intent.good_offset > limits.max_replay_bytes
        || !bundle_range_valid
        || !bundle_identity_valid
        || receipt_name(&intent.recovery_id) != name
        || recovery_id(
            &intent.run_id,
            &intent.genesis_hash,
            intent.good_offset,
            &intent.discarded_hash,
            &intent.nonce,
        )? != intent.recovery_id
    {
        return Err(JournalError::ReceiptCorruption {
            name: name.to_owned(),
        });
    }
    Ok(())
}

/// Publish a receipt through a no-replace hard link.  If an entry already
/// exists, it must decode byte-for-byte as the intended canonical record.
fn publish_receipt_with_limits<T: Serialize + for<'de> Deserialize<'de> + Eq>(
    dir: &OwnedFd,
    name: &str,
    receipt: &T,
    limits: JournalLimits,
) -> Result<(), JournalError> {
    let expected = canonical_json(receipt)?;
    limit(expected.len() as u64, limits.max_receipt_bytes)?;
    match fs::openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => {
            verify_fd_kind_mode(&fd, "recovery receipt", FileType::RegularFile, 0o600)?;
            let mut actual = Vec::new();
            File::from(fd)
                .take(
                    u64::try_from(expected.len())
                        .unwrap_or(u64::MAX)
                        .saturating_add(1),
                )
                .read_to_end(&mut actual)?;
            let parsed: T =
                serde_json::from_slice(&actual).map_err(|_| JournalError::ReceiptCorruption {
                    name: name.to_owned(),
                })?;
            if actual != expected || canonical_json(&parsed)? != expected {
                return Err(JournalError::ReceiptCorruption {
                    name: name.to_owned(),
                });
            }
            fs::fsync(dir).map_err(StoreError::Io)?;
            return Ok(());
        }
        Err(rustix::io::Errno::NOENT) => {}
        Err(error) => return Err(JournalError::Store(StoreError::Io(error))),
    }
    // O_TMPFILE gives a private inode until the create-only link; no receipt
    // is observable before its contents and file metadata are durable.
    let fd = fs::openat(
        dir,
        ".",
        OFlags::TMPFILE | OFlags::RDWR | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )
    .map_err(|_| JournalError::Store(StoreError::UnsupportedPlatform))?;
    verify_fd_kind_mode(
        &fd,
        "recovery receipt temporary",
        FileType::RegularFile,
        0o600,
    )?;
    let mut file = File::from(fd);
    file.write_all(&expected)?;
    file.sync_all()?;
    match fs::linkat(&file, "", dir, name, AtFlags::EMPTY_PATH) {
        Ok(()) => {
            fs::fsync(dir).map_err(StoreError::Io)?;
            Ok(())
        }
        Err(rustix::io::Errno::EXIST) => {
            drop(file);
            publish_receipt_with_limits(dir, name, receipt, limits)?;
            fs::fsync(dir)
                .map_err(StoreError::Io)
                .map_err(JournalError::from)
        }
        Err(error) => Err(JournalError::Store(StoreError::Io(error))),
    }
}

// Test fixtures and callers without an explicit configured bound retain the
// store default; protocol paths always call the limited form.
#[cfg(test)]
fn publish_receipt<T: Serialize + for<'de> Deserialize<'de> + Eq>(
    dir: &OwnedFd,
    name: &str,
    receipt: &T,
) -> Result<(), JournalError> {
    publish_receipt_with_limits(
        dir,
        name,
        receipt,
        JournalLimits::from_store(super::StoreLimits::default()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use reviewgraphen_core::{
        ArtifactRegistered, ArtifactRegisteredV3, ArtifactSensitivity, ArtifactSource,
        ArtifactSourceV3, ArtifactSourceV4, AssessmentDispositionV3, AssessmentReviewStatusV3,
        AssignmentValueV4, AuthorityTrustRootsV3, ClaimPolarity, DecisionInputV3,
        DecisionOutcomeV3, EventCommand, EventLog, EventLogV4, ExecutionClaimInputV2,
        ExecutionOutcome, ExecutionRecordInput, FAKE_REVIEWER_ID, FIXTURE_DESCRIPTOR_ID,
        FIXTURE_HARNESS_ID, FIXTURE_HARNESS_REVISION, FIXTURE_HARNESS_SOURCE_HASH,
        FIXTURE_MEDIA_TYPE, FIXTURE_PROCEDURE_ID, FIXTURE_TEST_ARTIFACT_ID, FIXTURE_WITNESS_HASH,
        GluingInputTrustBindingV4, HarnessTrustRootInputV3, HumanAuthorityCapabilityV3,
        HumanTrustGrantInputV3, M4_PROPERTY_ID, MvpRulePack, ObligationLifecycle, PlanBudget,
        ProgramSpace, ReviewAggregate, RunGenesisBootstrapRequestV4, SnapshotSourceRecordEntry,
        SnapshotSourcesRecorded, ValidatedExecutionBundle, VerificationAttemptStageV3,
        VerificationBundleRequestV4, evaluate_static_fact_v1, plan, prepare_context,
    };
    use reviewgraphen_ingest::{IngestRequest, ingest_with_sources};
    use serde_json::Value;
    use std::{
        collections::{BTreeMap, BTreeSet},
        io::Cursor,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::Command,
        sync::{Arc, Barrier, mpsc},
    };

    fn fixture_event_for(run: &str) -> (JournalIdentity, EventEnvelope) {
        let program = ProgramSpace::from_json_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let run_id = StableId::parse(run).unwrap();
        let mut log = EventLog::new(run_id.clone(), aggregate).unwrap();
        let genesis = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let obligation = log.aggregate().obligations().next().unwrap().id().clone();
        log.append(EventCommand::obligation_transition(
            obligation,
            ObligationLifecycle::Planned,
        ))
        .unwrap();
        let event = log.envelopes().nth(1).unwrap().clone();
        (
            JournalIdentity::new(run_id, JournalGenesis::V2(genesis)).unwrap(),
            event,
        )
    }
    fn fixture_event() -> (JournalIdentity, EventEnvelope) {
        fixture_event_for("run:journal-test")
    }

    fn v3_fixture(
        root: &StoreRoot,
    ) -> (
        JournalIdentity,
        EventEnvelope,
        AuthorityTrustRootsV3,
        Vec<u8>,
    ) {
        let program = ProgramSpace::from_json_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let repository_id = program.repository_id().clone();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let run_id = StableId::parse("run:journal-v3-test").unwrap();
        let log = EventLog::new_v3(run_id.clone(), aggregate).unwrap();
        let genesis = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let genesis_hash = ContentHash::sha256(&genesis);
        let cas_hash = CasHash::parse(genesis_hash.to_string()).unwrap();
        super::super::CasStore::open(root)
            .unwrap()
            .put(
                &cas_hash,
                Some(u64::try_from(genesis.len()).unwrap()),
                genesis.as_slice(),
            )
            .unwrap();
        let roots = AuthorityTrustRootsV3::new(
            ContentHash::sha256(b"journal-v3-policy"),
            repository_id,
            ContentHash::parse("sha256:1111111111111111").unwrap(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        (
            JournalIdentity::new(run_id, JournalGenesis::V3(genesis.clone())).unwrap(),
            log.envelopes().next().unwrap().clone(),
            roots,
            genesis,
        )
    }

    fn v4_bootstrap_log(run: &str) -> EventLogV4 {
        let program = ProgramSpace::from_json_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let repository_identity = program.repository_identity().to_owned();
        let snapshot_id = program.snapshot_id().clone();
        let profile_id = program.profile_id().to_owned();
        let profile_version = program.profile_version().to_owned();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let v3 = EventLog::new_v3(StableId::parse(run).unwrap(), aggregate).unwrap();
        let genesis = v3
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        EventLogV4::from_bootstrap_request(
            RunGenesisBootstrapRequestV4::new(
                StableId::parse(run).unwrap(),
                genesis,
                repository_identity,
                snapshot_id,
                profile_id,
                profile_version,
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn v4_marker_at_current_tail(
        journal: &EventJournal<'_>,
        planned: &[EventEnvelope],
    ) -> VerificationBundlePendingMarkerV3 {
        let reader = journal.reader().unwrap();
        VerificationBundlePendingMarkerV3::new(&journal.identity, &reader.state, planned).unwrap()
    }

    fn put_test_cas(root: &StoreRoot, bytes: &[u8]) {
        let hash = CasHash::parse(ContentHash::sha256(bytes).to_string()).unwrap();
        super::super::CasStore::open(root)
            .unwrap()
            .put(&hash, Some(bytes.len() as u64), bytes)
            .unwrap();
    }

    struct IngestPlannedPrefix {
        bootstrap: EventLogV4,
        target_bootstrap: RunGenesisBootstrapRequestV4,
        v3_envelopes: Vec<EventEnvelope>,
        registrations: Vec<ArtifactRegisteredV3>,
        sources: SnapshotSourcesRecorded,
        plan: reviewgraphen_core::ReviewPlan,
    }

    fn ingest_planned_prefix(
        root: &StoreRoot,
        run: &str,
        program: ProgramSpace,
        source_bundle: &SnapshotSourceBundle,
    ) -> IngestPlannedPrefix {
        assert_eq!(program.snapshot_id(), source_bundle.snapshot_id());
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let run_id = StableId::parse(run).unwrap();
        let snapshot_id = aggregate.program().snapshot_id().clone();
        let mut v3 = EventLog::new_v3(run_id.clone(), aggregate).unwrap();
        let genesis = v3
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        put_test_cas(root, &genesis);

        let mut registrations = Vec::new();
        let mut entries = Vec::new();
        for source in source_bundle.entries() {
            put_test_cas(root, source.bytes());
            let hash = source.content_hash().clone();
            let registration = ArtifactRegisteredV3::new(
                run_id.clone(),
                hash.clone(),
                "text/plain",
                u64::try_from(source.bytes().len()).unwrap(),
                ArtifactSensitivity::WorkspaceSource,
                ArtifactSourceV3::SnapshotIngest {
                    adapter_id: "reviewgraphen-ingest-e2e".to_owned(),
                    run_id: run_id.clone(),
                    snapshot_id: snapshot_id.clone(),
                },
            )
            .unwrap();
            entries.push(
                SnapshotSourceRecordEntry::new(
                    source.artifact_id().clone(),
                    source.path(),
                    hash.clone(),
                    registration.registration_id().clone(),
                    hash,
                    u64::try_from(
                        source
                            .bytes()
                            .iter()
                            .filter(|byte| **byte == b'\n')
                            .count()
                            .saturating_add(usize::from(!source.bytes().ends_with(b"\n"))),
                    )
                    .unwrap()
                    .max(1),
                )
                .unwrap(),
            );
            v3.append(EventCommand::artifact_registered_v3(registration.clone()))
                .unwrap();
            registrations.push(registration);
        }
        registrations.sort_by(|left, right| left.registration_id().cmp(right.registration_id()));
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        let sources = SnapshotSourcesRecorded::new(snapshot_id, entries).unwrap();
        v3.append(EventCommand::snapshot_sources_recorded(sources.clone()))
            .unwrap();
        let plan = plan(v3.aggregate(), PlanBudget::new(16, 16).unwrap()).unwrap();
        v3.append(EventCommand::review_plan_recorded(plan.clone()))
            .unwrap();
        let source_bootstrap = RunGenesisBootstrapRequestV4::new(
            run_id.clone(),
            genesis.clone(),
            v3.aggregate().program().repository_identity(),
            v3.aggregate().program().snapshot_id().clone(),
            v3.aggregate().program().profile_id(),
            v3.aggregate().program().profile_version(),
        )
        .unwrap();
        let target_bootstrap = RunGenesisBootstrapRequestV4::new(
            run_id,
            genesis,
            v3.aggregate().program().repository_identity(),
            v3.aggregate().program().snapshot_id().clone(),
            v3.aggregate().program().profile_id(),
            v3.aggregate().program().profile_version(),
        )
        .unwrap();
        IngestPlannedPrefix {
            bootstrap: EventLogV4::from_bootstrap_request(source_bootstrap).unwrap(),
            target_bootstrap,
            v3_envelopes: v3.envelopes().cloned().collect(),
            registrations,
            sources,
            plan,
        }
    }

    fn publish_ingest_source_v4<'a>(
        root: &'a StoreRoot,
        run: &str,
        program: ProgramSpace,
        source_bundle: &SnapshotSourceBundle,
    ) -> (EventJournal<'a>, AuthorityTrustRootsV4) {
        let repository_id = program.repository_id().clone();
        let repository_source_hash = program
            .accepted_git_revision_closure()
            .unwrap()
            .target_tree_hash()
            .clone();
        let planned = ingest_planned_prefix(root, run, program, source_bundle);
        let prefix = rewrap_v3_fixture_prefix_as_v4(&planned.v3_envelopes, &planned.bootstrap);
        let (journal, _) = EventJournal::publish_new_v4(root, planned.bootstrap).unwrap();
        let mut writer = journal.writer_v4().unwrap();
        writer.append_batch(&prefix[1..]).unwrap();
        drop(writer);
        let roots = AuthorityTrustRootsV4::new(
            ContentHash::sha256(b"ingest-store-mapping-e2e-policy"),
            repository_id,
            repository_source_hash,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        (journal, roots)
    }

    fn planned_ingest_target_v5(
        root: &StoreRoot,
        run: &str,
        program: ProgramSpace,
        source_bundle: &SnapshotSourceBundle,
    ) -> EventLogV5 {
        let planned = ingest_planned_prefix(root, run, program, source_bundle);
        EventLogV5::from_planned_bootstrap_request(
            planned.target_bootstrap,
            planned.registrations,
            planned.sources,
            planned.plan,
        )
        .unwrap()
    }

    fn test_cas_inventory(root: &StoreRoot) -> Vec<(PathBuf, u64)> {
        let base = root.path().join("artifacts").join("sha256");
        let mut result = Vec::new();
        for prefix in std::fs::read_dir(base).unwrap() {
            let prefix = prefix.unwrap();
            if !prefix.file_type().unwrap().is_dir() {
                continue;
            }
            for object in std::fs::read_dir(prefix.path()).unwrap() {
                let object = object.unwrap();
                if object.file_type().unwrap().is_file() {
                    result.push((object.path(), object.metadata().unwrap().len()));
                }
            }
        }
        result.sort_unstable();
        result
    }

    fn clone_test_harness_root(root: &HarnessTrustRootInputV3) -> HarnessTrustRootInputV3 {
        HarnessTrustRootInputV3 {
            policy_revision_hash: root.policy_revision_hash.clone(),
            repository_id: root.repository_id.clone(),
            repository_source_hash: root.repository_source_hash.clone(),
            harness_id: root.harness_id.clone(),
            harness_revision: root.harness_revision.clone(),
            harness_source_hash: root.harness_source_hash.clone(),
            test_artifact_id: root.test_artifact_id.clone(),
            descriptor_id: root.descriptor_id.clone(),
            procedure_version: root.procedure_version.clone(),
            result_hash: root.result_hash.clone(),
            result_size: root.result_size,
            result_media_type: root.result_media_type.clone(),
            result_sensitivity: root.result_sensitivity,
            run_id: root.run_id.clone(),
            genesis_hash: root.genesis_hash.clone(),
            snapshot_id: root.snapshot_id.clone(),
            universe_id: root.universe_id.clone(),
            property_id: root.property_id.clone(),
            claim_id: root.claim_id.clone(),
            claim_body_hash: root.claim_body_hash.clone(),
        }
    }

    fn public_v3_fixture_journal<'a>(
        root: &'a StoreRoot,
        run: &str,
    ) -> (
        EventJournal<'a>,
        AuthorityTrustRootsV3,
        StableId,
        HarnessTrustRootInputV3,
        EventLogV4,
        Vec<EventEnvelope>,
        StaticFactEvaluationV1,
        BuiltContextProjection,
        ArtifactRegisteredV3,
        ValidatedExecutionBundle,
    ) {
        public_v3_fixture_journal_for_profile(root, run, false)
    }

    fn public_v3_fixture_journal_for_profile<'a>(
        root: &'a StoreRoot,
        run: &str,
        exact_m5_profile: bool,
    ) -> (
        EventJournal<'a>,
        AuthorityTrustRootsV3,
        StableId,
        HarnessTrustRootInputV3,
        EventLogV4,
        Vec<EventEnvelope>,
        StaticFactEvaluationV1,
        BuiltContextProjection,
        ArtifactRegisteredV3,
        ValidatedExecutionBundle,
    ) {
        public_v3_fixture_journal_for_profile_and_program_v3(root, run, exact_m5_profile, false)
    }

    fn public_v3_fixture_journal_for_profile_and_program_v3<'a>(
        root: &'a StoreRoot,
        run: &str,
        exact_m5_profile: bool,
        incremental_program_v3: bool,
    ) -> (
        EventJournal<'a>,
        AuthorityTrustRootsV3,
        StableId,
        HarnessTrustRootInputV3,
        EventLogV4,
        Vec<EventEnvelope>,
        StaticFactEvaluationV1,
        BuiltContextProjection,
        ArtifactRegisteredV3,
        ValidatedExecutionBundle,
    ) {
        let mut input: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        if exact_m5_profile {
            input["profile"]["id"] = Value::String("double-submit-payment".to_owned());
            input["profile"]["version"] = Value::String("1".to_owned());
            for context in input["contexts"].as_array_mut().unwrap() {
                if context["id"] == reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID {
                    let members = context["member_ids"].as_array_mut().unwrap();
                    members.extend([
                        Value::String("file:checkout-controller".to_owned()),
                        Value::String("file:payment-repository".to_owned()),
                    ]);
                    members
                        .sort_by(|left, right| left.as_str().unwrap().cmp(right.as_str().unwrap()));
                }
            }
            input["evidence"] = serde_json::json!([{
                "id": "evidence:seeded-payment-source",
                "kind": "static_analysis",
                "target_ids": ["function:payment-charge"],
                "artifact_ref": null,
                "content_hash": null,
                "attributes": {"seeded": true},
                "provenance": input["artifacts"][0]["provenance"].clone(),
            }]);
        }
        let mut contains = input["relations"][0].clone();
        contains["id"] = Value::String("relation:file-contains-payment-charge".to_owned());
        contains["kind"] = Value::String("contains".to_owned());
        contains["source_id"] = Value::String("file:payment-repository".to_owned());
        contains["target_ids"] = serde_json::json!(["function:payment-charge"]);
        contains["directed"] = Value::Bool(true);
        input["relations"].as_array_mut().unwrap().push(contains);
        if incremental_program_v3 {
            input["schema"] = Value::String("reviewgraphen.program_space.input.v3".to_owned());
            input["source"]["kind"] = Value::String("git".to_owned());
            input["source"]["revision"] =
                Value::String("1111111111111111111111111111111111111111".to_owned());
            input["source"]["content_hash"] =
                Value::String("git:3333333333333333333333333333333333333333".to_owned());
            input["snapshot"]["base_revision"] =
                Value::String("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned());
            input["snapshot"]["target_revision"] =
                Value::String("1111111111111111111111111111111111111111".to_owned());
            input["snapshot"]["tree_hash"] =
                Value::String("git:3333333333333333333333333333333333333333".to_owned());
            input["snapshot"]["dirty"] = Value::Bool(false);
            for relation in input["relations"].as_array_mut().unwrap() {
                relation["ordered_target_ids"] = relation["target_ids"].clone();
            }
            let mut anchors = serde_json::Map::new();
            for artifact in input["artifacts"].as_array_mut().unwrap() {
                let kind = artifact["kind"].as_str().unwrap().to_owned();
                if artifact["language"] == "rust"
                    && matches!(kind.as_str(), "function" | "method" | "type")
                {
                    let id = artifact["id"].as_str().unwrap().to_owned();
                    artifact["provenance"]["extraction_method"] =
                        Value::String("reviewgraphen.ingest.rust_syn.v1".to_owned());
                    anchors.insert(
                        id.clone(),
                        serde_json::json!({
                            "descriptor": "reviewgraphen.rust_symbol_anchor@1",
                            "language": "rust",
                            "symbol_kind": kind,
                            "signature_shape_hash": ContentHash::sha256(
                                format!("signature:{id}").as_bytes()
                            ),
                            "normalized_body_hash": ContentHash::sha256(
                                format!("body:{id}").as_bytes()
                            )
                        }),
                    );
                }
            }
            input["incremental_facts"] = serde_json::json!({
                "git_revision_closure": {
                    "base_commit_oid": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "base_tree_hash": "git:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "target_commit_oid": "1111111111111111111111111111111111111111",
                    "target_tree_hash": "git:3333333333333333333333333333333333333333"
                },
                "rust_anchor_extractor_id": "reviewgraphen.ingest.rust_syn.anchor.v1",
                "rust_anchor_syn_version": "2.0.119",
                "rust_symbol_anchors": anchors
            });
        }
        let test = input["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["id"] == reviewgraphen_core::FIXTURE_TEST_ARTIFACT_ID)
            .unwrap();
        test["location"]["start_line"] = Value::Null;
        test["location"]["end_line"] = Value::Null;
        let invariant = input["invariants"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|invariant| invariant["property_id"] == M4_PROPERTY_ID)
            .unwrap();
        invariant["scope_ids"] = serde_json::json!(["context:payment", "context:ui-event"]);
        let bytes_by_path = BTreeMap::from([
            ("src/checkout_controller.rs", b"checkout\n".repeat(40)),
            ("src/payment_repository.rs", b"repository\n".repeat(40)),
        ]);
        for artifact in input["artifacts"].as_array_mut().unwrap() {
            if artifact["kind"] == "file" {
                let path = artifact["location"]["path"].as_str().unwrap();
                artifact["content_hash"] =
                    Value::String(ContentHash::sha256(&bytes_by_path[path]).to_string());
            }
        }
        let repository_source_hash =
            ContentHash::parse(input["source"]["content_hash"].as_str().unwrap().to_owned())
                .unwrap();
        let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
        let repository_id = program.repository_id().clone();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let run_id = StableId::parse(run).unwrap();
        let mut log = EventLog::new_v3(run_id.clone(), aggregate).unwrap();
        let snapshot_id = log.aggregate().program().snapshot_id().clone();
        let files = log
            .aggregate()
            .program()
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
            .cloned()
            .collect::<Vec<_>>();
        let mut entries = Vec::new();
        let mut source_by_id = BTreeMap::new();
        for artifact in files {
            let path = artifact.location.as_ref().unwrap().path.clone();
            let bytes = bytes_by_path[path.as_str()].clone();
            put_test_cas(root, &bytes);
            let hash = ContentHash::sha256(&bytes);
            let registration = ArtifactRegisteredV3::new(
                run_id.clone(),
                hash.clone(),
                "text/plain",
                bytes.len() as u64,
                ArtifactSensitivity::WorkspaceSource,
                ArtifactSourceV3::SnapshotIngest {
                    adapter_id: "store-v3-e2e-fixture".to_owned(),
                    run_id: run_id.clone(),
                    snapshot_id: snapshot_id.clone(),
                },
            )
            .unwrap();
            entries.push(
                SnapshotSourceRecordEntry::new(
                    artifact.id.clone(),
                    path,
                    hash.clone(),
                    registration.registration_id().clone(),
                    hash,
                    bytes.iter().filter(|byte| **byte == b'\n').count() as u64 + 1,
                )
                .unwrap(),
            );
            source_by_id.insert(artifact.id, bytes);
            log.append(EventCommand::artifact_registered_v3(registration))
                .unwrap();
        }
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        log.append(EventCommand::snapshot_sources_recorded(
            SnapshotSourcesRecorded::new(snapshot_id, entries).unwrap(),
        ))
        .unwrap();
        let review_plan = plan(log.aggregate(), PlanBudget::new(16, 16).unwrap()).unwrap();
        log.append(EventCommand::review_plan_recorded(review_plan.clone()))
            .unwrap();
        let (obligation_id, built) = review_plan
            .waves()
            .iter()
            .flat_map(|wave| wave.obligation_ids())
            .find_map(|candidate| {
                let obligation = log
                    .aggregate()
                    .obligations()
                    .find(|obligation| obligation.id() == candidate)?;
                if obligation.property_id() != M4_PROPERTY_ID {
                    return None;
                }
                let mut context = prepare_context(log.aggregate(), candidate.clone()).ok()?;
                while let Some(request) = context.next_source_request().ok()? {
                    context
                        .submit_source(&request, &source_by_id[request.artifact_id()])
                        .ok()?;
                }
                let built = context.finish().ok()?;
                (!built.envelope().normalized_included_source_ids().is_empty())
                    .then(|| (candidate.clone(), built))
            })
            .unwrap();
        let mut v4_context = prepare_context(log.aggregate(), obligation_id.clone()).unwrap();
        while let Some(request) = v4_context.next_source_request().unwrap() {
            v4_context
                .submit_source(&request, &source_by_id[request.artifact_id()])
                .unwrap();
        }
        let v4_built = v4_context.finish().unwrap();
        log.append(EventCommand::obligation_transition(
            obligation_id.clone(),
            ObligationLifecycle::Planned,
        ))
        .unwrap();
        log.append(EventCommand::obligation_transition(
            obligation_id.clone(),
            ObligationLifecycle::InProgress,
        ))
        .unwrap();
        let envelope = built.envelope().clone();
        log.append(EventCommand::context_envelope_projected(built))
            .unwrap();

        let wave = review_plan
            .waves()
            .iter()
            .find(|wave| wave.obligation_ids().contains(&obligation_id))
            .unwrap();
        let execution_input = ExecutionRecordInput::fake(
            review_plan.id().clone(),
            wave.id().clone(),
            obligation_id.clone(),
            envelope.id().clone(),
            envelope.snapshot_id().clone(),
            1,
        )
        .unwrap();
        let execution_id = execution_input.execution_id().unwrap();
        let raw = br#"{"attempt":1,"fixture":true,"version":3}"#.to_vec();
        put_test_cas(root, &raw);
        let raw_registration = ArtifactRegisteredV3::new(
            run_id.clone(),
            ContentHash::sha256(&raw),
            "application/json",
            raw.len() as u64,
            ArtifactSensitivity::Sensitive,
            ArtifactSourceV3::ReviewerExecution {
                execution_id,
                reviewer_id: FAKE_REVIEWER_ID.to_owned(),
                run_id: run_id.clone(),
            },
        )
        .unwrap();
        log.append(EventCommand::artifact_registered_v3(
            raw_registration.clone(),
        ))
        .unwrap();
        let obligation = log
            .aggregate()
            .obligations()
            .find(|obligation| obligation.id() == &obligation_id)
            .unwrap();
        let claim = ExecutionClaimInputV2::new(
            obligation.property_id(),
            obligation.normalized_target_refs().clone(),
            ClaimPolarity::IssuePresent,
            "store public V3 fixture demonstrates a duplicate submit",
            envelope.normalized_included_source_ids().clone(),
            BTreeSet::new(),
            BTreeSet::new(),
            Some(1.0),
        )
        .unwrap();
        let source_buffers = source_by_id.values().collect::<Vec<_>>();
        let v4_execution = ValidatedExecutionBundle::fake_v3(
            execution_input.clone(),
            &raw_registration,
            raw.clone(),
            source_buffers.clone(),
            vec![claim.clone()],
            ExecutionOutcome::Structured,
        )
        .unwrap();
        let execution = ValidatedExecutionBundle::fake_v3(
            execution_input,
            &raw_registration,
            raw,
            source_buffers,
            vec![claim],
            ExecutionOutcome::Structured,
        )
        .unwrap();
        let claim_id = execution.claims()[0].id().clone();
        log.append(EventCommand::review_execution_recorded(execution))
            .unwrap();

        let policy = ContentHash::sha256(b"store-public-v3-fixture-policy");
        let claim_record = log
            .aggregate()
            .execution_claims()
            .find(|claim| claim.id() == &claim_id)
            .unwrap();
        let evaluation = evaluate_static_fact_v1(
            log.aggregate().program(),
            log.aggregate()
                .obligations()
                .find(|obligation| obligation.id() == &obligation_id)
                .unwrap(),
            claim_record,
        )
        .unwrap();
        let harness_root = HarnessTrustRootInputV3 {
            policy_revision_hash: policy.clone(),
            repository_id: repository_id.clone(),
            repository_source_hash: repository_source_hash.clone(),
            harness_id: FIXTURE_HARNESS_ID.to_owned(),
            harness_revision: FIXTURE_HARNESS_REVISION.to_owned(),
            harness_source_hash: ContentHash::parse(FIXTURE_HARNESS_SOURCE_HASH).unwrap(),
            test_artifact_id: StableId::parse(FIXTURE_TEST_ARTIFACT_ID).unwrap(),
            descriptor_id: FIXTURE_DESCRIPTOR_ID.to_owned(),
            procedure_version: FIXTURE_PROCEDURE_ID.to_owned(),
            result_hash: ContentHash::parse(FIXTURE_WITNESS_HASH).unwrap(),
            result_size: 145,
            result_media_type: FIXTURE_MEDIA_TYPE.to_owned(),
            result_sensitivity: ArtifactSensitivity::CanonicalState,
            run_id: log.run_id().clone(),
            genesis_hash: log.genesis_hash().clone(),
            snapshot_id: log.aggregate().program().snapshot_id().clone(),
            universe_id: log.aggregate().universe().id().clone(),
            property_id: M4_PROPERTY_ID.to_owned(),
            claim_id: claim_id.clone(),
            claim_body_hash: claim_record.body_hash().unwrap(),
        };
        let roots = AuthorityTrustRootsV3::new(
            policy.clone(),
            repository_id.clone(),
            repository_source_hash.clone(),
            vec![clone_test_harness_root(&harness_root)],
            vec![HumanTrustGrantInputV3 {
                policy_revision_hash: policy,
                actor: "human:store-reviewer".to_owned(),
                authority_id: "store-review-board".to_owned(),
                capabilities: BTreeSet::from([HumanAuthorityCapabilityV3::AcceptFinding]),
                run_id: run_id.clone(),
                snapshot_id: log.aggregate().program().snapshot_id().clone(),
                universe_id: log.aggregate().universe().id().clone(),
                property_ids: BTreeSet::from([M4_PROPERTY_ID.to_owned()]),
                claim_ids: BTreeSet::from([claim_id.clone()]),
                valid_from: "2026-01-01T00:00:00Z".to_owned(),
                valid_until: "2027-01-01T00:00:00Z".to_owned(),
            }],
        )
        .unwrap();
        let genesis = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let v4_bootstrap = EventLogV4::from_bootstrap_request(
            RunGenesisBootstrapRequestV4::new(
                run_id.clone(),
                genesis.clone(),
                log.aggregate().program().repository_identity(),
                log.aggregate().program().snapshot_id().clone(),
                log.aggregate().program().profile_id(),
                log.aggregate().program().profile_version(),
            )
            .unwrap(),
        )
        .unwrap();
        let v3_envelopes = log.envelopes().cloned().collect::<Vec<_>>();
        put_test_cas(root, &genesis);
        let identity = JournalIdentity::new(run_id, JournalGenesis::V3(genesis)).unwrap();
        let manifest = log.events()[0].envelope().clone();
        let journal = EventJournal::initialize_v3(root, identity, manifest).unwrap();
        let prefix = log.events()[1..]
            .iter()
            .map(|event| event.envelope().clone())
            .collect::<Vec<_>>();
        let mut writer = journal.writer_v3().unwrap();
        writer.append_batch(&prefix).unwrap();
        drop(writer);
        (
            journal,
            roots,
            claim_id,
            harness_root,
            v4_bootstrap,
            v3_envelopes,
            evaluation,
            v4_built,
            raw_registration,
            v4_execution,
        )
    }

    fn rewrap_v3_fixture_prefix_as_v4(
        v3: &[EventEnvelope],
        bootstrap: &EventLogV4,
    ) -> Vec<EventEnvelope> {
        let mut result = vec![bootstrap.envelopes()[0].clone()];
        for legacy in v3.iter().skip(1) {
            let mut value: Value =
                serde_json::from_slice(&legacy.canonical_bytes().unwrap()).unwrap();
            let sequence = result.last().unwrap().sequence() + 1;
            let actor = value["actor"].as_str().unwrap().to_owned();
            let payload_hash =
                ContentHash::parse(value["payload_hash"].as_str().unwrap().to_owned()).unwrap();
            let previous_event_hash = result.last().unwrap().event_hash().clone();
            let id_bindings = BTreeMap::from([
                ("actor".to_owned(), Value::String(actor.clone())),
                (
                    "genesis_hash".to_owned(),
                    Value::String(bootstrap.genesis_hash().to_string()),
                ),
                (
                    "logical_time".to_owned(),
                    Value::Number(serde_json::Number::from(sequence)),
                ),
                (
                    "payload_hash".to_owned(),
                    Value::String(payload_hash.to_string()),
                ),
                (
                    "previous_event_hash".to_owned(),
                    Value::String(previous_event_hash.to_string()),
                ),
                (
                    "run".to_owned(),
                    Value::String(bootstrap.run_id().to_string()),
                ),
                (
                    "schema".to_owned(),
                    Value::String(EVENT_CONTRACT_SCHEMA_V4.to_owned()),
                ),
                (
                    "sequence".to_owned(),
                    Value::Number(serde_json::Number::from(sequence)),
                ),
            ]);
            let event_id = StableId::derived("event", &id_bindings).unwrap();
            let hash_bindings = BTreeMap::from([
                ("actor".to_owned(), Value::String(actor)),
                ("event_id".to_owned(), Value::String(event_id.to_string())),
                (
                    "genesis_hash".to_owned(),
                    Value::String(bootstrap.genesis_hash().to_string()),
                ),
                (
                    "logical_time".to_owned(),
                    Value::Number(serde_json::Number::from(sequence)),
                ),
                (
                    "payload_hash".to_owned(),
                    Value::String(payload_hash.to_string()),
                ),
                (
                    "previous_event_hash".to_owned(),
                    Value::String(previous_event_hash.to_string()),
                ),
                (
                    "run".to_owned(),
                    Value::String(bootstrap.run_id().to_string()),
                ),
                (
                    "schema".to_owned(),
                    Value::String(EVENT_CONTRACT_SCHEMA_V4.to_owned()),
                ),
                (
                    "sequence".to_owned(),
                    Value::Number(serde_json::Number::from(sequence)),
                ),
            ]);
            let event_hash = ContentHash::sha256(&canonical_json(&hash_bindings).unwrap());
            value["schema"] = Value::String(EVENT_CONTRACT_SCHEMA_V4.to_owned());
            value["id"] = Value::String(event_id.to_string());
            value["run_id"] = Value::String(bootstrap.run_id().to_string());
            value["genesis_hash"] = Value::String(bootstrap.genesis_hash().to_string());
            value["sequence"] = Value::Number(serde_json::Number::from(sequence));
            value["logical_time"] = Value::Number(serde_json::Number::from(sequence));
            value["previous_event_hash"] = Value::String(previous_event_hash.to_string());
            value["event_hash"] = Value::String(event_hash.to_string());
            result.push(
                EventEnvelope::from_json_slice(&canonical_json(&value).unwrap()).unwrap_or_else(
                    |error| panic!("V3-to-V4 rewrap failed at {sequence}: {error}"),
                ),
            );
        }
        result
    }

    struct PublicV4GluingInput {
        descriptor: GluingInputDescriptorV4,
        sources: Vec<TrustedGluingInputSourceV4>,
    }

    fn public_v4_gluing_input(
        bootstrap: &EventLogV4,
        harness_root: &HarnessTrustRootInputV3,
        plan_id: &StableId,
        context_id: &str,
        assignment: AssignmentValueV4,
        qualification_source_ids: BTreeSet<StableId>,
    ) -> (
        GluingInputDescriptorV4,
        GluingInputTrustBindingV4,
        Vec<TrustedGluingInputSourceV4>,
    ) {
        let context_id = StableId::parse(context_id).unwrap();
        let descriptor = GluingInputDescriptorV4::new(
            bootstrap.run_id().clone(),
            harness_root.snapshot_id.clone(),
            harness_root.universe_id.clone(),
            plan_id.clone(),
            context_id.clone(),
            assignment,
            qualification_source_ids,
        )
        .unwrap();
        let bytes = canonical_json(&descriptor).unwrap();
        let hash = ContentHash::sha256(&bytes);
        let size = u64::try_from(bytes.len()).unwrap();
        let source = ArtifactSourceV4::GluingInput {
            context_id: context_id.clone(),
            descriptor_hash: hash.clone(),
            descriptor_id: descriptor.id().clone(),
            descriptor_media_type: GLUING_INPUT_MEDIA_TYPE_V4.to_owned(),
            descriptor_sensitivity: ArtifactSensitivity::CanonicalState,
            descriptor_size: size,
            genesis_hash: bootstrap.genesis_hash().clone(),
            plan_id: plan_id.clone(),
            policy_revision_hash: harness_root.policy_revision_hash.clone(),
            profile_descriptor_id: reviewgraphen_core::DOUBLE_SUBMIT_GLUING_DESCRIPTOR_ID
                .to_owned(),
            repository_id: harness_root.repository_id.clone(),
            repository_source_hash: harness_root.repository_source_hash.clone(),
            run_id: bootstrap.run_id().clone(),
            snapshot_id: harness_root.snapshot_id.clone(),
            universe_id: harness_root.universe_id.clone(),
        };
        let make_binding = || {
            GluingInputTrustBindingV4::new(
                harness_root.policy_revision_hash.clone(),
                harness_root.repository_id.clone(),
                harness_root.repository_source_hash.clone(),
                bootstrap.run_id().clone(),
                bootstrap.genesis_hash().clone(),
                harness_root.snapshot_id.clone(),
                harness_root.universe_id.clone(),
                plan_id.clone(),
                reviewgraphen_core::DOUBLE_SUBMIT_GLUING_DESCRIPTOR_ID,
                context_id.clone(),
                descriptor.id().clone(),
                hash.clone(),
                size,
                GLUING_INPUT_MEDIA_TYPE_V4,
                ArtifactSensitivity::CanonicalState,
                source.clone(),
            )
            .unwrap()
        };
        let root_binding = make_binding();
        let trusted = (0..3)
            .map(|_| TrustedGluingInputSourceV4::from_trusted_host(make_binding()).unwrap())
            .collect();
        (descriptor, root_binding, trusted)
    }

    fn public_v4_static_bundle_journal<'a>(
        root: &'a StoreRoot,
        run: &str,
    ) -> (EventJournal<'a>, AuthorityTrustRootsV4, Vec<EventEnvelope>) {
        let (journal, roots, planned, _, _) =
            public_v4_static_bundle_journal_with_append(root, run, false, false, false);
        (journal, roots, planned)
    }

    fn public_v4_gluing_journal<'a>(
        root: &'a StoreRoot,
        run: &str,
    ) -> (
        EventJournal<'a>,
        AuthorityTrustRootsV4,
        [PublicV4GluingInput; 2],
    ) {
        let (journal, roots, _, receipt, inputs) =
            public_v4_static_bundle_journal_with_append(root, run, true, true, false);
        assert!(receipt.is_some());
        (
            journal,
            roots,
            inputs.expect("exact M5 profile gluing inputs"),
        )
    }

    fn public_v4_gluing_journal_incremental_v3<'a>(
        root: &'a StoreRoot,
        run: &str,
    ) -> (
        EventJournal<'a>,
        AuthorityTrustRootsV4,
        [PublicV4GluingInput; 2],
    ) {
        let (journal, roots, _, receipt, inputs) =
            public_v4_static_bundle_journal_with_append(root, run, true, true, true);
        assert!(receipt.is_some());
        (
            journal,
            roots,
            inputs.expect("exact M5 profile gluing inputs"),
        )
    }

    fn public_v4_profile_base_roots() -> AuthorityTrustRootsV4 {
        AuthorityTrustRootsV4::new(
            ContentHash::sha256(b"store-public-v3-fixture-policy"),
            StableId::parse("repository:double-submit-payment").unwrap(),
            ContentHash::parse("sha256:1111111111111111").unwrap(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    fn incremental_v3_profile_base_roots() -> AuthorityTrustRootsV4 {
        AuthorityTrustRootsV4::new(
            ContentHash::sha256(b"store-public-v3-fixture-policy"),
            StableId::parse("repository:double-submit-payment").unwrap(),
            ContentHash::parse("git:3333333333333333333333333333333333333333").unwrap(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    fn public_v4_static_bundle_journal_with_append<'a>(
        root: &'a StoreRoot,
        run: &str,
        append_bundle: bool,
        exact_m5_profile: bool,
        incremental_program_v3: bool,
    ) -> (
        EventJournal<'a>,
        AuthorityTrustRootsV4,
        Vec<EventEnvelope>,
        Option<V4VerificationBundleAppendReceipt>,
        Option<[PublicV4GluingInput; 2]>,
    ) {
        assert!(!exact_m5_profile || append_bundle);
        let source_workspace = tempfile::tempdir().unwrap();
        let source_root =
            StoreRoot::open(source_workspace.path(), crate::StoreLimits::default()).unwrap();
        let (
            _source_journal,
            _v3_roots,
            claim_id,
            harness_root,
            bootstrap,
            v3_envelopes,
            evaluation,
            v4_built,
            raw_registration,
            v4_execution,
        ) = public_v3_fixture_journal_for_profile_and_program_v3(
            &source_root,
            run,
            exact_m5_profile,
            incremental_program_v3,
        );
        for (path, _) in test_cas_inventory(&source_root) {
            put_test_cas(root, &std::fs::read(path).unwrap());
        }
        let plan_id = v4_execution.execution().plan_id().clone();
        let roots = AuthorityTrustRootsV4::new(
            harness_root.policy_revision_hash.clone(),
            harness_root.repository_id.clone(),
            harness_root.repository_source_hash.clone(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let prefix = rewrap_v3_fixture_prefix_as_v4(&v3_envelopes[..7], &bootstrap);
        let (journal, _) = EventJournal::publish_new_v4(root, bootstrap).unwrap();
        let mut writer = journal.writer_v4().unwrap();
        writer.append_batch(&prefix[1..]).unwrap();
        drop(writer);

        let input_bytes = evaluation.input().canonical_bytes().unwrap();
        let output_bytes = evaluation.result().canonical_bytes().unwrap();
        put_test_cas(root, &input_bytes);
        put_test_cas(root, &output_bytes);
        let (mut session, mut basis) = journal.replayed_v4_session(&roots).unwrap();
        let context = session
            .prepare_inherited_d2_event(EventCommand::context_envelope_projected(v4_built), &basis)
            .unwrap();
        session
            .append_inherited_d2_event(context, &mut basis)
            .unwrap();

        let raw_admission = session
            .prepare_reviewer_raw_artifact_registration(raw_registration, &v4_execution, &basis)
            .unwrap();
        session
            .append_inherited_artifact_registration(raw_admission, &mut basis)
            .unwrap();

        let execution = session
            .prepare_inherited_d2_event(
                EventCommand::review_execution_recorded(v4_execution),
                &basis,
            )
            .unwrap();
        session
            .append_inherited_d2_event(execution, &mut basis)
            .unwrap();

        let input = session
            .prepare_static_verifier_input_registration(
                &claim_id,
                ContentHash::sha256(&input_bytes),
                input_bytes.len() as u64,
                &basis,
            )
            .unwrap();
        let input_receipt = session
            .append_inherited_artifact_registration(input, &mut basis)
            .unwrap();

        let output = session
            .prepare_static_verifier_output_registration(
                &claim_id,
                ContentHash::sha256(&output_bytes),
                output_bytes.len() as u64,
                &basis,
            )
            .unwrap();
        let output_receipt = session
            .append_inherited_artifact_registration(output, &mut basis)
            .unwrap();
        let bundle = session
            .mint_verification_bundle(
                VerificationBundleRequestV4::static_fact(
                    claim_id.clone(),
                    input_receipt.core().registration_id().clone(),
                    output_receipt.core().registration_id().clone(),
                ),
                None,
                &basis,
            )
            .unwrap();
        let planned = bundle
            .envelopes(&session.log, &basis, &session.session_identity)
            .unwrap()
            .to_vec();
        let receipt = append_bundle.then(|| {
            session
                .append_verification_bundle(bundle, &mut basis)
                .unwrap()
        });
        assert_eq!(planned.len(), 3);
        let gluing = exact_m5_profile.then(|| {
            let selected_evidence_id = session
                .log
                .claim_assessment_v3(&claim_id)
                .and_then(|assessment| assessment.evidence_ids().next().cloned())
                .expect("selected M5 payment assessment evidence");
            let (payment_descriptor, payment_binding, payment_sources) = public_v4_gluing_input(
                &session.log,
                &harness_root,
                &plan_id,
                reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
                AssignmentValueV4::Required,
                BTreeSet::from([
                    StableId::parse(reviewgraphen_core::DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID).unwrap(),
                    selected_evidence_id,
                ]),
            );
            let (ui_descriptor, ui_binding, ui_sources) = public_v4_gluing_input(
                &session.log,
                &harness_root,
                &plan_id,
                reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID,
                AssignmentValueV4::Satisfied,
                BTreeSet::new(),
            );
            (
                [
                    PublicV4GluingInput {
                        descriptor: payment_descriptor,
                        sources: payment_sources,
                    },
                    PublicV4GluingInput {
                        descriptor: ui_descriptor,
                        sources: ui_sources,
                    },
                ],
                vec![payment_binding, ui_binding],
            )
        });
        drop(session);
        let (inputs, bindings) = match gluing {
            Some((inputs, bindings)) => (Some(inputs), bindings),
            None => (None, Vec::new()),
        };
        let roots = AuthorityTrustRootsV4::new(
            harness_root.policy_revision_hash.clone(),
            harness_root.repository_id.clone(),
            harness_root.repository_source_hash.clone(),
            Vec::new(),
            Vec::new(),
            bindings,
        )
        .unwrap();
        (journal, roots, planned, receipt, inputs)
    }

    fn public_v4_rebuild_with_limits(
        limits: crate::StoreLimits,
    ) -> Result<crate::IndexRebuildReceiptV4, crate::IndexError> {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), limits).unwrap();
        let (journal, roots, _, _, _, _, _, _, _, _) =
            public_v3_fixture_journal(&root, "run:store-public-v4-limits");
        crate::DerivedIndexV4::open(&root)
            .unwrap()
            .rebuild_v4(&journal, &roots)
    }

    #[test]
    fn public_v4_rebuild_resource_limits_are_exact_and_one_less_refuses() {
        crate::index::reset_projection_decode_count_for_test();
        let baseline = public_v4_rebuild_with_limits(crate::StoreLimits::default()).unwrap();
        assert_eq!(
            crate::index::projection_decode_count_for_test(),
            baseline.event_count * 3
        );
        let obligation_probes = crate::index::projection_obligation_probe_count_for_test();
        assert!(obligation_probes > 0);
        assert!(obligation_probes <= baseline.event_count * 2);
        assert_eq!(
            baseline.accounting.sql_bytes,
            baseline.accounting.text_bytes
                + baseline.accounting.integer_cells * 8
                + baseline.accounting.rows
        );
        assert_ne!(
            baseline.accounting.owned_bytes,
            baseline.accounting.query_bytes * 8
        );

        let exact = crate::StoreLimits {
            max_index_rows: baseline.accounting.rows,
            max_index_query_bytes: baseline.accounting.query_bytes,
            max_index_working_bytes: baseline.accounting.working_bytes,
            max_index_serialized_bytes: baseline.serialized_bytes,
            ..crate::StoreLimits::default()
        };
        let exact_receipt = public_v4_rebuild_with_limits(exact).unwrap();
        assert_eq!(exact_receipt.accounting, baseline.accounting);
        assert_eq!(exact_receipt.serialized_bytes, baseline.serialized_bytes);

        for constrained in [
            crate::StoreLimits {
                max_index_rows: baseline.accounting.rows - 1,
                ..exact
            },
            crate::StoreLimits {
                max_index_query_bytes: baseline.accounting.query_bytes - 1,
                ..exact
            },
            crate::StoreLimits {
                max_index_working_bytes: baseline.accounting.working_bytes - 1,
                ..exact
            },
        ] {
            assert!(matches!(
                public_v4_rebuild_with_limits(constrained),
                Err(crate::IndexError::Incomplete { .. })
            ));
        }

        let overflow = crate::StoreLimits {
            max_index_rows: u64::MAX,
            max_index_query_bytes: u64::MAX,
            max_index_working_bytes: u64::MAX,
            ..crate::StoreLimits::default()
        };
        assert!(public_v4_rebuild_with_limits(overflow).is_err());
    }

    #[test]
    fn public_v4_snapshot_query_and_working_limits_are_exact_and_one_less_refuses() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), crate::StoreLimits::default()).unwrap();
        let (journal, roots, _, _, _, _, _, _, _, _) =
            public_v3_fixture_journal(&root, "run:store-public-v4-snapshot-limits");
        let identity = journal.identity.clone();
        let receipt = crate::DerivedIndexV4::open(&root)
            .unwrap()
            .rebuild_v4(&journal, &roots)
            .unwrap();
        drop(journal);
        drop(root);

        let exact = crate::StoreLimits {
            max_index_rows: receipt.accounting.rows,
            max_index_serialized_bytes: receipt.serialized_bytes,
            max_index_query_bytes: receipt.accounting.query_bytes,
            max_index_working_bytes: receipt.accounting.working_bytes,
            ..crate::StoreLimits::default()
        };

        let query = |limits: crate::StoreLimits| {
            let root = StoreRoot::open(workspace.path(), limits).unwrap();
            let journal = EventJournal::open(&root, identity.clone()).unwrap();
            crate::DerivedIndexV4::open(&root)
                .unwrap()
                .snapshot_current_v4(&journal, &roots)
        };
        let snapshot = query(exact).unwrap();
        assert_eq!(
            u64::try_from(canonical_json(&snapshot).unwrap().len()).unwrap(),
            receipt.accounting.query_bytes
        );

        for constrained in [
            crate::StoreLimits {
                max_index_rows: receipt.accounting.rows - 1,
                ..exact
            },
            crate::StoreLimits {
                max_index_query_bytes: receipt.accounting.query_bytes - 1,
                ..exact
            },
            crate::StoreLimits {
                max_index_working_bytes: receipt.accounting.working_bytes - 1,
                ..exact
            },
        ] {
            assert!(matches!(
                query(constrained),
                Err(crate::IndexError::Incomplete { .. })
            ));
        }
    }

    #[test]
    fn public_v4_json_staging_is_exact_and_refuses_hostile_canonical_cells_before_allocation() {
        struct CountingVisitor(u64);
        impl crate::V4SelectionVisitor for CountingVisitor {
            type Error = std::convert::Infallible;

            fn visit(&mut self, _item: crate::V4SelectionItem<'_>) -> Result<(), Self::Error> {
                self.0 += 1;
                Ok(())
            }
        }

        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), crate::StoreLimits::default()).unwrap();
        let (journal, roots, _, _, _, _, _, _, _, _) =
            public_v3_fixture_journal(&root, "run:store-public-v4-json-staging");
        let index = crate::DerivedIndexV4::open(&root).unwrap();
        index.rebuild_v4(&journal, &roots).unwrap();
        let snapshot = index.snapshot_current_v4(&journal, &roots).unwrap();
        let execution = snapshot.executions.first().unwrap();
        let selected: BTreeSet<StableId> =
            serde_json::from_str(&execution.obligation_ids_canonical_json).unwrap();
        let plan_id = execution.plan_id.clone();
        let expected_offset = snapshot.marker.confirmed_offset;
        let expected_events = snapshot.marker.event_count;
        let expected_tail = snapshot.marker.tail_hash.clone();
        let active_path = root.path().join("indexes/reviewgraphen.sqlite");
        let original_image = std::fs::read(&active_path).unwrap();

        // A published image and its deterministic rebuild have the same cell
        // staging peak. Candidate validation admits the exact value and rejects
        // exact-1 before either generic Value or typed source allocation.
        let connection = rusqlite::Connection::open(&active_path).unwrap();
        let baseline_staging =
            crate::index::v4_json_staging_for_connection_for_test(&connection, index.limits())
                .unwrap();
        drop(connection);
        assert!(baseline_staging > 0);
        crate::index::set_v4_json_staging_limit_for_test(Some(baseline_staging));
        crate::index::reset_v4_json_decode_allocation_count_for_test();
        index.rebuild_v4(&journal, &roots).unwrap();
        assert!(crate::index::v4_json_decode_allocation_count_for_test() > 0);
        crate::index::set_v4_json_staging_limit_for_test(Some(baseline_staging - 1));
        crate::index::reset_v4_json_decode_allocation_count_for_test();
        assert!(matches!(
            index.rebuild_v4(&journal, &roots),
            Err(crate::IndexError::Incomplete { limit, observed })
                if limit + 1 == observed && observed == baseline_staging
        ));
        assert_eq!(crate::index::v4_json_decode_allocation_count_for_test(), 0);

        let hostile_number = format!(
            "[{}]",
            std::iter::repeat_n("1e+20", 4_096)
                .collect::<Vec<_>>()
                .join(",")
        );
        let decoded_number: serde_json::Value = serde_json::from_str(&hostile_number).unwrap();
        let hostile_number = String::from_utf8(canonical_json(&decoded_number).unwrap()).unwrap();
        let hostile_escape =
            String::from_utf8(canonical_json(&vec!["\0\n\\\"é".repeat(4_096)]).unwrap()).unwrap();
        let hostile_source = String::from_utf8(
            canonical_json(&ArtifactSourceV3::RunGenesis {
                run_id: StableId::parse(format!("run:{}", "source".repeat(4_096))).unwrap(),
            })
            .unwrap(),
        )
        .unwrap();
        let hostile_cases = [
            (
                "UPDATE program_relations SET target_ids_canonical_json=?1",
                hostile_number,
            ),
            (
                "UPDATE program_relations SET target_ids_canonical_json=?1",
                hostile_escape,
            ),
            (
                "UPDATE artifact_registrations SET source_canonical_json=?1 WHERE source_kind='run_genesis'",
                hostile_source,
            ),
        ];

        for (case_index, (update, hostile)) in hostile_cases.into_iter().enumerate() {
            std::fs::write(&active_path, &original_image).unwrap();
            let connection = rusqlite::Connection::open(&active_path).unwrap();
            assert!(connection.execute(update, [&hostile]).unwrap() > 0);
            let exact_staging =
                crate::index::v4_json_staging_for_connection_for_test(&connection, index.limits())
                    .unwrap();
            drop(connection);
            assert!(exact_staging > baseline_staging);

            crate::index::set_v4_json_staging_limit_for_test(Some(exact_staging));
            crate::index::reset_v4_json_decode_allocation_count_for_test();
            assert!(matches!(
                index.snapshot_current_v4(&journal, &roots),
                Err(crate::IndexError::CorruptIndex)
            ));
            assert!(crate::index::v4_json_decode_allocation_count_for_test() > 0);

            crate::index::set_v4_json_staging_limit_for_test(Some(exact_staging - 1));
            crate::index::reset_v4_json_decode_allocation_count_for_test();
            assert!(matches!(
                index.snapshot_current_v4(&journal, &roots),
                Err(crate::IndexError::Incomplete { limit, observed })
                    if limit + 1 == observed && observed == exact_staging
            ));
            assert_eq!(crate::index::v4_json_decode_allocation_count_for_test(), 0);

            if case_index == 2 {
                let mut visitor = CountingVisitor(0);
                assert!(matches!(
                    index.visit_current_v4_selection(
                        &journal,
                        &roots,
                        crate::V4SelectionRequest {
                            plan_id: &plan_id,
                            selected_obligation_ids: &selected,
                            expected_confirmed_offset: expected_offset,
                            expected_event_count: expected_events,
                            expected_tail_hash: &expected_tail,
                        },
                        &mut visitor,
                    ),
                    Err(crate::V4SelectionVisitError::Index(
                        crate::IndexError::Incomplete { limit, observed }
                    )) if limit + 1 == observed && observed == exact_staging
                ));
                assert_eq!(visitor.0, 0);
            }
        }
        crate::index::set_v4_json_staging_limit_for_test(None);
        std::fs::write(&active_path, original_image).unwrap();
    }

    #[test]
    fn validated_v4_handle_reuses_one_snapshot_and_refuses_drift_tamper_and_wrong_journal() {
        struct CountingVisitor(u64);
        impl crate::V4SelectionVisitor for CountingVisitor {
            type Error = std::convert::Infallible;

            fn visit(&mut self, _item: crate::V4SelectionItem<'_>) -> Result<(), Self::Error> {
                self.0 += 1;
                Ok(())
            }
        }

        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), crate::StoreLimits::default()).unwrap();
        let (journal, roots, _, _, _, _, _, _, _, _) =
            public_v3_fixture_journal(&root, "run:store-validated-v4-handle");
        let index = crate::DerivedIndexV4::open(&root).unwrap();
        index.rebuild_v4(&journal, &roots).unwrap();
        crate::index::reset_v4_full_snapshot_construction_count_for_test();
        let handle = index
            .validated_snapshot_current_v4(&journal, &roots)
            .unwrap();
        assert_eq!(
            crate::index::v4_full_snapshot_construction_count_for_test(),
            1
        );
        let snapshot = handle.snapshot();
        let execution = snapshot.executions.first().unwrap();
        let selected: BTreeSet<StableId> =
            serde_json::from_str(&execution.obligation_ids_canonical_json).unwrap();
        let request = crate::V4SelectionRequest {
            plan_id: &execution.plan_id,
            selected_obligation_ids: &selected,
            expected_confirmed_offset: snapshot.marker.confirmed_offset,
            expected_event_count: snapshot.marker.event_count,
            expected_tail_hash: &snapshot.marker.tail_hash,
        };

        let mut first = CountingVisitor(0);
        let first_summary = handle
            .visit_selection(&journal, request, &mut first)
            .unwrap();
        let mut second = CountingVisitor(0);
        let second_summary = handle
            .visit_selection(&journal, request, &mut second)
            .unwrap();
        assert_eq!(first_summary, second_summary);
        assert_eq!(first.0, second.0);
        assert!(first.0 > 0);
        assert_eq!(
            crate::index::v4_full_snapshot_construction_count_for_test(),
            1
        );

        let wrong_tail = ContentHash::sha256(b"validated handle request drift");
        let mut drift = CountingVisitor(0);
        assert!(matches!(
            handle.visit_selection(
                &journal,
                crate::V4SelectionRequest {
                    expected_tail_hash: &wrong_tail,
                    ..request
                },
                &mut drift,
            ),
            Err(crate::V4SelectionVisitError::Index(
                crate::IndexError::ProjectionContractViolation
            ))
        ));
        assert_eq!(drift.0, 0);

        let active_path = root.path().join("indexes/reviewgraphen.sqlite");
        let original_image = std::fs::read(&active_path).unwrap();
        let connection = rusqlite::Connection::open(&active_path).unwrap();
        assert!(
            connection
                .execute("UPDATE program_relations SET relation_kind='tampered'", [],)
                .unwrap()
                > 0
        );
        drop(connection);
        let mut tamper = CountingVisitor(0);
        assert!(matches!(
            handle.visit_selection(&journal, request, &mut tamper),
            Err(crate::V4SelectionVisitError::Index(
                crate::IndexError::CorruptIndex
            ))
        ));
        assert_eq!(tamper.0, 0);
        std::fs::write(&active_path, original_image).unwrap();

        let (wrong_journal, _, _, _, _, _, _, _, _, _) =
            public_v3_fixture_journal(&root, "run:store-validated-v4-wrong-journal");
        let mut wrong = CountingVisitor(0);
        assert!(matches!(
            handle.visit_selection(&wrong_journal, request, &mut wrong),
            Err(crate::V4SelectionVisitError::Index(_))
        ));
        assert_eq!(wrong.0, 0);
        assert_eq!(
            crate::index::v4_full_snapshot_construction_count_for_test(),
            1
        );
    }

    #[test]
    fn public_v4_selection_operational_bound_is_exact_and_one_less_refuses_before_callbacks() {
        struct CountingVisitor(u64);
        impl crate::V4SelectionVisitor for CountingVisitor {
            type Error = std::convert::Infallible;

            fn visit(&mut self, _item: crate::V4SelectionItem<'_>) -> Result<(), Self::Error> {
                self.0 += 1;
                Ok(())
            }
        }

        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), crate::StoreLimits::default()).unwrap();
        let (journal, roots, _, _, _, _, _, _, _, _) =
            public_v3_fixture_journal(&root, "run:store-public-v4-selection-limits");
        let identity = journal.identity.clone();
        let index = crate::DerivedIndexV4::open(&root).unwrap();
        let receipt = index.rebuild_v4(&journal, &roots).unwrap();
        let snapshot = index.snapshot_current_v4(&journal, &roots).unwrap();
        let execution = snapshot.executions.first().unwrap();
        let selected: BTreeSet<StableId> =
            serde_json::from_str(&execution.obligation_ids_canonical_json).unwrap();
        let peak = crate::index::v4_selection_sql_operational_charge_for_test(
            &snapshot,
            &selected,
            index.limits(),
        )
        .unwrap();
        assert!(peak <= 256 * 1024 * 1024);
        let plan_id = execution.plan_id.clone();
        let expected_offset = snapshot.marker.confirmed_offset;
        let expected_events = snapshot.marker.event_count;
        let expected_tail = snapshot.marker.tail_hash.clone();
        drop(journal);
        drop(root);

        let exact = crate::StoreLimits {
            max_index_rows: receipt.accounting.rows,
            max_index_serialized_bytes: receipt.serialized_bytes,
            max_index_query_bytes: receipt.accounting.query_bytes,
            // Exact normative Working4 remains admissible; selection scratch
            // is governed by its separate Store operational ceiling.
            max_index_working_bytes: receipt.accounting.working_bytes,
            ..crate::StoreLimits::default()
        };
        let root = StoreRoot::open(workspace.path(), exact).unwrap();
        let journal = EventJournal::open(&root, identity.clone()).unwrap();
        let index = crate::DerivedIndexV4::open(&root).unwrap();
        crate::index::set_v4_selection_operational_limit_for_test(Some(peak));
        let mut exact_visitor = CountingVisitor(0);
        let summary = index
            .visit_current_v4_selection(
                &journal,
                &roots,
                crate::V4SelectionRequest {
                    plan_id: &plan_id,
                    selected_obligation_ids: &selected,
                    expected_confirmed_offset: expected_offset,
                    expected_event_count: expected_events,
                    expected_tail_hash: &expected_tail,
                },
                &mut exact_visitor,
            )
            .unwrap();
        assert_eq!(
            exact_visitor.0,
            summary.counts.artifact_registrations
                + summary.counts.executions
                + summary.counts.claims
                + summary.counts.evidence
                + summary.counts.evidence_bindings
                + summary.counts.verifications
                + summary.counts.decisions
                + summary.counts.findings
                + summary.counts.claim_assessments
                + summary.counts.obstructions
                + summary.counts.denominator_ids
                + summary.counts.visited_ids
                + summary.counts.completed_ids
                + summary.counts.evidence_supported_ids
                + summary.counts.verified_ids
                + summary.counts.accepted_ids
        );
        let wrong_tail = ContentHash::sha256(b"visitor expected-marker drift");
        let mut drift_visitor = CountingVisitor(0);
        assert!(matches!(
            index.visit_current_v4_selection(
                &journal,
                &roots,
                crate::V4SelectionRequest {
                    plan_id: &plan_id,
                    selected_obligation_ids: &selected,
                    expected_confirmed_offset: expected_offset,
                    expected_event_count: expected_events,
                    expected_tail_hash: &wrong_tail,
                },
                &mut drift_visitor,
            ),
            Err(crate::V4SelectionVisitError::Index(
                crate::IndexError::ProjectionContractViolation
            ))
        ));
        assert_eq!(drift_visitor.0, 0);
        crate::index::set_v4_selection_operational_limit_for_test(Some(peak - 1));
        let mut refused_visitor = CountingVisitor(0);
        assert!(matches!(
            index.visit_current_v4_selection(
                &journal,
                &roots,
                crate::V4SelectionRequest {
                    plan_id: &plan_id,
                    selected_obligation_ids: &selected,
                    expected_confirmed_offset: expected_offset,
                    expected_event_count: expected_events,
                    expected_tail_hash: &expected_tail,
                },
                &mut refused_visitor,
            ),
            Err(crate::V4SelectionVisitError::Index(
                crate::IndexError::Incomplete { limit, observed }
            )) if limit + 1 == observed && observed == peak
        ));
        assert_eq!(refused_visitor.0, 0);
        crate::index::set_v4_selection_operational_limit_for_test(None);
    }

    #[test]
    fn v3_identity_initialize_and_roots_bound_replay_return_opaque_basis() {
        let (_workspace, root) = root();
        let (identity, manifest, roots, genesis) = v3_fixture(&root);
        assert!(matches!(
            identity.verified_core_genesis().unwrap(),
            EventStreamGenesis::V3(bytes) if bytes == genesis.as_slice()
        ));
        let mut changed_run = identity.clone();
        changed_run.run_id = StableId::parse("run:v3-changed-after-verification").unwrap();
        assert!(matches!(
            changed_run.verified_core_genesis(),
            Err(JournalError::Identity(_))
        ));
        let mut changed_bytes = identity.clone();
        let JournalGenesis::V3Shared(bytes) = &mut changed_bytes.genesis else {
            unreachable!()
        };
        Arc::make_mut(bytes)[0] ^= 1;
        assert!(matches!(
            changed_bytes.verified_core_genesis(),
            Err(JournalError::Identity(_))
        ));
        let journal = EventJournal::initialize_v3(&root, identity, manifest).unwrap();
        assert!(matches!(
            journal.writer(),
            Err(JournalError::V3ReplaySessionRequired)
        ));

        let (session, basis) = journal.replayed_v3_session(&roots).unwrap();
        assert_eq!(session.event_count().unwrap(), 1);
        assert_eq!(basis.confirmed_event_count(), 1);
        assert_eq!(session.tail_hash().unwrap(), basis.confirmed_tail_hash());
        assert_eq!(session.run_id().unwrap(), basis.run_id());
        assert!(session.matches_store_root(&root).unwrap());
    }

    #[test]
    fn v3_replay_refuses_wrong_repository_root_and_missing_cas() {
        let (_workspace, admitted_root) = root();
        let (identity, manifest, _roots, _genesis) = v3_fixture(&admitted_root);
        let wrong_roots = AuthorityTrustRootsV3::new(
            ContentHash::sha256(b"journal-v3-policy"),
            identity.run_id.clone(),
            ContentHash::parse("sha256:1111111111111111").unwrap(),
            Vec::new(),
            Vec::new(),
        );
        assert!(wrong_roots.is_err());
        let wrong_roots = AuthorityTrustRootsV3::new(
            ContentHash::sha256(b"journal-v3-policy"),
            StableId::parse("repository:wrong").unwrap(),
            ContentHash::parse("sha256:1111111111111111").unwrap(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let journal = EventJournal::initialize_v3(&admitted_root, identity, manifest).unwrap();
        assert!(journal.replayed_v3_session(&wrong_roots).is_err());

        let (_workspace, missing_root) = root();
        let program = ProgramSpace::from_json_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let repository_id = program.repository_id().clone();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let run_id = StableId::parse("run:journal-v3-missing-cas").unwrap();
        let log = EventLog::new_v3(run_id.clone(), aggregate).unwrap();
        let genesis = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let identity = JournalIdentity::new(run_id, JournalGenesis::V3(genesis)).unwrap();
        let missing = EventJournal::initialize_v3(
            &missing_root,
            identity,
            log.envelopes().next().unwrap().clone(),
        )
        .unwrap();
        let roots = AuthorityTrustRootsV3::new(
            ContentHash::sha256(b"journal-v3-policy"),
            repository_id,
            ContentHash::parse("sha256:1111111111111111").unwrap(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        assert!(missing.replayed_v3_session(&roots).is_err());
    }

    #[test]
    fn v3_session_refuses_a_basis_from_another_roots_bound_session() {
        let (_workspace, root) = root();
        let (identity, manifest, first_roots, _genesis) = v3_fixture(&root);
        let journal = EventJournal::initialize_v3(&root, identity, manifest).unwrap();
        let (first_session, first_basis) = journal.replayed_v3_session(&first_roots).unwrap();
        drop(first_session);
        let second_roots = AuthorityTrustRootsV3::new(
            ContentHash::sha256(b"journal-v3-other-policy"),
            StableId::parse("repository:double-submit-payment").unwrap(),
            ContentHash::parse("sha256:1111111111111111").unwrap(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let (second_session, second_basis) = journal.replayed_v3_session(&second_roots).unwrap();
        assert!(matches!(
            second_session.checked_candidate(&first_basis),
            Err(JournalError::Domain(
                reviewgraphen_core::DomainError::AuthorityReplayBasisMismatch
            ))
        ));
        assert!(second_session.checked_candidate(&second_basis).is_ok());
    }

    #[test]
    fn v3_session_marker_gate_rejects_every_nonresume_surface_and_raw_append_bypass() {
        let (_workspace, root) = root();
        let (identity, manifest, roots, _genesis) = v3_fixture(&root);
        let journal = EventJournal::initialize_v3(&root, identity, manifest).unwrap();
        let (mut session, basis) = journal.replayed_v3_session(&roots).unwrap();
        publish_bundle_file(&session.writer.run, BUNDLE_PENDING_STAGE, b"0\n").unwrap();
        assert!(matches!(
            session.run_id(),
            Err(JournalError::SessionResumeRequired)
        ));
        assert!(matches!(
            session.aggregate(),
            Err(JournalError::SessionResumeRequired)
        ));
        assert!(matches!(
            session.tail_hash(),
            Err(JournalError::SessionResumeRequired)
        ));
        assert!(matches!(
            session.event_count(),
            Err(JournalError::SessionResumeRequired)
        ));
        assert!(matches!(
            session.store_root_identity(),
            Err(JournalError::SessionResumeRequired)
        ));
        assert!(matches!(
            session.matches_store_root(&root),
            Err(JournalError::SessionResumeRequired)
        ));
        assert!(matches!(
            session.mint_finding(
                &StableId::parse("claim:blocked-by-resume").unwrap(),
                "projection:test",
                &basis,
            ),
            Err(JournalError::SessionResumeRequired)
        ));
        assert!(matches!(
            session.execute_fixture_harness(
                &StableId::parse("claim:blocked-by-resume").unwrap(),
                &basis,
            ),
            Err(JournalError::SessionResumeRequired)
        ));
        assert!(matches!(
            session.claim_assessment(&StableId::parse("claim:blocked-by-resume").unwrap()),
            Err(JournalError::SessionResumeRequired)
        ));
        assert!(matches!(
            session.evidence_count(),
            Err(JournalError::SessionResumeRequired)
        ));
        assert!(matches!(
            session.writer.append_batch(&[]),
            Err(JournalError::SessionResumeRequired)
        ));

        fs::unlinkat(&session.writer.run, BUNDLE_PENDING_STAGE, AtFlags::empty()).unwrap();
        session.state = ReplayedV3RunSessionState::ResumeOnly;
        assert!(matches!(
            session.run_id(),
            Err(JournalError::SessionResumeRequired)
        ));
    }

    #[test]
    fn bundle_resume_error_mapping_preserves_resource_and_normative_domain_errors() {
        assert!(matches!(
            map_bundle_resume_domain_error(
                reviewgraphen_core::DomainError::BundleResumeAuthorityMismatch
            ),
            JournalError::BundleResumeAuthorityMismatch
        ));
        assert!(matches!(
            map_bundle_resume_domain_error(reviewgraphen_core::DomainError::Incomplete {
                operation: "resume-test",
                limit: 7,
                observed: 8,
            }),
            JournalError::Incomplete {
                limit: 7,
                observed: 8
            }
        ));
        assert!(matches!(
            map_bundle_resume_domain_error(reviewgraphen_core::DomainError::AlreadyComplete),
            JournalError::Domain(reviewgraphen_core::DomainError::AlreadyComplete)
        ));
    }

    #[test]
    fn public_v3_fixture_bundle_recovers_line_synced_partial_suffix_end_to_end() {
        for (index, fault, expected_stage) in [
            (0_u8, AppendFault::BundleAfterDurableLine1, 1_u64),
            (1, AppendFault::BundleAfterDurableLine2, 2),
            (2, AppendFault::BundleStageSync, 1),
        ] {
            let (_workspace, root) = root();
            let run = format!("run:store-public-v3-e2e-{index}");
            let (journal, roots, claim_id, harness_root, _, _, _, _, _, _) =
                public_v3_fixture_journal(&root, &run);
            if index == 0 {
                let before_cas = test_cas_inventory(&root);
                let mut wrong_harness = clone_test_harness_root(&harness_root);
                wrong_harness.harness_revision = "fixture-harness-wrong-revision".to_owned();
                assert!(
                    AuthorityTrustRootsV3::new(
                        harness_root.policy_revision_hash.clone(),
                        harness_root.repository_id.clone(),
                        harness_root.repository_source_hash.clone(),
                        vec![wrong_harness],
                        Vec::new(),
                    )
                    .is_err()
                );
                assert_eq!(test_cas_inventory(&root), before_cas);

                let missing_roots = AuthorityTrustRootsV3::new(
                    harness_root.policy_revision_hash.clone(),
                    harness_root.repository_id.clone(),
                    harness_root.repository_source_hash.clone(),
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap();
                let (blocked, blocked_basis) = journal.replayed_v3_session(&missing_roots).unwrap();
                let before_events = blocked.event_count().unwrap();
                let before_tail = blocked.tail_hash().unwrap().clone();
                assert!(
                    blocked
                        .expect_fixture_verification_attempt_v3(&claim_id, &blocked_basis)
                        .is_err()
                );
                assert!(
                    blocked
                        .execute_fixture_harness(&claim_id, &blocked_basis)
                        .is_err()
                );
                assert_eq!(blocked.event_count().unwrap(), before_events);
                assert_eq!(blocked.tail_hash().unwrap(), &before_tail);
                assert_eq!(test_cas_inventory(&root), before_cas);
                drop(blocked);
            }
            let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
            let ready = session
                .expect_fixture_verification_attempt_v3(&claim_id, &basis)
                .unwrap();
            assert_eq!(
                session
                    .inspect_m4_verification_attempt_v3(&ready, &basis)
                    .unwrap(),
                VerificationAttemptStageV3::Ready
            );
            let mut fixture = session.execute_fixture_harness(&claim_id, &basis).unwrap();
            let witness_bytes = fixture.witness_bytes().to_vec();
            let result_bytes = fixture.fixture_result_bytes().to_vec();
            put_test_cas(&root, &witness_bytes);
            put_test_cas(&root, &result_bytes);
            assert!(matches!(
                session.inspect_m4_verification_attempt_v3(&ready, &basis),
                Err(JournalError::OrphanCasObjectV3 { hash }) if hash == *ready.input_hash()
            ));
            let before_validation_count = session.event_count().unwrap();
            let before_validation_tail = session.tail_hash().unwrap().clone();
            session.validate_v3_operation_basis(&basis).unwrap();
            assert_eq!(session.event_count().unwrap(), before_validation_count);
            assert_eq!(session.tail_hash().unwrap(), &before_validation_tail);
            assert!(
                session
                    .cas_contains_exact(
                        &ContentHash::sha256(&witness_bytes),
                        u64::try_from(witness_bytes.len()).unwrap(),
                    )
                    .unwrap()
            );
            assert!(
                session
                    .cas_contains_exact(
                        &ContentHash::sha256(&witness_bytes),
                        u64::try_from(witness_bytes.len()).unwrap() + 1,
                    )
                    .is_err()
            );

            let witness = session
                .prepare_external_fixture_witness_registration(&mut fixture, &basis)
                .unwrap();
            let witness_id = witness.registration_id().clone();
            session
                .append_authority_registration(witness, &mut basis)
                .unwrap();
            let output = session
                .prepare_fixture_verifier_output_registration(&mut fixture, &basis)
                .unwrap();
            let output_id = output.registration_id().clone();
            session
                .append_authority_registration(output, &mut basis)
                .unwrap();
            let registered = session
                .expect_fixture_verification_attempt_v3(&claim_id, &basis)
                .unwrap();
            assert_eq!(
                session
                    .inspect_m4_verification_attempt_v3(&registered, &basis)
                    .unwrap(),
                VerificationAttemptStageV3::OutputRegistered
            );
            let admission = session
                .admit_external_fixture_witness(&mut fixture, &witness_id, &basis)
                .unwrap();
            let bundle = session
                .mint_fixture_verification_bundle(admission, &output_id, &basis)
                .unwrap();
            let before_digest = basis.basis_digest().clone();
            let before_tail = basis.confirmed_tail_hash().clone();
            let before_events = basis.confirmed_event_count();
            session.writer.inject_faults([fault]);
            let interrupted = session.append_verification_bundle(bundle, &mut basis);
            if fault == AppendFault::BundleStageSync {
                assert!(matches!(interrupted, Err(JournalError::SessionUncertain)));
                assert!(matches!(
                    session.run_id(),
                    Err(JournalError::SessionUncertain)
                ));
            } else {
                assert!(matches!(
                    interrupted,
                    Err(JournalError::BundleAppendInterrupted { durable_stage })
                        if durable_stage.confirmed_events() == expected_stage
                            && durable_stage.expected_events() == 3
                ));
                assert!(matches!(
                    session.run_id(),
                    Err(JournalError::SessionResumeRequired)
                ));
            }
            assert_eq!(basis.basis_digest(), &before_digest);
            assert_eq!(basis.confirmed_tail_hash(), &before_tail);
            assert_eq!(basis.confirmed_event_count(), before_events);
            drop(session);

            let (intents, completions) = journal.recovery_dirs().unwrap();
            let audit_before =
                recovery_audit(&intents, &completions, &journal.identity, journal.limits).unwrap();
            for _ in 0..2 {
                assert!(matches!(
                    journal.recover("test", "partial-no-mutation"),
                    Err(JournalError::BundleAppendInterrupted { durable_stage })
                        if durable_stage.confirmed_events() == expected_stage
                ));
            }
            let (after_intents, after_completions) = journal.recovery_dirs().unwrap();
            let audit_after = recovery_audit(
                &after_intents,
                &after_completions,
                &journal.identity,
                journal.limits,
            )
            .unwrap();
            assert_eq!(audit_after.receipt_files, audit_before.receipt_files);
            assert_eq!(
                audit_after.receipt_scan_bytes,
                audit_before.receipt_scan_bytes
            );
            assert_eq!(audit_after.completed.len(), audit_before.completed.len());

            let wrong_roots = AuthorityTrustRootsV3::new(
                ContentHash::sha256(b"wrong-store-public-v3-fixture-policy"),
                StableId::parse("repository:double-submit-payment").unwrap(),
                ContentHash::parse("sha256:1111111111111111").unwrap(),
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
            assert!(
                journal
                    .recover_verification_bundle_resume(&wrong_roots)
                    .is_err()
            );

            if index == 0 {
                let witness_hash =
                    CasHash::parse(ContentHash::sha256(&witness_bytes).to_string()).unwrap();
                let object = root
                    .path()
                    .join("artifacts")
                    .join("sha256")
                    .join(witness_hash.prefix())
                    .join(witness_hash.hex());
                let missing = object.with_extension("missing-for-recovery-test");
                std::fs::rename(&object, &missing).unwrap();
                assert!(journal.recover_verification_bundle_resume(&roots).is_err());
                std::fs::rename(&missing, &object).unwrap();
            }

            let (recovered, mut current_basis, authority) =
                journal.recover_verification_bundle_resume(&roots).unwrap();
            assert_eq!(recovered.durable_stage().confirmed_events(), expected_stage);
            assert_eq!(recovered.durable_stage().expected_events(), 3);
            assert_eq!(
                current_basis.confirmed_event_count(),
                before_events + expected_stage
            );
            let (healthy, receipt) = recovered
                .resume_verification_bundle(authority, &mut current_basis)
                .unwrap();
            assert_eq!(receipt.journal().len(), (3 - expected_stage) as usize);
            assert_ne!(current_basis.basis_digest(), &before_digest);
            assert_eq!(healthy.evidence_count().unwrap(), 1);
            assert_eq!(healthy.evidence_binding_count().unwrap(), 1);
            assert_eq!(healthy.verification_count().unwrap(), 1);
            let complete = healthy
                .expect_fixture_verification_attempt_v3(&claim_id, &current_basis)
                .unwrap();
            assert_eq!(
                healthy
                    .inspect_m4_verification_attempt_v3(&complete, &current_basis)
                    .unwrap(),
                VerificationAttemptStageV3::Complete
            );
            drop(healthy);

            assert!(matches!(
                journal.recover_verification_bundle_resume(&roots),
                Err(JournalError::BundleResumeAuthorityMismatch)
            ));
            let (mut replayed, mut replayed_basis) = journal.replayed_v3_session(&roots).unwrap();
            assert_eq!(replayed.evidence_count().unwrap(), 1);
            assert_eq!(replayed.evidence_binding_count().unwrap(), 1);
            assert_eq!(replayed.verification_count().unwrap(), 1);
            assert_eq!(
                replayed_basis.confirmed_event_count(),
                current_basis.confirmed_event_count()
            );
            let assessment = replayed.claim_assessment(&claim_id).unwrap().unwrap();
            assert_eq!(assessment.disposition(), AssessmentDispositionV3::Supported);
            assert_eq!(
                assessment.review_status(),
                AssessmentReviewStatusV3::Unreviewed
            );
            let active_path = root.path().join("indexes").join("reviewgraphen.sqlite");
            let stale_image = if index == 0 {
                drop(replayed);
                let derived = crate::DerivedIndexV4::open(&root).unwrap();
                derived.rebuild_v4(&journal, &roots).unwrap();
                let image = std::fs::read(&active_path).unwrap();
                let reopened = journal.replayed_v3_session(&roots).unwrap();
                replayed = reopened.0;
                replayed_basis = reopened.1;
                Some(image)
            } else {
                None
            };
            let decision = replayed
                .mint_decision(
                    &claim_id,
                    DecisionInputV3::new(
                        DecisionOutcomeV3::Accept,
                        "human:store-reviewer",
                        "store-review-board",
                        "explicit acceptance of the reproduced counterexample",
                        "2026-08-10T00:00:00Z",
                        None,
                    ),
                    &replayed_basis,
                )
                .unwrap();
            replayed
                .append_decision(decision, &mut replayed_basis)
                .unwrap();
            let finding = replayed
                .mint_finding(
                    &claim_id,
                    "reviewgraphen.finding_projection@1",
                    &replayed_basis,
                )
                .unwrap();
            replayed
                .append_finding(finding, &mut replayed_basis)
                .unwrap();
            let assessment = replayed.claim_assessment(&claim_id).unwrap().unwrap();
            assert_eq!(assessment.disposition(), AssessmentDispositionV3::Accepted);
            assert_eq!(
                assessment.review_status(),
                AssessmentReviewStatusV3::Accepted
            );
            drop(replayed);

            let derived = crate::DerivedIndexV4::open(&root).unwrap();
            if let Some(stale_image) = stale_image.as_ref() {
                assert!(matches!(
                    derived.snapshot_current_v4(&journal, &roots),
                    Err(crate::IndexError::CommittedIndexStale { .. })
                ));
                for statement in [
                    "UPDATE program_relations SET body_hash='sha256:0000000000000000'",
                    "UPDATE artifact_registrations SET source_kind='reviewer_execution' WHERE source_kind='run_genesis'",
                ] {
                    let connection = rusqlite::Connection::open(&active_path).unwrap();
                    assert!(connection.execute(statement, []).unwrap() > 0);
                    drop(connection);
                    assert!(matches!(
                        derived.snapshot_current_v4(&journal, &roots),
                        Err(crate::IndexError::CorruptIndex)
                    ));
                    std::fs::write(&active_path, stale_image).unwrap();
                }
                let connection = rusqlite::Connection::open(&active_path).unwrap();
                connection
                    .pragma_update(None, "ignore_check_constraints", true)
                    .unwrap();
                assert!(
                    connection
                        .execute("UPDATE obligations SET lifecycle='invalid'", [])
                        .unwrap()
                        > 0
                );
                drop(connection);
                assert!(matches!(
                    derived.snapshot_current_v4(&journal, &roots),
                    Err(crate::IndexError::CorruptIndex)
                ));
                std::fs::write(&active_path, stale_image).unwrap();

                let connection = rusqlite::Connection::open(&active_path).unwrap();
                connection
                    .pragma_update(None, "foreign_keys", false)
                    .unwrap();
                assert!(
                    connection
                        .execute(
                            "UPDATE evidence_bindings_v3 SET evidence_id='evidence:missing'",
                            [],
                        )
                        .unwrap()
                        > 0
                );
                drop(connection);
                assert!(matches!(
                    derived.snapshot_current_v4(&journal, &roots),
                    Err(crate::IndexError::CorruptIndex)
                ));
                std::fs::write(&active_path, stale_image).unwrap();
            }
            let receipt = derived.rebuild_v4(&journal, &roots).unwrap();
            assert_eq!(
                receipt.authority_replay_basis_digest,
                replayed_basis.basis_digest().clone()
            );
            let snapshot = derived.snapshot_current_v4(&journal, &roots).unwrap();
            assert_eq!(snapshot.evidence.len(), 1);
            assert_eq!(snapshot.evidence_bindings.len(), 1);
            assert_eq!(snapshot.verifications.len(), 1);
            assert_eq!(snapshot.decisions.len(), 1);
            assert_eq!(snapshot.findings.len(), 1);
            assert_eq!(snapshot.claim_assessments.len(), 1);
            assert_eq!(snapshot.claim_assessments[0].disposition, "accepted");
            assert_eq!(snapshot.findings[0].status, "accepted");
            if index == 0 {
                let current_image = std::fs::read(&active_path).unwrap();
                for statement in [
                    "UPDATE index_meta SET tail_hash='sha256:0000000000000000'",
                    "UPDATE index_meta SET authority_replay_basis_digest='sha256:0000000000000000'",
                    "UPDATE evidence_v3 SET body_hash='sha256:0000000000000000'",
                    "UPDATE evidence_bindings_v3 SET body_hash='sha256:0000000000000000'",
                    "UPDATE verifications_v3 SET body_hash='sha256:0000000000000000'",
                    "UPDATE decisions_v3 SET body_hash='sha256:0000000000000000'",
                    "UPDATE findings_v3 SET body_hash='sha256:0000000000000000'",
                    "UPDATE artifact_registrations SET source_canonical_json='{\"kind\":\"run_genesis\",\"run_id\":\"run:wrong\"}' WHERE source_kind='run_genesis'",
                    "UPDATE artifact_registrations SET cas_hash='sha256:0000000000000000' WHERE source_kind='external_harness_witness'",
                    "UPDATE review_plans SET budget_canonical_json=budget_canonical_json||' '",
                    "UPDATE context_envelopes SET losses_canonical_json=losses_canonical_json||' '",
                    "UPDATE executions SET inference_settings_canonical_json=inference_settings_canonical_json||' '",
                    "UPDATE claims SET assumptions_canonical_json=assumptions_canonical_json||' '",
                    "UPDATE evidence_v3 SET subject_ids_canonical_json=subject_ids_canonical_json||' '",
                    "UPDATE verifications_v3 SET limitations_canonical_json=limitations_canonical_json||' '",
                    "UPDATE decisions_v3 SET source_ids_canonical_json=source_ids_canonical_json||' '",
                    "UPDATE findings_v3 SET evidence_ids_canonical_json=evidence_ids_canonical_json||' '",
                    "UPDATE claim_assessments_v3 SET verification_ids_canonical_json=verification_ids_canonical_json||' '",
                ] {
                    let connection = rusqlite::Connection::open(&active_path).unwrap();
                    connection
                        .pragma_update(None, "ignore_check_constraints", true)
                        .unwrap();
                    assert!(
                        connection.execute(statement, []).unwrap() > 0,
                        "{statement}"
                    );
                    drop(connection);
                    assert!(matches!(
                        derived.snapshot_current_v4(&journal, &roots),
                        Err(crate::IndexError::CorruptIndex)
                    ));
                    std::fs::write(&active_path, &current_image).unwrap();
                }
                for statement in [
                    "UPDATE evidence_v3 SET kind='invalid'",
                    "UPDATE evidence_bindings_v3 SET relation='invalid'",
                    "UPDATE verifications_v3 SET outcome='invalid'",
                    "UPDATE decisions_v3 SET outcome='invalid'",
                    "UPDATE findings_v3 SET status='invalid'",
                    "UPDATE claim_assessments_v3 SET disposition='invalid'",
                ] {
                    let connection = rusqlite::Connection::open(&active_path).unwrap();
                    connection
                        .pragma_update(None, "ignore_check_constraints", true)
                        .unwrap();
                    assert!(
                        connection.execute(statement, []).unwrap() > 0,
                        "{statement}"
                    );
                    drop(connection);
                    assert!(matches!(
                        derived.snapshot_current_v4(&journal, &roots),
                        Err(crate::IndexError::CorruptIndex)
                    ));
                    std::fs::write(&active_path, &current_image).unwrap();
                }
                for statement in [
                    "UPDATE evidence_v3 SET input_registration_id='registration:missing'",
                    "UPDATE evidence_bindings_v3 SET claim_id='claim:missing'",
                    "UPDATE verifications_v3 SET output_registration_id='registration:missing'",
                    "UPDATE decisions_v3 SET claim_id='claim:missing'",
                    "UPDATE findings_v3 SET decision_id='decision:missing'",
                    "UPDATE claim_assessments_v3 SET current_finding_id='finding:missing'",
                ] {
                    let connection = rusqlite::Connection::open(&active_path).unwrap();
                    connection
                        .pragma_update(None, "foreign_keys", false)
                        .unwrap();
                    assert!(
                        connection.execute(statement, []).unwrap() > 0,
                        "{statement}"
                    );
                    drop(connection);
                    assert!(matches!(
                        derived.snapshot_current_v4(&journal, &roots),
                        Err(crate::IndexError::CorruptIndex)
                    ));
                    std::fs::write(&active_path, &current_image).unwrap();
                }
                for found in [2_u32, 3] {
                    let connection = rusqlite::Connection::open(&active_path).unwrap();
                    connection
                        .pragma_update(None, "user_version", found)
                        .unwrap();
                    drop(connection);
                    assert!(matches!(
                        derived.snapshot_current_v4(&journal, &roots),
                        Err(crate::IndexError::RebuildRequired {
                            found: actual,
                            required: 4
                        }) if actual == found
                    ));
                    std::fs::write(&active_path, &current_image).unwrap();
                }
                let old_image = stale_image.as_ref().unwrap();
                for fault in [
                    crate::index::PublishFault::AfterWrite,
                    crate::index::PublishFault::BeforeCandidateSync,
                    crate::index::PublishFault::AfterCandidateLinkBeforeDirectorySync,
                    crate::index::PublishFault::AfterCandidateSync,
                    crate::index::PublishFault::AfterImageDropBeforeCandidateRead,
                    crate::index::PublishFault::AfterCandidateInodeCheck,
                    crate::index::PublishFault::AfterCandidateHashCheck,
                    crate::index::PublishFault::AfterValidation,
                    crate::index::PublishFault::AfterRename,
                    crate::index::PublishFault::AfterActiveInodeCheck,
                    crate::index::PublishFault::AfterActiveHashCheck,
                    crate::index::PublishFault::AfterActiveVerify,
                    crate::index::PublishFault::BeforeFinalDirectorySync,
                ] {
                    std::fs::write(&active_path, old_image).unwrap();
                    derived.inject_publish_fault(fault);
                    let result = derived.rebuild_v4(&journal, &roots);
                    let before_rename = matches!(
                        fault,
                        crate::index::PublishFault::AfterWrite
                            | crate::index::PublishFault::BeforeCandidateSync
                            | crate::index::PublishFault::AfterCandidateLinkBeforeDirectorySync
                            | crate::index::PublishFault::AfterCandidateSync
                            | crate::index::PublishFault::AfterImageDropBeforeCandidateRead
                            | crate::index::PublishFault::AfterCandidateInodeCheck
                            | crate::index::PublishFault::AfterCandidateHashCheck
                            | crate::index::PublishFault::AfterValidation
                    );
                    if before_rename {
                        assert!(matches!(result, Err(crate::IndexError::Io(_))));
                        assert_eq!(std::fs::read(&active_path).unwrap(), *old_image);
                    } else {
                        assert!(matches!(
                            result,
                            Err(crate::IndexError::PublicationDurabilityUncertain { .. })
                        ));
                        assert_eq!(std::fs::read(&active_path).unwrap(), current_image);
                        derived.snapshot_current_v4(&journal, &roots).unwrap();
                    }
                }
                std::fs::write(&active_path, &current_image).unwrap();
            }
            let expected_rows = 1_u64
                + snapshot.events.len() as u64
                + snapshot.projected_findings.len() as u64
                + snapshot.shadows.len() as u64
                + snapshot.program_objects.len() as u64
                + snapshot.program_relations.len() as u64
                + u64::from(snapshot.universe.is_some())
                + snapshot.obligations.len() as u64
                + snapshot.obligation_lifecycle.len() as u64
                + snapshot.executions.len() as u64
                + snapshot.claims.len() as u64
                + snapshot.artifact_registrations.len() as u64
                + snapshot.snapshot_sources.len() as u64
                + snapshot.context_envelopes.len() as u64
                + snapshot.review_plans.len() as u64
                + snapshot.evidence.len() as u64
                + snapshot.evidence_bindings.len() as u64
                + snapshot.verifications.len() as u64
                + snapshot.decisions.len() as u64
                + snapshot.findings.len() as u64
                + snapshot.claim_assessments.len() as u64;
            assert_eq!(receipt.accounting.rows, expected_rows);
            assert_eq!(
                receipt.accounting.sql_bytes,
                receipt.accounting.text_bytes
                    + 8 * receipt.accounting.integer_cells
                    + receipt.accounting.rows
            );
            assert_eq!(
                receipt.accounting.query_bytes,
                canonical_json(&snapshot).unwrap().len() as u64
            );
            let accounting_session = journal.replayed_v3_session(&roots).unwrap();
            let (streamed, materialized_oracle) =
                crate::index::preflight_accounting_limits_for_test(
                    &accounting_session.0,
                    &accounting_session.1,
                    u64::MAX,
                    u64::MAX,
                )
                .unwrap();
            assert_eq!(streamed, materialized_oracle);
            assert_eq!(streamed.rows, receipt.accounting.rows);
            assert_eq!(streamed.integer_cells, receipt.accounting.integer_cells);
            assert_eq!(streamed.text_bytes, receipt.accounting.text_bytes);
            assert_eq!(streamed.sql_bytes, receipt.accounting.sql_bytes);
            assert_eq!(streamed.query_bytes, receipt.accounting.query_bytes);
            assert_eq!(streamed.owned_bytes, receipt.accounting.owned_bytes);
            assert!(receipt.accounting.working_bytes >= streamed.working_bytes);
            crate::index::preflight_accounting_limits_for_test(
                &accounting_session.0,
                &accounting_session.1,
                streamed.query_bytes,
                streamed.working_bytes,
            )
            .unwrap();
            assert!(matches!(
                crate::index::preflight_accounting_limits_for_test(
                    &accounting_session.0,
                    &accounting_session.1,
                    streamed.query_bytes - 1,
                    streamed.working_bytes,
                ),
                Err(crate::IndexError::Incomplete { limit, observed })
                    if limit + 1 == observed && observed == streamed.query_bytes
            ));
            assert!(matches!(
                crate::index::preflight_accounting_limits_for_test(
                    &accounting_session.0,
                    &accounting_session.1,
                    streamed.query_bytes,
                    streamed.working_bytes - 1,
                ),
                Err(crate::IndexError::Incomplete { limit, observed })
                    if limit + 1 == observed && observed == streamed.working_bytes
            ));
            drop(accounting_session);
            assert_eq!(
                derived.snapshot_current_v4(&journal, &roots).unwrap(),
                snapshot
            );
            let rebuilt = derived.rebuild_v4(&journal, &roots).unwrap();
            assert_eq!(rebuilt.image_hash, receipt.image_hash);
            assert_eq!(rebuilt.accounting, receipt.accounting);
            if index == 0 {
                let (mut conflict_session, mut conflict_basis) =
                    journal.replayed_v3_session(&roots).unwrap();
                let evaluation = {
                    let aggregate = conflict_session.aggregate().unwrap();
                    let claim = aggregate
                        .execution_claims()
                        .find(|claim| claim.id() == &claim_id)
                        .unwrap();
                    let obligation = aggregate
                        .obligations()
                        .find(|obligation| Some(obligation.id()) == claim.obligation_ids().first())
                        .unwrap();
                    evaluate_static_fact_v1(aggregate.program(), obligation, claim).unwrap()
                };
                let input_bytes = canonical_json(evaluation.input()).unwrap();
                let output_bytes = canonical_json(evaluation.result()).unwrap();
                put_test_cas(&root, &input_bytes);
                put_test_cas(&root, &output_bytes);
                let input = conflict_session
                    .prepare_static_verifier_artifact_registration(
                        claim_id.clone(),
                        VerifierArtifactRoleV3::Input,
                        ContentHash::sha256(&input_bytes),
                        u64::try_from(input_bytes.len()).unwrap(),
                        &conflict_basis,
                    )
                    .unwrap();
                let input_id = input.registration_id().clone();
                conflict_session
                    .append_authority_registration(input, &mut conflict_basis)
                    .unwrap();
                let output = conflict_session
                    .prepare_static_verifier_artifact_registration(
                        claim_id.clone(),
                        VerifierArtifactRoleV3::Output,
                        ContentHash::sha256(&output_bytes),
                        u64::try_from(output_bytes.len()).unwrap(),
                        &conflict_basis,
                    )
                    .unwrap();
                let output_id = output.registration_id().clone();
                conflict_session
                    .append_authority_registration(output, &mut conflict_basis)
                    .unwrap();
                let bundle = conflict_session
                    .mint_static_verification_bundle(
                        &claim_id,
                        &input_id,
                        &output_id,
                        &conflict_basis,
                    )
                    .unwrap();
                conflict_session
                    .append_verification_bundle(bundle, &mut conflict_basis)
                    .unwrap();
                let conflict_assessment = conflict_session
                    .claim_assessment(&claim_id)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    conflict_assessment.disposition(),
                    AssessmentDispositionV3::Supported
                );
                assert!(conflict_assessment.decision_conflict());
                assert!(conflict_assessment.current_finding_id().is_none());
                assert_eq!(conflict_assessment.verification_ids().len(), 2);
                drop(conflict_session);

                assert!(matches!(
                    derived.snapshot_current_v4(&journal, &roots),
                    Err(crate::IndexError::CommittedIndexStale { .. })
                ));
                derived.rebuild_v4(&journal, &roots).unwrap();
                let conflict_snapshot = derived.snapshot_current_v4(&journal, &roots).unwrap();
                assert_eq!(conflict_snapshot.verifications.len(), 2);
                assert_eq!(conflict_snapshot.findings.len(), 1);
                let projected = &conflict_snapshot.claim_assessments[0];
                assert_eq!(projected.disposition, "supported");
                assert_eq!(projected.review_status, "human_reviewed");
                assert!(projected.decision_conflict);
                assert!(projected.active_decision_id.is_none());
                assert!(projected.current_finding_id.is_none());
                assert_eq!(
                    serde_json::from_str::<Vec<StableId>>(
                        &projected.verification_ids_canonical_json
                    )
                    .unwrap()
                    .len(),
                    2
                );
            }
        }
    }

    #[test]
    fn v3_initialize_limits_and_manifest_cursor_are_exact() {
        let (_workspace, exact_root) = root();
        let (identity, manifest, _roots, _genesis) = v3_fixture(&exact_root);
        let line_len = u64::try_from(manifest.canonical_bytes().unwrap().len() + 1).unwrap();
        let mut exact = JournalLimits::from_store(exact_root.limits());
        exact.max_event_line_bytes = line_len;
        exact.max_events = 1;
        exact.max_replay_bytes = line_len;
        EventJournal::initialize_v3_with_limits(
            &exact_root,
            identity.clone(),
            manifest.clone(),
            exact,
        )
        .unwrap();

        let (_workspace, small_root) = root();
        let (small_identity, small_manifest, _roots, _genesis) = v3_fixture(&small_root);
        let mut small = JournalLimits::from_store(small_root.limits());
        small.max_event_line_bytes = line_len - 1;
        small.max_events = 1;
        small.max_replay_bytes = line_len;
        assert!(matches!(
            EventJournal::initialize_v3_with_limits(
                &small_root,
                small_identity,
                small_manifest,
                small,
            ),
            Err(JournalError::Incomplete { limit, observed })
                if limit == line_len - 1 && observed == line_len
        ));

        let (_workspace, bad_root) = root();
        let (bad_identity, bad_manifest, bad_roots, _genesis) = v3_fixture(&bad_root);
        let bad_run_id = bad_identity.run_id.clone();
        let bad_journal =
            EventJournal::initialize_v3(&bad_root, bad_identity, bad_manifest.clone()).unwrap();
        let mut value = serde_json::to_value(bad_manifest).unwrap();
        value["sequence"] = Value::from(2_u64);
        let mut bytes = canonical_json(&value).unwrap();
        bytes.push(b'\n');
        std::fs::write(
            bad_root
                .path()
                .join(RUNS_DIR)
                .join(run_dir_name(&bad_run_id))
                .join(JOURNAL_FILE),
            bytes,
        )
        .unwrap();
        assert!(bad_journal.replayed_v3_session(&bad_roots).is_err());
    }

    #[test]
    fn v3_authority_resolver_rehashes_the_exact_cas_object() {
        let (_workspace, root) = root();
        let (identity, manifest, roots, genesis) = v3_fixture(&root);
        let journal = EventJournal::initialize_v3(&root, identity, manifest).unwrap();
        let genesis_hash = ContentHash::sha256(&genesis);
        let hash = CasHash::parse(genesis_hash.to_string()).unwrap();
        let object = root
            .path()
            .join("artifacts")
            .join("sha256")
            .join(hash.prefix())
            .join(hash.hex());
        let mut corrupt = genesis.clone();
        corrupt[0] ^= 1;
        std::fs::write(object, &corrupt).unwrap();
        let resolver = JournalAuthorityResolverV3 {
            reader: CasReader::open_existing(&root).unwrap(),
        };
        let mut destination = vec![0_u8; corrupt.len()];
        assert!(
            resolver
                .read_exact(&genesis_hash, &mut destination)
                .is_err()
        );
        // Genesis replay remains bound to the identity's verified immutable
        // bytes; authority artifact reads use the resolver exercised above.
        assert!(journal.replayed_v3_session(&roots).is_ok());
    }

    #[test]
    fn verified_genesis_certificate_refuses_public_identity_mutation() {
        let (identity, _) = fixture_event();
        assert!(matches!(
            identity.verified_core_genesis().unwrap(),
            EventStreamGenesis::V2Verified(_)
        ));

        let mut changed_run = identity.clone();
        changed_run.run_id = StableId::parse("run:changed-after-verification").unwrap();
        assert!(matches!(
            changed_run.verified_core_genesis(),
            Err(JournalError::Identity(_))
        ));

        let mut changed_bytes = identity;
        let JournalGenesis::V2Shared(bytes) = &mut changed_bytes.genesis else {
            unreachable!()
        };
        Arc::make_mut(bytes)[0] ^= 1;
        assert!(matches!(
            changed_bytes.verified_core_genesis(),
            Err(JournalError::Identity(_))
        ));
    }

    #[test]
    fn index_stream_replays_one_line_at_a_time_and_identity_shares_genesis_backing() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let identity_clone = identity.clone();
        assert!(Arc::ptr_eq(
            identity.v2_genesis_backing().expect("V2 genesis backing"),
            identity_clone
                .v2_genesis_backing()
                .expect("V2 genesis backing"),
        ));

        let journal = open_fixture(&root, identity);
        journal.writer().unwrap().append(event).unwrap();
        let ordinary = journal.reader().unwrap();
        let expected_offset = ordinary.confirmed_offset();
        let expected_tail = ordinary.tail_hash().clone();
        drop(ordinary);

        let mut streamed = journal
            .index_reader(root.limits().max_index_working_bytes)
            .unwrap();
        let mut seen = 0_u64;
        let certificate = streamed
            .with_locked_prefix::<()>(|envelope: &EventEnvelope, boundary| {
                seen += 1;
                assert_eq!(envelope.sequence(), seen);
                assert!(boundary <= expected_offset);
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, certificate.event_count);
        assert_eq!(certificate.confirmed_offset, expected_offset);
        assert_eq!(certificate.tail_hash, expected_tail);
        assert_eq!(
            certificate.line_buffer_capacity,
            root.limits().max_event_line_bytes
        );
    }

    #[test]
    fn index_stream_maps_event_structure_cap_to_incomplete_not_corruption() {
        fn family_json(kind: &str, elements: usize, depth: usize) -> Vec<u8> {
            let mut json = format!("{{\"payload\":{{\"type\":\"{kind}\",\"padding\":").into_bytes();
            json.extend(std::iter::repeat_n(b'[', depth));
            for element in 0..elements {
                if element != 0 {
                    json.push(b',');
                }
                json.push(b'0');
            }
            json.extend(std::iter::repeat_n(b']', depth));
            json.extend_from_slice(b"}}\n");
            json
        }

        for kind in [
            "obligation_transition",
            "claim_proposed",
            "evidence_recorded",
            "evidence_bound",
            "verification_recorded",
            "decision_recorded",
            "finding_recorded",
            "run_genesis_manifest",
            "artifact_registered",
            "snapshot_sources_recorded",
            "review_plan_recorded",
            "context_envelope_projected",
        ] {
            for line in [family_json(kind, 65_533, 1), family_json(kind, 1, 126)] {
                let (_workspace, root) = root();
                let (identity, _) = fixture_event();
                let journal = open_fixture(&root, identity);
                std::fs::write(log_path(&root), line).unwrap();
                let mut reader = journal
                    .index_reader(root.limits().max_index_working_bytes)
                    .unwrap();
                let result = reader.with_locked_prefix::<()>(|_, _| Ok(()));
                assert!(
                    matches!(
                        result,
                        Err(IndexReplayError::Journal(
                            JournalError::CorruptNeedsRecovery { .. }
                        ))
                    ),
                    "exact structural cap for {kind}: {result:?}"
                );
            }
            for (line, expected_limit) in [
                (family_json(kind, 65_534, 1), 65_536),
                (family_json(kind, 1, 127), 128),
            ] {
                let (_workspace, root) = root();
                let (identity, _) = fixture_event();
                let journal = open_fixture(&root, identity);
                std::fs::write(log_path(&root), line).unwrap();
                let mut reader = journal
                    .index_reader(root.limits().max_index_working_bytes)
                    .unwrap();
                let result = reader.with_locked_prefix::<()>(|_, _| Ok(()));
                assert!(
                    matches!(
                        result,
                        Err(IndexReplayError::Journal(JournalError::Incomplete {
                            limit,
                            observed,
                        })) if limit == expected_limit && observed == expected_limit + 1
                    ),
                    "{kind}: {result:?}"
                );
            }
        }
    }

    #[test]
    fn index_reader_maps_genesis_structure_plus_one_before_typed_decode() {
        fn array_json(elements: usize) -> Arc<[u8]> {
            let mut json = Vec::with_capacity(elements.saturating_mul(2).saturating_add(8));
            json.extend_from_slice(b"{\"a\":[");
            for element in 0..elements {
                if element != 0 {
                    json.push(b',');
                }
                json.push(b'0');
            }
            json.extend_from_slice(b"]}");
            Arc::from(json)
        }
        fn depth_json(depth: usize) -> Arc<[u8]> {
            let mut json = Vec::with_capacity(depth.saturating_mul(2).saturating_add(1));
            json.extend(std::iter::repeat_n(b'[', depth));
            json.push(b'0');
            json.extend(std::iter::repeat_n(b']', depth));
            Arc::from(json)
        }
        let (_workspace, root) = root();
        let (identity, _) = fixture_event();
        let mut journal = open_fixture(&root, identity);
        journal.identity.genesis = JournalGenesis::V2Shared(array_json(1_048_575));
        journal.identity.verified_v2 = None;
        assert!(
            journal
                .index_reader(root.limits().max_index_working_bytes)
                .is_ok()
        );
        journal.identity.genesis = JournalGenesis::V2Shared(array_json(1_048_576));
        assert!(matches!(
            journal.index_reader(root.limits().max_index_working_bytes),
            Err(JournalError::Incomplete {
                limit: 1_048_576,
                observed: 1_048_577,
            })
        ));
        journal.identity.genesis = JournalGenesis::V2Shared(depth_json(128));
        assert!(
            journal
                .index_reader(root.limits().max_index_working_bytes)
                .is_ok()
        );
        journal.identity.genesis = JournalGenesis::V2Shared(depth_json(129));
        assert!(matches!(
            journal.index_reader(root.limits().max_index_working_bytes),
            Err(JournalError::Incomplete {
                limit: 128,
                observed: 129,
            })
        ));
    }

    fn d1_fixture() -> (JournalIdentity, Vec<EventEnvelope>) {
        let run_id = StableId::parse("run:journal-d1").unwrap();
        let source_bytes = BTreeMap::from([
            (
                "src/checkout_controller.rs",
                b"controller line\n".repeat(100),
            ),
            (
                "src/payment_repository.rs",
                b"repository line\n".repeat(100),
            ),
        ]);
        let mut value: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let mut contains = value["relations"].as_array().unwrap()[0].clone();
        contains["id"] = Value::String("relation:file-contains-payment-charge".to_owned());
        contains["kind"] = Value::String("contains".to_owned());
        contains["source_id"] = Value::String("file:payment-repository".to_owned());
        contains["target_ids"] = serde_json::json!(["function:payment-charge"]);
        contains["directed"] = Value::Bool(true);
        value["relations"].as_array_mut().unwrap().push(contains);
        let test = value["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["id"] == "test:double-submit")
            .unwrap();
        test["location"]["start_line"] = Value::Null;
        test["location"]["end_line"] = Value::Null;
        for artifact in value["artifacts"].as_array_mut().unwrap() {
            if artifact["kind"] == "file" {
                let path = artifact["location"]["path"].as_str().unwrap();
                artifact["content_hash"] =
                    Value::String(ContentHash::sha256(&source_bytes[path]).to_string());
            }
        }
        let program: ProgramSpace = serde_json::from_value(value).unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let aggregate = ReviewAggregate::new(program.clone(), universe, obligations).unwrap();
        let mut log = EventLog::new(run_id.clone(), aggregate).unwrap();
        let genesis = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();

        let mut entries = Vec::new();
        let mut bytes_by_id = BTreeMap::new();
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
                adapter_id: "journal-d1-fixture@1".to_owned(),
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
                run_id.clone(),
                registration_id.clone(),
                hash.clone(),
                media_type,
                u64::try_from(bytes.len()).unwrap(),
                ArtifactSensitivity::WorkspaceSource,
                source,
            )
            .unwrap();
            log.append(EventCommand::artifact_registered(registration))
                .unwrap();
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
            bytes_by_id.insert(artifact.id.clone(), bytes);
        }
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        log.append(EventCommand::snapshot_sources_recorded(
            SnapshotSourcesRecorded::new(program.snapshot_id().clone(), entries).unwrap(),
        ))
        .unwrap();
        let review_plan = plan(log.aggregate(), PlanBudget::new(16, 2).unwrap()).unwrap();
        log.append(EventCommand::review_plan_recorded(review_plan))
            .unwrap();
        let obligation = log.aggregate().obligations().next().unwrap().id().clone();
        let mut session = prepare_context(log.aggregate(), obligation).unwrap();
        while let Some(request) = session.next_source_request().unwrap() {
            session
                .submit_source(&request, &bytes_by_id[request.artifact_id()])
                .unwrap();
        }
        log.append(EventCommand::context_envelope_projected(
            session.finish().unwrap(),
        ))
        .unwrap();
        (
            JournalIdentity::new(run_id, JournalGenesis::V2(genesis)).unwrap(),
            log.envelopes().cloned().collect(),
        )
    }

    /// All V2 test streams begin with the same durable, sequence-one
    /// manifest as production.  The fixture event is deliberately sequence
    /// two, so append and recovery tests never exercise an empty V2 log.
    fn first_manifest(identity: &JournalIdentity) -> EventEnvelope {
        let JournalGenesis::V2Shared(bytes) = &identity.genesis else {
            panic!("journal test fixture is V2");
        };
        let aggregate = reviewgraphen_core::RunGenesisSnapshot::from_canonical_bytes(bytes)
            .unwrap()
            .rebuild_aggregate()
            .unwrap();
        EventLog::new(identity.run_id.clone(), aggregate)
            .unwrap()
            .envelopes()
            .next()
            .unwrap()
            .clone()
    }

    fn open_fixture<'a>(root: &'a StoreRoot, identity: JournalIdentity) -> EventJournal<'a> {
        let genesis = first_manifest(&identity);
        EventJournal::initialize_v2(root, identity, genesis).unwrap()
    }

    fn three_transition_events(identity: &JournalIdentity) -> Vec<EventEnvelope> {
        let JournalGenesis::V2Shared(bytes) = &identity.genesis else {
            panic!("transition fixture requires V2 genesis");
        };
        let aggregate = reviewgraphen_core::RunGenesisSnapshot::from_canonical_bytes(bytes)
            .unwrap()
            .rebuild_aggregate()
            .unwrap();
        let mut log = EventLog::new(identity.run_id.clone(), aggregate).unwrap();
        let obligations = log
            .aggregate()
            .obligations()
            .take(3)
            .map(|obligation| obligation.id().clone())
            .collect::<Vec<_>>();
        assert_eq!(obligations.len(), 3);
        for obligation in obligations {
            log.append(EventCommand::obligation_transition(
                obligation,
                ObligationLifecycle::Planned,
            ))
            .unwrap();
        }
        log.envelopes().skip(1).cloned().collect()
    }

    #[test]
    fn verification_bundle_marker_records_exact_durable_stages_and_blocks_normal_reopen() {
        for (fault, expected) in [
            (AppendFault::BundleAfterDurableLine1, 1_u64),
            (AppendFault::BundleAfterDurableLine2, 2_u64),
        ] {
            let (_workspace, root) = root();
            let (identity, _) = fixture_event();
            let events = three_transition_events(&identity);
            let journal = open_fixture(&root, identity);
            let mut writer = journal.writer().unwrap();
            writer.inject_faults([fault]);
            let result = writer.append_verification_bundle_suffix(&events);
            assert!(matches!(
                result,
                Err(JournalError::BundleAppendInterrupted { durable_stage })
                    if durable_stage.confirmed_events() == expected
                        && durable_stage.expected_events() == 3
            ));
            assert_eq!(read_bundle_stage(&writer.run, 3).unwrap(), expected);
            assert_eq!(
                writer.events().len(),
                usize::try_from(expected + 1).unwrap()
            );
            drop(writer);
            assert!(matches!(
                journal.reader(),
                Err(JournalError::SessionResumeRequired)
            ));
            assert!(matches!(
                journal.recover("test", "bundle-stage"),
                Err(JournalError::BundleAppendInterrupted { durable_stage })
                    if durable_stage.confirmed_events() == expected
            ));
        }
    }

    #[test]
    fn verification_bundle_pre_sync_rolls_back_and_post_sync_requires_recovery() {
        let (_workspace, pre_root) = root();
        let (identity, _) = fixture_event();
        let events = three_transition_events(&identity);
        let journal = open_fixture(&pre_root, identity);
        let mut writer = journal.writer().unwrap();
        writer.inject_faults([AppendFault::PartialWrite]);
        assert!(matches!(
            writer.append_verification_bundle_suffix(&events),
            Err(JournalError::Io(_))
        ));
        assert_eq!(writer.events().len(), 1);
        assert!(
            read_bundle_pending(&writer.run, &writer.identity, writer.limits)
                .unwrap()
                .is_none()
        );
        drop(writer);
        assert_eq!(journal.reader().unwrap().events().len(), 1);

        let (_workspace, post_root) = root();
        let (identity, _) = fixture_event();
        let events = three_transition_events(&identity);
        let journal = open_fixture(&post_root, identity);
        let mut writer = journal.writer().unwrap();
        writer.inject_faults([AppendFault::ClearMarkerDirectorySync]);
        assert!(writer.append_verification_bundle_suffix(&events).is_err());
        assert_eq!(writer.append_durability, AppendDurability::Uncertain);
        assert_eq!(read_bundle_stage(&writer.run, 3).unwrap(), 3);
        drop(writer);
        journal.recover("test", "bundle-post-sync").unwrap();
        assert_eq!(journal.reader().unwrap().events().len(), 4);
    }

    #[test]
    fn verification_bundle_stage_sync_uncertainty_never_exposes_state() {
        let (_workspace, staged_root) = root();
        let (identity, _) = fixture_event();
        let events = three_transition_events(&identity);
        let journal = open_fixture(&staged_root, identity);
        let mut writer = journal.writer().unwrap();
        writer.inject_faults([AppendFault::BundleStageSync]);
        assert!(writer.append_verification_bundle_suffix(&events).is_err());
        assert_eq!(writer.append_durability, AppendDurability::Uncertain);
        drop(writer);
        assert!(journal.reader().is_err());
    }

    #[test]
    fn bundle_torn_recovery_publishes_exact_intent_before_mutation_and_is_idempotent() {
        for fault in [
            RecoveryFault::BundleAfterIntent,
            RecoveryFault::BundleAfterTruncateSync,
        ] {
            let (_workspace, root) = root();
            let (identity, _) = fixture_event();
            let events = three_transition_events(&identity);
            let journal = open_fixture(&root, identity);
            let mut writer = journal.writer().unwrap();
            writer.inject_faults([AppendFault::BundleAfterDurableLine1]);
            assert!(matches!(
                writer.append_verification_bundle_suffix(&events),
                Err(JournalError::BundleAppendInterrupted { .. })
            ));
            let good_offset = writer.state.confirmed_offset;
            drop(writer);

            let torn = &events[1].canonical_bytes().unwrap()[..11];
            let mut log = std::fs::OpenOptions::new()
                .append(true)
                .open(log_path(&root))
                .unwrap();
            log.write_all(torn).unwrap();
            log.sync_data().unwrap();
            drop(log);
            let pre_size = std::fs::metadata(log_path(&root)).unwrap().len();

            // Inspection computes the exact discarded range but is physically
            // pure; no truncate is permitted before a durable intent.
            let fd = journal.open_file(true).unwrap();
            fs::flock(&fd, FlockOperation::LockExclusive).unwrap();
            let mut locked = File::from(fd);
            let inspected = journal
                .inspect_bundle_pending_locked(&mut locked)
                .unwrap()
                .unwrap();
            assert_eq!(inspected.discarded, torn);
            assert_eq!(locked.metadata().unwrap().len(), pre_size);
            drop(locked);

            journal.inject_recovery_faults([fault]);
            assert!(matches!(
                journal.recover("test", "bundle-torn"),
                Err(JournalError::Io(_))
            ));
            let (intents, completions) = journal.recovery_dirs().unwrap();
            let audit =
                recovery_audit(&intents, &completions, &journal.identity, journal.limits).unwrap();
            let intent = audit.pending.unwrap();
            assert_eq!(intent.good_offset, good_offset);
            assert_eq!(intent.discarded_offset, Some(good_offset));
            assert_eq!(intent.discarded_len, Some(torn.len() as u64));
            assert_eq!(intent.pre_size, Some(pre_size));
            assert_eq!(intent.discarded_hash, ContentHash::sha256(torn));
            let physical = std::fs::metadata(log_path(&root)).unwrap().len();
            if fault == RecoveryFault::BundleAfterIntent {
                assert_eq!(physical, pre_size);
            } else {
                assert_eq!(physical, good_offset);
            }

            // The generic receipt recovery completes the exact same intent;
            // the next pass derives stage one from marker + journal.
            journal.recover("ignored", "ignored").unwrap();
            let (completed_intents, completed_completions) = journal.recovery_dirs().unwrap();
            let completed = recovery_audit(
                &completed_intents,
                &completed_completions,
                &journal.identity,
                journal.limits,
            )
            .unwrap();
            assert_eq!(completed.completed.len(), 1);
            assert_eq!(completed.receipt_files, 2);
            for _ in 0..2 {
                assert!(matches!(
                    journal.recover("test", "bundle-stage"),
                    Err(JournalError::BundleAppendInterrupted { durable_stage })
                        if durable_stage.confirmed_events() == 1
                ));
            }
            let (retried_intents, retried_completions) = journal.recovery_dirs().unwrap();
            let retried = recovery_audit(
                &retried_intents,
                &retried_completions,
                &journal.identity,
                journal.limits,
            )
            .unwrap();
            assert_eq!(retried.receipt_files, completed.receipt_files);
            assert_eq!(retried.receipt_scan_bytes, completed.receipt_scan_bytes);
            assert_eq!(retried.completed, completed.completed);
            assert!(matches!(
                journal.reader(),
                Err(JournalError::SessionResumeRequired)
            ));
        }
    }

    #[test]
    fn advisory_bundle_stage_is_rebuilt_and_stage_zero_remnant_is_audited() {
        let (_workspace, staged_root) = root();
        let (identity, _) = fixture_event();
        let events = three_transition_events(&identity);
        let journal = open_fixture(&staged_root, identity);
        let mut writer = journal.writer().unwrap();
        writer.inject_faults([AppendFault::BundleAfterDurableLine1]);
        assert!(writer.append_verification_bundle_suffix(&events).is_err());
        drop(writer);
        std::fs::write(
            journal_dir(&staged_root).join(BUNDLE_PENDING_STAGE),
            b"torn",
        )
        .unwrap();
        assert!(matches!(
            journal.recover("test", "advisory-stage"),
            Err(JournalError::BundleAppendInterrupted { durable_stage })
                if durable_stage.confirmed_events() == 1
        ));
        assert_eq!(read_bundle_stage(&journal.run, 3).unwrap(), 1);

        let (_workspace, clean_root) = root();
        let (identity, _) = fixture_event();
        let clean = open_fixture(&clean_root, identity);
        publish_bundle_file(&clean.run, BUNDLE_PENDING_STAGE, b"0\n").unwrap();
        assert!(matches!(
            clean.reader(),
            Err(JournalError::SessionResumeRequired)
        ));
        clean.recover("test", "stage-remnant").unwrap();
        assert!(!bundle_file_exists(&clean.run, BUNDLE_PENDING_STAGE).unwrap());
        assert_eq!(clean.reader().unwrap().events().len(), 1);
    }

    #[test]
    fn bundle_cleanup_retries_reuse_one_deterministic_receipt_pair() {
        for cleanup_case in ["stage-only", "confirmed-zero", "already-complete"] {
            for fault in [
                RecoveryFault::BundleAfterIntent,
                RecoveryFault::BundleAfterCompletion,
                RecoveryFault::BundleAfterMarkerUnlink,
            ] {
                let (_workspace, root) = root();
                let (identity, _) = fixture_event();
                let JournalGenesis::V2Shared(genesis) = &identity.genesis else {
                    unreachable!()
                };
                put_test_cas(&root, genesis);
                let events = three_transition_events(&identity);
                let journal = open_fixture(&root, identity);

                match cleanup_case {
                    "stage-only" => {
                        publish_bundle_file(&journal.run, BUNDLE_PENDING_STAGE, b"0\n").unwrap();
                        fs::fsync(&journal.run).unwrap();
                    }
                    "confirmed-zero" => {
                        let mut writer = journal.writer().unwrap();
                        let marker = VerificationBundlePendingMarkerV3::new(
                            &writer.identity,
                            &writer.state,
                            &events,
                        )
                        .unwrap();
                        writer.publish_bundle_pending(&marker).unwrap();
                    }
                    "already-complete" => {
                        let mut writer = journal.writer().unwrap();
                        writer.inject_faults([AppendFault::ClearMarkerDirectorySync]);
                        assert!(writer.append_verification_bundle_suffix(&events).is_err());
                    }
                    _ => unreachable!(),
                }

                if fault == RecoveryFault::BundleAfterIntent {
                    // Cross both create-only receipt boundaries separately so
                    // the final retry begins with one completed pair.
                    journal.inject_recovery_faults([
                        RecoveryFault::BundleAfterIntent,
                        RecoveryFault::BundleAfterCompletion,
                    ]);
                    assert!(matches!(
                        journal.recover("ignored", "ignored"),
                        Err(JournalError::Io(_))
                    ));
                    assert!(matches!(
                        journal.recover("ignored-again", "ignored-again"),
                        Err(JournalError::Io(_))
                    ));
                } else {
                    journal.inject_recovery_faults([fault]);
                    assert!(matches!(
                        journal.recover("ignored", "ignored"),
                        Err(JournalError::Io(_))
                    ));
                }

                let (before_intents, before_completions) = journal.recovery_dirs().unwrap();
                let before = recovery_audit(
                    &before_intents,
                    &before_completions,
                    &journal.identity,
                    journal.limits,
                )
                .unwrap();
                assert!(before.pending.is_none());
                assert_eq!(before.completed.len(), 1);
                assert_eq!(before.receipt_files, 2);

                let recovered = journal
                    .recover("different-actor", "different-tool")
                    .unwrap();
                assert!(recovered.resumed);
                let (after_intents, after_completions) = journal.recovery_dirs().unwrap();
                let after = recovery_audit(
                    &after_intents,
                    &after_completions,
                    &journal.identity,
                    journal.limits,
                )
                .unwrap();
                assert_eq!(after.receipt_files, before.receipt_files);
                assert_eq!(after.receipt_scan_bytes, before.receipt_scan_bytes);
                assert_eq!(after.completed, before.completed);

                let expected_events = if cleanup_case == "already-complete" {
                    4
                } else {
                    1
                };
                assert_eq!(journal.reader().unwrap().events().len(), expected_events);
                let cas = super::super::CasStore::open(&root).unwrap();
                let index = super::super::DerivedIndex::open(&root).unwrap();
                let rebuilt = index.rebuild(&journal, &cas).unwrap();
                assert_eq!(rebuilt.event_count, expected_events as u64);
                assert_eq!(
                    index.snapshot_current(&journal).unwrap().marker.event_count,
                    expected_events as u64
                );
            }
        }
    }

    #[test]
    fn foreign_bundle_receipts_are_rejected_before_any_recovery_mutation() {
        fn inventory(path: &std::path::Path) -> Vec<(String, Vec<u8>)> {
            let mut entries = std::fs::read_dir(path)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    (
                        entry.file_name().to_string_lossy().into_owned(),
                        std::fs::read(entry.path()).unwrap(),
                    )
                })
                .collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            entries
        }

        for completed in [false, true] {
            let mutations: &[&str] = if completed {
                &[
                    "nonce",
                    "digest",
                    "range",
                    "hash",
                    "actor",
                    "tool",
                    "timestamp",
                ]
            } else {
                &[
                    "nonce",
                    "kind",
                    "digest",
                    "range",
                    "hash",
                    "actor",
                    "tool",
                    "timestamp",
                ]
            };
            for mutation in mutations {
                let (_workspace, root) = root();
                let (identity, _) = fixture_event();
                let events = three_transition_events(&identity);
                let journal = open_fixture(&root, identity);
                let mut writer = journal.writer().unwrap();
                let marker = VerificationBundlePendingMarkerV3::new(
                    &writer.identity,
                    &writer.state,
                    &events,
                )
                .unwrap();
                let good_offset = writer.state.confirmed_offset;
                let pre_tail_hash = writer.state.tail_hash.clone();
                writer.publish_bundle_pending(&marker).unwrap();
                drop(writer);

                let discarded = b"{";
                let mut log = std::fs::OpenOptions::new()
                    .append(true)
                    .open(log_path(&root))
                    .unwrap();
                log.write_all(discarded).unwrap();
                log.sync_data().unwrap();
                drop(log);
                let physical_size = std::fs::metadata(log_path(&root)).unwrap().len();

                let mut bundle_digest = marker.bundle_digest.clone();
                let mut kind = "torn";
                let mut discarded_hash = ContentHash::sha256(discarded);
                let mut discarded_len = 1_u64;
                let mut discarded_offset = Some(good_offset);
                let mut pre_size = Some(physical_size);
                if *mutation == "kind" {
                    kind = "confirmed-zero-cleanup";
                    discarded_hash = ContentHash::sha256(b"");
                    discarded_len = 0;
                    discarded_offset = None;
                    pre_size = None;
                } else if *mutation == "digest" {
                    bundle_digest = ContentHash::sha256(b"foreign bundle");
                } else if *mutation == "range" {
                    discarded_len = 2;
                    pre_size = good_offset.checked_add(discarded_len);
                } else if *mutation == "hash" {
                    discarded_hash = ContentHash::sha256(b"foreign discarded bytes");
                }
                let mut nonce = bundle_recovery_nonce(
                    &journal.identity,
                    &bundle_digest,
                    marker.pre_offset,
                    good_offset,
                    &discarded_hash,
                    discarded_len,
                    kind,
                )
                .unwrap();
                if *mutation == "nonce" {
                    nonce = "a".repeat(64);
                }
                let recovery_id = recovery_id(
                    &journal.identity.run_id,
                    &journal.identity.genesis_hash(),
                    good_offset,
                    &discarded_hash,
                    &nonce,
                )
                .unwrap();
                let intent = RecoveryIntent {
                    recovery_id: recovery_id.clone(),
                    run_id: journal.identity.run_id.clone(),
                    genesis_hash: journal.identity.genesis_hash(),
                    nonce,
                    good_offset,
                    discarded_hash,
                    discarded_offset,
                    discarded_len: (discarded_len != 0).then_some(discarded_len),
                    pre_size,
                    bundle_digest: Some(bundle_digest),
                    bundle_pre_offset: Some(marker.pre_offset),
                    bundle_recovery_kind: Some(kind.to_owned()),
                    pre_tail_hash,
                    actor: if *mutation == "actor" {
                        "foreign-actor".to_owned()
                    } else {
                        "reviewgraphen-store".to_owned()
                    },
                    tool_version: if *mutation == "tool" {
                        "bundle-recovery:foreign".to_owned()
                    } else {
                        format!("bundle-recovery:{kind}")
                    },
                    timestamp_unix_seconds: u64::from(*mutation == "timestamp"),
                };
                let completion = RecoveryCompletion {
                    recovery_id: recovery_id.clone(),
                    post_file_hash: ContentHash::sha256(
                        &std::fs::read(log_path(&root)).unwrap()[..good_offset as usize],
                    ),
                };
                let (intents, completions) = journal.recovery_dirs().unwrap();
                publish_receipt(&intents, &receipt_name(&recovery_id), &intent).unwrap();
                if completed {
                    publish_receipt(&completions, &receipt_name(&recovery_id), &completion)
                        .unwrap();
                }
                drop(intents);
                drop(completions);

                let run = journal_dir(&root);
                let intents_path = run.join(RECOVERY_DIR).join(INTENTS_DIR);
                let completions_path = run.join(RECOVERY_DIR).join(COMPLETIONS_DIR);
                let before_intents = inventory(&intents_path);
                let before_completions = inventory(&completions_path);
                let before_log = std::fs::read(log_path(&root)).unwrap();
                let before_marker = std::fs::read(run.join(BUNDLE_PENDING_MARKER)).unwrap();
                let before_stage = std::fs::read(run.join(BUNDLE_PENDING_STAGE)).unwrap();

                assert!(matches!(
                    journal.recover("operator", "foreign-receipt-test"),
                    Err(JournalError::ReceiptCorruption { .. })
                ));
                assert_eq!(inventory(&intents_path), before_intents);
                assert_eq!(inventory(&completions_path), before_completions);
                assert_eq!(std::fs::read(log_path(&root)).unwrap(), before_log);
                assert_eq!(
                    std::fs::read(run.join(BUNDLE_PENDING_MARKER)).unwrap(),
                    before_marker
                );
                assert_eq!(
                    std::fs::read(run.join(BUNDLE_PENDING_STAGE)).unwrap(),
                    before_stage
                );
            }
        }
    }

    #[test]
    fn distinct_bundle_lineages_may_share_a_tail_and_remain_indexable() {
        let (_workspace, root) = root();
        let (identity, _) = fixture_event();
        let JournalGenesis::V2Shared(genesis) = &identity.genesis else {
            unreachable!()
        };
        put_test_cas(&root, genesis);
        let events = three_transition_events(&identity);
        let journal = open_fixture(&root, identity);

        let mut first_writer = journal.writer().unwrap();
        let first_marker = VerificationBundlePendingMarkerV3::new(
            &first_writer.identity,
            &first_writer.state,
            &events,
        )
        .unwrap();
        let shared_offset = first_writer.state.confirmed_offset;
        first_writer.publish_bundle_pending(&first_marker).unwrap();
        drop(first_writer);
        let first = journal.recover("ignored", "ignored").unwrap();

        let mut second_writer = journal.writer().unwrap();
        let second_marker = VerificationBundlePendingMarkerV3::new(
            &second_writer.identity,
            &second_writer.state,
            &events[..1],
        )
        .unwrap();
        assert_ne!(first_marker.bundle_digest, second_marker.bundle_digest);
        assert_eq!(second_writer.state.confirmed_offset, shared_offset);
        second_writer
            .publish_bundle_pending(&second_marker)
            .unwrap();
        drop(second_writer);
        let second = journal.recover("also-ignored", "also-ignored").unwrap();

        assert_eq!(first.intent.good_offset, shared_offset);
        assert_eq!(second.intent.good_offset, shared_offset);
        assert!(!same_bundle_recovery_lineage(&first.intent, &second.intent));
        assert_ne!(first.intent.recovery_id, second.intent.recovery_id);
        let (intents, completions) = journal.recovery_dirs().unwrap();
        let audit =
            recovery_audit(&intents, &completions, &journal.identity, journal.limits).unwrap();
        assert!(audit.pending.is_none());
        assert_eq!(audit.completed.len(), 2);
        assert_eq!(audit.receipt_files, 4);
        assert_eq!(journal.reader().unwrap().events().len(), 1);

        let cas = super::super::CasStore::open(&root).unwrap();
        let index = super::super::DerivedIndex::open(&root).unwrap();
        let rebuilt = index.rebuild(&journal, &cas).unwrap();
        assert_eq!(rebuilt.event_count, 1);
        assert_eq!(
            index.snapshot_current(&journal).unwrap().marker.event_count,
            1
        );

        let genesis_bytes = journal
            .identity
            .v2_genesis_backing()
            .expect("V2 genesis backing");
        let original_identity = journal.identity.cloned_local_metadata_capacity().unwrap();
        let reader_identity = u64::try_from(
            journal
                .identity
                .local_metadata_capacity()
                .checked_add(journal.identity.shared_certificate_capacity())
                .unwrap(),
        )
        .unwrap();
        let canonical_scratch = journal.limits.max_event_line_bytes - 1;
        let fixed = u64::try_from(genesis_bytes.len())
            .unwrap()
            .checked_add(original_identity * 2)
            .and_then(|value| value.checked_add(reader_identity))
            .and_then(|value| value.checked_add(journal.limits.max_event_line_bytes))
            .and_then(|value| value.checked_add(canonical_scratch))
            .unwrap();
        let run = journal_dir(&root);
        let receipt_sizes = [INTENTS_DIR, COMPLETIONS_DIR]
            .into_iter()
            .flat_map(|directory| {
                std::fs::read_dir(run.join(RECOVERY_DIR).join(directory))
                    .unwrap()
                    .map(|entry| entry.unwrap().metadata().unwrap().len())
            })
            .collect::<Vec<_>>();
        let receipt_bytes = receipt_sizes.iter().sum::<u64>();
        let max_receipt = *receipt_sizes.iter().max().unwrap();
        let names = 4_u64;
        let preflight = fixed
            .checked_add(receipt_bytes * 2)
            .and_then(|value| value.checked_add(max_receipt * 2))
            .and_then(|value| value.checked_add(2 * std::mem::size_of::<RecoveryIntent>() as u64))
            .and_then(|value| {
                value.checked_add(
                    2 * std::mem::size_of::<(RecoveryIntent, RecoveryCompletion)>() as u64,
                )
            })
            .and_then(|value| value.checked_add(names * std::mem::size_of::<String>() as u64))
            .and_then(|value| value.checked_add(names * 69))
            .unwrap();
        let generous = root.limits().max_index_working_bytes;
        let accounted_reader = journal.index_reader(generous).unwrap();
        let final_peak = accounted_reader
            .retained_metadata_capacity()
            .unwrap()
            .checked_add(original_identity * 2)
            .and_then(|value| value.checked_add(genesis_bytes.len() as u64))
            .and_then(|value| value.checked_add(journal.limits.max_event_line_bytes))
            .and_then(|value| value.checked_add(canonical_scratch))
            .unwrap();
        let exact = preflight.max(final_peak);
        drop(accounted_reader);
        let mut exact_reader = journal.index_reader(exact).unwrap();
        assert_eq!(
            exact_reader
                .with_locked_prefix::<()>(|_, _| Ok(()))
                .unwrap()
                .event_count,
            1
        );
        assert!(matches!(
            journal.index_reader(exact - 1),
            Err(JournalError::Incomplete { limit, .. }) if limit == exact - 1
        ));

        let mut retained_reader = journal.index_reader(generous).unwrap();
        let before = retained_reader.retained_metadata_capacity().unwrap();
        let intent = &mut retained_reader.completed_receipts[0].0;
        let old_digest = intent.bundle_digest.as_ref().unwrap().allocated_bytes();
        let old_kind = intent.bundle_recovery_kind.as_ref().unwrap().capacity();
        intent.bundle_digest =
            Some(ContentHash::parse(format!("sha256:{}", "a".repeat(128))).unwrap());
        intent.bundle_recovery_kind = Some("x".repeat(97));
        let expected_delta = intent.bundle_digest.as_ref().unwrap().allocated_bytes()
            + intent.bundle_recovery_kind.as_ref().unwrap().capacity()
            - old_digest
            - old_kind;
        assert_eq!(
            retained_reader.retained_metadata_capacity().unwrap() - before,
            expected_delta as u64
        );
    }

    #[test]
    fn mixed_legacy_and_bundle_lineages_refuse_both_directions_without_changes() {
        for completed in [false, true] {
            // An existing generic lineage must block a bundle cleanup at the
            // same tail before either receipt publication or gate cleanup.
            let (_workspace, first_root) = root();
            let (identity, _) = fixture_event();
            let events = three_transition_events(&identity);
            let journal = open_fixture(&first_root, identity);
            let mut writer = journal.writer().unwrap();
            let marker =
                VerificationBundlePendingMarkerV3::new(&writer.identity, &writer.state, &events)
                    .unwrap();
            let good = writer.state.confirmed_offset;
            let tail = writer.state.tail_hash.clone();
            writer.publish_bundle_pending(&marker).unwrap();
            drop(writer);
            let generic = intent_for(&journal, good, b"", tail, 1);
            let completion = RecoveryCompletion {
                recovery_id: generic.recovery_id.clone(),
                post_file_hash: ContentHash::sha256(&std::fs::read(log_path(&first_root)).unwrap()),
            };
            let (intents, completions) = journal.recovery_dirs().unwrap();
            publish_receipt(&intents, &receipt_name(&generic.recovery_id), &generic).unwrap();
            if completed {
                publish_receipt(
                    &completions,
                    &receipt_name(&completion.recovery_id),
                    &completion,
                )
                .unwrap();
            }
            drop(intents);
            drop(completions);
            let before_receipts = raw_recovery_inventory(&first_root);
            let before_log = std::fs::read(log_path(&first_root)).unwrap();
            let run = journal_dir(&first_root);
            let before_marker = std::fs::read(run.join(BUNDLE_PENDING_MARKER)).unwrap();
            let before_stage = std::fs::read(run.join(BUNDLE_PENDING_STAGE)).unwrap();
            assert!(matches!(
                journal.recover("operator", "mixed-lineage"),
                Err(JournalError::ReceiptCorruption { .. })
            ));
            assert_eq!(raw_recovery_inventory(&first_root), before_receipts);
            assert_eq!(std::fs::read(log_path(&first_root)).unwrap(), before_log);
            assert_eq!(
                std::fs::read(run.join(BUNDLE_PENDING_MARKER)).unwrap(),
                before_marker
            );
            assert_eq!(
                std::fs::read(run.join(BUNDLE_PENDING_STAGE)).unwrap(),
                before_stage
            );

            // An existing bundle lineage must likewise block generic torn
            // recovery at the same tail. Pending bundle recovery remains the
            // sole operation and completed bundle history conflicts with a
            // fresh legacy proposal.
            let (_workspace, reverse_root) = root();
            let (identity, _) = fixture_event();
            let reverse = open_fixture(&reverse_root, identity);
            publish_bundle_file(&reverse.run, BUNDLE_PENDING_STAGE, b"0\n").unwrap();
            if completed {
                reverse.recover("ignored", "ignored").unwrap();
            } else {
                reverse.inject_recovery_faults([RecoveryFault::BundleAfterIntent]);
                assert!(matches!(
                    reverse.recover("ignored", "ignored"),
                    Err(JournalError::Io(_))
                ));
            }
            let mut log = std::fs::OpenOptions::new()
                .append(true)
                .open(log_path(&reverse_root))
                .unwrap();
            log.write_all(b"{").unwrap();
            log.sync_data().unwrap();
            drop(log);
            let before_receipts = raw_recovery_inventory(&reverse_root);
            let before_log = std::fs::read(log_path(&reverse_root)).unwrap();
            let stage_before =
                std::fs::read(journal_dir(&reverse_root).join(BUNDLE_PENDING_STAGE)).ok();
            assert!(matches!(
                reverse.recover("operator", "mixed-lineage"),
                Err(JournalError::ReceiptCorruption { .. })
            ));
            assert_eq!(raw_recovery_inventory(&reverse_root), before_receipts);
            assert_eq!(std::fs::read(log_path(&reverse_root)).unwrap(), before_log);
            assert_eq!(
                std::fs::read(journal_dir(&reverse_root).join(BUNDLE_PENDING_STAGE)).ok(),
                stage_before
            );
        }
    }

    #[test]
    fn verification_bundle_resume_appends_only_the_exact_missing_suffix_once() {
        let (_workspace, root) = root();
        let (identity, _) = fixture_event();
        let events = three_transition_events(&identity);
        let journal = open_fixture(&root, identity);
        let mut writer = journal.writer().unwrap();
        writer.inject_faults([AppendFault::BundleAfterDurableLine1]);
        assert!(matches!(
            writer.append_verification_bundle_suffix(&events),
            Err(JournalError::BundleAppendInterrupted { .. })
        ));
        let marker = read_bundle_pending(&writer.run, &writer.identity, writer.limits)
            .unwrap()
            .unwrap();
        let before = writer.events().len();
        assert!(matches!(
            writer.resume_verification_bundle_suffix(&marker, &events[2..], 1),
            Err(JournalError::BundleResumeAuthorityMismatch)
        ));
        assert_eq!(writer.events().len(), before);

        let receipts = writer
            .resume_verification_bundle_suffix(&marker, &events[1..], 1)
            .unwrap();
        assert_eq!(receipts.len(), 2);
        assert_eq!(writer.events().len(), 4);
        assert!(
            read_bundle_pending(&writer.run, &writer.identity, writer.limits)
                .unwrap()
                .is_none()
        );
        assert!(
            writer
                .resume_verification_bundle_suffix(&marker, &events[1..], 1)
                .is_err()
        );
        assert_eq!(writer.events().len(), 4);
    }

    #[test]
    fn repeated_partial_recovery_keeps_receipts_stable_and_resumed_index_current() {
        let (_workspace, root) = root();
        let (identity, _) = fixture_event();
        let JournalGenesis::V2Shared(genesis) = &identity.genesis else {
            unreachable!()
        };
        put_test_cas(&root, genesis);
        let events = three_transition_events(&identity);
        let journal = open_fixture(&root, identity);
        let mut writer = journal.writer().unwrap();
        writer.inject_faults([AppendFault::BundleAfterDurableLine1]);
        assert!(matches!(
            writer.append_verification_bundle_suffix(&events),
            Err(JournalError::BundleAppendInterrupted { .. })
        ));
        drop(writer);
        let (intents, completions) = journal.recovery_dirs().unwrap();
        let before =
            recovery_audit(&intents, &completions, &journal.identity, journal.limits).unwrap();
        for _ in 0..2 {
            assert!(matches!(
                journal.recover("test", "no-mutation-partial"),
                Err(JournalError::BundleAppendInterrupted { durable_stage })
                    if durable_stage.confirmed_events() == 1
            ));
        }
        let (after_intents, after_completions) = journal.recovery_dirs().unwrap();
        let after = recovery_audit(
            &after_intents,
            &after_completions,
            &journal.identity,
            journal.limits,
        )
        .unwrap();
        assert_eq!(after.receipt_files, before.receipt_files);
        assert_eq!(after.receipt_scan_bytes, before.receipt_scan_bytes);
        assert_eq!(after.completed.len(), before.completed.len());

        let mut writer = JournalWriter {
            file: {
                let fd = journal.open_file(true).unwrap();
                fs::flock(&fd, FlockOperation::LockExclusive).unwrap();
                File::from(fd)
            },
            identity: journal.identity.clone(),
            limits: journal.limits,
            state: scan_bytes(
                &std::fs::read(log_path(&root)).unwrap(),
                &journal.identity,
                journal.limits,
                true,
            )
            .unwrap(),
            intents,
            completions,
            run: dup(&journal.run).unwrap(),
            poisoned: false,
            append_durability: AppendDurability::Confirmed,
            faults: std::collections::VecDeque::new(),
        };
        let marker = read_bundle_pending(&writer.run, &writer.identity, writer.limits)
            .unwrap()
            .unwrap();
        writer
            .resume_verification_bundle_suffix(&marker, &events[1..], 1)
            .unwrap();
        drop(writer);

        let cas = super::super::CasStore::open(&root).unwrap();
        let index = super::super::DerivedIndex::open(&root).unwrap();
        let rebuilt = index.rebuild(&journal, &cas).unwrap();
        assert_eq!(rebuilt.event_count, 4);
        assert_eq!(
            index.snapshot_current(&journal).unwrap().marker.event_count,
            4
        );
    }

    fn root() -> (tempfile::TempDir, StoreRoot) {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), super::super::StoreLimits::default()).unwrap();
        (workspace, root)
    }

    fn mutate_active_v5_image(root: &StoreRoot, sql: &str) {
        let active = root.path().join("indexes").join("reviewgraphen.sqlite");
        let image = std::fs::read(&active).unwrap();
        let length = image.len();
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .deserialize_read_exact(rusqlite::MAIN_DB, Cursor::new(image), length, false)
            .unwrap();
        connection
            .pragma_update(None, "foreign_keys", false)
            .unwrap();
        connection.execute_batch(sql).unwrap();
        let bytes = connection.serialize(rusqlite::MAIN_DB).unwrap().to_vec();
        std::fs::write(&active, bytes).unwrap();
        std::fs::set_permissions(&active, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn journal_dir(root: &StoreRoot) -> std::path::PathBuf {
        root.path()
            .join(RUNS_DIR)
            .join(run_dir_name(&StableId::parse("run:journal-test").unwrap()))
    }

    fn raw_recovery_inventory(root: &StoreRoot) -> Vec<(String, Vec<u8>)> {
        let run = journal_dir(root).join(RECOVERY_DIR);
        let mut entries = [INTENTS_DIR, COMPLETIONS_DIR]
            .into_iter()
            .flat_map(|directory| {
                std::fs::read_dir(run.join(directory))
                    .unwrap()
                    .map(move |entry| {
                        let entry = entry.unwrap();
                        (
                            format!("{directory}/{}", entry.file_name().to_string_lossy()),
                            std::fs::read(entry.path()).unwrap(),
                        )
                    })
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        entries
    }

    fn create_empty_v2_layout(root: &StoreRoot, identity: &JournalIdentity) {
        let run = root
            .path()
            .join(RUNS_DIR)
            .join(run_dir_name(&identity.run_id));
        std::fs::create_dir_all(run.join(RECOVERY_DIR).join(INTENTS_DIR)).unwrap();
        std::fs::create_dir_all(run.join(RECOVERY_DIR).join(COMPLETIONS_DIR)).unwrap();
        std::fs::write(run.join(JOURNAL_FILE), b"").unwrap();
        for directory in [
            root.path().join(RUNS_DIR),
            run.clone(),
            run.join(RECOVERY_DIR),
            run.join(RECOVERY_DIR).join(INTENTS_DIR),
            run.join(RECOVERY_DIR).join(COMPLETIONS_DIR),
        ] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::fs::set_permissions(
            run.join(JOURNAL_FILE),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
    fn log_path(root: &StoreRoot) -> std::path::PathBuf {
        log_path_for(root, &StableId::parse("run:journal-test").unwrap())
    }
    fn log_path_for(root: &StoreRoot, run_id: &StableId) -> std::path::PathBuf {
        root.path()
            .join(RUNS_DIR)
            .join(run_dir_name(run_id))
            .join(JOURNAL_FILE)
    }
    fn recovery_path(root: &StoreRoot) -> std::path::PathBuf {
        journal_dir(root).join(RECOVERY_DIR)
    }

    fn event_line(event: &EventEnvelope) -> Vec<u8> {
        let mut line = event.canonical_bytes().unwrap();
        line.push(b'\n');
        line
    }

    fn initialized_bytes(identity: &JournalIdentity) -> Vec<u8> {
        event_line(&first_manifest(identity))
    }

    fn initialized_with_event(identity: &JournalIdentity, event: &EventEnvelope) -> Vec<u8> {
        let mut bytes = initialized_bytes(identity);
        bytes.extend(event_line(event));
        bytes
    }

    fn append_torn(
        journal: &EventJournal<'_>,
        event: &EventEnvelope,
        fragment: &[u8],
    ) -> JournalAppendReceipt {
        let mut writer = journal.writer().unwrap();
        let receipt = writer.append(event.clone()).unwrap();
        drop(writer);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(log_path(journal.root))
            .unwrap();
        file.write_all(fragment).unwrap();
        file.sync_data().unwrap();
        receipt
    }

    fn intent_for(
        journal: &EventJournal<'_>,
        good_offset: u64,
        discarded: &[u8],
        pre_tail_hash: ContentHash,
        timestamp_unix_seconds: u64,
    ) -> RecoveryIntent {
        let discarded_hash = ContentHash::sha256(discarded);
        let nonce = "a".repeat(64);
        RecoveryIntent {
            recovery_id: recovery_id(
                &journal.identity.run_id,
                &journal.identity.genesis_hash(),
                good_offset,
                &discarded_hash,
                &nonce,
            )
            .unwrap(),
            run_id: journal.identity.run_id.clone(),
            genesis_hash: journal.identity.genesis_hash(),
            nonce,
            good_offset,
            discarded_hash,
            discarded_offset: None,
            discarded_len: None,
            pre_size: None,
            bundle_digest: None,
            bundle_pre_offset: None,
            bundle_recovery_kind: None,
            pre_tail_hash,
            actor: "test".to_owned(),
            tool_version: "reviewgraphen-store@1".to_owned(),
            timestamp_unix_seconds,
        }
    }

    #[test]
    fn initialize_v2_publishes_only_a_nonempty_per_run_log() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let genesis = first_manifest(&identity);
        let journal =
            EventJournal::initialize_v2(&root, identity.clone(), genesis.clone()).unwrap();
        assert_eq!(
            std::fs::read(log_path(&root)).unwrap(),
            event_line(&genesis)
        );
        let reader = journal.reader().unwrap();
        assert_eq!(reader.events().len(), 1);
        assert_eq!(reader.events()[0].event_hash(), genesis.event_hash());
        drop(reader);
        journal.writer().unwrap().append(event).unwrap();
        // A retry after publication is safe and verifies the immutable first
        // record rather than treating a crash/retry as a different run.
        EventJournal::initialize_v2(&root, identity, genesis).unwrap();
    }

    #[test]
    fn empty_v2_log_is_rejected_before_reader_writer_or_recovery_can_use_it() {
        let (_workspace, root) = root();
        let (identity, _event) = fixture_event();
        create_empty_v2_layout(&root, &identity);
        let journal = EventJournal::open(&root, identity).unwrap();
        assert!(matches!(
            journal.reader(),
            Err(JournalError::V2GenesisRequired)
        ));
        assert!(matches!(
            journal.writer(),
            Err(JournalError::V2GenesisRequired)
        ));
        assert!(matches!(
            journal.recover("test", "reviewgraphen-store@1"),
            Err(JournalError::V2GenesisRequired)
        ));
    }

    #[test]
    fn distinct_runs_never_share_log_or_recovery_components() {
        let (_workspace, root) = root();
        let (first_identity, _first_event) = fixture_event_for("run:journal-a");
        let (second_identity, _second_event) = fixture_event_for("run:journal-b");
        let first_genesis = first_manifest(&first_identity);
        let second_genesis = first_manifest(&second_identity);
        EventJournal::initialize_v2(&root, first_identity.clone(), first_genesis.clone()).unwrap();
        EventJournal::initialize_v2(&root, second_identity.clone(), second_genesis.clone())
            .unwrap();
        let first = root
            .path()
            .join(RUNS_DIR)
            .join(run_dir_name(&first_identity.run_id));
        let second = root
            .path()
            .join(RUNS_DIR)
            .join(run_dir_name(&second_identity.run_id));
        assert_eq!(
            std::fs::read(first.join(JOURNAL_FILE)).unwrap(),
            event_line(&first_genesis)
        );
        assert_eq!(
            std::fs::read(second.join(JOURNAL_FILE)).unwrap(),
            event_line(&second_genesis)
        );
        assert!(first.join(RECOVERY_DIR).join(INTENTS_DIR).is_dir());
        assert!(second.join(RECOVERY_DIR).join(COMPLETIONS_DIR).is_dir());
    }

    #[test]
    fn run_directory_is_a_digest_not_a_stable_id_path_component() {
        let (_workspace, root) = root();
        let (identity, _event) = fixture_event_for("run:traverse/../other");
        let genesis = first_manifest(&identity);
        EventJournal::initialize_v2(&root, identity.clone(), genesis).unwrap();
        let runs = root.path().join(RUNS_DIR);
        let digest = run_dir_name(&identity.run_id);
        assert!(runs.join(&digest).join(JOURNAL_FILE).is_file());
        assert!(!runs.join("traverse").exists());
        assert_eq!(digest.len(), 64);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn explicit_recover_clears_a_durable_marker_after_a_complete_append() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        journal.writer().unwrap().append(event).unwrap();
        let marker = journal_dir(&root).join(APPEND_PENDING_MARKER);
        std::fs::write(&marker, APPEND_PENDING_BYTES).unwrap();
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            journal.reader(),
            Err(JournalError::CorruptNeedsRecovery { .. })
        ));
        let receipt = journal.recover("test", "reviewgraphen-store@1").unwrap();
        assert_eq!(receipt.intent.discarded_hash, ContentHash::sha256(b""));
        assert!(!journal_dir(&root).join(APPEND_PENDING_MARKER).exists());
        assert_eq!(journal.reader().unwrap().events().len(), 2);
    }

    #[test]
    fn marker_recovery_reserves_both_receipts_before_publishing_either() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity.clone());
        journal.writer().unwrap().append(event).unwrap();
        let marker = journal_dir(&root).join(APPEND_PENDING_MARKER);
        std::fs::write(&marker, APPEND_PENDING_BYTES).unwrap();
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o600)).unwrap();
        let limited = EventJournal::open_with_limits(
            &root,
            identity,
            JournalLimits {
                max_receipt_files: 1,
                ..JournalLimits::from_store(root.limits())
            },
        )
        .unwrap();
        assert!(matches!(
            limited.recover("test", "reviewgraphen-store@1"),
            Err(JournalError::Incomplete {
                limit: 1,
                observed: 2
            })
        ));
        assert!(marker.exists());
        assert!(
            std::fs::read_dir(recovery_path(&root).join(INTENTS_DIR))
                .unwrap()
                .next()
                .is_none()
        );
        assert!(
            std::fs::read_dir(recovery_path(&root).join(COMPLETIONS_DIR))
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn marker_recovery_log_sync_failure_preserves_the_marker_for_retry() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        journal.writer().unwrap().append(event).unwrap();
        let marker = journal_dir(&root).join(APPEND_PENDING_MARKER);
        std::fs::write(&marker, APPEND_PENDING_BYTES).unwrap();
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o600)).unwrap();
        journal.inject_recovery_faults([RecoveryFault::MarkerLogSync]);
        assert!(matches!(
            journal.recover("test", "reviewgraphen-store@1"),
            Err(JournalError::Io(_))
        ));
        assert!(marker.exists());
        assert!(journal.recover("test", "reviewgraphen-store@1").is_ok());
    }

    #[test]
    fn marker_clear_sync_failure_restores_the_gate_and_writer_is_poisoned() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let mut writer = journal.writer().unwrap();
        writer.inject_faults([AppendFault::ClearMarkerDirectorySync]);
        assert!(matches!(writer.append(event), Err(JournalError::Io(_))));
        assert!(matches!(
            writer.append(fixture_event().1),
            Err(JournalError::Poisoned)
        ));
        drop(writer);
        assert!(journal_dir(&root).join(APPEND_PENDING_MARKER).exists());
        assert!(journal.recover("test", "reviewgraphen-store@1").is_ok());
    }

    #[test]
    fn append_rejects_an_under_sized_receipt_limit_before_the_marker_or_log_changes() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let genesis = first_manifest(&identity);
        let initial_limits = JournalLimits::from_store(root.limits());
        let journal = EventJournal::initialize_v2_with_limits(
            &root,
            identity.clone(),
            genesis,
            initial_limits,
        )
        .unwrap();
        let sizes = rollback_receipt_size_bound(&identity, 0, &chain_genesis(&identity)).unwrap();
        let limited = EventJournal::open_with_limits(
            &root,
            identity,
            JournalLimits {
                max_receipt_bytes: sizes[0] - 1,
                ..initial_limits
            },
        )
        .unwrap();
        let before = std::fs::read(log_path(&root)).unwrap();
        assert!(matches!(
            limited.writer().unwrap().append(event),
            Err(JournalError::Incomplete { .. })
        ));
        assert_eq!(std::fs::read(log_path(&root)).unwrap(), before);
        assert!(!journal_dir(&root).join(APPEND_PENDING_MARKER).exists());
        drop(journal);
    }

    #[test]
    fn a_later_chain_failure_reports_the_start_of_that_complete_line() {
        let (identity, _) = fixture_event();
        let genesis = first_manifest(&identity);
        let JournalGenesis::V2Shared(bytes) = &identity.genesis else {
            panic!("fixture is V2")
        };
        let aggregate = reviewgraphen_core::RunGenesisSnapshot::from_canonical_bytes(bytes)
            .unwrap()
            .rebuild_aggregate()
            .unwrap();
        let mut log = EventLog::new(identity.run_id.clone(), aggregate).unwrap();
        let obligations = log
            .aggregate()
            .obligations()
            .take(2)
            .map(|obligation| obligation.id().clone())
            .collect::<Vec<_>>();
        log.append(EventCommand::obligation_transition(
            obligations[0].clone(),
            ObligationLifecycle::Planned,
        ))
        .unwrap();
        log.append(EventCommand::obligation_transition(
            obligations[1].clone(),
            ObligationLifecycle::Planned,
        ))
        .unwrap();
        let broken = log.envelopes().nth(2).unwrap().clone();
        let mut bytes = event_line(&genesis);
        let expected = u64::try_from(bytes.len()).unwrap();
        bytes.extend(event_line(&broken));
        assert!(matches!(
            scan_bytes(&bytes, &identity, JournalLimits::from_store(super::super::StoreLimits::default()), true),
            Err(JournalError::CorruptNeedsRecovery {
                good_offset,
                auto_recoverable: false,
            }) if good_offset == expected
        ));
    }

    #[test]
    fn pending_recovery_rejects_a_total_log_limit_before_any_mutation() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity.clone());
        let receipt = append_torn(&journal, &event, b"{");
        let before = std::fs::read(log_path(&root)).unwrap();
        let intent = intent_for(
            &journal,
            receipt.tail_offset,
            b"{",
            event.event_hash().clone(),
            1,
        );
        let (intents, _completions) = journal.recovery_dirs().unwrap();
        publish_receipt(&intents, &receipt_name(&intent.recovery_id), &intent).unwrap();
        let limited = EventJournal::open_with_limits(
            &root,
            identity,
            JournalLimits {
                max_replay_bytes: receipt.tail_offset,
                ..JournalLimits::from_store(root.limits())
            },
        )
        .unwrap();
        assert!(matches!(
            limited.recover("test", "reviewgraphen-store@1"),
            Err(JournalError::Incomplete {
                limit,
                observed,
            }) if limit == receipt.tail_offset && observed == receipt.tail_offset + 1
        ));
        assert_eq!(std::fs::read(log_path(&root)).unwrap(), before);
    }

    #[test]
    fn v2_writer_initializes_only_with_the_manifest_and_reader_holds_valid_prefix() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let mut writer = journal.writer().unwrap();
        let receipt = writer.append(event).unwrap();
        assert_eq!(receipt.sequence, 2);
        drop(writer);
        let reader = journal.reader().unwrap();
        assert_eq!(reader.events().len(), 2);
        assert_eq!(reader.confirmed_offset(), receipt.tail_offset);
    }

    #[test]
    fn replayed_v2_session_appends_one_validated_command_and_retains_writer_lock() {
        let (_workspace, root) = root();
        let (identity, _event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let mut session = journal
            .replayed_v2_session(&EventAdmissions::default())
            .unwrap();
        let obligation = session
            .aggregate()
            .unwrap()
            .obligations()
            .next()
            .unwrap()
            .id()
            .clone();
        let receipt = session
            .append_command(EventCommand::obligation_transition(
                obligation,
                ObligationLifecycle::Planned,
            ))
            .unwrap();
        assert_eq!(receipt.sequence, 2);

        let blocked_reader = journal.open_file(false).unwrap();
        assert!(fs::flock(&blocked_reader, FlockOperation::NonBlockingLockShared).is_err());
        drop(blocked_reader);
        drop(session);
        assert_eq!(journal.reader().unwrap().events().len(), 2);
    }

    #[test]
    fn replayed_v2_session_never_advances_memory_after_durable_append_failure() {
        let (_workspace, root) = root();
        let (identity, _event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let mut session = journal
            .replayed_v2_session(&EventAdmissions::default())
            .unwrap();
        let obligation = session
            .aggregate()
            .unwrap()
            .obligations()
            .next()
            .unwrap()
            .id()
            .clone();
        let tail = session.tail_hash().unwrap().clone();
        session.writer.inject_faults([AppendFault::PartialWrite]);
        assert!(matches!(
            session.append_command(EventCommand::obligation_transition(
                obligation.clone(),
                ObligationLifecycle::Planned,
            )),
            Err(JournalError::Io(_))
        ));
        assert_eq!(session.tail_hash().unwrap(), &tail);
        assert_eq!(session.writer.events().len(), 1);
        assert_eq!(
            session
                .aggregate()
                .unwrap()
                .obligations()
                .find(|candidate| candidate.id() == &obligation)
                .unwrap()
                .lifecycle(),
            ObligationLifecycle::Generated
        );
        assert_eq!(
            session
                .append_command(EventCommand::obligation_transition(
                    obligation,
                    ObligationLifecycle::Planned,
                ))
                .unwrap()
                .sequence,
            2
        );
    }

    #[test]
    fn replayed_v2_session_refuses_reads_and_retries_after_post_sync_uncertainty() {
        let (_workspace, root) = root();
        let (identity, _event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let mut session = journal
            .replayed_v2_session(&EventAdmissions::default())
            .unwrap();
        let obligation = session
            .aggregate()
            .unwrap()
            .obligations()
            .next()
            .unwrap()
            .id()
            .clone();
        session
            .writer
            .inject_faults([AppendFault::ClearMarkerDirectorySync]);
        assert!(matches!(
            session.append_command(EventCommand::obligation_transition(
                obligation.clone(),
                ObligationLifecycle::Planned,
            )),
            Err(JournalError::SessionUncertain)
        ));
        assert!(matches!(
            session.aggregate(),
            Err(JournalError::SessionUncertain)
        ));
        assert!(matches!(
            session.tail_hash(),
            Err(JournalError::SessionUncertain)
        ));
        assert!(matches!(
            session.append_command(EventCommand::obligation_transition(
                obligation.clone(),
                ObligationLifecycle::Planned,
            )),
            Err(JournalError::SessionUncertain)
        ));
        drop(session);

        let recovered = journal.recover("test", "reviewgraphen-store@1").unwrap();
        assert_eq!(recovered.intent.discarded_hash, ContentHash::sha256(b""));
        assert_eq!(journal.reader().unwrap().events().len(), 2);
        let reopened = journal
            .replayed_v2_session(&EventAdmissions::default())
            .unwrap();
        assert_eq!(
            reopened
                .aggregate()
                .unwrap()
                .obligations()
                .find(|candidate| candidate.id() == &obligation)
                .unwrap()
                .lifecycle(),
            ObligationLifecycle::Planned
        );
    }

    #[test]
    fn replayed_v2_session_refuses_v1_and_invalid_or_stale_durable_prefixes() {
        let (_workspace, root) = root();
        let v1_identity = JournalIdentity::new(
            StableId::parse("run:journal-v1-session").unwrap(),
            JournalGenesis::V1(ContentHash::sha256(b"v1 genesis")),
        )
        .unwrap();
        let v1 = EventJournal::initialize_v1_for_test(&root, v1_identity, &[]).unwrap();
        assert!(matches!(
            v1.replayed_v2_session(&EventAdmissions::default()),
            Err(JournalError::V1ReadOnly)
        ));

        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        journal.writer().unwrap().append(event.clone()).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(log_path(&root))
            .unwrap();
        file.write_all(&event_line(&event)).unwrap();
        file.sync_data().unwrap();
        assert!(matches!(
            journal.replayed_v2_session(&EventAdmissions::default()),
            Err(JournalError::CorruptNeedsRecovery {
                auto_recoverable: false,
                ..
            })
        ));
    }

    #[test]
    fn only_unterminated_final_fragment_is_receipted_and_truncated() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let mut writer = journal.writer().unwrap();
        let receipt = writer.append(event).unwrap();
        drop(writer);
        let path = log_path(&root);
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(b"{\"torn\"").unwrap();
        file.sync_data().unwrap();
        assert!(matches!(
            journal.writer(),
            Err(JournalError::CorruptNeedsRecovery {
                auto_recoverable: true,
                ..
            })
        ));
        let recovered = journal.recover("reviewgraphen-store@1", "test").unwrap();
        assert_eq!(recovered.intent.good_offset, receipt.tail_offset);
        assert_eq!(journal.reader().unwrap().events().len(), 2);
    }

    #[test]
    fn complete_invalid_tail_never_enters_auto_recovery() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let mut writer = journal.writer().unwrap();
        writer.append(event).unwrap();
        drop(writer);
        let path = log_path(&root);
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(b"{}\n").unwrap();
        file.sync_data().unwrap();
        assert!(matches!(
            journal.recover("reviewgraphen-store@1", "test"),
            Err(JournalError::CorruptNeedsRecovery {
                auto_recoverable: false,
                ..
            })
        ));
    }

    #[test]
    fn pending_intent_after_truncate_with_a_nonempty_discard_refuses() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let mut writer = journal.writer().unwrap();
        let receipt = writer.append(event).unwrap();
        drop(writer);
        let path = log_path(&root);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"{").unwrap();
        file.sync_data().unwrap();
        drop(file);
        let mut log = File::from(journal.open_file(true).unwrap());
        let state = scan_with_torn(&mut log, &journal.identity, journal.limits, false).unwrap();
        let torn = state.torn.unwrap();
        let discarded_hash = ContentHash::sha256(&torn.discarded);
        let nonce = "b".repeat(64);
        let recovery_id = recovery_id(
            &journal.identity.run_id,
            &journal.identity.genesis_hash(),
            torn.good_offset,
            &discarded_hash,
            &nonce,
        )
        .unwrap();
        let intent = RecoveryIntent {
            recovery_id: recovery_id.clone(),
            run_id: journal.identity.run_id.clone(),
            genesis_hash: journal.identity.genesis_hash(),
            nonce,
            good_offset: torn.good_offset,
            discarded_hash,
            discarded_offset: None,
            discarded_len: None,
            pre_size: None,
            bundle_digest: None,
            bundle_pre_offset: None,
            bundle_recovery_kind: None,
            pre_tail_hash: torn.pre_tail_hash,
            actor: "actor".to_owned(),
            tool_version: "test".to_owned(),
            timestamp_unix_seconds: 1,
        };
        let (intents, _completions) = journal.recovery_dirs().unwrap();
        publish_receipt(&intents, &receipt_name(&recovery_id), &intent).unwrap();
        log.set_len(receipt.tail_offset).unwrap();
        log.sync_data().unwrap();
        drop(log);
        // A completed truncate may have lost only its sync acknowledgement;
        // recovery validates the retained prefix and durably completes it.
        journal
            .recover("different-actor", "different-tool")
            .unwrap();
    }

    #[test]
    fn append_is_canonical_single_newline_and_reopens_after_durable_sync() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let mut writer = journal.writer().unwrap();
        let receipt = writer.append(event.clone()).unwrap();
        assert_eq!(
            std::fs::read(log_path(&root)).unwrap(),
            initialized_with_event(&journal.identity, &event)
        );
        drop(writer);
        let reader = journal.reader().unwrap();
        reader.with_locked_snapshot(|events, offset, _| {
            assert_eq!(events.len(), 2);
            assert_eq!(events[1].sequence(), event.sequence());
            assert_eq!(offset, receipt.tail_offset);
        });
    }

    #[test]
    fn append_precheck_leaves_existing_bytes_unchanged() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let mut writer = journal.writer().unwrap();
        writer.append(event.clone()).unwrap();
        let before = std::fs::read(log_path(&root)).unwrap();
        assert!(matches!(writer.append(event), Err(JournalError::Domain(_))));
        assert_eq!(std::fs::read(log_path(&root)).unwrap(), before);
    }

    #[test]
    fn append_faults_roll_back_and_the_writer_remains_reusable() {
        for fault in [
            AppendFault::PartialWrite,
            AppendFault::Flush,
            AppendFault::SyncData,
        ] {
            let (_workspace, root) = root();
            let (identity, event) = fixture_event();
            let journal = open_fixture(&root, identity);
            let mut writer = journal.writer().unwrap();
            writer.inject_faults([fault]);
            assert!(matches!(
                writer.append(event.clone()),
                Err(JournalError::Io(_))
            ));
            assert_eq!(
                std::fs::read(log_path(&root)).unwrap(),
                initialized_bytes(&writer.identity)
            );
            assert_eq!(writer.events().len(), 1);
            assert_eq!(writer.append(event).unwrap().sequence, 2);
        }
    }

    #[test]
    fn rollback_faults_poison_until_an_explicit_recovery() {
        for rollback_fault in [AppendFault::Truncate, AppendFault::RollbackSync] {
            let (_workspace, root) = root();
            let (identity, event) = fixture_event();
            let journal = open_fixture(&root, identity);
            let mut writer = journal.writer().unwrap();
            writer.inject_faults([AppendFault::PartialWrite, rollback_fault]);
            assert!(matches!(
                writer.append(event.clone()),
                Err(JournalError::Poisoned)
            ));
            assert!(matches!(
                writer.append(event.clone()),
                Err(JournalError::Poisoned)
            ));
            drop(writer);
            assert!(matches!(
                journal.writer(),
                Err(JournalError::CorruptNeedsRecovery {
                    auto_recoverable: true,
                    ..
                })
            ));
            let recovered = journal.recover("test", "reviewgraphen-store@1");
            assert!(recovered.is_ok());
        }
    }

    #[test]
    fn rollback_intent_publish_failure_leaves_a_pending_reopen_gate() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let mut writer = journal.writer().unwrap();
        writer.inject_faults([AppendFault::PartialWrite, AppendFault::IntentPublish]);
        assert!(matches!(writer.append(event), Err(JournalError::Poisoned)));
        drop(writer);
        assert!(matches!(
            journal.writer(),
            Err(JournalError::CorruptNeedsRecovery { .. })
        ));
        assert!(matches!(
            journal.reader(),
            Err(JournalError::CorruptNeedsRecovery { .. })
        ));
    }

    #[test]
    fn rollback_intent_failure_still_leaves_torn_bytes_that_refuse_reopen() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let mut writer = journal.writer().unwrap();
        writer.inject_faults([AppendFault::PartialWrite, AppendFault::IntentPublish]);
        assert!(matches!(writer.append(event), Err(JournalError::Poisoned)));
        drop(writer);
        assert!(matches!(
            journal.writer(),
            Err(JournalError::CorruptNeedsRecovery { .. })
        ));
    }

    #[test]
    fn append_bounds_are_exact_and_plus_one_refuses_without_partial_bytes() {
        let (_workspace, primary_root) = root();
        let (identity, event) = fixture_event();
        let genesis = first_manifest(&identity);
        let genesis_len = u64::try_from(event_line(&genesis).len()).unwrap();
        let line_len = u64::try_from(event_line(&event).len()).unwrap();
        let exact = JournalLimits {
            max_event_line_bytes: genesis_len.max(line_len),
            max_events: 2,
            max_replay_bytes: genesis_len + line_len,
            ..JournalLimits::from_store(primary_root.limits())
        };
        let journal = EventJournal::initialize_v2_with_limits(
            &primary_root,
            identity.clone(),
            genesis.clone(),
            exact,
        )
        .unwrap();
        assert_eq!(
            journal
                .writer()
                .unwrap()
                .append(event.clone())
                .unwrap()
                .sequence,
            2
        );
        let exact_bytes = std::fs::read(log_path(&primary_root)).unwrap();
        assert!(matches!(
            journal.writer().unwrap().append(event.clone()),
            Err(JournalError::Incomplete {
                limit: 2,
                observed: 3
            })
        ));
        assert_eq!(std::fs::read(log_path(&primary_root)).unwrap(), exact_bytes);

        // A too-small first-record bound fails before a durable run exists.
        let (_workspace2, root2) = root();
        assert!(matches!(
            EventJournal::initialize_v2_with_limits(
                &root2,
                identity.clone(),
                genesis.clone(),
                JournalLimits {
                    max_event_line_bytes: genesis_len - 1,
                    ..exact
                },
            ),
            Err(JournalError::Incomplete { .. })
        ));
        assert!(!log_path(&root2).exists());

        let (_workspace3, root3) = root();
        assert!(matches!(
            EventJournal::initialize_v2_with_limits(
                &root3,
                identity,
                genesis,
                JournalLimits {
                    max_replay_bytes: genesis_len - 1,
                    ..exact
                },
            ),
            Err(JournalError::Incomplete { .. })
        ));
        assert!(!log_path(&root3).exists());
        drop(journal);
    }

    #[test]
    fn d1_plan_and_context_round_trip_with_exact_configured_line_bound() {
        let (identity, events) = d1_fixture();
        let lines = events.iter().map(event_line).collect::<Vec<_>>();
        let context_len = u64::try_from(lines.last().unwrap().len()).unwrap();
        let prior_max = lines[..lines.len() - 1]
            .iter()
            .map(Vec::len)
            .max()
            .and_then(|value| u64::try_from(value).ok())
            .unwrap();
        assert!(context_len > prior_max);
        let replay_bytes = lines
            .iter()
            .try_fold(0_u64, |total, line| {
                total.checked_add(u64::try_from(line.len()).ok()?)
            })
            .unwrap();
        let event_count = u64::try_from(events.len()).unwrap();

        let (_workspace, primary_root) = root();
        let exact = JournalLimits {
            max_event_line_bytes: context_len,
            max_events: event_count,
            max_replay_bytes: replay_bytes,
            ..JournalLimits::from_store(primary_root.limits())
        };
        let journal = EventJournal::initialize_v2_with_limits(
            &primary_root,
            identity.clone(),
            events[0].clone(),
            exact,
        )
        .unwrap();
        {
            let mut writer = journal.writer().unwrap();
            for event in &events[1..] {
                writer.append(event.clone()).unwrap();
            }
        }
        let expected = lines.concat();
        assert_eq!(
            std::fs::read(log_path_for(&primary_root, &identity.run_id)).unwrap(),
            expected
        );
        let reader = journal.reader().unwrap();
        assert_eq!(reader.events().len(), events.len());
        for (actual, expected) in reader.events().iter().zip(&events) {
            assert_eq!(
                actual.canonical_bytes().unwrap(),
                expected.canonical_bytes().unwrap()
            );
            assert_eq!(actual.event_hash(), expected.event_hash());
        }

        let (_workspace_small, small_root) = root();
        let small_log_path = log_path_for(&small_root, &identity.run_id);
        let lower = JournalLimits {
            max_event_line_bytes: context_len - 1,
            ..exact
        };
        let small = EventJournal::initialize_v2_with_limits(
            &small_root,
            identity,
            events[0].clone(),
            lower,
        )
        .unwrap();
        let mut writer = small.writer().unwrap();
        for event in &events[1..events.len() - 1] {
            writer.append(event.clone()).unwrap();
        }
        let before = std::fs::read(&small_log_path).unwrap();
        assert!(matches!(
            writer.append(events.last().unwrap().clone()),
            Err(JournalError::Incomplete {
                limit,
                observed,
            }) if limit == context_len - 1 && observed == context_len
        ));
        assert_eq!(std::fs::read(small_log_path).unwrap(), before);
    }

    fn rehash_event_value(event: &mut Value) {
        let schema = event["schema"].as_str().unwrap().to_owned();
        let run = event["run_id"].as_str().unwrap().to_owned();
        let genesis = event["genesis_hash"].as_str().unwrap().to_owned();
        let sequence = event["sequence"].as_u64().unwrap();
        let actor = event["actor"].as_str().unwrap().to_owned();
        let logical_time = event["logical_time"].as_u64().unwrap();
        let payload_hash = event["payload_hash"].as_str().unwrap().to_owned();
        let previous = event["previous_event_hash"].as_str().unwrap().to_owned();
        let id = StableId::derived(
            "event",
            &BTreeMap::from([
                ("actor".to_owned(), Value::String(actor.clone())),
                ("genesis_hash".to_owned(), Value::String(genesis.clone())),
                ("logical_time".to_owned(), Value::from(logical_time)),
                (
                    "payload_hash".to_owned(),
                    Value::String(payload_hash.clone()),
                ),
                (
                    "previous_event_hash".to_owned(),
                    Value::String(previous.clone()),
                ),
                ("run".to_owned(), Value::String(run.clone())),
                ("schema".to_owned(), Value::String(schema.clone())),
                ("sequence".to_owned(), Value::from(sequence)),
            ]),
        )
        .unwrap();
        event["id"] = Value::String(id.to_string());
        let hash = ContentHash::sha256(
            &canonical_json(&BTreeMap::from([
                ("actor".to_owned(), Value::String(actor)),
                ("event_id".to_owned(), Value::String(id.to_string())),
                ("genesis_hash".to_owned(), Value::String(genesis)),
                ("logical_time".to_owned(), Value::from(logical_time)),
                ("payload_hash".to_owned(), Value::String(payload_hash)),
                ("previous_event_hash".to_owned(), Value::String(previous)),
                ("run".to_owned(), Value::String(run)),
                ("schema".to_owned(), Value::String(schema)),
                ("sequence".to_owned(), Value::from(sequence)),
            ]))
            .unwrap(),
        );
        event["event_hash"] = Value::String(hash.to_string());
    }

    #[test]
    fn changed_previous_hash_with_a_rehashed_successor_chain_is_rejected() {
        let (identity, events) = d1_fixture();
        let (_workspace, root) = root();
        let journal =
            EventJournal::initialize_v2(&root, identity.clone(), events[0].clone()).unwrap();
        let mut values = events
            .iter()
            .map(|event| serde_json::to_value(event).unwrap())
            .collect::<Vec<_>>();
        values[2]["previous_event_hash"] =
            Value::String(ContentHash::sha256(b"spliced predecessor").to_string());
        rehash_event_value(&mut values[2]);
        for index in 3..values.len() {
            values[index]["previous_event_hash"] = values[index - 1]["event_hash"].clone();
            rehash_event_value(&mut values[index]);
        }
        let mut bytes = Vec::new();
        for value in values {
            bytes.extend(canonical_json(&value).unwrap());
            bytes.push(b'\n');
        }
        std::fs::write(log_path_for(&root, &identity.run_id), bytes).unwrap();
        assert!(matches!(
            journal.reader(),
            Err(JournalError::CorruptNeedsRecovery {
                auto_recoverable: false,
                ..
            })
        ));
    }

    #[test]
    fn physical_d1_outer_line_preparse_has_exact_and_configured_boundaries() {
        const EXACT: usize = 1_048_576;
        for (length, configured, expected_incomplete) in [
            (EXACT, EXACT as u64, false),
            (EXACT, (EXACT - 1) as u64, true),
            (EXACT + 1, EXACT as u64, true),
            (EXACT + 1, (EXACT + 1) as u64, false),
        ] {
            let (identity, events) = d1_fixture();
            let (_workspace, root) = root();
            let limits = JournalLimits {
                max_event_line_bytes: configured,
                max_replay_bytes: (EXACT + 1) as u64,
                ..JournalLimits::from_store(root.limits())
            };
            let journal = EventJournal::initialize_v2_with_limits(
                &root,
                identity.clone(),
                events[0].clone(),
                limits,
            )
            .unwrap();
            let mut physical = vec![b' '; length];
            physical[0] = b'{';
            physical[length - 1] = b'\n';
            std::fs::write(log_path_for(&root, &identity.run_id), physical).unwrap();
            let result = journal.reader();
            if expected_incomplete {
                assert!(matches!(
                    result,
                    Err(JournalError::Incomplete { limit, observed })
                        if limit == configured && observed == length as u64
                ));
            } else {
                assert!(matches!(
                    result,
                    Err(JournalError::CorruptNeedsRecovery {
                        auto_recoverable: false,
                        ..
                    })
                ));
            }
        }
    }

    #[test]
    fn only_physical_torn_suffix_is_recoverable_and_manual_cases_preserve_bytes() {
        let (_identity, reference_event) = fixture_event();
        let valid_without_newline = {
            let line = event_line(&reference_event);
            line[..line.len() - 1].to_vec()
        };
        let cases: [(&str, Vec<u8>, bool); 4] = [
            ("valid-json-without-newline", valid_without_newline, true),
            ("complete-invalid-final", b"{}\n".to_vec(), false),
            ("invalid-interior", b"{}\n{}\n".to_vec(), false),
            ("broken-chain-final", event_line(&reference_event), false),
        ];
        for (_name, bytes, recoverable) in cases {
            let (_workspace, root) = root();
            let (identity, event) = fixture_event();
            let journal = open_fixture(&root, identity);
            let receipt = append_torn(&journal, &event, &bytes);
            let before = std::fs::read(log_path(&root)).unwrap();
            if recoverable {
                let recovered = journal.recover("test", "reviewgraphen-store@1").unwrap();
                assert_eq!(recovered.intent.good_offset, receipt.tail_offset);
                assert_eq!(
                    std::fs::read(log_path(&root)).unwrap(),
                    initialized_with_event(&journal.identity, &event)
                );
            } else {
                assert!(matches!(
                    journal.recover("test", "reviewgraphen-store@1"),
                    Err(JournalError::CorruptNeedsRecovery {
                        auto_recoverable: false,
                        ..
                    })
                ));
                assert_eq!(std::fs::read(log_path(&root)).unwrap(), before);
            }
        }
    }

    #[test]
    fn intent_only_recovery_revalidates_the_suffix_before_resuming() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let receipt = append_torn(&journal, &event, b"{");
        let mut log = File::from(journal.open_file(true).unwrap());
        let state = scan_with_torn(&mut log, &journal.identity, journal.limits, false).unwrap();
        let torn = state.torn.unwrap();
        let intent = intent_for(
            &journal,
            receipt.tail_offset,
            &torn.discarded,
            torn.pre_tail_hash,
            1,
        );
        let (intents, _completions) = journal.recovery_dirs().unwrap();
        publish_receipt(&intents, &receipt_name(&intent.recovery_id), &intent).unwrap();
        drop(log);
        let resumed = journal.recover("another actor", "another tool").unwrap();
        assert!(resumed.resumed);
        assert_eq!(resumed.intent, intent);
        assert_eq!(
            std::fs::read(log_path(&root)).unwrap(),
            initialized_with_event(&journal.identity, &event)
        );
    }

    #[test]
    fn recovery_receipt_audit_refuses_invalid_or_orphan_records_without_touching_the_log() {
        enum Case {
            InvalidIntent,
            TornCompletion,
            CompletionOnly,
            WrongCompletionHash,
            MultiplePending,
        }
        for case in [
            Case::InvalidIntent,
            Case::TornCompletion,
            Case::CompletionOnly,
            Case::WrongCompletionHash,
            Case::MultiplePending,
        ] {
            let (_workspace, root) = root();
            let (identity, event) = fixture_event();
            let journal = open_fixture(&root, identity);
            let receipt = append_torn(&journal, &event, b"{");
            let before = std::fs::read(log_path(&root)).unwrap();
            let (intents, completions) = journal.recovery_dirs().unwrap();
            let intent = intent_for(
                &journal,
                receipt.tail_offset,
                b"{",
                event.event_hash().clone(),
                1,
            );
            match case {
                Case::InvalidIntent => {
                    publish_receipt(&intents, &receipt_name(&intent.recovery_id), &intent).unwrap();
                    std::fs::write(
                        recovery_path(&root)
                            .join(INTENTS_DIR)
                            .join(receipt_name(&intent.recovery_id)),
                        b"{",
                    )
                    .unwrap();
                }
                Case::TornCompletion => {
                    publish_receipt(&intents, &receipt_name(&intent.recovery_id), &intent).unwrap();
                    let completion = RecoveryCompletion {
                        recovery_id: intent.recovery_id.clone(),
                        post_file_hash: ContentHash::sha256(&event_line(&event)),
                    };
                    let name = receipt_name(&completion.recovery_id);
                    publish_receipt(&completions, &name, &completion).unwrap();
                    std::fs::write(recovery_path(&root).join(COMPLETIONS_DIR).join(name), b"{")
                        .unwrap();
                }
                Case::CompletionOnly => {
                    let completion = RecoveryCompletion {
                        recovery_id: intent.recovery_id.clone(),
                        post_file_hash: ContentHash::sha256(b"orphan"),
                    };
                    publish_receipt(
                        &completions,
                        &receipt_name(&completion.recovery_id),
                        &completion,
                    )
                    .unwrap();
                }
                Case::WrongCompletionHash => {
                    publish_receipt(&intents, &receipt_name(&intent.recovery_id), &intent).unwrap();
                    let completion = RecoveryCompletion {
                        recovery_id: intent.recovery_id.clone(),
                        post_file_hash: ContentHash::sha256(b"wrong canonical but hash-shaped"),
                    };
                    publish_receipt(
                        &completions,
                        &receipt_name(&completion.recovery_id),
                        &completion,
                    )
                    .unwrap();
                }
                Case::MultiplePending => {
                    publish_receipt(&intents, &receipt_name(&intent.recovery_id), &intent).unwrap();
                    let other = intent_for(
                        &journal,
                        receipt.tail_offset + 1,
                        b"other",
                        event.event_hash().clone(),
                        2,
                    );
                    publish_receipt(&intents, &receipt_name(&other.recovery_id), &other).unwrap();
                }
            }
            assert!(matches!(
                journal.recover("test", "reviewgraphen-store@1"),
                Err(JournalError::ReceiptCorruption { .. })
            ));
            assert_eq!(std::fs::read(log_path(&root)).unwrap(), before);
        }
    }

    #[test]
    fn recovery_refuses_mismatched_suffix_and_a_log_shorter_than_its_intent() {
        for shorter_than_good_offset in [false, true] {
            let (_workspace, root) = root();
            let (identity, event) = fixture_event();
            let journal = open_fixture(&root, identity);
            let receipt = append_torn(&journal, &event, b"{");
            let before = std::fs::read(log_path(&root)).unwrap();
            let (good_offset, discarded) = if shorter_than_good_offset {
                (receipt.tail_offset + 1, b"{".as_slice())
            } else {
                (receipt.tail_offset, b"different".as_slice())
            };
            let intent = intent_for(
                &journal,
                good_offset,
                discarded,
                event.event_hash().clone(),
                1,
            );
            let (intents, _completions) = journal.recovery_dirs().unwrap();
            publish_receipt(&intents, &receipt_name(&intent.recovery_id), &intent).unwrap();
            assert!(matches!(
                journal.recover("test", "reviewgraphen-store@1"),
                Err(JournalError::ReceiptCorruption { .. })
            ));
            assert_eq!(std::fs::read(log_path(&root)).unwrap(), before);
        }
    }

    #[test]
    fn receipt_publish_is_canonical_create_only_and_collision_safe() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let receipt = append_torn(&journal, &event, b"{");
        let intent = intent_for(
            &journal,
            receipt.tail_offset,
            b"{",
            event.event_hash().clone(),
            1,
        );
        let (intents, _completions) = journal.recovery_dirs().unwrap();
        let name = receipt_name(&intent.recovery_id);
        publish_receipt(&intents, &name, &intent).unwrap();
        publish_receipt(&intents, &name, &intent).unwrap();
        let mut colliding = intent.clone();
        colliding.actor = "different actor".to_owned();
        assert!(matches!(
            publish_receipt(&intents, &name, &colliding),
            Err(JournalError::ReceiptCorruption { .. })
        ));
        let bytes = std::fs::read(recovery_path(&root).join(INTENTS_DIR).join(name)).unwrap();
        assert_eq!(bytes, canonical_json(&intent).unwrap());
    }

    #[test]
    fn two_readers_share_the_lock_and_writer_excludes_them_without_sleeping() {
        let (_workspace, root) = root();
        let (identity, event) = fixture_event();
        let journal = open_fixture(&root, identity);
        journal.writer().unwrap().append(event).unwrap();
        let readers_ready = Arc::new(Barrier::new(3));
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
        std::thread::scope(|scope| {
            for _ in 0..2 {
                let journal = &journal;
                let ready = Arc::clone(&readers_ready);
                let release = Arc::clone(&release_rx);
                scope.spawn(move || {
                    let reader = journal.reader().unwrap();
                    ready.wait();
                    release.lock().unwrap().recv().unwrap();
                    drop(reader);
                });
            }
            readers_ready.wait();
            let blocked_writer = journal.open_file(true).unwrap();
            assert!(fs::flock(&blocked_writer, FlockOperation::NonBlockingLockExclusive).is_err());
            drop(blocked_writer);
            let (writer_tx, writer_rx) = mpsc::channel();
            let journal = &journal;
            scope.spawn(move || {
                let writer = journal.writer().unwrap();
                writer_tx.send(()).unwrap();
                drop(writer);
            });
            assert!(writer_rx.try_recv().is_err());
            release_tx.send(()).unwrap();
            assert!(writer_rx.try_recv().is_err());
            release_tx.send(()).unwrap();
            writer_rx.recv().unwrap();
        });
    }

    #[test]
    fn writer_excludes_reader_without_sleeping() {
        let (_workspace, root) = root();
        let (identity, _event) = fixture_event();
        let journal = open_fixture(&root, identity);
        let writer = journal.writer().unwrap();
        let blocked_reader = journal.open_file(false).unwrap();
        assert!(fs::flock(&blocked_reader, FlockOperation::NonBlockingLockShared).is_err());
        drop(blocked_reader);
        let (reader_tx, reader_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let reader = journal.reader().unwrap();
                reader_tx.send(reader.events().len()).unwrap();
            });
            assert!(reader_rx.try_recv().is_err());
            drop(writer);
            assert_eq!(reader_rx.recv().unwrap(), 1);
        });
    }

    #[test]
    fn v3_recovery_replay_returns_with_the_same_exclusive_lock_held() {
        let (_workspace, root) = root();
        let (identity, manifest, roots, _genesis) = v3_fixture(&root);
        let run_id = identity.run_id.clone();
        let journal = EventJournal::initialize_v3(&root, identity, manifest).unwrap();
        let path = root
            .path()
            .join(RUNS_DIR)
            .join(run_dir_name(&run_id))
            .join(JOURNAL_FILE);
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(b"{").unwrap();
        file.sync_data().unwrap();
        drop(file);

        let (receipt, session, basis) = journal
            .recover_replayed_v3_session("test", "v3-lock", &roots)
            .unwrap();
        assert_eq!(receipt.intent.run_id, run_id);
        assert_eq!(session.event_count().unwrap(), 1);
        assert_eq!(basis.confirmed_event_count(), 1);
        let blocked_reader = journal.open_file(false).unwrap();
        assert!(fs::flock(&blocked_reader, FlockOperation::NonBlockingLockShared).is_err());
        let blocked_writer = journal.open_file(true).unwrap();
        assert!(fs::flock(&blocked_writer, FlockOperation::NonBlockingLockExclusive).is_err());
        drop(session);
        fs::flock(&blocked_reader, FlockOperation::NonBlockingLockShared).unwrap();
    }

    #[test]
    fn v4_bootstrap_publishes_exact_genesis_and_genesis_recovery_is_attributed() {
        let (_workspace, root) = root();
        let log = v4_bootstrap_log("run:journal-v4-bootstrap");
        let run_id = log.run_id().clone();
        let genesis_hash = log.genesis_hash().clone();
        let expected_event = log.envelopes()[0].clone();
        let (_journal, published) = EventJournal::publish_new_v4(&root, log).unwrap();
        assert_eq!(published.run_id, run_id);
        assert_eq!(published.genesis_hash, genesis_hash);
        assert_eq!(published.event_id, *expected_event.id());
        assert_eq!(published.event_hash, *expected_event.event_hash());

        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(run_id, genesis_hash, RecoveryKindV4::GenesisBootstrap),
        )
        .unwrap();
        let recovered = EventJournal::recover_new_v4(
            &root,
            key,
            RecoveryProvenanceV4::new("test", "v4-bootstrap").unwrap(),
        )
        .unwrap();
        match recovered {
            GenesisRecoveryV4::Committed { receipt, journal } => {
                assert_eq!(receipt.schema(), RECOVERY_RECEIPT_SCHEMA_V4);
                assert_eq!(receipt.run_id(), &published.run_id);
                assert_eq!(receipt.genesis_hash(), &published.genesis_hash);
                assert_eq!(receipt.pre_recovery_offset(), published.confirmed_offset);
                assert_eq!(
                    receipt.recovery_key_kind(),
                    RecoveryKindV4::GenesisBootstrap
                );
                assert!(receipt.pending_digest().is_none());
                assert_eq!(receipt.provenance().actor(), "test");
                assert_eq!(receipt.provenance().tool_version(), "v4-bootstrap");
                assert!(matches!(
                    receipt.outcome(),
                    RecoveryOutcomeV4::GenesisCommitted { .. }
                ));
                assert_eq!(journal.identity.version(), EventContractVersion::V4);
            }
            GenesisRecoveryV4::NotCommitted { .. } => panic!("durable V4 genesis was lost"),
        }
    }

    #[test]
    fn v4_canonical_tail_recovery_rechecks_opaque_inspection_key() {
        let (_workspace, root) = root();
        let log = v4_bootstrap_log("run:journal-v4-tail");
        let run_id = log.run_id().clone();
        let genesis_hash = log.genesis_hash().clone();
        let roots = AuthorityTrustRootsV4::new(
            ContentHash::sha256(b"v4-tail-policy"),
            StableId::parse("repository:double-submit-payment").unwrap(),
            ContentHash::parse("sha256:1111111111111111").unwrap(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let (journal, _published) = EventJournal::publish_new_v4(&root, log).unwrap();
        let path = root
            .path()
            .join(RUNS_DIR)
            .join(run_dir_name(&run_id))
            .join(JOURNAL_FILE);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"{").unwrap();
        file.sync_all().unwrap();
        drop(file);

        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(
                run_id.clone(),
                genesis_hash.clone(),
                RecoveryKindV4::CanonicalTail,
            ),
        )
        .unwrap();
        let (receipt, recovered) = journal
            .recover_replayed_v4_session(
                &roots,
                key,
                RecoveryProvenanceV4::new("test", "v4-tail").unwrap(),
            )
            .unwrap();
        assert!(matches!(
            receipt.outcome(),
            RecoveryOutcomeV4::TailRecovered { .. }
        ));
        let RecoveredV4Session::Editable { session, basis } = recovered else {
            panic!("canonical-tail recovery must be editable")
        };
        assert_eq!(basis.confirmed_event_count(), 1);
        let projected = crate::index::v5_snapshot_for_test(&session, &basis).unwrap();
        assert_eq!(projected.marker.event_count, basis.confirmed_event_count());
        assert!(try_acquire_v4_root_lock(&root).unwrap().is_none());
        drop(session);
        assert_eq!(std::fs::read(&path).unwrap().last(), Some(&b'\n'));

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"{").unwrap();
        file.sync_all().unwrap();
        drop(file);
        let stale_key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(run_id, genesis_hash, RecoveryKindV4::CanonicalTail),
        )
        .unwrap();
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(b"x").unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert!(matches!(
            journal.recover_replayed_v4_session(
                &roots,
                stale_key,
                RecoveryProvenanceV4::new("test", "v4-stale").unwrap(),
            ),
            Err(JournalError::RecoveryKeyMismatchV4)
        ));
    }

    #[test]
    fn v4_m4_inspection_refuses_an_absent_foreign_run_without_creating_state() {
        let (_workspace, root) = root();
        assert!(matches!(
            EventJournal::inspect_recovery_v4(
                &root,
                RecoveryInspectionV4::new(
                    StableId::parse("run:journal-v4-m4-seam").unwrap(),
                    ContentHash::sha256(b"v4-m4-seam"),
                    RecoveryKindV4::M4BundleResume,
                ),
            ),
            Err(JournalError::RecoveryKeyMismatchV4)
        ));
    }

    #[test]
    fn v4_genesis_recovery_removes_only_uncommitted_empty_or_partial_log() {
        let (_workspace, root) = root();
        let log = v4_bootstrap_log("run:journal-v4-uncommitted");
        let run_id = log.run_id().clone();
        let genesis_hash = log.genesis_hash().clone();
        let (_journal, _receipt) = EventJournal::publish_new_v4(&root, log).unwrap();
        let path = root
            .path()
            .join(RUNS_DIR)
            .join(run_dir_name(&run_id))
            .join(JOURNAL_FILE);
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(0).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(run_id, genesis_hash, RecoveryKindV4::GenesisBootstrap),
        )
        .unwrap();
        let recovered = EventJournal::recover_new_v4(
            &root,
            key,
            RecoveryProvenanceV4::new("test", "v4-uncommitted").unwrap(),
        )
        .unwrap();
        assert!(matches!(recovered, GenesisRecoveryV4::NotCommitted { .. }));
        assert!(!path.exists());
    }

    #[test]
    fn v4_absent_genesis_key_refuses_a_marker_created_after_inspection() {
        let (_workspace, root) = root();
        let run_id = StableId::parse("run:journal-v4-absent-marker-race").unwrap();
        let genesis_hash = ContentHash::sha256(b"absent-marker-race");
        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(
                run_id.clone(),
                genesis_hash,
                RecoveryKindV4::GenesisBootstrap,
            ),
        )
        .unwrap();
        let runs = open_or_create_dir(root.fd(), RUNS_DIR, "runs directory").unwrap();
        let run = open_or_create_dir(&runs, &run_dir_name(&run_id), "run directory").unwrap();
        publish_bundle_file(&run, APPEND_PENDING_MARKER, APPEND_PENDING_BYTES).unwrap();
        assert!(matches!(
            EventJournal::recover_new_v4(
                &root,
                key,
                RecoveryProvenanceV4::new("test", "absent-marker-race").unwrap(),
            ),
            Err(JournalError::RecoveryKeyMismatchV4)
        ));
    }

    #[test]
    fn v4_root_lock_uses_distinct_open_file_descriptions_for_all_store_handles() {
        let (workspace, root) = root();
        let held = acquire_v4_root_lock(&root).unwrap();
        assert!(try_acquire_v4_root_lock(&root).unwrap().is_none());
        let reopened =
            StoreRoot::open(workspace.path(), super::super::StoreLimits::default()).unwrap();
        assert!(try_acquire_v4_root_lock(&reopened).unwrap().is_none());
        drop(held);
        assert!(try_acquire_v4_root_lock(&root).unwrap().is_some());
        assert!(try_acquire_v4_root_lock(&reopened).unwrap().is_some());
    }

    #[test]
    fn legacy_recovery_never_mutates_a_v4_journal() {
        let (_workspace, root) = root();
        let log = v4_bootstrap_log("run:journal-v4-legacy-refusal");
        let run_id = log.run_id().clone();
        let (journal, _) = EventJournal::publish_new_v4(&root, log).unwrap();
        let path = root
            .path()
            .join(RUNS_DIR)
            .join(run_dir_name(&run_id))
            .join(JOURNAL_FILE);
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            journal.recover("legacy", "legacy@1"),
            Err(JournalError::V4ReplaySessionRequired)
        ));
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn v4_bootstrap_failpoints_separate_not_committed_from_uncertain() {
        for (index, fault) in [
            V4BootstrapFault::BeforeDirectorySetup,
            V4BootstrapFault::BeforeLineWrite,
            V4BootstrapFault::BeforeFileSync,
            V4BootstrapFault::AfterFileSync,
        ]
        .into_iter()
        .enumerate()
        {
            let (_workspace, root) = root();
            let run = format!("run:journal-v4-pre-line-{index}");
            let log = v4_bootstrap_log(&run);
            inject_v4_bootstrap_fault(fault);
            assert!(matches!(
                EventJournal::publish_new_v4(&root, log),
                Err(JournalError::GenesisNotCommittedV4 { .. })
            ));
            let path = root
                .path()
                .join(RUNS_DIR)
                .join(run_dir_name(&StableId::parse(run).unwrap()))
                .join(JOURNAL_FILE);
            assert!(!path.exists());
        }

        for (index, fault) in [
            V4BootstrapFault::AfterLink,
            V4BootstrapFault::AfterDirectorySync,
            V4BootstrapFault::BeforeSessionOpen,
        ]
        .into_iter()
        .enumerate()
        {
            let (_workspace, root) = root();
            let run = format!("run:journal-v4-uncertain-{index}");
            let log = v4_bootstrap_log(&run);
            let run_id = log.run_id().clone();
            let genesis_hash = log.genesis_hash().clone();
            inject_v4_bootstrap_fault(fault);
            assert!(matches!(
                EventJournal::publish_new_v4(&root, log),
                Err(JournalError::GenesisSessionUncertainV4)
            ));
            let key = EventJournal::inspect_recovery_v4(
                &root,
                RecoveryInspectionV4::new(run_id, genesis_hash, RecoveryKindV4::GenesisBootstrap),
            )
            .unwrap();
            assert!(matches!(
                EventJournal::recover_new_v4(
                    &root,
                    key,
                    RecoveryProvenanceV4::new("test", "uncertain").unwrap(),
                )
                .unwrap(),
                GenesisRecoveryV4::Committed { .. }
            ));
        }
    }

    #[test]
    fn v4_bootstrap_is_create_only_even_for_identical_or_extended_existing_logs() {
        let (_workspace, root) = root();
        let run = "run:journal-v4-create-only";
        EventJournal::publish_new_v4(&root, v4_bootstrap_log(run)).unwrap();
        assert!(matches!(
            EventJournal::publish_new_v4(&root, v4_bootstrap_log(run)),
            Err(JournalError::ReceiptCorruption { .. })
        ));
        let path = root
            .path()
            .join(RUNS_DIR)
            .join(run_dir_name(&StableId::parse(run).unwrap()))
            .join(JOURNAL_FILE);
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(b"{").unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert!(matches!(
            EventJournal::publish_new_v4(&root, v4_bootstrap_log(run)),
            Err(JournalError::ReceiptCorruption { .. })
        ));
    }

    #[test]
    fn v4_inspection_matches_only_the_closed_requested_durable_state() {
        let (_workspace, root) = root();
        let run = "run:journal-v4-inspection";
        let log = v4_bootstrap_log(run);
        let run_id = log.run_id().clone();
        let genesis_hash = log.genesis_hash().clone();
        let (journal, _) = EventJournal::publish_new_v4(&root, log).unwrap();
        assert!(matches!(
            EventJournal::inspect_recovery_v4(
                &root,
                RecoveryInspectionV4::new(
                    run_id.clone(),
                    genesis_hash.clone(),
                    RecoveryKindV4::CanonicalTail,
                ),
            ),
            Err(JournalError::RecoveryKeyMismatchV4)
        ));

        restore_append_marker(&journal.run).unwrap();
        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(run_id, genesis_hash, RecoveryKindV4::CanonicalTail),
        )
        .unwrap();
        assert_eq!(
            key.pending_digest.as_ref(),
            Some(&ContentHash::sha256(APPEND_PENDING_BYTES))
        );
        let receipt = EventJournal::recover_canonical_tail_v4(
            &root,
            key,
            RecoveryProvenanceV4::new("test", "marker-cleanup").unwrap(),
        )
        .unwrap();
        assert!(receipt.pending_digest().is_some());
        assert!(!bundle_file_exists(&journal.run, APPEND_PENDING_MARKER).unwrap());
    }

    #[test]
    fn v4_m4_marker_inspection_seals_the_exact_marker_hash() {
        let (_workspace, root) = root();
        let run = "run:journal-v4-m4-marker";
        let log = v4_bootstrap_log(run);
        let planned = log.envelopes()[0].clone();
        let (journal, _) = EventJournal::publish_new_v4(&root, log).unwrap();
        let marker = v4_marker_at_current_tail(&journal, &[planned]);
        publish_bundle_file(
            &journal.run,
            BUNDLE_PENDING_MARKER,
            &canonical_json(&marker).unwrap(),
        )
        .unwrap();
        let marker_bytes = canonical_json(&marker).unwrap();
        let (_, observed_hash) = read_bundle_pending_with_hash(
            &journal.run,
            &journal.identity,
            JournalLimits::from_store(root.limits()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(observed_hash, ContentHash::sha256(&marker_bytes));
    }

    #[test]
    fn v4_m4_prefix_stage_classification_is_exhaustive_for_every_legal_prefix() {
        let (_workspace, root) = root();
        let log = v4_bootstrap_log("run:journal-v4-m4-prefix-stages");
        let planned_event = log.envelopes()[0].clone();
        let (journal, _) = EventJournal::publish_new_v4(&root, log).unwrap();
        let reader = journal.reader().unwrap();
        let pre_state = ScanState {
            events: Vec::new(),
            confirmed_offset: 0,
            tail_hash: v4_chain_genesis_hash(
                &journal.identity.run_id,
                &journal.identity.genesis_hash(),
            ),
            torn: None,
        };
        for (confirmed, expected) in [
            (0, M4BundlePrefixClassificationV4::Stage0),
            (1, M4BundlePrefixClassificationV4::StrictInterior),
            (2, M4BundlePrefixClassificationV4::StrictInterior),
            (3, M4BundlePrefixClassificationV4::AlreadyComplete),
        ] {
            let planned = vec![planned_event.clone(); 3];
            let marker =
                VerificationBundlePendingMarkerV3::new(&journal.identity, &pre_state, &planned)
                    .unwrap();
            let marker_hash = ContentHash::sha256(&canonical_json(&marker).unwrap());
            let pending = BundlePendingInspection {
                marker,
                marker_hash,
                planned,
                pre_state: pre_state.clone(),
                current_state: reader.state.clone(),
                confirmed_events: confirmed,
                discarded: Vec::new(),
            };
            let stage = v4_bundle_prefix_stage(&pending).unwrap();
            assert_eq!(stage.classification(), expected);
            assert_eq!(stage.confirmed_events(), confirmed as u64);
            assert_eq!(stage.expected_events(), 3);
        }
    }

    #[test]
    fn v4_m4_inspection_rejects_marker_attribution_and_stage_corruption() {
        let (_workspace, first_root) = root();
        let run = "run:journal-v4-m4-marker-attribution";
        let log = v4_bootstrap_log(run);
        let run_id = log.run_id().clone();
        let genesis_hash = log.genesis_hash().clone();
        let planned = log.envelopes()[0].clone();
        let (journal, _) = EventJournal::publish_new_v4(&first_root, log).unwrap();
        let mut marker = v4_marker_at_current_tail(&journal, &[planned]);
        marker.expected_event_hashes[0] = ContentHash::sha256(b"foreign-event-hash");
        publish_bundle_file(
            &journal.run,
            BUNDLE_PENDING_MARKER,
            &canonical_json(&marker).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            EventJournal::inspect_recovery_v4(
                &first_root,
                RecoveryInspectionV4::new(run_id, genesis_hash, RecoveryKindV4::M4BundleResume,),
            ),
            Err(JournalError::ReceiptCorruption { .. })
        ));

        let (_workspace, second_root) = root();
        let run = "run:journal-v4-m4-stage-corruption";
        let log = v4_bootstrap_log(run);
        let planned = log.envelopes()[0].clone();
        let (journal, _) = EventJournal::publish_new_v4(&second_root, log).unwrap();
        let marker = v4_marker_at_current_tail(&journal, &[planned]);
        publish_bundle_file(
            &journal.run,
            BUNDLE_PENDING_MARKER,
            &canonical_json(&marker).unwrap(),
        )
        .unwrap();
        publish_bundle_file(&journal.run, BUNDLE_PENDING_STAGE, b"invalid\n").unwrap();
        assert!(matches!(
            read_bundle_stage(&journal.run, marker.expected_count),
            Err(JournalError::ReceiptCorruption { .. })
        ));
    }

    #[test]
    fn v4_m4_recovery_rechecks_the_key_and_never_cleans_an_invalid_authority_plan() {
        let (_workspace, root) = root();
        let program_bytes =
            include_bytes!("../../../examples/double-submit-payment/program-space.json");
        let program = ProgramSpace::from_json_slice(program_bytes).unwrap();
        let program_json: Value = serde_json::from_slice(program_bytes).unwrap();
        let repository_source_hash = ContentHash::parse(
            program_json["source"]["content_hash"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
        .unwrap();
        let roots = AuthorityTrustRootsV4::new(
            ContentHash::sha256(b"m4-key-recheck-policy"),
            program.repository_id().clone(),
            repository_source_hash,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        let (_, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let obligation_id = obligations.first().unwrap().id().clone();
        let log = v4_bootstrap_log("run:journal-v4-m4-key-recheck");
        let run_id = log.run_id().clone();
        let genesis_hash = log.genesis_hash().clone();
        let genesis_bytes = log.canonical_genesis_bytes().to_vec();
        let confirmed = log.envelopes().to_vec();
        let (journal, _) = EventJournal::publish_new_v4(&root, log).unwrap();
        let resolver = JournalAuthorityResolverV4 {
            reader: CasReader::open_existing(&root).unwrap(),
        };
        let session_identity = OpaqueSessionIdentityV4::fresh();
        let (log, basis) = EventLogV4::replay_confirmed_v4_prefix_for_session(
            run_id.clone(),
            &genesis_bytes,
            &confirmed,
            &resolver,
            &roots,
            EventReplayLimits::new(16, 2 * 1024 * 1024),
            &session_identity,
        )
        .unwrap();
        let prepared = log
            .prepare_inherited_d2_event_v4(
                EventCommand::obligation_transition(obligation_id, ObligationLifecycle::Planned),
                &basis,
            )
            .unwrap();
        let planned = prepared
            .envelope(&log, &basis, &session_identity)
            .unwrap()
            .clone();
        let path = root
            .path()
            .join(RUNS_DIR)
            .join(run_dir_name(&run_id))
            .join(JOURNAL_FILE);
        let marker = v4_marker_at_current_tail(&journal, std::slice::from_ref(&planned));
        publish_bundle_file(
            &journal.run,
            BUNDLE_PENDING_MARKER,
            &canonical_json(&marker).unwrap(),
        )
        .unwrap();
        let invalid_plan_key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(
                run_id.clone(),
                genesis_hash.clone(),
                RecoveryKindV4::M4BundleResume,
            ),
        )
        .unwrap();
        assert!(matches!(
            journal.recover_replayed_v4_session(
                &roots,
                invalid_plan_key,
                RecoveryProvenanceV4::new("test", "m4-invalid-plan").unwrap(),
            ),
            Err(JournalError::BundleResumeAuthorityMismatch)
        ));
        assert!(bundle_file_exists(&journal.run, BUNDLE_PENDING_MARKER).unwrap());
        let stale_key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(
                run_id.clone(),
                genesis_hash.clone(),
                RecoveryKindV4::M4BundleResume,
            ),
        )
        .unwrap();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        let mut line = planned.canonical_bytes().unwrap();
        line.push(b'\n');
        file.write_all(&line).unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert!(matches!(
            journal.recover_replayed_v4_session(
                &roots,
                stale_key,
                RecoveryProvenanceV4::new("test", "m4-key-recheck").unwrap(),
            ),
            Err(JournalError::RecoveryKeyMismatchV4)
        ));
        assert!(bundle_file_exists(&journal.run, BUNDLE_PENDING_MARKER).unwrap());
    }

    #[test]
    fn v4_m4_store_recovery_covers_every_real_static_prefix_and_resumes_only_interior() {
        for confirmed in 0..=3_usize {
            let (_workspace, root) = root();
            let run = format!("run:journal-v4-m4-real-prefix-{confirmed}");
            let (journal, roots, planned) = public_v4_static_bundle_journal(&root, &run);
            let mut writer = journal.writer_v4().unwrap();
            let marker =
                VerificationBundlePendingMarkerV3::new(&journal.identity, &writer.state, &planned)
                    .unwrap();
            let JournalGenesis::V4Shared(genesis) = &writer.identity.genesis else {
                unreachable!()
            };
            let resolver = JournalAuthorityResolverV4 {
                reader: CasReader::open_existing(&root).unwrap(),
            };
            let recovery_session = OpaqueSessionIdentityV4::fresh();
            EventLogV4::recover_verification_bundle_v4_for_session(
                writer.identity.run_id.clone(),
                genesis,
                &writer.state.events,
                &planned,
                &resolver,
                &roots,
                EventReplayLimits::new(writer.limits.max_events, writer.limits.max_replay_bytes),
                &recovery_session,
            )
            .unwrap();
            let marker_hash = ContentHash::sha256(&canonical_json(&marker).unwrap());
            publish_bundle_file(
                &journal.run,
                BUNDLE_PENDING_MARKER,
                &canonical_json(&marker).unwrap(),
            )
            .unwrap();
            publish_bundle_file(&journal.run, BUNDLE_PENDING_STAGE, b"0\n").unwrap();
            if confirmed != 0 {
                writer.file.seek(SeekFrom::End(0)).unwrap();
                for envelope in &planned[..confirmed] {
                    writer
                        .file
                        .write_all(&envelope.canonical_bytes().unwrap())
                        .unwrap();
                    writer.file.write_all(b"\n").unwrap();
                }
                writer.file.sync_data().unwrap();
                persist_bundle_stage_file(&journal.run, confirmed as u64).unwrap();
            }
            drop(writer);

            let key = EventJournal::inspect_recovery_v4(
                &root,
                RecoveryInspectionV4::new(
                    journal.identity.run_id.clone(),
                    journal.identity.genesis_hash(),
                    RecoveryKindV4::M4BundleResume,
                ),
            )
            .unwrap();
            let (receipt, recovered) = journal
                .recover_replayed_v4_session(
                    &roots,
                    key,
                    RecoveryProvenanceV4::new("test", format!("m4-prefix-{confirmed}")).unwrap(),
                )
                .unwrap_or_else(|error| panic!("real M4 prefix {confirmed} failed: {error}"));
            assert_eq!(receipt.pre_file_hash(), receipt.post_file_hash());
            let durable_file_hash = ContentHash::sha256(
                &std::fs::read(
                    root.path()
                        .join(RUNS_DIR)
                        .join(run_dir_name(&journal.identity.run_id))
                        .join(JOURNAL_FILE),
                )
                .unwrap(),
            );
            assert_eq!(receipt.post_file_hash(), Some(&durable_file_hash));
            assert_eq!(receipt.event_contract_version(), EVENT_CONTRACT_SCHEMA_V4);
            assert_eq!(receipt.provenance().actor(), "test");
            assert_eq!(
                receipt.provenance().tool_version(),
                format!("m4-prefix-{confirmed}")
            );
            let marker_receipt = receipt.marker_recovery().unwrap();
            assert_eq!(marker_receipt.pre_marker_hash, marker_hash);
            assert_eq!(
                marker_receipt.prefix_stage.confirmed_events(),
                confirmed as u64
            );
            assert_eq!(marker_receipt.prefix_stage.expected_events(), 3);

            match (confirmed, recovered) {
                (0 | 3, RecoveredV4Session::Editable { session, basis }) => {
                    assert!(matches!(
                        receipt.outcome(),
                        RecoveryOutcomeV4::M4BundleCleanupOrdinary { .. }
                    ));
                    assert_eq!(
                        marker_receipt.action,
                        M4BundleMarkerActionV4::ClearedAndSynced
                    );
                    assert_eq!(marker_receipt.post_marker_hash, None);
                    assert!(!bundle_file_exists(&journal.run, BUNDLE_PENDING_MARKER).unwrap());
                    let projected = crate::index::v5_snapshot_for_test(&session, &basis).unwrap();
                    assert_eq!(projected.marker.event_count, basis.confirmed_event_count());
                    let competing = journal.open_file(false).unwrap();
                    assert_eq!(
                        fs::flock(&competing, FlockOperation::NonBlockingLockExclusive),
                        Err(rustix::io::Errno::WOULDBLOCK)
                    );
                    drop(session);
                    fs::flock(&competing, FlockOperation::NonBlockingLockExclusive).unwrap();
                }
                (
                    1 | 2,
                    RecoveredV4Session::M4BundleResumeRequired {
                        session,
                        resume_authority,
                    },
                ) => {
                    assert!(matches!(
                        receipt.outcome(),
                        RecoveryOutcomeV4::M4BundleResumeRequired { .. }
                    ));
                    assert_eq!(
                        marker_receipt.action,
                        M4BundleMarkerActionV4::RetainedForResume
                    );
                    assert_eq!(marker_receipt.post_marker_hash.as_ref(), Some(&marker_hash));
                    assert!(bundle_file_exists(&journal.run, BUNDLE_PENDING_MARKER).unwrap());
                    assert_eq!(session.durable_stage().confirmed_events(), confirmed as u64);
                    let competing = journal.open_file(false).unwrap();
                    assert_eq!(
                        fs::flock(&competing, FlockOperation::NonBlockingLockExclusive),
                        Err(rustix::io::Errno::WOULDBLOCK)
                    );
                    let (session, basis, append) = session
                        .resume_verification_bundle(resume_authority)
                        .unwrap();
                    assert_eq!(append.journal().len(), 3 - confirmed);
                    assert_eq!(append.authority().event_ids().len(), 3);
                    let projected = crate::index::v5_snapshot_for_test(&session, &basis).unwrap();
                    assert_eq!(projected.marker.event_count, basis.confirmed_event_count());
                    assert!(!bundle_file_exists(&journal.run, BUNDLE_PENDING_MARKER).unwrap());
                    assert_eq!(
                        fs::flock(&competing, FlockOperation::NonBlockingLockExclusive),
                        Err(rustix::io::Errno::WOULDBLOCK)
                    );
                    drop(session);
                    fs::flock(&competing, FlockOperation::NonBlockingLockExclusive).unwrap();
                    fs::flock(&competing, FlockOperation::Unlock).unwrap();
                    drop(competing);
                    assert!(matches!(
                        EventJournal::inspect_recovery_v4(
                            &root,
                            RecoveryInspectionV4::new(
                                journal.identity.run_id.clone(),
                                journal.identity.genesis_hash(),
                                RecoveryKindV4::M4BundleResume,
                            ),
                        ),
                        Err(JournalError::RecoveryKeyMismatchV4)
                    ));
                }
                _ => panic!("M4 recovery returned a branch inconsistent with prefix stage"),
            }
        }
    }

    #[test]
    fn v4_public_store_surface_durably_appends_fresh_m4_and_holds_all_session_locks() {
        let (_workspace, root) = root();
        let (journal, roots, planned, receipt, _) = public_v4_static_bundle_journal_with_append(
            &root,
            "run:journal-v4-fresh-m4-store-surface",
            true,
            false,
            false,
        );
        let receipt = receipt.expect("fresh M4 append receipt");
        assert_eq!(receipt.journal().len(), planned.len());
        assert_eq!(receipt.authority().event_ids().len(), planned.len());
        assert_eq!(
            receipt.authority().confirmed_tail_hash(),
            planned.last().unwrap().event_hash()
        );

        let (session, basis) = journal.replayed_v4_session(&roots).unwrap();
        assert_eq!(
            basis.confirmed_event_count(),
            session.writer.events().len() as u64
        );
        assert_eq!(
            basis.confirmed_tail_hash(),
            planned.last().unwrap().event_hash()
        );
        assert!(try_acquire_v4_root_lock(&root).unwrap().is_none());
        let blocked_reader = journal.open_file(false).unwrap();
        assert!(fs::flock(&blocked_reader, FlockOperation::NonBlockingLockShared).is_err());
        drop(blocked_reader);
        drop(session);
        assert!(try_acquire_v4_root_lock(&root).unwrap().is_some());
        let unlocked_reader = journal.open_file(false).unwrap();
        fs::flock(&unlocked_reader, FlockOperation::NonBlockingLockShared).unwrap();
    }

    #[test]
    fn v4_store_publishes_two_gluing_inputs_and_one_atomic_bundle() {
        let (workspace, root) = root();
        let (journal, roots, [mut payment, mut ui]) =
            public_v4_gluing_journal(&root, "run:journal-v4-gluing-happy");
        let (mut session, mut basis) = journal.replayed_v4_session(&roots).unwrap();
        let before = basis.confirmed_event_count();
        let zero_prefix = crate::index::v5_snapshot_for_test(&session, &basis).unwrap();
        assert!(zero_prefix.artifact_registrations_v4.is_empty());
        assert!(zero_prefix.gluing_input_descriptors.is_empty());
        assert!(zero_prefix.context_covers.is_empty());
        let (zero_accounting, zero_oracle) = crate::index::v5_preflight_accounting_limits_for_test(
            &session,
            &basis,
            u64::MAX,
            u64::MAX,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(zero_accounting, zero_oracle);
        let payment_result = session
            .publish_gluing_input(
                payment.sources.remove(0),
                payment.descriptor.clone(),
                &mut basis,
            )
            .unwrap();
        let GluingInputPublicationV4::Confirmed {
            cas: payment_cas,
            append: payment_append,
        } = payment_result
        else {
            panic!("payment input was not newly confirmed")
        };
        assert!(!payment_cas.existed);
        assert_eq!(
            payment_append.core().context_id(),
            payment.descriptor.context_id()
        );
        assert_eq!(basis.gluing_input_entry_count(), 1);
        let one_prefix = crate::index::v5_snapshot_for_test(&session, &basis).unwrap();
        assert_eq!(one_prefix.artifact_registrations_v4.len(), 1);
        assert_eq!(one_prefix.gluing_input_descriptors.len(), 1);
        assert!(one_prefix.context_covers.is_empty());
        let (one_accounting, one_oracle) = crate::index::v5_preflight_accounting_limits_for_test(
            &session,
            &basis,
            u64::MAX,
            u64::MAX,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(one_accounting, one_oracle);

        let ui_result = session
            .publish_gluing_input(ui.sources.remove(0), ui.descriptor.clone(), &mut basis)
            .unwrap();
        assert!(matches!(
            ui_result,
            GluingInputPublicationV4::Confirmed { .. }
        ));
        assert_eq!(basis.gluing_input_entry_count(), 2);
        assert_eq!(basis.confirmed_event_count(), before + 2);
        let two_prefix = crate::index::v5_snapshot_for_test(&session, &basis).unwrap();
        assert_eq!(two_prefix.artifact_registrations_v4.len(), 2);
        assert_eq!(two_prefix.gluing_input_descriptors.len(), 2);
        assert!(two_prefix.context_covers.is_empty());
        let (two_accounting, two_oracle) = crate::index::v5_preflight_accounting_limits_for_test(
            &session,
            &basis,
            u64::MAX,
            u64::MAX,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(two_accounting, two_oracle);

        let bundle = session.mint_gluing_bundle(&basis).unwrap();
        let bundle_receipt = session.append_gluing_bundle(bundle, &mut basis).unwrap();
        assert_eq!(
            bundle_receipt.core().result(),
            reviewgraphen_core::GluingResultV4::Unknown
        );
        assert_eq!(basis.confirmed_event_count(), before + 3);
        assert!(matches!(
            session.mint_gluing_bundle(&basis),
            Err(JournalError::Domain(
                reviewgraphen_core::DomainError::AlreadyComplete
            ))
        ));

        let count = basis.confirmed_event_count();
        let existing = session
            .publish_gluing_input(
                payment.sources.remove(0),
                payment.descriptor.clone(),
                &mut basis,
            )
            .unwrap();
        assert!(matches!(
            existing,
            GluingInputPublicationV4::AlreadyRegistered { ref context_id, .. }
                if context_id == payment.descriptor.context_id()
        ));
        assert_eq!(basis.confirmed_event_count(), count);
        drop(session);

        let (replayed, replayed_basis) = journal.replayed_v4_session(&roots).unwrap();
        assert_eq!(replayed_basis.confirmed_event_count(), count);
        assert_eq!(replayed_basis.gluing_input_entry_count(), 2);
        assert!(
            replayed
                .log
                .registered_gluing_input_projection_v4(payment.descriptor.context_id())
                .is_some()
        );
        assert!(
            replayed
                .log
                .registered_gluing_input_projection_v4(ui.descriptor.context_id())
                .is_some()
        );
        let (preflight, oracle) = crate::index::v5_preflight_accounting_limits_for_test(
            &replayed,
            &replayed_basis,
            u64::MAX,
            u64::MAX,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(preflight, oracle);
        for (rows, query, working) in [
            (
                preflight.rows,
                preflight.query_bytes,
                preflight.working_bytes,
            ),
            (
                preflight.rows - 1,
                preflight.query_bytes,
                preflight.working_bytes,
            ),
            (
                preflight.rows,
                preflight.query_bytes - 1,
                preflight.working_bytes,
            ),
            (
                preflight.rows,
                preflight.query_bytes,
                preflight.working_bytes - 1,
            ),
        ] {
            crate::index::reset_v5_full_snapshot_construction_count_for_test();
            let result = crate::index::v5_preflight_accounting_limits_for_test(
                &replayed,
                &replayed_basis,
                rows,
                query,
                working,
            );
            if rows == preflight.rows
                && query == preflight.query_bytes
                && working == preflight.working_bytes
            {
                assert!(result.is_ok());
                let (decodes, materializations, snapshots, sqlite, serializations) =
                    crate::index::v5_post_phase0_counts_for_test();
                assert_eq!(decodes, 0);
                assert_eq!(
                    (materializations, snapshots, sqlite, serializations),
                    (1, 1, 0, 0)
                );
            } else {
                assert!(matches!(result, Err(crate::IndexError::Incomplete { .. })));
                assert_eq!(
                    crate::index::v5_post_phase0_counts_for_test(),
                    (0, 0, 0, 0, 0)
                );
            }
        }
        drop(replayed);
        for (rows, query, working) in [
            (
                preflight.rows - 1,
                preflight.query_bytes,
                preflight.working_bytes,
            ),
            (
                preflight.rows,
                preflight.query_bytes - 1,
                preflight.working_bytes,
            ),
            (
                preflight.rows,
                preflight.query_bytes,
                preflight.working_bytes - 1,
            ),
        ] {
            let limits = super::super::StoreLimits {
                max_index_rows: rows,
                max_index_query_bytes: query,
                max_index_working_bytes: working,
                ..super::super::StoreLimits::default()
            };
            let limited_root = StoreRoot::open(workspace.path(), limits).unwrap();
            let limited_index = crate::DerivedIndexV5::open(&limited_root).unwrap();
            crate::index::reset_v5_full_snapshot_construction_count_for_test();
            assert!(matches!(
                limited_index.rebuild_v5(&journal, &roots),
                Err(crate::IndexError::Incomplete { .. })
            ));
            assert_eq!(
                crate::index::v5_post_phase0_counts_for_test(),
                (0, 0, 0, 0, 0)
            );
        }
        let exact_limits = super::super::StoreLimits {
            max_index_rows: preflight.rows,
            max_index_query_bytes: preflight.query_bytes,
            ..super::super::StoreLimits::default()
        };
        let exact_root = StoreRoot::open(workspace.path(), exact_limits).unwrap();
        let exact_index = crate::DerivedIndexV5::open(&exact_root).unwrap();
        crate::index::reset_v5_full_snapshot_construction_count_for_test();
        exact_index.rebuild_v5(&journal, &roots).unwrap();
        let (decodes, materializations, snapshots, sqlite, serializations) =
            crate::index::v5_post_phase0_counts_for_test();
        assert_eq!(decodes, 0);
        assert_eq!((materializations, snapshots), (1, 1));
        assert!(sqlite > 0);
        assert!(serializations > 0);
        let index = crate::DerivedIndexV5::open(&root).unwrap();
        let rebuild = index.rebuild_v5(&journal, &roots).unwrap();
        assert_eq!(rebuild.event_count, count);
        let rebuilt_again = index.rebuild_v5(&journal, &roots).unwrap();
        assert_eq!(rebuilt_again.image_hash, rebuild.image_hash);
        let snapshot = index.snapshot_current_v5(&journal, &roots).unwrap();
        assert_eq!(
            &snapshot.authority_replay_basis_digest,
            replayed_basis.basis_digest()
        );
        assert_eq!(snapshot.artifact_registrations_v4.len(), 2);
        assert_eq!(snapshot.gluing_input_descriptors.len(), 2);
        assert_eq!(snapshot.context_covers.len(), 1);
        assert!(!snapshot.sections.is_empty());
        assert_eq!(snapshot.gluing_attempts.len(), 1);
        assert_eq!(
            snapshot.global_candidates.len() + snapshot.gluing_obstructions.len(),
            1
        );

        let tamper_cases = [
            "UPDATE events SET actor='tampered:v5-actor' WHERE sequence=(SELECT MAX(sequence) FROM events)",
            "UPDATE artifact_registrations_v4 SET source_canonical_json='{}' WHERE rowid=(SELECT MIN(rowid) FROM artifact_registrations_v4)",
            "UPDATE artifact_registrations_v4 SET body_hash='sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' WHERE rowid=(SELECT MIN(rowid) FROM artifact_registrations_v4)",
            "UPDATE gluing_input_descriptors_v4 SET descriptor_hash='sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' WHERE rowid=(SELECT MIN(rowid) FROM gluing_input_descriptors_v4)",
            "UPDATE gluing_input_descriptors_v4 SET registration_id='registration:missing-v5-reciprocal' WHERE rowid=(SELECT MIN(rowid) FROM gluing_input_descriptors_v4)",
            "UPDATE gluing_input_descriptors_v4 SET qualification_source_ids_canonical_json='[ ]' WHERE rowid=(SELECT MIN(rowid) FROM gluing_input_descriptors_v4)",
            "UPDATE gluing_input_descriptors_v4 SET event_sequence=(SELECT MAX(event_sequence) FROM gluing_input_descriptors_v4), event_id=(SELECT event_id FROM gluing_input_descriptors_v4 ORDER BY event_sequence DESC LIMIT 1) WHERE context_id='context:payment'",
            "UPDATE context_covers_v4 SET event_sequence=1,event_id=(SELECT event_id FROM events WHERE sequence=1),body_hash='sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc'",
            "UPDATE sections_v4 SET source_ids_canonical_json='[ ]',body_hash='sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd' WHERE rowid=(SELECT MIN(rowid) FROM sections_v4)",
            "UPDATE restrictions_v4 SET attempt_id='attempt:missing-v5',body_hash='sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee' WHERE rowid=(SELECT MIN(rowid) FROM restrictions_v4)",
            "UPDATE gluing_attempts_v4 SET result='failed',body_hash='sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff'",
            "UPDATE gluing_obstructions_v4 SET blocks_canonical_json='[ ]',body_hash='sha256:1111111111111111111111111111111111111111111111111111111111111111'",
            "INSERT INTO global_candidates_v4 SELECT event_sequence,event_id,attempt_id,'global-candidate:unexpected-v5','reviewgraphen.global_candidate.v4',cover_id,'invariant:payment-at-most-once','payment.at_most_once','[]','[]','[]','[]','[]','[]','[]','[]','[]','sha256:2222222222222222222222222222222222222222222222222222222222222222' FROM gluing_attempts_v4",
        ];
        for mutation in tamper_cases {
            index.rebuild_v5(&journal, &roots).unwrap();
            mutate_active_v5_image(&root, mutation);
            assert!(matches!(
                index.snapshot_current_v5(&journal, &roots),
                Err(crate::IndexError::CorruptIndex)
            ));
        }
    }

    #[test]
    fn stale_v5_image_with_m5_row_tamper_is_corrupt_not_stale() {
        let (_workspace, root) = root();
        let (journal, roots, [mut payment, mut ui]) =
            public_v4_gluing_journal(&root, "run:journal-v4-stale-m5-tamper");
        let (mut session, mut basis) = journal.replayed_v4_session(&roots).unwrap();
        assert!(matches!(
            session
                .publish_gluing_input(
                    payment.sources.remove(0),
                    payment.descriptor.clone(),
                    &mut basis,
                )
                .unwrap(),
            GluingInputPublicationV4::Confirmed { .. }
        ));
        assert!(matches!(
            session
                .publish_gluing_input(ui.sources.remove(0), ui.descriptor.clone(), &mut basis)
                .unwrap(),
            GluingInputPublicationV4::Confirmed { .. }
        ));
        drop(session);

        let index = crate::DerivedIndexV5::open(&root).unwrap();
        let stale_receipt = index.rebuild_v5(&journal, &roots).unwrap();
        let (mut session, mut basis) = journal.replayed_v4_session(&roots).unwrap();
        let bundle = session.mint_gluing_bundle(&basis).unwrap();
        session.append_gluing_bundle(bundle, &mut basis).unwrap();
        assert!(basis.confirmed_event_count() > stale_receipt.event_count);
        drop(session);

        mutate_active_v5_image(
            &root,
            "UPDATE artifact_registrations_v4 SET body_hash='sha256:3333333333333333333333333333333333333333333333333333333333333333' WHERE rowid=(SELECT MIN(rowid) FROM artifact_registrations_v4)",
        );
        assert!(matches!(
            index.snapshot_current_v5(&journal, &roots),
            Err(crate::IndexError::CorruptIndex)
        ));
    }

    #[test]
    fn v4_profile_session_keeps_root_run_and_journal_locks_through_bundle() {
        let (_workspace, root) = root();
        let (journal, _roots, [payment, ui]) =
            public_v4_gluing_journal(&root, "run:journal-v4-profile-session-locks");
        let assignments = M5DoubleSubmitAssignmentsV4::new(
            payment.descriptor.assignment_value(),
            ui.descriptor.assignment_value(),
        );
        let bundle = journal
            .with_m5_gluing_profile_session(
                public_v4_profile_base_roots(),
                assignments,
                |profile| {
                    assert_eq!(profile.remaining_input_count(), 2);
                    assert!(!profile.source_ids().is_empty());
                    assert!(try_acquire_v4_root_lock(&root).unwrap().is_none());
                    let competing = journal.open_file(false).unwrap();
                    assert_eq!(
                        fs::flock(&competing, FlockOperation::NonBlockingLockExclusive),
                        Err(rustix::io::Errno::WOULDBLOCK)
                    );
                    assert!(matches!(
                        profile.publish_next_gluing_input()?,
                        Some(GluingInputPublicationV4::Confirmed { .. })
                    ));
                    assert!(matches!(
                        profile.publish_next_gluing_input()?,
                        Some(GluingInputPublicationV4::Confirmed { .. })
                    ));
                    assert_eq!(profile.remaining_input_count(), 0);
                    profile.append_gluing_bundle()
                },
            )
            .unwrap();
        assert_eq!(
            bundle.core().result(),
            reviewgraphen_core::GluingResultV4::Unknown
        );
        assert!(try_acquire_v4_root_lock(&root).unwrap().is_some());
    }

    #[test]
    fn incremental_pair_locks_both_run_orders_and_retains_both_journals() {
        for source_run in ["run:a-source-before-target", "run:z-source-after-target"] {
            let (_workspace, root) = root();
            let (source, roots, _) = public_v4_gluing_journal(&root, source_run);
            let target_log = crate::index::v6::tests::planned_target(&root);
            let (target, _) = EventJournal::publish_new_v5(&root, target_log).unwrap();
            let (source_session, _, target_session) = source
                .replayed_incremental_pair_v5(&target, &roots)
                .unwrap();
            let source_competing = source.open_file(false).unwrap();
            let target_competing = target.open_file(false).unwrap();
            assert_eq!(
                fs::flock(&source_competing, FlockOperation::NonBlockingLockExclusive),
                Err(rustix::io::Errno::WOULDBLOCK)
            );
            assert_eq!(
                fs::flock(&target_competing, FlockOperation::NonBlockingLockExclusive),
                Err(rustix::io::Errno::WOULDBLOCK)
            );
            drop(target_session);
            drop(source_session);
            fs::flock(&source_competing, FlockOperation::NonBlockingLockExclusive).unwrap();
            fs::flock(&target_competing, FlockOperation::NonBlockingLockExclusive).unwrap();
        }
    }

    #[test]
    fn incremental_dual_session_peak_and_source_prefix_limits_are_exact() {
        let exact = DualSessionAccountingV5::checked([
            MAX_INCREMENTAL_SESSION_WORKING_BYTES,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ])
        .unwrap();
        assert_eq!(exact.peak_bytes, MAX_INCREMENTAL_SESSION_WORKING_BYTES);
        assert!(matches!(
            DualSessionAccountingV5::checked([
                MAX_INCREMENTAL_SESSION_WORKING_BYTES,
                1,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ]),
            Err(IncrementalSessionError::Incomplete { observed, .. })
                if observed == MAX_INCREMENTAL_SESSION_WORKING_BYTES + 1
        ));
        assert!(matches!(
            DualSessionAccountingV5::checked([u64::MAX, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(IncrementalSessionError::Incomplete {
                observed: u64::MAX,
                ..
            })
        ));
        assert_eq!(
            admit_incremental_source_journal_bytes(MAX_INCREMENTAL_JOURNAL_PREFIX_BYTES).unwrap(),
            MAX_INCREMENTAL_JOURNAL_PREFIX_BYTES
        );
        assert!(matches!(
            admit_incremental_source_journal_bytes(MAX_INCREMENTAL_JOURNAL_PREFIX_BYTES + 1),
            Err(JournalError::Incomplete { observed, .. })
                if observed == MAX_INCREMENTAL_JOURNAL_PREFIX_BYTES + 1
        ));
    }

    fn git(repo: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(repo)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn commit_ingest_e2e_tree(repo: &Path, source: &str, message: &str) -> String {
        std::fs::write(repo.join("src/lib.rs"), source).unwrap();
        git(repo, &["add", "src/lib.rs"]);
        git(repo, &["commit", "--quiet", "--message", message]);
        git(repo, &["rev-parse", "HEAD"])
    }

    fn with_ingest_e2e_m5_profile(program: &ProgramSpace) -> ProgramSpace {
        let mut value = serde_json::to_value(program).unwrap();
        value["profile"]["id"] = Value::String("double-submit-payment".to_owned());
        value["profile"]["version"] = Value::String("1".to_owned());
        let provenance = value["artifacts"][0]["provenance"].clone();
        let rust_functions = value["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|artifact| artifact["kind"] == "function")
            .map(|artifact| artifact["id"].clone())
            .collect::<Vec<_>>();
        assert_eq!(rust_functions.len(), 2);
        value["artifacts"].as_array_mut().unwrap().extend([
            serde_json::json!({
                "id": reviewgraphen_core::DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID,
                "kind": "requirement",
                "label": "M5 fixed overlap anchor for the ingest mapping E2E",
                "attributes": {"test_profile_scaffold": true},
                "provenance": provenance,
            }),
            serde_json::json!({
                "id": "event:ingest-e2e-submit",
                "kind": "event",
                "label": "M5 ingest E2E submit event",
                "attributes": {"test_profile_scaffold": true},
                "provenance": provenance,
            }),
            serde_json::json!({
                "id": "function:ingest-e2e-payment",
                "kind": "requirement",
                "label": "M5 ingest E2E payment step",
                "attributes": {"test_profile_scaffold": true},
                "provenance": provenance,
            }),
            serde_json::json!({
                "id": "external-service:ingest-e2e-provider",
                "kind": "external_service",
                "label": "M5 ingest E2E payment provider",
                "attributes": {
                    "external_side_effect": true,
                    "test_profile_scaffold": true
                },
                "provenance": provenance,
            }),
        ]);
        let scaffold_relations = [
            serde_json::json!({
                "id": "relation:ingest-e2e-submit-handler",
                "kind": "handled_by",
                "source_id": "event:ingest-e2e-submit",
                "target_ids": [reviewgraphen_core::DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID],
                "directed": true,
                "attributes": {
                    "concurrency": "unbounded_reentry",
                    "test_profile_scaffold": true
                },
                "provenance": provenance,
            }),
            serde_json::json!({
                "id": "relation:ingest-e2e-submit-payment",
                "kind": "calls",
                "source_id": reviewgraphen_core::DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID,
                "target_ids": ["function:ingest-e2e-payment"],
                "directed": true,
                "attributes": {"test_profile_scaffold": true},
                "provenance": provenance,
            }),
            serde_json::json!({
                "id": "relation:ingest-e2e-payment-provider",
                "kind": "calls",
                "source_id": "function:ingest-e2e-payment",
                "target_ids": ["external-service:ingest-e2e-provider"],
                "directed": true,
                "attributes": {
                    "idempotency_key_forwarded": false,
                    "test_profile_scaffold": true
                },
                "provenance": provenance,
            }),
        ];
        for mut relation in scaffold_relations {
            relation["ordered_target_ids"] = relation["target_ids"].clone();
            value["relations"].as_array_mut().unwrap().push(relation);
        }
        let mut members = vec![Value::String(
            reviewgraphen_core::DOUBLE_SUBMIT_REQUIRED_OVERLAP_ID.to_owned(),
        )];
        members.extend(rust_functions);
        members.extend([
            Value::String("relation:ingest-e2e-submit-handler".to_owned()),
            Value::String("relation:ingest-e2e-submit-payment".to_owned()),
            Value::String("relation:ingest-e2e-payment-provider".to_owned()),
        ]);
        members.sort_by(|left, right| left.as_str().unwrap().cmp(right.as_str().unwrap()));
        value["contexts"] = serde_json::json!([
            {
                "id": reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
                "kind": "review_context",
                "label": "Payment context for the ingest mapping E2E",
                "member_ids": members,
                "attributes": {"test_profile_scaffold": true},
                "provenance": provenance,
            },
            {
                "id": reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID,
                "kind": "review_context",
                "label": "UI context for the ingest mapping E2E",
                "member_ids": members,
                "attributes": {"test_profile_scaffold": true},
                "provenance": provenance,
            }
        ]);
        value["invariants"] = serde_json::json!([{
            "id": reviewgraphen_core::DOUBLE_SUBMIT_INVARIANT_ID,
            "property_id": reviewgraphen_core::DOUBLE_SUBMIT_PROPERTY_ID,
            "description": "Fixed M5 invariant used only to complete the source review seam.",
            "scope_ids": [
                reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
                reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID
            ],
            "severity": "critical",
            "verification_mode": "mapping_e2e_profile_scaffold",
            "provenance": provenance,
        }]);
        ProgramSpace::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    fn ingest_e2e_profile_roots(program: &ProgramSpace) -> AuthorityTrustRootsV4 {
        AuthorityTrustRootsV4::new(
            ContentHash::sha256(b"ingest-store-mapping-e2e-policy"),
            program.repository_id().clone(),
            program
                .accepted_git_revision_closure()
                .unwrap()
                .target_tree_hash()
                .clone(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap()
    }

    #[test]
    fn real_git_ingest_pair_maps_unchanged_and_formatting_only_rust_facts_as_preserved() {
        let repository_workspace = tempfile::tempdir().unwrap();
        let repository = repository_workspace.path().join("repository");
        std::fs::create_dir_all(repository.join("src")).unwrap();
        git(&repository, &["init", "--quiet"]);
        git(&repository, &["config", "user.name", "ReviewGraphen Test"]);
        git(
            &repository,
            &["config", "user.email", "reviewgraphen@example.invalid"],
        );
        std::fs::write(
            repository.join("Cargo.toml"),
            "[package]\nname = \"ingest-store-e2e\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            repository.join("src/lib.rs"),
            "pub fn seed() -> u64 { 0 }\n",
        )
        .unwrap();
        git(&repository, &["add", "Cargo.toml", "src/lib.rs"]);
        git(&repository, &["commit", "--quiet", "--message", "base"]);
        let base = git(&repository, &["rev-parse", "HEAD"]);
        let source_commit = commit_ingest_e2e_tree(
            &repository,
            "pub fn unchanged() -> u64 { 7 }\n\npub fn formatted(value: u64) -> u64 {\n    value + 1\n}\n",
            "source snapshot",
        );
        let target_commit = commit_ingest_e2e_tree(
            &repository,
            "pub fn unchanged() -> u64 { 7 }\n\npub fn formatted(value: u64) -> u64 {\n    value  +  1\n}\n",
            "formatting only",
        );
        let source_ingest = ingest_with_sources(
            &IngestRequest::new(
                repository_workspace.path(),
                &repository,
                "reviewgraphen://ingest-store-mapping-e2e",
                &base,
                &source_commit,
            ),
            1_048_576,
        )
        .unwrap();
        let target_ingest = ingest_with_sources(
            &IngestRequest::new(
                repository_workspace.path(),
                &repository,
                "reviewgraphen://ingest-store-mapping-e2e",
                &source_commit,
                &target_commit,
            ),
            1_048_576,
        )
        .unwrap();
        assert_eq!(
            source_ingest
                .program_space
                .accepted_git_revision_closure()
                .unwrap()
                .target_commit_oid(),
            target_ingest
                .program_space
                .accepted_git_revision_closure()
                .unwrap()
                .base_commit_oid()
        );

        let source_program = with_ingest_e2e_m5_profile(&source_ingest.program_space);
        let target_program = with_ingest_e2e_m5_profile(&target_ingest.program_space);
        let function_id = |program: &ProgramSpace, suffix: &str| {
            program
                .artifacts()
                .iter()
                .find(|artifact| artifact.kind == "function" && artifact.label.ends_with(suffix))
                .unwrap()
                .id
                .clone()
        };
        let unchanged_id = function_id(&source_program, "::unchanged");
        let formatted_id = function_id(&source_program, "::formatted");
        let target_formatted_id = function_id(&target_program, "::formatted");
        assert_eq!(
            &source_program.accepted_rust_symbol_anchors().unwrap()[&formatted_id],
            &target_program.accepted_rust_symbol_anchors().unwrap()[&target_formatted_id],
            "Ingest's accepted Rust anchor must normalize formatting-only edits",
        );

        let (_workspace, root) = root();
        let (source, roots) = publish_ingest_source_v4(
            &root,
            "run:ingest-store-source",
            source_program.clone(),
            &source_ingest.source_bundle,
        );
        let assignments = M5DoubleSubmitAssignmentsV4::new(
            AssignmentValueV4::Unknown,
            AssignmentValueV4::Unknown,
        );
        source
            .with_m5_gluing_profile_session(roots, assignments, |profile| {
                assert!(profile.publish_next_gluing_input()?.is_some());
                assert!(profile.publish_next_gluing_input()?.is_some());
                profile.append_gluing_bundle().map(|_| ())
            })
            .unwrap();
        let authority = match source
            .inspect_m5_report_authority_v4(ingest_e2e_profile_roots(&source_program), assignments)
            .unwrap()
        {
            M5ReportAuthorityInspectionV4::Complete(authority) => authority,
            M5ReportAuthorityInspectionV4::Incomplete { .. } => panic!("M5 must be complete"),
        };
        let source_index = crate::DerivedIndexV5::open(&root).unwrap();
        authority.rebuild_v5(&source_index, &source).unwrap();

        let target_log = planned_ingest_target_v5(
            &root,
            "run:ingest-store-target",
            target_program,
            &target_ingest.source_bundle,
        );
        let (target, _) = EventJournal::publish_new_v5(&root, target_log).unwrap();
        let target_index = crate::DerivedIndexV6::open(&root).unwrap();
        target_index.rebuild_pre_incremental_v6(&target).unwrap();
        let accepted = authority
            .derive_incremental_mapping_v5(&source, &source_index, &target, &target_index)
            .unwrap();

        let mapping_for = |source_id: &StableId| {
            accepted
                .mapping_phase()
                .mappings()
                .iter()
                .find(|mapping| mapping.from_ids().contains(source_id))
                .unwrap()
        };
        assert_eq!(
            mapping_for(&unchanged_id).status(),
            reviewgraphen_core::MappingStatusV5::Preserved
        );
        let formatted = mapping_for(&formatted_id);
        assert_eq!(
            formatted.status(),
            reviewgraphen_core::MappingStatusV5::Preserved
        );
        assert_eq!(
            formatted.candidate_key_kind(),
            reviewgraphen_core::CandidateKeyKindV5::SamePath
        );
    }

    #[test]
    fn incremental_authority_refuses_incomplete_m5_and_cross_root_target() {
        let (_workspace, root) = root();
        let (source, _roots, [payment, ui]) =
            public_v4_gluing_journal(&root, "run:incremental-authority-source");
        let assignments = M5DoubleSubmitAssignmentsV4::new(
            payment.descriptor.assignment_value(),
            ui.descriptor.assignment_value(),
        );
        for expected in 0..=2_u64 {
            assert!(matches!(
                source
                    .inspect_m5_report_authority_v4(
                        public_v4_profile_base_roots(),
                        assignments,
                    )
                    .unwrap(),
                M5ReportAuthorityInspectionV4::Incomplete { registered_inputs }
                    if registered_inputs == expected
            ));
            if expected < 2 {
                source
                    .with_m5_gluing_profile_session(
                        public_v4_profile_base_roots(),
                        assignments,
                        |profile| {
                            assert!(profile.publish_next_gluing_input()?.is_some());
                            Ok(())
                        },
                    )
                    .unwrap();
            }
        }
        source
            .with_m5_gluing_profile_session(
                public_v4_profile_base_roots(),
                assignments,
                |profile| {
                    assert_eq!(profile.remaining_input_count(), 0);
                    profile.append_gluing_bundle().map(|_| ())
                },
            )
            .unwrap();
        let authority = match source
            .inspect_m5_report_authority_v4(public_v4_profile_base_roots(), assignments)
            .unwrap()
        {
            M5ReportAuthorityInspectionV4::Complete(authority) => authority,
            M5ReportAuthorityInspectionV4::Incomplete { .. } => panic!("M5 must be complete"),
        };
        let source_index = crate::DerivedIndexV5::open(&root).unwrap();
        authority.rebuild_v5(&source_index, &source).unwrap();

        let same_root_target_log = crate::index::v6::tests::planned_target(&root);
        let (same_root_target, _) =
            EventJournal::publish_new_v5(&root, same_root_target_log).unwrap();
        let same_root_target_index = crate::DerivedIndexV6::open(&root).unwrap();
        same_root_target_index
            .rebuild_pre_incremental_v6(&same_root_target)
            .unwrap();
        assert!(matches!(
            authority.derive_incremental_mapping_v5(
                &source,
                &source_index,
                &same_root_target,
                &same_root_target_index,
            ),
            Err(IncrementalSessionError::Authority(
                "source ProgramSpace lacks accepted Git revision closure"
            ))
        ));

        let foreign_workspace = tempfile::tempdir().unwrap();
        let foreign_root =
            StoreRoot::open(foreign_workspace.path(), crate::StoreLimits::default()).unwrap();
        let target_log = crate::index::v6::tests::planned_target(&foreign_root);
        let (target, _) = EventJournal::publish_new_v5(&foreign_root, target_log).unwrap();
        let target_index = crate::DerivedIndexV6::open(&foreign_root).unwrap();
        target_index.rebuild_pre_incremental_v6(&target).unwrap();
        assert!(matches!(
            authority
                .derive_incremental_mapping_v5(&source, &source_index, &target, &target_index,),
            Err(IncrementalSessionError::Authority(_))
        ));
    }

    #[test]
    fn completed_m5_and_complete_v3_target_derive_locked_incremental_mapping() {
        let (_workspace, root) = root();
        let (source, _roots, [payment, ui]) =
            public_v4_gluing_journal_incremental_v3(&root, "run:incremental-v3-source");
        let assignments = M5DoubleSubmitAssignmentsV4::new(
            payment.descriptor.assignment_value(),
            ui.descriptor.assignment_value(),
        );
        source
            .with_m5_gluing_profile_session(
                incremental_v3_profile_base_roots(),
                assignments,
                |profile| {
                    assert!(profile.publish_next_gluing_input()?.is_some());
                    assert!(profile.publish_next_gluing_input()?.is_some());
                    profile.append_gluing_bundle().map(|_| ())
                },
            )
            .unwrap();
        let authority = match source
            .inspect_m5_report_authority_v4(incremental_v3_profile_base_roots(), assignments)
            .unwrap()
        {
            M5ReportAuthorityInspectionV4::Complete(authority) => authority,
            M5ReportAuthorityInspectionV4::Incomplete { .. } => panic!("M5 must be complete"),
        };
        let source_index = crate::DerivedIndexV5::open(&root).unwrap();
        authority.rebuild_v5(&source_index, &source).unwrap();

        let target_log = crate::index::v6::tests::planned_target(&root);
        let (target, _) = EventJournal::publish_new_v5(&root, target_log).unwrap();
        let target_index = crate::DerivedIndexV6::open(&root).unwrap();
        target_index.rebuild_pre_incremental_v6(&target).unwrap();

        let accepted = authority
            .derive_incremental_mapping_v5(&source, &source_index, &target, &target_index)
            .unwrap();
        assert_eq!(
            accepted.morphism().source_closure_id(),
            accepted.closure().id()
        );
        assert!(!accepted.mapping_phase().mappings().is_empty());
        let proof = &accepted.proof;
        let buffers = proof._live_buffers.as_ref().unwrap();
        let source_line = u64::try_from(buffers.source_event_line.len()).unwrap();
        let target_line = u64::try_from(buffers.target_event_line.len()).unwrap();
        let oracle = [
            u64::try_from(proof.source_journal_bytes.len()).unwrap(),
            u64::try_from(proof.target_journal_bytes.len()).unwrap(),
            u64::try_from(proof.source_index_canonical_bytes.len()).unwrap(),
            u64::try_from(proof.target_index.canonical_snapshot_bytes().len()).unwrap(),
            proof._source_index.recursive_owned_bytes().unwrap(),
            proof.target_index.decoded_owned_bytes(),
            u64::try_from(buffers.source_cas.len()).unwrap(),
            u64::try_from(proof.target_index.largest_verified_cas_bytes().len()).unwrap(),
            proof._source_session.retained_event_bytes().unwrap() + source_line,
            proof.target_index.retained_event_bytes().unwrap() + target_line,
            u64::try_from(accepted.mapping_phase().working_peak_upper_bound_bytes()).unwrap(),
        ];
        assert_eq!(
            accepted.working_peak_bytes(),
            oracle.into_iter().sum::<u64>()
        );
        assert_eq!(
            buffers.source_event_line,
            retain_largest_canonical_event_line(&proof.source_journal_bytes, source_line,).unwrap()
        );
        assert_eq!(
            buffers.target_event_line,
            retain_largest_canonical_event_line(&proof.target_journal_bytes, target_line,).unwrap()
        );
    }

    #[test]
    fn incremental_session_refuses_accepted_git_oid_and_tree_discontinuity() {
        fn run_case(target_base_oid: &str, target_base_tree: &str) {
            let (_workspace, root) = root();
            let (source, _roots, [payment, ui]) = public_v4_gluing_journal_incremental_v3(
                &root,
                "run:incremental-v3-discontinuous-source",
            );
            let assignments = M5DoubleSubmitAssignmentsV4::new(
                payment.descriptor.assignment_value(),
                ui.descriptor.assignment_value(),
            );
            source
                .with_m5_gluing_profile_session(
                    incremental_v3_profile_base_roots(),
                    assignments,
                    |profile| {
                        assert!(profile.publish_next_gluing_input()?.is_some());
                        assert!(profile.publish_next_gluing_input()?.is_some());
                        profile.append_gluing_bundle().map(|_| ())
                    },
                )
                .unwrap();
            let authority = match source
                .inspect_m5_report_authority_v4(incremental_v3_profile_base_roots(), assignments)
                .unwrap()
            {
                M5ReportAuthorityInspectionV4::Complete(authority) => authority,
                M5ReportAuthorityInspectionV4::Incomplete { .. } => panic!("M5 must be complete"),
            };
            let source_index = crate::DerivedIndexV5::open(&root).unwrap();
            authority.rebuild_v5(&source_index, &source).unwrap();
            let target_log = crate::index::v6::tests::planned_target_with_base(
                &root,
                target_base_oid,
                target_base_tree,
            );
            let (target, _) = EventJournal::publish_new_v5(&root, target_log).unwrap();
            let target_index = crate::DerivedIndexV6::open(&root).unwrap();
            target_index.rebuild_pre_incremental_v6(&target).unwrap();
            assert!(matches!(
                authority.derive_incremental_mapping_v5(
                    &source,
                    &source_index,
                    &target,
                    &target_index,
                ),
                Err(IncrementalSessionError::Authority(
                    "accepted source target and target base Git revisions differ"
                ))
            ));
        }

        run_case(
            "9999999999999999999999999999999999999999",
            "git:3333333333333333333333333333333333333333",
        );
        run_case(
            "1111111111111111111111111111111111111111",
            "git:9999999999999999999999999999999999999999",
        );
    }

    #[test]
    fn v4_gluing_cas_crash_boundary_preserves_no_event_and_adopts_only_exact_bytes() {
        for (index, fault) in [
            V4GluingFault::BeforeCasPublication,
            V4GluingFault::AfterCasPublication,
        ]
        .into_iter()
        .enumerate()
        {
            let (_workspace, root) = root();
            let run = format!("run:journal-v4-gluing-cas-fault-{index}");
            let (journal, roots, [mut payment, _]) = public_v4_gluing_journal(&root, &run);
            let (mut session, mut basis) = journal.replayed_v4_session(&roots).unwrap();
            let before = basis.confirmed_event_count();
            let hash = payment.sources[0].descriptor_hash().clone();
            let cas_hash = CasHash::parse(hash.to_string()).unwrap();
            inject_v4_gluing_fault(fault);
            assert!(matches!(
                session.publish_gluing_input(
                    payment.sources.remove(0),
                    payment.descriptor.clone(),
                    &mut basis,
                ),
                Err(JournalError::GluingInputPublicationInterruptedV4 { .. })
            ));
            assert_eq!(basis.confirmed_event_count(), before);
            assert_eq!(basis.gluing_input_entry_count(), 0);
            assert!(
                session
                    .log
                    .registered_gluing_input_projection_v4(payment.descriptor.context_id())
                    .is_none()
            );
            let reader = CasReader::open_existing(&root).unwrap();
            match fault {
                V4GluingFault::BeforeCasPublication => {
                    let mut destination =
                        vec![0; canonical_json(&payment.descriptor).unwrap().len()];
                    assert!(matches!(
                        reader.read_exact_slice(&cas_hash, &mut destination),
                        Err(StoreError::MissingArtifact)
                    ));
                    let confirmed = session
                        .publish_gluing_input(
                            payment.sources.remove(0),
                            payment.descriptor,
                            &mut basis,
                        )
                        .unwrap();
                    assert!(matches!(
                        confirmed,
                        GluingInputPublicationV4::Confirmed { ref cas, .. } if !cas.existed
                    ));
                }
                V4GluingFault::AfterCasPublication => {
                    let expected = canonical_json(&payment.descriptor).unwrap();
                    let mut observed = vec![0; expected.len()];
                    reader.read_exact_slice(&cas_hash, &mut observed).unwrap();
                    assert_eq!(observed, expected);
                    let mut colliding = expected.clone();
                    colliding[0] ^= 1;
                    assert!(matches!(
                        CasStore::open(&root).unwrap().put(
                            &cas_hash,
                            Some(colliding.len() as u64),
                            colliding.as_slice(),
                        ),
                        Err(StoreError::HashMismatch)
                    ));
                    reader.read_exact_slice(&cas_hash, &mut observed).unwrap();
                    assert_eq!(observed, expected);
                    let adopted = session
                        .adopt_gluing_input(payment.sources.remove(0), &mut basis)
                        .unwrap();
                    assert!(matches!(
                        adopted,
                        GluingInputPublicationV4::Confirmed { ref cas, .. } if cas.existed
                    ));
                }
            }
            assert_eq!(basis.confirmed_event_count(), before + 1);
            assert_eq!(basis.gluing_input_entry_count(), 1);
        }

        let (_workspace, root) = root();
        let (journal, roots, [_, mut ui]) =
            public_v4_gluing_journal(&root, "run:journal-v4-gluing-out-of-order-orphan");
        let (mut session, mut basis) = journal.replayed_v4_session(&roots).unwrap();
        let ui_hash = ui.sources[0].descriptor_hash().clone();
        let ui_cas_hash = CasHash::parse(ui_hash.to_string()).unwrap();
        let ui_bytes = canonical_json(&ui.descriptor).unwrap();
        assert!(matches!(
            session.publish_gluing_input(ui.sources.remove(0), ui.descriptor.clone(), &mut basis,),
            Err(JournalError::Domain(
                reviewgraphen_core::DomainError::GluingInputAdmissionMismatch
            ))
        ));
        let reader = CasReader::open_existing(&root).unwrap();
        let mut destination = vec![0; ui_bytes.len()];
        assert!(matches!(
            reader.read_exact_slice(&ui_cas_hash, &mut destination),
            Err(StoreError::MissingArtifact)
        ));
        CasStore::open(&root)
            .unwrap()
            .put(
                &ui_cas_hash,
                Some(ui_bytes.len() as u64),
                ui_bytes.as_slice(),
            )
            .unwrap();
        assert!(matches!(
            session.adopt_gluing_input(ui.sources.remove(0), &mut basis),
            Err(JournalError::OrphanCanonicalInput { ref hash }) if hash == &ui_hash
        ));
        assert_eq!(basis.gluing_input_entry_count(), 0);
    }

    #[test]
    fn v4_gluing_registration_post_sync_uncertainty_requires_keyed_tail_recovery() {
        let (_workspace, root) = root();
        let (journal, roots, [mut payment, ui]) =
            public_v4_gluing_journal(&root, "run:journal-v4-gluing-registration-uncertain");
        let run_id = journal.identity.run_id.clone();
        let genesis_hash = journal.identity.genesis_hash();
        let (mut session, mut basis) = journal.replayed_v4_session(&roots).unwrap();
        let before = basis.confirmed_event_count();
        session
            .writer
            .inject_faults([AppendFault::ClearMarkerDirectorySync]);
        assert!(matches!(
            session.publish_gluing_input(
                payment.sources.remove(0),
                payment.descriptor.clone(),
                &mut basis,
            ),
            Err(JournalError::SessionUncertain)
        ));
        assert_eq!(basis.confirmed_event_count(), before);
        assert_eq!(basis.gluing_input_entry_count(), 0);
        assert!(bundle_file_exists(&journal.run, APPEND_PENDING_MARKER).unwrap());
        drop(session);

        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(run_id, genesis_hash, RecoveryKindV4::CanonicalTail),
        )
        .unwrap();
        let assignments = M5DoubleSubmitAssignmentsV4::new(
            payment.descriptor.assignment_value(),
            ui.descriptor.assignment_value(),
        );
        let recovered = journal
            .recover_with_m5_gluing_profile_session(
                public_v4_profile_base_roots(),
                assignments,
                key,
                RecoveryProvenanceV4::new("test", "v4-gluing-registration-uncertain").unwrap(),
                |profile| {
                    assert_eq!(profile.remaining_input_count(), 1);
                    assert!(try_acquire_v4_root_lock(&root).unwrap().is_none());
                    assert!(matches!(
                        profile.publish_next_gluing_input()?,
                        Some(GluingInputPublicationV4::Confirmed { .. })
                    ));
                    profile.append_gluing_bundle()
                },
            )
            .unwrap();
        let M5GluingProfileRecoveryV4::Continued {
            recovery: receipt,
            value: bundle,
        } = recovered
        else {
            panic!("one-registration prefix must continue the M5 profile")
        };
        assert!(matches!(
            receipt.outcome(),
            RecoveryOutcomeV4::TailRecovered { .. }
        ));
        assert!(!bundle_file_exists(&journal.run, APPEND_PENDING_MARKER).unwrap());
        assert_eq!(
            bundle.core().result(),
            reviewgraphen_core::GluingResultV4::Unknown
        );
        assert!(try_acquire_v4_root_lock(&root).unwrap().is_some());
    }

    #[test]
    fn v4_gluing_bundle_post_sync_recovery_returns_verified_complete_without_callback() {
        let (_workspace, root) = root();
        let (journal, _roots, [payment, ui]) =
            public_v4_gluing_journal(&root, "run:journal-v4-profile-bundle-uncertain");
        let assignments = M5DoubleSubmitAssignmentsV4::new(
            payment.descriptor.assignment_value(),
            ui.descriptor.assignment_value(),
        );
        let run_id = journal.identity.run_id.clone();
        let genesis_hash = journal.identity.genesis_hash();
        assert!(matches!(
            journal.with_m5_gluing_profile_session(
                public_v4_profile_base_roots(),
                assignments,
                |profile| {
                    assert!(profile.publish_next_gluing_input()?.is_some());
                    assert!(profile.publish_next_gluing_input()?.is_some());
                    profile
                        .session
                        .writer
                        .inject_faults([AppendFault::ClearMarkerDirectorySync]);
                    profile.append_gluing_bundle()
                },
            ),
            Err(JournalError::SessionUncertain)
        ));

        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(run_id, genesis_hash, RecoveryKindV4::CanonicalTail),
        )
        .unwrap();
        let recovered = journal
            .recover_with_m5_gluing_profile_session(
                public_v4_profile_base_roots(),
                assignments,
                key,
                RecoveryProvenanceV4::new("test", "v4-profile-bundle-uncertain").unwrap(),
                |_| -> Result<(), JournalError> {
                    panic!("a durable recovered bundle must not run the append callback")
                },
            )
            .unwrap();
        let M5GluingProfileRecoveryV4::AlreadyComplete {
            recovery,
            completed,
        } = recovered
        else {
            panic!("post-sync bundle recovery must report verified completion")
        };
        assert!(matches!(
            recovery.outcome(),
            RecoveryOutcomeV4::TailRecovered { .. }
        ));
        assert_eq!(
            completed.result(),
            reviewgraphen_core::GluingResultV4::Unknown
        );
        assert!(completed.obstruction_id().is_some());
        assert!(!completed.obstruction_source_ids().is_empty());
        assert!(!completed.source_ids().is_empty());
        assert!(!bundle_file_exists(&journal.run, APPEND_PENDING_MARKER).unwrap());
        assert!(matches!(
            journal.with_m5_gluing_profile_session(
                public_v4_profile_base_roots(),
                assignments,
                |_| Ok(()),
            ),
            Err(JournalError::Domain(
                reviewgraphen_core::DomainError::AlreadyComplete
            ))
        ));
    }

    #[test]
    fn v4_gluing_tail_recovery_returns_no_session_when_roots_replay_fails() {
        let (_workspace, root) = root();
        let (journal, roots, [mut payment, _]) =
            public_v4_gluing_journal(&root, "run:journal-v4-gluing-recovery-wrong-roots");
        let (mut session, mut basis) = journal.replayed_v4_session(&roots).unwrap();
        session
            .writer
            .inject_faults([AppendFault::ClearMarkerDirectorySync]);
        assert!(matches!(
            session
                .publish_gluing_input(payment.sources.remove(0), payment.descriptor, &mut basis,),
            Err(JournalError::SessionUncertain)
        ));
        drop(session);

        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(
                journal.identity.run_id.clone(),
                journal.identity.genesis_hash(),
                RecoveryKindV4::CanonicalTail,
            ),
        )
        .unwrap();
        let missing_gluing_roots = AuthorityTrustRootsV4::new(
            ContentHash::sha256(b"store-public-v3-fixture-policy"),
            StableId::parse("repository:double-submit-payment").unwrap(),
            ContentHash::parse("sha256:1111111111111111").unwrap(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        assert!(matches!(
            journal.recover_replayed_v4_session(
                &missing_gluing_roots,
                key,
                RecoveryProvenanceV4::new("test", "v4-gluing-wrong-roots").unwrap(),
            ),
            Err(JournalError::Domain(_))
        ));
        assert!(!bundle_file_exists(&journal.run, APPEND_PENDING_MARKER).unwrap());
        assert!(try_acquire_v4_root_lock(&root).unwrap().is_some());
        let (_session, recovered_basis) = journal.replayed_v4_session(&roots).unwrap();
        assert_eq!(recovered_basis.gluing_input_entry_count(), 1);
    }

    #[test]
    fn v4_gluing_bundle_is_one_line_with_rollback_or_keyed_uncertainty() {
        let (_workspace, root) = root();
        let (journal, roots, [mut payment, mut ui]) =
            public_v4_gluing_journal(&root, "run:journal-v4-gluing-bundle-uncertain");
        let run_id = journal.identity.run_id.clone();
        let genesis_hash = journal.identity.genesis_hash();
        let (mut session, mut basis) = journal.replayed_v4_session(&roots).unwrap();
        session
            .publish_gluing_input(payment.sources.remove(0), payment.descriptor, &mut basis)
            .unwrap();
        session
            .publish_gluing_input(ui.sources.remove(0), ui.descriptor, &mut basis)
            .unwrap();
        let before = basis.confirmed_event_count();

        let first = session.mint_gluing_bundle(&basis).unwrap();
        session.writer.inject_faults([AppendFault::PartialWrite]);
        assert!(matches!(
            session.append_gluing_bundle(first, &mut basis),
            Err(JournalError::Io(_))
        ));
        assert_eq!(basis.confirmed_event_count(), before);
        assert!(!bundle_file_exists(&journal.run, APPEND_PENDING_MARKER).unwrap());
        assert_eq!(session.writer.events().len() as u64, before);

        let second = session.mint_gluing_bundle(&basis).unwrap();
        session
            .writer
            .inject_faults([AppendFault::ClearMarkerDirectorySync]);
        assert!(matches!(
            session.append_gluing_bundle(second, &mut basis),
            Err(JournalError::SessionUncertain)
        ));
        assert_eq!(basis.confirmed_event_count(), before);
        assert!(bundle_file_exists(&journal.run, APPEND_PENDING_MARKER).unwrap());
        drop(session);

        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(run_id, genesis_hash, RecoveryKindV4::CanonicalTail),
        )
        .unwrap();
        let (receipt, recovered) = journal
            .recover_replayed_v4_session(
                &roots,
                key,
                RecoveryProvenanceV4::new("test", "v4-gluing-bundle-uncertain").unwrap(),
            )
            .unwrap();
        assert!(matches!(
            receipt.outcome(),
            RecoveryOutcomeV4::TailRecovered { .. }
        ));
        let RecoveredV4Session::Editable {
            session: replayed,
            basis: recovered_basis,
        } = recovered
        else {
            panic!("canonical-tail recovery must return an editable session")
        };
        assert_eq!(recovered_basis.confirmed_event_count(), before + 1);
        assert_eq!(recovered_basis.gluing_input_entry_count(), 2);
        assert!(matches!(
            replayed.mint_gluing_bundle(&recovered_basis),
            Err(JournalError::Domain(
                reviewgraphen_core::DomainError::AlreadyComplete
            ))
        ));
        assert!(!bundle_file_exists(&journal.run, BUNDLE_PENDING_MARKER).unwrap());
        assert!(!bundle_file_exists(&journal.run, BUNDLE_PENDING_STAGE).unwrap());
    }

    #[test]
    fn v4_m4_store_refuses_nonprefix_real_plans_and_overlong_suffix_without_cleanup() {
        let (_workspace, root) = root();
        let (journal, _roots, planned) =
            public_v4_static_bundle_journal(&root, "run:journal-v4-m4-real-refusals");
        let before = std::fs::read(
            root.path()
                .join(RUNS_DIR)
                .join(run_dir_name(&journal.identity.run_id))
                .join(JOURNAL_FILE),
        )
        .unwrap();
        for candidate in [
            vec![planned[0].clone(), planned[0].clone(), planned[2].clone()],
            vec![planned[0].clone(), planned[2].clone()],
            vec![planned[1].clone(), planned[0].clone(), planned[2].clone()],
        ] {
            let reader = journal.reader().unwrap();
            let marker = VerificationBundlePendingMarkerV3::new(
                &journal.identity,
                &reader.state,
                &candidate,
            )
            .unwrap();
            drop(reader);
            publish_bundle_file(
                &journal.run,
                BUNDLE_PENDING_MARKER,
                &canonical_json(&marker).unwrap(),
            )
            .unwrap();
            assert!(
                EventJournal::inspect_recovery_v4(
                    &root,
                    RecoveryInspectionV4::new(
                        journal.identity.run_id.clone(),
                        journal.identity.genesis_hash(),
                        RecoveryKindV4::M4BundleResume,
                    ),
                )
                .is_err()
            );
            assert!(bundle_file_exists(&journal.run, BUNDLE_PENDING_MARKER).unwrap());
            assert_eq!(
                std::fs::read(
                    root.path()
                        .join(RUNS_DIR)
                        .join(run_dir_name(&journal.identity.run_id))
                        .join(JOURNAL_FILE),
                )
                .unwrap(),
                before
            );
            fs::unlinkat(&journal.run, BUNDLE_PENDING_MARKER, AtFlags::empty()).unwrap();
            fs::fsync(&journal.run).unwrap();
        }

        let mut writer = journal.writer_v4().unwrap();
        let marker =
            VerificationBundlePendingMarkerV3::new(&journal.identity, &writer.state, &planned[..2])
                .unwrap();
        publish_bundle_file(
            &journal.run,
            BUNDLE_PENDING_MARKER,
            &canonical_json(&marker).unwrap(),
        )
        .unwrap();
        writer.file.seek(SeekFrom::End(0)).unwrap();
        for envelope in &planned {
            writer
                .file
                .write_all(&envelope.canonical_bytes().unwrap())
                .unwrap();
            writer.file.write_all(b"\n").unwrap();
        }
        writer.file.sync_data().unwrap();
        drop(writer);
        assert!(
            EventJournal::inspect_recovery_v4(
                &root,
                RecoveryInspectionV4::new(
                    journal.identity.run_id.clone(),
                    journal.identity.genesis_hash(),
                    RecoveryKindV4::M4BundleResume,
                ),
            )
            .is_err()
        );
        assert!(bundle_file_exists(&journal.run, BUNDLE_PENDING_MARKER).unwrap());
    }

    #[test]
    fn v4_m4_cleanup_directory_sync_failure_is_typed_after_marker_mutation() {
        let (_workspace, root) = root();
        let log = v4_bootstrap_log("run:journal-v4-m4-cleanup-sync");
        let planned = log.envelopes()[0].clone();
        let (journal, _) = EventJournal::publish_new_v4(&root, log).unwrap();
        let marker = v4_marker_at_current_tail(&journal, &[planned]);
        publish_bundle_file(
            &journal.run,
            BUNDLE_PENDING_MARKER,
            &canonical_json(&marker).unwrap(),
        )
        .unwrap();
        publish_bundle_file(&journal.run, BUNDLE_PENDING_STAGE, b"0\n").unwrap();
        let outcome = RecoveryOutcomeV4::M4BundleCleanupOrdinary {
            prefix_stage: M4BundlePrefixStageV4 {
                classification: M4BundlePrefixClassificationV4::Stage0,
                confirmed_events: 0,
                expected_events: 1,
            },
        };
        inject_v4_recovery_fault(V4RecoveryFault::M4Directory);
        assert!(matches!(
            clear_v4_bundle_marker(&journal.run, &outcome),
            Err(JournalError::RecoveryDurabilityUncertainV4 {
                outcome: RecoveryOutcomeV4::M4BundleCleanupOrdinary { .. }
            })
        ));
        assert!(!bundle_file_exists(&journal.run, BUNDLE_PENDING_MARKER).unwrap());
        assert!(!bundle_file_exists(&journal.run, BUNDLE_PENDING_STAGE).unwrap());
    }

    #[test]
    fn v4_recovery_sync_failures_are_typed_after_mutation() {
        let (_workspace, root) = root();
        let log = v4_bootstrap_log("run:journal-v4-genesis-sync");
        let run_id = log.run_id().clone();
        let genesis_hash = log.genesis_hash().clone();
        EventJournal::publish_new_v4(&root, log).unwrap();
        let path = root
            .path()
            .join(RUNS_DIR)
            .join(run_dir_name(&run_id))
            .join(JOURNAL_FILE);
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(0).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(run_id, genesis_hash, RecoveryKindV4::GenesisBootstrap),
        )
        .unwrap();
        inject_v4_recovery_fault(V4RecoveryFault::GenesisDirectory);
        assert!(matches!(
            EventJournal::recover_new_v4(
                &root,
                key,
                RecoveryProvenanceV4::new("test", "genesis-sync").unwrap(),
            ),
            Err(JournalError::RecoveryDurabilityUncertainV4 {
                outcome: RecoveryOutcomeV4::GenesisNotCommitted
            })
        ));
        assert!(!path.exists());

        let log = v4_bootstrap_log("run:journal-v4-tail-sync");
        let run_id = log.run_id().clone();
        let genesis_hash = log.genesis_hash().clone();
        EventJournal::publish_new_v4(&root, log).unwrap();
        let path = root
            .path()
            .join(RUNS_DIR)
            .join(run_dir_name(&run_id))
            .join(JOURNAL_FILE);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"{").unwrap();
        file.sync_all().unwrap();
        drop(file);
        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(
                run_id.clone(),
                genesis_hash.clone(),
                RecoveryKindV4::CanonicalTail,
            ),
        )
        .unwrap();
        inject_v4_recovery_fault(V4RecoveryFault::TailFile);
        assert!(matches!(
            EventJournal::recover_canonical_tail_v4(
                &root,
                key,
                RecoveryProvenanceV4::new("test", "tail-sync").unwrap(),
            ),
            Err(JournalError::RecoveryDurabilityUncertainV4 {
                outcome: RecoveryOutcomeV4::TailRecovered { .. }
            })
        ));
        let retry_key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(run_id, genesis_hash, RecoveryKindV4::CanonicalTail),
        )
        .unwrap();
        EventJournal::recover_canonical_tail_v4(
            &root,
            retry_key,
            RecoveryProvenanceV4::new("test", "tail-sync-retry").unwrap(),
        )
        .unwrap();
        assert_eq!(std::fs::read(path).unwrap().last(), Some(&b'\n'));

        let log = v4_bootstrap_log("run:journal-v4-tail-directory-sync");
        let run_id = log.run_id().clone();
        let genesis_hash = log.genesis_hash().clone();
        EventJournal::publish_new_v4(&root, log).unwrap();
        let run_path = root.path().join(RUNS_DIR).join(run_dir_name(&run_id));
        let path = run_path.join(JOURNAL_FILE);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"{").unwrap();
        file.sync_all().unwrap();
        drop(file);
        let key = EventJournal::inspect_recovery_v4(
            &root,
            RecoveryInspectionV4::new(run_id, genesis_hash, RecoveryKindV4::CanonicalTail),
        )
        .unwrap();
        inject_v4_recovery_fault(V4RecoveryFault::TailDirectory);
        assert!(matches!(
            EventJournal::recover_canonical_tail_v4(
                &root,
                key,
                RecoveryProvenanceV4::new("test", "tail-directory-sync").unwrap(),
            ),
            Err(JournalError::RecoveryDurabilityUncertainV4 {
                outcome: RecoveryOutcomeV4::TailRecovered { .. }
            })
        ));
        assert_eq!(std::fs::read(path).unwrap().last(), Some(&b'\n'));
        assert!(!run_path.join(APPEND_PENDING_MARKER).exists());
    }
}
