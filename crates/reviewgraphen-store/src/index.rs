//! Descriptor-anchored serialized SQLite derived index.
//!
//! SQLite is deliberately confined to an in-memory connection.  This module
//! owns the bytes crossing that boundary; callers never provide a path or a
//! connection and the canonical journal remains the authority.

use super::{
    CasHash, CasStore, EventJournal, JournalError, JournalIdentity, StoreError, StoreLimits,
    StoreRoot, open_or_create_dir,
};
use reviewgraphen_core::{
    ContentHash, DecodedPayload, EventContractVersion, EventEnvelope, OfflineProjectionState,
    RunGenesisSnapshot, StableId, canonical_json,
};
use rusqlite::{Connection, MAIN_DB, OpenFlags, limits::Limit};
use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, Dir, FileType, FlockOperation, Mode, OFlags},
    process::{getegid, geteuid},
    rand::{GetRandomFlags, getrandom},
};
use serde::Serialize;
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

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublishFault {
    AfterWrite,
    BeforeCandidateSync,
    AfterCandidateLinkBeforeDirectorySync,
    AfterCandidateSync,
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
#[derive(Clone, Debug, Eq, PartialEq)]
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

/// Typed envelope metadata retained by the derived index.
#[derive(Clone, Debug, Eq, PartialEq)]
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexShadow {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub record_id: StableId,
    pub kind: String,
    pub body_hash: ContentHash,
    pub authority_reconciled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexProgramObject {
    pub object_id: StableId,
    pub object_kind: String,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexProgramRelation {
    pub relation_id: StableId,
    pub relation_kind: String,
    pub source_id: StableId,
    pub target_ids_canonical_json: String,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq)]
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexObligation {
    pub obligation_id: StableId,
    pub target_kind: String,
    pub target_ids_canonical_json: String,
    pub property_id: String,
    pub lifecycle: String,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexObligationLifecycle {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub obligation_id: StableId,
    pub next_lifecycle: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexClaim {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub claim_id: StableId,
    pub execution_id: StableId,
    pub polarity: String,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq)]
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
#[derive(Clone, Debug, Eq, PartialEq)]
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexContextEnvelope {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub envelope_id: StableId,
    pub obligation_id: StableId,
    pub projection_hash: ContentHash,
    pub source_ids_canonical_json: String,
    pub loss_ids_canonical_json: String,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexReviewPlan {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub plan_id: StableId,
    pub run_id: StableId,
    pub budget_hash: ContentHash,
    pub policy_hash: ContentHash,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexExecution {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub execution_id: StableId,
    pub reviewer_id: String,
    pub envelope_id: StableId,
    pub outcome: String,
    pub raw_registration_id: StableId,
    pub raw_hash: ContentHash,
    pub obligation_ids_canonical_json: String,
    pub claim_ids_canonical_json: String,
    pub body_hash: ContentHash,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexFinding {
    pub event_sequence: u64,
    pub event_id: StableId,
    pub finding_id: StableId,
    pub body_hash: ContentHash,
    pub projection_status: String,
}

/// A deterministic, complete projection of every currently representable
/// index table. Empty vectors are meaningful for V1 metadata-only streams.
#[derive(Clone, Debug, Eq, PartialEq)]
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
    pub claims: Vec<IndexClaim>,
    pub artifact_registrations: Vec<IndexArtifactRegistration>,
    pub snapshot_sources: Vec<IndexSnapshotSource>,
    pub context_envelopes: Vec<IndexContextEnvelope>,
    pub review_plans: Vec<IndexReviewPlan>,
    pub executions: Vec<IndexExecution>,
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
        let reader = journal.reader()?;
        let identity = reader.identity().clone();
        let result = reader.with_locked_snapshot(|events, offset, tail| {
            self.build_locked(&identity, events, offset, tail, cas)
        });
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
        let reader = journal.reader()?;
        let identity = reader.identity().clone();
        let result = reader.with_locked_snapshot(|events, offset, tail| {
            let active_before = fs::statat(&self.indexes, ACTIVE_FILE, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(map_entry_error)?;
            verify_index_stat(&active_before, FileType::RegularFile, 0o600)?;
            let image = self.read_active_image_locked()?;
            let connection = deserialize_read_only(image, self.limits)
                .map_err(normalize_external_image_error)?;
            let snapshot = snapshot_from_connection(
                &connection,
                self.limits,
                query_cache_reservation(self.limits)?,
            )
            .map_err(normalize_external_image_error)?;
            let event_count =
                u64::try_from(events.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
            if snapshot.marker.run_id != identity.run_id
                || snapshot.marker.genesis_hash != identity.genesis_hash()
                || snapshot.marker.confirmed_offset != offset
                || snapshot.marker.tail_hash != *tail
                || snapshot.marker.event_count != event_count
                || snapshot.marker.event_contract_version != identity.version().schema()
            {
                return Err(IndexError::CommittedIndexStale {
                    indexed_offset: snapshot.marker.confirmed_offset,
                    indexed_tail: snapshot.marker.tail_hash,
                    committed_offset: offset,
                    committed_tail: tail.clone(),
                });
            }
            #[cfg(test)]
            self.wait_after_snapshot_marker_check();
            let active_after = fs::statat(&self.indexes, ACTIVE_FILE, AtFlags::SYMLINK_NOFOLLOW)
                .map_err(map_entry_error)?;
            verify_index_stat(&active_after, FileType::RegularFile, 0o600)?;
            ensure_same_inode(&active_before, &active_after)?;
            Ok(snapshot)
        });
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

    pub(crate) fn new_in_memory_connection(&self) -> Result<Connection, IndexError> {
        let connection = Connection::open_in_memory_with_flags(
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure_connection(&connection, self.limits)?;
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
        run_id: &StableId,
        tail_hash: &ContentHash,
    ) -> Result<ContentHash, IndexError> {
        let lock = self.lock_exclusive()?;
        let result = self.publish_image_locked(image, run_id, tail_hash);
        lock.verify_unchanged()?;
        result
    }

    fn publish_image_locked(
        &self,
        image: Vec<u8>,
        run_id: &StableId,
        tail_hash: &ContentHash,
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
        let candidate = self.open_entry(&name)?;
        ensure_same_inode(&fs::fstat(&tmp)?, &fs::fstat(&candidate)?)?;
        let candidate_bytes = read_fd_exact(candidate, self.limits.max_serialized_bytes)?;
        if ContentHash::sha256(&candidate_bytes) != hash {
            return Err(IndexError::CorruptIndex);
        }
        // Prepared is not merely byte-hash valid: an independent in-memory,
        // read-only deserialize must re-check schema, marker, FK and complete
        // typed snapshot before it can replace the active image.
        let candidate_connection = deserialize_read_only(candidate_bytes, self.limits)
            .map_err(normalize_external_image_error)?;
        let _ = snapshot_from_connection(
            &candidate_connection,
            self.limits,
            query_cache_reservation(self.limits)?,
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
                run_id: run_id.clone(),
                tail_hash: tail_hash.clone(),
                image_hash: hash,
            });
        }
        let durable = (|| -> Result<(), IndexError> {
            let active = self.open_entry(ACTIVE_FILE)?;
            ensure_same_inode(&fs::fstat(&tmp)?, &fs::fstat(&active)?)?;
            let active_bytes = read_fd_exact(active, self.limits.max_serialized_bytes)?;
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
                run_id: run_id.clone(),
                tail_hash: tail_hash.clone(),
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

    fn read_active_image_locked(&self) -> Result<Vec<u8>, IndexError> {
        let bytes = read_fd_exact(
            self.open_entry(ACTIVE_FILE)?,
            self.limits.max_serialized_bytes,
        )?;
        check_image_len(bytes.len(), self.limits)?;
        Ok(bytes)
    }

    fn build_locked(
        &self,
        identity: &JournalIdentity,
        events: &[reviewgraphen_core::EventEnvelope],
        offset: u64,
        tail: &ContentHash,
        cas: &CasStore<'_>,
    ) -> Result<IndexRebuildReceipt, IndexError> {
        let version = identity.version();
        let initial_view = EventEnvelope::validated_view(
            version,
            &identity.run_id,
            identity.core_genesis(),
            events,
        )
        .map_err(|_| IndexError::ProjectionContractViolation)?;
        let (view, initial) = match version {
            EventContractVersion::V1 => (initial_view, None),
            EventContractVersion::V2 => {
                let Some(first) = initial_view.events().first() else {
                    return Err(IndexError::ProjectionContractViolation);
                };
                let DecodedPayload::RunGenesisManifest(manifest) = first.payload() else {
                    return Err(IndexError::ProjectionContractViolation);
                };
                let hash = CasHash::parse(manifest.genesis_artifact().cas_hash().to_string())
                    .map_err(|_| IndexError::ProjectionContractViolation)?;
                let bytes = cas
                    .read(&hash)
                    .map_err(|_| IndexError::ProjectionContractViolation)?;
                let reviewgraphen_core::EventStreamGenesis::V2(initial_bytes) =
                    identity.core_genesis()
                else {
                    return Err(IndexError::ProjectionContractViolation);
                };
                if bytes != initial_bytes || ContentHash::sha256(&bytes) != identity.genesis_hash()
                {
                    return Err(IndexError::ProjectionContractViolation);
                }
                let snapshot = RunGenesisSnapshot::from_canonical_bytes(&bytes)
                    .map_err(|_| IndexError::ProjectionContractViolation)?;
                let rebuilt = snapshot
                    .rebuild_aggregate()
                    .map_err(|_| IndexError::ProjectionContractViolation)?;
                let definitive = EventEnvelope::validated_view(
                    version,
                    &identity.run_id,
                    reviewgraphen_core::EventStreamGenesis::V2(&bytes),
                    events,
                )
                .map_err(|_| IndexError::ProjectionContractViolation)?;
                (definitive, Some((rebuilt, snapshot)))
            }
        };
        let connection = self.new_in_memory_connection()?;
        let transaction = connection.unchecked_transaction()?;
        let mut rows = 0_u64;
        for event in view.events() {
            reserve_row(&mut rows, self.limits)?;
            insert_event(
                &transaction,
                event.envelope(),
                version.schema(),
                payload_kind_decoded(event.payload()),
            )?;
        }
        if let Some((initial, genesis)) = initial {
            insert_baseline(&transaction, &genesis, &mut rows, self.limits)?;
            let mut expected_lifecycles = genesis
                .obligations()
                .iter()
                .map(|obligation| {
                    Ok((
                        obligation.id().clone(),
                        serialized_enum(&obligation.lifecycle())?,
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, IndexError>>()?;
            let mut projection = OfflineProjectionState::new(&view, initial)
                .map_err(|_| IndexError::ProjectionContractViolation)?;
            for event in view.events() {
                let accepted = projection
                    .apply(event)
                    .map_err(|_| IndexError::ProjectionContractViolation)?;
                match (accepted, event.payload()) {
                    (false, DecodedPayload::EvidenceRecorded(_))
                    | (false, DecodedPayload::EvidenceBound(_))
                    | (false, DecodedPayload::VerificationRecorded(_))
                    | (false, DecodedPayload::DecisionRecorded(_)) => {
                        let metadata_records = projection.unreconciled_records();
                        let metadata = metadata_records
                            .last()
                            .ok_or(IndexError::ProjectionContractViolation)?;
                        reserve_row(&mut rows, self.limits)?;
                        transaction.execute("INSERT INTO unreconciled_authority_records(event_sequence,event_id,record_id,kind,body_hash,authority_reconciled) VALUES(?1,?2,?3,?4,?5,0)", rusqlite::params![to_i64(event.envelope().sequence())?, event.envelope().id().to_string(), metadata.id().to_string(), unreconciled_kind(metadata.kind()), metadata.body_hash().to_string()]).map_err(map_sql)?;
                    }
                    (false, DecodedPayload::FindingRecorded(_)) => {
                        let finding_metadata = projection.projected_findings();
                        let metadata = finding_metadata
                            .last()
                            .ok_or(IndexError::ProjectionContractViolation)?;
                        reserve_row(&mut rows, self.limits)?;
                        transaction.execute("INSERT INTO projected_findings(event_sequence,event_id,finding_id,body_hash,projection_status) VALUES(?1,?2,?3,?4,'shadow_only')", rusqlite::params![to_i64(event.envelope().sequence())?, event.envelope().id().to_string(), metadata.id().to_string(), metadata.body_hash().to_string()]).map_err(map_sql)?;
                    }
                    (
                        true,
                        DecodedPayload::ObligationTransition {
                            obligation_id,
                            next,
                        },
                    ) => {
                        reserve_row(&mut rows, self.limits)?;
                        let lifecycle = serialized_enum(next)?;
                        let previous = expected_lifecycles
                            .get(obligation_id)
                            .ok_or(IndexError::ProjectionContractViolation)?
                            .clone();
                        transaction.execute("INSERT INTO obligation_lifecycle(event_sequence,event_id,obligation_id,next_lifecycle) VALUES(?1,?2,?3,?4)", rusqlite::params![to_i64(event.envelope().sequence())?, event.envelope().id().to_string(), obligation_id.to_string(), lifecycle]).map_err(map_sql)?;
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
                    (true, DecodedPayload::ClaimProposed(claim)) => insert_claim(
                        &transaction,
                        event.envelope(),
                        claim,
                        &mut rows,
                        self.limits,
                    )?,
                    (true, DecodedPayload::RunGenesisManifest(manifest)) => {
                        insert_registration(
                            &transaction,
                            event.envelope(),
                            manifest.genesis_artifact(),
                            &mut rows,
                            self.limits,
                        )?;
                    }
                    (true, DecodedPayload::ArtifactRegistered(registration)) => {
                        insert_registration(
                            &transaction,
                            event.envelope(),
                            registration,
                            &mut rows,
                            self.limits,
                        )?
                    }
                    (true, DecodedPayload::SnapshotSourcesRecorded(sources)) => insert_sources(
                        &transaction,
                        event.envelope(),
                        sources,
                        &mut rows,
                        self.limits,
                    )?,
                    _ => return Err(IndexError::ProjectionContractViolation),
                }
            }
            if !projection.is_complete() || projection.tail_hash() != tail {
                return Err(IndexError::ProjectionContractViolation);
            }
        }
        reserve_row(&mut rows, self.limits)?;
        let event_count = u64::try_from(events.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
        let mode = if version == EventContractVersion::V1 {
            "v1_event_metadata_only"
        } else {
            "v2_domain"
        };
        transaction.execute("INSERT INTO index_meta(singleton,index_schema_version,projection_contract_version,event_contract_version,projection_mode,run_id,genesis_hash,confirmed_offset,tail_hash,event_count) VALUES(1,1,'reviewgraphen.index_projection.v1',?1,?2,?3,?4,?5,?6,?7)", rusqlite::params![version.schema(), mode, identity.run_id.to_string(), identity.genesis_hash().to_string(), to_i64(offset)?, tail.to_string(), to_i64(event_count)?]).map_err(map_sql)?;
        transaction.commit()?;
        if !connection.is_autocommit() {
            return Err(IndexError::ProjectionContractViolation);
        }
        // Validate the complete marker/table/FK/integrity contract before
        // any bytes cross the SQLite-to-store publication boundary.
        let _ = snapshot_from_connection(
            &connection,
            self.limits,
            build_cache_reservation(self.limits)?,
        )?;
        let image = serialize_connection(&connection, self.limits)?;
        let serialized_bytes =
            u64::try_from(image.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
        // SQLite build/cache memory is released before descriptor publication
        // takes ownership of the serialized image.
        drop(connection);
        let image_hash = self.publish_image_locked(image, &identity.run_id, tail)?;
        Ok(IndexRebuildReceipt {
            run_id: identity.run_id.clone(),
            genesis_hash: identity.genesis_hash(),
            confirmed_offset: offset,
            tail_hash: tail.clone(),
            event_count,
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

fn configure_connection(connection: &Connection, limits: IndexLimits) -> Result<(), IndexError> {
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
        "PRAGMA user_version = 1",
    ] {
        checked_batch(connection, statement, limits)?;
    }
    connection.pragma_update(None, "max_page_count", max_pages)?;
    let cache_kib = cache_kib(limits, build_cache_reservation(limits)?)?;
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
        ("user_version", 1),
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
 index_schema_version INTEGER NOT NULL CHECK (index_schema_version = 1),
 projection_contract_version TEXT NOT NULL CHECK (projection_contract_version = 'reviewgraphen.index_projection.v1'),
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
 payload_kind TEXT NOT NULL CHECK (payload_kind IN ('obligation_transition','claim_proposed','evidence_recorded','evidence_bound','verification_recorded','decision_recorded','finding_recorded','run_genesis_manifest','artifact_registered','snapshot_sources_recorded')),
 actor TEXT NOT NULL, logical_time INTEGER NOT NULL CHECK (logical_time >= 0), UNIQUE(sequence,event_id)
) STRICT;
CREATE TABLE program_objects (object_id TEXT PRIMARY KEY, object_kind TEXT NOT NULL, body_hash TEXT NOT NULL) STRICT;
CREATE TABLE program_relations (relation_id TEXT PRIMARY KEY, relation_kind TEXT NOT NULL, source_id TEXT NOT NULL, target_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL) STRICT;
CREATE TABLE universe (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), universe_id TEXT NOT NULL UNIQUE, snapshot_id TEXT NOT NULL, profile_id TEXT NOT NULL, rule_set_hash TEXT NOT NULL, extractor_set_hash TEXT NOT NULL, policy_version TEXT NOT NULL, rule_pack_version TEXT NOT NULL, body_hash TEXT NOT NULL) STRICT;
CREATE TABLE obligations (obligation_id TEXT PRIMARY KEY, target_kind TEXT NOT NULL CHECK(target_kind IN ('node','relation','path','invariant','subgraph')), target_ids_canonical_json TEXT NOT NULL, property_id TEXT NOT NULL, lifecycle TEXT NOT NULL CHECK(lifecycle IN ('generated','planned','in_progress','completed','stale','superseded','cancelled')), body_hash TEXT NOT NULL) STRICT;
CREATE TABLE obligation_lifecycle (event_sequence INTEGER NOT NULL, event_id TEXT NOT NULL, obligation_id TEXT NOT NULL, next_lifecycle TEXT NOT NULL CHECK(next_lifecycle IN ('generated','planned','in_progress','completed','stale','superseded','cancelled')), PRIMARY KEY(event_sequence, obligation_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE claims (event_sequence INTEGER NOT NULL, event_id TEXT NOT NULL, claim_id TEXT NOT NULL UNIQUE, execution_id TEXT NOT NULL, polarity TEXT NOT NULL CHECK(polarity IN ('issue_present','issue_absent','inconclusive','not_applicable','conflict')), body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,claim_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE artifact_registrations (event_sequence INTEGER NOT NULL, event_id TEXT NOT NULL, registration_id TEXT NOT NULL UNIQUE, run_id TEXT NOT NULL, cas_hash TEXT NOT NULL, media_type TEXT NOT NULL, size INTEGER NOT NULL CHECK(size >= 0), sensitivity TEXT NOT NULL CHECK(sensitivity IN ('canonical_state','workspace_source','sensitive')), source_kind TEXT NOT NULL CHECK(source_kind IN ('run_genesis','snapshot_ingest','reviewer_execution')), source_id TEXT NOT NULL, body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,registration_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE snapshot_source_index (event_sequence INTEGER NOT NULL, event_id TEXT NOT NULL, snapshot_id TEXT NOT NULL, artifact_id TEXT NOT NULL, registration_id TEXT NOT NULL, path TEXT NOT NULL, content_hash TEXT NOT NULL, cas_hash TEXT NOT NULL, line_count INTEGER NOT NULL CHECK(line_count >= 0), PRIMARY KEY(snapshot_id,path,artifact_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE context_envelopes (event_sequence INTEGER NOT NULL, event_id TEXT NOT NULL, envelope_id TEXT NOT NULL UNIQUE, obligation_id TEXT NOT NULL, projection_hash TEXT NOT NULL, source_ids_canonical_json TEXT NOT NULL, loss_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,envelope_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE review_plans (event_sequence INTEGER NOT NULL, event_id TEXT NOT NULL, plan_id TEXT NOT NULL UNIQUE, run_id TEXT NOT NULL, budget_hash TEXT NOT NULL, policy_hash TEXT NOT NULL, body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,plan_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE executions (event_sequence INTEGER NOT NULL, event_id TEXT NOT NULL, execution_id TEXT NOT NULL UNIQUE, reviewer_id TEXT NOT NULL, envelope_id TEXT NOT NULL, outcome TEXT NOT NULL CHECK(outcome IN ('completed','abstained','malformed','provider_failure')), raw_registration_id TEXT NOT NULL, raw_hash TEXT NOT NULL, obligation_ids_canonical_json TEXT NOT NULL, claim_ids_canonical_json TEXT NOT NULL, body_hash TEXT NOT NULL, PRIMARY KEY(event_sequence,execution_id), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE unreconciled_authority_records (event_sequence INTEGER NOT NULL CHECK(event_sequence > 0), event_id TEXT NOT NULL, record_id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('evidence_recorded','evidence_bound','verification_recorded','decision_recorded')), body_hash TEXT NOT NULL, authority_reconciled INTEGER NOT NULL DEFAULT 0 CHECK(authority_reconciled = 0), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
CREATE TABLE projected_findings (event_sequence INTEGER NOT NULL CHECK(event_sequence > 0), event_id TEXT NOT NULL, finding_id TEXT PRIMARY KEY, body_hash TEXT NOT NULL, projection_status TEXT NOT NULL DEFAULT 'shadow_only' CHECK(projection_status = 'shadow_only'), FOREIGN KEY(event_sequence,event_id) REFERENCES events(sequence,event_id)) STRICT;
"#;
    checked_batch(connection, DDL, limits)
}

pub(crate) fn serialize_connection(
    connection: &Connection,
    limits: IndexLimits,
) -> Result<Vec<u8>, IndexError> {
    let image = connection.serialize(MAIN_DB)?;
    let one = u64::try_from(image.len()).map_err(|_| IndexError::IntegerOutOfRange)?;
    // At this point the build connection still owns its page cache while
    // SQLite exposes a serialized view and `to_vec` creates the publication
    // image.  Publication begins only after its caller drops `connection`.
    let cache = configured_cache_bytes(limits, build_cache_reservation(limits)?)?;
    let peak = one
        .checked_mul(3)
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
    let bytes = image.to_vec();
    check_image_len(bytes.len(), limits)?;
    Ok(bytes)
}

/// Reconstructs an image solely into an ephemeral SQLite connection.  The
/// caller retains no borrowed image and the connection is query-only before
/// a marker or projection table can be inspected.
pub(crate) fn deserialize_read_only(
    image: Vec<u8>,
    limits: IndexLimits,
) -> Result<Connection, IndexError> {
    let image_len = image.len();
    check_image_len(image_len, limits)?;
    let image_bytes = u64::try_from(image_len).map_err(|_| IndexError::IntegerOutOfRange)?;
    // The input Vec is moved into Cursor below and released by
    // `deserialize_read_exact`; before that transfer, account it beside the
    // reconstructed main image and the query cache. Public result material is
    // charged later, after this Vec has gone away.
    let cache = configured_cache_bytes(limits, query_cache_reservation(limits)?)?;
    let deserialize_peak = image_bytes
        .checked_add(image_bytes)
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
    configure_query_connection(&connection, limits)?;
    connection.pragma_update(None, "query_only", true)?;
    let query_only: i64 = connection.pragma_query_value(None, "query_only", |row| row.get(0))?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    // SQLite reports an in-memory deserialize as writable even with its
    // read-only deserialize flag; `query_only=1` is the enforceable mutation
    // boundary for this path and is read back below.
    if query_only != 1 || version != 1 {
        return Err(IndexError::CorruptIndex);
    }
    Ok(connection)
}

fn configure_query_connection(
    connection: &Connection,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "trusted_schema", false)?;
    let cache_kib = cache_kib(limits, query_cache_reservation(limits)?)?;
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

fn build_cache_reservation(limits: IndexLimits) -> Result<u64, IndexError> {
    limits
        .max_serialized_bytes
        .checked_mul(3)
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })
}

fn query_cache_reservation(limits: IndexLimits) -> Result<u64, IndexError> {
    limits
        .max_serialized_bytes
        .checked_mul(2)
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
    let remaining =
        limits
            .max_working_bytes
            .checked_sub(reserved)
            .ok_or(IndexError::Incomplete {
                limit: limits.max_working_bytes,
                observed: reserved,
            })?;
    let kib = remaining / 1024;
    if kib == 0 {
        return Err(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: reserved,
        });
    }
    i64::try_from(kib).map_err(|_| IndexError::IntegerOutOfRange)
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

fn read_fd_exact(fd: OwnedFd, max: u64) -> Result<Vec<u8>, IndexError> {
    let before = fs::fstat(&fd)?;
    verify_index_stat(&before, FileType::RegularFile, 0o600)?;
    let size = u64::try_from(before.st_size).map_err(|_| IndexError::CorruptIndex)?;
    if size > max {
        return Err(IndexError::Incomplete {
            limit: max,
            observed: size,
        });
    }
    let capacity = usize::try_from(size).map_err(|_| IndexError::IntegerOutOfRange)?;
    let mut file = File::from(fd);
    let mut bytes = Vec::with_capacity(capacity);
    (&mut file)
        .take(size.checked_add(1).ok_or(IndexError::IntegerOutOfRange)?)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).map_err(|_| IndexError::IntegerOutOfRange)? != size {
        return Err(IndexError::CorruptIndex);
    }
    let after = file.metadata()?;
    if after.len() != size {
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
        | IndexError::IntegerOutOfRange => error,
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

fn insert_claim(
    tx: &rusqlite::Transaction<'_>,
    event: &EventEnvelope,
    claim: &reviewgraphen_core::ReviewClaim,
    rows: &mut u64,
    limits: IndexLimits,
) -> Result<(), IndexError> {
    reserve_row(rows, limits)?;
    tx.execute("INSERT INTO claims(event_sequence,event_id,claim_id,execution_id,polarity,body_hash) VALUES(?1,?2,?3,?4,?5,?6)", rusqlite::params![to_i64(event.sequence())?,event.id().to_string(),claim.id().to_string(),claim.execution_id().to_string(),serialized_enum(&claim.polarity())?,body_hash(claim)?.to_string()]).map_err(map_sql)?;
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

fn payload_kind_decoded(payload: &DecodedPayload) -> &'static str {
    match payload {
        DecodedPayload::ObligationTransition { .. } => "obligation_transition",
        DecodedPayload::ClaimProposed(_) => "claim_proposed",
        DecodedPayload::EvidenceRecorded(_) => "evidence_recorded",
        DecodedPayload::EvidenceBound(_) => "evidence_bound",
        DecodedPayload::VerificationRecorded(_) => "verification_recorded",
        DecodedPayload::DecisionRecorded(_) => "decision_recorded",
        DecodedPayload::FindingRecorded(_) => "finding_recorded",
        DecodedPayload::RunGenesisManifest(_) => "run_genesis_manifest",
        DecodedPayload::ArtifactRegistered(_) => "artifact_registered",
        DecodedPayload::SnapshotSourcesRecorded(_) => "snapshot_sources_recorded",
    }
}

fn unreconciled_kind(kind: reviewgraphen_core::UnreconciledRecordKind) -> &'static str {
    match kind {
        reviewgraphen_core::UnreconciledRecordKind::Evidence => "evidence_recorded",
        reviewgraphen_core::UnreconciledRecordKind::EvidenceBinding => "evidence_bound",
        reviewgraphen_core::UnreconciledRecordKind::Verification => "verification_recorded",
        reviewgraphen_core::UnreconciledRecordKind::Decision => "decision_recorded",
    }
}

fn snapshot_from_connection(
    connection: &Connection,
    limits: IndexLimits,
    cache_reservation: u64,
) -> Result<IndexSnapshot, IndexError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != 1 {
        return Err(IndexError::CorruptIndex);
    }
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
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
    let marker = connection.query_row("SELECT index_schema_version,projection_contract_version,event_contract_version,projection_mode,run_id,genesis_hash,confirmed_offset,tail_hash,event_count FROM index_meta WHERE singleton=1", [], |row| {
        Ok(IndexMarker { index_schema_version: u64::try_from(row.get::<_,i64>(0)?).map_err(|_| rusqlite::Error::InvalidQuery)?, sqlite_user_version: u64::try_from(version).map_err(|_| rusqlite::Error::InvalidQuery)?, projection_contract_version: row.get(1)?, event_contract_version: row.get(2)?, projection_mode: row.get(3)?, run_id: StableId::parse(row.get::<_,String>(4)?).map_err(|_| rusqlite::Error::InvalidQuery)?, genesis_hash: ContentHash::parse(row.get::<_,String>(5)?).map_err(|_| rusqlite::Error::InvalidQuery)?, confirmed_offset: u64::try_from(row.get::<_,i64>(6)?).map_err(|_| rusqlite::Error::InvalidQuery)?, tail_hash: ContentHash::parse(row.get::<_,String>(7)?).map_err(|_| rusqlite::Error::InvalidQuery)?, event_count: u64::try_from(row.get::<_,i64>(8)?).map_err(|_| rusqlite::Error::InvalidQuery)? })
    }).map_err(|_| IndexError::CorruptIndex)?;
    if marker.index_schema_version != marker.sqlite_user_version || marker.sqlite_user_version != 1
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
            "SELECT COUNT(*) FROM claims",
            "SELECT COUNT(*) FROM artifact_registrations",
            "SELECT COUNT(*) FROM snapshot_source_index",
            "SELECT COUNT(*) FROM context_envelopes",
            "SELECT COUNT(*) FROM review_plans",
            "SELECT COUNT(*) FROM executions",
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
    let query_bytes = preflight_query_budget(connection, &marker, limits)?;
    let page_count: i64 = connection.pragma_query_value(None, "page_count", |row| row.get(0))?;
    let main_bytes = u64::try_from(page_count)
        .map_err(|_| IndexError::CorruptIndex)?
        .checked_mul(PAGE_SIZE)
        .ok_or(IndexError::Incomplete {
            limit: limits.max_working_bytes,
            observed: u64::MAX,
        })?;
    let cache_bytes = configured_cache_bytes(limits, cache_reservation)?;
    let peak = main_bytes
        .checked_add(cache_bytes)
        .and_then(|value| value.checked_add(query_bytes))
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
    let mut events = Vec::new();
    let mut statement = connection.prepare("SELECT sequence,event_id,schema,event_hash,payload_hash,payload_kind,actor,logical_time FROM events ORDER BY sequence,event_id")?;
    let rows = statement.query_map([], |row| {
        Ok(IndexEvent {
            sequence: u64::try_from(row.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: StableId::parse(row.get::<_, String>(1)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            schema: row.get(2)?,
            event_hash: ContentHash::parse(row.get::<_, String>(3)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            payload_hash: ContentHash::parse(row.get::<_, String>(4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            payload_kind: row.get(5)?,
            actor: row.get(6)?,
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
    let mut shadows = Vec::new();
    let mut shadow_statement = connection.prepare("SELECT event_sequence,event_id,record_id,kind,body_hash,authority_reconciled FROM unreconciled_authority_records ORDER BY event_sequence,record_id")?;
    let shadow_rows = shadow_statement.query_map([], |row| {
        Ok(IndexShadow {
            event_sequence: u64::try_from(row.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: StableId::parse(row.get::<_, String>(1)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            record_id: StableId::parse(row.get::<_, String>(2)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            kind: row.get(3)?,
            body_hash: ContentHash::parse(row.get::<_, String>(4)?)
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
    let claims = query_claims(connection)?;
    let artifact_registrations = query_registrations(connection)?;
    let snapshot_sources = query_sources(connection)?;
    let context_envelopes = query_context_envelopes(connection)?;
    let review_plans = query_review_plans(connection)?;
    let executions = query_executions(connection)?;
    Ok(IndexSnapshot {
        marker,
        events,
        shadows,
        findings,
        program_objects,
        program_relations,
        universe,
        obligations,
        obligation_lifecycle,
        claims,
        artifact_registrations,
        snapshot_sources,
        context_envelopes,
        review_plans,
        executions,
    })
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
    let mut used = 0_u64;
    for value in [
        marker.projection_contract_version.as_str(),
        marker.event_contract_version.as_str(),
        marker.projection_mode.as_str(),
        &marker.run_id.to_string(),
        &marker.genesis_hash.to_string(),
        &marker.tail_hash.to_string(),
    ] {
        charge_bytes(
            &mut used,
            limits,
            u64::try_from(value.len()).map_err(|_| IndexError::IntegerOutOfRange)?,
        )?;
    }
    charge_bytes(&mut used, limits, 24)?;
    let tables = [
        (
            "SELECT COUNT(*), COALESCE(SUM(length(event_id)+length(schema)+length(event_hash)+length(payload_hash)+length(payload_kind)+length(actor)),0) FROM events",
            vector_row_cost::<IndexEvent>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(event_id)+length(record_id)+length(kind)+length(body_hash)),0) FROM unreconciled_authority_records",
            vector_row_cost::<IndexShadow>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(event_id)+length(finding_id)+length(body_hash)+length(projection_status)),0) FROM projected_findings",
            vector_row_cost::<IndexFinding>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(object_id)+length(object_kind)+length(body_hash)),0) FROM program_objects",
            vector_row_cost::<IndexProgramObject>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(relation_id)+length(relation_kind)+length(source_id)+length(target_ids_canonical_json)+length(body_hash)),0) FROM program_relations",
            vector_row_cost::<IndexProgramRelation>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(universe_id)+length(snapshot_id)+length(profile_id)+length(rule_set_hash)+length(extractor_set_hash)+length(policy_version)+length(rule_pack_version)+length(body_hash)),0) FROM universe",
            vector_row_cost::<IndexUniverse>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(obligation_id)+length(target_kind)+length(target_ids_canonical_json)+length(property_id)+length(lifecycle)+length(body_hash)),0) FROM obligations",
            vector_row_cost::<IndexObligation>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(event_id)+length(obligation_id)+length(next_lifecycle)),0) FROM obligation_lifecycle",
            vector_row_cost::<IndexObligationLifecycle>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(event_id)+length(claim_id)+length(execution_id)+length(polarity)+length(body_hash)),0) FROM claims",
            vector_row_cost::<IndexClaim>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(event_id)+length(registration_id)+length(run_id)+length(cas_hash)+length(media_type)+length(sensitivity)+length(source_kind)+length(source_id)+length(body_hash)),0) FROM artifact_registrations",
            vector_row_cost::<IndexArtifactRegistration>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(event_id)+length(snapshot_id)+length(artifact_id)+length(registration_id)+length(path)+length(content_hash)+length(cas_hash)),0) FROM snapshot_source_index",
            vector_row_cost::<IndexSnapshotSource>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(event_id)+length(envelope_id)+length(obligation_id)+length(projection_hash)+length(source_ids_canonical_json)+length(loss_ids_canonical_json)+length(body_hash)),0) FROM context_envelopes",
            vector_row_cost::<IndexContextEnvelope>(),
        ),
        (
            "SELECT COUNT(*), COALESCE(SUM(length(event_id)+length(plan_id)+length(run_id)+length(budget_hash)+length(policy_hash)+length(body_hash)),0) FROM review_plans",
            vector_row_cost::<IndexReviewPlan>(),
        ),
    ];
    for (sql, scalar_bytes) in tables {
        charge_query_measurement(connection, sql, scalar_bytes, &mut used, limits)?;
    }
    charge_query_measurement(
        connection,
        "SELECT COUNT(*), COALESCE(SUM(length(event_id)+length(execution_id)+length(reviewer_id)+length(envelope_id)+length(outcome)+length(raw_registration_id)+length(raw_hash)+length(obligation_ids_canonical_json)+length(claim_ids_canonical_json)+length(body_hash)),0) FROM executions",
        vector_row_cost::<IndexExecution>(),
        &mut used,
        limits,
    )?;
    Ok(used)
}

fn vector_row_cost<T>() -> u64 {
    // `Vec` growth can hold both the old and new allocation during a resize.
    // Charge two elements per returned row before materializing any row.
    u64::try_from(std::mem::size_of::<T>()).expect("type size fits u64") * 2
}

fn charge_query_measurement(
    connection: &Connection,
    sql: &str,
    scalar_bytes: u64,
    used: &mut u64,
    limits: IndexLimits,
) -> Result<(), IndexError> {
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
    charge_bytes(used, limits, scalar_bytes)
}

fn parse_id(value: String) -> rusqlite::Result<StableId> {
    StableId::parse(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn parse_hash(value: String) -> rusqlite::Result<ContentHash> {
    ContentHash::parse(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn query_program_objects(c: &Connection) -> Result<Vec<IndexProgramObject>, IndexError> {
    let mut s = c.prepare(
        "SELECT object_id,object_kind,body_hash FROM program_objects ORDER BY object_id",
    )?;
    let r = s.query_map([], |x| {
        Ok(IndexProgramObject {
            object_id: parse_id(x.get(0)?)?,
            object_kind: x.get(1)?,
            body_hash: parse_hash(x.get(2)?)?,
        })
    })?;
    r.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| IndexError::CorruptIndex)
}
fn query_program_relations(c: &Connection) -> Result<Vec<IndexProgramRelation>, IndexError> {
    let mut s=c.prepare("SELECT relation_id,relation_kind,source_id,target_ids_canonical_json,body_hash FROM program_relations ORDER BY relation_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexProgramRelation {
            relation_id: parse_id(x.get(0)?)?,
            relation_kind: x.get(1)?,
            source_id: parse_id(x.get(2)?)?,
            target_ids_canonical_json: x.get(3)?,
            body_hash: parse_hash(x.get(4)?)?,
        })
    })?;
    let rows = r
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| IndexError::CorruptIndex)?;
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
            universe_id: parse_id(x.get(0)?)?,
            snapshot_id: parse_id(x.get(1)?)?,
            profile_id: x.get(2)?,
            rule_set_hash: parse_hash(x.get(3)?)?,
            extractor_set_hash: parse_hash(x.get(4)?)?,
            policy_version: x.get(5)?,
            rule_pack_version: x.get(6)?,
            body_hash: parse_hash(x.get(7)?)?,
        })),
    }
}
fn query_obligations(c: &Connection) -> Result<Vec<IndexObligation>, IndexError> {
    let mut s=c.prepare("SELECT obligation_id,target_kind,target_ids_canonical_json,property_id,lifecycle,body_hash FROM obligations ORDER BY obligation_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexObligation {
            obligation_id: parse_id(x.get(0)?)?,
            target_kind: x.get(1)?,
            target_ids_canonical_json: x.get(2)?,
            property_id: x.get(3)?,
            lifecycle: x.get(4)?,
            body_hash: parse_hash(x.get(5)?)?,
        })
    })?;
    let rows = r
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| IndexError::CorruptIndex)?;
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
            event_id: parse_id(x.get(1)?)?,
            obligation_id: parse_id(x.get(2)?)?,
            next_lifecycle: x.get(3)?,
        })
    })?;
    r.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| IndexError::CorruptIndex)
}
fn query_claims(c: &Connection) -> Result<Vec<IndexClaim>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,claim_id,execution_id,polarity,body_hash FROM claims ORDER BY event_sequence,claim_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexClaim {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(x.get(1)?)?,
            claim_id: parse_id(x.get(2)?)?,
            execution_id: parse_id(x.get(3)?)?,
            polarity: x.get(4)?,
            body_hash: parse_hash(x.get(5)?)?,
        })
    })?;
    r.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| IndexError::CorruptIndex)
}
fn query_registrations(c: &Connection) -> Result<Vec<IndexArtifactRegistration>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,registration_id,run_id,cas_hash,media_type,size,sensitivity,source_kind,source_id,body_hash FROM artifact_registrations ORDER BY event_sequence,registration_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexArtifactRegistration {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(x.get(1)?)?,
            registration_id: parse_id(x.get(2)?)?,
            run_id: parse_id(x.get(3)?)?,
            cas_hash: parse_hash(x.get(4)?)?,
            media_type: x.get(5)?,
            size: u64::try_from(x.get::<_, i64>(6)?).map_err(|_| rusqlite::Error::InvalidQuery)?,
            sensitivity: x.get(7)?,
            source_kind: x.get(8)?,
            source_id: x.get(9)?,
            body_hash: parse_hash(x.get(10)?)?,
        })
    })?;
    r.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| IndexError::CorruptIndex)
}
fn query_sources(c: &Connection) -> Result<Vec<IndexSnapshotSource>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,snapshot_id,artifact_id,registration_id,path,content_hash,cas_hash,line_count FROM snapshot_source_index ORDER BY snapshot_id,path,artifact_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexSnapshotSource {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(x.get(1)?)?,
            snapshot_id: parse_id(x.get(2)?)?,
            artifact_id: parse_id(x.get(3)?)?,
            registration_id: parse_id(x.get(4)?)?,
            path: x.get(5)?,
            content_hash: parse_hash(x.get(6)?)?,
            cas_hash: parse_hash(x.get(7)?)?,
            line_count: u64::try_from(x.get::<_, i64>(8)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
        })
    })?;
    r.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| IndexError::CorruptIndex)
}
fn query_findings(c: &Connection) -> Result<Vec<IndexFinding>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,finding_id,body_hash,projection_status FROM projected_findings ORDER BY event_sequence,finding_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexFinding {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(x.get(1)?)?,
            finding_id: parse_id(x.get(2)?)?,
            body_hash: parse_hash(x.get(3)?)?,
            projection_status: x.get(4)?,
        })
    })?;
    r.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| IndexError::CorruptIndex)
}
fn query_context_envelopes(c: &Connection) -> Result<Vec<IndexContextEnvelope>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,envelope_id,obligation_id,projection_hash,source_ids_canonical_json,loss_ids_canonical_json,body_hash FROM context_envelopes ORDER BY event_sequence,envelope_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexContextEnvelope {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(x.get(1)?)?,
            envelope_id: parse_id(x.get(2)?)?,
            obligation_id: parse_id(x.get(3)?)?,
            projection_hash: parse_hash(x.get(4)?)?,
            source_ids_canonical_json: x.get(5)?,
            loss_ids_canonical_json: x.get(6)?,
            body_hash: parse_hash(x.get(7)?)?,
        })
    })?;
    let rows = r
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| IndexError::CorruptIndex)?;
    for row in &rows {
        validate_canonical_ids(&row.source_ids_canonical_json)?;
        validate_canonical_ids(&row.loss_ids_canonical_json)?;
    }
    Ok(rows)
}
fn query_review_plans(c: &Connection) -> Result<Vec<IndexReviewPlan>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,plan_id,run_id,budget_hash,policy_hash,body_hash FROM review_plans ORDER BY event_sequence,plan_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexReviewPlan {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(x.get(1)?)?,
            plan_id: parse_id(x.get(2)?)?,
            run_id: parse_id(x.get(3)?)?,
            budget_hash: parse_hash(x.get(4)?)?,
            policy_hash: parse_hash(x.get(5)?)?,
            body_hash: parse_hash(x.get(6)?)?,
        })
    })?;
    r.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| IndexError::CorruptIndex)
}
fn query_executions(c: &Connection) -> Result<Vec<IndexExecution>, IndexError> {
    let mut s=c.prepare("SELECT event_sequence,event_id,execution_id,reviewer_id,envelope_id,outcome,raw_registration_id,raw_hash,obligation_ids_canonical_json,claim_ids_canonical_json,body_hash FROM executions ORDER BY event_sequence,execution_id")?;
    let r = s.query_map([], |x| {
        Ok(IndexExecution {
            event_sequence: u64::try_from(x.get::<_, i64>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?,
            event_id: parse_id(x.get(1)?)?,
            execution_id: parse_id(x.get(2)?)?,
            reviewer_id: x.get(3)?,
            envelope_id: parse_id(x.get(4)?)?,
            outcome: x.get(5)?,
            raw_registration_id: parse_id(x.get(6)?)?,
            raw_hash: parse_hash(x.get(7)?)?,
            obligation_ids_canonical_json: x.get(8)?,
            claim_ids_canonical_json: x.get(9)?,
            body_hash: parse_hash(x.get(10)?)?,
        })
    })?;
    let rows = r
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| IndexError::CorruptIndex)?;
    for row in &rows {
        validate_canonical_ids(&row.obligation_ids_canonical_json)?;
        validate_canonical_ids(&row.claim_ids_canonical_json)?;
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reviewgraphen_core::{
        EventCommand, EventLog, Evidence, EvidenceDetails, MvpRulePack, ObligationLifecycle,
        ProgramSpace, Provenance, ReviewAggregate, SourceRef,
    };
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::Path;
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::mpsc,
        time::Duration,
    };

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
        assert!(matches!(
            index.snapshot_current(&journal),
            Err(IndexError::CommittedIndexStale { .. })
        ));
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
            Err(IndexError::ProjectionContractViolation)
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
        assert_eq!(version, 1);
        let image = serialize_connection(&connection, index.limits()).unwrap();
        assert!(!image.is_empty());
        assert_eq!(image.len() % PAGE_SIZE as usize, 0);
        let readonly = deserialize_read_only(image, index.limits()).unwrap();
        let reopened: i64 = readonly
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(reopened, 1);
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
    fn publication_failpoints_preserve_old_before_rename_and_report_uncertain_after() {
        let run = StableId::parse("run:index-publish-failpoint").unwrap();
        let tail = ContentHash::sha256(b"tail");
        for fault in [
            PublishFault::AfterWrite,
            PublishFault::BeforeCandidateSync,
            PublishFault::AfterCandidateLinkBeforeDirectorySync,
            PublishFault::AfterCandidateSync,
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
                "INSERT INTO index_meta(singleton,index_schema_version,projection_contract_version,event_contract_version,projection_mode,run_id,genesis_hash,confirmed_offset,tail_hash,event_count) VALUES(1,1,'reviewgraphen.index_projection.v1','reviewgraphen.review_event.v1','v1_event_metadata_only',?1,?2,0,?3,1)",
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
                connection.pragma_update(None, "user_version", 2).unwrap();
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
        )
        .unwrap();
        assert_eq!(snapshot.marker.projection_mode, "v1_event_metadata_only");
        assert_eq!(snapshot.marker.sqlite_user_version, 1);
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
        connection.execute("INSERT INTO index_meta(singleton,index_schema_version,projection_contract_version,event_contract_version,projection_mode,run_id,genesis_hash,confirmed_offset,tail_hash,event_count) VALUES(1,1,'reviewgraphen.index_projection.v1','reviewgraphen.review_event.v2','v2_domain','run:index-v2-corruption',?1,1,?1,1)", [&hash]).unwrap();
        connection.execute("INSERT INTO events(sequence,event_id,schema,event_hash,payload_hash,payload_kind,actor,logical_time) VALUES(1,'event:index-v2-corruption','reviewgraphen.review_event.v2',?1,?1,'run_genesis_manifest','system',1)", [&hash]).unwrap();
        connection.execute("INSERT INTO program_objects(object_id,object_kind,body_hash) VALUES('node:index-v2-corruption','node',?1)", [&hash]).unwrap();
        connection.execute("INSERT INTO universe(singleton,universe_id,snapshot_id,profile_id,rule_set_hash,extractor_set_hash,policy_version,rule_pack_version,body_hash) VALUES(1,'universe:index-v2-corruption','snapshot:index-v2-corruption','profile',?1,?1,'policy','pack',?1)", [&hash]).unwrap();
        connection.execute("INSERT INTO obligations(obligation_id,target_kind,target_ids_canonical_json,property_id,lifecycle,body_hash) VALUES('obligation:index-v2-corruption','node','[\"node:z\",\"node:a\"]','property','generated',?1)", [&hash]).unwrap();
        assert!(matches!(
            snapshot_from_connection(
                &connection,
                index.limits(),
                build_cache_reservation(index.limits()).unwrap(),
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
