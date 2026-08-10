//! Linux-only durable-store root admission.
//!
//! This crate deliberately starts with the trusted root boundary. CAS, JSONL,
//! and index layers must use the returned directory descriptor rather than
//! reconstructing paths from ambient state.

#[cfg(target_os = "linux")]
mod index;
#[cfg(target_os = "linux")]
mod journal;

#[cfg(target_os = "linux")]
pub use index::{
    DerivedIndex, IndexArtifactRegistration, IndexClaim, IndexContextEnvelope, IndexError,
    IndexEvent, IndexFinding, IndexLimits, IndexMarker, IndexObligation, IndexObligationLifecycle,
    IndexProgramObject, IndexProgramRelation, IndexRebuildReceipt, IndexReviewPlan, IndexShadow,
    IndexSnapshot, IndexSnapshotSource, IndexUniverse,
};
#[cfg(target_os = "linux")]
pub use journal::{
    EventJournal, JournalAppendReceipt, JournalError, JournalGenesis, JournalIdentity,
    JournalLimits, JournalReader, JournalRecoveryReceipt, JournalWriter, RecoveryCompletion,
    RecoveryIntent, ReplayedV2RunSession,
};

#[cfg(target_os = "linux")]
use rustix::{
    fd::OwnedFd,
    fs::{
        self, AtFlags, CWD, Dir, FileType, FlockOperation, Mode, OFlags, RenameFlags, ResolveFlags,
    },
    process::{getegid, geteuid, getgid, getuid},
    rand::{GetRandomFlags, getrandom},
};
#[cfg(target_os = "linux")]
use sha2::{Digest, Sha256};
#[cfg(target_os = "linux")]
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
#[cfg(all(test, target_os = "linux"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// Resource limits reserved for subsequent CAS and journal units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoreLimits {
    /// Largest allowed CAS object in bytes.
    pub max_object_bytes: u64,
    /// Maximum entries a single temporary-GC pass may inspect.
    pub max_tmp_gc_entries: u64,
    /// Maximum apparent regular-file bytes a GC pass may scan.
    pub max_tmp_gc_scan_bytes: u64,
    /// Courtesy age floor; leases remain the correctness boundary.
    pub min_tmp_gc_age: Duration,
    /// Maximum canonical JSONL event line accepted by the durable journal.
    pub max_event_line_bytes: u64,
    /// Maximum events a single journal validation/replay may admit.
    pub max_events: u64,
    /// Maximum actual bytes a single journal validation/replay may read.
    pub max_replay_bytes: u64,
    /// Aggregate physical rows admitted into one derived SQLite index.
    pub max_index_rows: u64,
    /// Maximum serialized active SQLite image size.
    pub max_index_serialized_bytes: u64,
    /// Maximum simultaneous store-owned index buffers.
    pub max_index_working_bytes: u64,
    /// Maximum typed index snapshot returned by one query.
    pub max_index_query_bytes: u64,
    /// Maximum UTF-8 bytes in a prepared index statement.
    pub max_index_statement_bytes: u64,
}

impl Default for StoreLimits {
    fn default() -> Self {
        Self {
            max_object_bytes: 64 * 1024 * 1024,
            max_tmp_gc_entries: 10_000,
            max_tmp_gc_scan_bytes: 1024 * 1024 * 1024,
            min_tmp_gc_age: Duration::from_secs(300),
            max_event_line_bytes: 1024 * 1024,
            max_events: 1_000_000,
            max_replay_bytes: 1024 * 1024 * 1024,
            max_index_rows: 1_000_000,
            max_index_serialized_bytes: 64 * 1024 * 1024,
            // A maximum-size rebuild can temporarily own the main database,
            // SQLite's serialized view, the returned image, and its bounded
            // cache.  The descriptor publication phase starts only after the
            // connection is dropped.
            max_index_working_bytes: 256 * 1024 * 1024,
            max_index_query_bytes: 16 * 1024 * 1024,
            max_index_statement_bytes: 64 * 1024,
        }
    }
}

/// Typed store-boundary failures.
#[derive(Debug, Error)]
pub enum StoreError {
    /// The host cannot provide the FD-relative no-follow contract.
    #[error("reviewgraphen-store requires Linux FD-relative no-follow filesystem operations")]
    UnsupportedPlatform,
    /// A path or descriptor operation failed at the trusted boundary.
    #[error("store filesystem operation failed: {0}")]
    Io(#[from] rustix::io::Errno),
    /// Workspace canonicalization failed before its single anchor open.
    #[error("workspace canonicalization failed: {0}")]
    Canonicalize(#[source] std::io::Error),
    /// Streaming caller I/O failed while a temporary object was being written.
    #[error("CAS stream I/O failed: {0}")]
    Stream(#[source] std::io::Error),
    /// The process has real/effective credentials that do not agree.
    #[error("real and effective uid/gid differ; store admission refuses credential ambiguity")]
    CredentialMismatch,
    /// Ownership does not belong to the current process.
    #[error("{path} owner does not match the current uid/gid")]
    OwnerMismatch { path: PathBuf },
    /// Store directory mode is not exactly owner-only 0700.
    #[error("{path} must have mode 0700")]
    InsecureMode { path: PathBuf },
    /// The reserved store-root name is a symlink or otherwise not a directory.
    #[error("{path} is not an admissible store directory: {kind}")]
    InvalidStoreRoot { path: PathBuf, kind: &'static str },
    #[error("invalid CAS hash")]
    InvalidCasHash,
    #[error("declared object size differs from bytes read")]
    DeclaredSizeMismatch,
    #[error("object exceeds configured byte limit {limit} (observed {observed})")]
    ObjectTooLarge { limit: u64, observed: u64 },
    #[error("bounded store operation incomplete at limit {limit} (observed {observed})")]
    Incomplete { limit: u64, observed: u64 },
    #[error("object bytes do not match expected hash")]
    HashMismatch,
    #[error("CAS object is missing")]
    MissingArtifact,
    #[error("CAS object is corrupt")]
    CorruptedArtifact,
    #[error("unable to allocate a unique CAS temporary name")]
    TempNameExhausted,
    #[error("test-only simulated crash after durable temporary write")]
    SimulatedCrash,
}

/// Strict SHA-256 identifier used as a CAS path component.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct CasHash(String);

impl CasHash {
    pub fn parse(value: impl Into<String>) -> Result<Self, StoreError> {
        let value = value.into();
        if value.len() != 71
            || !value.starts_with("sha256:")
            || !value[7..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(StoreError::InvalidCasHash);
        }
        Ok(Self(value))
    }
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.0[7..9]
    }
    #[must_use]
    pub fn hex(&self) -> &str {
        &self.0[7..]
    }
}
impl std::fmt::Display for CasHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Receipt from an idempotent CAS publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CasReceipt {
    pub hash: CasHash,
    pub size: u64,
    pub existed: bool,
}

/// Result of lease-aware temporary-object garbage collection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TempGcReceipt {
    pub removed: u64,
    pub skipped_locked: u64,
    pub skipped_non_regular: u64,
}

/// Content-addressed bytes rooted exclusively at an admitted `StoreRoot` FD.
#[cfg(target_os = "linux")]
pub struct CasStore<'a> {
    root: &'a StoreRoot,
    #[allow(dead_code)] // Retained as the admitted parent descriptor of sha256.
    cas: OwnedFd,
    sha256: OwnedFd,
    tmp: OwnedFd,
}

/// Read-only opener for already-published CAS objects. Unlike `CasStore`, it
/// never creates directories, staging files, markers, or fsyncs metadata.
#[cfg(target_os = "linux")]
pub struct CasReader<'a> {
    root: &'a StoreRoot,
    sha256: OwnedFd,
}

