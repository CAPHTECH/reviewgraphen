//! Locked, durably appended JSONL event journals.
//!
//! The journal deliberately does not apply domain events.  It only admits a
//! complete, core-validated chain while holding the OS lock that protects the
//! bytes.  Projection and authority reconciliation remain core/index work.

use super::{StoreError, StoreRoot, open_or_create_dir, verify_fd_kind_mode};
use reviewgraphen_core::{
    ContentHash, EventContractVersion, EventEnvelope, EventStreamGenesis, StableId, canonical_json,
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
}

impl JournalGenesis {
    fn version(&self) -> EventContractVersion {
        match self {
            Self::V1(_) => EventContractVersion::V1,
            Self::V2(_) | Self::V2Shared(_) => EventContractVersion::V2,
        }
    }
    fn hash(&self) -> ContentHash {
        match self {
            Self::V1(hash) => hash.clone(),
            Self::V2(bytes) => ContentHash::sha256(bytes),
            Self::V2Shared(bytes) => ContentHash::sha256(bytes),
        }
    }
    fn core_genesis(&self) -> EventStreamGenesis<'_> {
        match self {
            Self::V1(hash) => EventStreamGenesis::V1(hash),
            Self::V2(bytes) => EventStreamGenesis::V2(bytes),
            Self::V2Shared(bytes) => EventStreamGenesis::V2(bytes),
        }
    }
}

/// The one logical event stream owned by a durable JSONL file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalIdentity {
    pub run_id: StableId,
    pub genesis: JournalGenesis,
    verified_v2: Option<Arc<reviewgraphen_core::VerifiedV2Genesis>>,
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
        Ok(Self {
            run_id,
            genesis,
            verified_v2,
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
        match (&self.genesis, &self.verified_v2) {
            (JournalGenesis::V1(hash), None) => Ok(EventStreamGenesis::V1(hash)),
            (JournalGenesis::V2Shared(bytes), Some(verified))
                if verified.run_id() == &self.run_id
                    && verified.genesis_hash() == &ContentHash::sha256(bytes) =>
            {
                Ok(EventStreamGenesis::V2Verified(verified.as_ref()))
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
            JournalGenesis::V1(_) | JournalGenesis::V2(_) => None,
        }
    }

    fn local_metadata_capacity(&self) -> usize {
        self.run_id.allocated_bytes()
            + match &self.genesis {
                JournalGenesis::V1(hash) => hash.allocated_bytes(),
                JournalGenesis::V2(_) | JournalGenesis::V2Shared(_) => 0,
            }
    }

    fn shared_certificate_capacity(&self) -> usize {
        self.verified_v2.as_ref().map_or(0, |verified| {
            verified
                .allocated_bytes()
                .saturating_add(std::mem::size_of::<usize>() * 2)
        })
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
    #[error(
        "journal needs recovery at byte offset {good_offset}; auto_recoverable={auto_recoverable}"
    )]
    CorruptNeedsRecovery {
        good_offset: u64,
        auto_recoverable: bool,
    },
    #[error("V2 durable logs require a sequence-one RunGenesisManifest")]
    V2GenesisRequired,
    #[error("journal writer is poisoned after failed durable rollback")]
    Poisoned,
    #[error("journal receipt collision or invalid receipt at {name}")]
    ReceiptCorruption { name: String },
    #[error("journal operation exceeded {limit} bytes/events after observing {observed}")]
    Incomplete { limit: u64, observed: u64 },
}

/// Confirmation returned only after the appended line has reached `sync_data`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalAppendReceipt {
    pub sequence: u64,
    pub event_hash: ContentHash,
    pub tail_offset: u64,
}

/// Intent written before mutating a torn tail.  The serialized form is
/// canonical and create-only, so a recovery operation itself is auditable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryIntent {
    pub recovery_id: StableId,
    pub run_id: StableId,
    pub genesis_hash: ContentHash,
    /// A kernel-generated nonce makes every recovery attempt a new receipt.
    pub nonce: String,
    pub good_offset: u64,
    pub discarded_hash: ContentHash,
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
pub struct JournalWriter {
    file: File,
    identity: JournalIdentity,
    limits: JournalLimits,
    state: ScanState,
    intents: OwnedFd,
    completions: OwnedFd,
    run: OwnedFd,
    poisoned: bool,
    #[cfg(test)]
    faults: std::collections::VecDeque<AppendFault>,
}

/// A deterministic, per-writer failure point used only by the journal's
/// contract tests.  Keeping the injector on the writer rather than in global
/// state makes concurrent tests independent and documents the exact durable
/// boundary being exercised.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AppendFault {
    PartialWrite,
    Flush,
    SyncData,
    Truncate,
    RollbackSync,
    IntentPublish,
    ClearMarkerDirectorySync,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryFault {
    MarkerLogSync,
    ClearMarkerDirectorySync,
}

