//! Descriptor-anchored serialized SQLite derived index.
//!
//! SQLite is deliberately confined to an in-memory connection.  This module
//! owns the bytes crossing that boundary; callers never provide a path or a
//! connection and the canonical journal remains the authority.

use super::journal::IndexReplayError;
use super::{
    CasHash, CasStore, EventJournal, JournalError, JournalIdentity, StoreError, StoreLimits,
    StoreRoot, open_or_create_dir,
};
#[cfg(test)]
use reviewgraphen_core::ReviewAggregate;
use reviewgraphen_core::{
    ContentHash, ContextPolicyV1, DecodedPayload, EventContractVersion, EventEnvelope,
    OfflineProjectionState, PlannerPolicyV1, ReviewContextEnvelope, ReviewPlan, RunGenesisSnapshot,
    StableId, canonical_json,
};
use rusqlite::{Connection, MAIN_DB, OpenFlags, limits::Limit};
use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, Dir, FileType, FlockOperation, Mode, OFlags},
    process::{getegid, geteuid},
    rand::{GetRandomFlags, getrandom},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Cursor, Read, Write},
};
use thiserror::Error;

#[cfg(test)]
use std::os::unix::fs::PermissionsExt;
#[cfg(test)]
use std::sync::{Mutex, mpsc};

const INDEX_DIR: &str = "indexes";
const LOCK_FILE: &str = "index.lock";
const ACTIVE_FILE: &str = "reviewgraphen.sqlite";
const PAGE_SIZE: u64 = 4096;
const INDEX_SCHEMA_VERSION: u32 = 3;
const PROJECTION_CONTRACT_VERSION: &str = "reviewgraphen.index_projection.v3";

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublishFault {
    AfterWrite,
    BeforeCandidateSync,
    AfterCandidateLinkBeforeDirectorySync,
    AfterCandidateSync,
    AfterImageDropBeforeCandidateRead,
    AfterValidation,
    AfterRename,
    AfterActiveVerify,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LockFault {
    PreStat,
    OpenBeforeFlock,
    FlockBeforePostStat,
}

#[cfg(test)]
struct SnapshotMarkerPause {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

/// Bounds specific to the disposable SQLite projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IndexLimits {
    pub max_rows: u64,
    pub max_serialized_bytes: u64,
    pub max_working_bytes: u64,
    pub max_query_bytes: u64,
    pub max_statement_bytes: u64,
}

impl TryFrom<StoreLimits> for IndexLimits {
    type Error = IndexError;

    fn try_from(value: StoreLimits) -> Result<Self, Self::Error> {
        let limits = Self {
            max_rows: value.max_index_rows,
            max_serialized_bytes: value.max_index_serialized_bytes,
            max_working_bytes: value.max_index_working_bytes,
            max_query_bytes: value.max_index_query_bytes,
            max_statement_bytes: value.max_index_statement_bytes,
        };
        limits.validate()?;
        Ok(limits)
    }
}

impl IndexLimits {
    fn validate(self) -> Result<(), IndexError> {
        if self.max_rows == 0
            || self.max_serialized_bytes < PAGE_SIZE
            || self.max_working_bytes
                < self
                    .max_serialized_bytes
                    .checked_mul(3)
                    .and_then(|value| value.checked_add(1024))
                    .unwrap_or(u64::MAX)
            || self.max_query_bytes == 0
            || self.max_query_bytes > self.max_working_bytes
            || self.max_statement_bytes == 0
        {
            return Err(IndexError::InvalidLimits);
        }
        Ok(())
    }

    fn max_pages(self) -> Result<u64, IndexError> {
        let pages = self.max_serialized_bytes / PAGE_SIZE;
        if pages == 0 || pages > i64::MAX as u64 {
            return Err(IndexError::InvalidLimits);
        }
        Ok(pages)
    }
}

/// The singleton, tail-bound marker persisted in every derived image.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexMarker {
    pub index_schema_version: u64,
    pub sqlite_user_version: u64,
    pub projection_contract_version: String,
    pub event_contract_version: String,
    pub projection_mode: String,
    pub run_id: StableId,
    pub genesis_hash: ContentHash,
    pub confirmed_offset: u64,
    pub tail_hash: ContentHash,
    pub event_count: u64,
}

impl IndexMarker {
    fn requested_capacity(&self) -> Result<u64, IndexError> {
        let bytes = self
            .projection_contract_version
            .capacity()
            .checked_add(self.event_contract_version.capacity())
            .and_then(|value| value.checked_add(self.projection_mode.capacity()))
            .and_then(|value| value.checked_add(self.run_id.allocated_bytes()))
            .and_then(|value| value.checked_add(self.genesis_hash.allocated_bytes()))
            .and_then(|value| value.checked_add(self.tail_hash.allocated_bytes()))
            .ok_or(IndexError::IntegerOutOfRange)?;
        u64::try_from(bytes).map_err(|_| IndexError::IntegerOutOfRange)
    }
}

/// Typed envelope metadata retained by the derived index.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexEvent {
    pub sequence: u64,
    pub event_id: StableId,
    pub schema: String,
    pub event_hash: ContentHash,
    pub payload_hash: ContentHash,
    pub payload_kind: String,
    pub actor: String,
    pub logical_time: u64,
}

