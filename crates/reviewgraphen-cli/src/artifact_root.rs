//! Descriptor-relative artifact-root admission and publication (T1).
//!
//! The pathname checks of the runtime admission run first, unchanged, so the
//! refusal rules and reasons stay as they were. Then the invocation root is
//! opened as a directory descriptor, every component of the `--artifacts`
//! argument is walked with `O_NOFOLLOW | O_DIRECTORY`, the root is created
//! with `mkdirat` under the held parent and opened `O_NOFOLLOW`, and every
//! entry inside it is created relative to the held root (or `records/`)
//! descriptor with `O_CREAT | O_EXCL | O_NOFOLLOW`. A component swapped for a
//! symlink therefore cannot redirect a creation or a write outside.
//!
//! Because a descriptor-relative writer would otherwise publish silently into
//! a directory that was moved away, the held chain is re-verified before the
//! first entry and after the last one: each held directory must still be the
//! entry (same device and inode, not followed) under its name in the held
//! directory above it, from the invocation root down to the artifact root. A
//! mismatch fails closed. Unwinding removes, relative to held descriptors and
//! only after an identity check, exactly what this call created, with
//! non-recursive removals; a foreign entry is never removed.

use std::path::{Component, Path};

/// One file to publish: its path relative to the artifact root (at most one
/// directory level, `records/<name>`), its bytes, and the fixed refusal
/// reason reported if it cannot be created or written.
pub(crate) struct PlannedFile<'a> {
    pub(crate) path: &'a str,
    pub(crate) bytes: &'a [u8],
    pub(crate) error: &'static str,
}

/// What a route publishes, in creation order.
pub(crate) struct WritePlan<'a> {
    /// Create `records/` before any file (v2–v4 and TS v5 when a path nests).
    pub(crate) records: bool,
    pub(crate) files: Vec<PlannedFile<'a>>,
    /// Run the G7-R1 post-admission test hook (v2–v4 only).
    pub(crate) post_admission_hook: bool,
}

const CHANGED: &str = "generic review artifact root changed during admission";
const NOT_FRESH: &str =
    "generic review artifact directory is not the fresh directory this call created";

/// A refusal of admission or publication: the fixed reason reported on
/// stderr and the typed record of the unwind that followed it.
#[derive(Debug)]
pub(crate) struct ArtifactFailure {
    pub(crate) reason: String,
    pub(crate) unwind: Vec<UnwindRecord>,
}

/// What the unwind did with one entry this call created.
#[derive(Debug)]
pub(crate) struct UnwindRecord {
    /// Path relative to the artifact root (`""` is the root itself).
    pub(crate) entry: String,
    pub(crate) outcome: UnwindOutcome,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UnwindOutcome {
    Removed,
    /// Left in place by design: the name no longer holds what this call
    /// created (never removed), or the directory holds entries this call did
    /// not create (non-recursive removal, never forced).
    Kept(KeptReason),
    /// The removal failed for another reason (raw errno).
    Failed(i32),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum KeptReason {
    /// The identity `statat` returned `ENOENT`: nothing is at the name.
    Absent,
    NotCreatedByThisCall,
    NotEmpty,
}

/// How a fresh-directory check treated the permission bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModeCheck {
    /// Compared with `0o777 & !umask`.
    Compared,
    /// Skipped because the umask could not be read without changing it; the
    /// ownership and emptiness checks still applied.
    SkippedUmaskUnavailable,
}

impl ArtifactFailure {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            unwind: Vec::new(),
        }
    }

    /// The stderr text: the fixed reason, plus the entries whose removal
    /// failed unexpectedly (by-design `Kept` outcomes are not reported, so
    /// the existing refusal texts are unchanged).
    pub(crate) fn message(&self) -> String {
        let failed = self
            .unwind
            .iter()
            .filter_map(|record| match record.outcome {
                UnwindOutcome::Failed(errno) => Some(format!("{:?} (errno {errno})", record.entry)),
                _ => None,
            })
            .collect::<Vec<_>>();
        if failed.is_empty() {
            self.reason.clone()
        } else {
            format!("{}; unwind incomplete: {}", self.reason, failed.join(", "))
        }
    }
}

#[cfg(unix)]
pub(crate) use unix::AdmittedRoot;

#[cfg(unix)]
mod unix {
    use super::{
        ArtifactFailure, CHANGED, Component, KeptReason, ModeCheck, NOT_FRESH, Path, UnwindOutcome,
        UnwindRecord, WritePlan,
    };
    use reviewgraphen_runtime::generic::{
        GenericReviewError, check_fresh_generic_review_artifact_root_v2,
    };
    use rustix::{
        fd::{AsFd, OwnedFd},
        fs::{self, AtFlags, Dir, FileType, Mode, OFlags},
        io::Errno,
        process,
    };
    #[cfg(test)]
    use std::path::PathBuf;
    use std::{ffi::OsString, io::Write as _};

