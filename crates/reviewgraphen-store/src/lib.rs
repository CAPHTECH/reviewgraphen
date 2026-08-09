//! Linux-only durable-store root admission.
//!
//! This crate deliberately starts with the trusted root boundary. CAS, JSONL,
//! and index layers must use the returned directory descriptor rather than
//! reconstructing paths from ambient state.

#[cfg(target_os = "linux")]
use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, CWD, FileType, Mode, OFlags, ResolveFlags},
    process::{getegid, geteuid, getgid, getuid},
};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Resource limits reserved for subsequent CAS and journal units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoreLimits {
    /// Largest allowed CAS object in bytes.
    pub max_object_bytes: u64,
}

impl Default for StoreLimits {
    fn default() -> Self {
        Self {
            max_object_bytes: 64 * 1024 * 1024,
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
        match fs::mkdirat(&workspace_fd, ".reviewgraphen", Mode::from_raw_mode(0o700)) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(error) => return Err(StoreError::Io(error)),
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
    use std::os::unix::fs::{PermissionsExt, symlink};

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