/// Offline-only authority metadata.  It is never accepted state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexShadow {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub record_id: StableId,
    pub kind: String,
    pub body_hash: ContentHash,
    pub authority_reconciled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexProgramObject {
    pub object_id: StableId,
    pub object_kind: String,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexProgramRelation {
    pub relation_id: StableId,
    pub relation_kind: String,
    pub source_id: StableId,
    pub target_ids_canonical_json: String,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexUniverse {
    pub universe_id: StableId,
    pub snapshot_id: StableId,
    pub profile_id: String,
    pub rule_set_hash: ContentHash,
    pub extractor_set_hash: ContentHash,
    pub policy_version: String,
    pub rule_pack_version: String,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexObligation {
    pub obligation_id: StableId,
    pub target_kind: String,
    pub target_ids_canonical_json: String,
    pub property_id: String,
    pub lifecycle: String,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexObligationLifecycle {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub obligation_id: StableId,
    pub next_lifecycle: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexClaim {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub claim_id: StableId,
    pub execution_id: StableId,
    pub obligation_ids_canonical_json: String,
    pub property_id: String,
    pub target_refs_canonical_json: String,
    pub polarity: String,
    pub disposition: String,
    pub summary: String,
    pub source_ids_canonical_json: String,
    pub assumptions_canonical_json: String,
    pub requested_evidence_canonical_json: String,
    pub candidate_confidence_canonical_json: String,
    pub author_kind: String,
    pub review_status: String,
    pub identity_body_hash: ContentHash,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexExecution {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub execution_id: StableId,
    pub plan_id: StableId,
    pub wave_id: StableId,
    pub snapshot_id: StableId,
    pub envelope_id: StableId,
    pub obligation_ids_canonical_json: String,
    pub reviewer_kind: String,
    pub reviewer_id: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub model_revision: Option<String>,
    pub system_prompt_version: String,
    pub prompt_template_version: String,
    pub inference_settings_canonical_json: String,
    pub tool_policy_version: String,
    pub tool_calls_canonical_json: String,
    pub attempt: u32,
    pub raw_registration_id: StableId,
    pub raw_hash: ContentHash,
    pub parsed_claim_ids_canonical_json: String,
    pub outcome_kind: String,
    pub outcome_canonical_json: String,
    pub identity_body_hash: ContentHash,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexArtifactRegistration {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub registration_id: StableId,
    pub run_id: StableId,
    pub cas_hash: ContentHash,
    pub media_type: String,
    pub size: u64,
    pub sensitivity: String,
    pub source_kind: String,
    pub source_id: String,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexSnapshotSource {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub snapshot_id: StableId,
    pub artifact_id: StableId,
    pub registration_id: StableId,
    pub path: String,
    pub content_hash: ContentHash,
    pub cas_hash: ContentHash,
    pub line_count: u64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexContextEnvelope {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub envelope_id: StableId,
    pub snapshot_id: StableId,
    pub context_policy_version: String,
    pub context_policy_hash: ContentHash,
    pub candidate_ids_canonical_json: String,
    pub obligation_ids_canonical_json: String,
    pub context_policy_canonical_json: String,
    pub included_sources_canonical_json: String,
    pub excluded_sources_canonical_json: String,
    pub unknowns_canonical_json: String,
    pub assumptions_canonical_json: String,
    pub losses_canonical_json: String,
    pub projection_hash: ContentHash,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexReviewPlan {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub plan_id: StableId,
    pub universe_id: StableId,
    pub snapshot_id: StableId,
    pub planner_input_hash: ContentHash,
    pub planner_policy_version: String,
    pub planner_policy_hash: ContentHash,
    pub budget_canonical_json: String,
    pub budget_hash: ContentHash,
    pub risk_breakdown_canonical_json: String,
    pub waves_canonical_json: String,
    pub deferred_canonical_json: String,
    pub identity_body_hash: ContentHash,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexFinding {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub finding_id: StableId,
    pub body_hash: ContentHash,
    pub projection_status: String,
}

/// A deterministic, complete projection of every currently representable
/// index table. Empty vectors are meaningful for V1 metadata-only streams.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IndexSnapshot {
    pub marker: IndexMarker,
    pub events: Vec<IndexEvent>,
    pub shadows: Vec<IndexShadow>,
    pub findings: Vec<IndexFinding>,
    pub program_objects: Vec<IndexProgramObject>,
    pub program_relations: Vec<IndexProgramRelation>,
    pub universe: Option<IndexUniverse>,
    pub obligations: Vec<IndexObligation>,
    pub obligation_lifecycle: Vec<IndexObligationLifecycle>,
    pub executions: Vec<IndexExecution>,
    pub claims: Vec<IndexClaim>,
    pub artifact_registrations: Vec<IndexArtifactRegistration>,
    pub snapshot_sources: Vec<IndexSnapshotSource>,
    pub context_envelopes: Vec<IndexContextEnvelope>,
    pub review_plans: Vec<IndexReviewPlan>,
}

/// Receipt emitted only after a complete rebuild publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexRebuildReceipt {
    pub run_id: StableId,
    pub genesis_hash: ContentHash,
    pub confirmed_offset: u64,
    pub tail_hash: ContentHash,
    pub event_count: u64,
    pub serialized_bytes: u64,
    pub image_hash: ContentHash,
}

/// Fail-closed errors for index admission, image handling, and projection.
#[derive(Debug, Error)]
pub enum IndexError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("derived-index filesystem I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite derived-index operation failed")]
    Sql(#[source] rusqlite::Error),
    #[error("invalid derived-index resource limits")]
    InvalidLimits,
    #[error("derived index is missing")]
    Missing,
    #[error("derived index schema {found} must be rebuilt as schema {required}")]
    RebuildRequired { found: u32, required: u32 },
    #[error("derived index bytes or layout are corrupt")]
    CorruptIndex,
    #[error("derived index path entry changed while locked")]
    IndexPathRace,
    #[error("projection violated its typed contract")]
    ProjectionContractViolation,
    #[error("duplicate derived-index key")]
    DuplicateIndexKey,
    #[error("derived-index publication may have been renamed but is not durably acknowledged")]
    PublicationDurabilityUncertain {
        run_id: StableId,
        tail_hash: ContentHash,
        image_hash: ContentHash,
    },
    #[error("derived index is stale relative to the locked journal tail")]
    CommittedIndexStale {
        indexed_offset: u64,
        indexed_tail: ContentHash,
        committed_offset: u64,
        committed_tail: ContentHash,
    },
    #[error("derived-index operation exceeded bound {limit} (observed {observed})")]
    Incomplete { limit: u64, observed: u64 },
    #[error("integer cannot be represented without loss")]
    IntegerOutOfRange,
    #[error("the platform cannot provide atomic descriptor-relative publication")]
    UnsupportedPlatform,
}

impl From<rusqlite::Error> for IndexError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

impl From<rustix::io::Errno> for IndexError {
    fn from(error: rustix::io::Errno) -> Self {
        Self::Store(StoreError::Io(error))
    }
}

#[cfg(test)]
fn test_publish_failure(stage: &'static str) -> IndexError {
    IndexError::Io(std::io::Error::other(format!(
        "test-only derived-index publication failure: {stage}"
    )))
}

/// An admitted index directory.  It retains descriptors only; every future
/// rebuild/query operation obtains its own lock in the mandated order.
pub struct DerivedIndex<'a> {
    root: &'a StoreRoot,
    indexes: OwnedFd,
    limits: IndexLimits,
    #[cfg(test)]
    publish_faults: Mutex<Vec<PublishFault>>,
    #[cfg(test)]
    lock_faults: Mutex<Vec<LockFault>>,
    #[cfg(test)]
    snapshot_marker_pause: Mutex<Option<SnapshotMarkerPause>>,
}

impl<'a> DerivedIndex<'a> {
    /// Opens or creates the fixed owner-only `indexes/` directory and lock.
    /// This does not read an active image or claim it is current.
    pub fn open(root: &'a StoreRoot) -> Result<Self, IndexError> {
        let limits = IndexLimits::try_from(root.limits())?;
        let indexes = open_or_create_dir(root.fd(), INDEX_DIR, "derived indexes directory")?;
        let index = Self {
            root,
            indexes,
            limits,
            #[cfg(test)]
            publish_faults: Mutex::new(Vec::new()),
            #[cfg(test)]
            lock_faults: Mutex::new(Vec::new()),
            #[cfg(test)]
            snapshot_marker_pause: Mutex::new(None),
        };
        index.ensure_lock_file()?;
        Ok(index)
    }

    #[must_use]
    pub const fn limits(&self) -> IndexLimits {
        self.limits
    }

    #[cfg(test)]
    pub(crate) fn inject_publish_fault(&self, fault: PublishFault) {
        self.publish_faults.lock().unwrap().push(fault);
    }

    #[cfg(test)]
    fn take_publish_fault(&self, fault: PublishFault) -> bool {
        let mut faults = self.publish_faults.lock().unwrap();
        if let Some(index) = faults.iter().position(|value| *value == fault) {
            faults.remove(index);
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_lock_fault(&self, fault: LockFault) {
        self.lock_faults.lock().unwrap().push(fault);
    }

    #[cfg(test)]
    fn replace_lock_entry_for_test(&self, fault: LockFault) -> Result<(), IndexError> {
        let mut faults = self.lock_faults.lock().unwrap();
        let Some(position) = faults.iter().position(|value| *value == fault) else {
            return Ok(());
        };
        faults.remove(position);
        drop(faults);
        let directory = self.root.path().join(INDEX_DIR);
        let displaced = directory.join(format!("fault-displaced-{fault:?}.lock"));
        std::fs::rename(directory.join(LOCK_FILE), &displaced)?;
        std::fs::write(directory.join(LOCK_FILE), b"")?;
        std::fs::set_permissions(
            directory.join(LOCK_FILE),
            std::fs::Permissions::from_mode(0o600),
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn pause_after_snapshot_marker_check(
        &self,
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    ) {
        *self.snapshot_marker_pause.lock().unwrap() =
            Some(SnapshotMarkerPause { entered, release });
    }

    #[cfg(test)]
    fn wait_after_snapshot_marker_check(&self) {
        let pause = self.snapshot_marker_pause.lock().unwrap().take();
        if let Some(pause) = pause {
            pause.entered.send(()).unwrap();
            pause.release.recv().unwrap();
        }
    }

    /// Rebuilds a disposable image from one complete, lock-held journal
    /// prefix. The index lock is deliberately obtained before the journal
    /// reader, and neither guard is accepted from a caller.
    pub fn rebuild(
        &self,
        journal: &EventJournal<'_>,
        cas: &CasStore<'_>,
    ) -> Result<IndexRebuildReceipt, IndexError> {
        let index_lock = self.lock_exclusive()?;
        self.audit_candidates(true)?;
        let mut reader = journal
            .index_reader(self.limits.max_working_bytes)
            .map_err(map_index_journal)?;
        let identity = reader.identity().clone();
        let result = self.build_streaming_locked(&mut reader, &identity, cas);
        index_lock.verify_unchanged()?;
        result
    }

    /// Reads a tail-bound typed snapshot. An active image is never served
    /// after the lock-held journal marker comparison fails.
    pub fn snapshot_current(
        &self,
        journal: &EventJournal<'_>,
    ) -> Result<IndexSnapshot, IndexError> {
        let index_lock = self.lock_shared()?;
        self.audit_candidates(false)?;
        let mut reader = journal
            .index_reader(self.limits.max_working_bytes)
            .map_err(map_index_journal)?;
        let identity = reader.identity().clone();
        let retained = streaming_journal_retained(&reader, &identity, self.limits)?;
        let mut projection = streaming_projection(&identity)?;
        let certificate = reader
            .with_locked_prefix(|event, _| {
                if let Some(state) = projection.as_mut() {
                    let decoded = event
                        .decode_for_streaming_projection()
                        .map_err(|_| IndexError::ProjectionContractViolation)?;
                    let applied = state
                        .apply(&decoded)
                        .map_err(|_| IndexError::ProjectionContractViolation)?;
                    validate_offline_classification(applied, decoded.payload())?;
                }
                Ok(())
            })
            .map_err(map_index_replay)?;
        admit_streaming_scan(retained, &certificate, self.limits)?;
        if projection.as_ref().is_some_and(|state| {
            !state.streaming_cursor_matches(certificate.event_count, &certificate.tail_hash)
        }) {
            return Err(IndexError::ProjectionContractViolation);
        }
        drop(projection);
        let result = (|| {
            let active_before = fs::statat(&self.indexes, ACTIVE_FILE, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(map_entry_error)?;
            verify_index_stat(&active_before, FileType::RegularFile, 0o600)?;
            let image = self.read_active_image_locked_with_retained(retained)?;
            let connection = deserialize_read_only_with_journal(image, self.limits, retained)
                .map_err(normalize_external_image_error)?;
            // Refuse a committed-tail mismatch before cursor comparison: a
            // legitimately older disposable image is stale, not corrupt.
            let committed_marker = index_marker_from_connection(&connection, self.limits)
                .map_err(normalize_external_image_error)?;
            if committed_marker.run_id != identity.run_id
                || committed_marker.genesis_hash != identity.genesis_hash()
                || committed_marker.confirmed_offset != certificate.confirmed_offset
                || committed_marker.tail_hash != certificate.tail_hash
                || committed_marker.event_count != certificate.event_count
                || committed_marker.event_contract_version != identity.version().schema()
            {
                return Err(IndexError::CommittedIndexStale {
                    indexed_offset: committed_marker.confirmed_offset,
                    indexed_tail: committed_marker.tail_hash,
                    committed_offset: certificate.confirmed_offset,
                    committed_tail: certificate.tail_hash.clone(),
                });
            }
            drop(committed_marker);
            let snapshot = snapshot_from_connection_with_journal(
                &connection,
                self.limits,
                query_cache_reservation_with_journal(self.limits, retained)?,
                retained,
                0,
                Some(D1ValidationSource::streaming()),
            )
            .map_err(normalize_external_image_error)?;
            if snapshot.marker.run_id != identity.run_id
                || snapshot.marker.genesis_hash != identity.genesis_hash()
                || snapshot.marker.confirmed_offset != certificate.confirmed_offset
                || snapshot.marker.tail_hash != certificate.tail_hash
                || snapshot.marker.event_count != certificate.event_count
                || snapshot.marker.event_contract_version != identity.version().schema()
            {
                return Err(IndexError::CommittedIndexStale {
                    indexed_offset: snapshot.marker.confirmed_offset,
                    indexed_tail: snapshot.marker.tail_hash,
                    committed_offset: certificate.confirmed_offset,
                    committed_tail: certificate.tail_hash.clone(),
                });
            }
            admit_query_compare_peak(
                &connection,
                retained,
                &certificate,
                &snapshot.marker,
                self.limits,
            )?;
            let mut comparator = StreamingSnapshotComparator::new(&snapshot, identity.version());
            let compared = reader
                .with_locked_prefix(|event, _| comparator.compare_event(event))
                .map_err(map_index_replay)?;
            if compared.confirmed_offset != certificate.confirmed_offset
                || compared.tail_hash != certificate.tail_hash
                || compared.event_count != certificate.event_count
            {
                return Err(IndexError::IndexPathRace);
            }
            comparator.finish()?;
            #[cfg(test)]
            self.wait_after_snapshot_marker_check();
            let active_after = fs::statat(&self.indexes, ACTIVE_FILE, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(map_entry_error)?;
            verify_index_stat(&active_after, FileType::RegularFile, 0o600)?;
            ensure_same_inode(&active_before, &active_after)?;
            Ok(snapshot)
        })();
        index_lock.verify_unchanged()?;
        result
    }

    fn ensure_lock_file(&self) -> Result<(), IndexError> {
        match fs::openat(
            &self.indexes,
            LOCK_FILE,
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::from_raw_mode(0o600),
        ) {
            Ok(fd) => {
                verify_index_fd(&fd, FileType::RegularFile, 0o600)?;
                fs::fsync(&self.indexes)?;
            }
            Err(rustix::io::Errno::EXIST) => {
                let _ = self.open_entry(LOCK_FILE)?;
            }
            Err(error) => return Err(IndexError::Store(StoreError::Io(error))),
        }
        Ok(())
    }

    fn open_entry(&self, name: &str) -> Result<OwnedFd, IndexError> {
        let entry =
            fs::statat(&self.indexes, name, AtFlags::SYMLINK_NOFOLLOW).map_err(map_entry_error)?;
        verify_index_stat(&entry, FileType::RegularFile, 0o600)?;
        let fd = fs::openat(
            &self.indexes,
            name,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(map_entry_error)?;
        verify_index_fd(&fd, FileType::RegularFile, 0o600)?;
        ensure_same_inode(&entry, &fs::fstat(&fd)?)?;
        Ok(fd)
    }

    fn lock(&self, operation: FlockOperation) -> Result<IndexLock<'_>, IndexError> {
        let before = fs::statat(&self.indexes, LOCK_FILE, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(map_entry_error)?;
        verify_index_stat(&before, FileType::RegularFile, 0o600)?;
        #[cfg(test)]
        self.replace_lock_entry_for_test(LockFault::PreStat)?;
        let fd = self.open_entry(LOCK_FILE)?;
        #[cfg(test)]
        self.replace_lock_entry_for_test(LockFault::OpenBeforeFlock)?;
        fs::flock(&fd, operation)?;
        #[cfg(test)]
        self.replace_lock_entry_for_test(LockFault::FlockBeforePostStat)?;
        let after = fs::statat(&self.indexes, LOCK_FILE, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(map_entry_error)?;
        verify_index_stat(&after, FileType::RegularFile, 0o600)?;
        ensure_same_inode(&before, &after)?;
        Ok(IndexLock {
            indexes: &self.indexes,
            fd,
            entry: after,
        })
    }

    pub(crate) fn lock_shared(&self) -> Result<IndexLock<'_>, IndexError> {
        self.lock(FlockOperation::LockShared)
    }

    pub(crate) fn lock_exclusive(&self) -> Result<IndexLock<'_>, IndexError> {
        self.lock(FlockOperation::LockExclusive)
    }

    #[cfg(test)]
    pub(crate) fn new_in_memory_connection(&self) -> Result<Connection, IndexError> {
        self.new_in_memory_connection_with_retained(0)
    }

    fn new_in_memory_connection_with_retained(
        &self,
        retained_view: u64,
    ) -> Result<Connection, IndexError> {
        preflight_build_connection(self.limits, retained_view)?;
        let connection = Connection::open_in_memory_with_flags(
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure_connection(&connection, self.limits, retained_view)?;
        create_schema(&connection, self.limits)?;
        Ok(connection)
    }

    /// Writes a fully validated serialized image using the explicit candidate
    /// -> rename -> directory-sync publication state machine.  This is crate
    /// private until rebuild supplies a tail-bound receipt.
    #[cfg(test)]
    pub(crate) fn publish_image(
        &self,
        image: Vec<u8>,
        _run_id: &StableId,
        _tail_hash: &ContentHash,
    ) -> Result<ContentHash, IndexError> {
        let lock = self.lock_exclusive()?;
        let inspection = deserialize_read_only(image.clone(), self.limits)
            .map_err(normalize_external_image_error)?;
        let expected = index_marker_from_connection(&inspection, self.limits)?;
        drop(inspection);
        let retained = expected.requested_capacity()?;
        let result = self.publish_image_locked(image, &expected, None, retained);
        lock.verify_unchanged()?;
        result
    }

    fn publish_image_locked(
        &self,
        image: Vec<u8>,
        expected_marker: &IndexMarker,
        validation_source: Option<D1ValidationSource<'_>>,
        retained_view: u64,
    ) -> Result<ContentHash, IndexError> {
        check_image_len(image.len(), self.limits)?;
        self.audit_candidates(true)?;
        let hash = ContentHash::sha256(&image);
        let mut nonce = [0_u8; 32];
        getrandom(&mut nonce, GetRandomFlags::empty()).map_err(StoreError::Io)?;
        let name = candidate_name(&nonce);
        let tmp = fs::openat(
            &self.indexes,
            ".",
            OFlags::RDWR | OFlags::TMPFILE | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|error| {
            if error == rustix::io::Errno::NOSYS || error == rustix::io::Errno::OPNOTSUPP {
                IndexError::UnsupportedPlatform
            } else {
                IndexError::Store(StoreError::Io(error))
            }
        })?;
        verify_index_fd(&tmp, FileType::RegularFile, 0o600)?;
        let mut file = File::from(tmp);
        file.write_all(&image)?;
        #[cfg(test)]
        if self.take_publish_fault(PublishFault::AfterWrite) {
            return Err(test_publish_failure("after candidate write"));
        }
        file.sync_all()?;
        #[cfg(test)]
        if self.take_publish_fault(PublishFault::BeforeCandidateSync) {
            return Err(test_publish_failure(
                "after candidate file sync before link",
            ));
        }
        let tmp: OwnedFd = file.into();
        fs::linkat(&tmp, "", &self.indexes, &name, AtFlags::EMPTY_PATH).map_err(|error| {
            if error == rustix::io::Errno::EXIST {
                IndexError::IndexPathRace
            } else {
                IndexError::Store(StoreError::Io(error))
            }
        })?;
        #[cfg(test)]
        if self.take_publish_fault(PublishFault::AfterCandidateLinkBeforeDirectorySync) {
            return Err(test_publish_failure(
                "after candidate link before directory sync",
            ));
        }
        fs::fsync(&self.indexes)?;
        #[cfg(test)]
        if self.take_publish_fault(PublishFault::AfterCandidateSync) {
            return Err(test_publish_failure("after candidate directory sync"));
        }
        // The publication input is no longer retained while the candidate is
        // reread and reconstructed. This makes candidate validation a
        // distinct bounded stage rather than two simultaneous image buffers.
        drop(image);
        #[cfg(test)]
        if self.take_publish_fault(PublishFault::AfterImageDropBeforeCandidateRead) {
            return Err(test_publish_failure(
                "after publication image drop before candidate read allocation",
            ));
        }
        let candidate = self.open_entry(&name)?;
        ensure_same_inode(&fs::fstat(&tmp)?, &fs::fstat(&candidate)?)?;
        let candidate_bytes = read_fd_exact_with_retained(
            candidate,
            self.limits.max_serialized_bytes,
            retained_view,
            self.limits.max_working_bytes,
        )?;
        if ContentHash::sha256(&candidate_bytes) != hash {
            return Err(IndexError::CorruptIndex);
        }
        // Prepared is not merely byte-hash valid: an independent in-memory,
        // read-only deserialize must re-check schema, marker, FK and complete
        // typed snapshot before it can replace the active image.
        let candidate_read_image =
            u64::try_from(candidate_bytes.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
        let candidate_connection =
            deserialize_read_only_with_journal(candidate_bytes, self.limits, retained_view)
                .map_err(normalize_external_image_error)?;
        let candidate_marker = index_marker_from_connection(&candidate_connection, self.limits)
            .map_err(normalize_external_image_error)?;
        if &candidate_marker != expected_marker {
            return Err(IndexError::CorruptIndex);
        }
        drop(candidate_marker);
        let _ = snapshot_from_connection_with_journal(
            &candidate_connection,
            self.limits,
            query_cache_reservation_with_journal(self.limits, retained_view)?,
            retained_view,
            candidate_read_image,
            validation_source,
        )
        .map_err(normalize_external_image_error)?;
        drop(candidate_connection);
        #[cfg(test)]
        if self.take_publish_fault(PublishFault::AfterValidation) {
            return Err(test_publish_failure("after candidate validation"));
        }
        fs::renameat(&self.indexes, &name, &self.indexes, ACTIVE_FILE)
            .map_err(|error| IndexError::Store(StoreError::Io(error)))?;
        // Once rename succeeds, neither an inspection failure nor a directory
        // sync failure can honestly say which image survives a crash. Never
        // roll back through another uncertain rename.
        #[cfg(test)]
        if self.take_publish_fault(PublishFault::AfterRename) {
            return Err(IndexError::PublicationDurabilityUncertain {
                run_id: expected_marker.run_id.clone(),
                tail_hash: expected_marker.tail_hash.clone(),
                image_hash: hash,
            });
        }
        let durable = (|| -> Result<(), IndexError> {
            let active = self.open_entry(ACTIVE_FILE)?;
            ensure_same_inode(&fs::fstat(&tmp)?, &fs::fstat(&active)?)?;
            let active_bytes = read_fd_exact_with_retained(
                active,
                self.limits.max_serialized_bytes,
                retained_view,
                self.limits.max_working_bytes,
            )?;
            if ContentHash::sha256(&active_bytes) != hash {
                return Err(IndexError::CorruptIndex);
            }
            #[cfg(test)]
            if self.take_publish_fault(PublishFault::AfterActiveVerify) {
                return Err(test_publish_failure("after active verification"));
            }
            fs::fsync(&self.indexes)?;
            Ok(())
        })();
        if durable.is_err() {
            return Err(IndexError::PublicationDurabilityUncertain {
                run_id: expected_marker.run_id.clone(),
                tail_hash: expected_marker.tail_hash.clone(),
                image_hash: hash,
            });
        }
        Ok(hash)
    }

    #[cfg(test)]
    pub(crate) fn read_active_image(&self) -> Result<Vec<u8>, IndexError> {
        let _lock = self.lock_shared()?;
        self.audit_candidates(false)?;
        self.read_active_image_locked()
    }

    #[cfg(test)]
    fn read_active_image_locked(&self) -> Result<Vec<u8>, IndexError> {
        self.read_active_image_locked_with_retained(0)
    }

    fn read_active_image_locked_with_retained(&self, retained: u64) -> Result<Vec<u8>, IndexError> {
        let bytes = read_fd_exact_with_retained(
            self.open_entry(ACTIVE_FILE)?,
            self.limits.max_serialized_bytes,
            retained,
            self.limits.max_working_bytes,
        )?;
        check_image_len(bytes.len(), self.limits)?;
        Ok(bytes)
    }

    fn build_streaming_locked(
        &self,
        reader: &mut super::journal::IndexJournalReader,
        identity: &JournalIdentity,
        cas: &CasStore<'_>,
    ) -> Result<IndexRebuildReceipt, IndexError> {
        let version = identity.version();
        let retained = streaming_journal_retained(reader, identity, self.limits)?;
        let genesis = if version == EventContractVersion::V2 {
            let bytes = identity
                .v2_genesis_backing()
                .ok_or(IndexError::ProjectionContractViolation)?;
            let hash = CasHash::parse(identity.genesis_hash().to_string())
                .map_err(|_| IndexError::ProjectionContractViolation)?;
            let cas_chunk = u64::try_from(CasStore::verification_chunk_capacity(bytes.len()))
                .map_err(|_| IndexError::IntegerOutOfRange)?;
            let cas_peak = retained
                .checked_add(cas_chunk)
                .ok_or(IndexError::Incomplete {
                    limit: self.limits.max_working_bytes,
                    observed: u64::MAX,
                })?;
            if cas_peak > self.limits.max_working_bytes {
                return Err(IndexError::Incomplete {
                    limit: self.limits.max_working_bytes,
                    observed: cas_peak,
                });
            }
            cas.verify_exact_bytes_streaming(&hash, bytes)
                .map_err(IndexError::Store)?;
            Some(
                RunGenesisSnapshot::from_canonical_bytes_for_index(bytes)
                    .map_err(|_| IndexError::ProjectionContractViolation)?,
            )
        } else {
            None
        };

        let mut projection = streaming_projection(identity)?;
        let certificate = reader
            .with_locked_prefix(|event, _| {
                if let Some(state) = projection.as_mut() {
                    let decoded = event
                        .decode_for_streaming_projection()
                        .map_err(|_| IndexError::ProjectionContractViolation)?;
                    let applied = state
                        .apply(&decoded)
                        .map_err(|_| IndexError::ProjectionContractViolation)?;
                    validate_offline_classification(applied, decoded.payload())?;
                }
                Ok(())
            })
            .map_err(map_index_replay)?;
        admit_streaming_scan(retained, &certificate, self.limits)?;
        if projection.as_ref().is_some_and(|state| {
            !state.streaming_cursor_matches(certificate.event_count, &certificate.tail_hash)
        }) {
            return Err(IndexError::ProjectionContractViolation);
        }
        drop(projection);

        let genesis_tuple_bound = identity
            .v2_genesis_backing()
            .map_or(0, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        let current_insert_tuple = certificate
            .line_buffer_capacity
            .max(genesis_tuple_bound)
            .checked_mul(2)
            .ok_or(IndexError::Incomplete {
                limit: self.limits.max_working_bytes,
                observed: u64::MAX,
            })?;
        let rebuild_projection_retained = retained
            .checked_add(certificate.line_buffer_capacity)
            .and_then(|value| value.checked_add(certificate.canonical_scratch_capacity))
            // Baseline fields partition G; event fields partition one JSONL
            // record. A second equal reservation covers the temporary owned
            // SQL parameter strings produced for that tuple.
            .and_then(|value| value.checked_add(current_insert_tuple))
            .ok_or(IndexError::Incomplete {
                limit: self.limits.max_working_bytes,
                observed: u64::MAX,
            })?;
        let connection =
            self.new_in_memory_connection_with_retained(rebuild_projection_retained)?;
        let transaction = connection.unchecked_transaction()?;
        let mut rows = 0_u64;
        let mode = if version == EventContractVersion::V1 {
            "v1_event_metadata_only"
        } else {
            "v2_domain"
        };
        reserve_row(&mut rows, self.limits)?;
        insert_index_marker(
            &transaction,
            identity,
            version,
            mode,
            certificate.confirmed_offset,
            &certificate.tail_hash,
            certificate.event_count,
        )?;
        validate_index_marker_version(&transaction)?;
        let mut expected_lifecycles = if let Some(genesis) = genesis.as_ref() {
            insert_baseline(&transaction, genesis, &mut rows, self.limits)?;
            genesis
                .obligations()
                .iter()
                .map(|obligation| {
                    Ok((
                        obligation.id().clone(),
                        serialized_enum(&obligation.lifecycle())?,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, IndexError>>()?
        } else {
            BTreeMap::new()
        };
        let inserted = reader
            .with_locked_prefix(|envelope, _| {
                let event = envelope
                    .decode_for_streaming_projection()
                    .map_err(|_| IndexError::ProjectionContractViolation)?;
                // A D2 execution is one domain-atomic unit: validate every
                // pre-existing reference before admitting even its envelope
                // row to the disposable projection.
                if let DecodedPayload::ReviewExecutionRecorded { execution, claims } = event.payload() {
                    prevalidate_execution_domain(&transaction, envelope, execution, claims)?;
                }
                reserve_row(&mut rows, self.limits)?;
                insert_event(
                    &transaction,
                    envelope,
                    version.schema(),
                    payload_kind_decoded(event.payload())?,
                )?;
                if genesis.is_some() {
                    match event.payload() {
                    DecodedPayload::EvidenceRecorded(value) => insert_shadow_record(
                        &transaction,
                        envelope,
                        value.id(),
                        "evidence_recorded",
                        value,
                        &mut rows,
                        self.limits,
                    )?,
                    DecodedPayload::EvidenceBound(value) => insert_shadow_record(
                        &transaction,
                        envelope,
                        value.id(),
                        "evidence_bound",
                        value,
                        &mut rows,
                        self.limits,
                    )?,
                    DecodedPayload::VerificationRecorded(value) => insert_shadow_record(
                        &transaction,
                        envelope,
                        value.id(),
                        "verification_recorded",
                        value,
                        &mut rows,
                        self.limits,
                    )?,
                    DecodedPayload::DecisionRecorded(value) => insert_shadow_record(
                        &transaction,
                        envelope,
                        value.id(),
                        "decision_recorded",
                        value,
                        &mut rows,
                        self.limits,
                    )?,
                    DecodedPayload::FindingRecorded(value) => {
                        reserve_row(&mut rows, self.limits)?;
                        transaction.execute("INSERT INTO projected_findings(event_sequence,event_id,finding_id,body_hash,projection_status) VALUES(?1,?2,?3,?4,'shadow_only')", rusqlite::params![to_i64(envelope.sequence())?, envelope.id().to_string(), value.id().to_string(), body_hash(value)?.to_string()]).map_err(map_sql)?;
                    }
                    DecodedPayload::ObligationTransition {
                        obligation_id,
                        next,
                    } => {
                        reserve_row(&mut rows, self.limits)?;
                        let lifecycle = serialized_enum(next)?;
                        let previous = expected_lifecycles
                            .get(obligation_id)
                            .ok_or(IndexError::ProjectionContractViolation)?
                            .clone();
                        transaction.execute("INSERT INTO obligation_lifecycle(event_sequence,event_id,obligation_id,next_lifecycle) VALUES(?1,?2,?3,?4)", rusqlite::params![to_i64(envelope.sequence())?, envelope.id().to_string(), obligation_id.to_string(), lifecycle]).map_err(map_sql)?;
                        let changed = transaction
                            .execute(
                                "UPDATE obligations SET lifecycle=?1 WHERE obligation_id=?2 AND lifecycle=?3",
                                rusqlite::params![
                                    lifecycle,
                                    obligation_id.to_string(),
                                    previous,
                                ],
                            )
                            .map_err(map_sql)?;
                        if changed != 1 {
                            return Err(IndexError::ProjectionContractViolation);
                        }
                        expected_lifecycles.insert(obligation_id.clone(), serialized_enum(next)?);
                    }
                    // `ClaimProposed` is a legacy-v1 payload. V1 is metadata
                    // only and V2 D2 claims exist solely inside their atomic
                    // execution event, so neither has a narrow claim row.
                    DecodedPayload::ClaimProposed(_) => {}
                    DecodedPayload::RunGenesisManifest(manifest) => {
                        insert_registration(
                            &transaction,
                            envelope,
                            manifest.genesis_artifact(),
                            &mut rows,
                            self.limits,
                        )?;
                    }
                    DecodedPayload::ArtifactRegistered(registration) => insert_registration(
                        &transaction,
                        envelope,
                        registration,
                        &mut rows,
                        self.limits,
                    )?,
                    DecodedPayload::SnapshotSourcesRecorded(sources) => insert_sources(
                        &transaction,
                        envelope,
                        sources,
                        &mut rows,
                        self.limits,
                    )?,
                    DecodedPayload::ReviewPlanRecorded(plan) => insert_review_plan(
                        &transaction,
                        envelope,
                        plan,
                        &mut rows,
                        self.limits,
                    )?,
                    DecodedPayload::ContextEnvelopeProjected(context) => insert_context_envelope(
                        &transaction,
                        envelope,
                        context,
                        &mut rows,
                        self.limits,
                    )?,
                    DecodedPayload::ReviewExecutionRecorded { execution, claims } => {
                        insert_execution_and_claims(
                            &transaction,
                            envelope,
                            execution,
                            claims,
                            &mut rows,
                            self.limits,
                        )?;
                    }
                    DecodedPayload::RunGenesisManifestV3(_)
                    | DecodedPayload::ArtifactRegisteredV3(_)
                    | DecodedPayload::EvidenceRecordedV3(_)
                    | DecodedPayload::EvidenceBoundV3(_)
                    | DecodedPayload::VerificationRecordedV3(_)
                    | DecodedPayload::DecisionRecordedV3(_)
                    | DecodedPayload::FindingRecordedV3(_) => {
                        return Err(IndexError::ProjectionContractViolation);
                    }
                    }
                }
                Ok(())
            })
            .map_err(map_index_replay)?;
        if inserted != certificate {
            return Err(IndexError::IndexPathRace);
        }
        transaction.commit()?;
        if !connection.is_autocommit() {
            return Err(IndexError::ProjectionContractViolation);
        }
        // Validate the complete marker/table/FK/integrity contract before
        // any bytes cross the SQLite-to-store publication boundary.
        let rebuilt_snapshot = snapshot_from_connection_with_journal(
            &connection,
            self.limits,
            build_cache_reservation_with_journal(self.limits, retained)?,
            retained,
            0,
            Some(D1ValidationSource::streaming()),
        )?;
        admit_query_compare_peak(
            &connection,
            retained,
            &certificate,
            &rebuilt_snapshot.marker,
            self.limits,
        )?;
        let mut comparator = StreamingSnapshotComparator::new(&rebuilt_snapshot, version);
        let compared = reader
            .with_locked_prefix(|event, _| comparator.compare_event(event))
            .map_err(map_index_replay)?;
        if compared != certificate {
            return Err(IndexError::IndexPathRace);
        }
        comparator.finish()?;
        let IndexSnapshot {
            marker: rebuilt_marker,
            ..
        } = rebuilt_snapshot;
        let publication_retained = retained
            .checked_add(rebuilt_marker.requested_capacity()?)
            .ok_or(IndexError::Incomplete {
                limit: self.limits.max_working_bytes,
                observed: u64::MAX,
            })?;
        let image =
            serialize_connection_with_retained(&connection, self.limits, publication_retained)?;
        let serialized_bytes =
            u64::try_from(image.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
        // SQLite build/cache memory is released before descriptor publication
        // takes ownership of the serialized image.
        drop(connection);
        let image_hash = self.publish_image_locked(
            image,
            &rebuilt_marker,
            Some(D1ValidationSource::streaming()),
            publication_retained,
        )?;
        Ok(IndexRebuildReceipt {
            run_id: identity.run_id.clone(),
            genesis_hash: identity.genesis_hash(),
            confirmed_offset: certificate.confirmed_offset,
            tail_hash: certificate.tail_hash,
            event_count: certificate.event_count,
            serialized_bytes,
            image_hash,
        })
    }

    /// Audits every store-owned candidate name before query/rebuild.  Queries
    /// never delete; rebuild publication may remove only an inode it has
    /// checked immediately before unlinking.
    fn audit_candidates(&self, remove: bool) -> Result<(), IndexError> {
        let limits = self.root.limits();
        let mut directory = Dir::read_from(&self.indexes)?;
        let mut candidates = Vec::new();
        let mut count = 0_u64;
        let mut apparent_bytes = 0_u64;
        while let Some(entry) = directory.read() {
            let entry = entry?;
            let name = entry
                .file_name()
                .to_str()
                .map_err(|_| IndexError::CorruptIndex)?;
            if matches!(name, "." | ".." | LOCK_FILE | ACTIVE_FILE) {
                continue;
            }
            if !is_candidate_name(name) {
                return Err(IndexError::CorruptIndex);
            }
            let stat = fs::statat(&self.indexes, name, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(map_entry_error)?;
            verify_index_stat(&stat, FileType::RegularFile, 0o600)?;
            count = count.checked_add(1).ok_or(IndexError::Incomplete {
                limit: limits.max_tmp_gc_entries,
                observed: u64::MAX,
            })?;
            if count > limits.max_tmp_gc_entries {
                return Err(IndexError::Incomplete {
                    limit: limits.max_tmp_gc_entries,
                    observed: count,
                });
            }
            let size = u64::try_from(stat.st_size).map_err(|_| IndexError::CorruptIndex)?;
            apparent_bytes = apparent_bytes
                .checked_add(size)
                .ok_or(IndexError::Incomplete {
                    limit: limits.max_tmp_gc_scan_bytes,
                    observed: u64::MAX,
                })?;
            if apparent_bytes > limits.max_tmp_gc_scan_bytes {
                return Err(IndexError::Incomplete {
                    limit: limits.max_tmp_gc_scan_bytes,
                    observed: apparent_bytes,
                });
            }
            candidates.push((name.to_owned(), stat));
        }
        if remove && !candidates.is_empty() {
            for (name, expected) in candidates {
                let fd = self.open_entry(&name)?;
                ensure_same_inode(&expected, &fs::fstat(&fd)?)?;
                let current = fs::statat(&self.indexes, &name, AtFlags::SYMLINK_NOFOLLOW)
                    .map_err(map_entry_error)?;
                ensure_same_inode(&expected, &current)?;
                fs::unlinkat(&self.indexes, &name, AtFlags::empty())?;
            }
            fs::fsync(&self.indexes)?;
        }
        Ok(())
    }
}

/// A held operation lock.  Call `verify_unchanged` at the final seam before
/// returning a result; close then releases flock.
pub(crate) struct IndexLock<'a> {
    indexes: &'a OwnedFd,
    fd: OwnedFd,
    entry: fs::Stat,
}

impl IndexLock<'_> {
    pub(crate) fn verify_unchanged(&self) -> Result<(), IndexError> {
        verify_index_fd(&self.fd, FileType::RegularFile, 0o600)?;
        let current = fs::statat(self.indexes, LOCK_FILE, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(map_entry_error)?;
        verify_index_stat(&current, FileType::RegularFile, 0o600)?;
        ensure_same_inode(&self.entry, &current)
    }
}

fn configure_connection(
    connection: &Connection,
    limits: IndexLimits,
    retained_view: u64,
) -> Result<(), IndexError> {
    // All SQL is fixed checked-in text.  No user input reaches SQLite.
    let max_pages =
        i64::try_from(limits.max_pages()?).map_err(|_| IndexError::IntegerOutOfRange)?;
    for statement in [
        "PRAGMA page_size = 4096",
        "PRAGMA temp_store = MEMORY",
        "PRAGMA foreign_keys = ON",
        "PRAGMA journal_mode = MEMORY",
        "PRAGMA locking_mode = EXCLUSIVE",
        "PRAGMA trusted_schema = OFF",
        "PRAGMA user_version = 3",
    ] {
        checked_batch(connection, statement, limits)?;
    }
    connection.pragma_update(None, "max_page_count", max_pages)?;
    let cache_kib = cache_kib(
        limits,
        build_cache_reservation_with_journal(limits, retained_view)?,
    )?;
    connection.pragma_update(None, "cache_size", -cache_kib)?;
    let image_limit =
        i32::try_from(limits.max_serialized_bytes).map_err(|_| IndexError::IntegerOutOfRange)?;
    let statement_limit =
        i32::try_from(limits.max_statement_bytes).map_err(|_| IndexError::IntegerOutOfRange)?;
    for (limit, value) in [
        (Limit::SQLITE_LIMIT_LENGTH, image_limit),
        (Limit::SQLITE_LIMIT_SQL_LENGTH, statement_limit),
        (Limit::SQLITE_LIMIT_COLUMN, 64),
        (Limit::SQLITE_LIMIT_COMPOUND_SELECT, 32),
        (Limit::SQLITE_LIMIT_EXPR_DEPTH, 128),
        (Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 256),
    ] {
        connection.set_limit(limit, value)?;
        if connection.limit(limit)? != value {
            return Err(IndexError::ProjectionContractViolation);
        }
    }
    for (pragma, expected) in [
        (
            "page_size",
            i64::try_from(PAGE_SIZE).map_err(|_| IndexError::IntegerOutOfRange)?,
        ),
        ("user_version", i64::from(INDEX_SCHEMA_VERSION)),
    ] {
        let observed: i64 =
            connection.query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))?;
        if observed != expected {
            return Err(IndexError::ProjectionContractViolation);
        }
    }
    let temp_store: i64 = connection.pragma_query_value(None, "temp_store", |row| row.get(0))?;
    let foreign_keys: i64 =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    let cache_size: i64 = connection.pragma_query_value(None, "cache_size", |row| row.get(0))?;
    let trusted_schema: i64 =
        connection.pragma_query_value(None, "trusted_schema", |row| row.get(0))?;
    let max_page_count: i64 =
        connection.pragma_query_value(None, "max_page_count", |row| row.get(0))?;
    let journal_mode: String =
        connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    let locking_mode: String =
        connection.pragma_query_value(None, "locking_mode", |row| row.get(0))?;
    if temp_store != 2
        || foreign_keys != 1
        || trusted_schema != 0
        || cache_size != -cache_kib
        || max_page_count != max_pages
        || journal_mode != "memory"
        || locking_mode != "exclusive"
    {
        return Err(IndexError::ProjectionContractViolation);
    }
    Ok(())
}

fn preflight_build_connection(limits: IndexLimits, retained_view: u64) -> Result<(), IndexError> {
    // Refuse before SQLite can allocate a connection, reconstructed main
    // image, page cache, or schema. The future serialize peak is also checked
    // here because the configured build cache is selected from that retained
    // reservation.
    let serialize_reservation = build_cache_reservation_with_journal(limits, retained_view)?;
    let configured_cache = configured_cache_bytes(limits, serialize_reservation)?;
    let build_peak = retained_view
        .checked_add(limits.max_serialized_bytes)
        .and_then(|value| value.checked_add(configured_cache))
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?;
    if build_peak > limits.max_working_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: build_peak,
        });
    }
    Ok(())
}

fn checked_batch(
    connection: &Connection,
    statement: &str,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    let observed = u64::try_from(statement.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
    if observed > limits.max_statement_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_statement_bytes,
            observed,
        });
    }
    connection.execute_batch(statement)?;
    Ok(())
}

fn create_schema(connection: &Connection, limits: IndexLimits) -> Result<(), IndexError> {
    // Schema literals intentionally use closed checks. Projection code adds
    // typed row insertion with the corresponding release, never generic text.
    const DDL: &str = r#"
CREATE TABLE index_meta (
 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
 index_schema_version INTEGER NOT NULL CHECK (index_schema_version = 3),
 projection_contract_version TEXT NOT NULL CHECK (projection_contract_version = 'reviewgraphen.index_projection.v3'),
 event_contract_version TEXT NOT NULL CHECK (event_contract_version IN ('reviewgraphen.review_event.v1','reviewgraphen.review_event.v2')),
 projection_mode TEXT NOT NULL CHECK (projection_mode IN ('v1_event_metadata_only','v2_domain')),
 run_id TEXT NOT NULL, genesis_hash TEXT NOT NULL,
 confirmed_offset INTEGER NOT NULL CHECK (confirmed_offset >= 0), tail_hash TEXT NOT NULL,
 event_count INTEGER NOT NULL CHECK (event_count >= 0),
 CHECK ((event_contract_version = 'reviewgraphen.review_event.v1' AND projection_mode = 'v1_event_metadata_only') OR (event_contract_version = 'reviewgraphen.review_event.v2' AND projection_mode = 'v2_domain'))
) STRICT;
CREATE TABLE events (
 sequence INTEGER PRIMARY KEY CHECK (sequence > 0), event_id TEXT NOT NULL UNIQUE,
 schema TEXT NOT NULL CHECK (schema IN ('reviewgraphen.review_event.v1','reviewgraphen.review_event.v2')),
 event_hash TEXT NOT NULL, payload_hash TEXT NOT NULL,
 payload_kind TEXT NOT NULL CHECK (payload_kind IN ('obligation_transition','claim_proposed','evidence_recorded','evidence_bound','verification_recorded','decision_recorded','finding_recorded','run_genesis_manifest','artifact_registered','snapshot_sources_recorded','review_plan_recorded','context_envelope_projected','review_execution_recorded')),
 actor TEXT NOT NULL, logical_time INTEGER NOT NULL CHECK (logical_time >= 0), UNIQUE(sequence,event_id)
) STRICT;
CREATE TABLE program_objects (object_id TEXT PRIMARY KEY, object_kind TEXT NOT NULL, body_hash TEXT NOT NULL) STRICT;
CREATE TABLE program_relations (relation_id TEXT PRIMARY KEY, relation_kind TEXT NOT NULL, source_id TEXT NOT NULL, target_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL) STRICT;
CREATE TABLE universe (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), universe_id TEXT NOT NULL UNIQUE, snapshot_id TEXT NOT NULL, profile_id TEXT NOT NULL, rule_set_hash TEXT NOT NULL, extractor_set_hash TEXT NOT NULL, policy_version TEXT NOT NULL, rule_pack_version TEXT NOT NULL, body_hash TEXT NOT NULL) STRICT;
CREATE TABLE obligations (obligation_id TEXT PRIMARY KEY, target_kind TEXT NOT NULL CHECK(target_kind IN ('node','relation','path','invariant','subgraph')), target_ids_canonical_json TEXT NOT NULL, property_id TEXT NOT NULL, lifecycle TEXT NOT NULL CHECK(lifecycle IN ('generated','planned','in_progress','completed','stale','superseded','cancelled')), body_hash TEXT NOT NULL) STRICT;
CREATE TABLE obligation_lifecycle (event_sequence INTEGER NOT NULL, event_id TEXT NOT NULL, obligation_id TEXT NOT NULL, next_lifecycle TEXT NOT NULL CHECK(next_lifecycle IN ('generated','planned','in_progress','completed','stale','superseded','cancelled')), PRIMARY KEY(event_sequence, obligation_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE executions (
 event_sequence INTEGER NOT NULL CHECK (event_sequence > 0), event_id TEXT NOT NULL,
 execution_id TEXT NOT NULL UNIQUE, plan_id TEXT NOT NULL, wave_id TEXT NOT NULL,
 snapshot_id TEXT NOT NULL, envelope_id TEXT NOT NULL, obligation_ids_canonical_json TEXT NOT NULL,
 reviewer_kind TEXT NOT NULL CHECK (reviewer_kind = 'fake'),
 reviewer_id TEXT NOT NULL CHECK (reviewer_id = 'reviewgraphen.fake_reviewer@1'),
 provider TEXT, model TEXT, model_revision TEXT,
 system_prompt_version TEXT NOT NULL CHECK (system_prompt_version = 'reviewgraphen.system.no_tools@1'),
 prompt_template_version TEXT NOT NULL CHECK (prompt_template_version = 'fixture@1'),
 inference_settings_canonical_json TEXT NOT NULL CHECK (inference_settings_canonical_json = '{}'),
 tool_policy_version TEXT NOT NULL CHECK (tool_policy_version = 'reviewgraphen.tool_policy.none@1'),
 tool_calls_canonical_json TEXT NOT NULL CHECK (tool_calls_canonical_json = '[]'),
 attempt INTEGER NOT NULL CHECK (attempt > 0), raw_registration_id TEXT NOT NULL,
 raw_hash TEXT NOT NULL, parsed_claim_ids_canonical_json TEXT NOT NULL,
 outcome_kind TEXT NOT NULL CHECK (outcome_kind IN ('structured','abstained','malformed','provider_failure')),
 outcome_canonical_json TEXT NOT NULL, identity_body_hash TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY (event_sequence, execution_id),
 FOREIGN KEY (event_sequence, event_id) REFERENCES events (sequence, event_id),
 FOREIGN KEY (plan_id) REFERENCES review_plans (plan_id),
 FOREIGN KEY (envelope_id) REFERENCES context_envelopes (envelope_id),
 FOREIGN KEY (raw_registration_id) REFERENCES artifact_registrations (registration_id),
 CHECK (provider IS NULL AND model IS NULL AND model_revision IS NULL),
 CHECK (attempt <= 4294967295)
) STRICT;
CREATE TABLE claims (
 event_sequence INTEGER NOT NULL CHECK (event_sequence > 0), event_id TEXT NOT NULL,
 claim_id TEXT NOT NULL UNIQUE, execution_id TEXT NOT NULL,
 obligation_ids_canonical_json TEXT NOT NULL, property_id TEXT NOT NULL,
 target_refs_canonical_json TEXT NOT NULL,
 polarity TEXT NOT NULL CHECK (polarity IN ('issue_present','issue_absent','inconclusive','not_applicable','conflict')),
 disposition TEXT NOT NULL CHECK (disposition = 'proposed'), summary TEXT NOT NULL,
 source_ids_canonical_json TEXT NOT NULL, assumptions_canonical_json TEXT NOT NULL,
 requested_evidence_canonical_json TEXT NOT NULL,
 candidate_confidence_canonical_json TEXT NOT NULL,
 author_kind TEXT NOT NULL CHECK (author_kind = 'ai'),
 review_status TEXT NOT NULL CHECK (review_status = 'unreviewed'),
 identity_body_hash TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY (event_sequence, claim_id),
 FOREIGN KEY (event_sequence, event_id) REFERENCES events (sequence, event_id),
 FOREIGN KEY (execution_id) REFERENCES executions (execution_id)
) STRICT;
CREATE TABLE artifact_registrations (event_sequence INTEGER NOT NULL, event_id TEXT NOT NULL, registration_id TEXT NOT NULL UNIQUE, run_id TEXT NOT NULL, cas_hash TEXT NOT NULL, media_type TEXT NOT NULL, size INTEGER NOT NULL CHECK(size >= 0), sensitivity TEXT NOT NULL CHECK(sensitivity IN ('canonical_state','workspace_source','sensitive')), source_kind TEXT NOT NULL CHECK(source_kind IN ('run_genesis','snapshot_ingest','reviewer_execution')), source_id TEXT NOT NULL, body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,registration_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE snapshot_source_index (event_sequence INTEGER NOT NULL, event_id TEXT NOT NULL, snapshot_id TEXT NOT NULL, artifact_id TEXT NOT NULL, registration_id TEXT NOT NULL, path TEXT NOT NULL, content_hash TEXT NOT NULL, cas_hash TEXT NOT NULL, line_count INTEGER NOT NULL CHECK(line_count >= 0), PRIMARY KEY(snapshot_id,path,artifact_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE review_plans (
 event_sequence INTEGER NOT NULL CHECK(event_sequence > 0), event_id TEXT NOT NULL,
 plan_id TEXT NOT NULL UNIQUE, universe_id TEXT NOT NULL, snapshot_id TEXT NOT NULL,
 planner_input_hash TEXT NOT NULL,
 planner_policy_version TEXT NOT NULL CHECK(planner_policy_version = 'scheduler.baseline@1'),
 planner_policy_hash TEXT NOT NULL, budget_canonical_json TEXT NOT NULL,
 budget_hash TEXT NOT NULL, risk_breakdown_canonical_json TEXT NOT NULL,
 waves_canonical_json TEXT NOT NULL, deferred_canonical_json TEXT NOT NULL,
 identity_body_hash TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,plan_id),
 FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;
CREATE TABLE context_envelopes (
 event_sequence INTEGER NOT NULL CHECK(event_sequence > 0), event_id TEXT NOT NULL,
 envelope_id TEXT NOT NULL UNIQUE, snapshot_id TEXT NOT NULL,
 context_policy_version TEXT NOT NULL CHECK(context_policy_version = 'context.baseline@1'),
 context_policy_hash TEXT NOT NULL, candidate_ids_canonical_json TEXT NOT NULL,
 obligation_ids_canonical_json TEXT NOT NULL, context_policy_canonical_json TEXT NOT NULL,
 included_sources_canonical_json TEXT NOT NULL, excluded_sources_canonical_json TEXT NOT NULL,
 unknowns_canonical_json TEXT NOT NULL, assumptions_canonical_json TEXT NOT NULL,
 losses_canonical_json TEXT NOT NULL, projection_hash TEXT NOT NULL, body_hash TEXT NOT NULL,
 PRIMARY KEY(event_sequence,envelope_id),
 FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)
) STRICT;
CREATE TABLE unreconciled_authority_records (event_sequence INTEGER NOT NULL CHECK(event_sequence > 0), event_id TEXT NOT NULL, record_id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('evidence_recorded','evidence_bound','verification_recorded','decision_recorded')), body_hash TEXT NOT NULL, authority_reconciled INTEGER NOT NULL DEFAULT 0 CHECK(authority_reconciled = 0), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE projected_findings (event_sequence INTEGER NOT NULL CHECK(event_sequence > 0), event_id TEXT NOT NULL, finding_id TEXT PRIMARY KEY, body_hash TEXT NOT NULL, projection_status TEXT NOT NULL DEFAULT 'shadow_only' CHECK(projection_status = 'shadow_only'), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
"#;
    checked_batch(connection, DDL, limits)
}

#[cfg(test)]
pub(crate) fn serialize_connection(
    connection: &Connection,
    limits: IndexLimits,
) -> Result<Vec<u8>, IndexError> {
    serialize_connection_with_retained(connection, limits, 0)
}

fn serialize_connection_with_retained(
    connection: &Connection,
    limits: IndexLimits,
    retained_view: u64,
) -> Result<Vec<u8>, IndexError> {
    let page_count: i64 = connection.pragma_query_value(None, "page_count", |row| row.get(0))?;
    let one = u64::try_from(page_count)
        .map_err(|_| IndexError::ProjectionContractViolation)?
        .checked_mul(PAGE_SIZE)
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?;
    if one == 0 || one > limits.max_serialized_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_serialized_bytes,
            observed: one,
        });
    }
    // At this point the build connection still owns its page cache while
    // SQLite exposes a serialized view and `to_vec` creates the publication
    // image.  Publication begins only after its caller drops `connection`.
    let cache = configured_cache_bytes(
        limits,
        build_cache_reservation_with_journal(limits, retained_view)?,
    )?;
    let peak = retained_view
        .checked_add(one.checked_mul(3).ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?)
        .and_then(|value| value.checked_add(cache))
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?;
    if peak > limits.max_working_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: peak,
        });
    }
    // `Connection::serialize` may allocate its returned view, so admission is
    // based on checked page_count * page_size before entering SQLite.
    let image = connection.serialize(MAIN_DB)?;
    if u64::try_from(image.len()).map_err(|_| IndexError::IntegerOutOfRange)? != one {
        return Err(IndexError::ProjectionContractViolation);
    }
    let bytes = image.to_vec();
    check_image_len(bytes.len(), limits)?;
    Ok(bytes)
}

/// Reconstructs an image solely into an ephemeral SQLite connection.  The
/// caller retains no borrowed image and the connection is query-only before
/// a marker or projection table can be inspected.
#[cfg(test)]
pub(crate) fn deserialize_read_only(
    image: Vec<u8>,
    limits: IndexLimits,
) -> Result<Connection, IndexError> {
    deserialize_read_only_with_journal(image, limits, 0)
}

fn deserialize_read_only_with_journal(
    image: Vec<u8>,
    limits: IndexLimits,
    journal_view: u64,
) -> Result<Connection, IndexError> {
    let image_len = image.len();
    check_image_len(image_len, limits)?;
    let image_bytes = u64::try_from(image_len).map_err(|_| IndexError::IntegerOutOfRange)?;
    // The input Vec is moved into Cursor below and released by
    // `deserialize_read_exact`; before that transfer, account it beside the
    // reconstructed main image and the query cache. Public result material is
    // charged later, after this Vec has gone away.
    let cache = configured_cache_bytes(
        limits,
        query_cache_reservation_with_journal(limits, journal_view)?,
    )?;
    let deserialize_peak = journal_view
        .checked_add(image_bytes)
        .and_then(|value| value.checked_add(image_bytes))
        .and_then(|value| value.checked_add(cache))
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?;
    if deserialize_peak > limits.max_working_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: deserialize_peak,
        });
    }
    let mut connection = Connection::open_in_memory_with_flags(
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.deserialize_read_exact(MAIN_DB, Cursor::new(image), image_len, true)?;
    // Classification is the first read from untrusted reconstructed bytes.
    // No pragma mutation, limit installation, or schema/table access may
    // precede the disposable schema-version decision.
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 1 || version == 2 {
        return Err(IndexError::RebuildRequired {
            found: u32::try_from(version).map_err(|_| IndexError::CorruptIndex)?,
            required: INDEX_SCHEMA_VERSION,
        });
    }
    if version != i64::from(INDEX_SCHEMA_VERSION) {
        return Err(IndexError::CorruptIndex);
    }
    configure_query_connection(&connection, limits, journal_view)?;
    connection.pragma_update(None, "query_only", true)?;
    let query_only: i64 = connection.pragma_query_value(None, "query_only", |row| row.get(0))?;
    // SQLite reports an in-memory deserialize as writable even with its
    // read-only deserialize flag; `query_only=1` is the enforceable mutation
    // boundary for this path and is read back below.
    if query_only != 1 {
        return Err(IndexError::CorruptIndex);
    }
    Ok(connection)
}

fn configure_query_connection(
    connection: &Connection,
    limits: IndexLimits,
    journal_view: u64,
) -> Result<(), IndexError> {
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "trusted_schema", false)?;
    let cache_kib = cache_kib(
        limits,
        query_cache_reservation_with_journal(limits, journal_view)?,
    )?;
    connection.pragma_update(None, "cache_size", -cache_kib)?;
    let temp_store: i64 = connection.pragma_query_value(None, "temp_store", |row| row.get(0))?;
    let foreign_keys: i64 =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    let trusted_schema: i64 =
        connection.pragma_query_value(None, "trusted_schema", |row| row.get(0))?;
    let cache_size: i64 = connection.pragma_query_value(None, "cache_size", |row| row.get(0))?;
    if temp_store != 2 || foreign_keys != 1 || trusted_schema != 0 || cache_size != -cache_kib {
        return Err(IndexError::CorruptIndex);
    }
    let length =
        i32::try_from(limits.max_serialized_bytes).map_err(|_| IndexError::IntegerOutOfRange)?;
    let sql =
        i32::try_from(limits.max_statement_bytes).map_err(|_| IndexError::IntegerOutOfRange)?;
    for (kind, value) in [
        (Limit::SQLITE_LIMIT_LENGTH, length),
        (Limit::SQLITE_LIMIT_SQL_LENGTH, sql),
        (Limit::SQLITE_LIMIT_COLUMN, 64),
        (Limit::SQLITE_LIMIT_COMPOUND_SELECT, 32),
        (Limit::SQLITE_LIMIT_EXPR_DEPTH, 128),
        (Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 256),
    ] {
        connection.set_limit(kind, value)?;
        if connection.limit(kind)? != value {
            return Err(IndexError::CorruptIndex);
        }
    }
    Ok(())
}

fn check_image_len(len: usize, limits: IndexLimits) -> Result<(), IndexError> {
    let observed = u64::try_from(len).map_err(|_| IndexError::IntegerOutOfRange)?;
    if observed == 0 || observed > limits.max_serialized_bytes || observed % PAGE_SIZE != 0 {
        return Err(IndexError::Incomplete {
            limit: limits.max_serialized_bytes,
            observed,
        });
    }
    Ok(())
}

#[cfg(test)]
fn build_cache_reservation(limits: IndexLimits) -> Result<u64, IndexError> {
    build_cache_reservation_with_journal(limits, 0)
}

fn build_cache_reservation_with_journal(
    limits: IndexLimits,
    retained_view: u64,
) -> Result<u64, IndexError> {
    retained_view
        .checked_add(
            limits
                .max_serialized_bytes
                .checked_mul(3)
                .ok_or(IndexError::Incomplete {
                    limit: limits.max_working_bytes,
                    observed: u64::MAX,
                })?,
        )
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })
}

#[cfg(test)]
fn query_cache_reservation(limits: IndexLimits) -> Result<u64, IndexError> {
    query_cache_reservation_with_journal(limits, 0)
}

fn query_cache_reservation_with_journal(
    limits: IndexLimits,
    journal_view: u64,
) -> Result<u64, IndexError> {
    journal_view
        .checked_add(
            limits
                .max_serialized_bytes
                .checked_mul(2)
                .ok_or(IndexError::Incomplete {
                    limit: limits.max_working_bytes,
                    observed: u64::MAX,
                })?,
        )
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?
        .checked_add(limits.max_query_bytes)
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })
}

fn cache_kib(limits: IndexLimits, reserved: u64) -> Result<i64, IndexError> {
    let required = reserved.checked_add(1024).ok_or(IndexError::Incomplete {
        limit: limits.max_working_bytes,
        observed: u64::MAX,
    })?;
    if required > limits.max_working_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: required,
        });
    }
    Ok(1)
}

fn configured_cache_bytes(limits: IndexLimits, reserved: u64) -> Result<u64, IndexError> {
    u64::try_from(cache_kib(limits, reserved)?)
        .map_err(|_| IndexError::IntegerOutOfRange)?
        .checked_mul(1024)
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })
}

fn admit_candidate_reread_peak(
    components: [u64; 7],
    working_limit: u64,
) -> Result<u64, IndexError> {
    let observed = components.into_iter().try_fold(0_u64, |used, value| {
        used.checked_add(value).ok_or(IndexError::Incomplete {
            limit: working_limit,
            observed: u64::MAX,
        })
    })?;
    if observed > working_limit {
        return Err(IndexError::Incomplete {
            limit: working_limit,
            observed,
        });
    }
    Ok(observed)
}

fn candidate_name(nonce: &[u8; 32]) -> String {
    let mut text = String::with_capacity("candidate-".len() + nonce.len() * 2 + ".sqlite".len());
    text.push_str("candidate-");
    for byte in nonce {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text.push_str(".sqlite");
    text
}

fn is_candidate_name(name: &str) -> bool {
    let Some(hex) = name
        .strip_prefix("candidate-")
        .and_then(|value| value.strip_suffix(".sqlite"))
    else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn read_fd_exact_with_retained(
    fd: OwnedFd,
    max: u64,
    retained: u64,
    working_limit: u64,
) -> Result<Vec<u8>, IndexError> {
    let before = fs::fstat(&fd)?;
    verify_index_stat(&before, FileType::RegularFile, 0o600)?;
    let size = u64::try_from(before.st_size).map_err(|_| IndexError::CorruptIndex)?;
    if size > max {
        return Err(IndexError::Incomplete {
            limit: max,
            observed: size,
        });
    }
    let observed = retained.checked_add(size).ok_or(IndexError::Incomplete {
        limit: working_limit,
        observed: u64::MAX,
    })?;
    if observed > working_limit {
        return Err(IndexError::Incomplete {
            limit: working_limit,
            observed,
        });
    }
    let capacity = usize::try_from(size).map_err(|_| IndexError::IntegerOutOfRange)?;
    let mut file = File::from(fd);
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| IndexError::Incomplete {
            limit: working_limit,
            observed,
        })?;
    bytes.resize(capacity, 0);
    if let Err(error) = file.read_exact(&mut bytes) {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            return Err(IndexError::CorruptIndex);
        }
        return Err(IndexError::Io(error));
    }
    let mut eof_probe = [0_u8; 1];
    if file.read(&mut eof_probe)? != 0 {
        return Err(IndexError::CorruptIndex);
    }
    let after = fs::fstat(&file)?;
    ensure_same_inode(&before, &after)?;
    let after_size = u64::try_from(after.st_size).map_err(|_| IndexError::IndexPathRace)?;
    if after_size != size {
        return Err(IndexError::IndexPathRace);
    }
    Ok(bytes)
}

fn verify_index_fd(fd: &OwnedFd, kind: FileType, mode: u32) -> Result<(), IndexError> {
    verify_index_stat(&fs::fstat(fd)?, kind, mode)
}

fn verify_index_stat(stat: &fs::Stat, kind: FileType, mode: u32) -> Result<(), IndexError> {
    if FileType::from_raw_mode(stat.st_mode) != kind
        || stat.st_uid != geteuid().as_raw()
        || stat.st_gid != getegid().as_raw()
        || Mode::from_raw_mode(stat.st_mode).as_raw_mode() & 0o7777 != mode
    {
        return Err(IndexError::CorruptIndex);
    }
    Ok(())
}

fn ensure_same_inode(left: &fs::Stat, right: &fs::Stat) -> Result<(), IndexError> {
    if left.st_dev != right.st_dev || left.st_ino != right.st_ino {
        Err(IndexError::IndexPathRace)
    } else {
        Ok(())
    }
}

fn map_entry_error(error: rustix::io::Errno) -> IndexError {
    if error == rustix::io::Errno::NOENT {
        IndexError::Missing
    } else if error == rustix::io::Errno::LOOP || error == rustix::io::Errno::NOTDIR {
        IndexError::CorruptIndex
    } else {
        IndexError::Store(StoreError::Io(error))
    }
}