    type Identity = (u64, u64);

    /// The mode every created directory is requested with (as `create_dir`).
    // `RawMode` is `u32` on Linux and `u16` on macOS.
    const DIRECTORY_MODE: rustix::fs::RawMode = 0o777;

    /// Directory descriptors that are only used as `*at` anchors. On Linux
    /// `O_PATH` needs no read permission on the directory, like the pathname
    /// creation it replaces.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const DIRECTORY_FLAGS: OFlags = OFlags::PATH
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);

    struct HeldDirectory {
        fd: OwnedFd,
        identity: Identity,
    }

    enum Created {
        /// A file, by name, in the root or (`in_records`) in `records/`.
        File {
            in_records: bool,
            name: OsString,
            identity: Identity,
        },
        Records {
            identity: Identity,
        },
    }

    /// A freshly created artifact root held by descriptor, with the chain of
    /// held directories from the invocation root down to it.
    pub(crate) struct AdmittedRoot {
        /// The artifact-root pathname, only for the test seams.
        #[cfg(test)]
        path: PathBuf,
        /// `chain[0]` is the invocation root; `chain[i]` for `i ≥ 1` is the
        /// directory named `names[i - 1]` inside `chain[i - 1]`. The last
        /// element is the artifact root this call created.
        chain: Vec<HeldDirectory>,
        names: Vec<OsString>,
        umask: Option<u32>,
    }

    fn identity_of(stat: &fs::Stat) -> Identity {
        #[allow(clippy::useless_conversion, clippy::unnecessary_cast)]
        (stat.st_dev as u64, stat.st_ino as u64)
    }

    fn fd_identity(fd: &OwnedFd) -> Result<Identity, Errno> {
        fs::fstat(fd).map(|stat| identity_of(&stat))
    }

    fn entry_identity(directory: &OwnedFd, name: &OsString) -> Option<Identity> {
        fs::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
            .ok()
            .map(|stat| identity_of(&stat))
    }

    fn open_directory(directory: &OwnedFd, name: &OsString) -> Result<HeldDirectory, Errno> {
        let fd = fs::openat(directory, name, DIRECTORY_FLAGS, Mode::empty())?;
        let identity = fd_identity(&fd)?;
        Ok(HeldDirectory { fd, identity })
    }

    /// The process umask, read WITHOUT ever changing it:
    /// on Linux the `Umask:` line of `/proc/self/status` (kernel ≥ 4.7).
    /// `None` when that is unavailable or unparseable, and on every other
    /// platform (POSIX has no read-only umask query); the caller then skips
    /// only the mode comparison.
    fn current_umask() -> Option<u32> {
        #[cfg(test)]
        if crate::t1s3_seam::umask_forced_unavailable() {
            return None;
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            std::fs::read_to_string("/proc/self/status")
                .ok()
                .and_then(|status| {
                    status.lines().find_map(|line| {
                        line.strip_prefix("Umask:")
                            .and_then(|value| u32::from_str_radix(value.trim(), 8).ok())
                    })
                })
                .filter(|mask| *mask <= 0o777)
        }
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            None
        }
    }

    /// T1-R2: the directory held by `fd`, which this call just created with
    /// `mkdirat`, must be a directory owned by the effective uid, with exactly
    /// the permission bits `mkdirat` gives it under `umask` (skipped, and
    /// reported as such, when the umask is unknown), and empty (only `.` and
    /// `..`). Anything else is a directory substituted after `mkdirat` and is
    /// never adopted (`None`). The emptiness read needs owner read and search
    /// permission on the fresh directory: an unusual umask that removes them
    /// (e.g. 0400 or 0100) makes the check impossible and the run fails
    /// closed — a known conservative limitation.
    fn verify_fresh(fd: &OwnedFd, umask: Option<u32>) -> Option<ModeCheck> {
        let stat = fs::fstat(fd).ok()?;
        #[allow(clippy::useless_conversion, clippy::unnecessary_cast)]
        let (mode, uid) = (stat.st_mode as u32, stat.st_uid as u32);
        if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
            || uid != process::geteuid().as_raw()
        {
            return None;
        }
        #[allow(clippy::unnecessary_cast)]
        let directory_mode = DIRECTORY_MODE as u32;
        let mode_check = match umask {
            Some(umask) if mode & 0o777 != directory_mode & !umask => return None,
            Some(_) => ModeCheck::Compared,
            None => ModeCheck::SkippedUmaskUnavailable,
        };
        // Read through "." of the held descriptor, never through the name.
        let readable = fs::openat(
            fd,
            c".",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .ok()?;
        let entries = Dir::new(readable).ok()?;
        for entry in entries {
            match entry {
                Ok(entry) if matches!(entry.file_name().to_bytes(), b"." | b"..") => {}
                _ => return None,
            }
        }
        #[cfg(test)]
        if mode_check == ModeCheck::SkippedUmaskUnavailable {
            crate::t1s3_seam::record_mode_check_skip();
        }
        Some(mode_check)
    }

    /// `before_unlink` runs between the identity check and `unlinkat` (the
    /// T1-S2 test seam point for files; a no-op otherwise). Only `ENOENT`
    /// from the identity `statat` means "not present"; any other error is
    /// `Failed(errno)` and the entry stays. `label` is the
    /// root-relative entry name (used by the T1-S3 test seam only).
    #[cfg_attr(not(test), allow(unused_variables))]
    fn removal(
        directory: &OwnedFd,
        name: &OsString,
        label: &str,
        identity: Identity,
        flags: AtFlags,
        before_unlink: impl FnOnce(),
    ) -> UnwindOutcome {
        #[cfg(test)]
        let injected = crate::t1s3_seam::unwind_stat_fault(label);
        #[cfg(not(test))]
        let injected: Option<Errno> = None;
        let current = match injected {
            Some(errno) => Err(errno),
            None => fs::statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)
                .map(|stat| identity_of(&stat)),
        };
        match current {
            Ok(current) if current == identity => {}
            Ok(_) => return UnwindOutcome::Kept(KeptReason::NotCreatedByThisCall),
            Err(Errno::NOENT) => return UnwindOutcome::Kept(KeptReason::Absent),
            Err(error) => return UnwindOutcome::Failed(error.raw_os_error()),
        }
        before_unlink();
        #[cfg(test)]
        let injected = crate::t1s3_seam::unwind_unlink_fault(label);
        #[cfg(not(test))]
        let injected: Option<Errno> = None;
        let removed = match injected {
            Some(errno) => Err(errno),
            None => fs::unlinkat(directory, name, flags),
        };
        // Every removal error is reported, typed; only a directory that
        // still holds entries this call did not create is kept by design.
        match removed {
            Ok(()) => UnwindOutcome::Removed,
            Err(Errno::NOTEMPTY | Errno::EXIST) if flags.contains(AtFlags::REMOVEDIR) => {
                UnwindOutcome::Kept(KeptReason::NotEmpty)
            }
            Err(error) => UnwindOutcome::Failed(error.raw_os_error()),
        }
    }

    impl AdmittedRoot {
        /// Runs the pathname admission checks, walks the argument by
        /// descriptor, creates the root under the held parent and verifies
        /// the held chain. `invocation_root` is the canonical cwd and
        /// `root` is `invocation_root.join(argument)`.
        pub(crate) fn admit(
            invocation_root: &Path,
            argument: &Path,
            root: &Path,
        ) -> Result<Self, ArtifactFailure> {
            check_fresh_generic_review_artifact_root_v2(root)
                .map_err(|error| ArtifactFailure::new(error.to_string()))?;
            let mut names = Vec::new();
            for component in argument.components() {
                match component {
                    Component::Normal(name) => names.push(name.to_os_string()),
                    _ => {
                        return Err(ArtifactFailure::new(
                            "generic review artifact root path traversal",
                        ));
                    }
                }
            }
            let Some((root_name, parents)) = names.split_last() else {
                return Err(ArtifactFailure::new(
                    "generic review artifact root path traversal",
                ));
            };
            let changed = |_| ArtifactFailure::new(CHANGED);
            let invocation = fs::open(invocation_root, DIRECTORY_FLAGS, Mode::empty())
                .map_err(|_| ArtifactFailure::new("unable to resolve invocation cwd"))?;
            let identity = fd_identity(&invocation).map_err(changed)?;
            let mut chain = vec![HeldDirectory {
                fd: invocation,
                identity,
            }];
            #[cfg(test)]
            let mut walked = invocation_root.to_path_buf();
            for name in parents {
                let parent = &chain.last().expect("chain starts non-empty").fd;
                let next = open_directory(parent, name).map_err(changed)?;
                chain.push(next);
                #[cfg(test)]
                {
                    walked.push(name);
                    crate::t1s2_seam::fire(crate::t1s2_seam::Point::MidWalk, &walked);
                }
            }
            let umask = current_umask();
            #[cfg(test)]
            crate::t1_seam::fire(crate::t1_seam::SwapPoint::BeforeRootCreate, root);
            let parent = &chain.last().expect("chain starts non-empty").fd;
            match fs::mkdirat(parent, root_name, Mode::from_raw_mode(DIRECTORY_MODE)) {
                Ok(()) => {}
                Err(Errno::EXIST) => {
                    return Err(ArtifactFailure::new(
                        GenericReviewError::ArtifactRootAlreadyExists.to_string(),
                    ));
                }
                Err(error) => {
                    return Err(ArtifactFailure::new(
                        GenericReviewError::Io(error.into()).to_string(),
                    ));
                }
            }
            #[cfg(test)]
            crate::t1r2_seam::fire(crate::t1r2_seam::CreatedDirectory::Root, root);
            // No identity sample of the created entry is taken before the
            // open; the fresh check below is done on the opened descriptor.
            #[cfg(test)]
            crate::t1s2_seam::fire(crate::t1s2_seam::Point::BeforeReopen, root);
            let opened = open_directory(parent, root_name);
            #[cfg(test)]
            if opened.is_ok() {
                crate::t1s2_seam::fire(crate::t1s2_seam::Point::AfterReopen, root);
            }
            // Until the opened directory is shown to be the fresh one this
            // call created, nothing at the name is ever removed: a refusal
            // here may leave this call's own empty directory behind, never
            // touch a foreign one.
            let root_held = match opened {
                Ok(held) if verify_fresh(&held.fd, umask).is_some() => held,
                Ok(_) => return Err(ArtifactFailure::new(NOT_FRESH)),
                Err(_) => return Err(ArtifactFailure::new(CHANGED)),
            };
            chain.push(root_held);
            let admitted = Self {
                #[cfg(test)]
                path: root.to_path_buf(),
                chain,
                names,
                umask,
            };
            if !admitted.chain_is_intact() {
                let mut failure = ArtifactFailure::new(CHANGED);
                failure.unwind = admitted.unwind(&[], None);
                return Err(failure);
            }
            Ok(admitted)
        }

        /// Each held directory is still the (unfollowed) entry under its name
        /// in the held directory above it, from the invocation root down.
        fn chain_is_intact(&self) -> bool {
            self.names.iter().enumerate().all(|(index, name)| {
                entry_identity(&self.chain[index].fd, name) == Some(self.chain[index + 1].identity)
            })
        }

        fn root(&self) -> &HeldDirectory {
            self.chain.last().expect("admitted chain holds the root")
        }

        fn parent(&self) -> &HeldDirectory {
            &self.chain[self.chain.len() - 2]
        }

        /// Creates every planned entry relative to the held root, verifying
        /// the held chain before the first entry and after the last. On any
        /// failure it removes exactly what it created (and the root when it
        /// is still this call's root and empty) and returns the reason.
        pub(crate) fn write_or_unwind(self, plan: &WritePlan<'_>) -> Result<(), ArtifactFailure> {
            #[cfg(test)]
            if plan.post_admission_hook {
                crate::g7_r1_seam::run_post_admission_hook(&self.path);
            }
            #[cfg(not(test))]
            let _ = plan.post_admission_hook;
            #[cfg(test)]
            crate::t1_seam::fire(crate::t1_seam::SwapPoint::AfterRootCreate, &self.path);
            let mut created = Vec::new();
            let mut records = None;
            self.write_tracked(plan, &mut created, &mut records)
                .map_err(|reason| ArtifactFailure {
                    reason,
                    unwind: self.unwind(&created, records.as_ref()),
                })
        }

        fn write_tracked(
            &self,
            plan: &WritePlan<'_>,
            created: &mut Vec<Created>,
            records: &mut Option<OwnedFd>,
        ) -> Result<(), String> {
            if !self.chain_is_intact() {
                return Err(CHANGED.to_owned());
            }
            let root = &self.root().fd;
            if plan.records {
                let name = OsString::from("records");
                fs::mkdirat(root, &name, Mode::from_raw_mode(DIRECTORY_MODE))
                    .map_err(|_| "unable to create generic review records".to_owned())?;
                #[cfg(test)]
                crate::t1r2_seam::fire(
                    crate::t1r2_seam::CreatedDirectory::Records,
                    &self.path.join("records"),
                );
                // Recorded for unwind only once shown to be the fresh
                // directory this call created; a substitute is never removed.
                let held = open_directory(root, &name)
                    .map_err(|_| "unable to create generic review records".to_owned())?;
                if verify_fresh(&held.fd, self.umask).is_none() {
                    return Err(NOT_FRESH.to_owned());
                }
                created.push(Created::Records {
                    identity: held.identity,
                });
                *records = Some(held.fd);
            }
            #[cfg_attr(not(test), allow(unused_variables))]
            for (index, file) in plan.files.iter().enumerate() {
                #[cfg(test)]
                crate::t1s2_seam::fire(
                    crate::t1s2_seam::Point::BetweenFiles { written: index },
                    &self.path,
                );
                let (in_records, name) = match file.path.split_once('/') {
                    None => (false, file.path),
                    Some(("records", name)) if !name.contains('/') && records.is_some() => {
                        (true, name)
                    }
                    Some(_) => return Err(file.error.to_owned()),
                };
                let directory = if in_records {
                    records.as_ref().expect("records held")
                } else {
                    root
                };
                write_new_file(directory, in_records, name, file.bytes, created)
                    .map_err(|()| file.error.to_owned())?;
            }
            if !self.chain_is_intact() {
                return Err(CHANGED.to_owned());
            }
            Ok(())
        }

        /// Non-recursive, identity-checked removal of what this call created,
        /// newest first, then the root itself through the held parent. Every
        /// outcome is recorded. Residual (V3): between the identity check and
        /// `unlinkat` a same-named replacement can still be removed.
        fn unwind(&self, created: &[Created], records: Option<&OwnedFd>) -> Vec<UnwindRecord> {
            let root = &self.root().fd;
            let mut log = Vec::new();
            for entry in created.iter().rev() {
                match entry {
                    Created::File {
                        in_records,
                        name,
                        identity,
                    } => {
                        let label = if *in_records {
                            format!("records/{}", name.to_string_lossy())
                        } else {
                            name.to_string_lossy().into_owned()
                        };
                        let before_unlink = || {
                            #[cfg(test)]
                            crate::t1s2_seam::fire(
                                crate::t1s2_seam::Point::UnwindBeforeUnlink,
                                &self.path.join(&label),
                            );
                        };
                        let outcome = match (*in_records, records) {
                            (false, _) => removal(
                                root,
                                name,
                                &label,
                                *identity,
                                AtFlags::empty(),
                                before_unlink,
                            ),
                            (true, Some(records)) => removal(
                                records,
                                name,
                                &label,
                                *identity,
                                AtFlags::empty(),
                                before_unlink,
                            ),
                            (true, None) => UnwindOutcome::Kept(KeptReason::NotCreatedByThisCall),
                        };
                        log.push(UnwindRecord {
                            entry: label,
                            outcome,
                        });
                    }
                    Created::Records { identity } => log.push(UnwindRecord {
                        entry: "records".to_owned(),
                        outcome: removal(
                            root,
                            &OsString::from("records"),
                            "records",
                            *identity,
                            AtFlags::REMOVEDIR,
                            || {},
                        ),
                    }),
                }
            }
            let name = self.names.last().expect("root has a name");
            log.push(UnwindRecord {
                entry: String::new(),
                outcome: removal(
                    &self.parent().fd,
                    name,
                    "",
                    self.root().identity,
                    AtFlags::REMOVEDIR,
                    || {},
                ),
            });
            log
        }
    }

    fn write_new_file(
        directory: &OwnedFd,
        in_records: bool,
        name: &str,
        bytes: &[u8],
        created: &mut Vec<Created>,
    ) -> Result<(), ()> {
        let fd = fs::openat(
            directory.as_fd(),
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o666),
        )
        .map_err(|_| ())?;
        let identity = fd_identity(&fd).map_err(|_| ())?;
        created.push(Created::File {
            in_records,
            name: OsString::from(name),
            identity,
        });
        std::fs::File::from(fd).write_all(bytes).map_err(|_| ())
    }
}

/// Descriptor-safe artifact writes need `*at` system calls; elsewhere the
/// route refuses before creating anything.
#[cfg(not(unix))]
pub(crate) struct AdmittedRoot;

#[cfg(not(unix))]
impl AdmittedRoot {
    pub(crate) fn admit(
        _invocation_root: &Path,
        _argument: &Path,
        _root: &Path,
    ) -> Result<Self, ArtifactFailure> {
        let _ = (
            CHANGED,
            NOT_FRESH,
            Component::CurDir,
            KeptReason::NotEmpty,
            KeptReason::Absent,
            ModeCheck::Compared,
        );
        Err(ArtifactFailure::new(
            "descriptor-safe artifact writes are unsupported on this platform",
        ))
    }

    pub(crate) fn write_or_unwind(self, _plan: &WritePlan<'_>) -> Result<(), ArtifactFailure> {
        Err(ArtifactFailure::new(
            "descriptor-safe artifact writes are unsupported on this platform",
        ))
    }
}
