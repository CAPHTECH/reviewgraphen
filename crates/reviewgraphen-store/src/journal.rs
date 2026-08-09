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
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    os::fd::AsFd,
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(test)]
use std::sync::Mutex;
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
    /// Exact canonical `RunGenesisSnapshot` bytes for a V2 stream.
    V2(Vec<u8>),
}

impl JournalGenesis {
    fn version(&self) -> EventContractVersion {
        match self {
            Self::V1(_) => EventContractVersion::V1,
            Self::V2(_) => EventContractVersion::V2,
        }
    }
    fn hash(&self) -> ContentHash {
        match self {
            Self::V1(hash) => hash.clone(),
            Self::V2(bytes) => ContentHash::sha256(bytes),
        }
    }
    fn core_genesis(&self) -> EventStreamGenesis<'_> {
        match self {
            Self::V1(hash) => EventStreamGenesis::V1(hash),
            Self::V2(bytes) => EventStreamGenesis::V2(bytes),
        }
    }
}

/// The one logical event stream owned by a durable JSONL file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalIdentity {
    pub run_id: StableId,
    pub genesis: JournalGenesis,
}

impl JournalIdentity {
    pub fn new(run_id: StableId, genesis: JournalGenesis) -> Result<Self, JournalError> {
        if run_id.kind() != "run" {
            return Err(JournalError::Identity("journal requires a run StableId"));
        }
        if let JournalGenesis::V2(bytes) = &genesis {
            // Strict decoding here means no path can initialize a V2 journal
            // with merely hash-shaped, noncanonical genesis bytes.
            let _ = reviewgraphen_core::RunGenesisSnapshot::from_canonical_bytes(bytes)?;
        }
        Ok(Self { run_id, genesis })
    }
    #[must_use]
    pub fn version(&self) -> EventContractVersion {
        self.genesis.version()
    }
    #[must_use]
    pub fn genesis_hash(&self) -> ContentHash {
        self.genesis.hash()
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
        let mut line = canonical_json(&first_manifest_envelope)?;
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
    pub fn reader(&self) -> Result<JournalReader, JournalError> {
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
        let state = scan(&mut file, &self.identity, self.limits, true)?;
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
            reserve_recovery_capacity(
                &audit,
                self.limits,
                &[receipt_bytes(&completion)?],
            )?;
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
                    return Err(JournalError::Io(injected_io_error("marker recovery log sync")));
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
                    return Err(JournalError::Io(injected_io_error("marker recovery log sync")));
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
        let mut line = canonical_json(&envelope)?;
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
        let audit = recovery_audit(&self.intents, &self.completions, &self.identity, self.limits)?;
        sync_recovery_dirs(&self.intents, &self.completions)?;
        let rollback_sizes = rollback_receipt_size_bound(&self.identity, pre, &self.state.tail_hash)?;
        reserve_recovery_capacity(
            &audit,
            self.limits,
            &rollback_sizes,
        )?;
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

fn scan(
    file: &mut File,
    identity: &JournalIdentity,
    limits: JournalLimits,
    reject_empty_v2: bool,
) -> Result<ScanState, JournalError> {
    let state = scan_with_torn(file, identity, limits, reject_empty_v2)?;
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
        bytes.extend_from_slice(&buf[..n]);
    }
    scan_bytes(&bytes, identity, limits, reject_empty_v2)
}

fn scan_bytes(
    bytes: &[u8],
    identity: &JournalIdentity,
    limits: JournalLimits,
    reject_empty_v2: bool,
) -> Result<ScanState, JournalError> {
    let mut events = Vec::new();
    let mut starts = Vec::new();
    let mut good = 0u64;
    let mut start = 0usize;
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
        let event = EventEnvelope::from_json_slice(&bytes[start..end]).map_err(|_| {
            JournalError::CorruptNeedsRecovery {
                good_offset: good,
                auto_recoverable: false,
            }
        })?;
        if canonical_json(&event).map_err(JournalError::Domain)? != bytes[start..end] {
            return Err(JournalError::CorruptNeedsRecovery {
                good_offset: good,
                auto_recoverable: false,
            });
        }
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
        identity.genesis.core_genesis(),
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
                canonical_json(first).ok().as_deref() == Some(&line[..line.len() - 1])
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
    let mut intents_by_id = Vec::new();
    let mut count = 0u64;
    let mut scanned = 0u64;
    for name in receipt_names(intents, limits, &mut count, &mut scanned)? {
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
    for name in receipt_names(completions, limits, &mut count, &mut scanned)? {
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
    let scanned = audit
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
        EventCommand, EventLog, MvpRulePack, ObligationLifecycle, ProgramSpace, ReviewAggregate,
    };
    use std::{
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

    /// All V2 test streams begin with the same durable, sequence-one
    /// manifest as production.  The fixture event is deliberately sequence
    /// two, so append and recovery tests never exercise an empty V2 log.
    fn first_manifest(identity: &JournalIdentity) -> EventEnvelope {
        let JournalGenesis::V2(bytes) = &identity.genesis else {
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
        let run = root.path().join(RUNS_DIR).join(run_dir_name(&identity.run_id));
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
        std::fs::set_permissions(run.join(JOURNAL_FILE), std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn log_path(root: &StoreRoot) -> std::path::PathBuf {
        journal_dir(root).join(JOURNAL_FILE)
    }
    fn recovery_path(root: &StoreRoot) -> std::path::PathBuf {
        journal_dir(root).join(RECOVERY_DIR)
    }

    fn event_line(event: &EventEnvelope) -> Vec<u8> {
        let mut line = canonical_json(event).unwrap();
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
        let journal = EventJournal::initialize_v2(&root, identity.clone(), genesis.clone()).unwrap();
        assert_eq!(std::fs::read(log_path(&root)).unwrap(), event_line(&genesis));
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
        assert!(matches!(journal.reader(), Err(JournalError::V2GenesisRequired)));
        assert!(matches!(journal.writer(), Err(JournalError::V2GenesisRequired)));
        assert!(matches!(journal.recover("test", "reviewgraphen-store@1"), Err(JournalError::V2GenesisRequired)));
    }

    #[test]
    fn distinct_runs_never_share_log_or_recovery_components() {
        let (_workspace, root) = root();
        let (first_identity, _first_event) = fixture_event_for("run:journal-a");
        let (second_identity, _second_event) = fixture_event_for("run:journal-b");
        let first_genesis = first_manifest(&first_identity);
        let second_genesis = first_manifest(&second_identity);
        EventJournal::initialize_v2(&root, first_identity.clone(), first_genesis.clone()).unwrap();
        EventJournal::initialize_v2(&root, second_identity.clone(), second_genesis.clone()).unwrap();
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
        assert!(std::fs::read_dir(recovery_path(&root).join(INTENTS_DIR))
            .unwrap()
            .next()
            .is_none());
        assert!(std::fs::read_dir(recovery_path(&root).join(COMPLETIONS_DIR))
            .unwrap()
            .next()
            .is_none());
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
        assert!(matches!(writer.append(fixture_event().1), Err(JournalError::Poisoned)));
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
        let JournalGenesis::V2(bytes) = &identity.genesis else { panic!("fixture is V2") };
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
        assert_eq!(std::fs::read(log_path(&root)).unwrap(), initialized_with_event(&journal.identity, &event));
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
            assert_eq!(std::fs::read(log_path(&root)).unwrap(), initialized_bytes(&writer.identity));
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
        assert!(matches!(EventJournal::initialize_v2_with_limits(
            &root2,
            identity.clone(),
            genesis.clone(),
            JournalLimits {
                max_event_line_bytes: genesis_len - 1,
                ..exact
            },
        ), Err(JournalError::Incomplete { .. })
        ));
        assert!(!log_path(&root2).exists());

        let (_workspace3, root3) = root();
        assert!(matches!(EventJournal::initialize_v2_with_limits(
            &root3,
            identity,
            genesis,
            JournalLimits {
                max_replay_bytes: genesis_len - 1,
                ..exact
            },
        ), Err(JournalError::Incomplete { .. })
        ));
        assert!(!log_path(&root3).exists());
        drop(journal);
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
                assert_eq!(std::fs::read(log_path(&root)).unwrap(), initialized_with_event(&journal.identity, &event));
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
        assert_eq!(std::fs::read(log_path(&root)).unwrap(), initialized_with_event(&journal.identity, &event));
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