fn normalize_external_image_error(error: IndexError) -> IndexError {
    match error {
        IndexError::Incomplete { .. }
        | IndexError::InvalidLimits
        | IndexError::IntegerOutOfRange
        | IndexError::RebuildRequired { .. } => error,
        _ => IndexError::CorruptIndex,
    }
}

fn map_sql(error: rusqlite::Error) -> IndexError {
    match error {
        rusqlite::Error::SqliteFailure(native, _) => match native.code {
            rusqlite::ffi::ErrorCode::ConstraintViolation
                if native.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
                    || native.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                IndexError::DuplicateIndexKey
            }
            rusqlite::ffi::ErrorCode::ConstraintViolation => {
                IndexError::ProjectionContractViolation
            }
            rusqlite::ffi::ErrorCode::DiskFull
            | rusqlite::ffi::ErrorCode::TooBig
            | rusqlite::ffi::ErrorCode::OutOfMemory => IndexError::Incomplete {
                limit: 0,
                observed: 0,
            },
            rusqlite::ffi::ErrorCode::DatabaseCorrupt
            | rusqlite::ffi::ErrorCode::NotADatabase
            | rusqlite::ffi::ErrorCode::SchemaChanged
            | rusqlite::ffi::ErrorCode::TypeMismatch => IndexError::CorruptIndex,
            _ => IndexError::Sql(rusqlite::Error::SqliteFailure(native, None)),
        },
        other => IndexError::Sql(other),
    }
}

fn to_i64(value: u64) -> Result<i64, IndexError> {
    i64::try_from(value).map_err(|_| IndexError::IntegerOutOfRange)
}

fn reserve_row(rows: &mut u64, limits: IndexLimits) -> Result<(), IndexError> {
    *rows = rows.checked_add(1).ok_or(IndexError::Incomplete {
        limit: limits.max_rows,
        observed: u64::MAX,
    })?;
    if *rows > limits.max_rows {
        return Err(IndexError::Incomplete {
            limit: limits.max_rows,
            observed: *rows,
        });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_index_marker(
    tx: &rusqlite::Transaction<'_>,
    identity: &JournalIdentity,
    version: EventContractVersion,
    mode: &str,
    offset: u64,
    tail: &ContentHash,
    event_count: u64,
) -> Result<(), IndexError> {
    tx.execute(
        "INSERT INTO index_meta(singleton,index_schema_version,projection_contract_version,event_contract_version,projection_mode,run_id,genesis_hash,confirmed_offset,tail_hash,event_count) VALUES(1,3,'reviewgraphen.index_projection.v3',?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![
            version.schema(),
            mode,
            identity.run_id.to_string(),
            identity.genesis_hash().to_string(),
            to_i64(offset)?,
            tail.to_string(),
            to_i64(event_count)?
        ],
    )
    .map_err(map_sql)?;
    Ok(())
}

fn validate_index_marker_version(connection: &Connection) -> Result<(), IndexError> {
    let user_version: i64 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let count: i64 =
        connection.query_row("SELECT COUNT(*) FROM index_meta", [], |row| row.get(0))?;
    let (schema_version, projection): (i64, String) = connection.query_row(
        "SELECT index_schema_version,projection_contract_version FROM index_meta WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if count != 1
        || user_version != i64::from(INDEX_SCHEMA_VERSION)
        || schema_version != i64::from(INDEX_SCHEMA_VERSION)
        || projection != PROJECTION_CONTRACT_VERSION
    {
        return Err(IndexError::ProjectionContractViolation);
    }
    Ok(())
}

fn insert_event(
    tx: &rusqlite::Transaction<'_>,
    event: &EventEnvelope,
    schema: &str,
    kind: &'static str,
) -> Result<(), IndexError> {
    tx.execute("INSERT INTO events(sequence,event_id,schema,event_hash,payload_hash,payload_kind,actor,logical_time) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", rusqlite::params![to_i64(event.sequence())?, event.id().to_string(), schema, event.event_hash().to_string(), event.payload_hash().to_string(), kind, event.actor(), to_i64(event.logical_time())?]).map_err(map_sql)?;
    Ok(())
}

fn serialized_enum<T: Serialize>(value: &T) -> Result<String, IndexError> {
    serde_json::from_slice::<String>(
        &canonical_json(value).map_err(|_| IndexError::ProjectionContractViolation)?,
    )
    .map_err(|_| IndexError::ProjectionContractViolation)
}

fn canonical_ids(values: impl IntoIterator<Item = StableId>) -> Result<String, IndexError> {
    let values = values.into_iter().collect::<BTreeSet<_>>();
    let values = values
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    String::from_utf8(canonical_json(&values).map_err(|_| IndexError::ProjectionContractViolation)?)
        .map_err(|_| IndexError::ProjectionContractViolation)
}

fn validate_canonical_ids(value: &str) -> Result<(), IndexError> {
    let raw: Vec<String> = serde_json::from_str(value).map_err(|_| IndexError::CorruptIndex)?;
    let ids = raw
        .into_iter()
        .map(StableId::parse)
        .collect::<reviewgraphen_core::Result<Vec<_>>>()
        .map_err(|_| IndexError::CorruptIndex)?;
    if ids.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(IndexError::CorruptIndex);
    }
    let canonical = canonical_ids(ids)?;
    if canonical.as_bytes() != value.as_bytes() {
        return Err(IndexError::CorruptIndex);
    }
    Ok(())
}

fn body_hash<T: Serialize>(value: &T) -> Result<ContentHash, IndexError> {
    Ok(ContentHash::sha256(
        &canonical_json(value).map_err(|_| IndexError::ProjectionContractViolation)?,
    ))
}

fn insert_shadow_record<T: Serialize>(
    tx: &rusqlite::Transaction<'_>,
    event: &EventEnvelope,
    record_id: &StableId,
    kind: &'static str,
    value: &T,
    rows: &mut u64,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    reserve_row(rows, limits)?;
    tx.execute(
        "INSERT INTO unreconciled_authority_records(event_sequence,event_id,record_id,kind,body_hash,authority_reconciled) VALUES(?1,?2,?3,?4,?5,0)",
        rusqlite::params![
            to_i64(event.sequence())?,
            event.id().to_string(),
            record_id.to_string(),
            kind,
            body_hash(value)?.to_string()
        ],
    )
    .map_err(map_sql)?;
    Ok(())
}

fn insert_baseline(
    tx: &rusqlite::Transaction<'_>,
    genesis: &RunGenesisSnapshot,
    rows: &mut u64,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    let program = genesis.program_space();
    for artifact in program.artifacts() {
        reserve_row(rows, limits)?;
        tx.execute(
            "INSERT INTO program_objects(object_id,object_kind,body_hash) VALUES(?1,?2,?3)",
            rusqlite::params![
                artifact.id.to_string(),
                artifact.kind,
                body_hash(artifact)?.to_string()
            ],
        )
        .map_err(map_sql)?;
    }
    for relation in program.relations() {
        reserve_row(rows, limits)?;
        tx.execute("INSERT INTO program_relations(relation_id,relation_kind,source_id,target_ids_canonical_json,body_hash) VALUES(?1,?2,?3,?4,?5)", rusqlite::params![relation.id.to_string(), relation.kind, relation.source_id.to_string(), canonical_ids(relation.target_ids.iter().cloned())?, body_hash(relation)?.to_string()]).map_err(map_sql)?;
    }
    reserve_row(rows, limits)?;
    let universe = genesis.universe();
    tx.execute(
        "INSERT INTO universe(singleton,universe_id,snapshot_id,profile_id,rule_set_hash,extractor_set_hash,policy_version,rule_pack_version,body_hash) VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8)",
        rusqlite::params![universe.id().to_string(), universe.snapshot_id().to_string(), universe.profile_id(), universe.rule_set_hash().to_string(), universe.extractor_set_hash().to_string(), universe.policy_version(), universe.rule_pack_version(), body_hash(universe)?.to_string()],
    )
    .map_err(map_sql)?;
    for obligation in genesis.obligations() {
        reserve_row(rows, limits)?;
        tx.execute("INSERT INTO obligations(obligation_id,target_kind,target_ids_canonical_json,property_id,lifecycle,body_hash) VALUES(?1,?2,?3,?4,?5,?6)", rusqlite::params![obligation.id().to_string(), obligation.target_kind(), canonical_ids(obligation.normalized_target_refs().iter().cloned())?, obligation.property_id(), serialized_enum(&obligation.lifecycle())?, body_hash(obligation)?.to_string()]).map_err(map_sql)?;
    }
    Ok(())
}

fn canonical_string_set(values: &BTreeSet<String>) -> Result<String, IndexError> {
    String::from_utf8(canonical_json(values).map_err(|_| IndexError::ProjectionContractViolation)?)
        .map_err(|_| IndexError::ProjectionContractViolation)
}

/// Checks the whole D2 reference closure against rows already projected by
/// earlier journal events. This runs before body construction, row admission,
/// or an INSERT; SQLite foreign keys remain a backstop rather than the domain
/// validator for an atomic execution event.
fn prevalidate_execution_domain(
    tx: &rusqlite::Transaction<'_>,
    event: &EventEnvelope,
    execution: &reviewgraphen_core::ExecutionRecord,
    claims: &[reviewgraphen_core::ExecutionClaimV2],
) -> Result<(), IndexError> {
    let missing = || IndexError::ProjectionContractViolation;
    let (plan_snapshot, waves): (String, String) = tx
        .query_row(
            "SELECT snapshot_id,waves_canonical_json FROM review_plans WHERE plan_id=?1",
            [execution.plan_id().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| missing())?;
    let plan_snapshot = parse_id(plan_snapshot)?;
    let waves: Vec<WaveWire> = exact_typed_json(&waves)?;
    let wave = waves
        .iter()
        .find(|candidate| {
            wave_id(execution.plan_id(), candidate)
                .as_ref()
                .is_ok_and(|id| id == execution.wave_id())
        })
        .ok_or_else(missing)?;
    let wave_ids = wave.obligation_ids.iter().cloned().collect::<BTreeSet<_>>();

    let (envelope_snapshot, envelope_obligations, envelope_sources): (String, String, String) = tx
        .query_row(
            "SELECT snapshot_id,obligation_ids_canonical_json,included_sources_canonical_json FROM context_envelopes WHERE envelope_id=?1",
            [execution.envelope_id().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| missing())?;
    let envelope_snapshot = parse_id(envelope_snapshot)?;
    let envelope_ids = exact_id_set(&envelope_obligations)?;
    let sources: Vec<reviewgraphen_core::SourceArtifactRef> = exact_typed_json(&envelope_sources)?;
    let source_ids = sources
        .iter()
        .map(|source| source.artifact_id().clone())
        .collect::<BTreeSet<_>>();

    let (raw_hash, sensitivity, source): (String, String, String) = tx
        .query_row(
            "SELECT cas_hash,sensitivity,source_id FROM artifact_registrations WHERE registration_id=?1",
            [execution.raw_artifact_registration_id().to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|_| missing())?;
    let raw_hash = parse_hash(raw_hash)?;
    let sensitivity: reviewgraphen_core::ArtifactSensitivity = parse_closed_enum(&sensitivity)?;
    let source: reviewgraphen_core::ArtifactSource = exact_typed_json(&source)?;

    if plan_snapshot != *execution.snapshot_id()
        || envelope_snapshot != *execution.snapshot_id()
        || envelope_ids != *execution.obligation_ids()
        || !execution.obligation_ids().is_subset(&wave_ids)
        || raw_hash != *execution.raw_artifact_hash()
        || sensitivity != reviewgraphen_core::ArtifactSensitivity::Sensitive
        || !matches!(source, reviewgraphen_core::ArtifactSource::ReviewerExecution { run_id, execution_id, reviewer_id } if run_id == *event.run_id() && execution_id == *execution.id() && reviewer_id == execution.reviewer_id())
    {
        return Err(missing());
    }

    let claim_ids = claims
        .iter()
        .map(|claim| claim.id().clone())
        .collect::<BTreeSet<_>>();
    if claim_ids.len() != claims.len() || claim_ids != *execution.parsed_claim_ids() {
        return Err(missing());
    }
    for claim in claims {
        let obligation_id = claim.obligation_ids().iter().next().ok_or_else(missing)?;
        let (property_id, targets): (String, String) = tx
            .query_row(
                "SELECT property_id,target_ids_canonical_json FROM obligations WHERE obligation_id=?1",
                [obligation_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| missing())?;
        let targets = exact_id_set(&targets)?;
        if claim.execution_id() != execution.id()
            || claim.obligation_ids() != execution.obligation_ids()
            || claim.property_id() != property_id
            || !claim.target_refs().is_subset(&targets)
            || !claim.source_ids().is_subset(&source_ids)
        {
            return Err(missing());
        }
    }
    if (execution.outcome().is_structured() && claims.is_empty())
        || (!execution.outcome().is_structured() && !claims.is_empty())
    {
        return Err(missing());
    }
    Ok(())
}

/// D2's execution and all of its claims are one canonical event.  SQLite is
/// only a projection, but it keeps the same all-or-nothing seam as the event:
/// every row is precomputed and validated before the first INSERT and both
/// tables are written through the caller's single transaction.
fn insert_execution_and_claims(
    tx: &rusqlite::Transaction<'_>,
    event: &EventEnvelope,
    execution: &reviewgraphen_core::ExecutionRecord,
    claims: &[reviewgraphen_core::ExecutionClaimV2],
    rows: &mut u64,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    prevalidate_execution_domain(tx, event, execution, claims)?;
    let obligation_ids = canonical_ids(execution.obligation_ids().iter().cloned())?;
    let inference_settings = String::from_utf8(
        canonical_json(execution.inference_settings())
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    )
    .map_err(|_| IndexError::ProjectionContractViolation)?;
    let parsed_claim_ids = canonical_ids(execution.parsed_claim_ids().iter().cloned())?;
    let outcome = String::from_utf8(
        canonical_json(execution.outcome()).map_err(|_| IndexError::ProjectionContractViolation)?,
    )
    .map_err(|_| IndexError::ProjectionContractViolation)?;
    let outcome_kind = match execution.outcome() {
        reviewgraphen_core::ExecutionOutcome::Structured => "structured",
        reviewgraphen_core::ExecutionOutcome::Abstained { .. } => "abstained",
        reviewgraphen_core::ExecutionOutcome::Malformed { .. } => "malformed",
        reviewgraphen_core::ExecutionOutcome::ProviderFailure { .. } => "provider_failure",
    };

    // Construct each complete canonical body before reserving the first row,
    // so a malformed claim cannot leave an execution-only projection behind.
    let mut projected_claims = Vec::new();
    projected_claims
        .try_reserve_exact(claims.len())
        .map_err(|_| IndexError::Incomplete {
            limit: limits.max_rows,
            observed: u64::MAX,
        })?;
    for claim in claims {
        let projected = IndexClaim {
            event_sequence: event.sequence(),
            event_id: event.id().clone(),
            claim_id: claim.id().clone(),
            execution_id: claim.execution_id().clone(),
            obligation_ids_canonical_json: canonical_ids(claim.obligation_ids().iter().cloned())?,
            property_id: claim.property_id().to_owned(),
            target_refs_canonical_json: canonical_ids(claim.target_refs().iter().cloned())?,
            polarity: serialized_enum(&claim.polarity())?,
            disposition: serialized_enum(&claim.disposition())?,
            summary: claim.summary().to_owned(),
            source_ids_canonical_json: canonical_ids(claim.source_ids().iter().cloned())?,
            assumptions_canonical_json: canonical_string_set(claim.assumptions())?,
            requested_evidence_canonical_json: canonical_string_set(claim.requested_evidence())?,
            candidate_confidence_canonical_json: String::from_utf8(
                canonical_json(&claim.candidate_confidence())
                    .map_err(|_| IndexError::ProjectionContractViolation)?,
            )
            .map_err(|_| IndexError::ProjectionContractViolation)?,
            author_kind: serialized_enum(&claim.author_kind())?,
            review_status: serialized_enum(&claim.review_status())?,
            identity_body_hash: claim
                .identity_body_hash()
                .map_err(|_| IndexError::ProjectionContractViolation)?,
            body_hash: claim
                .body_hash()
                .map_err(|_| IndexError::ProjectionContractViolation)?,
        };
        if projected.execution_id != *execution.id()
            || projected.obligation_ids_canonical_json != obligation_ids
        {
            return Err(IndexError::ProjectionContractViolation);
        }
        projected_claims.push(projected);
    }
    if projected_claims
        .iter()
        .map(|claim| claim.claim_id.clone())
        .collect::<BTreeSet<_>>()
        != *execution.parsed_claim_ids()
        || projected_claims
            .windows(2)
            .any(|pair| pair[0].claim_id >= pair[1].claim_id)
    {
        return Err(IndexError::ProjectionContractViolation);
    }

    let projected_execution = IndexExecution {
        event_sequence: event.sequence(),
        event_id: event.id().clone(),
        execution_id: execution.id().clone(),
        plan_id: execution.plan_id().clone(),
        wave_id: execution.wave_id().clone(),
        snapshot_id: execution.snapshot_id().clone(),
        envelope_id: execution.envelope_id().clone(),
        obligation_ids_canonical_json: obligation_ids,
        reviewer_kind: execution.reviewer_kind().to_owned(),
        reviewer_id: execution.reviewer_id().to_owned(),
        provider: execution.provider().map(str::to_owned),
        model: execution.model().map(str::to_owned),
        model_revision: execution.model_revision().map(str::to_owned),
        system_prompt_version: execution.system_prompt_version().to_owned(),
        prompt_template_version: execution.prompt_template_version().to_owned(),
        inference_settings_canonical_json: inference_settings,
        tool_policy_version: execution.tool_policy_version().to_owned(),
        tool_calls_canonical_json: "[]".to_owned(),
        attempt: execution.attempt(),
        raw_registration_id: execution.raw_artifact_registration_id().clone(),
        raw_hash: execution.raw_artifact_hash().clone(),
        parsed_claim_ids_canonical_json: parsed_claim_ids,
        outcome_kind: outcome_kind.to_owned(),
        outcome_canonical_json: outcome,
        identity_body_hash: execution
            .identity_body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
        body_hash: execution
            .body_hash()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    };

    reserve_row(rows, limits)?;
    for _ in &projected_claims {
        reserve_row(rows, limits)?;
    }
    tx.execute(
        "INSERT INTO executions(event_sequence,event_id,execution_id,plan_id,wave_id,snapshot_id,envelope_id,obligation_ids_canonical_json,reviewer_kind,reviewer_id,provider,model,model_revision,system_prompt_version,prompt_template_version,inference_settings_canonical_json,tool_policy_version,tool_calls_canonical_json,attempt,raw_registration_id,raw_hash,parsed_claim_ids_canonical_json,outcome_kind,outcome_canonical_json,identity_body_hash,body_hash) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)",
        rusqlite::params![to_i64(projected_execution.event_sequence)?, projected_execution.event_id.to_string(), projected_execution.execution_id.to_string(), projected_execution.plan_id.to_string(), projected_execution.wave_id.to_string(), projected_execution.snapshot_id.to_string(), projected_execution.envelope_id.to_string(), projected_execution.obligation_ids_canonical_json, projected_execution.reviewer_kind, projected_execution.reviewer_id, projected_execution.provider, projected_execution.model, projected_execution.model_revision, projected_execution.system_prompt_version, projected_execution.prompt_template_version, projected_execution.inference_settings_canonical_json, projected_execution.tool_policy_version, projected_execution.tool_calls_canonical_json, i64::from(projected_execution.attempt), projected_execution.raw_registration_id.to_string(), projected_execution.raw_hash.to_string(), projected_execution.parsed_claim_ids_canonical_json, projected_execution.outcome_kind, projected_execution.outcome_canonical_json, projected_execution.identity_body_hash.to_string(), projected_execution.body_hash.to_string()],
    )
    .map_err(map_sql)?;
    for claim in projected_claims {
        tx.execute(
            "INSERT INTO claims(event_sequence,event_id,claim_id,execution_id,obligation_ids_canonical_json,property_id,target_refs_canonical_json,polarity,disposition,summary,source_ids_canonical_json,assumptions_canonical_json,requested_evidence_canonical_json,candidate_confidence_canonical_json,author_kind,review_status,identity_body_hash,body_hash) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            rusqlite::params![to_i64(claim.event_sequence)?, claim.event_id.to_string(), claim.claim_id.to_string(), claim.execution_id.to_string(), claim.obligation_ids_canonical_json, claim.property_id, claim.target_refs_canonical_json, claim.polarity, claim.disposition, claim.summary, claim.source_ids_canonical_json, claim.assumptions_canonical_json, claim.requested_evidence_canonical_json, claim.candidate_confidence_canonical_json, claim.author_kind, claim.review_status, claim.identity_body_hash.to_string(), claim.body_hash.to_string()],
        )
        .map_err(map_sql)?;
    }
    Ok(())
}

fn artifact_source_kind(source: &reviewgraphen_core::ArtifactSource) -> Result<String, IndexError> {
    let value =
        serde_json::to_value(source).map_err(|_| IndexError::ProjectionContractViolation)?;
    value
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or(IndexError::ProjectionContractViolation)
}

fn insert_registration(
    tx: &rusqlite::Transaction<'_>,
    event: &EventEnvelope,
    registration: &reviewgraphen_core::ArtifactRegistered,
    rows: &mut u64,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    reserve_row(rows, limits)?;
    let source_id = String::from_utf8(
        canonical_json(registration.source())
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    )
    .map_err(|_| IndexError::ProjectionContractViolation)?;
    tx.execute("INSERT INTO artifact_registrations(event_sequence,event_id,registration_id,run_id,cas_hash,media_type,size,sensitivity,source_kind,source_id,body_hash) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", rusqlite::params![to_i64(event.sequence())?,event.id().to_string(),registration.registration_id().to_string(),registration.run_id().to_string(),registration.cas_hash().to_string(),registration.media_type(),to_i64(registration.size())?,serialized_enum(&registration.sensitivity())?,artifact_source_kind(registration.source())?,source_id,body_hash(registration)?.to_string()]).map_err(map_sql)?;
    Ok(())
}

fn insert_sources(
    tx: &rusqlite::Transaction<'_>,
    event: &EventEnvelope,
    sources: &reviewgraphen_core::SnapshotSourcesRecorded,
    rows: &mut u64,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    for entry in sources.entries() {
        reserve_row(rows, limits)?;
        tx.execute("INSERT INTO snapshot_source_index(event_sequence,event_id,snapshot_id,artifact_id,registration_id,path,content_hash,cas_hash,line_count) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", rusqlite::params![to_i64(event.sequence())?,event.id().to_string(),sources.snapshot_id().to_string(),entry.artifact_id().to_string(),entry.registration_id().to_string(),entry.path(),entry.content_hash().to_string(),entry.cas_hash().to_string(),to_i64(entry.line_count())?]).map_err(map_sql)?;
    }
    Ok(())
}

fn canonical_component(
    object: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<String, IndexError> {
    let value = object
        .get(key)
        .ok_or(IndexError::ProjectionContractViolation)?;
    String::from_utf8(canonical_json(value).map_err(|_| IndexError::ProjectionContractViolation)?)
        .map_err(|_| IndexError::ProjectionContractViolation)
}

fn canonical_object(
    bytes: &[u8],
) -> Result<serde_json::Map<String, serde_json::Value>, IndexError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| IndexError::ProjectionContractViolation)?;
    value
        .as_object()
        .cloned()
        .ok_or(IndexError::ProjectionContractViolation)
}

fn insert_review_plan(
    tx: &rusqlite::Transaction<'_>,
    event: &EventEnvelope,
    plan: &ReviewPlan,
    rows: &mut u64,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    let row = projected_review_plan(event, plan)?;
    reserve_row(rows, limits)?;
    tx.execute(
        "INSERT INTO review_plans(event_sequence,event_id,plan_id,universe_id,snapshot_id,planner_input_hash,planner_policy_version,planner_policy_hash,budget_canonical_json,budget_hash,risk_breakdown_canonical_json,waves_canonical_json,deferred_canonical_json,identity_body_hash,body_hash) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        rusqlite::params![
            to_i64(row.event_sequence)?,
            row.event_id.to_string(),
            row.plan_id.to_string(),
            row.universe_id.to_string(),
            row.snapshot_id.to_string(),
            row.planner_input_hash.to_string(),
            row.planner_policy_version,
            row.planner_policy_hash.to_string(),
            row.budget_canonical_json,
            row.budget_hash.to_string(),
            row.risk_breakdown_canonical_json,
            row.waves_canonical_json,
            row.deferred_canonical_json,
            row.identity_body_hash.to_string(),
            row.body_hash.to_string(),
        ],
    )
    .map_err(map_sql)?;
    Ok(())
}

fn projected_review_plan(
    event: &EventEnvelope,
    plan: &ReviewPlan,
) -> Result<IndexReviewPlan, IndexError> {
    let body = plan
        .canonical_bytes()
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    let object = canonical_object(&body)?;
    let budget = String::from_utf8(
        canonical_json(&plan.budget()).map_err(|_| IndexError::ProjectionContractViolation)?,
    )
    .map_err(|_| IndexError::ProjectionContractViolation)?;
    if budget.len() > 50 || canonical_component(&object, "budget")? != budget {
        return Err(IndexError::ProjectionContractViolation);
    }
    let budget_hash = ContentHash::sha256(budget.as_bytes());
    let identity_body_hash = plan
        .identity_body_hash()
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    if plan.id()
        != &StableId::parse(format!("plan:{identity_body_hash}"))
            .map_err(|_| IndexError::ProjectionContractViolation)?
        || plan.planner_policy_hash()
            != &PlannerPolicyV1::baseline()
                .hash()
                .map_err(|_| IndexError::ProjectionContractViolation)?
    {
        return Err(IndexError::ProjectionContractViolation);
    }
    Ok(IndexReviewPlan {
        event_sequence: event.sequence(),
        event_id: event.id().clone(),
        plan_id: plan.id().clone(),
        universe_id: plan.universe_id().clone(),
        snapshot_id: plan.snapshot_id().clone(),
        planner_input_hash: plan.planner_input_hash().clone(),
        planner_policy_version: plan.planner_policy_version().to_owned(),
        planner_policy_hash: plan.planner_policy_hash().clone(),
        budget_canonical_json: budget,
        budget_hash,
        risk_breakdown_canonical_json: canonical_component(&object, "risk_breakdown")?,
        waves_canonical_json: canonical_component(&object, "waves")?,
        deferred_canonical_json: canonical_component(&object, "deferred")?,
        identity_body_hash,
        body_hash: ContentHash::sha256(&body),
    })
}

fn insert_context_envelope(
    tx: &rusqlite::Transaction<'_>,
    event: &EventEnvelope,
    envelope: &ReviewContextEnvelope,
    rows: &mut u64,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    let row = projected_context_envelope(event, envelope)?;
    reserve_row(rows, limits)?;
    tx.execute(
        "INSERT INTO context_envelopes(event_sequence,event_id,envelope_id,snapshot_id,context_policy_version,context_policy_hash,candidate_ids_canonical_json,obligation_ids_canonical_json,context_policy_canonical_json,included_sources_canonical_json,excluded_sources_canonical_json,unknowns_canonical_json,assumptions_canonical_json,losses_canonical_json,projection_hash,body_hash) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        rusqlite::params![
            to_i64(row.event_sequence)?,
            row.event_id.to_string(),
            row.envelope_id.to_string(),
            row.snapshot_id.to_string(),
            row.context_policy_version,
            row.context_policy_hash.to_string(),
            row.candidate_ids_canonical_json,
            row.obligation_ids_canonical_json,
            row.context_policy_canonical_json,
            row.included_sources_canonical_json,
            row.excluded_sources_canonical_json,
            row.unknowns_canonical_json,
            row.assumptions_canonical_json,
            row.losses_canonical_json,
            row.projection_hash.to_string(),
            row.body_hash.to_string(),
        ],
    )
    .map_err(map_sql)?;
    Ok(())
}

fn projected_context_envelope(
    event: &EventEnvelope,
    envelope: &ReviewContextEnvelope,
) -> Result<IndexContextEnvelope, IndexError> {
    let body = envelope
        .canonical_bytes()
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    let object = canonical_object(&body)?;
    let policy = String::from_utf8(
        envelope
            .context_policy()
            .canonical_bytes()
            .map_err(|_| IndexError::ProjectionContractViolation)?,
    )
    .map_err(|_| IndexError::ProjectionContractViolation)?;
    let policy_matches = envelope.context_policy() == &ContextPolicyV1::baseline();
    let hash_matches = envelope.context_policy_hash() == &ContentHash::sha256(policy.as_bytes());
    let id_matches = envelope.id()
        == &StableId::parse(format!("context-envelope:{}", envelope.projection_hash()))
            .map_err(|_| IndexError::ProjectionContractViolation)?;
    if !policy_matches || !hash_matches || !id_matches {
        return Err(IndexError::ProjectionContractViolation);
    }
    Ok(IndexContextEnvelope {
        event_sequence: event.sequence(),
        event_id: event.id().clone(),
        envelope_id: envelope.id().clone(),
        snapshot_id: envelope.snapshot_id().clone(),
        context_policy_version: envelope.projection_policy_version().to_owned(),
        context_policy_hash: envelope.context_policy_hash().clone(),
        candidate_ids_canonical_json: canonical_component(&object, "candidate_source_ids")?,
        obligation_ids_canonical_json: canonical_component(&object, "obligation_ids")?,
        context_policy_canonical_json: policy,
        included_sources_canonical_json: canonical_component(&object, "included_sources")?,
        excluded_sources_canonical_json: canonical_component(&object, "excluded_sources")?,
        unknowns_canonical_json: canonical_component(&object, "unknowns")?,
        assumptions_canonical_json: canonical_component(&object, "assumptions")?,
        losses_canonical_json: canonical_component(&object, "losses")?,
        projection_hash: envelope.projection_hash().clone(),
        body_hash: ContentHash::sha256(&body),
    })
}

fn payload_kind_decoded(payload: &DecodedPayload) -> Result<&'static str, IndexError> {
    Ok(match payload {
        DecodedPayload::ObligationTransition { .. } => "obligation_transition",
        DecodedPayload::ClaimProposed(_) => "claim_proposed",
        DecodedPayload::EvidenceRecorded(_) => "evidence_recorded",
        DecodedPayload::EvidenceBound(_) => "evidence_bound",
        DecodedPayload::VerificationRecorded(_) => "verification_recorded",
        DecodedPayload::DecisionRecorded(_) => "decision_recorded",
        DecodedPayload::FindingRecorded(_) => "finding_recorded",
        DecodedPayload::RunGenesisManifest(_) => "run_genesis_manifest",
        DecodedPayload::RunGenesisManifestV3(_) => {
            return Err(IndexError::ProjectionContractViolation);
        }
        DecodedPayload::ArtifactRegistered(_) => "artifact_registered",
        DecodedPayload::ArtifactRegisteredV3(_) => {
            return Err(IndexError::ProjectionContractViolation);
        }
        DecodedPayload::EvidenceRecordedV3(_)
        | DecodedPayload::EvidenceBoundV3(_)
        | DecodedPayload::VerificationRecordedV3(_)
        | DecodedPayload::DecisionRecordedV3(_)
        | DecodedPayload::FindingRecordedV3(_) => {
            return Err(IndexError::ProjectionContractViolation);
        }
        DecodedPayload::SnapshotSourcesRecorded(_) => "snapshot_sources_recorded",
        DecodedPayload::ReviewPlanRecorded(_) => "review_plan_recorded",
        DecodedPayload::ContextEnvelopeProjected(_) => "context_envelope_projected",
        DecodedPayload::ReviewExecutionRecorded { .. } => "review_execution_recorded",
    })
}

fn validate_offline_classification(
    applied_to_accepted: bool,
    payload: &DecodedPayload,
) -> Result<(), IndexError> {
    match (applied_to_accepted, payload) {
        (
            false,
            DecodedPayload::EvidenceRecorded(_)
            | DecodedPayload::EvidenceBound(_)
            | DecodedPayload::VerificationRecorded(_)
            | DecodedPayload::DecisionRecorded(_)
            | DecodedPayload::FindingRecorded(_)
            | DecodedPayload::ContextEnvelopeProjected(_)
            | DecodedPayload::ReviewExecutionRecorded { .. },
        )
        | (
            true,
            DecodedPayload::ObligationTransition { .. }
            | DecodedPayload::ClaimProposed(_)
            | DecodedPayload::RunGenesisManifest(_)
            | DecodedPayload::ArtifactRegistered(_)
            | DecodedPayload::SnapshotSourcesRecorded(_)
            | DecodedPayload::ReviewPlanRecorded(_),
        ) => Ok(()),
        _ => Err(IndexError::ProjectionContractViolation),
    }
}

fn map_index_journal(error: JournalError) -> IndexError {
    match error {
        JournalError::Incomplete { limit, observed } => IndexError::Incomplete { limit, observed },
        other => IndexError::Journal(other),
    }
}

fn streaming_journal_retained(
    reader: &super::journal::IndexJournalReader,
    identity: &JournalIdentity,
    limits: IndexLimits,
) -> Result<u64, IndexError> {
    let genesis = identity
        .v2_genesis_backing()
        .map_or(0, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
    let metadata = reader
        .retained_metadata_capacity()
        .map_err(map_index_journal)?;
    let local_identity = identity
        .cloned_local_metadata_capacity()
        .map_err(map_index_journal)?;
    let retained = genesis
        .checked_add(metadata)
        .and_then(|value| value.checked_add(local_identity))
        // `EventJournal` retains the originating identity while the reader
        // and this cursor copy are live. Its V2 Arc allocations are shared;
        // only the local run/V1-hash backing is an additional request.
        .and_then(|value| value.checked_add(local_identity))
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?;
    if retained > limits.max_working_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: retained,
        });
    }
    Ok(retained)
}

fn streaming_projection(
    identity: &JournalIdentity,
) -> Result<Option<OfflineProjectionState>, IndexError> {
    if identity.version() == EventContractVersion::V1 {
        return Ok(None);
    }
    let bytes = identity
        .v2_genesis_backing()
        .ok_or(IndexError::ProjectionContractViolation)?;
    let initial = RunGenesisSnapshot::from_canonical_bytes_for_index(bytes)
        .and_then(|snapshot| snapshot.rebuild_aggregate())
        .map_err(|_| IndexError::ProjectionContractViolation)?;
    let verified = match identity
        .verified_core_genesis()
        .map_err(map_index_journal)?
    {
        reviewgraphen_core::EventStreamGenesis::V2Verified(verified) => verified,
        _ => return Err(IndexError::ProjectionContractViolation),
    };
    OfflineProjectionState::new_streaming_v2(&identity.run_id, verified, initial)
        .map(Some)
        .map_err(|_| IndexError::ProjectionContractViolation)
}

fn admit_streaming_scan(
    retained: u64,
    certificate: &super::journal::PrefixCertificate,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    let observed = retained
        .checked_add(certificate.line_buffer_capacity)
        .and_then(|value| value.checked_add(certificate.canonical_scratch_capacity))
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?;
    if observed > limits.max_working_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed,
        });
    }
    Ok(())
}

fn map_index_replay(error: IndexReplayError<IndexError>) -> IndexError {
    match error {
        IndexReplayError::Journal(error) => map_index_journal(error),
        IndexReplayError::Visitor(error) => error,
    }
}

struct StreamingSnapshotComparator<'a> {
    snapshot: &'a IndexSnapshot,
    event_index: usize,
    registration_index: usize,
    plan_index: usize,
    context_index: usize,
    execution_index: usize,
    claim_index: usize,
    schema: &'static str,
}

impl<'a> StreamingSnapshotComparator<'a> {
    fn new(snapshot: &'a IndexSnapshot, version: EventContractVersion) -> Self {
        Self {
            snapshot,
            event_index: 0,
            registration_index: 0,
            plan_index: 0,
            context_index: 0,
            execution_index: 0,
            claim_index: 0,
            schema: version.schema(),
        }
    }