#[derive(Clone, Debug)]
struct ScanState {
    events: Vec<EventEnvelope>,
    confirmed_offset: u64,
    tail_hash: ContentHash,
    torn: Option<TornTail>,
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

impl<'a> EventJournal<'a> {
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
            if self.identity.version() == EventContractVersion::V2 && intent.good_offset == 0 {
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
            if self.identity.version() == EventContractVersion::V2 && intent.good_offset == 0 {
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
        if self.identity.version() == EventContractVersion::V1 {
            return Err(JournalError::V1ReadOnly);
        }
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
            #[cfg(test)]
            faults: std::collections::VecDeque::new(),
        })
    }
    pub fn recover(
        &self,
        actor: impl Into<String>,
        tool_version: impl Into<String>,
    ) -> Result<JournalRecoveryReceipt, JournalError> {
        if self.identity.version() == EventContractVersion::V1 {
            return Err(JournalError::V1ReadOnly);
        }
        let actor = actor.into();
        let tool_version = tool_version.into();
        if actor.trim().is_empty()
            || tool_version.trim().is_empty()
            || actor.len() > 1024
            || tool_version.len() > 1024
        {
            return Err(JournalError::Identity(
                "recovery actor and tool version must be non-empty",
            ));
        }
        let fd = self.open_file(true)?;
        fs::flock(&fd, FlockOperation::LockExclusive).map_err(StoreError::Io)?;
        let mut file = File::from(fd);
        let (intents, completions) = self.recovery_dirs()?;
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
            let actual_len = file.metadata()?.len();
            limit(actual_len, self.limits.max_replay_bytes)?;
            if actual_len < intent.good_offset {
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
                if ContentHash::sha256(&suffix) != intent.discarded_hash {
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
            self.clear_append_marker_if_present()?;
            return Ok(JournalRecoveryReceipt {
                intent,
                completion,
                resumed: true,
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
                    pre_tail_hash: marker_state.tail_hash,
                    actor: actor.clone(),
                    tool_version: tool_version.clone(),
                    timestamp_unix_seconds: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| JournalError::Identity("system time before Unix epoch"))?
                        .as_secs(),
                };
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
            pre_tail_hash: torn.pre_tail_hash,
            actor,
            tool_version,
            timestamp_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| JournalError::Identity("system time before Unix epoch"))?
                .as_secs(),
        };
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
    /// Appends exactly one canonical JSON object and newline after validating
    /// the entire candidate prefix through core.  A failure before durable
    /// confirmation rolls back to the previously scanned byte offset.
    pub fn append(
        &mut self,
        envelope: EventEnvelope,
    ) -> Result<JournalAppendReceipt, JournalError> {
        if self.poisoned {
            return Err(JournalError::Poisoned);
        }
        let mut line = envelope.canonical_bytes()?;
        line.push(b'\n');
        limit(line.len() as u64, self.limits.max_event_line_bytes)?;
        let mut candidate = self.state.events.clone();
        candidate.push(envelope.clone());
        limit(
            u64::try_from(candidate.len()).map_err(|_| JournalError::Incomplete {
                limit: self.limits.max_events,
                observed: u64::MAX,
            })?,
            self.limits.max_events,
        )?;
        let candidate_end = self
            .state
            .confirmed_offset
            .checked_add(
                u64::try_from(line.len()).map_err(|_| JournalError::Incomplete {
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
        let write_result = self.write_candidate(&line);
        if let Err(error) = write_result {
            // Publish the exact suffix *before* attempting the destructive
            // rollback.  Thus a crash after any unacknowledged write/flush/
            // sync boundary is discoverable on the next open.
            let intent = match self.record_rollback_intent(pre) {
                Ok(intent) => intent,
                Err(_) => {
                    self.poisoned = true;
                    return Err(JournalError::Poisoned);
                }
            };
            if self.rollback(pre).is_err() {
                self.poisoned = true;
                return Err(JournalError::Poisoned);
            }
            if self.complete_rollback_intent(&intent).is_err() {
                self.poisoned = true;
                return Err(JournalError::Poisoned);
            }
            self.clear_append_marker()?;
            return Err(JournalError::Io(error));
        }
        self.state.confirmed_offset = pre
            .checked_add(
                u64::try_from(line.len()).map_err(|_| JournalError::Incomplete {
                    limit: u64::MAX,
                    observed: u64::MAX,
                })?,
            )
            .ok_or(JournalError::Incomplete {
                limit: u64::MAX,
                observed: u64::MAX,
            })?;
        self.state.tail_hash = envelope.event_hash().clone();
        self.state.events = candidate;
        if let Err(error) = self.clear_append_marker() {
            // The event reached sync_data. Never leave this writer pointing
            // at the old offset if marker cleanup durability is unknown.
            self.poisoned = true;
            return Err(error);
        }
        Ok(JournalAppendReceipt {
            sequence: envelope.sequence(),
            event_hash: envelope.event_hash().clone(),
            tail_offset: self.state.confirmed_offset,
        })
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
            pre_tail_hash: self.state.tail_hash.clone(),
            actor: "reviewgraphen-store".to_owned(),
            tool_version: "append-rollback".to_owned(),
            timestamp_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| JournalError::Identity("system time before Unix epoch"))?
                .as_secs(),
        };
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
        #[cfg(test)]
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

    #[cfg(test)]
    fn inject_faults(&mut self, faults: impl IntoIterator<Item = AppendFault>) {
        self.faults.extend(faults);
    }

    #[cfg(test)]
    fn take_fault(&mut self, expected: AppendFault) -> bool {
        self.faults.front().copied() == Some(expected) && self.faults.pop_front().is_some()
    }
}

#[cfg(test)]
fn injected_io_error(operation: &'static str) -> std::io::Error {
    std::io::Error::other(format!("test-only injected {operation} failure"))
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

    let mut wanted = std::collections::BTreeMap::new();
    for (intent, completion) in completed_receipts {
        if wanted
            .insert(intent.good_offset, (intent, completion))
            .is_some()
        {
            return Err(IndexReplayError::Journal(JournalError::ReceiptCorruption {
                name: receipt_name(&intent.recovery_id),
            }));
        }
    }
    let mut digest = Sha256::new();
    let mut offset = 0_u64;
    let mut count = 0_u64;
    let mut tail = chain_genesis(identity);
    let canonical_scratch_capacity = canonical_scratch;
    if let Some((intent, completion)) = wanted.remove(&0)
        && (intent.pre_tail_hash != tail || completion.post_file_hash != ContentHash::sha256(b""))
    {
        return Err(IndexReplayError::Journal(JournalError::ReceiptCorruption {
            name: receipt_name(&intent.recovery_id),
        }));
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
            if let Some((intent, completion)) = wanted.remove(&boundary) {
                let actual = ContentHash::parse(format!("sha256:{:x}", digest.clone().finalize()))
                    .map_err(JournalError::from)
                    .map_err(IndexReplayError::Journal)?;
                if intent.pre_tail_hash != tail || completion.post_file_hash != actual {
                    return Err(IndexReplayError::Journal(JournalError::ReceiptCorruption {
                        name: receipt_name(&intent.recovery_id),
                    }));
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
    if identity.version() == EventContractVersion::V2 && count == 0 {
        return Err(IndexReplayError::Journal(JournalError::V2GenesisRequired));
    }
    if let Some((_, (intent, _))) = wanted.into_iter().next() {
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
        if reject_empty_v2 && identity.version() == EventContractVersion::V2 && events.is_empty() {
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
    if reject_empty_v2 && identity.version() == EventContractVersion::V2 && events.is_empty() {
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

#[cfg(test)]
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
    if intent.recovery_id.kind() != "recovery"
        || intent.run_id != identity.run_id
        || intent.genesis_hash != identity.genesis_hash()
        || (identity.version() == EventContractVersion::V2 && intent.good_offset == 0)
        || intent.nonce.len() != 64
        || !intent.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        || intent.actor.is_empty()
        || intent.actor.len() > 1024
        || intent.tool_version.is_empty()
        || intent.tool_version.len() > 1024
        || intent.good_offset > limits.max_replay_bytes
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
        ArtifactRegistered, ArtifactSensitivity, ArtifactSource, EventCommand, EventLog,
        MvpRulePack, ObligationLifecycle, PlanBudget, ProgramSpace, ReviewAggregate,
        SnapshotSourceRecordEntry, SnapshotSourcesRecorded, plan, prepare_context,
    };
    use serde_json::Value;
    use std::{
        collections::BTreeMap,
        os::unix::fs::PermissionsExt,
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

    fn root() -> (tempfile::TempDir, StoreRoot) {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), super::super::StoreLimits::default()).unwrap();
        (workspace, root)
    }

    fn journal_dir(root: &StoreRoot) -> std::path::PathBuf {
        root.path()
            .join(RUNS_DIR)
            .join(run_dir_name(&StableId::parse("run:journal-test").unwrap()))
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
}