#[cfg(target_os = "linux")]
impl<'a> CasReader<'a> {
    pub fn open_existing(root: &'a StoreRoot) -> Result<Self, StoreError> {
        let cas = open_existing_dir(root.fd(), "artifacts", "CAS artifacts directory")?;
        let sha256 = open_existing_dir(&cas, "sha256", "CAS SHA-256 directory")?;
        Ok(Self { root, sha256 })
    }

    pub fn read(&self, hash: &CasHash) -> Result<Vec<u8>, StoreError> {
        let mut bytes = Vec::new();
        self.read_into(hash, None, &mut bytes)?;
        Ok(bytes)
    }

    /// Reads a verified object into caller-owned storage. The caller can
    /// reserve its bounded capacity before this method opens the object.
    pub fn read_into(
        &self,
        hash: &CasHash,
        expected_size: Option<u64>,
        bytes: &mut Vec<u8>,
    ) -> Result<(), StoreError> {
        if !bytes.is_empty() {
            return Err(StoreError::CorruptedArtifact);
        }
        let parent = fs::openat(
            &self.sha256,
            hash.prefix(),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(map_artifact_open_error)?;
        verify_artifact_fd(&parent, FileType::Directory, 0o700)?;
        let entry = fs::statat(&parent, hash.hex(), AtFlags::SYMLINK_NOFOLLOW)
            .map_err(map_artifact_open_error)?;
        verify_artifact_stat(&entry, FileType::RegularFile, 0o600)?;
        let fd = fs::openat(
            &parent,
            hash.hex(),
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(map_artifact_open_error)?;
        verify_artifact_fd(&fd, FileType::RegularFile, 0o600)?;
        let opened = fs::fstat(&fd)?;
        if opened.st_dev != entry.st_dev || opened.st_ino != entry.st_ino {
            return Err(StoreError::CorruptedArtifact);
        }
        let observed_size =
            u64::try_from(opened.st_size).map_err(|_| StoreError::CorruptedArtifact)?;
        if observed_size > self.root.limits.max_object_bytes
            || expected_size.is_some_and(|expected| expected != observed_size)
        {
            return Err(StoreError::CorruptedArtifact);
        }
        let expected_len =
            usize::try_from(observed_size).map_err(|_| StoreError::CorruptedArtifact)?;
        let admitted_capacity = bytes.capacity();
        if admitted_capacity < expected_len {
            return Err(StoreError::Incomplete {
                limit: u64::try_from(admitted_capacity).unwrap_or(u64::MAX),
                observed: observed_size,
            });
        }
        bytes.resize(expected_len, 0);
        let mut file = std::fs::File::from(fd);
        file.read_exact(bytes).map_err(StoreError::Stream)?;
        let mut trailing = [0u8; 1];
        if file.read(&mut trailing).map_err(StoreError::Stream)? != 0
            || bytes.capacity() != admitted_capacity
        {
            return Err(StoreError::CorruptedArtifact);
        }
        if u64::try_from(bytes.len()).ok() != Some(observed_size) {
            return Err(StoreError::CorruptedArtifact);
        }
        let actual = CasHash::parse(format!("sha256:{:x}", Sha256::digest(&*bytes)))?;
        if &actual != hash {
            return Err(StoreError::CorruptedArtifact);
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl<'a> CasStore<'a> {
    const VERIFY_CHUNK_BYTES: usize = 64 * 1024;

    pub(crate) const fn verification_chunk_capacity(expected_len: usize) -> usize {
        if expected_len < Self::VERIFY_CHUNK_BYTES {
            expected_len
        } else {
            Self::VERIFY_CHUNK_BYTES
        }
    }

    pub fn open(root: &'a StoreRoot) -> Result<Self, StoreError> {
        // Each component is created/opened/verified independently.  In
        // particular, no slash-containing path is ever handed to *at.
        let cas = open_or_create_dir(root.fd(), "artifacts", "CAS artifacts directory")?;
        let sha256 = open_or_create_dir(&cas, "sha256", "CAS SHA-256 directory")?;
        let tmp = open_or_create_dir(&cas, "tmp", "CAS temporary directory")?;
        Ok(Self {
            root,
            cas,
            sha256,
            tmp,
        })
    }
    pub fn put<R: Read>(
        &self,
        expected: &CasHash,
        declared_size: Option<u64>,
        mut input: R,
    ) -> Result<CasReceipt, StoreError> {
        if let Some(observed) = declared_size.filter(|n| *n > self.root.limits.max_object_bytes) {
            return Err(StoreError::ObjectTooLarge {
                limit: self.root.limits.max_object_bytes,
                observed,
            });
        }
        let (temp_name, temp) = self.create_temp()?;
        let mut file = std::fs::File::from(temp);
        let result = (|| {
            let mut digest = Sha256::new();
            let mut size = 0u64;
            let mut buf = [0u8; 8192];
            loop {
                let read = input.read(&mut buf).map_err(StoreError::Stream)?;
                if read == 0 {
                    break;
                }
                size = size
                    .checked_add(read as u64)
                    .ok_or(StoreError::ObjectTooLarge {
                        limit: self.root.limits.max_object_bytes,
                        observed: u64::MAX,
                    })?;
                if size > self.root.limits.max_object_bytes {
                    return Err(StoreError::ObjectTooLarge {
                        limit: self.root.limits.max_object_bytes,
                        observed: size,
                    });
                }
                digest.update(&buf[..read]);
                file.write_all(&buf[..read]).map_err(StoreError::Stream)?;
            }
            if declared_size.is_some_and(|n| n != size) {
                return Err(StoreError::DeclaredSizeMismatch);
            }
            let actual = CasHash::parse(format!("sha256:{:x}", digest.finalize()))?;
            if &actual != expected {
                return Err(StoreError::HashMismatch);
            }
            file.sync_all().map_err(StoreError::Stream)?;
            if crash_after_temp_sync() {
                return Err(StoreError::SimulatedCrash);
            }
            let prefix =
                open_or_create_dir(&self.sha256, expected.prefix(), "CAS prefix directory")?;
            // linkat is a true create-only publication: EEXIST is never an
            // overwrite race, unlike a preflight existence check plus rename.
            match self.publish_create_only(&temp_name, &prefix, expected.hex()) {
                Ok(()) => {
                    fs::fsync(&prefix)?;
                    Ok(CasReceipt {
                        hash: actual,
                        size,
                        existed: false,
                    })
                }
                Err(rustix::io::Errno::EXIST) => {
                    let existing = self.read(expected)?;
                    if existing.len() as u64 != size {
                        return Err(StoreError::CorruptedArtifact);
                    }
                    Ok(CasReceipt {
                        hash: actual,
                        size,
                        existed: true,
                    })
                }
                Err(error) => Err(map_publish_error(error)),
            }
        })();
        if matches!(result, Err(StoreError::SimulatedCrash)) {
            drop(file);
            return result;
        }
        // The lease remains associated with `file` through every publish or
        // error path.  Removing the directory entry only happens after the
        // result has been determined, so an abandoned write is never visible.
        let cleanup = fs::unlinkat(&self.tmp, &temp_name, AtFlags::empty());
        let cleanup = cleanup.and_then(|()| fs::fsync(&self.tmp));
        // Keep the lease held until the temporary name is gone.  A concurrent
        // GC may then observe either the locked temp or no temp, never an
        // unlocked in-flight writer.
        drop(file);
        match (result, cleanup) {
            (Ok(receipt), Ok(())) => Ok(receipt),
            (Ok(receipt), Err(rustix::io::Errno::NOENT)) => Ok(receipt),
            (Ok(_), Err(error)) => Err(StoreError::Io(error)),
            (Err(error), _) => Err(error),
        }
    }
    pub fn read(&self, hash: &CasHash) -> Result<Vec<u8>, StoreError> {
        let fd = self.open_verified_object(hash)?;
        let mut bytes = Vec::new();
        std::fs::File::from(fd)
            .take(self.root.limits.max_object_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(StoreError::Stream)?;
        if bytes.len() as u64 > self.root.limits.max_object_bytes {
            return Err(StoreError::CorruptedArtifact);
        }
        let actual = CasHash::parse(format!("sha256:{:x}", Sha256::digest(&bytes)))?;
        if &actual != hash {
            return Err(StoreError::CorruptedArtifact);
        }
        Ok(bytes)
    }

    /// Verify a CAS object against an already-retained byte slice without
    /// materializing a second full object. The only requested store buffer is
    /// a fixed-capacity chunk of at most 64 KiB.
    pub(crate) fn verify_exact_bytes_streaming(
        &self,
        hash: &CasHash,
        expected: &[u8],
    ) -> Result<(), StoreError> {
        let fd = self.open_verified_object(hash)?;
        let stat = fs::fstat(&fd)?;
        let expected_len =
            u64::try_from(expected.len()).map_err(|_| StoreError::CorruptedArtifact)?;
        let observed_len =
            u64::try_from(stat.st_size).map_err(|_| StoreError::CorruptedArtifact)?;
        if observed_len != expected_len || observed_len > self.root.limits.max_object_bytes {
            return Err(StoreError::CorruptedArtifact);
        }

        let chunk_capacity = Self::verification_chunk_capacity(expected.len());
        let mut chunk = Vec::new();
        chunk
            .try_reserve_exact(chunk_capacity)
            .map_err(|error| StoreError::Stream(std::io::Error::other(error)))?;
        chunk.resize(chunk_capacity, 0);
        let mut file = std::fs::File::from(fd);
        let mut digest = Sha256::new();
        let mut offset = 0usize;
        while offset < expected.len() {
            let wanted = (expected.len() - offset).min(chunk.len());
            file.read_exact(&mut chunk[..wanted])
                .map_err(StoreError::Stream)?;
            if chunk[..wanted] != expected[offset..offset + wanted] {
                return Err(StoreError::CorruptedArtifact);
            }
            digest.update(&chunk[..wanted]);
            offset += wanted;
        }
        let mut trailing = [0u8; 1];
        if file.read(&mut trailing).map_err(StoreError::Stream)? != 0 {
            return Err(StoreError::CorruptedArtifact);
        }
        let actual = CasHash::parse(format!("sha256:{:x}", digest.finalize()))?;
        if &actual != hash {
            return Err(StoreError::CorruptedArtifact);
        }
        Ok(())
    }

    fn open_verified_object(&self, hash: &CasHash) -> Result<OwnedFd, StoreError> {
        let parent = fs::openat(
            &self.sha256,
            hash.prefix(),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(map_artifact_open_error)?;
        verify_artifact_fd(&parent, FileType::Directory, 0o700)?;
        // Check the directory entry without following it before opening. This
        // rejects FIFOs/devices without ever issuing a potentially blocking
        // open, while O_NONBLOCK and the identity check close the replacement
        // race between statat and openat.
        let entry_stat = fs::statat(&parent, hash.hex(), AtFlags::SYMLINK_NOFOLLOW)
            .map_err(map_artifact_open_error)?;
        verify_artifact_stat(&entry_stat, FileType::RegularFile, 0o600)?;
        let fd = fs::openat(
            &parent,
            hash.hex(),
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(map_artifact_open_error)?;
        verify_artifact_fd(&fd, FileType::RegularFile, 0o600)?;
        let opened_stat = fs::fstat(&fd)?;
        if opened_stat.st_dev != entry_stat.st_dev || opened_stat.st_ino != entry_stat.st_ino {
            return Err(StoreError::CorruptedArtifact);
        }
        Ok(fd)
    }

    /// Delete only regular staging files for which this process can acquire
    /// the writer lease.  A held lease always wins over any future age policy.
    pub fn gc_tmp(&self) -> Result<TempGcReceipt, StoreError> {
        let mut receipt = TempGcReceipt::default();
        let mut entries = Dir::new(rustix::io::dup(&self.tmp)?)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::Incomplete {
                limit: 0,
                observed: 0,
            })?
            .as_secs();
        let mut scanned_entries = 0u64;
        let mut scanned_bytes = 0u64;
        for entry in &mut entries {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_bytes() == b"." || name.to_bytes() == b".." {
                continue;
            }
            scanned_entries = scanned_entries
                .checked_add(1)
                .ok_or(StoreError::Incomplete {
                    limit: self.root.limits.max_tmp_gc_entries,
                    observed: u64::MAX,
                })?;
            if scanned_entries > self.root.limits.max_tmp_gc_entries {
                return Err(StoreError::Incomplete {
                    limit: self.root.limits.max_tmp_gc_entries,
                    observed: scanned_entries,
                });
            }
            let stat = match fs::statat(&self.tmp, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => stat,
                Err(rustix::io::Errno::NOENT) => continue,
                Err(error) => return Err(StoreError::Io(error)),
            };
            if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
                receipt.skipped_non_regular += 1;
                continue;
            }
            verify_temp_stat(&stat)?;
            let apparent_size =
                u64::try_from(stat.st_size).map_err(|_| StoreError::CorruptedArtifact)?;
            scanned_bytes =
                scanned_bytes
                    .checked_add(apparent_size)
                    .ok_or(StoreError::Incomplete {
                        limit: self.root.limits.max_tmp_gc_scan_bytes,
                        observed: u64::MAX,
                    })?;
            if scanned_bytes > self.root.limits.max_tmp_gc_scan_bytes {
                return Err(StoreError::Incomplete {
                    limit: self.root.limits.max_tmp_gc_scan_bytes,
                    observed: scanned_bytes,
                });
            }
            // The min-age policy prevents a just-created name from becoming
            // GC eligible before its writer has acquired the lease.
            if now.saturating_sub(stat.st_mtime as u64) < self.root.limits.min_tmp_gc_age.as_secs()
            {
                continue;
            }
            let fd = match fs::openat(
                &self.tmp,
                name,
                OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            ) {
                Ok(fd) => fd,
                Err(
                    rustix::io::Errno::NOENT
                    | rustix::io::Errno::LOOP
                    | rustix::io::Errno::NOTDIR
                    | rustix::io::Errno::ISDIR,
                ) => {
                    receipt.skipped_non_regular += 1;
                    continue;
                }
                Err(error) => return Err(StoreError::Io(error)),
            };
            let opened_stat = fs::fstat(&fd)?;
            if FileType::from_raw_mode(opened_stat.st_mode) != FileType::RegularFile
                || opened_stat.st_dev != stat.st_dev
                || opened_stat.st_ino != stat.st_ino
            {
                receipt.skipped_non_regular += 1;
                continue;
            }
            verify_fd_kind_mode(&fd, "CAS temporary object", FileType::RegularFile, 0o600)?;
            match fs::flock(&fd, FlockOperation::NonBlockingLockExclusive) {
                Ok(()) => match fs::unlinkat(&self.tmp, name, AtFlags::empty()) {
                    Ok(()) => receipt.removed += 1,
                    Err(rustix::io::Errno::NOENT) => {}
                    Err(error) => return Err(StoreError::Io(error)),
                },
                Err(rustix::io::Errno::WOULDBLOCK) => receipt.skipped_locked += 1,
                Err(error) => return Err(StoreError::Io(error)),
            }
        }
        Ok(receipt)
    }

    fn create_temp(&self) -> Result<(String, OwnedFd), StoreError> {
        const TEMP_NAME_ATTEMPTS: u64 = 32;
        let temp = fs::openat(
            &self.tmp,
            ".",
            OFlags::TMPFILE | OFlags::RDWR | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(map_tmpfile_error)?;
        verify_fd_kind_mode(&temp, "temporary CAS object", FileType::RegularFile, 0o600)?;
        fs::flock(&temp, FlockOperation::NonBlockingLockExclusive)?;
        for _ in 0..TEMP_NAME_ATTEMPTS {
            let name = temp_name()?;
            match fs::linkat(&temp, "", &self.tmp, &name, AtFlags::EMPTY_PATH) {
                Ok(()) => return Ok((name, temp)),
                Err(rustix::io::Errno::EXIST) => continue,
                Err(error) => return Err(map_tmpfile_error(error)),
            }
        }
        Err(StoreError::TempNameExhausted)
    }

    fn publish_create_only(
        &self,
        temp_name: &str,
        prefix: &OwnedFd,
        object_name: &str,
    ) -> Result<(), rustix::io::Errno> {
        match fs::linkat(&self.tmp, temp_name, prefix, object_name, AtFlags::empty()) {
            Ok(()) => Ok(()),
            Err(
                rustix::io::Errno::NOSYS | rustix::io::Errno::OPNOTSUPP | rustix::io::Errno::PERM,
            ) => fs::renameat_with(
                &self.tmp,
                temp_name,
                prefix,
                object_name,
                RenameFlags::NOREPLACE,
            ),
            Err(error) => Err(error),
        }
    }
}

#[cfg(target_os = "linux")]
fn map_publish_error(error: rustix::io::Errno) -> StoreError {
    if matches!(
        error,
        rustix::io::Errno::NOSYS | rustix::io::Errno::OPNOTSUPP | rustix::io::Errno::INVAL
    ) {
        StoreError::UnsupportedPlatform
    } else {
        StoreError::Io(error)
    }
}

#[cfg(target_os = "linux")]
fn map_tmpfile_error(error: rustix::io::Errno) -> StoreError {
    if matches!(
        error,
        rustix::io::Errno::NOSYS | rustix::io::Errno::OPNOTSUPP | rustix::io::Errno::INVAL
    ) {
        StoreError::UnsupportedPlatform
    } else {
        StoreError::Io(error)
    }
}

#[cfg(all(test, target_os = "linux"))]
static CRASH_AFTER_TEMP_SYNC: AtomicBool = AtomicBool::new(false);

#[cfg(all(test, target_os = "linux"))]
fn crash_after_temp_sync() -> bool {
    CRASH_AFTER_TEMP_SYNC.swap(false, Ordering::SeqCst)
}

#[cfg(all(not(test), target_os = "linux"))]
fn crash_after_temp_sync() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn temp_name() -> Result<String, StoreError> {
    let mut bytes = [0u8; 24];
    getrandom(&mut bytes, GetRandomFlags::empty())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
#[cfg(target_os = "linux")]
fn open_or_create_dir(
    parent: &OwnedFd,
    name: &str,
    label: &'static str,
) -> Result<OwnedFd, StoreError> {
    let created = match fs::mkdirat(parent, name, Mode::from_raw_mode(0o700)) {
        Ok(()) => true,
        Err(rustix::io::Errno::EXIST) => false,
        Err(error) => return Err(StoreError::Io(error)),
    };
    if created {
        fs::fsync(parent)?;
    }
    let fd = fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    verify_fd_kind_mode(&fd, label, FileType::Directory, 0o700)?;
    Ok(fd)
}

#[cfg(target_os = "linux")]
fn open_existing_dir(
    parent: &OwnedFd,
    name: &str,
    label: &'static str,
) -> Result<OwnedFd, StoreError> {
    let fd = fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| {
        if error == rustix::io::Errno::NOENT {
            StoreError::MissingArtifact
        } else {
            StoreError::Io(error)
        }
    })?;
    verify_fd_kind_mode(&fd, label, FileType::Directory, 0o700)?;
    Ok(fd)
}

#[cfg(target_os = "linux")]
fn verify_fd_kind_mode(
    fd: &OwnedFd,
    path: &'static str,
    expected_kind: FileType,
    expected_mode: u32,
) -> Result<(), StoreError> {
    let stat = fs::fstat(fd)?;
    if FileType::from_raw_mode(stat.st_mode) != expected_kind {
        return Err(StoreError::InvalidStoreRoot {
            path: PathBuf::from(path),
            kind: "unexpected filesystem object type",
        });
    }
    if stat.st_uid != geteuid().as_raw() || stat.st_gid != getegid().as_raw() {
        return Err(StoreError::OwnerMismatch {
            path: PathBuf::from(path),
        });
    }
    if Mode::from_raw_mode(stat.st_mode).as_raw_mode() & 0o7777 != expected_mode {
        return Err(StoreError::InsecureMode {
            path: PathBuf::from(path),
        });
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn map_artifact_open_error(error: rustix::io::Errno) -> StoreError {
    if error == rustix::io::Errno::NOENT {
        StoreError::MissingArtifact
    } else if error == rustix::io::Errno::LOOP || error == rustix::io::Errno::NOTDIR {
        StoreError::CorruptedArtifact
    } else {
        StoreError::Io(error)
    }
}

#[cfg(target_os = "linux")]
fn verify_artifact_fd(
    fd: &OwnedFd,
    expected_kind: FileType,
    expected_mode: u32,
) -> Result<(), StoreError> {
    match verify_fd_kind_mode(fd, "CAS object", expected_kind, expected_mode) {
        Ok(()) => Ok(()),
        Err(
            StoreError::InvalidStoreRoot { .. }
            | StoreError::OwnerMismatch { .. }
            | StoreError::InsecureMode { .. },
        ) => Err(StoreError::CorruptedArtifact),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "linux")]
fn verify_artifact_stat(
    stat: &fs::Stat,
    expected_kind: FileType,
    expected_mode: u32,
) -> Result<(), StoreError> {
    if FileType::from_raw_mode(stat.st_mode) != expected_kind
        || stat.st_uid != geteuid().as_raw()
        || stat.st_gid != getegid().as_raw()
        || Mode::from_raw_mode(stat.st_mode).as_raw_mode() & 0o7777 != expected_mode
    {
        Err(StoreError::CorruptedArtifact)
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn verify_temp_stat(stat: &fs::Stat) -> Result<(), StoreError> {
    if stat.st_uid != geteuid().as_raw() || stat.st_gid != getegid().as_raw() {
        return Err(StoreError::OwnerMismatch {
            path: PathBuf::from("CAS temporary object"),
        });
    }
    if Mode::from_raw_mode(stat.st_mode).as_raw_mode() & 0o7777 != 0o600 {
        return Err(StoreError::InsecureMode {
            path: PathBuf::from("CAS temporary object"),
        });
    }
    Ok(())
}

/// An admitted `.reviewgraphen` directory anchored by a directory descriptor.
#[derive(Debug)]
pub struct StoreRoot {
    #[cfg(target_os = "linux")]
    #[allow(dead_code)] // Retained for the following CAS/journal subunits.
    fd: OwnedFd,
    display_path: PathBuf,
    limits: StoreLimits,
}

impl StoreRoot {
    /// Canonicalizes and anchors `workspace` once, then opens or creates its
    /// `.reviewgraphen` child solely with `*at` operations and `O_NOFOLLOW`.
    #[cfg(target_os = "linux")]
    pub fn open(workspace: impl AsRef<Path>, limits: StoreLimits) -> Result<Self, StoreError> {
        ensure_unambiguous_credentials(
            getuid().as_raw(),
            geteuid().as_raw(),
            getgid().as_raw(),
            getegid().as_raw(),
        )?;
        if std::fs::symlink_metadata(workspace.as_ref())
            .map_err(StoreError::Canonicalize)?
            .file_type()
            .is_symlink()
        {
            return Err(StoreError::InvalidStoreRoot {
                path: workspace.as_ref().to_owned(),
                kind: "workspace symlink",
            });
        }
        let workspace = std::fs::canonicalize(workspace).map_err(StoreError::Canonicalize)?;
        let workspace_fd = fs::openat2(
            CWD,
            &workspace,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS,
        )
        .map_err(map_openat2_error)?;
        verify_owner(&workspace_fd, &workspace)?;
        let root_created =
            match fs::mkdirat(&workspace_fd, ".reviewgraphen", Mode::from_raw_mode(0o700)) {
                Ok(()) => true,
                Err(rustix::io::Errno::EXIST) => false,
                Err(error) => return Err(StoreError::Io(error)),
            };
        if root_created {
            fs::fsync(&workspace_fd)?;
        }
        let display_path = workspace.join(".reviewgraphen");
        let root_stat = fs::statat(&workspace_fd, ".reviewgraphen", AtFlags::SYMLINK_NOFOLLOW)?;
        match FileType::from_raw_mode(root_stat.st_mode) {
            FileType::Directory => {}
            FileType::Symlink => {
                return Err(StoreError::InvalidStoreRoot {
                    path: display_path,
                    kind: "symlink",
                });
            }
            _ => {
                return Err(StoreError::InvalidStoreRoot {
                    path: display_path,
                    kind: "non-directory",
                });
            }
        }
        let fd = fs::openat(
            &workspace_fd,
            ".reviewgraphen",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        verify_owner(&fd, &display_path)?;
        let mode = Mode::from_raw_mode(fs::fstat(&fd)?.st_mode).as_raw_mode() & 0o7777;
        if mode != 0o700 {
            return Err(StoreError::InsecureMode { path: display_path });
        }
        Ok(Self {
            fd,
            display_path,
            limits,
        })
    }

    /// Refuses unsupported platforms rather than providing a path-based fallback.
    #[cfg(not(target_os = "linux"))]
    pub fn open(_workspace: impl AsRef<Path>, _limits: StoreLimits) -> Result<Self, StoreError> {
        Err(StoreError::UnsupportedPlatform)
    }

    /// Display-only canonical path; storage operations must use the descriptor.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.display_path
    }

    /// Configured store limits.
    #[must_use]
    pub const fn limits(&self) -> StoreLimits {
        self.limits
    }

    /// Internal root descriptor for FD-relative store operations.
    #[allow(dead_code)] // Retained for the following CAS/journal subunits.
    #[cfg(target_os = "linux")]
    pub(crate) fn fd(&self) -> &OwnedFd {
        &self.fd
    }
}

#[cfg(target_os = "linux")]
fn verify_owner(fd: &OwnedFd, path: &Path) -> Result<(), StoreError> {
    let stat = fs::fstat(fd)?;
    verify_owner_ids(
        stat.st_uid,
        stat.st_gid,
        geteuid().as_raw(),
        getegid().as_raw(),
        path,
    )
}

fn verify_owner_ids(
    actual_uid: u32,
    actual_gid: u32,
    expected_uid: u32,
    expected_gid: u32,
    path: &Path,
) -> Result<(), StoreError> {
    if actual_uid != expected_uid || actual_gid != expected_gid {
        return Err(StoreError::OwnerMismatch {
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn ensure_unambiguous_credentials(
    real_uid: u32,
    effective_uid: u32,
    real_gid: u32,
    effective_gid: u32,
) -> Result<(), StoreError> {
    if real_uid != effective_uid || real_gid != effective_gid {
        Err(StoreError::CredentialMismatch)
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn map_openat2_error(error: rustix::io::Errno) -> StoreError {
    if error == rustix::io::Errno::NOSYS {
        StoreError::UnsupportedPlatform
    } else {
        StoreError::Io(error)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::io;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn abc_hash() -> CasHash {
        CasHash::parse("sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
            .unwrap()
    }

    fn store_with_limit(limit: u64) -> (tempfile::TempDir, StoreRoot) {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(
            workspace.path(),
            StoreLimits {
                max_object_bytes: limit,
                min_tmp_gc_age: Duration::ZERO,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        (workspace, root)
    }

    #[test]
    fn creates_owner_only_store_root() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        assert_eq!(root.path(), workspace.path().join(".reviewgraphen"));
        assert_eq!(
            std::fs::metadata(root.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn rejects_symlink_at_store_name() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), workspace.path().join(".reviewgraphen")).unwrap();
        assert!(matches!(
            StoreRoot::open(workspace.path(), StoreLimits::default()),
            Err(StoreError::InvalidStoreRoot {
                kind: "symlink",
                ..
            })
        ));
    }

    #[test]
    fn rejects_regular_file_at_store_name() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join(".reviewgraphen"), b"not a directory").unwrap();
        assert!(matches!(
            StoreRoot::open(workspace.path(), StoreLimits::default()),
            Err(StoreError::InvalidStoreRoot {
                kind: "non-directory",
                ..
            })
        ));
    }

    #[test]
    fn rejects_insecure_existing_store_mode() {
        let workspace = tempfile::tempdir().unwrap();
        let store = workspace.path().join(".reviewgraphen");
        std::fs::create_dir(&store).unwrap();
        std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            StoreRoot::open(workspace.path(), StoreLimits::default()),
            Err(StoreError::InsecureMode { .. })
        ));
    }

    #[test]
    fn rejects_special_mode_bits() {
        let workspace = tempfile::tempdir().unwrap();
        let store = workspace.path().join(".reviewgraphen");
        std::fs::create_dir(&store).unwrap();
        std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o4700)).unwrap();
        assert!(matches!(
            StoreRoot::open(workspace.path(), StoreLimits::default()),
            Err(StoreError::InsecureMode { .. })
        ));
    }

    #[test]
    fn rejects_credential_ambiguity() {
        assert!(matches!(
            ensure_unambiguous_credentials(1, 2, 3, 3),
            Err(StoreError::CredentialMismatch)
        ));
    }

    #[test]
    fn reports_owner_mismatch_without_chown() {
        let path = Path::new("/admitted-root");
        assert!(matches!(
            verify_owner_ids(10, 20, 10, 21, path),
            Err(StoreError::OwnerMismatch { path: returned }) if returned == path
        ));
    }

    #[test]
    fn maps_missing_openat2_to_typed_platform_refusal() {
        assert!(matches!(
            map_openat2_error(rustix::io::Errno::NOSYS),
            StoreError::UnsupportedPlatform
        ));
        assert!(matches!(
            map_openat2_error(rustix::io::Errno::ACCESS),
            StoreError::Io(rustix::io::Errno::ACCESS)
        ));
    }

    #[test]
    fn create_only_publish_classifier_refuses_an_unsupported_rename_fallback() {
        for error in [
            rustix::io::Errno::NOSYS,
            rustix::io::Errno::OPNOTSUPP,
            rustix::io::Errno::INVAL,
        ] {
            assert!(matches!(
                map_publish_error(error),
                StoreError::UnsupportedPlatform
            ));
        }
        assert!(matches!(
            map_publish_error(rustix::io::Errno::EXIST),
            StoreError::Io(rustix::io::Errno::EXIST)
        ));
    }

    #[test]
    fn cas_put_read_is_bounded_and_idempotent() {
        let (_workspace, root) = store_with_limit(3);
        let store = CasStore::open(&root).unwrap();
        let hash = abc_hash();
        assert!(!store.put(&hash, Some(3), &b"abc"[..]).unwrap().existed);
        assert_eq!(store.read(&hash).unwrap(), b"abc");
        assert!(store.put(&hash, Some(2), &b"abc"[..]).is_err());
        assert!(matches!(
            store.put(&hash, None, &b"abcd"[..]),
            Err(StoreError::ObjectTooLarge { .. })
        ));
        assert!(store.put(&hash, Some(3), &b"abc"[..]).unwrap().existed);
    }

    #[test]
    fn read_only_cas_reader_accepts_zero_byte_object_into_exact_buffer() {
        let (_workspace, root) = store_with_limit(3);
        let store = CasStore::open(&root).unwrap();
        let hash = CasHash::parse(
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        )
        .unwrap();
        store.put(&hash, Some(0), &b""[..]).unwrap();
        drop(store);
        let reader = CasReader::open_existing(&root).unwrap();
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(0).unwrap();
        reader.read_into(&hash, Some(0), &mut bytes).unwrap();
        assert!(bytes.is_empty());
        assert_eq!(bytes.capacity(), 0);
    }

    #[test]
    fn read_only_cas_reader_refuses_to_grow_beyond_admitted_capacity() {
        let (_workspace, root) = store_with_limit(3);
        let store = CasStore::open(&root).unwrap();
        let hash = abc_hash();
        store.put(&hash, Some(3), &b"abc"[..]).unwrap();
        drop(store);
        let reader = CasReader::open_existing(&root).unwrap();
        let mut undersized = Vec::with_capacity(2);
        assert!(matches!(
            reader.read_into(&hash, Some(3), &mut undersized),
            Err(StoreError::Incomplete {
                limit: 2,
                observed: 3
            })
        ));
        assert!(undersized.is_empty());
        assert_eq!(undersized.capacity(), 2);

        let mut exact = Vec::with_capacity(3);
        reader.read_into(&hash, Some(3), &mut exact).unwrap();
        assert_eq!(exact, b"abc");
        assert_eq!(exact.capacity(), 3);
    }

    #[test]
    fn cas_hash_is_strict() {
        assert!(
            CasHash::parse(
                "sha256:BA7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            )
            .is_err()
        );
        assert!(CasHash::parse("sha256:../bad").is_err());
        for invalid in [
            "blake3:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015a/",
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015a.",
            "sha256:ba7816bf",
        ] {
            assert!(CasHash::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn cas_open_refuses_symlinked_or_insecure_directory_components() {
        let (_workspace, root) = store_with_limit(3);
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("artifacts")).unwrap();
        assert!(CasStore::open(&root).is_err());

        std::fs::remove_file(root.path().join("artifacts")).unwrap();
        std::fs::create_dir(root.path().join("artifacts")).unwrap();
        std::fs::set_permissions(
            root.path().join("artifacts"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert!(matches!(
            CasStore::open(&root),
            Err(StoreError::InsecureMode { .. })
        ));
    }

    #[test]
    fn failed_puts_leave_no_temp_or_published_object() {
        let (_workspace, root) = store_with_limit(3);
        let store = CasStore::open(&root).unwrap();
        let hash = abc_hash();
        assert!(matches!(
            store.put(&hash, Some(3), &b"abd"[..]),
            Err(StoreError::HashMismatch)
        ));
        assert!(matches!(
            store.put(&hash, None, &b"abcd"[..]),
            Err(StoreError::ObjectTooLarge { .. })
        ));
        assert_eq!(
            std::fs::read_dir(root.path().join("artifacts").join("tmp"))
                .unwrap()
                .count(),
            0
        );
        assert!(matches!(
            store.read(&hash),
            Err(StoreError::MissingArtifact)
        ));
    }

    #[test]
    fn detects_corrupt_existing_object_instead_of_replacing_it() {
        let (_workspace, root) = store_with_limit(3);
        let store = CasStore::open(&root).unwrap();
        let hash = abc_hash();
        store.put(&hash, Some(3), &b"abc"[..]).unwrap();
        let object = root
            .path()
            .join("artifacts")
            .join("sha256")
            .join(hash.prefix())
            .join(hash.hex());
        std::fs::write(&object, b"bad").unwrap();
        assert!(matches!(
            store.put(&hash, Some(3), &b"abc"[..]),
            Err(StoreError::CorruptedArtifact)
        ));
        assert_eq!(std::fs::read(&object).unwrap(), b"bad");
    }

    #[test]
    fn rejects_a_symlink_at_an_expected_object_path() {
        let (_workspace, root) = store_with_limit(3);
        let store = CasStore::open(&root).unwrap();
        let hash = abc_hash();
        let prefix = root
            .path()
            .join("artifacts")
            .join("sha256")
            .join(hash.prefix());
        std::fs::create_dir_all(&prefix).unwrap();
        std::fs::set_permissions(&prefix, std::fs::Permissions::from_mode(0o700)).unwrap();
        let target = root.path().join("not-an-object");
        std::fs::write(&target, b"abc").unwrap();
        symlink(&target, prefix.join(hash.hex())).unwrap();
        assert!(matches!(
            store.read(&hash),
            Err(StoreError::CorruptedArtifact)
        ));
    }

    #[test]
    fn rejects_fifo_directory_and_loose_mode_without_opening_them_as_objects() {
        let (_workspace, root) = store_with_limit(3);
        let store = CasStore::open(&root).unwrap();
        let hash = abc_hash();
        let prefix = root
            .path()
            .join("artifacts")
            .join("sha256")
            .join(hash.prefix());
        std::fs::create_dir_all(&prefix).unwrap();
        std::fs::set_permissions(&prefix, std::fs::Permissions::from_mode(0o700)).unwrap();
        let prefix_fd = fs::openat(
            &store.sha256,
            hash.prefix(),
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .unwrap();
        fs::mkfifoat(&prefix_fd, hash.hex(), Mode::from_raw_mode(0o600)).unwrap();
        assert!(matches!(
            store.read(&hash),
            Err(StoreError::CorruptedArtifact)
        ));
        std::fs::remove_file(prefix.join(hash.hex())).unwrap();
        std::fs::create_dir(prefix.join(hash.hex())).unwrap();
        assert!(matches!(
            store.read(&hash),
            Err(StoreError::CorruptedArtifact)
        ));
        std::fs::remove_dir(prefix.join(hash.hex())).unwrap();
        std::fs::write(prefix.join(hash.hex()), b"abc").unwrap();
        std::fs::set_permissions(
            prefix.join(hash.hex()),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(matches!(
            store.read(&hash),
            Err(StoreError::CorruptedArtifact)
        ));
    }

    #[test]
    fn gc_only_deletes_regular_unleased_temp_files() {
        let (_workspace, root) = store_with_limit(3);
        let store = CasStore::open(&root).unwrap();
        let stale = fs::openat(
            &store.tmp,
            "stale",
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::from_raw_mode(0o600),
        )
        .unwrap();
        drop(stale);
        let held = fs::openat(
            &store.tmp,
            "held",
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::from_raw_mode(0o600),
        )
        .unwrap();
        fs::flock(&held, FlockOperation::NonBlockingLockExclusive).unwrap();
        symlink(
            root.path(),
            root.path().join("artifacts").join("tmp").join("link"),
        )
        .unwrap();
        let receipt = store.gc_tmp().unwrap();
        assert_eq!(receipt.removed, 1);
        assert_eq!(receipt.skipped_locked, 1);
        assert_eq!(receipt.skipped_non_regular, 1);
        assert!(
            root.path()
                .join("artifacts")
                .join("tmp")
                .join("held")
                .exists()
        );
        assert!(
            root.path()
                .join("artifacts")
                .join("tmp")
                .join("link")
                .exists()
        );
    }

    #[test]
    fn zero_age_gc_skips_a_visible_live_temp_because_its_lease_is_already_held() {
        let (_workspace, root) = store_with_limit(3);
        let store = CasStore::open(&root).unwrap();
        let (name, held) = store.create_temp().unwrap();
        let receipt = store.gc_tmp().unwrap();
        assert_eq!(receipt.removed, 0);
        assert_eq!(receipt.skipped_locked, 1);
        fs::unlinkat(&store.tmp, &name, AtFlags::empty()).unwrap();
        drop(held);
    }

    #[test]
    fn gc_rejects_a_young_loose_regular_file_before_applying_its_age_policy() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(
            workspace.path(),
            StoreLimits {
                min_tmp_gc_age: Duration::from_secs(3600),
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let store = CasStore::open(&root).unwrap();
        let path = root.path().join("artifacts").join("tmp").join("young");
        std::fs::write(&path, b"x").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert!(matches!(
            store.gc_tmp(),
            Err(StoreError::InsecureMode { .. })
        ));
    }

    #[test]
    fn stream_failure_cleans_up_the_leased_temp() {
        struct FailingReader;
        impl Read for FailingReader {
            fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("simulated stream failure"))
            }
        }
        let (_workspace, root) = store_with_limit(3);
        let store = CasStore::open(&root).unwrap();
        assert!(matches!(
            store.put(&abc_hash(), None, FailingReader),
            Err(StoreError::Stream(_))
        ));
        assert_eq!(
            std::fs::read_dir(root.path().join("artifacts").join("tmp"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn simulated_crash_after_temp_sync_never_publishes_and_is_gc_recoverable() {
        let (workspace, root) = store_with_limit(3);
        let store = CasStore::open(&root).unwrap();
        CRASH_AFTER_TEMP_SYNC.store(true, Ordering::SeqCst);
        assert!(matches!(
            store.put(&abc_hash(), Some(3), &b"abc"[..]),
            Err(StoreError::SimulatedCrash)
        ));
        drop(store);
        drop(root);
        let reopened_root = StoreRoot::open(
            workspace.path(),
            StoreLimits {
                max_object_bytes: 3,
                min_tmp_gc_age: Duration::ZERO,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let reopened = CasStore::open(&reopened_root).unwrap();
        assert!(matches!(
            reopened.read(&abc_hash()),
            Err(StoreError::MissingArtifact)
        ));
        assert_eq!(
            std::fs::read_dir(reopened_root.path().join("artifacts").join("tmp"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(reopened.gc_tmp().unwrap().removed, 1);
    }
}

#[cfg(all(test, not(target_os = "linux")))]
mod unsupported_platform_tests {
    use super::*;

    #[test]
    fn refuses_path_based_store_root_fallback() {
        assert!(matches!(
            StoreRoot::open(".", StoreLimits::default()),
            Err(StoreError::UnsupportedPlatform)
        ));
    }
}