    fn compare_event(&mut self, envelope: &EventEnvelope) -> Result<(), IndexError> {
        let validated = envelope
            .decode_for_streaming_projection()
            .map_err(|_| IndexError::CorruptIndex)?;
        let row = self
            .snapshot
            .events
            .get(self.event_index)
            .ok_or(IndexError::CorruptIndex)?;
        if !event_row_matches(&validated, row, self.schema)? {
            return Err(IndexError::CorruptIndex);
        }
        self.event_index += 1;
        match validated.payload() {
            DecodedPayload::RunGenesisManifest(manifest) => {
                self.compare_registration(envelope, manifest.genesis_artifact())?;
            }
            DecodedPayload::ArtifactRegistered(registration) => {
                self.compare_registration(envelope, registration)?;
            }
            DecodedPayload::ReviewPlanRecorded(plan) => {
                let actual = self
                    .snapshot
                    .review_plans
                    .get(self.plan_index)
                    .ok_or(IndexError::CorruptIndex)?;
                if actual.event_sequence != envelope.sequence()
                    || actual.event_id != *envelope.id()
                    || actual.plan_id != *plan.id()
                    || actual.body_hash
                        != ContentHash::sha256(
                            &plan
                                .canonical_bytes()
                                .map_err(|_| IndexError::CorruptIndex)?,
                        )
                {
                    return Err(IndexError::CorruptIndex);
                }
                self.plan_index += 1;
            }
            DecodedPayload::ContextEnvelopeProjected(context) => {
                let actual = self
                    .snapshot
                    .context_envelopes
                    .get(self.context_index)
                    .ok_or(IndexError::CorruptIndex)?;
                if actual.event_sequence != envelope.sequence()
                    || actual.event_id != *envelope.id()
                    || actual.envelope_id != *context.id()
                    || actual.body_hash
                        != ContentHash::sha256(
                            &context
                                .canonical_bytes()
                                .map_err(|_| IndexError::CorruptIndex)?,
                        )
                {
                    return Err(IndexError::CorruptIndex);
                }
                self.context_index += 1;
            }
            DecodedPayload::ReviewExecutionRecorded { execution, claims } => {
                let actual = self
                    .snapshot
                    .executions
                    .get(self.execution_index)
                    .ok_or(IndexError::CorruptIndex)?;
                if actual.event_sequence != envelope.sequence()
                    || actual.event_id != *envelope.id()
                    || validate_execution_row(actual)? != *execution
                    || actual.body_hash
                        != ContentHash::sha256(
                            &execution
                                .canonical_bytes()
                                .map_err(|_| IndexError::CorruptIndex)?,
                        )
                {
                    return Err(IndexError::CorruptIndex);
                }
                self.execution_index += 1;
                for claim in claims {
                    let actual = self
                        .snapshot
                        .claims
                        .get(self.claim_index)
                        .ok_or(IndexError::CorruptIndex)?;
                    if actual.event_sequence != envelope.sequence()
                        || actual.event_id != *envelope.id()
                        || validate_claim_row(actual)? != *claim
                        || actual.body_hash
                            != ContentHash::sha256(
                                &claim
                                    .canonical_bytes()
                                    .map_err(|_| IndexError::CorruptIndex)?,
                            )
                    {
                        return Err(IndexError::CorruptIndex);
                    }
                    self.claim_index += 1;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn compare_registration(
        &mut self,
        envelope: &EventEnvelope,
        registration: &reviewgraphen_core::ArtifactRegistered,
    ) -> Result<(), IndexError> {
        let actual = self
            .snapshot
            .artifact_registrations
            .get(self.registration_index)
            .ok_or(IndexError::CorruptIndex)?;
        let canonical = canonical_json(registration).map_err(|_| IndexError::CorruptIndex)?;
        if actual.event_sequence != envelope.sequence()
            || actual.event_id != *envelope.id()
            || validate_registration_row(actual)? != *registration
            || actual.body_hash != ContentHash::sha256(&canonical)
        {
            return Err(IndexError::CorruptIndex);
        }
        self.registration_index += 1;
        Ok(())
    }

    fn finish(self) -> Result<(), IndexError> {
        if self.event_index != self.snapshot.events.len()
            || self.registration_index != self.snapshot.artifact_registrations.len()
            || self.plan_index != self.snapshot.review_plans.len()
            || self.context_index != self.snapshot.context_envelopes.len()
            || self.execution_index != self.snapshot.executions.len()
            || self.claim_index != self.snapshot.claims.len()
        {
            return Err(IndexError::CorruptIndex);
        }
        Ok(())
    }
}

fn event_row_matches(
    validated: &reviewgraphen_core::ValidatedEvent<'_>,
    row: &IndexEvent,
    schema: &str,
) -> Result<bool, IndexError> {
    let envelope = validated.envelope();
    Ok(row.sequence == envelope.sequence()
        && row.event_id == *envelope.id()
        && row.schema == schema
        && row.event_hash == *envelope.event_hash()
        && row.payload_hash == *envelope.payload_hash()
        && row.payload_kind == payload_kind_decoded(validated.payload())?
        && row.actor == envelope.actor()
        && row.logical_time == envelope.logical_time())
}

fn event_compare_tuple_bytes(connection: &Connection) -> Result<u64, IndexError> {
    let dynamic: i64 = connection.query_row(
        "SELECT COALESCE(MAX(length(CAST(event_id AS BLOB))+length(CAST(schema AS BLOB))+length(CAST(event_hash AS BLOB))+length(CAST(payload_hash AS BLOB))+length(CAST(payload_kind AS BLOB))+length(CAST(actor AS BLOB))),0) FROM events",
        [],
        |row| row.get(0),
    )?;
    u64::try_from(dynamic)
        .map_err(|_| IndexError::CorruptIndex)?
        .checked_add(
            u64::try_from(std::mem::size_of::<IndexEvent>())
                .map_err(|_| IndexError::IntegerOutOfRange)?,
        )
        // A plan/context comparison retains one compact SHA-256 body hash;
        // its canonical body writer is core-owned transient state.
        .and_then(|value| value.checked_add(71))
        .ok_or(IndexError::Incomplete {
            limit: u64::MAX,
            observed: u64::MAX,
        })
}

fn admit_query_compare_peak(
    connection: &Connection,
    retained: u64,
    certificate: &super::journal::PrefixCertificate,
    marker: &IndexMarker,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    let result = preflight_query_budget(connection, marker, limits)?;
    let page_count: i64 = connection.pragma_query_value(None, "page_count", |row| row.get(0))?;
    let main = u64::try_from(page_count)
        .map_err(|_| IndexError::CorruptIndex)?
        .checked_mul(PAGE_SIZE)
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?;
    let cache = configured_cache_bytes(
        limits,
        query_cache_reservation_with_journal(limits, retained)?,
    )?;
    let compare = event_compare_tuple_bytes(connection)?;
    let observed = retained
        .checked_add(certificate.line_buffer_capacity)
        .and_then(|value| value.checked_add(certificate.canonical_scratch_capacity))
        .and_then(|value| value.checked_add(main))
        .and_then(|value| value.checked_add(cache))
        .and_then(|value| value.checked_add(result))
        .and_then(|value| value.checked_add(compare))
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?;
    if observed > limits.max_working_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed,
        });
    }
    Ok(())
}

#[cfg(test)]
fn snapshot_from_connection(
    connection: &Connection,
    limits: IndexLimits,
    cache_reservation: u64,
    aggregate: Option<&ReviewAggregate>,
) -> Result<IndexSnapshot, IndexError> {
    snapshot_from_connection_with_journal(
        connection,
        limits,
        cache_reservation,
        0,
        0,
        aggregate.map(D1ValidationSource::Aggregate),
    )
}

#[derive(Clone, Copy)]
enum D1ValidationSource<'a> {
    #[cfg(test)]
    Aggregate(&'a ReviewAggregate),
    /// The row's canonical body is checked here and bound to its exact
    /// journal payload hash by the following lock-held streaming pass.
    StreamingJournal(std::marker::PhantomData<&'a ()>),
}

impl D1ValidationSource<'_> {
    const fn streaming() -> Self {
        Self::StreamingJournal(std::marker::PhantomData)
    }
}

fn preflight_marker_query(connection: &Connection, limits: IndexLimits) -> Result<u64, IndexError> {
    let text: i64 = connection
        .query_row(
            "SELECT COALESCE(length(CAST(projection_contract_version AS BLOB))+length(CAST(event_contract_version AS BLOB))+length(CAST(projection_mode AS BLOB))+length(CAST(run_id AS BLOB))+length(CAST(genesis_hash AS BLOB))+length(CAST(tail_hash AS BLOB)),0) FROM index_meta WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(|_| IndexError::CorruptIndex)?;
    let text = u64::try_from(text).map_err(|_| IndexError::CorruptIndex)?;
    let fixed = u64::try_from(std::mem::size_of::<IndexMarker>())
        .map_err(|_| IndexError::IntegerOutOfRange)?;
    let requested = text.checked_add(fixed).ok_or(IndexError::Incomplete {
        limit: limits.max_query_bytes,
        observed: u64::MAX,
    })?;
    if requested > limits.max_query_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_query_bytes,
            observed: requested,
        });
    }
    Ok(requested)
}

fn index_marker_from_connection(
    connection: &Connection,
    limits: IndexLimits,
) -> Result<IndexMarker, IndexError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 1 || version == 2 {
        return Err(IndexError::RebuildRequired {
            found: u32::try_from(version).map_err(|_| IndexError::CorruptIndex)?,
            required: INDEX_SCHEMA_VERSION,
        });
    }
    if version != i64::from(INDEX_SCHEMA_VERSION) {
        return Err(IndexError::CorruptIndex);
    }
    let count: i64 =
        connection.query_row("SELECT COUNT(*) FROM index_meta", [], |row| row.get(0))?;
    if count != 1 {
        return Err(IndexError::CorruptIndex);
    }
    preflight_marker_query(connection, limits)?;
    let marker = connection.query_row("SELECT index_schema_version,projection_contract_version,event_contract_version,projection_mode,run_id,genesis_hash,confirmed_offset,tail_hash,event_count FROM index_meta WHERE singleton=1", [], |row| {
        Ok(IndexMarker { index_schema_version: u64::try_from(row.get::<_,i64>(0)?).map_err(|_| rusqlite::Error::InvalidQuery)?, sqlite_user_version: u64::try_from(version).map_err(|_| rusqlite::Error::InvalidQuery)?, projection_contract_version: row_text(row, 1)?, event_contract_version: row_text(row, 2)?, projection_mode: row_text(row, 3)?, run_id: StableId::parse(row_text(row, 4)?).map_err(|_| rusqlite::Error::InvalidQuery)?, genesis_hash: ContentHash::parse(row_text(row, 5)?).map_err(|_| rusqlite::Error::InvalidQuery)?, confirmed_offset: u64::try_from(row.get::<_,i64>(6)?).map_err(|_| rusqlite::Error::InvalidQuery)?, tail_hash: ContentHash::parse(row_text(row, 7)?).map_err(|_| rusqlite::Error::InvalidQuery)?, event_count: u64::try_from(row.get::<_,i64>(8)?).map_err(|_| rusqlite::Error::InvalidQuery)? })
    }).map_err(|_| IndexError::CorruptIndex)?;
    if marker.index_schema_version != marker.sqlite_user_version
        || marker.sqlite_user_version != u64::from(INDEX_SCHEMA_VERSION)
        || marker.projection_contract_version != PROJECTION_CONTRACT_VERSION
    {
        return Err(IndexError::CorruptIndex);
    }
    Ok(marker)
}

fn snapshot_from_connection_with_journal(
    connection: &Connection,
    limits: IndexLimits,
    cache_reservation: u64,
    journal_view: u64,
    candidate_read_image: u64,
    validation_source: Option<D1ValidationSource<'_>>,
) -> Result<IndexSnapshot, IndexError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 1 || version == 2 {
        return Err(IndexError::RebuildRequired {
            found: u32::try_from(version).map_err(|_| IndexError::CorruptIndex)?,
            required: INDEX_SCHEMA_VERSION,
        });
    }
    if version != i64::from(INDEX_SCHEMA_VERSION) {
        return Err(IndexError::CorruptIndex);
    }
    let integrity_ok: bool = connection.query_row("PRAGMA integrity_check", [], |row| {
        Ok(row.get_ref(0)?.as_str()? == "ok")
    })?;
    if !integrity_ok {
        return Err(IndexError::CorruptIndex);
    }
    let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
    if foreign_keys.query([])?.next()?.is_some() {
        return Err(IndexError::CorruptIndex);
    }
    let count: i64 =
        connection.query_row("SELECT COUNT(*) FROM index_meta", [], |row| row.get(0))?;
    if count != 1 {
        return Err(IndexError::CorruptIndex);
    }
    preflight_marker_query(connection, limits)?;
    let marker = connection.query_row("SELECT index_schema_version,projection_contract_version,event_contract_version,projection_mode,run_id,genesis_hash,confirmed_offset,tail_hash,event_count FROM index_meta WHERE singleton=1", [], |row| {
        Ok(IndexMarker { index_schema_version: u64::try_from(row.get::<_,i64>(0)?).map_err(|_| rusqlite::Error::InvalidQuery)?, sqlite_user_version: u64::try_from(version).map_err(|_| rusqlite::Error::InvalidQuery)?, projection_contract_version: row_text(row, 1)?, event_contract_version: row_text(row, 2)?, projection_mode: row_text(row, 3)?, run_id: StableId::parse(row_text(row, 4)?).map_err(|_| rusqlite::Error::InvalidQuery)?, genesis_hash: ContentHash::parse(row_text(row, 5)?).map_err(|_| rusqlite::Error::InvalidQuery)?, confirmed_offset: u64::try_from(row.get::<_,i64>(6)?).map_err(|_| rusqlite::Error::InvalidQuery)?, tail_hash: ContentHash::parse(row_text(row, 7)?).map_err(|_| rusqlite::Error::InvalidQuery)?, event_count: u64::try_from(row.get::<_,i64>(8)?).map_err(|_| rusqlite::Error::InvalidQuery)? })
    }).map_err(|_| IndexError::CorruptIndex)?;
    if marker.index_schema_version != marker.sqlite_user_version
        || marker.sqlite_user_version != u64::from(INDEX_SCHEMA_VERSION)
        || marker.projection_contract_version != PROJECTION_CONTRACT_VERSION
    {
        return Err(IndexError::CorruptIndex);
    }
    if marker.projection_mode == "v1_event_metadata_only" {
        for sql in [
            "SELECT COUNT(*) FROM program_objects",
            "SELECT COUNT(*) FROM program_relations",
            "SELECT COUNT(*) FROM universe",
            "SELECT COUNT(*) FROM obligations",
            "SELECT COUNT(*) FROM obligation_lifecycle",
            "SELECT COUNT(*) FROM executions",
            "SELECT COUNT(*) FROM claims",
            "SELECT COUNT(*) FROM artifact_registrations",
            "SELECT COUNT(*) FROM snapshot_source_index",
            "SELECT COUNT(*) FROM context_envelopes",
            "SELECT COUNT(*) FROM review_plans",
            "SELECT COUNT(*) FROM unreconciled_authority_records",
            "SELECT COUNT(*) FROM projected_findings",
        ] {
            let count: i64 = connection.query_row(sql, [], |row| row.get(0))?;
            if count != 0 {
                return Err(IndexError::CorruptIndex);
            }
        }
    } else if marker.projection_mode != "v2_domain" {
        return Err(IndexError::CorruptIndex);
    } else {
        let count: i64 =
            connection.query_row("SELECT COUNT(*) FROM universe", [], |row| row.get(0))?;
        if count != 1 {
            return Err(IndexError::CorruptIndex);
        }
        for sql in [
            "SELECT COUNT(*) FROM program_objects",
            "SELECT COUNT(*) FROM obligations",
        ] {
            let count: i64 = connection.query_row(sql, [], |row| row.get(0))?;
            if count == 0 {
                return Err(IndexError::CorruptIndex);
            }
        }
    }
    // The projection returns owned strings and vectors.  Measure the complete
    // fixed query shape before asking SQLite for a row, so an over-budget image
    // cannot materialize even a partial public snapshot.
    let (query_bytes, table_text_bytes) =
        preflight_query_budget_details(connection, &marker, limits)?;
    let page_count: i64 = connection.pragma_query_value(None, "page_count", |row| row.get(0))?;
    let main_bytes = u64::try_from(page_count)
        .map_err(|_| IndexError::CorruptIndex)?
        .checked_mul(PAGE_SIZE)
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?;
    let cache_bytes = configured_cache_bytes(limits, cache_reservation)?;
    let compare_tuple = event_compare_tuple_bytes(connection)?;
    admit_candidate_reread_peak(
        [
            journal_view,
            candidate_read_image,
            main_bytes,
            cache_bytes,
            query_bytes,
            compare_tuple,
            0,
        ],
        limits.max_working_bytes,
    )?;
    let text_admission = QueryTextAdmission::new(table_text_bytes)?;
    let mut events = Vec::new();
    events
        .try_reserve_exact(
            usize::try_from(marker.event_count).map_err(|_| IndexError::IntegerOutOfRange)?,
        )
        .map_err(|_| IndexError::Incomplete {
            limit: limits.max_query_bytes,
            observed: query_bytes,
        })?;
    let mut statement = connection.prepare("SELECT sequence,event_id,schema,event_hash,payload_hash,payload_kind,actor,logical_time FROM events ORDER BY sequence,event_id")?;
    let rows = statement.query_map([], |row| {
        Ok(IndexEvent {
            sequence: u64::try_from(row.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: StableId::parse(row_text(row, 1)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            schema: row_text(row, 2)?,
            event_hash: ContentHash::parse(row_text(row, 3)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            payload_hash: ContentHash::parse(row_text(row, 4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            payload_kind: row_text(row, 5)?,
            actor: row_text(row, 6)?,
            logical_time: u64::try_from(row.get::<_, i64>(7)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        })
    })?;
    for row in rows {
        let row = row.map_err(|_| IndexError::CorruptIndex)?;
        events.push(row);
    }
    if u64::try_from(events.len()).map_err(|_| IndexError::IntegerOutOfRange)? != marker.event_count
    {
        return Err(IndexError::CorruptIndex);
    }
    if events
        .iter()
        .any(|event| event.schema != marker.event_contract_version)
    {
        return Err(IndexError::CorruptIndex);
    }
    if marker.projection_mode == "v2_domain"
        && events
            .first()
            .is_none_or(|event| event.sequence != 1 || event.payload_kind != "run_genesis_manifest")
    {
        return Err(IndexError::CorruptIndex);
    }
    let mut shadows = reserved_query_vec(
        connection,
        "SELECT COUNT(*) FROM unreconciled_authority_records",
    )?;
    let mut shadow_statement = connection.prepare("SELECT event_sequence,event_id,record_id,kind,body_hash,authority_reconciled FROM unreconciled_authority_records ORDER BY event_sequence,record_id")?;
    let shadow_rows = shadow_statement.query_map([], |row| {
        Ok(IndexShadow {
            event_sequence: u64::try_from(row.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: StableId::parse(row_text(row, 1)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            record_id: StableId::parse(row_text(row, 2)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            kind: row_text(row, 3)?,
            body_hash: ContentHash::parse(row_text(row, 4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            authority_reconciled: row.get::<_, i64>(5)? != 0,
        })
    })?;
    for row in shadow_rows {
        let shadow = row.map_err(|_| IndexError::CorruptIndex)?;
        if shadow.authority_reconciled {
            return Err(IndexError::CorruptIndex);
        }
        shadows.push(shadow);
    }
    let findings = query_findings(connection)?;
    let program_objects = query_program_objects(connection)?;
    let program_relations = query_program_relations(connection)?;
    let universe = query_universe(connection)?;
    let obligations = query_obligations(connection)?;
    let obligation_lifecycle = query_obligation_lifecycle(connection)?;
    let executions = query_executions(connection)?;
    let claims = query_claims(connection)?;
    let artifact_registrations = query_registrations(connection)?;
    let snapshot_sources = query_sources(connection)?;
    let context_envelopes = query_context_envelopes(connection, validation_source)?;
    let review_plans = query_review_plans(connection, validation_source)?;
    validate_d2_projection_closure(
        &events,
        &obligations,
        &artifact_registrations,
        &context_envelopes,
        &review_plans,
        &executions,
        &claims,
        &marker,
    )?;
    text_admission.finish()?;
    let snapshot = IndexSnapshot {
        marker,
        events,
        shadows,
        findings,
        program_objects,
        program_relations,
        universe,
        obligations,
        obligation_lifecycle,
        executions,
        claims,
        artifact_registrations,
        snapshot_sources,
        context_envelopes,
        review_plans,
    };
    if preflight_query_budget(connection, &snapshot.marker, limits)? != query_bytes
        || snapshot_query_budget(&snapshot, limits)? != query_bytes
    {
        return Err(IndexError::CorruptIndex);
    }
    Ok(snapshot)
}

fn charge_texts<'a>(
    used: &mut u64,
    limits: IndexLimits,
    values: impl IntoIterator<Item = &'a str>,
) -> Result<(), IndexError> {
    for value in values {
        charge_bytes(
            used,
            limits,
            u64::try_from(value.len()).map_err(|_| IndexError::IntegerOutOfRange)?,
        )?;
    }
    Ok(())
}

fn charge_rows<T>(used: &mut u64, limits: IndexLimits, rows: &[T]) -> Result<(), IndexError> {
    let bytes = u64::try_from(rows.len())
        .map_err(|_| IndexError::IntegerOutOfRange)?
        .checked_mul(vector_row_cost::<T>())
        .ok_or(IndexError::Incomplete {
            limit: limits.max_query_bytes,
            observed: u64::MAX,
        })?;
    charge_bytes(used, limits, bytes)
}

/// Reconcile every decoded field with the earlier fixed-SQL length/count
/// preflight. This catches type coercion, row drift, or a query implementation
/// that accidentally requests a different owned result shape.
fn snapshot_query_budget(snapshot: &IndexSnapshot, limits: IndexLimits) -> Result<u64, IndexError> {
    let mut used = u64::try_from(std::mem::size_of::<IndexMarker>())
        .map_err(|_| IndexError::IntegerOutOfRange)?;
    charge_texts(
        &mut used,
        limits,
        [
            snapshot.marker.projection_contract_version.as_str(),
            snapshot.marker.event_contract_version.as_str(),
            snapshot.marker.projection_mode.as_str(),
            snapshot.marker.run_id.as_str(),
            snapshot.marker.genesis_hash.as_str(),
            snapshot.marker.tail_hash.as_str(),
        ],
    )?;

    charge_rows(&mut used, limits, &snapshot.events)?;
    for row in &snapshot.events {
        charge_texts(
            &mut used,
            limits,
            [
                row.event_id.as_str(),
                row.schema.as_str(),
                row.event_hash.as_str(),
                row.payload_hash.as_str(),
                row.payload_kind.as_str(),
                row.actor.as_str(),
            ],
        )?;
    }
    charge_rows(&mut used, limits, &snapshot.shadows)?;
    for row in &snapshot.shadows {
        charge_texts(
            &mut used,
            limits,
            [
                row.event_id.as_str(),
                row.record_id.as_str(),
                row.kind.as_str(),
                row.body_hash.as_str(),
            ],
        )?;
    }
    charge_rows(&mut used, limits, &snapshot.findings)?;
    for row in &snapshot.findings {
        charge_texts(
            &mut used,
            limits,
            [
                row.event_id.as_str(),
                row.finding_id.as_str(),
                row.body_hash.as_str(),
                row.projection_status.as_str(),
            ],
        )?;
    }
    charge_rows(&mut used, limits, &snapshot.program_objects)?;
    for row in &snapshot.program_objects {
        charge_texts(
            &mut used,
            limits,
            [
                row.object_id.as_str(),
                row.object_kind.as_str(),
                row.body_hash.as_str(),
            ],
        )?;
    }
    charge_rows(&mut used, limits, &snapshot.program_relations)?;
    for row in &snapshot.program_relations {
        charge_texts(
            &mut used,
            limits,
            [
                row.relation_id.as_str(),
                row.relation_kind.as_str(),
                row.source_id.as_str(),
                row.target_ids_canonical_json.as_str(),
                row.body_hash.as_str(),
            ],
        )?;
    }
    if let Some(row) = &snapshot.universe {
        charge_texts(
            &mut used,
            limits,
            [
                row.universe_id.as_str(),
                row.snapshot_id.as_str(),
                row.profile_id.as_str(),
                row.rule_set_hash.as_str(),
                row.extractor_set_hash.as_str(),
                row.policy_version.as_str(),
                row.rule_pack_version.as_str(),
                row.body_hash.as_str(),
            ],
        )?;
    }
    charge_rows(&mut used, limits, &snapshot.obligations)?;
    for row in &snapshot.obligations {
        charge_texts(
            &mut used,
            limits,
            [
                row.obligation_id.as_str(),
                row.target_kind.as_str(),
                row.target_ids_canonical_json.as_str(),
                row.property_id.as_str(),
                row.lifecycle.as_str(),
                row.body_hash.as_str(),
            ],
        )?;
    }
    charge_rows(&mut used, limits, &snapshot.obligation_lifecycle)?;
    for row in &snapshot.obligation_lifecycle {
        charge_texts(
            &mut used,
            limits,
            [
                row.event_id.as_str(),
                row.obligation_id.as_str(),
                row.next_lifecycle.as_str(),
            ],
        )?;
    }
    charge_rows(&mut used, limits, &snapshot.claims)?;
    for row in &snapshot.claims {
        charge_texts(
            &mut used,
            limits,
            [
                row.event_id.as_str(),
                row.claim_id.as_str(),
                row.execution_id.as_str(),
                row.obligation_ids_canonical_json.as_str(),
                row.property_id.as_str(),
                row.target_refs_canonical_json.as_str(),
                row.polarity.as_str(),
                row.disposition.as_str(),
                row.summary.as_str(),
                row.source_ids_canonical_json.as_str(),
                row.assumptions_canonical_json.as_str(),
                row.requested_evidence_canonical_json.as_str(),
                row.candidate_confidence_canonical_json.as_str(),
                row.author_kind.as_str(),
                row.review_status.as_str(),
                row.identity_body_hash.as_str(),
                row.body_hash.as_str(),
            ],
        )?;
    }
    charge_rows(&mut used, limits, &snapshot.executions)?;
    for row in &snapshot.executions {
        charge_texts(
            &mut used,
            limits,
            [
                row.event_id.as_str(),
                row.execution_id.as_str(),
                row.plan_id.as_str(),
                row.wave_id.as_str(),
                row.snapshot_id.as_str(),
                row.envelope_id.as_str(),
                row.obligation_ids_canonical_json.as_str(),
                row.reviewer_kind.as_str(),
                row.reviewer_id.as_str(),
                row.provider.as_deref().unwrap_or(""),
                row.model.as_deref().unwrap_or(""),
                row.model_revision.as_deref().unwrap_or(""),
                row.system_prompt_version.as_str(),
                row.prompt_template_version.as_str(),
                row.inference_settings_canonical_json.as_str(),
                row.tool_policy_version.as_str(),
                row.tool_calls_canonical_json.as_str(),
                row.raw_registration_id.as_str(),
                row.raw_hash.as_str(),
                row.parsed_claim_ids_canonical_json.as_str(),
                row.outcome_kind.as_str(),
                row.outcome_canonical_json.as_str(),
                row.identity_body_hash.as_str(),
                row.body_hash.as_str(),
            ],
        )?;
    }
    charge_rows(&mut used, limits, &snapshot.artifact_registrations)?;
    for row in &snapshot.artifact_registrations {
        charge_texts(
            &mut used,
            limits,
            [
                row.event_id.as_str(),
                row.registration_id.as_str(),
                row.run_id.as_str(),
                row.cas_hash.as_str(),
                row.media_type.as_str(),
                row.sensitivity.as_str(),
                row.source_kind.as_str(),
                row.source_id.as_str(),
                row.body_hash.as_str(),
            ],
        )?;
    }
    charge_rows(&mut used, limits, &snapshot.snapshot_sources)?;
    for row in &snapshot.snapshot_sources {
        charge_texts(
            &mut used,
            limits,
            [
                row.event_id.as_str(),
                row.snapshot_id.as_str(),
                row.artifact_id.as_str(),
                row.registration_id.as_str(),
                row.path.as_str(),
                row.content_hash.as_str(),
                row.cas_hash.as_str(),
            ],
        )?;
    }
    charge_rows(&mut used, limits, &snapshot.context_envelopes)?;
    for row in &snapshot.context_envelopes {
        charge_texts(
            &mut used,
            limits,
            [
                row.event_id.as_str(),
                row.envelope_id.as_str(),
                row.snapshot_id.as_str(),
                row.context_policy_version.as_str(),
                row.context_policy_hash.as_str(),
                row.candidate_ids_canonical_json.as_str(),
                row.obligation_ids_canonical_json.as_str(),
                row.context_policy_canonical_json.as_str(),
                row.included_sources_canonical_json.as_str(),
                row.excluded_sources_canonical_json.as_str(),
                row.unknowns_canonical_json.as_str(),
                row.assumptions_canonical_json.as_str(),
                row.losses_canonical_json.as_str(),
                row.projection_hash.as_str(),
                row.body_hash.as_str(),
            ],
        )?;
    }
    charge_rows(&mut used, limits, &snapshot.review_plans)?;
    for row in &snapshot.review_plans {
        charge_texts(
            &mut used,
            limits,
            [
                row.event_id.as_str(),
                row.plan_id.as_str(),
                row.universe_id.as_str(),
                row.snapshot_id.as_str(),
                row.planner_input_hash.as_str(),
                row.planner_policy_version.as_str(),
                row.planner_policy_hash.as_str(),
                row.budget_canonical_json.as_str(),
                row.budget_hash.as_str(),
                row.risk_breakdown_canonical_json.as_str(),
                row.waves_canonical_json.as_str(),
                row.deferred_canonical_json.as_str(),
                row.identity_body_hash.as_str(),
                row.body_hash.as_str(),
            ],
        )?;
    }
    Ok(used)
}

fn charge_bytes(used: &mut u64, limits: IndexLimits, value: u64) -> Result<(), IndexError> {
    *used = used.checked_add(value).ok_or(IndexError::Incomplete {
        limit: limits.max_query_bytes,
        observed: u64::MAX,
    })?;
    if *used > limits.max_query_bytes {
        return Err(IndexError::Incomplete {
            limit: limits.max_query_bytes,
            observed: *used,
        });
    }
    Ok(())
}

/// Admit the complete public result shape before any table row is decoded.
/// `text_bytes` is exact SQLite UTF-8 length; `scalar_bytes` is deliberately
/// conservative for fixed-width values and vector bookkeeping.
fn preflight_query_budget(
    connection: &Connection,
    marker: &IndexMarker,
    limits: IndexLimits,
) -> Result<u64, IndexError> {
    Ok(preflight_query_budget_details(connection, marker, limits)?.0)
}

fn preflight_query_budget_details(
    connection: &Connection,
    marker: &IndexMarker,
    limits: IndexLimits,
) -> Result<(u64, u64), IndexError> {
    let mut used = u64::try_from(std::mem::size_of::<IndexMarker>())
        .map_err(|_| IndexError::IntegerOutOfRange)?;
    for value in [
        marker.projection_contract_version.as_str(),
        marker.event_contract_version.as_str(),
        marker.projection_mode.as_str(),
        marker.run_id.as_str(),
        marker.genesis_hash.as_str(),
        marker.tail_hash.as_str(),
    ] {
        charge_bytes(
            &mut used,
            limits,
            u64::try_from(value.len()).map_err(|_| IndexError::IntegerOutOfRange)?,
        )?;
    }
    let tables = [
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(event_id AS BLOB))+length(CAST(schema AS BLOB))+length(CAST(event_hash AS BLOB))+length(CAST(payload_hash AS BLOB))+length(CAST(payload_kind AS BLOB))+length(CAST(actor AS BLOB))),0) FROM events",
            vector_row_cost::<IndexEvent>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(event_id AS BLOB))+length(CAST(record_id AS BLOB))+length(CAST(kind AS BLOB))+length(CAST(body_hash AS BLOB))),0) FROM unreconciled_authority_records",
            vector_row_cost::<IndexShadow>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(event_id AS BLOB))+length(CAST(finding_id AS BLOB))+length(CAST(body_hash AS BLOB))+length(CAST(projection_status AS BLOB))),0) FROM projected_findings",
            vector_row_cost::<IndexFinding>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(object_id AS BLOB))+length(CAST(object_kind AS BLOB))+length(CAST(body_hash AS BLOB))),0) FROM program_objects",
            vector_row_cost::<IndexProgramObject>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(relation_id AS BLOB))+length(CAST(relation_kind AS BLOB))+length(CAST(source_id AS BLOB))+length(CAST(target_ids_canonical_json AS BLOB))+length(CAST(body_hash AS BLOB))),0) FROM program_relations",
            vector_row_cost::<IndexProgramRelation>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(universe_id AS BLOB))+length(CAST(snapshot_id AS BLOB))+length(CAST(profile_id AS BLOB))+length(CAST(rule_set_hash AS BLOB))+length(CAST(extractor_set_hash AS BLOB))+length(CAST(policy_version AS BLOB))+length(CAST(rule_pack_version AS BLOB))+length(CAST(body_hash AS BLOB))),0) FROM universe",
            0,
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(obligation_id AS BLOB))+length(CAST(target_kind AS BLOB))+length(CAST(target_ids_canonical_json AS BLOB))+length(CAST(property_id AS BLOB))+length(CAST(lifecycle AS BLOB))+length(CAST(body_hash AS BLOB))),0) FROM obligations",
            vector_row_cost::<IndexObligation>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(event_id AS BLOB))+length(CAST(obligation_id AS BLOB))+length(CAST(next_lifecycle AS BLOB))),0) FROM obligation_lifecycle",
            vector_row_cost::<IndexObligationLifecycle>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(event_id AS BLOB))+length(CAST(execution_id AS BLOB))+length(CAST(plan_id AS BLOB))+length(CAST(wave_id AS BLOB))+length(CAST(snapshot_id AS BLOB))+length(CAST(envelope_id AS BLOB))+length(CAST(obligation_ids_canonical_json AS BLOB))+length(CAST(reviewer_kind AS BLOB))+length(CAST(reviewer_id AS BLOB))+COALESCE(length(CAST(provider AS BLOB)),0)+COALESCE(length(CAST(model AS BLOB)),0)+COALESCE(length(CAST(model_revision AS BLOB)),0)+length(CAST(system_prompt_version AS BLOB))+length(CAST(prompt_template_version AS BLOB))+length(CAST(inference_settings_canonical_json AS BLOB))+length(CAST(tool_policy_version AS BLOB))+length(CAST(tool_calls_canonical_json AS BLOB))+length(CAST(raw_registration_id AS BLOB))+length(CAST(raw_hash AS BLOB))+length(CAST(parsed_claim_ids_canonical_json AS BLOB))+length(CAST(outcome_kind AS BLOB))+length(CAST(outcome_canonical_json AS BLOB))+length(CAST(identity_body_hash AS BLOB))+length(CAST(body_hash AS BLOB))),0) FROM executions",
            vector_row_cost::<IndexExecution>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(event_id AS BLOB))+length(CAST(claim_id AS BLOB))+length(CAST(execution_id AS BLOB))+length(CAST(obligation_ids_canonical_json AS BLOB))+length(CAST(property_id AS BLOB))+length(CAST(target_refs_canonical_json AS BLOB))+length(CAST(polarity AS BLOB))+length(CAST(disposition AS BLOB))+length(CAST(summary AS BLOB))+length(CAST(source_ids_canonical_json AS BLOB))+length(CAST(assumptions_canonical_json AS BLOB))+length(CAST(requested_evidence_canonical_json AS BLOB))+length(CAST(candidate_confidence_canonical_json AS BLOB))+length(CAST(author_kind AS BLOB))+length(CAST(review_status AS BLOB))+length(CAST(identity_body_hash AS BLOB))+length(CAST(body_hash AS BLOB))),0) FROM claims",
            vector_row_cost::<IndexClaim>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(event_id AS BLOB))+length(CAST(registration_id AS BLOB))+length(CAST(run_id AS BLOB))+length(CAST(cas_hash AS BLOB))+length(CAST(media_type AS BLOB))+length(CAST(sensitivity AS BLOB))+length(CAST(source_kind AS BLOB))+length(CAST(source_id AS BLOB))+length(CAST(body_hash AS BLOB))),0) FROM artifact_registrations",
            vector_row_cost::<IndexArtifactRegistration>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(event_id AS BLOB))+length(CAST(snapshot_id AS BLOB))+length(CAST(artifact_id AS BLOB))+length(CAST(registration_id AS BLOB))+length(CAST(path AS BLOB))+length(CAST(content_hash AS BLOB))+length(CAST(cas_hash AS BLOB))),0) FROM snapshot_source_index",
            vector_row_cost::<IndexSnapshotSource>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(event_id AS BLOB))+length(CAST(envelope_id AS BLOB))+length(CAST(snapshot_id AS BLOB))+length(CAST(context_policy_version AS BLOB))+length(CAST(context_policy_hash AS BLOB))+length(CAST(candidate_ids_canonical_json AS BLOB))+length(CAST(obligation_ids_canonical_json AS BLOB))+length(CAST(context_policy_canonical_json AS BLOB))+length(CAST(included_sources_canonical_json AS BLOB))+length(CAST(excluded_sources_canonical_json AS BLOB))+length(CAST(unknowns_canonical_json AS BLOB))+length(CAST(assumptions_canonical_json AS BLOB))+length(CAST(losses_canonical_json AS BLOB))+length(CAST(projection_hash AS BLOB))+length(CAST(body_hash AS BLOB))),0) FROM context_envelopes",
            vector_row_cost::<IndexContextEnvelope>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(event_id AS BLOB))+length(CAST(plan_id AS BLOB))+length(CAST(universe_id AS BLOB))+length(CAST(snapshot_id AS BLOB))+length(CAST(planner_input_hash AS BLOB))+length(CAST(planner_policy_version AS BLOB))+length(CAST(planner_policy_hash AS BLOB))+length(CAST(budget_canonical_json AS BLOB))+length(CAST(budget_hash AS BLOB))+length(CAST(risk_breakdown_canonical_json AS BLOB))+length(CAST(waves_canonical_json AS BLOB))+length(CAST(deferred_canonical_json AS BLOB))+length(CAST(identity_body_hash AS BLOB))+length(CAST(body_hash AS BLOB))),0) FROM review_plans",
            vector_row_cost::<IndexReviewPlan>(),
        ),
    ];
    let mut table_text_bytes = 0_u64;
    for (sql, scalar_bytes) in tables {
        table_text_bytes = table_text_bytes
            .checked_add(charge_query_measurement(
                connection,
                sql,
                scalar_bytes,
                &mut used,
                limits,
            )?)
            .ok_or(IndexError::Incomplete {
                limit: limits.max_query_bytes,
                observed: u64::MAX,
            })?;
    }
    Ok((used, table_text_bytes))
}

fn vector_row_cost<T>() -> u64 {
    // Public query vectors use `try_reserve_exact` before decoding their first
    // row. The normative metric is the requested Layout byte count, not an
    // allocator's usable-size or growth policy.
    u64::try_from(std::mem::size_of::<T>()).expect("type size fits u64")
}

fn charge_query_measurement(
    connection: &Connection,
    sql: &str,
    scalar_bytes: u64,
    used: &mut u64,
    limits: IndexLimits,
) -> Result<u64, IndexError> {
    let (rows, text_bytes): (i64, i64) = connection
        .query_row(sql, [], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|_| IndexError::CorruptIndex)?;
    let rows = u64::try_from(rows).map_err(|_| IndexError::CorruptIndex)?;
    let text_bytes = u64::try_from(text_bytes).map_err(|_| IndexError::CorruptIndex)?;
    let scalar_bytes = rows
        .checked_mul(scalar_bytes)
        .ok_or(IndexError::Incomplete {
            limit: limits.max_query_bytes,
            observed: u64::MAX,
        })?;
    charge_bytes(used, limits, text_bytes)?;
    charge_bytes(used, limits, scalar_bytes)?;
    Ok(text_bytes)
}

fn parse_id(value: String) -> rusqlite::Result<StableId> {
    StableId::parse(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn parse_hash(value: String) -> rusqlite::Result<ContentHash> {
    ContentHash::parse(value).map_err(|_| rusqlite::Error::InvalidQuery)
}

thread_local! {
    static QUERY_TEXT_REMAINING: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

struct QueryTextAdmission;

impl QueryTextAdmission {
    fn new(bytes: u64) -> Result<Self, IndexError> {
        QUERY_TEXT_REMAINING.with(|remaining| {
            if remaining.replace(Some(bytes)).is_some() {
                Err(IndexError::ProjectionContractViolation)
            } else {
                Ok(Self)
            }
        })
    }

    fn finish(self) -> Result<(), IndexError> {
        let remaining = QUERY_TEXT_REMAINING.with(|value| value.replace(None));
        std::mem::forget(self);
        if remaining == Some(0) {
            Ok(())
        } else {
            Err(IndexError::CorruptIndex)
        }
    }
}

impl Drop for QueryTextAdmission {
    fn drop(&mut self) {
        QUERY_TEXT_REMAINING.with(|remaining| remaining.set(None));
    }
}

fn row_text(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<String> {
    let source = row.get_ref(index)?.as_str()?;
    QUERY_TEXT_REMAINING.with(|remaining| {
        if let Some(available) = remaining.get() {
            let requested =
                u64::try_from(source.len()).map_err(|_| rusqlite::Error::InvalidQuery)?;
            let next = available
                .checked_sub(requested)
                .ok_or(rusqlite::Error::InvalidQuery)?;
            // Consume the fixed-SQL measurement before requesting the owned
            // destination, so drift cannot allocate even one over-budget row.
            remaining.set(Some(next));
        }
        Ok::<_, rusqlite::Error>(())
    })?;
    let mut owned = String::new();
    owned
        .try_reserve_exact(source.len())
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    owned.push_str(source);
    Ok(owned)
}

fn row_optional_text(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<String>> {
    match row.get_ref(index)? {
        rusqlite::types::ValueRef::Null => Ok(None),
        rusqlite::types::ValueRef::Text(_) => row_text(row, index).map(Some),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn reserved_query_vec<T>(connection: &Connection, sql: &'static str) -> Result<Vec<T>, IndexError> {
    let count: i64 = connection
        .query_row(sql, [], |row| row.get(0))
        .map_err(|_| IndexError::CorruptIndex)?;
    let count = usize::try_from(count).map_err(|_| IndexError::CorruptIndex)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(count)
        .map_err(|_| IndexError::Incomplete {
            limit: u64::try_from(count)
                .unwrap_or(u64::MAX)
                .saturating_mul(u64::try_from(std::mem::size_of::<T>()).unwrap_or(u64::MAX)),
            observed: u64::MAX,
        })?;
    Ok(rows)
}
fn query_program_objects(c: &Connection) -> Result<Vec<IndexProgramObject>, IndexError> {
    let mut s = c.prepare(
        "SELECT object_id,object_kind,body_hash FROM program_objects ORDER BY object_id",
    )?;
    let r = s.query_map([], |x| {
        Ok(IndexProgramObject {
            object_id: parse_id(row_text(x, 0)?)?,
            object_kind: row_text(x, 1)?,
            body_hash: parse_hash(row_text(x, 2)?)?,
        })
    })?;
    let mut rows = reserved_query_vec(c, "SELECT COUNT(*) FROM program_objects")?;
    for row in r {
        rows.push(row.map_err(|_| IndexError::CorruptIndex)?);
    }
    Ok(rows)
}
fn query_program_relations(c: &Connection) -> Result<Vec<IndexProgramRelation>, IndexError> {
    let mut s=c.prepare("SELECT relation_id,relation_kind,source_id,target_ids_canonical_json,body_hash FROM program_relations ORDER BY relation_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexProgramRelation {
            relation_id: parse_id(row_text(x, 0)?)?,
            relation_kind: row_text(x, 1)?,
            source_id: parse_id(row_text(x, 2)?)?,
            target_ids_canonical_json: row_text(x, 3)?,
            body_hash: parse_hash(row_text(x, 4)?)?,
        })
    })?;
    let mut rows = reserved_query_vec(c, "SELECT COUNT(*) FROM program_relations")?;
    for row in r {
        rows.push(row.map_err(|_| IndexError::CorruptIndex)?);
    }
    for row in &rows {
        validate_canonical_ids(&row.target_ids_canonical_json)?;
    }
    Ok(rows)
}
fn query_universe(c: &Connection) -> Result<Option<IndexUniverse>, IndexError> {
    let mut s = c.prepare("SELECT universe_id,snapshot_id,profile_id,rule_set_hash,extractor_set_hash,policy_version,rule_pack_version,body_hash FROM universe WHERE singleton=1")?;
    let mut r = s.query([])?;
    match r.next()? {
        None => Ok(None),
        Some(x) => Ok(Some(IndexUniverse {
            universe_id: parse_id(row_text(x, 0)?)?,
            snapshot_id: parse_id(row_text(x, 1)?)?,
            profile_id: row_text(x, 2)?,
            rule_set_hash: parse_hash(row_text(x, 3)?)?,
            extractor_set_hash: parse_hash(row_text(x, 4)?)?,
            policy_version: row_text(x, 5)?,
            rule_pack_version: row_text(x, 6)?,
            body_hash: parse_hash(row_text(x, 7)?)?,
        })),
    }
}
fn query_obligations(c: &Connection) -> Result<Vec<IndexObligation>, IndexError> {
    let mut s=c.prepare("SELECT obligation_id,target_kind,target_ids_canonical_json,property_id,lifecycle,body_hash FROM obligations ORDER BY obligation_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexObligation {
            obligation_id: parse_id(row_text(x, 0)?)?,
            target_kind: row_text(x, 1)?,
            target_ids_canonical_json: row_text(x, 2)?,
            property_id: row_text(x, 3)?,
            lifecycle: row_text(x, 4)?,
            body_hash: parse_hash(row_text(x, 5)?)?,
        })
    })?;
    let mut rows = reserved_query_vec(c, "SELECT COUNT(*) FROM obligations")?;
    for row in r {
        rows.push(row.map_err(|_| IndexError::CorruptIndex)?);
    }
    for row in &rows {
        validate_canonical_ids(&row.target_ids_canonical_json)?;
    }
    Ok(rows)
}
fn query_obligation_lifecycle(c: &Connection) -> Result<Vec<IndexObligationLifecycle>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,obligation_id,next_lifecycle FROM obligation_lifecycle ORDER BY event_sequence,obligation_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexObligationLifecycle {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(row_text(x, 1)?)?,
            obligation_id: parse_id(row_text(x, 2)?)?,
            next_lifecycle: row_text(x, 3)?,
        })
    })?;
    let mut rows = reserved_query_vec(c, "SELECT COUNT(*) FROM obligation_lifecycle")?;
    for row in r {
        rows.push(row.map_err(|_| IndexError::CorruptIndex)?);
    }
    Ok(rows)
}
fn query_executions(c: &Connection) -> Result<Vec<IndexExecution>, IndexError> {
    let mut s = c.prepare("SELECT event_sequence,event_id,execution_id,plan_id,wave_id,snapshot_id,envelope_id,obligation_ids_canonical_json,reviewer_kind,reviewer_id,provider,model,model_revision,system_prompt_version,prompt_template_version,inference_settings_canonical_json,tool_policy_version,tool_calls_canonical_json,attempt,raw_registration_id,raw_hash,parsed_claim_ids_canonical_json,outcome_kind,outcome_canonical_json,identity_body_hash,body_hash FROM executions ORDER BY event_sequence,execution_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexExecution {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(row_text(x, 1)?)?,
            execution_id: parse_id(row_text(x, 2)?)?,
            plan_id: parse_id(row_text(x, 3)?)?,
            wave_id: parse_id(row_text(x, 4)?)?,
            snapshot_id: parse_id(row_text(x, 5)?)?,
            envelope_id: parse_id(row_text(x, 6)?)?,
            obligation_ids_canonical_json: row_text(x, 7)?,
            reviewer_kind: row_text(x, 8)?,
            reviewer_id: row_text(x, 9)?,
            provider: row_optional_text(x, 10)?,
            model: row_optional_text(x, 11)?,
            model_revision: row_optional_text(x, 12)?,
            system_prompt_version: row_text(x, 13)?,
            prompt_template_version: row_text(x, 14)?,
            inference_settings_canonical_json: row_text(x, 15)?,
            tool_policy_version: row_text(x, 16)?,
            tool_calls_canonical_json: row_text(x, 17)?,
            attempt: u32::try_from(x.get::<_, i64>(18)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            raw_registration_id: parse_id(row_text(x, 19)?)?,
            raw_hash: parse_hash(row_text(x, 20)?)?,
            parsed_claim_ids_canonical_json: row_text(x, 21)?,
            outcome_kind: row_text(x, 22)?,
            outcome_canonical_json: row_text(x, 23)?,
            identity_body_hash: parse_hash(row_text(x, 24)?)?,
            body_hash: parse_hash(row_text(x, 25)?)?,
        })
    })?;
    let mut rows = reserved_query_vec(c, "SELECT COUNT(*) FROM executions")?;
    for row in r {
        rows.push(row.map_err(|_| IndexError::CorruptIndex)?);
    }
    Ok(rows)
}

fn query_claims(c: &Connection) -> Result<Vec<IndexClaim>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,claim_id,execution_id,obligation_ids_canonical_json,property_id,target_refs_canonical_json,polarity,disposition,summary,source_ids_canonical_json,assumptions_canonical_json,requested_evidence_canonical_json,candidate_confidence_canonical_json,author_kind,review_status,identity_body_hash,body_hash FROM claims ORDER BY event_sequence,claim_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexClaim {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(row_text(x, 1)?)?,
            claim_id: parse_id(row_text(x, 2)?)?,
            execution_id: parse_id(row_text(x, 3)?)?,
            obligation_ids_canonical_json: row_text(x, 4)?,
            property_id: row_text(x, 5)?,
            target_refs_canonical_json: row_text(x, 6)?,
            polarity: row_text(x, 7)?,
            disposition: row_text(x, 8)?,
            summary: row_text(x, 9)?,
            source_ids_canonical_json: row_text(x, 10)?,
            assumptions_canonical_json: row_text(x, 11)?,
            requested_evidence_canonical_json: row_text(x, 12)?,
            candidate_confidence_canonical_json: row_text(x, 13)?,
            author_kind: row_text(x, 14)?,
            review_status: row_text(x, 15)?,
            identity_body_hash: parse_hash(row_text(x, 16)?)?,
            body_hash: parse_hash(row_text(x, 17)?)?,
        })
    })?;
    let mut rows = reserved_query_vec(c, "SELECT COUNT(*) FROM claims")?;
    for row in r {
        rows.push(row.map_err(|_| IndexError::CorruptIndex)?);
    }
    Ok(rows)
}

fn exact_typed_json<T>(text: &str) -> Result<T, IndexError>
where
    T: DeserializeOwned + Serialize,
{
    let value: T = serde_json::from_str(text).map_err(|_| IndexError::CorruptIndex)?;
    if canonical_json(&value).map_err(|_| IndexError::CorruptIndex)? != text.as_bytes() {
        return Err(IndexError::CorruptIndex);
    }
    Ok(value)
}

fn exact_id_set(text: &str) -> Result<BTreeSet<StableId>, IndexError> {
    let ids: Vec<StableId> = exact_typed_json(text)?;
    let set = ids.iter().cloned().collect::<BTreeSet<_>>();
    if set.len() != ids.len() || canonical_ids(set.iter().cloned())?.as_bytes() != text.as_bytes() {
        return Err(IndexError::CorruptIndex);
    }
    Ok(set)
}

#[derive(Serialize)]
struct ExecutionBody<'a> {
    attempt: u32,
    envelope_id: &'a StableId,
    id: &'a StableId,
    inference_settings: &'a BTreeMap<String, String>,
    model: Option<&'a str>,
    model_revision: Option<&'a str>,
    obligation_ids: &'a BTreeSet<StableId>,
    outcome: &'a reviewgraphen_core::ExecutionOutcome,
    parsed_claim_ids: &'a BTreeSet<StableId>,
    plan_id: &'a StableId,
    prompt_template_version: &'a str,
    provider: Option<&'a str>,
    raw_artifact_hash: &'a ContentHash,
    raw_artifact_registration_id: &'a StableId,
    reviewer_id: &'a str,
    reviewer_kind: &'a str,
    snapshot_id: &'a StableId,
    system_prompt_version: &'a str,
    tool_calls: &'a [()],
    tool_policy_version: &'a str,
    wave_id: &'a StableId,
}

#[derive(Serialize)]
struct ClaimBody<'a> {
    assumptions: &'a BTreeSet<String>,
    author_kind: reviewgraphen_core::ClaimAuthorKind,
    candidate_confidence: Option<f64>,
    disposition: reviewgraphen_core::ClaimDisposition,
    execution_id: &'a StableId,
    id: &'a StableId,
    obligation_ids: &'a BTreeSet<StableId>,
    polarity: reviewgraphen_core::ClaimPolarity,
    property_id: &'a str,
    requested_evidence: &'a BTreeSet<String>,
    review_status: reviewgraphen_core::ReviewStatus,
    source_ids: &'a BTreeSet<StableId>,
    summary: &'a str,
    target_refs: &'a BTreeSet<StableId>,
}

#[derive(Serialize)]
struct RegistrationBody<'a> {
    cas_hash: &'a ContentHash,
    media_type: &'a str,
    registration_id: &'a StableId,
    run_id: &'a StableId,
    sensitivity: reviewgraphen_core::ArtifactSensitivity,
    size: u64,
    source: &'a reviewgraphen_core::ArtifactSource,
}

#[derive(Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WaveWire {
    obligation_ids: Vec<StableId>,
    wave_index: u32,
}

#[derive(Serialize)]
struct WaveIdentity<'a> {
    ids: &'a [StableId],
    plan_id: &'a StableId,
    wave_index: u32,
}

fn wave_id(plan_id: &StableId, wave: &WaveWire) -> Result<StableId, IndexError> {
    let identity = canonical_json(&WaveIdentity {
        ids: &wave.obligation_ids,
        plan_id,
        wave_index: wave.wave_index,
    })
    .map_err(|_| IndexError::CorruptIndex)?;
    StableId::parse(format!("schedule-wave:{}", ContentHash::sha256(&identity)))
        .map_err(|_| IndexError::CorruptIndex)
}

fn parse_closed_enum<T: DeserializeOwned>(value: &str) -> Result<T, IndexError> {
    let json = canonical_json(&value).map_err(|_| IndexError::CorruptIndex)?;
    serde_json::from_slice(&json).map_err(|_| IndexError::CorruptIndex)
}

fn validate_execution_row(
    row: &IndexExecution,
) -> Result<reviewgraphen_core::ExecutionRecord, IndexError> {
    let obligation_ids = exact_id_set(&row.obligation_ids_canonical_json)?;
    let parsed_claim_ids = exact_id_set(&row.parsed_claim_ids_canonical_json)?;
    let inference_settings: BTreeMap<String, String> =
        exact_typed_json(&row.inference_settings_canonical_json)?;
    let tool_calls: Vec<()> = exact_typed_json(&row.tool_calls_canonical_json)?;
    if !tool_calls.is_empty()
        || row.provider.is_some()
        || row.model.is_some()
        || row.model_revision.is_some()
    {
        return Err(IndexError::CorruptIndex);
    }
    let outcome: reviewgraphen_core::ExecutionOutcome =
        exact_typed_json(&row.outcome_canonical_json)?;
    let expected_kind = match outcome {
        reviewgraphen_core::ExecutionOutcome::Structured => "structured",
        reviewgraphen_core::ExecutionOutcome::Abstained { .. } => "abstained",
        reviewgraphen_core::ExecutionOutcome::Malformed { .. } => "malformed",
        reviewgraphen_core::ExecutionOutcome::ProviderFailure { .. } => "provider_failure",
    };
    if row.outcome_kind != expected_kind {
        return Err(IndexError::CorruptIndex);
    }
    let body = canonical_json(&ExecutionBody {
        attempt: row.attempt,
        envelope_id: &row.envelope_id,
        id: &row.execution_id,
        inference_settings: &inference_settings,
        model: row.model.as_deref(),
        model_revision: row.model_revision.as_deref(),
        obligation_ids: &obligation_ids,
        outcome: &outcome,
        parsed_claim_ids: &parsed_claim_ids,
        plan_id: &row.plan_id,
        prompt_template_version: &row.prompt_template_version,
        provider: row.provider.as_deref(),
        raw_artifact_hash: &row.raw_hash,
        raw_artifact_registration_id: &row.raw_registration_id,
        reviewer_id: &row.reviewer_id,
        reviewer_kind: &row.reviewer_kind,
        snapshot_id: &row.snapshot_id,
        system_prompt_version: &row.system_prompt_version,
        tool_calls: &tool_calls,
        tool_policy_version: &row.tool_policy_version,
        wave_id: &row.wave_id,
    })
    .map_err(|_| IndexError::CorruptIndex)?;
    let execution: reviewgraphen_core::ExecutionRecord =
        serde_json::from_slice(&body).map_err(|_| IndexError::CorruptIndex)?;
    if execution
        .canonical_bytes()
        .map_err(|_| IndexError::CorruptIndex)?
        != body
        || execution
            .identity_body_hash()
            .map_err(|_| IndexError::CorruptIndex)?
            != row.identity_body_hash
        || execution
            .body_hash()
            .map_err(|_| IndexError::CorruptIndex)?
            != row.body_hash
    {
        return Err(IndexError::CorruptIndex);
    }
    Ok(execution)
}

fn validate_claim_row(
    row: &IndexClaim,
) -> Result<reviewgraphen_core::ExecutionClaimV2, IndexError> {
    let obligation_ids = exact_id_set(&row.obligation_ids_canonical_json)?;
    let target_refs = exact_id_set(&row.target_refs_canonical_json)?;
    let source_ids = exact_id_set(&row.source_ids_canonical_json)?;
    let assumptions: BTreeSet<String> = exact_typed_json(&row.assumptions_canonical_json)?;
    let requested_evidence: BTreeSet<String> =
        exact_typed_json(&row.requested_evidence_canonical_json)?;
    let confidence: Option<f64> = exact_typed_json(&row.candidate_confidence_canonical_json)?;
    let polarity = parse_closed_enum(&row.polarity)?;
    let disposition = parse_closed_enum(&row.disposition)?;
    let author_kind = parse_closed_enum(&row.author_kind)?;
    let review_status = parse_closed_enum(&row.review_status)?;
    let body = canonical_json(&ClaimBody {
        assumptions: &assumptions,
        author_kind,
        candidate_confidence: confidence,
        disposition,
        execution_id: &row.execution_id,
        id: &row.claim_id,
        obligation_ids: &obligation_ids,
        polarity,
        property_id: &row.property_id,
        requested_evidence: &requested_evidence,
        review_status,
        source_ids: &source_ids,
        summary: &row.summary,
        target_refs: &target_refs,
    })
    .map_err(|_| IndexError::CorruptIndex)?;
    let claim: reviewgraphen_core::ExecutionClaimV2 =
        serde_json::from_slice(&body).map_err(|_| IndexError::CorruptIndex)?;
    if claim
        .canonical_bytes()
        .map_err(|_| IndexError::CorruptIndex)?
        != body
        || claim
            .identity_body_hash()
            .map_err(|_| IndexError::CorruptIndex)?
            != row.identity_body_hash
        || claim.body_hash().map_err(|_| IndexError::CorruptIndex)? != row.body_hash
    {
        return Err(IndexError::CorruptIndex);
    }
    Ok(claim)
}

fn validate_registration_row(
    row: &IndexArtifactRegistration,
) -> Result<reviewgraphen_core::ArtifactRegistered, IndexError> {
    let source: reviewgraphen_core::ArtifactSource = exact_typed_json(&row.source_id)?;
    let sensitivity = parse_closed_enum(&row.sensitivity)?;
    if serialized_enum(&sensitivity)? != row.sensitivity
        || artifact_source_kind(&source)? != row.source_kind
    {
        return Err(IndexError::CorruptIndex);
    }
    let body = canonical_json(&RegistrationBody {
        cas_hash: &row.cas_hash,
        media_type: &row.media_type,
        registration_id: &row.registration_id,
        run_id: &row.run_id,
        sensitivity,
        size: row.size,
        source: &source,
    })
    .map_err(|_| IndexError::CorruptIndex)?;
    // Reconstruct through the public constructor, not serde.  This binds the
    // derived registration ID to every source/media/hash/sensitivity input.
    let registration = reviewgraphen_core::ArtifactRegistered::new(
        row.run_id.clone(),
        row.registration_id.clone(),
        row.cas_hash.clone(),
        row.media_type.clone(),
        row.size,
        sensitivity,
        source,
    )
    .map_err(|_| IndexError::CorruptIndex)?;
    let canonical = canonical_json(&registration).map_err(|_| IndexError::CorruptIndex)?;
    if canonical != body || ContentHash::sha256(&body) != row.body_hash {
        return Err(IndexError::CorruptIndex);
    }
    Ok(registration)
}

#[allow(clippy::too_many_arguments)]
fn validate_d2_projection_closure(
    events: &[IndexEvent],
    obligations: &[IndexObligation],
    registrations: &[IndexArtifactRegistration],
    envelopes: &[IndexContextEnvelope],
    plans: &[IndexReviewPlan],
    executions: &[IndexExecution],
    claims: &[IndexClaim],
    marker: &IndexMarker,
) -> Result<(), IndexError> {
    let event_by_sequence = events
        .iter()
        .map(|event| (event.sequence, event))
        .collect::<BTreeMap<_, _>>();
    let obligation_by_id = obligations
        .iter()
        .map(|obligation| (&obligation.obligation_id, obligation))
        .collect::<BTreeMap<_, _>>();
    let mut registration_by_id = BTreeMap::new();
    for registration in registrations {
        let typed = validate_registration_row(registration)?;
        if registration_by_id
            .insert(registration.registration_id.clone(), typed)
            .is_some()
        {
            return Err(IndexError::CorruptIndex);
        }
    }
    let envelope_by_id = envelopes
        .iter()
        .map(|envelope| (&envelope.envelope_id, envelope))
        .collect::<BTreeMap<_, _>>();
    let plan_by_id = plans
        .iter()
        .map(|plan| (&plan.plan_id, plan))
        .collect::<BTreeMap<_, _>>();
    let mut execution_by_id = BTreeMap::new();
    let mut execution_events = BTreeSet::new();
    let mut typed_executions = BTreeMap::new();
    for row in executions {
        let event = event_by_sequence
            .get(&row.event_sequence)
            .ok_or(IndexError::CorruptIndex)?;
        if event.event_id != row.event_id
            || event.payload_kind != "review_execution_recorded"
            || !execution_events.insert((row.event_sequence, row.event_id.clone()))
            || execution_by_id.insert(&row.execution_id, row).is_some()
        {
            return Err(IndexError::CorruptIndex);
        }
        let execution = validate_execution_row(row)?;
        let plan = plan_by_id
            .get(execution.plan_id())
            .ok_or(IndexError::CorruptIndex)?;
        let envelope = envelope_by_id
            .get(execution.envelope_id())
            .ok_or(IndexError::CorruptIndex)?;
        let envelope_ids = exact_id_set(&envelope.obligation_ids_canonical_json)?;
        let waves: Vec<WaveWire> = exact_typed_json(&plan.waves_canonical_json)?;
        let wave = waves
            .iter()
            .find(|wave| {
                wave_id(execution.plan_id(), wave)
                    .as_ref()
                    .is_ok_and(|id| id == execution.wave_id())
            })
            .ok_or(IndexError::CorruptIndex)?;
        let wave_ids = wave.obligation_ids.iter().cloned().collect::<BTreeSet<_>>();
        if plan.snapshot_id != *execution.snapshot_id()
            || envelope.snapshot_id != *execution.snapshot_id()
            || envelope_ids != *execution.obligation_ids()
            || !execution.obligation_ids().is_subset(&wave_ids)
        {
            return Err(IndexError::CorruptIndex);
        }
        let registration = registration_by_id
            .get(execution.raw_artifact_registration_id())
            .ok_or(IndexError::CorruptIndex)?;
        if registration.cas_hash() != execution.raw_artifact_hash()
            || registration.sensitivity() != reviewgraphen_core::ArtifactSensitivity::Sensitive
            || !matches!(registration.source(), reviewgraphen_core::ArtifactSource::ReviewerExecution { run_id, execution_id, reviewer_id } if run_id == &marker.run_id && execution_id == execution.id() && reviewer_id == execution.reviewer_id())
        {
            return Err(IndexError::CorruptIndex);
        }
        typed_executions.insert(row.execution_id.clone(), execution);
    }
    if execution_events.len() != executions.len()
        || events
            .iter()
            .filter(|event| event.payload_kind == "review_execution_recorded")
            .count()
            != executions.len()
    {
        return Err(IndexError::CorruptIndex);
    }
    let mut claims_by_execution = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    for row in claims {
        let event = event_by_sequence
            .get(&row.event_sequence)
            .ok_or(IndexError::CorruptIndex)?;
        let execution = typed_executions
            .get(&row.execution_id)
            .ok_or(IndexError::CorruptIndex)?;
        if event.event_id != row.event_id
            || row.event_sequence
                != executions
                    .iter()
                    .find(|candidate| candidate.execution_id == row.execution_id)
                    .ok_or(IndexError::CorruptIndex)?
                    .event_sequence
            || row.event_id
                != executions
                    .iter()
                    .find(|candidate| candidate.execution_id == row.execution_id)
                    .ok_or(IndexError::CorruptIndex)?
                    .event_id
        {
            return Err(IndexError::CorruptIndex);
        }
        let claim = validate_claim_row(row)?;
        let obligation_id = execution
            .obligation_ids()
            .iter()
            .next()
            .ok_or(IndexError::CorruptIndex)?;
        let obligation = obligation_by_id
            .get(obligation_id)
            .ok_or(IndexError::CorruptIndex)?;
        let target_ids = exact_id_set(&obligation.target_ids_canonical_json)?;
        let envelope = envelope_by_id
            .get(execution.envelope_id())
            .ok_or(IndexError::CorruptIndex)?;
        let sources: Vec<reviewgraphen_core::SourceArtifactRef> =
            exact_typed_json(&envelope.included_sources_canonical_json)?;
        let source_ids = sources
            .iter()
            .map(|source| source.artifact_id().clone())
            .collect::<BTreeSet<_>>();
        if claim.execution_id() != execution.id()
            || claim.obligation_ids() != execution.obligation_ids()
            || claim.property_id() != obligation.property_id
            || !claim.target_refs().is_subset(&target_ids)
            || !claim.source_ids().is_subset(&source_ids)
            || !claims_by_execution
                .entry(row.execution_id.clone())
                .or_default()
                .insert(row.claim_id.clone())
        {
            return Err(IndexError::CorruptIndex);
        }
    }
    for (id, execution) in typed_executions {
        let actual = claims_by_execution.remove(&id).unwrap_or_default();
        if actual != *execution.parsed_claim_ids()
            || (execution.outcome().is_structured() && actual.is_empty())
            || (!execution.outcome().is_structured() && !actual.is_empty())
        {
            return Err(IndexError::CorruptIndex);
        }
    }
    Ok(())
}
fn query_registrations(c: &Connection) -> Result<Vec<IndexArtifactRegistration>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,registration_id,run_id,cas_hash,media_type,size,sensitivity,source_kind,source_id,body_hash FROM artifact_registrations ORDER BY event_sequence,registration_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexArtifactRegistration {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(row_text(x, 1)?)?,
            registration_id: parse_id(row_text(x, 2)?)?,
            run_id: parse_id(row_text(x, 3)?)?,
            cas_hash: parse_hash(row_text(x, 4)?)?,
            media_type: row_text(x, 5)?,
            size: u64::try_from(x.get::<_, i64>(6)?).map_err(|_| rusqlite::Error::InvalidQuery)?,
            sensitivity: row_text(x, 7)?,
            source_kind: row_text(x, 8)?,
            source_id: row_text(x, 9)?,
            body_hash: parse_hash(row_text(x, 10)?)?,
        })
    })?;
    let mut rows = reserved_query_vec(c, "SELECT COUNT(*) FROM artifact_registrations")?;
    for row in r {
        rows.push(row.map_err(|_| IndexError::CorruptIndex)?);
    }
    Ok(rows)
}
fn query_sources(c: &Connection) -> Result<Vec<IndexSnapshotSource>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,snapshot_id,artifact_id,registration_id,path,content_hash,cas_hash,line_count FROM snapshot_source_index ORDER BY snapshot_id,path,artifact_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexSnapshotSource {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(row_text(x, 1)?)?,
            snapshot_id: parse_id(row_text(x, 2)?)?,
            artifact_id: parse_id(row_text(x, 3)?)?,
            registration_id: parse_id(row_text(x, 4)?)?,
            path: row_text(x, 5)?,
            content_hash: parse_hash(row_text(x, 6)?)?,
            cas_hash: parse_hash(row_text(x, 7)?)?,
            line_count: u64::try_from(x.get::<_, i64>(8)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        })
    })?;
    let mut rows = reserved_query_vec(c, "SELECT COUNT(*) FROM snapshot_source_index")?;
    for row in r {
        rows.push(row.map_err(|_| IndexError::CorruptIndex)?);
    }
    Ok(rows)
}
fn query_findings(c: &Connection) -> Result<Vec<IndexFinding>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,finding_id,body_hash,projection_status FROM projected_findings ORDER BY event_sequence,finding_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexFinding {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(row_text(x, 1)?)?,
            finding_id: parse_id(row_text(x, 2)?)?,
            body_hash: parse_hash(row_text(x, 3)?)?,
            projection_status: row_text(x, 4)?,
        })
    })?;
    let mut rows = reserved_query_vec(c, "SELECT COUNT(*) FROM projected_findings")?;
    for row in r {
        rows.push(row.map_err(|_| IndexError::CorruptIndex)?);
    }
    Ok(rows)
}

fn exact_json_value(text: &str) -> Result<serde_json::Value, IndexError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| IndexError::CorruptIndex)?;
    let canonical = canonical_json(&value).map_err(|_| IndexError::CorruptIndex)?;
    if canonical != text.as_bytes() {
        return Err(IndexError::CorruptIndex);
    }
    Ok(value)
}

fn exact_id_array(text: &str) -> Result<Vec<StableId>, IndexError> {
    validate_canonical_ids(text)?;
    serde_json::from_str::<Vec<String>>(text)
        .map_err(|_| IndexError::CorruptIndex)?
        .into_iter()
        .map(|value| StableId::parse(value).map_err(|_| IndexError::CorruptIndex))
        .collect()
}

fn object_with_fields(
    entries: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
) -> serde_json::Value {
    serde_json::Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

fn exact_component_object(
    text: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, IndexError> {
    exact_json_value(text)?
        .as_object()
        .cloned()
        .ok_or(IndexError::CorruptIndex)
}

fn validate_review_plan_row(
    row: &IndexReviewPlan,
    validation_source: D1ValidationSource<'_>,
) -> Result<(), IndexError> {
    if row.planner_policy_version != PlannerPolicyV1::VERSION
        || row.planner_policy_hash
            != PlannerPolicyV1::baseline()
                .hash()
                .map_err(|_| IndexError::CorruptIndex)?
        || row.budget_canonical_json.len() > 50
        || ContentHash::sha256(row.budget_canonical_json.as_bytes()) != row.budget_hash
    {
        return Err(IndexError::CorruptIndex);
    }
    let budget = exact_component_object(&row.budget_canonical_json)?;
    let risks = exact_json_value(&row.risk_breakdown_canonical_json)?;
    let waves = exact_json_value(&row.waves_canonical_json)?;
    let deferred = exact_json_value(&row.deferred_canonical_json)?;

    let full = object_with_fields([
        ("budget", serde_json::Value::Object(budget.clone())),
        ("deferred", deferred.clone()),
        ("id", serde_json::Value::String(row.plan_id.to_string())),
        (
            "planner_input_hash",
            serde_json::Value::String(row.planner_input_hash.to_string()),
        ),
        (
            "planner_policy_hash",
            serde_json::Value::String(row.planner_policy_hash.to_string()),
        ),
        (
            "planner_policy_version",
            serde_json::Value::String(row.planner_policy_version.clone()),
        ),
        ("risk_breakdown", risks),
        (
            "snapshot_id",
            serde_json::Value::String(row.snapshot_id.to_string()),
        ),
        (
            "universe_id",
            serde_json::Value::String(row.universe_id.to_string()),
        ),
        ("waves", waves.clone()),
    ]);
    let full_bytes = canonical_json(&full).map_err(|_| IndexError::CorruptIndex)?;
    if ContentHash::sha256(&full_bytes) != row.body_hash {
        return Err(IndexError::CorruptIndex);
    }
    let deferred_ids = deferred
        .as_array()
        .ok_or(IndexError::CorruptIndex)?
        .iter()
        .map(|value| value.get("id").cloned().ok_or(IndexError::CorruptIndex))
        .collect::<Result<Vec<_>, _>>()?;
    let identity = object_with_fields([
        ("budget", serde_json::Value::Object(budget)),
        ("deferred_ids", serde_json::Value::Array(deferred_ids)),
        (
            "planner_input_hash",
            serde_json::Value::String(row.planner_input_hash.to_string()),
        ),
        (
            "planner_policy_hash",
            serde_json::Value::String(row.planner_policy_hash.to_string()),
        ),
        (
            "planner_policy_version",
            serde_json::Value::String(row.planner_policy_version.clone()),
        ),
        (
            "snapshot_id",
            serde_json::Value::String(row.snapshot_id.to_string()),
        ),
        (
            "universe_id",
            serde_json::Value::String(row.universe_id.to_string()),
        ),
        ("waves", waves),
    ]);
    let identity_hash =
        ContentHash::sha256(&canonical_json(&identity).map_err(|_| IndexError::CorruptIndex)?);
    if identity_hash != row.identity_body_hash
        || StableId::parse(format!("plan:{identity_hash}")).map_err(|_| IndexError::CorruptIndex)?
            != row.plan_id
    {
        return Err(IndexError::CorruptIndex);
    }
    match validation_source {
        #[cfg(test)]
        D1ValidationSource::Aggregate(aggregate) => {
            let decoded = ReviewPlan::from_canonical_bytes(&full_bytes, aggregate)
                .map_err(|_| IndexError::CorruptIndex)?;
            if decoded
                .canonical_bytes()
                .map_err(|_| IndexError::CorruptIndex)?
                != full_bytes
                || decoded.id() != &row.plan_id
                || decoded.universe_id() != &row.universe_id
                || decoded.snapshot_id() != &row.snapshot_id
                || decoded.planner_input_hash() != &row.planner_input_hash
                || decoded.planner_policy_hash() != &row.planner_policy_hash
                || decoded
                    .identity_body_hash()
                    .map_err(|_| IndexError::CorruptIndex)?
                    != row.identity_body_hash
            {
                return Err(IndexError::CorruptIndex);
            }
        }
        D1ValidationSource::StreamingJournal(_) => {}
    }
    Ok(())
}

fn validate_context_row(
    c: &Connection,
    row: &IndexContextEnvelope,
    validation_source: D1ValidationSource<'_>,
) -> Result<(), IndexError> {
    if row.context_policy_version != ContextPolicyV1::VERSION
        || row.context_policy_canonical_json.as_bytes()
            != ContextPolicyV1::baseline()
                .canonical_bytes()
                .map_err(|_| IndexError::CorruptIndex)?
        || ContentHash::sha256(row.context_policy_canonical_json.as_bytes())
            != row.context_policy_hash
        || row.envelope_id
            != StableId::parse(format!("context-envelope:{}", row.projection_hash))
                .map_err(|_| IndexError::CorruptIndex)?
    {
        return Err(IndexError::CorruptIndex);
    }
    exact_json_value(&row.candidate_ids_canonical_json)?;
    exact_json_value(&row.obligation_ids_canonical_json)?;
    let included = exact_json_value(&row.included_sources_canonical_json)?;
    let excluded = exact_json_value(&row.excluded_sources_canonical_json)?;
    exact_json_value(&row.unknowns_canonical_json)?;
    exact_json_value(&row.assumptions_canonical_json)?;
    let losses = exact_json_value(&row.losses_canonical_json)?;
    let candidate_ids = exact_id_array(&row.candidate_ids_canonical_json)?;
    let obligation_ids = exact_id_array(&row.obligation_ids_canonical_json)?;
    let included_typed: Vec<reviewgraphen_core::SourceArtifactRef> =
        serde_json::from_str(&row.included_sources_canonical_json)
            .map_err(|_| IndexError::CorruptIndex)?;
    let excluded_typed: Vec<reviewgraphen_core::ExcludedSourceRef> =
        serde_json::from_str(&row.excluded_sources_canonical_json)
            .map_err(|_| IndexError::CorruptIndex)?;
    let unknowns_typed: Vec<reviewgraphen_core::EnvelopeUnknown> =
        serde_json::from_str(&row.unknowns_canonical_json).map_err(|_| IndexError::CorruptIndex)?;
    let losses_typed: Vec<reviewgraphen_core::EnvelopeLoss> =
        serde_json::from_str(&row.losses_canonical_json).map_err(|_| IndexError::CorruptIndex)?;
    let assumptions_typed: Vec<String> = serde_json::from_str(&row.assumptions_canonical_json)
        .map_err(|_| IndexError::CorruptIndex)?;
    if canonical_json(&included_typed).map_err(|_| IndexError::CorruptIndex)?
        != row.included_sources_canonical_json.as_bytes()
        || canonical_json(&excluded_typed).map_err(|_| IndexError::CorruptIndex)?
            != row.excluded_sources_canonical_json.as_bytes()
        || canonical_json(&unknowns_typed).map_err(|_| IndexError::CorruptIndex)?
            != row.unknowns_canonical_json.as_bytes()
        || canonical_json(&losses_typed).map_err(|_| IndexError::CorruptIndex)?
            != row.losses_canonical_json.as_bytes()
        || canonical_json(&assumptions_typed).map_err(|_| IndexError::CorruptIndex)?
            != row.assumptions_canonical_json.as_bytes()
    {
        return Err(IndexError::CorruptIndex);
    }
    if obligation_ids.len() != 1
        || !assumptions_typed.is_empty()
        || candidate_ids.iter().any(|id| id.kind() != "file")
        || obligation_ids.iter().any(|id| id.kind() != "obligation")
        || included_typed
            .windows(2)
            .any(|pair| pair[0].artifact_id() >= pair[1].artifact_id())
        || excluded_typed
            .windows(2)
            .any(|pair| pair[0].artifact_id() >= pair[1].artifact_id())
        || unknowns_typed.len() > ContextPolicyV1::baseline().max_unknowns()
        || losses_typed.len() > ContextPolicyV1::baseline().max_losses()
        || included_typed.iter().any(|source| {
            source.registration_id().kind() != "registration"
                || source.artifact_id().kind() != "file"
                || !source.content_hash().to_string().starts_with("sha256:")
                || !source.cas_hash().to_string().starts_with("sha256:")
                || !source.excerpt_hash().to_string().starts_with("sha256:")
                || source.excerpt_byte_length()
                    > u64::try_from(ContextPolicyV1::baseline().max_excerpt_bytes())
                        .unwrap_or(u64::MAX)
        })
        || unknowns_typed.iter().any(|unknown| {
            !matches!(
                unknown.description(),
                "context_unknown:unresolved_seed_reference"
                    | "context_unknown:unresolved_relation_endpoint"
                    | "context_unknown:unresolved_review_context_member"
                    | "context_unknown:unresolved_invariant_scope"
            )
        })
    {
        return Err(IndexError::CorruptIndex);
    }
    validate_context_groups(
        c,
        &obligation_ids[0],
        &candidate_ids,
        &included,
        &excluded,
        &losses,
    )?;
    let normalized = included
        .as_array()
        .ok_or(IndexError::CorruptIndex)?
        .iter()
        .map(|source| {
            source
                .get("artifact_id")
                .and_then(serde_json::Value::as_str)
                .map(|value| serde_json::Value::String(value.to_owned()))
                .ok_or(IndexError::CorruptIndex)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let normalized = String::from_utf8(
        canonical_json(&serde_json::Value::Array(normalized))
            .map_err(|_| IndexError::CorruptIndex)?,
    )
    .map_err(|_| IndexError::CorruptIndex)?;
    let full = context_full_body_bytes(row, &normalized)?;
    if ContentHash::sha256(&full) != row.body_hash {
        return Err(IndexError::CorruptIndex);
    }
    match validation_source {
        #[cfg(test)]
        D1ValidationSource::Aggregate(aggregate) => {
            let decoded = ReviewContextEnvelope::from_canonical_bytes(&full, aggregate)
                .map_err(|_| IndexError::CorruptIndex)?;
            if decoded
                .canonical_bytes()
                .map_err(|_| IndexError::CorruptIndex)?
                != full
                || decoded.id() != &row.envelope_id
                || decoded.snapshot_id() != &row.snapshot_id
                || decoded.projection_policy_version() != row.context_policy_version
                || decoded.context_policy_hash() != &row.context_policy_hash
                || decoded.projection_hash() != &row.projection_hash
            {
                return Err(IndexError::CorruptIndex);
            }
        }
        D1ValidationSource::StreamingJournal(_) => {}
    }
    let identity = context_identity_body_bytes(row)?;
    if ContentHash::sha256(&identity) != row.projection_hash {
        return Err(IndexError::CorruptIndex);
    }
    Ok(())
}

fn json_string(value: &str) -> Result<String, IndexError> {
    String::from_utf8(canonical_json(&value).map_err(|_| IndexError::CorruptIndex)?)
        .map_err(|_| IndexError::CorruptIndex)
}

fn context_full_body_bytes(
    row: &IndexContextEnvelope,
    normalized_included_ids: &str,
) -> Result<Vec<u8>, IndexError> {
    let text = format!(
        "{{\"assumptions\":{},\"candidate_source_ids\":{},\"context_policy\":{},\"context_policy_hash\":{},\"excluded_sources\":{},\"id\":{},\"included_sources\":{},\"losses\":{},\"normalized_included_source_ids\":{},\"obligation_ids\":{},\"projection_hash\":{},\"projection_policy_version\":{},\"snapshot_id\":{},\"unknowns\":{}}}",
        row.assumptions_canonical_json,
        row.candidate_ids_canonical_json,
        row.context_policy_canonical_json,
        json_string(&row.context_policy_hash.to_string())?,
        row.excluded_sources_canonical_json,
        json_string(&row.envelope_id.to_string())?,
        row.included_sources_canonical_json,
        row.losses_canonical_json,
        normalized_included_ids,
        row.obligation_ids_canonical_json,
        json_string(&row.projection_hash.to_string())?,
        json_string(&row.context_policy_version)?,
        json_string(&row.snapshot_id.to_string())?,
        row.unknowns_canonical_json,
    );
    Ok(text.into_bytes())
}

fn context_identity_body_bytes(row: &IndexContextEnvelope) -> Result<Vec<u8>, IndexError> {
    let text = format!(
        "{{\"assumptions\":{},\"candidate_source_ids\":{},\"context_policy\":{},\"context_policy_hash\":{},\"excluded_sources\":{},\"included_sources\":{},\"losses\":{},\"obligation_ids\":{},\"snapshot_id\":{},\"unknowns\":{}}}",
        row.assumptions_canonical_json,
        row.candidate_ids_canonical_json,
        row.context_policy_canonical_json,
        json_string(&row.context_policy_hash.to_string())?,
        row.excluded_sources_canonical_json,
        row.included_sources_canonical_json,
        row.losses_canonical_json,
        row.obligation_ids_canonical_json,
        json_string(&row.snapshot_id.to_string())?,
        row.unknowns_canonical_json,
    );
    Ok(text.into_bytes())
}

fn validate_context_groups(
    c: &Connection,
    obligation_id: &StableId,
    candidate_ids: &[StableId],
    included: &serde_json::Value,
    excluded: &serde_json::Value,
    losses: &serde_json::Value,
) -> Result<(), IndexError> {
    let property: String = c
        .query_row(
            "SELECT property_id FROM obligations WHERE obligation_id=?1",
            [obligation_id.to_string()],
            |row| row.get(0),
        )
        .map_err(|_| IndexError::CorruptIndex)?;
    let included_ids = source_artifact_ids(included)?;
    let excluded_ids = source_artifact_ids(excluded)?;
    let union = included_ids
        .union(&excluded_ids)
        .cloned()
        .collect::<BTreeSet<_>>();
    if !included_ids.is_disjoint(&excluded_ids)
        || union != candidate_ids.iter().cloned().collect::<BTreeSet<_>>()
    {
        return Err(IndexError::CorruptIndex);
    }
    for loss in losses.as_array().ok_or(IndexError::CorruptIndex)? {
        let object = loss.as_object().ok_or(IndexError::CorruptIndex)?;
        if object.len() != 4
            || object.get("severity").and_then(serde_json::Value::as_str) != Some("low")
        {
            return Err(IndexError::CorruptIndex);
        }
        let properties = object
            .get("affected_properties")
            .and_then(serde_json::Value::as_array)
            .ok_or(IndexError::CorruptIndex)?;
        if properties.len() != 1 || properties[0].as_str() != Some(&property) {
            return Err(IndexError::CorruptIndex);
        }
        let sources = object
            .get("source_ids")
            .and_then(serde_json::Value::as_array)
            .ok_or(IndexError::CorruptIndex)?;
        if sources.is_empty() {
            return Err(IndexError::CorruptIndex);
        }
        let mut prior: Option<StableId> = None;
        for source in sources {
            let source = StableId::parse(source.as_str().ok_or(IndexError::CorruptIndex)?)
                .map_err(|_| IndexError::CorruptIndex)?;
            if prior.as_ref().is_some_and(|value| value >= &source) {
                return Err(IndexError::CorruptIndex);
            }
            prior = Some(source);
        }
    }
    Ok(())
}

fn source_artifact_ids(value: &serde_json::Value) -> Result<BTreeSet<StableId>, IndexError> {
    let mut ids = BTreeSet::new();
    for source in value.as_array().ok_or(IndexError::CorruptIndex)? {
        let id = StableId::parse(
            source
                .get("artifact_id")
                .and_then(serde_json::Value::as_str)
                .ok_or(IndexError::CorruptIndex)?,
        )
        .map_err(|_| IndexError::CorruptIndex)?;
        if !ids.insert(id) {
            return Err(IndexError::CorruptIndex);
        }
    }
    Ok(ids)
}
fn query_context_envelopes(
    c: &Connection,
    validation_source: Option<D1ValidationSource<'_>>,
) -> Result<Vec<IndexContextEnvelope>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,envelope_id,snapshot_id,context_policy_version,context_policy_hash,candidate_ids_canonical_json,obligation_ids_canonical_json,context_policy_canonical_json,included_sources_canonical_json,excluded_sources_canonical_json,unknowns_canonical_json,assumptions_canonical_json,losses_canonical_json,projection_hash,body_hash FROM context_envelopes ORDER BY event_sequence,envelope_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexContextEnvelope {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(row_text(x, 1)?)?,
            envelope_id: parse_id(row_text(x, 2)?)?,
            snapshot_id: parse_id(row_text(x, 3)?)?,
            context_policy_version: row_text(x, 4)?,
            context_policy_hash: parse_hash(row_text(x, 5)?)?,
            candidate_ids_canonical_json: row_text(x, 6)?,
            obligation_ids_canonical_json: row_text(x, 7)?,
            context_policy_canonical_json: row_text(x, 8)?,
            included_sources_canonical_json: row_text(x, 9)?,
            excluded_sources_canonical_json: row_text(x, 10)?,
            unknowns_canonical_json: row_text(x, 11)?,
            assumptions_canonical_json: row_text(x, 12)?,
            losses_canonical_json: row_text(x, 13)?,
            projection_hash: parse_hash(row_text(x, 14)?)?,
            body_hash: parse_hash(row_text(x, 15)?)?,
        })
    })?;
    let mut rows = reserved_query_vec(c, "SELECT COUNT(*) FROM context_envelopes")?;
    for row in r {
        rows.push(row.map_err(|_| IndexError::CorruptIndex)?);
    }
    let validation_source = if rows.is_empty() {
        None
    } else {
        Some(validation_source.ok_or(IndexError::CorruptIndex)?)
    };
    for row in &rows {
        validate_context_row(
            c,
            row,
            validation_source.expect("nonempty rows require a validation source"),
        )?;
    }
    Ok(rows)
}
fn query_review_plans(
    c: &Connection,
    validation_source: Option<D1ValidationSource<'_>>,
) -> Result<Vec<IndexReviewPlan>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,plan_id,universe_id,snapshot_id,planner_input_hash,planner_policy_version,planner_policy_hash,budget_canonical_json,budget_hash,risk_breakdown_canonical_json,waves_canonical_json,deferred_canonical_json,identity_body_hash,body_hash FROM review_plans ORDER BY event_sequence,plan_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexReviewPlan {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(row_text(x, 1)?)?,
            plan_id: parse_id(row_text(x, 2)?)?,
            universe_id: parse_id(row_text(x, 3)?)?,
            snapshot_id: parse_id(row_text(x, 4)?)?,
            planner_input_hash: parse_hash(row_text(x, 5)?)?,
            planner_policy_version: row_text(x, 6)?,
            planner_policy_hash: parse_hash(row_text(x, 7)?)?,
            budget_canonical_json: row_text(x, 8)?,
            budget_hash: parse_hash(row_text(x, 9)?)?,
            risk_breakdown_canonical_json: row_text(x, 10)?,
            waves_canonical_json: row_text(x, 11)?,
            deferred_canonical_json: row_text(x, 12)?,
            identity_body_hash: parse_hash(row_text(x, 13)?)?,
            body_hash: parse_hash(row_text(x, 14)?)?,
        })
    })?;
    let mut rows = reserved_query_vec(c, "SELECT COUNT(*) FROM review_plans")?;
    for row in r {
        rows.push(row.map_err(|_| IndexError::CorruptIndex)?);
    }
    let validation_source = if rows.is_empty() {
        None
    } else {
        Some(validation_source.ok_or(IndexError::CorruptIndex)?)
    };
    for row in &rows {
        validate_review_plan_row(
            row,
            validation_source.expect("nonempty rows require a validation source"),
        )?;
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reviewgraphen_core::{
        ArtifactRegistered, ArtifactSensitivity, ArtifactSource, EventCommand, EventLog, Evidence,
        EvidenceDetails, ExecutionClaimInputV2, ExecutionOutcome, ExecutionRecordInput,
        MvpRulePack, ObligationLifecycle, PlanBudget, ProgramSpace, Provenance, ReviewAggregate,
        SnapshotSourceRecordEntry, SnapshotSourcesRecorded, SourceRef, ValidatedExecutionBundle,
        plan, prepare_context,
    };
    use serde_json::Value;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::Path;
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::mpsc,
        time::Duration,
    };

    type IndexMutation = Box<dyn Fn(&Connection)>;

    #[test]
    fn schema_v3_accepts_d2_execution_payload_kind() {
        let input = ExecutionRecordInput::fake(
            StableId::parse("plan:fixture").unwrap(),
            StableId::parse("schedule-wave:fixture").unwrap(),
            StableId::parse("obligation:fixture").unwrap(),
            StableId::parse("context-envelope:fixture").unwrap(),
            StableId::parse("snapshot:fixture").unwrap(),
            1,
        )
        .unwrap();
        let raw = b"fixture";
        let registration = ArtifactRegistered::reviewer_execution(
            StableId::parse("run:fixture").unwrap(),
            input.execution_id().unwrap(),
            "reviewgraphen.fake_reviewer@1",
            ContentHash::sha256(raw),
            "application/json",
            u64::try_from(raw.len()).unwrap(),
        )
        .unwrap();
        let claim = ExecutionClaimInputV2::new(
            "property.fixture",
            BTreeSet::from([StableId::parse("file:fixture").unwrap()]),
            reviewgraphen_core::ClaimPolarity::IssueAbsent,
            "bounded fixture",
            BTreeSet::from([StableId::parse("file:fixture").unwrap()]),
            BTreeSet::new(),
            BTreeSet::new(),
            None,
        )
        .unwrap();
        let bundle = ValidatedExecutionBundle::fake(
            input,
            &registration,
            raw.to_vec(),
            vec![],
            vec![claim],
            ExecutionOutcome::Structured,
        )
        .unwrap();
        let payload = DecodedPayload::ReviewExecutionRecorded {
            execution: bundle.execution().clone(),
            claims: bundle.claims().to_vec(),
        };
        assert_eq!(
            payload_kind_decoded(&payload).unwrap(),
            "review_execution_recorded"
        );
    }

    #[test]
    fn schema_v3_projects_atomic_d2_execution_and_full_claim_rows() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index) = d2_index_fixture(&root);
        index.rebuild(&journal, &cas).unwrap();
        let first = index.snapshot_current(&journal).unwrap();
        assert_eq!(first.marker.sqlite_user_version, 3);
        assert_eq!(first.executions.len(), 1);
        assert_eq!(first.claims.len(), 1);
        let execution = &first.executions[0];
        let claim = &first.claims[0];
        assert_eq!(execution.event_sequence, claim.event_sequence);
        assert_eq!(execution.event_id, claim.event_id);
        assert_eq!(execution.execution_id, claim.execution_id);
        assert_eq!(execution.reviewer_kind, "fake");
        assert_eq!(execution.inference_settings_canonical_json, "{}");
        assert_eq!(execution.tool_calls_canonical_json, "[]");
        assert_eq!(execution.outcome_kind, "structured");
        assert_eq!(claim.disposition, "proposed");
        assert_eq!(claim.author_kind, "ai");
        assert_eq!(claim.review_status, "unreviewed");
        assert_eq!(claim.candidate_confidence_canonical_json, "null");
        let image = index.read_active_image().unwrap();
        let connection = deserialize_read_only(image, index.limits()).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM executions", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM claims", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        drop(connection);
        std::fs::remove_file(root.path().join(INDEX_DIR).join(ACTIVE_FILE)).unwrap();
        index.rebuild(&journal, &cas).unwrap();
        assert_eq!(first, index.snapshot_current(&journal).unwrap());
    }

    #[test]
    fn schema_v3_rejects_d2_claim_tamper_and_same_event_atomicity_breaks() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index) = d2_index_fixture(&root);
        index.rebuild(&journal, &cas).unwrap();
        for mutation in ["UPDATE claims SET summary='tampered'", "DELETE FROM claims"] {
            index.rebuild(&journal, &cas).unwrap();
            mutate_active_image(&index, |connection| {
                connection.execute_batch(mutation).unwrap();
            });
            assert!(
                matches!(
                    index.snapshot_current(&journal),
                    Err(IndexError::CorruptIndex)
                ),
                "{mutation}"
            );
        }
        index.rebuild(&journal, &cas).unwrap();
        mutate_active_image(&index, |connection| {
            connection
                .pragma_update(None, "foreign_keys", false)
                .unwrap();
            connection
                .execute("UPDATE claims SET event_sequence=1", [])
                .unwrap();
        });
        assert!(matches!(
            index.snapshot_current(&journal),
            Err(IndexError::CorruptIndex)
        ));
    }

    #[test]
    fn schema_v3_d2_rows_accept_exact_aggregate_limit_and_refuse_plus_one_atomically() {
        let discovery_workspace = tempfile::tempdir().unwrap();
        let discovery_root =
            StoreRoot::open(discovery_workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index) = d2_index_fixture(&discovery_root);
        index.rebuild(&journal, &cas).unwrap();
        let connection =
            deserialize_read_only(index.read_active_image().unwrap(), index.limits()).unwrap();
        let tables = [
            "index_meta",
            "events",
            "program_objects",
            "program_relations",
            "universe",
            "obligations",
            "obligation_lifecycle",
            "executions",
            "claims",
            "artifact_registrations",
            "snapshot_source_index",
            "review_plans",
            "context_envelopes",
            "unreconciled_authority_records",
            "projected_findings",
        ];
        let exact_rows = tables
            .iter()
            .try_fold(0_u64, |total, table| {
                let count: i64 = connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })
                    .map_err(map_sql)?;
                total
                    .checked_add(u64::try_from(count).map_err(|_| IndexError::IntegerOutOfRange)?)
                    .ok_or(IndexError::IntegerOutOfRange)
            })
            .unwrap();
        assert!(exact_rows > 2);
        drop(connection);
        let exact_workspace = tempfile::tempdir().unwrap();
        let exact_root = StoreRoot::open(
            exact_workspace.path(),
            StoreLimits {
                max_index_rows: exact_rows,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let (exact_journal, exact_cas, exact_index) = d2_index_fixture(&exact_root);
        exact_index.rebuild(&exact_journal, &exact_cas).unwrap();
        let low_workspace = tempfile::tempdir().unwrap();
        let low_root = StoreRoot::open(
            low_workspace.path(),
            StoreLimits {
                max_index_rows: exact_rows - 1,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let (low_journal, low_cas, low_index) = d2_index_fixture(&low_root);
        assert!(matches!(
            low_index.rebuild(&low_journal, &low_cas),
            Err(IndexError::Incomplete { limit, observed }) if limit == exact_rows - 1 && observed == exact_rows
        ));
        assert!(matches!(
            low_index.snapshot_current(&low_journal),
            Err(IndexError::Missing)
        ));
    }

    fn d2_execution_payload(
        journal: &EventJournal<'_>,
    ) -> (
        EventEnvelope,
        reviewgraphen_core::ExecutionRecord,
        Vec<reviewgraphen_core::ExecutionClaimV2>,
    ) {
        let reader = journal.reader().unwrap();
        reader.with_locked_snapshot(|events, _, _| {
            events
                .iter()
                .find_map(|event| {
                    match event.decode_for_streaming_projection().unwrap().payload() {
                        DecodedPayload::ReviewExecutionRecorded { execution, claims } => {
                            Some((event.clone(), execution.clone(), claims.to_vec()))
                        }
                        _ => None,
                    }
                })
                .unwrap()
        })
    }

    #[test]
    fn d2_insert_prevalidates_every_domain_reference_before_any_row_reservation() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index) = d2_index_fixture(&root);
        index.rebuild(&journal, &cas).unwrap();
        let (event, execution, claims) = d2_execution_payload(&journal);
        let mutations: [fn(&Connection); 6] = [
            |connection| {
                connection.execute("DELETE FROM review_plans", []).unwrap();
            },
            |connection| {
                connection
                    .execute("UPDATE review_plans SET waves_canonical_json='[]'", [])
                    .unwrap();
            },
            |connection| {
                connection
                    .execute("DELETE FROM context_envelopes", [])
                    .unwrap();
            },
            |connection| {
                connection
                    .execute(
                        "DELETE FROM artifact_registrations WHERE sensitivity='sensitive'",
                        [],
                    )
                    .unwrap();
            },
            |connection| {
                connection
                    .execute("UPDATE obligations SET property_id='property:tampered'", [])
                    .unwrap();
            },
            |connection| {
                connection
                    .execute(
                        "UPDATE context_envelopes SET included_sources_canonical_json='[]'",
                        [],
                    )
                    .unwrap();
            },
        ];
        for mutation in mutations {
            index.rebuild(&journal, &cas).unwrap();
            let image = index.read_active_image().unwrap();
            let mut connection = Connection::open_in_memory().unwrap();
            connection
                .deserialize_read_exact(MAIN_DB, Cursor::new(image.clone()), image.len(), false)
                .unwrap();
            connection.execute("DELETE FROM claims", []).unwrap();
            connection.execute("DELETE FROM executions", []).unwrap();
            mutation(&connection);
            let transaction = connection.transaction().unwrap();
            let mut rows = 0;
            assert!(matches!(
                insert_execution_and_claims(
                    &transaction,
                    &event,
                    &execution,
                    &claims,
                    &mut rows,
                    index.limits(),
                ),
                Err(IndexError::ProjectionContractViolation)
            ));
            assert_eq!(rows, 0);
            assert_eq!(
                transaction
                    .query_row("SELECT COUNT(*) FROM executions", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert_eq!(
                transaction
                    .query_row("SELECT COUNT(*) FROM claims", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            transaction.rollback().unwrap();
        }
    }

    fn coherent_claim_body_hash(row: &IndexClaim) -> ContentHash {
        let obligation_ids = exact_id_set(&row.obligation_ids_canonical_json).unwrap();
        let target_refs = exact_id_set(&row.target_refs_canonical_json).unwrap();
        let source_ids = exact_id_set(&row.source_ids_canonical_json).unwrap();
        let assumptions = exact_typed_json(&row.assumptions_canonical_json).unwrap();
        let requested_evidence = exact_typed_json(&row.requested_evidence_canonical_json).unwrap();
        let confidence = exact_typed_json(&row.candidate_confidence_canonical_json).unwrap();
        let polarity = parse_closed_enum(&row.polarity).unwrap();
        let disposition = parse_closed_enum(&row.disposition).unwrap();
        let author_kind = parse_closed_enum(&row.author_kind).unwrap();
        let review_status = parse_closed_enum(&row.review_status).unwrap();
        ContentHash::sha256(
            &canonical_json(&ClaimBody {
                assumptions: &assumptions,
                author_kind,
                candidate_confidence: confidence,
                disposition,
                execution_id: &row.execution_id,
                id: &row.claim_id,
                obligation_ids: &obligation_ids,
                polarity,
                property_id: &row.property_id,
                requested_evidence: &requested_evidence,
                review_status,
                source_ids: &source_ids,
                summary: &row.summary,
                target_refs: &target_refs,
            })
            .unwrap(),
        )
    }

    fn coherent_registration_body_hash(row: &IndexArtifactRegistration) -> ContentHash {
        let source = exact_typed_json(&row.source_id).unwrap();
        let sensitivity = parse_closed_enum(&row.sensitivity).unwrap();
        ContentHash::sha256(
            &canonical_json(&RegistrationBody {
                cas_hash: &row.cas_hash,
                media_type: &row.media_type,
                registration_id: &row.registration_id,
                run_id: &row.run_id,
                sensitivity,
                size: row.size,
                source: &source,
            })
            .unwrap(),
        )
    }

    #[test]
    fn d2_journal_comparator_rejects_coherently_rehashed_claim_and_registration_rows() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index) = d2_index_fixture(&root);
        index.rebuild(&journal, &cas).unwrap();
        let snapshot = index.snapshot_current(&journal).unwrap();

        let mut claim = snapshot.claims[0].clone();
        claim.candidate_confidence_canonical_json = "0.5".to_owned();
        claim.body_hash = coherent_claim_body_hash(&claim);
        mutate_active_image(&index, |connection| {
            connection
                .execute(
                    "UPDATE claims SET candidate_confidence_canonical_json=?1,body_hash=?2",
                    rusqlite::params![
                        claim.candidate_confidence_canonical_json,
                        claim.body_hash.to_string()
                    ],
                )
                .unwrap();
        });
        assert!(matches!(
            index.snapshot_current(&journal),
            Err(IndexError::CorruptIndex)
        ));

        for (media_type, size) in [
            ("application/x-tampered", None),
            ("application/json", Some(99_u64)),
        ] {
            index.rebuild(&journal, &cas).unwrap();
            let mut registration = snapshot
                .artifact_registrations
                .iter()
                .find(|row| row.sensitivity == "sensitive")
                .unwrap()
                .clone();
            registration.media_type = media_type.to_owned();
            if let Some(size) = size {
                registration.size = size;
            }
            registration.body_hash = coherent_registration_body_hash(&registration);
            mutate_active_image(&index, |connection| {
                connection
                    .execute(
                        "UPDATE artifact_registrations SET media_type=?1,size=?2,body_hash=?3 WHERE registration_id=?4",
                        rusqlite::params![
                            registration.media_type,
                            to_i64(registration.size).unwrap(),
                            registration.body_hash.to_string(),
                            registration.registration_id.to_string(),
                        ],
                    )
                    .unwrap();
            });
            assert!(matches!(
                index.snapshot_current(&journal),
                Err(IndexError::CorruptIndex)
            ));
        }
    }

    #[test]
    fn exact_fd_read_admits_before_allocating_and_returns_exact_capacity() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"D1v2").unwrap();
        std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o600)).unwrap();

        let bytes = read_fd_exact_with_retained(file.reopen().unwrap().into(), 4, 5, 9).unwrap();
        assert_eq!(bytes, b"D1v2");
        assert_eq!(bytes.capacity(), 4);

        assert!(matches!(
            read_fd_exact_with_retained(file.reopen().unwrap().into(), 4, 5, 8),
            Err(IndexError::Incomplete {
                limit: 8,
                observed: 9
            })
        ));
        assert!(matches!(
            read_fd_exact_with_retained(file.reopen().unwrap().into(), 3, 0, 9),
            Err(IndexError::Incomplete {
                limit: 3,
                observed: 4
            })
        ));
    }

    #[test]
    fn build_connection_peak_is_refused_before_sqlite_open() {
        let retained = 777;
        let exact = retained + 3 * PAGE_SIZE + 1024;
        let base = IndexLimits {
            max_rows: 1,
            max_serialized_bytes: PAGE_SIZE,
            max_working_bytes: exact,
            max_query_bytes: 1,
            max_statement_bytes: 1,
        };
        preflight_build_connection(base, retained).unwrap();
        assert!(matches!(
            preflight_build_connection(
                IndexLimits {
                    max_working_bytes: exact - 1,
                    ..base
                },
                retained,
            ),
            Err(IndexError::Incomplete { limit, observed })
                if limit == exact - 1 && observed == exact
        ));
    }

    #[test]
    fn every_persisted_family_maps_structural_plus_one_through_actual_rebuild() {
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
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index, _) = v2_fixture(&root);
        let run_directory = std::fs::read_dir(root.path().join("runs"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let log = run_directory.join("logs.jsonl");
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
            for (line, expected_limit) in [
                (family_json(kind, 65_534, 1), 65_536),
                (family_json(kind, 1, 127), 128),
            ] {
                std::fs::write(&log, line).unwrap();
                let result = index.rebuild(&journal, &cas);
                assert!(
                    matches!(
                        result,
                        Err(IndexError::Incomplete { limit, observed })
                            if limit == expected_limit && observed == expected_limit + 1
                    ),
                    "{kind}: {result:?}"
                );
            }
        }
    }

    fn v2_fixture<'a>(
        root: &'a StoreRoot,
    ) -> (
        EventJournal<'a>,
        CasStore<'a>,
        DerivedIndex<'a>,
        EventEnvelope,
    ) {
        let program = ProgramSpace::from_json_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let run_id = StableId::parse("run:index-e2e").unwrap();
        let aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let mut log = EventLog::new(run_id.clone(), aggregate).unwrap();
        let bytes = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let cas = CasStore::open(root).unwrap();
        let hash = CasHash::parse(ContentHash::sha256(&bytes).to_string()).unwrap();
        cas.put(
            &hash,
            Some(u64::try_from(bytes.len()).unwrap()),
            bytes.as_slice(),
        )
        .unwrap();
        let identity =
            JournalIdentity::new(run_id, super::super::JournalGenesis::V2(bytes)).unwrap();
        let manifest = log.envelopes().next().unwrap().clone();
        let journal = EventJournal::initialize_v2(root, identity, manifest).unwrap();
        let obligation = log.aggregate().obligations().next().unwrap().id().clone();
        log.append(EventCommand::obligation_transition(
            obligation,
            ObligationLifecycle::Planned,
        ))
        .unwrap();
        let transition = log.envelopes().nth(1).unwrap().clone();
        let index = DerivedIndex::open(root).unwrap();
        (journal, cas, index, transition)
    }

    fn d1_index_fixture<'a>(
        root: &'a StoreRoot,
    ) -> (EventJournal<'a>, CasStore<'a>, DerivedIndex<'a>) {
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
        index_fixture_with_sources(root, "run:index-d1", source_bytes, false)
    }

    fn d1_index_fixture_with_sources<'a>(
        root: &'a StoreRoot,
        run: &str,
        source_bytes: BTreeMap<&'static str, Vec<u8>>,
    ) -> (EventJournal<'a>, CasStore<'a>, DerivedIndex<'a>) {
        index_fixture_with_sources(root, run, source_bytes, false)
    }

    fn d2_index_fixture<'a>(
        root: &'a StoreRoot,
    ) -> (EventJournal<'a>, CasStore<'a>, DerivedIndex<'a>) {
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
        index_fixture_with_sources(root, "run:index-d2", source_bytes, true)
    }

    fn index_fixture_with_sources<'a>(
        root: &'a StoreRoot,
        run: &str,
        source_bytes: BTreeMap<&'static str, Vec<u8>>,
        include_d2: bool,
    ) -> (EventJournal<'a>, CasStore<'a>, DerivedIndex<'a>) {
        let run_id = StableId::parse(run).unwrap();
        let mut value: Value = serde_json::from_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let mut contains = value["relations"].as_array().unwrap()[0].clone();
        contains["id"] = Value::String("relation:index-file-contains-payment-charge".to_owned());
        contains["kind"] = Value::String("contains".to_owned());
        contains["source_id"] = Value::String("file:payment-repository".to_owned());
        contains["target_ids"] = serde_json::json!(["function:payment-charge"]);
        contains["directed"] = Value::Bool(true);
        value["relations"].as_array_mut().unwrap().push(contains);
        if source_bytes.contains_key("src/auxiliary_repository.rs") {
            let mut auxiliary = value["artifacts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|artifact| artifact["id"] == "file:payment-repository")
                .unwrap()
                .clone();
            auxiliary["id"] = Value::String("file:auxiliary-repository".to_owned());
            auxiliary["label"] = Value::String("src/auxiliary_repository.rs".to_owned());
            auxiliary["location"]["path"] = Value::String("src/auxiliary_repository.rs".to_owned());
            value["artifacts"].as_array_mut().unwrap().push(auxiliary);
            let mut auxiliary_contains = value["relations"].as_array().unwrap()[0].clone();
            auxiliary_contains["id"] =
                Value::String("relation:index-auxiliary-contains-payment-charge".to_owned());
            auxiliary_contains["kind"] = Value::String("contains".to_owned());
            auxiliary_contains["source_id"] = Value::String("file:auxiliary-repository".to_owned());
            auxiliary_contains["target_ids"] = serde_json::json!(["function:payment-charge"]);
            auxiliary_contains["directed"] = Value::Bool(true);
            value["relations"]
                .as_array_mut()
                .unwrap()
                .push(auxiliary_contains);
        }
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
        let cas = CasStore::open(root).unwrap();
        let genesis_hash = CasHash::parse(ContentHash::sha256(&genesis).to_string()).unwrap();
        cas.put(
            &genesis_hash,
            Some(u64::try_from(genesis.len()).unwrap()),
            genesis.as_slice(),
        )
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
                adapter_id: "index-d1-fixture@1".to_owned(),
            };
            let registration_id = StableId::derived(
                "registration",
                &BTreeMap::from([
                    ("run_id".to_owned(), Value::String(run_id.to_string())),
                    ("cas_hash".to_owned(), Value::String(hash.to_string())),
                    (
                        "media_type".to_owned(),
                        Value::String("application/octet-stream".to_owned()),
                    ),
                    (
                        "sensitivity".to_owned(),
                        Value::String("workspace_source".to_owned()),
                    ),
                    ("source".to_owned(), serde_json::to_value(&source).unwrap()),
                ]),
            )
            .unwrap();
            log.append(EventCommand::artifact_registered(
                ArtifactRegistered::new(
                    run_id.clone(),
                    registration_id.clone(),
                    hash.clone(),
                    "application/octet-stream",
                    u64::try_from(bytes.len()).unwrap(),
                    ArtifactSensitivity::WorkspaceSource,
                    source,
                )
                .unwrap(),
            ))
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

        if include_d2 {
            let obligation = log.aggregate().obligations().next().unwrap().id().clone();
            log.append(EventCommand::obligation_transition(
                obligation.clone(),
                ObligationLifecycle::Planned,
            ))
            .unwrap();
            log.append(EventCommand::obligation_transition(
                obligation.clone(),
                ObligationLifecycle::InProgress,
            ))
            .unwrap();
            let plan = log.aggregate().review_plans().next().unwrap().clone();
            let envelope = log.aggregate().context_envelopes().next().unwrap().clone();
            let wave = plan
                .waves()
                .iter()
                .find(|wave| wave.obligation_ids().contains(&obligation))
                .unwrap();
            let input = ExecutionRecordInput::fake(
                plan.id().clone(),
                wave.id().clone(),
                obligation.clone(),
                envelope.id().clone(),
                program.snapshot_id().clone(),
                1,
            )
            .unwrap();
            let raw = b"{\"schema\":\"reviewgraphen.reviewer_output.v1\",\"claims\":[]}".to_vec();
            let raw_hash = ContentHash::sha256(&raw);
            let raw_cas = CasHash::parse(raw_hash.to_string()).unwrap();
            cas.put(
                &raw_cas,
                Some(u64::try_from(raw.len()).unwrap()),
                Cursor::new(raw.as_slice()),
            )
            .unwrap();
            let registration = ArtifactRegistered::reviewer_execution(
                run_id.clone(),
                input.execution_id().unwrap(),
                "reviewgraphen.fake_reviewer@1",
                raw_hash,
                "application/json",
                u64::try_from(raw.len()).unwrap(),
            )
            .unwrap();
            log.append(EventCommand::artifact_registered(registration.clone()))
                .unwrap();
            let target = log
                .aggregate()
                .obligations()
                .next()
                .unwrap()
                .normalized_target_refs()
                .iter()
                .next()
                .unwrap()
                .clone();
            let source = envelope
                .normalized_included_source_ids()
                .iter()
                .next()
                .unwrap()
                .clone();
            let claim = ExecutionClaimInputV2::new(
                log.aggregate().obligations().next().unwrap().property_id(),
                BTreeSet::from([target]),
                reviewgraphen_core::ClaimPolarity::IssueAbsent,
                "fixture structured no-issue claim",
                BTreeSet::from([source]),
                BTreeSet::new(),
                BTreeSet::new(),
                None,
            )
            .unwrap();
            let bundle = ValidatedExecutionBundle::fake(
                input,
                &registration,
                raw,
                Vec::new(),
                vec![claim],
                ExecutionOutcome::Structured,
            )
            .unwrap();
            log.append(EventCommand::review_execution_recorded(bundle))
                .unwrap();
        }

        let identity =
            JournalIdentity::new(run_id, super::super::JournalGenesis::V2(genesis)).unwrap();
        let events = log.envelopes().cloned().collect::<Vec<_>>();
        let journal = EventJournal::initialize_v2(root, identity, events[0].clone()).unwrap();
        let mut writer = journal.writer().unwrap();
        for event in events.into_iter().skip(1) {
            writer.append(event).unwrap();
        }
        drop(writer);
        let index = DerivedIndex::open(root).unwrap();
        (journal, cas, index)
    }

    fn terminal_aggregate(journal: &EventJournal<'_>) -> ReviewAggregate {
        let reader = journal.reader().unwrap();
        let identity = reader.identity().clone();
        reader.with_locked_snapshot(|events, _, _| {
            let view = EventEnvelope::validated_view(
                identity.version(),
                &identity.run_id,
                identity.core_genesis(),
                events,
            )
            .unwrap();
            let reviewgraphen_core::EventStreamGenesis::V2(bytes) = identity.core_genesis() else {
                unreachable!()
            };
            let initial = RunGenesisSnapshot::from_canonical_bytes(bytes)
                .unwrap()
                .rebuild_aggregate()
                .unwrap();
            let mut projection = OfflineProjectionState::new(&view, initial).unwrap();
            for event in view.events() {
                projection.apply(event).unwrap();
            }
            projection.aggregate().clone()
        })
    }

    #[test]
    fn schema_v2_projection_accepts_both_d1_payload_tags() {
        let program = ProgramSpace::from_json_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let review_plan = plan(&aggregate, PlanBudget::new(16, 2).unwrap()).unwrap();

        assert_eq!(
            payload_kind_decoded(&DecodedPayload::ReviewPlanRecorded(review_plan)).unwrap(),
            "review_plan_recorded"
        );
    }

    #[test]
    fn plan_budget_minimum_and_maximum_have_exact_bytes_and_hashes() {
        for (budget, expected) in [
            (
                PlanBudget::new(1, 1).unwrap(),
                r#"{"max_obligations_per_wave":1,"max_waves":1}"#,
            ),
            (
                PlanBudget::new(1024, 2048).unwrap(),
                r#"{"max_obligations_per_wave":2048,"max_waves":1024}"#,
            ),
        ] {
            let bytes = canonical_json(&budget).unwrap();
            assert_eq!(bytes, expected.as_bytes());
            assert!(bytes.len() <= 50);
            assert_eq!(
                ContentHash::sha256(&bytes),
                ContentHash::sha256(expected.as_bytes())
            );
        }
        assert_eq!(
            canonical_json(&PlanBudget::new(1024, 2048).unwrap())
                .unwrap()
                .len(),
            50
        );
        for mutation in [
            r#"{"max_waves":1,"max_obligations_per_wave":1}"#,
            r#"{"max_obligations_per_wave":1,"max_waves":1 }"#,
            r#"{"max_obligations_per_wave":0,"max_waves":1}"#,
            r#"{"extra":1,"max_obligations_per_wave":1,"max_waves":1}"#,
        ] {
            let rejected = (|| -> Result<(), IndexError> {
                let object = exact_component_object(mutation)?;
                if object.len() != 2 {
                    return Err(IndexError::CorruptIndex);
                }
                let per_wave = object
                    .get("max_obligations_per_wave")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or(IndexError::CorruptIndex)?;
                let waves = object
                    .get("max_waves")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or(IndexError::CorruptIndex)?;
                PlanBudget::new(waves, per_wave).map_err(|_| IndexError::CorruptIndex)?;
                Ok(())
            })();
            assert!(
                matches!(rejected, Err(IndexError::CorruptIndex)),
                "{mutation}"
            );
        }
    }

    #[test]
    fn d1_plan_and_context_rebuild_query_complete_v2_rows_deterministically() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index) = d1_index_fixture(&root);
        let reader = journal.reader().unwrap();
        let identity = reader.identity().clone();
        let aggregate = match identity.core_genesis() {
            reviewgraphen_core::EventStreamGenesis::V2(bytes) => {
                RunGenesisSnapshot::from_canonical_bytes(bytes)
                    .unwrap()
                    .rebuild_aggregate()
                    .unwrap()
            }
            reviewgraphen_core::EventStreamGenesis::V1(_) => unreachable!(),
            reviewgraphen_core::EventStreamGenesis::V2Verified(_) => unreachable!(),
            reviewgraphen_core::EventStreamGenesis::V3(_) => unreachable!(),
        };
        reader.with_locked_snapshot(|events, _, _| {
            let view = EventEnvelope::validated_view(
                identity.version(),
                &identity.run_id,
                identity.core_genesis(),
                events,
            )
            .unwrap();
            for event in view.events() {
                match event.payload() {
                    DecodedPayload::ReviewPlanRecorded(plan) => {
                        projected_review_plan(event.envelope(), plan).unwrap();
                    }
                    DecodedPayload::ContextEnvelopeProjected(context) => {
                        projected_context_envelope(event.envelope(), context).unwrap();
                    }
                    _ => {}
                }
            }
        });
        drop(reader);
        let first_receipt = index.rebuild(&journal, &cas).unwrap();
        let first = index.snapshot_current(&journal).unwrap();
        assert_eq!(first.marker.sqlite_user_version, 3);
        assert_eq!(first.marker.index_schema_version, 3);
        assert_eq!(
            first.marker.projection_contract_version,
            "reviewgraphen.index_projection.v3"
        );
        assert_eq!(first.review_plans.len(), 1);
        assert_eq!(first.context_envelopes.len(), 1);
        assert!(first.executions.is_empty());
        assert!(first.claims.is_empty());
        let plan = &first.review_plans[0];
        assert_eq!(
            plan.budget_hash,
            ContentHash::sha256(plan.budget_canonical_json.as_bytes())
        );
        assert_eq!(plan.planner_policy_version, PlannerPolicyV1::VERSION);
        validate_review_plan_row(plan, D1ValidationSource::Aggregate(&aggregate)).unwrap();
        let context = &first.context_envelopes[0];
        assert_eq!(
            context.context_policy_hash,
            ContentHash::sha256(context.context_policy_canonical_json.as_bytes())
        );
        assert!(
            exact_json_value(&context.losses_canonical_json)
                .unwrap()
                .is_array()
        );

        let connection =
            deserialize_read_only(index.read_active_image().unwrap(), index.limits()).unwrap();
        let executions: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='executions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(executions, 1);
        drop(connection);

        std::fs::remove_file(root.path().join(INDEX_DIR).join(ACTIVE_FILE)).unwrap();
        let second_receipt = index.rebuild(&journal, &cas).unwrap();
        let second = index.snapshot_current(&journal).unwrap();
        assert_eq!(first_receipt.event_count, second_receipt.event_count);
        assert_eq!(first, second);
    }

    #[test]
    fn core_strict_boundaries_reject_self_consistently_rehashed_d1_rows() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index) = d1_index_fixture(&root);
        index.rebuild(&journal, &cas).unwrap();
        let snapshot = index.snapshot_current(&journal).unwrap();
        let aggregate = terminal_aggregate(&journal);

        let mut plan_row = snapshot.review_plans[0].clone();
        let mut waves = exact_json_value(&plan_row.waves_canonical_json).unwrap();
        waves.as_array_mut().unwrap()[0]["obligation_ids"] = serde_json::json!([]);
        plan_row.waves_canonical_json = String::from_utf8(canonical_json(&waves).unwrap()).unwrap();
        let deferred = exact_json_value(&plan_row.deferred_canonical_json).unwrap();
        let budget = exact_json_value(&plan_row.budget_canonical_json).unwrap();
        let identity = object_with_fields([
            ("budget", budget.clone()),
            (
                "deferred_ids",
                Value::Array(
                    deferred
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|value| value["id"].clone())
                        .collect(),
                ),
            ),
            (
                "planner_input_hash",
                Value::String(plan_row.planner_input_hash.to_string()),
            ),
            (
                "planner_policy_hash",
                Value::String(plan_row.planner_policy_hash.to_string()),
            ),
            (
                "planner_policy_version",
                Value::String(plan_row.planner_policy_version.clone()),
            ),
            (
                "snapshot_id",
                Value::String(plan_row.snapshot_id.to_string()),
            ),
            (
                "universe_id",
                Value::String(plan_row.universe_id.to_string()),
            ),
            ("waves", waves.clone()),
        ]);
        plan_row.identity_body_hash = ContentHash::sha256(&canonical_json(&identity).unwrap());
        plan_row.plan_id =
            StableId::parse(format!("plan:{}", plan_row.identity_body_hash)).unwrap();
        let full = object_with_fields([
            ("budget", budget),
            ("deferred", deferred),
            ("id", Value::String(plan_row.plan_id.to_string())),
            (
                "planner_input_hash",
                Value::String(plan_row.planner_input_hash.to_string()),
            ),
            (
                "planner_policy_hash",
                Value::String(plan_row.planner_policy_hash.to_string()),
            ),
            (
                "planner_policy_version",
                Value::String(plan_row.planner_policy_version.clone()),
            ),
            (
                "risk_breakdown",
                exact_json_value(&plan_row.risk_breakdown_canonical_json).unwrap(),
            ),
            (
                "snapshot_id",
                Value::String(plan_row.snapshot_id.to_string()),
            ),
            (
                "universe_id",
                Value::String(plan_row.universe_id.to_string()),
            ),
            ("waves", waves),
        ]);
        plan_row.body_hash = ContentHash::sha256(&canonical_json(&full).unwrap());
        assert!(matches!(
            validate_review_plan_row(&plan_row, D1ValidationSource::Aggregate(&aggregate)),
            Err(IndexError::CorruptIndex)
        ));

        let mut context_row = snapshot.context_envelopes[0].clone();
        let mut included = exact_json_value(&context_row.included_sources_canonical_json).unwrap();
        included.as_array_mut().unwrap()[0]["excerpt"] =
            serde_json::json!({"end_line":101,"start_line":1});
        context_row.included_sources_canonical_json =
            String::from_utf8(canonical_json(&included).unwrap()).unwrap();
        context_row.projection_hash =
            ContentHash::sha256(&context_identity_body_bytes(&context_row).unwrap());
        context_row.envelope_id =
            StableId::parse(format!("context-envelope:{}", context_row.projection_hash)).unwrap();
        let normalized = String::from_utf8(
            canonical_json(&Value::Array(
                included
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|source| source["artifact_id"].clone())
                    .collect(),
            ))
            .unwrap(),
        )
        .unwrap();
        context_row.body_hash =
            ContentHash::sha256(&context_full_body_bytes(&context_row, &normalized).unwrap());
        let connection =
            deserialize_read_only(index.read_active_image().unwrap(), index.limits()).unwrap();
        assert!(matches!(
            validate_context_row(
                &connection,
                &context_row,
                D1ValidationSource::Aggregate(&aggregate),
            ),
            Err(IndexError::CorruptIndex)
        ));
    }

    #[test]
    fn schema_v1_image_is_rebuild_required_and_never_migrated_in_place() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index, _) = v2_fixture(&root);
        let legacy = Connection::open_in_memory().unwrap();
        legacy
            .pragma_update(None, "page_size", i64::try_from(PAGE_SIZE).unwrap())
            .unwrap();
        legacy.pragma_update(None, "user_version", 1).unwrap();
        legacy
            .execute_batch("CREATE TABLE legacy_rows(value TEXT) STRICT; INSERT INTO legacy_rows VALUES('must-not-migrate');")
            .unwrap();
        let legacy_bytes = legacy.serialize(MAIN_DB).unwrap().to_vec();
        let active = root.path().join(INDEX_DIR).join(ACTIVE_FILE);
        std::fs::write(&active, legacy_bytes).unwrap();
        std::fs::set_permissions(&active, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            index.snapshot_current(&journal),
            Err(IndexError::RebuildRequired {
                found: 1,
                required: 3
            })
        ));

        index.rebuild(&journal, &cas).unwrap();
        let snapshot = index.snapshot_current(&journal).unwrap();
        assert_eq!(snapshot.marker.index_schema_version, 3);
        let rebuilt =
            deserialize_read_only(index.read_active_image().unwrap(), index.limits()).unwrap();
        let legacy_tables: i64 = rebuilt
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='legacy_rows'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_tables, 0);
    }

    #[test]
    fn schema_v2_image_is_rebuild_required_and_never_upcast_in_place() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index, _) = v2_fixture(&root);
        index.rebuild(&journal, &cas).unwrap();
        mutate_active_image(&index, |connection| {
            connection.pragma_update(None, "user_version", 2).unwrap();
        });
        assert!(matches!(
            index.snapshot_current(&journal),
            Err(IndexError::RebuildRequired {
                found: 2,
                required: 3
            })
        ));
        index.rebuild(&journal, &cas).unwrap();
        assert_eq!(
            index
                .snapshot_current(&journal)
                .unwrap()
                .marker
                .index_schema_version,
            3
        );
    }

    #[test]
    fn locked_journal_rejects_every_sqlite_event_and_d1_hash_preimage_mutation() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index) = d1_index_fixture(&root);
        let mutations: Vec<IndexMutation> = vec![
            Box::new(|connection| {
                connection
                    .pragma_update(None, "foreign_keys", false)
                    .unwrap();
                connection
                    .execute("UPDATE events SET sequence=1001 WHERE sequence=1", [])
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .pragma_update(None, "foreign_keys", false)
                    .unwrap();
                connection
                    .execute(
                        "UPDATE events SET event_id='event:index-spliced' WHERE sequence=1",
                        [],
                    )
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute(
                        "UPDATE events SET schema='reviewgraphen.review_event.v1' WHERE sequence=1",
                        [],
                    )
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute(
                        "UPDATE events SET event_hash=?1 WHERE sequence=1",
                        [ContentHash::sha256(b"mutated-event").to_string()],
                    )
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute(
                        "UPDATE events SET payload_hash=?1 WHERE sequence=1",
                        [ContentHash::sha256(b"mutated-payload").to_string()],
                    )
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute(
                        "UPDATE events SET payload_kind='artifact_registered' WHERE sequence=1",
                        [],
                    )
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute("UPDATE events SET actor='spliced' WHERE sequence=1", [])
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute("UPDATE events SET logical_time=999 WHERE sequence=1", [])
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute(
                        "UPDATE review_plans SET identity_body_hash=?1",
                        [ContentHash::sha256(b"mutated-plan-identity").to_string()],
                    )
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute(
                        "UPDATE review_plans SET body_hash=?1",
                        [ContentHash::sha256(b"mutated-plan-body").to_string()],
                    )
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute(
                        "UPDATE review_plans SET budget_hash=?1",
                        [ContentHash::sha256(b"mutated-budget").to_string()],
                    )
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute(
                        "UPDATE review_plans SET deferred_canonical_json='[{\"id\":\"obligation:bad\",\"reason\":\"unknown\"}]'",
                        [],
                    )
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute(
                        "UPDATE context_envelopes SET projection_hash=?1",
                        [ContentHash::sha256(b"mutated-context-identity").to_string()],
                    )
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute(
                        "UPDATE context_envelopes SET body_hash=?1",
                        [ContentHash::sha256(b"mutated-context-body").to_string()],
                    )
                    .unwrap();
            }),
            Box::new(|connection| {
                connection
                    .execute(
                        "UPDATE context_envelopes SET losses_canonical_json='[] '",
                        [],
                    )
                    .unwrap();
            }),
        ];
        for (mutation_index, mutation) in mutations.into_iter().enumerate() {
            index.rebuild(&journal, &cas).unwrap();
            mutate_active_image(&index, |connection| mutation(connection));
            let result = index.snapshot_current(&journal);
            assert!(
                matches!(result, Err(IndexError::CorruptIndex)),
                "mutation {mutation_index}: {result:?}"
            );
        }
    }

    #[test]
    fn d1_combined_working_set_exact_peak_plus_one_and_overflow_are_typed() {
        let discovery_workspace = tempfile::tempdir().unwrap();
        let discovery_root =
            StoreRoot::open(discovery_workspace.path(), StoreLimits::default()).unwrap();
        let (discovery_journal, discovery_cas, discovery_index) = d1_index_fixture(&discovery_root);
        discovery_index
            .rebuild(&discovery_journal, &discovery_cas)
            .unwrap();
        let image = discovery_index.read_active_image().unwrap();
        let image_limit = u64::try_from(image.len()).unwrap();
        let connection = deserialize_read_only(image, discovery_index.limits()).unwrap();
        let marker = index_marker_from_connection(&connection, discovery_index.limits()).unwrap();
        let query_limit =
            preflight_query_budget(&connection, &marker, discovery_index.limits()).unwrap();
        let main_bytes = u64::try_from(
            connection
                .pragma_query_value::<i64, _>(None, "page_count", |row| row.get(0))
                .unwrap(),
        )
        .unwrap()
            * PAGE_SIZE;
        let compare_tuple = event_compare_tuple_bytes(&connection).unwrap();
        drop(connection);
        let mut reader = discovery_journal
            .index_reader(StoreLimits::default().max_index_working_bytes)
            .unwrap();
        let identity = reader.identity().clone();
        let retained =
            streaming_journal_retained(&reader, &identity, discovery_index.limits()).unwrap();
        let certificate = reader
            .with_locked_prefix::<IndexError>(|_, _| Ok(()))
            .unwrap();
        let scan_peak =
            retained + certificate.line_buffer_capacity + certificate.canonical_scratch_capacity;
        let genesis_tuple_bound = identity
            .v2_genesis_backing()
            .map_or(0, |bytes| u64::try_from(bytes.len()).unwrap());
        let insert_peak = scan_peak + certificate.line_buffer_capacity.max(genesis_tuple_bound) * 2;
        drop(reader);
        let deserialize_peak = retained + image_limit * 2 + 1024;
        let query_peak = scan_peak + main_bytes + 1024 + query_limit + compare_tuple;
        let cache_admission_peak = insert_peak + image_limit * 3 + 1024;
        let exact_working = scan_peak
            .max(insert_peak)
            .max(deserialize_peak)
            .max(query_peak)
            .max(cache_admission_peak);
        let exact_workspace = tempfile::tempdir().unwrap();
        let exact_root = StoreRoot::open(
            exact_workspace.path(),
            StoreLimits {
                max_index_serialized_bytes: image_limit,
                max_index_query_bytes: image_limit,
                max_index_working_bytes: exact_working,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let (exact_journal, exact_cas, exact_index) = d1_index_fixture(&exact_root);
        exact_index.rebuild(&exact_journal, &exact_cas).unwrap();
        assert_eq!(
            exact_index
                .snapshot_current(&exact_journal)
                .unwrap()
                .review_plans
                .len(),
            1
        );

        let lower_workspace = tempfile::tempdir().unwrap();
        let lower_root = StoreRoot::open(
            lower_workspace.path(),
            StoreLimits {
                max_index_serialized_bytes: image_limit,
                max_index_query_bytes: image_limit,
                max_index_working_bytes: exact_working - 1,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let (lower_journal, lower_cas, lower_index) = d1_index_fixture(&lower_root);
        let lower_result = lower_index.rebuild(&lower_journal, &lower_cas);
        let lower_debug = format!("{lower_result:?}");
        assert!(
            matches!(
                lower_result,
                Err(IndexError::Incomplete {
                    limit,
                    observed
                }) if limit == exact_working - 1 && observed == exact_working
            ),
            "{lower_debug}"
        );

        let limits = IndexLimits::try_from(StoreLimits::default()).unwrap();
        assert!(matches!(
            query_cache_reservation_with_journal(limits, u64::MAX),
            Err(IndexError::Incomplete {
                observed: u64::MAX,
                ..
            })
        ));
    }

    #[test]
    fn d2_nonempty_query_and_store_visible_working_peaks_are_exact_plus_one() {
        let discovery_workspace = tempfile::tempdir().unwrap();
        let discovery_root =
            StoreRoot::open(discovery_workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index) = d2_index_fixture(&discovery_root);
        index.rebuild(&journal, &cas).unwrap();
        let image = index.read_active_image().unwrap();
        let image_limit = u64::try_from(image.len()).unwrap();
        let connection = deserialize_read_only(image, index.limits()).unwrap();
        let marker = index_marker_from_connection(&connection, index.limits()).unwrap();
        let query_limit = preflight_query_budget(&connection, &marker, index.limits()).unwrap();
        assert!(query_limit > 1);
        let too_small_query = IndexLimits {
            max_query_bytes: query_limit - 1,
            ..index.limits()
        };
        assert!(matches!(
            snapshot_from_connection_with_journal(&connection, too_small_query, 0, 0, 0, None),
            Err(IndexError::Incomplete { limit, observed })
                if limit == query_limit - 1 && observed == query_limit
        ));
        let main_bytes = u64::try_from(
            connection
                .pragma_query_value::<i64, _>(None, "page_count", |row| row.get(0))
                .unwrap(),
        )
        .unwrap()
            * PAGE_SIZE;
        let compare_tuple = event_compare_tuple_bytes(&connection).unwrap();
        drop(connection);
        let mut reader = journal
            .index_reader(StoreLimits::default().max_index_working_bytes)
            .unwrap();
        let identity = reader.identity().clone();
        let retained = streaming_journal_retained(&reader, &identity, index.limits()).unwrap();
        let certificate = reader
            .with_locked_prefix::<IndexError>(|_, _| Ok(()))
            .unwrap();
        let scan_peak =
            retained + certificate.line_buffer_capacity + certificate.canonical_scratch_capacity;
        let genesis_tuple_bound = identity
            .v2_genesis_backing()
            .map_or(0, |bytes| u64::try_from(bytes.len()).unwrap());
        let insert_peak = scan_peak + certificate.line_buffer_capacity.max(genesis_tuple_bound) * 2;
        let deserialize_peak = retained + image_limit * 2 + 1024;
        let query_peak = scan_peak + main_bytes + 1024 + query_limit + compare_tuple;
        let cache_admission_peak = insert_peak + image_limit * 3 + 1024;
        let exact_working = scan_peak
            .max(insert_peak)
            .max(deserialize_peak)
            .max(query_peak)
            .max(cache_admission_peak);
        drop(reader);

        let exact_workspace = tempfile::tempdir().unwrap();
        let exact_root = StoreRoot::open(
            exact_workspace.path(),
            StoreLimits {
                max_index_serialized_bytes: image_limit,
                max_index_query_bytes: query_limit,
                max_index_working_bytes: exact_working,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let (exact_journal, exact_cas, exact_index) = d2_index_fixture(&exact_root);
        exact_index.rebuild(&exact_journal, &exact_cas).unwrap();
        assert_eq!(
            exact_index
                .snapshot_current(&exact_journal)
                .unwrap()
                .executions
                .len(),
            1
        );

        let low_workspace = tempfile::tempdir().unwrap();
        let low_root = StoreRoot::open(
            low_workspace.path(),
            StoreLimits {
                max_index_serialized_bytes: image_limit,
                max_index_query_bytes: query_limit,
                max_index_working_bytes: exact_working - 1,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let (low_journal, low_cas, low_index) = d2_index_fixture(&low_root);
        assert!(matches!(
            low_index.rebuild(&low_journal, &low_cas),
            Err(IndexError::Incomplete { limit, observed })
                if limit == exact_working - 1 && observed == exact_working
        ));
    }

    #[test]
    fn candidate_reread_peak_is_isolated_exact_and_one_byte_low() {
        // G, J_meta, candidate image, reconstructed main, query cache,
        // snapshot reservation, and T_compare are all independently nonzero.
        let components = [101, 103, 4096, 4096, 1024, 307, 71];
        let exact = components.into_iter().sum();
        assert_eq!(
            admit_candidate_reread_peak(components, exact).unwrap(),
            exact
        );
        assert!(matches!(
            admit_candidate_reread_peak(components, exact - 1),
            Err(IndexError::Incomplete { limit, observed })
                if limit == exact - 1 && observed == exact
        ));
        assert!(matches!(
            admit_candidate_reread_peak([u64::MAX, 1, 0, 0, 0, 0, 0], u64::MAX),
            Err(IndexError::Incomplete {
                limit: u64::MAX,
                observed: u64::MAX,
            })
        ));
    }

    #[test]
    fn context_losses_preserve_multiple_full_groups_and_reject_bad_properties() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let mut giant = b"controller line\n".repeat(100);
        giant.extend(std::iter::repeat_n(b'x', 300_000));
        let sources = BTreeMap::from([
            ("src/checkout_controller.rs", giant),
            (
                "src/payment_repository.rs",
                b"repository line\n".repeat(80_000),
            ),
            (
                "src/auxiliary_repository.rs",
                b"auxiliary line\n".repeat(80_000),
            ),
        ]);
        let (journal, cas, index) =
            d1_index_fixture_with_sources(&root, "run:index-d1-losses", sources);
        index.rebuild(&journal, &cas).unwrap();
        let snapshot = index.snapshot_current(&journal).unwrap();
        let row = snapshot.context_envelopes.first().unwrap();
        let losses: Vec<reviewgraphen_core::EnvelopeLoss> =
            serde_json::from_str(&row.losses_canonical_json).unwrap();
        assert!(losses.len() >= 2);
        assert!(
            losses
                .windows(2)
                .all(|pair| pair[0].description() != pair[1].description())
        );
        assert!(losses.iter().any(|loss| {
            loss.description() == "context_loss:artifact_bytes_cap" && loss.source_ids().len() >= 2
        }));
        let raw_losses = exact_json_value(&row.losses_canonical_json).unwrap();
        assert!(raw_losses.as_array().unwrap().iter().all(|loss| {
            loss.as_object()
                .is_some_and(|object| !object.contains_key("id") && object.len() == 4)
        }));

        for replacement in [serde_json::json!([]), serde_json::json!(["p:a", "p:b"])] {
            index.rebuild(&journal, &cas).unwrap();
            mutate_active_image(&index, |connection| {
                let mut losses: Value = connection
                    .query_row(
                        "SELECT losses_canonical_json FROM context_envelopes",
                        [],
                        |row| row.get::<_, String>(0),
                    )
                    .map(|text| serde_json::from_str(&text).unwrap())
                    .unwrap();
                losses.as_array_mut().unwrap()[0]["affected_properties"] = replacement;
                let canonical = String::from_utf8(canonical_json(&losses).unwrap()).unwrap();
                connection
                    .execute(
                        "UPDATE context_envelopes SET losses_canonical_json=?1",
                        [canonical],
                    )
                    .unwrap();
            });
            assert!(matches!(
                index.snapshot_current(&journal),
                Err(IndexError::CorruptIndex)
            ));
        }
    }

    #[test]
    fn schema_v3_ddl_checks_duplicates_closed_values_and_event_foreign_keys() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index) = d1_index_fixture(&root);
        index.rebuild(&journal, &cas).unwrap();
        let image = index.read_active_image().unwrap();
        let length = image.len();
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .deserialize_read_exact(MAIN_DB, Cursor::new(image), length, false)
            .unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        for sql in [
            "UPDATE review_plans SET planner_policy_version='scheduler.other@1'",
            "UPDATE context_envelopes SET context_policy_version='context.other@1'",
            "UPDATE events SET payload_kind='unknown_payload' WHERE sequence=1",
        ] {
            assert!(matches!(
                connection.execute(sql, []).map_err(map_sql),
                Err(IndexError::ProjectionContractViolation)
            ));
        }
        assert!(matches!(
            connection
                .execute(
                    "INSERT INTO review_plans SELECT * FROM review_plans LIMIT 1",
                    [],
                )
                .map_err(map_sql),
            Err(IndexError::DuplicateIndexKey)
        ));
        assert!(matches!(
            connection
                .execute(
                    "INSERT INTO obligation_lifecycle(event_sequence,event_id,obligation_id,next_lifecycle) SELECT 999,'event:missing',obligation_id,'generated' FROM obligations LIMIT 1",
                    [],
                )
                .map_err(map_sql),
            Err(IndexError::ProjectionContractViolation)
        ));
    }

    #[test]
    fn d1_aggregate_row_limit_accepts_exact_and_refuses_the_next_row_atomically() {
        let discovery_workspace = tempfile::tempdir().unwrap();
        let discovery_root =
            StoreRoot::open(discovery_workspace.path(), StoreLimits::default()).unwrap();
        let (discovery_journal, discovery_cas, discovery_index) = d1_index_fixture(&discovery_root);
        discovery_index
            .rebuild(&discovery_journal, &discovery_cas)
            .unwrap();
        let connection = deserialize_read_only(
            discovery_index.read_active_image().unwrap(),
            discovery_index.limits(),
        )
        .unwrap();
        let tables = [
            "index_meta",
            "events",
            "program_objects",
            "program_relations",
            "universe",
            "obligations",
            "obligation_lifecycle",
            "claims",
            "artifact_registrations",
            "snapshot_source_index",
            "review_plans",
            "context_envelopes",
            "unreconciled_authority_records",
            "projected_findings",
        ];
        let exact_rows = tables.iter().fold(0_u64, |total, table| {
            let count: i64 = connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            total + u64::try_from(count).unwrap()
        });
        assert!(exact_rows > 1);

        let exact_workspace = tempfile::tempdir().unwrap();
        let exact_root = StoreRoot::open(
            exact_workspace.path(),
            StoreLimits {
                max_index_rows: exact_rows,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let (exact_journal, exact_cas, exact_index) = d1_index_fixture(&exact_root);
        exact_index.rebuild(&exact_journal, &exact_cas).unwrap();

        let lower_workspace = tempfile::tempdir().unwrap();
        let lower_root = StoreRoot::open(
            lower_workspace.path(),
            StoreLimits {
                max_index_rows: exact_rows - 1,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let (lower_journal, lower_cas, lower_index) = d1_index_fixture(&lower_root);
        assert!(matches!(
            lower_index.rebuild(&lower_journal, &lower_cas),
            Err(IndexError::Incomplete {
                limit,
                observed
            }) if limit == exact_rows - 1 && observed == exact_rows
        ));
        assert!(matches!(
            lower_index.read_active_image(),
            Err(IndexError::Missing)
        ));
    }

    #[test]
    fn v2_rebuild_is_complete_deterministic_and_tail_stale_is_refused() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index, transition) = v2_fixture(&root);
        let first = index.rebuild(&journal, &cas).unwrap();
        let snapshot = index.snapshot_current(&journal).unwrap();
        assert_eq!(snapshot.marker.event_count, 1);
        assert!(!snapshot.program_objects.is_empty());
        assert!(snapshot.universe.is_some());
        assert_eq!(snapshot.artifact_registrations.len(), 1);
        std::fs::remove_file(root.path().join(INDEX_DIR).join(ACTIVE_FILE)).unwrap();
        let second = index.rebuild(&journal, &cas).unwrap();
        assert_eq!(first.event_count, second.event_count);
        assert_eq!(snapshot, index.snapshot_current(&journal).unwrap());
        journal.writer().unwrap().append(transition).unwrap();
        let stale = index.snapshot_current(&journal);
        assert!(
            matches!(stale, Err(IndexError::CommittedIndexStale { .. })),
            "unexpected stale result: {stale:?}"
        );
    }

    fn assert_no_sqlite_sidecars(directory: &Path) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                assert_no_sqlite_sidecars(&path);
            } else {
                let name = path.file_name().unwrap().to_string_lossy();
                assert!(
                    !name.ends_with("-journal")
                        && !name.ends_with("-wal")
                        && !name.ends_with("-shm"),
                    "unexpected SQLite sidecar: {}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn rebuild_and_query_create_no_sqlite_sidecar_files() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index, _) = v2_fixture(&root);
        index.rebuild(&journal, &cas).unwrap();
        index.snapshot_current(&journal).unwrap();
        assert_no_sqlite_sidecars(workspace.path());
    }

    #[test]
    fn v2_rebuild_refuses_missing_genesis_cas_without_replacing_active_image() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let program = ProgramSpace::from_json_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let run_id = StableId::parse("run:index-missing-cas").unwrap();
        let log = EventLog::new(
            run_id.clone(),
            ReviewAggregate::new(program, universe, obligations).unwrap(),
        )
        .unwrap();
        let bytes = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let identity =
            JournalIdentity::new(run_id, super::super::JournalGenesis::V2(bytes)).unwrap();
        let journal =
            EventJournal::initialize_v2(&root, identity, log.envelopes().next().unwrap().clone())
                .unwrap();
        let cas = CasStore::open(&root).unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        assert!(matches!(
            index.rebuild(&journal, &cas),
            Err(IndexError::Store(StoreError::MissingArtifact))
        ));
        assert!(matches!(
            index.read_active_image(),
            Err(IndexError::Missing)
        ));
    }

    #[test]
    fn v2_rebuild_retains_an_authority_event_only_as_a_shadow() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let program = ProgramSpace::from_json_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let run_id = StableId::parse("run:index-all-payloads").unwrap();
        let mut log = EventLog::new(
            run_id.clone(),
            ReviewAggregate::new(program, universe, obligations).unwrap(),
        )
        .unwrap();
        let obligation = log.aggregate().obligations().next().unwrap().id().clone();
        let source_id = log
            .aggregate()
            .program()
            .known_ids()
            .iter()
            .next()
            .unwrap()
            .clone();
        log.append(EventCommand::obligation_transition(
            obligation.clone(),
            ObligationLifecycle::Planned,
        ))
        .unwrap();
        let observed = log.aggregate().program().clone();
        let evidence = Evidence::new(
            StableId::parse("evidence:index-all-payloads").unwrap(),
            "static_fact",
            BTreeSet::from([source_id.clone()]),
            EvidenceDetails::new(None, None, BTreeMap::new()),
            Provenance::accepted_deterministic(
                SourceRef::new("tool", "index-fixture@1", Some("1".to_owned()), None, None)
                    .unwrap(),
                "index.fixture.v1",
                Some("1".to_owned()),
                Some(1.0),
            )
            .unwrap(),
            observed.evidence_snapshot_admission(),
        )
        .unwrap();
        let admission = log
            .admit_evidence(&observed.evidence_snapshot_admission(), &evidence)
            .unwrap();
        log.append(EventCommand::evidence_recorded(evidence, admission))
            .unwrap();
        let genesis = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let cas = CasStore::open(&root).unwrap();
        let hash = CasHash::parse(ContentHash::sha256(&genesis).to_string()).unwrap();
        cas.put(
            &hash,
            Some(u64::try_from(genesis.len()).unwrap()),
            Cursor::new(genesis.as_slice()),
        )
        .unwrap();
        let identity =
            JournalIdentity::new(run_id, super::super::JournalGenesis::V2(genesis)).unwrap();
        let events = log.envelopes().cloned().collect::<Vec<_>>();
        let journal = EventJournal::initialize_v2(&root, identity, events[0].clone()).unwrap();
        let mut writer = journal.writer().unwrap();
        for event in events.into_iter().skip(1) {
            writer.append(event).unwrap();
        }
        // `rebuild` obtains the index lock before a journal reader, so this
        // exclusive writer guard must be released before invoking it.
        drop(writer);
        let index = DerivedIndex::open(&root).unwrap();
        let receipt = index.rebuild(&journal, &cas).unwrap();
        let snapshot = index.snapshot_current(&journal).unwrap();
        assert_eq!(receipt.event_count, 3);
        assert_eq!(snapshot.obligation_lifecycle.len(), 1);
        assert_eq!(snapshot.shadows.len(), 1);
        assert!(snapshot.claims.is_empty());
        assert!(snapshot.findings.is_empty());
        assert!(snapshot.shadows.iter().all(|row| !row.authority_reconciled));
    }

    #[test]
    fn limits_reject_incoherent_memory_and_page_bounds() {
        let mut limits = StoreLimits {
            max_index_serialized_bytes: PAGE_SIZE - 1,
            ..StoreLimits::default()
        };
        assert!(matches!(
            IndexLimits::try_from(limits),
            Err(IndexError::InvalidLimits)
        ));
        limits = StoreLimits::default();
        limits.max_index_query_bytes = limits.max_index_working_bytes + 1;
        assert!(matches!(
            IndexLimits::try_from(limits),
            Err(IndexError::InvalidLimits)
        ));
        limits = StoreLimits::default();
        limits.max_index_working_bytes = limits.max_index_serialized_bytes * 3;
        assert!(matches!(
            IndexLimits::try_from(limits),
            Err(IndexError::InvalidLimits)
        ));
    }

    #[test]
    fn exact_serialize_peak_has_a_nonzero_cache_and_is_admissible() {
        let limits = IndexLimits {
            max_rows: 1,
            max_serialized_bytes: PAGE_SIZE,
            max_working_bytes: PAGE_SIZE * 3 + 1024,
            max_query_bytes: 1,
            max_statement_bytes: 8,
        };
        limits.validate().unwrap();
        let cache =
            configured_cache_bytes(limits, build_cache_reservation(limits).unwrap()).unwrap();
        assert_eq!(cache, 1024);
        assert_eq!(PAGE_SIZE * 3 + cache, limits.max_working_bytes);
    }

    #[test]
    fn actual_image_and_query_limits_accept_exactly_then_refuse_one_page_or_byte_less() {
        let discovery_workspace = tempfile::tempdir().unwrap();
        let discovery_root =
            StoreRoot::open(discovery_workspace.path(), StoreLimits::default()).unwrap();
        let discovery_index = DerivedIndex::open(&discovery_root).unwrap();
        let discovery_connection = discovery_index.new_in_memory_connection().unwrap();
        insert_v1_metadata(&discovery_connection);
        let discovery_snapshot = snapshot_from_connection(
            &discovery_connection,
            discovery_index.limits(),
            build_cache_reservation(discovery_index.limits()).unwrap(),
            None,
        )
        .unwrap();
        let serialized =
            serialize_connection(&discovery_connection, discovery_index.limits()).unwrap();
        let image_limit = u64::try_from(serialized.len()).unwrap();
        let query_limit = preflight_query_budget(
            &discovery_connection,
            &discovery_snapshot.marker,
            discovery_index.limits(),
        )
        .unwrap();
        assert!(image_limit > PAGE_SIZE);
        assert!(query_limit > 1);

        let exact_workspace = tempfile::tempdir().unwrap();
        let exact_root = StoreRoot::open(
            exact_workspace.path(),
            StoreLimits {
                max_index_serialized_bytes: image_limit,
                max_index_working_bytes: image_limit * 3 + 1024,
                max_index_query_bytes: query_limit,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let exact_index = DerivedIndex::open(&exact_root).unwrap();
        let exact_connection = exact_index.new_in_memory_connection().unwrap();
        insert_v1_metadata(&exact_connection);
        let exact_image = serialize_connection(&exact_connection, exact_index.limits()).unwrap();
        assert_eq!(u64::try_from(exact_image.len()).unwrap(), image_limit);
        let run = StableId::parse("run:index-exact-bound").unwrap();
        let tail = ContentHash::sha256(b"exact-bound-tail");
        exact_index.publish_image(exact_image, &run, &tail).unwrap();
        let query_connection = deserialize_read_only(
            exact_index.read_active_image().unwrap(),
            exact_index.limits(),
        )
        .unwrap();
        let exact_snapshot = snapshot_from_connection(
            &query_connection,
            exact_index.limits(),
            query_cache_reservation(exact_index.limits()).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(exact_snapshot.marker.event_count, 1);

        let too_small_image = IndexLimits {
            max_rows: exact_index.limits().max_rows,
            max_serialized_bytes: image_limit - PAGE_SIZE,
            max_working_bytes: (image_limit - PAGE_SIZE) * 3 + 1024,
            max_query_bytes: query_limit,
            max_statement_bytes: exact_index.limits().max_statement_bytes,
        };
        assert!(matches!(
            serialize_connection(&exact_connection, too_small_image),
            Err(IndexError::Incomplete { .. })
        ));
        let too_small_query = IndexLimits {
            max_query_bytes: query_limit - 1,
            ..exact_index.limits()
        };
        assert!(matches!(
            snapshot_from_connection(
                &query_connection,
                too_small_query,
                query_cache_reservation(too_small_query).unwrap(),
                None,
            ),
            Err(IndexError::Incomplete {
                limit,
                observed
            }) if limit == query_limit - 1 && observed == query_limit
        ));
    }

    #[test]
    fn exact_statement_and_row_limits_accept_then_plus_one_refuses() {
        let limits = IndexLimits {
            max_rows: 1,
            max_serialized_bytes: PAGE_SIZE,
            max_working_bytes: PAGE_SIZE,
            max_query_bytes: 1,
            max_statement_bytes: 8,
        };
        let connection = Connection::open_in_memory().unwrap();
        checked_batch(&connection, "SELECT 1", limits).unwrap();
        assert!(matches!(
            checked_batch(&connection, "SELECT 12", limits),
            Err(IndexError::Incomplete {
                limit: 8,
                observed: 9
            })
        ));
        let mut rows = 0;
        reserve_row(&mut rows, limits).unwrap();
        assert!(matches!(
            reserve_row(&mut rows, limits),
            Err(IndexError::Incomplete {
                limit: 1,
                observed: 2
            })
        ));
    }

    #[test]
    fn in_memory_schema_has_exact_user_version_and_serializes() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let connection = index.new_in_memory_connection().unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
        let events_ddl: String = connection
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type='table' AND name='events'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(events_ddl.contains("'review_plan_recorded'"));
        assert!(events_ddl.contains("'context_envelope_projected'"));
        assert!(events_ddl.contains("review_execution_recorded"));
        let table_columns = |table: &str| {
            let mut statement = connection
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        assert_eq!(
            table_columns("review_plans"),
            [
                "event_sequence",
                "event_id",
                "plan_id",
                "universe_id",
                "snapshot_id",
                "planner_input_hash",
                "planner_policy_version",
                "planner_policy_hash",
                "budget_canonical_json",
                "budget_hash",
                "risk_breakdown_canonical_json",
                "waves_canonical_json",
                "deferred_canonical_json",
                "identity_body_hash",
                "body_hash",
            ]
        );
        assert_eq!(
            table_columns("context_envelopes"),
            [
                "event_sequence",
                "event_id",
                "envelope_id",
                "snapshot_id",
                "context_policy_version",
                "context_policy_hash",
                "candidate_ids_canonical_json",
                "obligation_ids_canonical_json",
                "context_policy_canonical_json",
                "included_sources_canonical_json",
                "excluded_sources_canonical_json",
                "unknowns_canonical_json",
                "assumptions_canonical_json",
                "losses_canonical_json",
                "projection_hash",
                "body_hash",
            ]
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='executions'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        let image = serialize_connection(&connection, index.limits()).unwrap();
        assert!(!image.is_empty());
        assert_eq!(image.len() % PAGE_SIZE as usize, 0);
        let readonly = deserialize_read_only(image, index.limits()).unwrap();
        let reopened: i64 = readonly
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(reopened, 3);
        assert!(
            readonly
                .execute_batch("CREATE TABLE forbidden (id INTEGER)")
                .is_err()
        );
        assert_eq!(
            readonly
                .pragma_query_value(None, "temp_store", |row| row.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            readonly
                .pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            readonly
                .pragma_query_value(None, "trusted_schema", |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        for (kind, expected) in [
            (
                Limit::SQLITE_LIMIT_LENGTH,
                i32::try_from(index.limits().max_serialized_bytes).unwrap(),
            ),
            (
                Limit::SQLITE_LIMIT_SQL_LENGTH,
                i32::try_from(index.limits().max_statement_bytes).unwrap(),
            ),
            (Limit::SQLITE_LIMIT_COLUMN, 64),
            (Limit::SQLITE_LIMIT_COMPOUND_SELECT, 32),
            (Limit::SQLITE_LIMIT_EXPR_DEPTH, 128),
            (Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 256),
        ] {
            assert_eq!(readonly.limit(kind).unwrap(), expected);
        }
    }

    #[test]
    fn index_directory_lock_and_active_reject_symlink_or_loose_mode() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join(INDEX_DIR)).unwrap();
        assert!(DerivedIndex::open(&root).is_err());
        std::fs::remove_file(root.path().join(INDEX_DIR)).unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        std::fs::set_permissions(
            root.path().join(INDEX_DIR).join(LOCK_FILE),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(matches!(index.lock_shared(), Err(IndexError::CorruptIndex)));
    }

    #[test]
    fn index_entry_type_mode_matrix_is_fail_closed_and_fifo_nonblocking() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let active = root.path().join(INDEX_DIR).join(ACTIVE_FILE);
        std::fs::create_dir(&active).unwrap();
        assert!(matches!(
            index.read_active_image(),
            Err(IndexError::CorruptIndex)
        ));
        std::fs::remove_dir(&active).unwrap();
        let fifo =
            root.path()
                .join(INDEX_DIR)
                .join(format!("candidate-{}{}.sqlite", "f", "0".repeat(63)));
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::set_permissions(&fifo, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            index.audit_candidates(false),
            Err(IndexError::CorruptIndex)
        ));
        // Device-node construction requires privilege and is intentionally not
        // attempted; `verify_index_stat` rejects every non-RegularFile type.
    }

    #[test]
    fn split_lock_replacement_is_an_inode_race_not_a_success() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let guard = index.lock_shared().unwrap();
        let lock = root.path().join(INDEX_DIR).join(LOCK_FILE);
        let displaced = root.path().join(INDEX_DIR).join("displaced-index.lock");
        std::fs::rename(&lock, &displaced).unwrap();
        std::fs::write(&lock, b"").unwrap();
        std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            guard.verify_unchanged(),
            Err(IndexError::IndexPathRace)
        ));
    }

    #[test]
    fn every_audited_lock_acquisition_seam_detects_entry_replacement() {
        for fault in [
            LockFault::PreStat,
            LockFault::OpenBeforeFlock,
            LockFault::FlockBeforePostStat,
        ] {
            let workspace = tempfile::tempdir().unwrap();
            let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
            let index = DerivedIndex::open(&root).unwrap();
            index.inject_lock_fault(fault);
            assert!(matches!(
                index.lock_shared(),
                Err(IndexError::IndexPathRace)
            ));
        }
    }

    #[test]
    fn three_actor_lock_order_serializes_without_deadlock() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, _cas, index, _) = v2_fixture(&root);
        let primary_index = index.lock_shared().unwrap();
        let primary_journal = journal.reader().unwrap();
        std::thread::scope(|scope| {
            let index_ref = &index;
            let journal_ref = &journal;
            let (second_ready_tx, second_ready_rx) = mpsc::channel();
            let (second_release_tx, second_release_rx) = mpsc::channel();
            let second = scope.spawn(move || {
                let _index = index_ref.lock_shared().unwrap();
                let _journal = journal_ref.reader().unwrap();
                second_ready_tx.send(()).unwrap();
                second_release_rx.recv().unwrap();
            });
            second_ready_rx.recv().unwrap();
            let (rebuild_try_tx, rebuild_try_rx) = mpsc::channel();
            let (rebuild_acquired_tx, rebuild_acquired_rx) = mpsc::channel();
            let rebuild = scope.spawn(move || {
                rebuild_try_tx.send(()).unwrap();
                let _exclusive = index_ref.lock_exclusive().unwrap();
                rebuild_acquired_tx.send(()).unwrap();
            });
            let (writer_try_tx, writer_try_rx) = mpsc::channel();
            let (writer_acquired_tx, writer_acquired_rx) = mpsc::channel();
            let writer = scope.spawn(move || {
                writer_try_tx.send(()).unwrap();
                let _writer = journal_ref.writer().unwrap();
                writer_acquired_tx.send(()).unwrap();
            });
            rebuild_try_rx.recv().unwrap();
            writer_try_rx.recv().unwrap();
            assert!(
                rebuild_acquired_rx
                    .recv_timeout(Duration::from_millis(20))
                    .is_err()
            );
            assert!(
                writer_acquired_rx
                    .recv_timeout(Duration::from_millis(20))
                    .is_err()
            );
            second_release_tx.send(()).unwrap();
            second.join().unwrap();
            drop(primary_journal);
            writer_acquired_rx.recv().unwrap();
            drop(primary_index);
            rebuild_acquired_rx.recv().unwrap();
            writer.join().unwrap();
            rebuild.join().unwrap();
        });
    }

    #[test]
    fn snapshot_marker_pause_keeps_the_journal_tail_seam_locked() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index, _) = v2_fixture(&root);
        index.rebuild(&journal, &cas).unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        index.pause_after_snapshot_marker_check(entered_tx, release_rx);
        std::thread::scope(|scope| {
            let index_ref = &index;
            let journal_ref = &journal;
            let snapshot = scope.spawn(move || index_ref.snapshot_current(journal_ref));
            entered_rx.recv().unwrap();
            let (writer_try_tx, writer_try_rx) = mpsc::channel();
            let (writer_acquired_tx, writer_acquired_rx) = mpsc::channel();
            let writer = scope.spawn(move || {
                writer_try_tx.send(()).unwrap();
                let _writer = journal_ref.writer().unwrap();
                writer_acquired_tx.send(()).unwrap();
            });
            writer_try_rx.recv().unwrap();
            assert!(
                writer_acquired_rx
                    .recv_timeout(Duration::from_millis(20))
                    .is_err()
            );
            release_tx.send(()).unwrap();
            snapshot.join().unwrap().unwrap();
            writer_acquired_rx.recv().unwrap();
            writer.join().unwrap();
        });
    }

    #[test]
    fn bounded_active_read_and_atomic_publication() {
        let workspace = tempfile::tempdir().unwrap();
        let limits = StoreLimits {
            max_index_serialized_bytes: PAGE_SIZE,
            ..StoreLimits::default()
        };
        let root = StoreRoot::open(workspace.path(), limits).unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let run = StableId::parse("run:demo").unwrap();
        let tail = ContentHash::sha256(b"tail");
        let image = vec![0_u8; PAGE_SIZE as usize];
        assert!(matches!(
            index.publish_image(image, &run, &tail),
            Err(IndexError::CorruptIndex)
        ));
        std::fs::write(
            root.path().join(INDEX_DIR).join(ACTIVE_FILE),
            vec![0_u8; PAGE_SIZE as usize + 1],
        )
        .unwrap();
        std::fs::set_permissions(
            root.path().join(INDEX_DIR).join(ACTIVE_FILE),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert!(matches!(
            index.read_active_image(),
            Err(IndexError::Incomplete {
                limit: PAGE_SIZE,
                observed
            }) if observed == PAGE_SIZE + 1
        ));
    }

    #[test]
    fn candidate_marker_tuple_is_checked_before_projected_row_validation() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let connection = index.new_in_memory_connection().unwrap();
        insert_v1_metadata(&connection);
        let expected = index_marker_from_connection(&connection, index.limits()).unwrap();
        connection
            .execute(
                "UPDATE index_meta SET run_id='run:wrong-candidate-marker'",
                [],
            )
            .unwrap();
        connection.execute("DROP TABLE events", []).unwrap();
        let image = serialize_connection(&connection, index.limits()).unwrap();
        let lock = index.lock_exclusive().unwrap();
        index.inject_publish_fault(PublishFault::AfterValidation);
        assert!(matches!(
            index.publish_image_locked(image, &expected, None, 0),
            Err(IndexError::CorruptIndex)
        ));
        assert!(index.take_publish_fault(PublishFault::AfterValidation));
        lock.verify_unchanged().unwrap();
    }

    #[test]
    fn publication_failpoints_preserve_old_before_rename_and_report_uncertain_after() {
        let run = StableId::parse("run:index-publish-failpoint").unwrap();
        let tail = ContentHash::sha256(b"tail");
        for fault in [
            PublishFault::AfterWrite,
            PublishFault::BeforeCandidateSync,
            PublishFault::AfterCandidateLinkBeforeDirectorySync,
            PublishFault::AfterCandidateSync,
            PublishFault::AfterImageDropBeforeCandidateRead,
            PublishFault::AfterValidation,
            PublishFault::AfterRename,
            PublishFault::AfterActiveVerify,
        ] {
            let before_rename = matches!(
                fault,
                PublishFault::AfterWrite
                    | PublishFault::BeforeCandidateSync
                    | PublishFault::AfterCandidateLinkBeforeDirectorySync
                    | PublishFault::AfterCandidateSync
                    | PublishFault::AfterImageDropBeforeCandidateRead
                    | PublishFault::AfterValidation
            );
            let workspace = tempfile::tempdir().unwrap();
            let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
            let index = DerivedIndex::open(&root).unwrap();
            let connection = index.new_in_memory_connection().unwrap();
            insert_v1_metadata(&connection);
            let old = serialize_connection(&connection, index.limits()).unwrap();
            connection
                .execute("UPDATE events SET actor='replacement'", [])
                .unwrap();
            let replacement = serialize_connection(&connection, index.limits()).unwrap();
            assert_ne!(old, replacement);
            index.publish_image(old.clone(), &run, &tail).unwrap();
            index.inject_publish_fault(fault);
            let result = index.publish_image(replacement.clone(), &run, &tail);
            if before_rename {
                assert!(matches!(result, Err(IndexError::Io(_))), "{fault:?}");
                assert_eq!(index.read_active_image().unwrap(), old, "{fault:?}");
            } else {
                assert!(
                    matches!(
                        result,
                        Err(IndexError::PublicationDurabilityUncertain { .. })
                    ),
                    "{fault:?}"
                );
                assert_eq!(index.read_active_image().unwrap(), replacement, "{fault:?}");
            }
            // A fresh descriptor admission classifies the surviving image;
            // none of the injected stages leaves an ambiguous success.
            let reopened = DerivedIndex::open(&root).unwrap();
            let expected = if before_rename { old } else { replacement };
            assert_eq!(reopened.read_active_image().unwrap(), expected, "{fault:?}");
            if fault == PublishFault::AfterCandidateLinkBeforeDirectorySync {
                let candidate_count = std::fs::read_dir(root.path().join(INDEX_DIR))
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|entry| is_candidate_name(&entry.file_name().to_string_lossy()))
                    .count();
                assert_eq!(candidate_count, 1);
                // Query admission audits but does not delete candidates;
                // rebuild-side audit owns the bounded cleanup decision.
                reopened.audit_candidates(false).unwrap();
                reopened.audit_candidates(true).unwrap();
                let remaining = std::fs::read_dir(root.path().join(INDEX_DIR))
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|entry| is_candidate_name(&entry.file_name().to_string_lossy()))
                    .count();
                assert_eq!(remaining, 0);
            }
        }
    }

    #[test]
    fn cargo_manifest_pins_minimal_sqlite_feature_metadata() {
        let manifest = include_str!("../Cargo.toml");
        assert!(manifest.contains("version = \"=0.40.1\""));
        assert!(manifest.contains("default-features = false"));
        assert!(manifest.contains("features = [\"bundled\", \"serialize\", \"limits\"]"));
        let output = std::process::Command::new(env!("CARGO"))
            .args([
                "metadata",
                "--offline",
                "--format-version",
                "1",
                "--manifest-path",
                concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let packages = metadata["packages"].as_array().unwrap();
        let store = packages
            .iter()
            .find(|package| package["name"] == "reviewgraphen-store")
            .unwrap();
        let mut requested = store["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|dependency| dependency["name"] == "rusqlite")
            .unwrap()["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|feature| feature.as_str().unwrap())
            .collect::<Vec<_>>();
        requested.sort_unstable();
        assert_eq!(requested, ["bundled", "limits", "serialize"]);
        let rusqlite = packages
            .iter()
            .find(|package| package["name"] == "rusqlite" && package["version"] == "0.40.1")
            .unwrap();
        assert!(packages.iter().any(|package| {
            package["name"] == "libsqlite3-sys" && package["version"] == "0.38.2"
        }));
        let rusqlite_id = rusqlite["id"].as_str().unwrap();
        let nodes = metadata["resolve"]["nodes"].as_array().unwrap();
        let node = nodes
            .iter()
            .find(|node| node["id"].as_str() == Some(rusqlite_id))
            .unwrap();
        let mut features = node["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|feature| feature.as_str().unwrap())
            .collect::<Vec<_>>();
        features.sort_unstable();
        // `bundled` internally enables `modern_sqlite`; it is not Cargo's
        // default feature. The direct dependency request above is the exact
        // three-feature allow-list, while this checks the resolved closure.
        assert_eq!(
            features,
            ["bundled", "limits", "modern_sqlite", "serialize"]
        );
    }

    #[test]
    fn actual_validated_v1_journal_rebuilds_metadata_only() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let program = ProgramSpace::from_json_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let log = EventLog::new_v1_for_import(
            StableId::parse("run:index-v1-journal").unwrap(),
            aggregate,
        )
        .unwrap();
        let identity = JournalIdentity::new(
            log.run_id().clone(),
            super::super::JournalGenesis::V1(log.genesis_hash().clone()),
        )
        .unwrap();
        let journal = EventJournal::initialize_v1_for_test(&root, identity, &[]).unwrap();
        let cas = CasStore::open(&root).unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let receipt = index.rebuild(&journal, &cas).unwrap();
        assert_eq!(receipt.event_count, 0);
        let snapshot = index.snapshot_current(&journal).unwrap();
        assert_eq!(snapshot.marker.projection_mode, "v1_event_metadata_only");
        assert!(snapshot.events.is_empty());
        assert!(snapshot.obligations.is_empty());
    }

    #[test]
    fn synthetic_stat_owner_and_nonregular_matrix_are_fail_closed() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let mut stat = fs::statat(&index.indexes, LOCK_FILE, AtFlags::SYMLINK_NOFOLLOW).unwrap();
        stat.st_uid = stat.st_uid.saturating_add(1);
        assert!(matches!(
            verify_index_stat(&stat, FileType::RegularFile, 0o600),
            Err(IndexError::CorruptIndex)
        ));
        let regular = fs::statat(&index.indexes, LOCK_FILE, AtFlags::SYMLINK_NOFOLLOW).unwrap();
        assert!(matches!(
            verify_index_stat(&regular, FileType::BlockDevice, 0o600),
            Err(IndexError::CorruptIndex)
        ));
    }

    #[test]
    fn candidate_scan_is_bounded_before_a_query_can_expose_any_bytes() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(
            workspace.path(),
            StoreLimits {
                max_tmp_gc_entries: 1,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let directory = root.path().join(INDEX_DIR);
        for suffix in ['a', 'b'] {
            let path = directory.join(format!("candidate-{}{}.sqlite", suffix, "0".repeat(63)));
            std::fs::write(&path, b"candidate").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(matches!(
            index.read_active_image(),
            Err(IndexError::Incomplete {
                limit: 1,
                observed: 2
            })
        ));
    }

    #[test]
    fn candidate_gc_exact_limits_preserve_query_candidates_and_rebuild_removes_them() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(
            workspace.path(),
            StoreLimits {
                max_tmp_gc_entries: 1,
                max_tmp_gc_scan_bytes: 3,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let candidate =
            root.path()
                .join(INDEX_DIR)
                .join(format!("candidate-{}{}.sqlite", "a", "0".repeat(63)));
        std::fs::write(&candidate, b"abc").unwrap();
        std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o600)).unwrap();
        index.audit_candidates(false).unwrap();
        assert!(candidate.exists());
        index.audit_candidates(true).unwrap();
        assert!(!candidate.exists());
        let first =
            root.path()
                .join(INDEX_DIR)
                .join(format!("candidate-{}{}.sqlite", "b", "0".repeat(63)));
        let second =
            root.path()
                .join(INDEX_DIR)
                .join(format!("candidate-{}{}.sqlite", "c", "0".repeat(63)));
        for path in [&first, &second] {
            std::fs::write(path, b"abc").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(matches!(
            index.audit_candidates(false),
            Err(IndexError::Incomplete {
                limit: 1,
                observed: 2
            })
        ));
        assert!(first.exists() && second.exists());
    }

    fn insert_v1_metadata(connection: &Connection) {
        let run = StableId::parse("run:index-v1-metadata").unwrap();
        let genesis = ContentHash::sha256(b"genesis");
        let tail = ContentHash::sha256(b"tail");
        connection
            .execute(
                "INSERT INTO index_meta(singleton,index_schema_version,projection_contract_version,event_contract_version,projection_mode,run_id,genesis_hash,confirmed_offset,tail_hash,event_count) VALUES(1,3,'reviewgraphen.index_projection.v3','reviewgraphen.review_event.v1','v1_event_metadata_only',?1,?2,0,?3,1)",
                rusqlite::params![run.to_string(), genesis.to_string(), tail.to_string()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO events(sequence,event_id,schema,event_hash,payload_hash,payload_kind,actor,logical_time) VALUES(1,?1,'reviewgraphen.review_event.v1',?2,?3,'finding_recorded','system',1)",
                rusqlite::params![StableId::parse("event:index-v1-metadata").unwrap().to_string(), ContentHash::sha256(b"event").to_string(), ContentHash::sha256(b"payload").to_string()],
            )
            .unwrap();
    }

    fn mutate_active_image(index: &DerivedIndex<'_>, mutate: impl FnOnce(&Connection)) {
        let image = index.read_active_image().unwrap();
        let length = image.len();
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .deserialize_read_exact(MAIN_DB, Cursor::new(image), length, false)
            .unwrap();
        mutate(&connection);
        let bytes = connection.serialize(MAIN_DB).unwrap().to_vec();
        std::fs::write(index.root.path().join(INDEX_DIR).join(ACTIVE_FILE), bytes).unwrap();
        std::fs::set_permissions(
            index.root.path().join(INDEX_DIR).join(ACTIVE_FILE),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }

    #[test]
    fn current_snapshot_maps_external_schema_fk_and_integrity_failures_to_corruption() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let (journal, cas, index, _) = v2_fixture(&root);
        let mutations: [fn(&Connection); 3] = [
            |connection: &Connection| {
                connection.pragma_update(None, "user_version", 4).unwrap();
            },
            |connection: &Connection| {
                connection
                    .pragma_update(None, "foreign_keys", false)
                    .unwrap();
                connection
                    .execute("INSERT INTO obligation_lifecycle(event_sequence,event_id,obligation_id,next_lifecycle) VALUES(999,'event:missing','obligation:missing','generated')", [])
                    .unwrap();
            },
            |connection: &Connection| {
                connection
                    .execute_batch("DROP TABLE context_envelopes")
                    .unwrap();
            },
        ];
        for mutation in mutations {
            index.rebuild(&journal, &cas).unwrap();
            mutate_active_image(&index, mutation);
            assert!(matches!(
                index.snapshot_current(&journal),
                Err(IndexError::CorruptIndex)
            ));
        }
        index.rebuild(&journal, &cas).unwrap();
        let active = root.path().join(INDEX_DIR).join(ACTIVE_FILE);
        let mut bytes = std::fs::read(&active).unwrap();
        bytes[100] ^= 0xff;
        std::fs::write(&active, bytes).unwrap();
        std::fs::set_permissions(&active, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            index.snapshot_current(&journal),
            Err(IndexError::CorruptIndex)
        ));
    }

    #[test]
    fn v1_snapshot_is_strictly_metadata_only() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let connection = index.new_in_memory_connection().unwrap();
        insert_v1_metadata(&connection);
        let snapshot = snapshot_from_connection(
            &connection,
            index.limits(),
            build_cache_reservation(index.limits()).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(snapshot.marker.projection_mode, "v1_event_metadata_only");
        assert_eq!(snapshot.marker.sqlite_user_version, 3);
        assert_eq!(snapshot.events.len(), 1);
        assert!(snapshot.program_objects.is_empty());
        connection
            .execute(
                "INSERT INTO program_objects(object_id,object_kind,body_hash) VALUES('node:forbidden','node',?1)",
                [ContentHash::sha256(b"forbidden").to_string()],
            )
            .unwrap();
        assert!(matches!(
            snapshot_from_connection(
                &connection,
                index.limits(),
                build_cache_reservation(index.limits()).unwrap(),
                None,
            ),
            Err(IndexError::CorruptIndex)
        ));
    }

    #[test]
    fn marker_event_count_mismatch_is_corrupt() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let connection = index.new_in_memory_connection().unwrap();
        insert_v1_metadata(&connection);
        connection
            .execute("UPDATE index_meta SET event_count=2", [])
            .unwrap();
        assert!(matches!(
            snapshot_from_connection(
                &connection,
                index.limits(),
                build_cache_reservation(index.limits()).unwrap(),
                None,
            ),
            Err(IndexError::CorruptIndex)
        ));
    }

    #[test]
    fn readback_budget_is_refused_before_public_rows_are_materialized() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(
            workspace.path(),
            StoreLimits {
                max_index_query_bytes: 1,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let connection = index.new_in_memory_connection().unwrap();
        insert_v1_metadata(&connection);
        assert!(matches!(
            snapshot_from_connection(
                &connection,
                index.limits(),
                build_cache_reservation(index.limits()).unwrap(),
                None,
            ),
            Err(IndexError::Incomplete { limit: 1, .. })
        ));
    }

    #[test]
    fn v2_rejects_noncanonical_target_id_json_and_closed_enum_values() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let index = DerivedIndex::open(&root).unwrap();
        let connection = index.new_in_memory_connection().unwrap();
        let hash = ContentHash::sha256(b"index-v2-corruption").to_string();
        connection.execute("INSERT INTO index_meta(singleton,index_schema_version,projection_contract_version,event_contract_version,projection_mode,run_id,genesis_hash,confirmed_offset,tail_hash,event_count) VALUES(1,3,'reviewgraphen.index_projection.v3','reviewgraphen.review_event.v2','v2_domain','run:index-v2-corruption',?1,1,?1,1)", [&hash]).unwrap();
        connection.execute("INSERT INTO events(sequence,event_id,schema,event_hash,payload_hash,payload_kind,actor,logical_time) VALUES(1,'event:index-v2-corruption','reviewgraphen.review_event.v2',?1,?1,'run_genesis_manifest','system',1)", [&hash]).unwrap();
        connection.execute("INSERT INTO program_objects(object_id,object_kind,body_hash) VALUES('node:index-v2-corruption','node',?1)", [&hash]).unwrap();
        connection.execute("INSERT INTO universe(singleton,universe_id,snapshot_id,profile_id,rule_set_hash,extractor_set_hash,policy_version,rule_pack_version,body_hash) VALUES(1,'universe:index-v2-corruption','snapshot:index-v2-corruption','profile',?1,?1,'policy','pack',?1)", [&hash]).unwrap();
        connection.execute("INSERT INTO obligations(obligation_id,target_kind,target_ids_canonical_json,property_id,lifecycle,body_hash) VALUES('obligation:index-v2-corruption','node','[\"node:z\",\"node:a\"]','property','generated',?1)", [&hash]).unwrap();
        assert!(matches!(
            snapshot_from_connection(
                &connection,
                index.limits(),
                build_cache_reservation(index.limits()).unwrap(),
                None,
            ),
            Err(IndexError::CorruptIndex)
        ));
        assert!(
            connection
                .execute("UPDATE obligations SET lifecycle='accepted'", [])
                .is_err()
        );
    }
}
