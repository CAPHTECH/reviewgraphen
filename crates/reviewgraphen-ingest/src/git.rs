use crate::{
    AdapterReport, AdapterStatus, ArtifactDraft, CapabilityState, IngestError, IngestRequest,
    IngestionObstructionKind, IssueDraft, LocationDraft, ObstructionSeverity, RelationDraft,
};
use reviewgraphen_core::{ContentHash, StableId};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Wall-clock bound for one allow-listed `git`/`cargo` child process. A
/// bounded local read/list operation on a reasonably sized repository
/// should never approach this; it exists as a backstop against a hung or
/// adversarially slow subprocess, not a tuned performance budget.
const SUBPROCESS_TIMEOUT: Duration = Duration::from_secs(30);

/// Captured-output bound (stdout and stderr each) for one allow-listed
/// child process. This is a coarse subprocess safety net, independent of
/// and in addition to `IngestLimits::max_file_bytes`, which already bounds
/// one tracked file's content after a successful `git show`.
const SUBPROCESS_MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

/// Environment variables propagated, unmodified, into every allow-listed
/// `git` subprocess. Every other ambient variable is stripped first via
/// `Command::env_clear()`, so a caller's shell environment (locale,
/// `GIT_CONFIG_*`/`GIT_EXTERNAL_DIFF` overrides, credential helpers, ...)
/// can never make the same snapshot tuple produce different accepted facts.
/// `PATH` is kept only so the child `git` process itself (and anything it
/// execs internally) can be resolved and run.
const GIT_INHERITED_ENV_VARS: &[&str] = &["PATH"];

/// Diff algorithm bound into the one Git invocation that generates a
/// textual patch (`ChangedLines`; `--name-status` needs none). Overrides any
/// repo `diff.algorithm` config, so the same base/target pair always
/// produces the same hunk boundaries -- and therefore the same
/// `changed_lines` fact -- regardless of the cloned repo's own config.
const GIT_DIFF_ALGORITHM: &str = "myers";

/// Similarity threshold for rename/copy detection (`-M<n>%`/`-C<n>%`),
/// overriding any repo `diff.renames` default. Pinned at Git's own
/// historical default, so this fixes the value already in effect rather
/// than changing behavior.
const GIT_RENAME_SIMILARITY_PERCENT: u8 = 50;

/// Rename/copy detection limit (`-l<n>`), overriding any repo
/// `diff.renameLimit`. A repo-configured low limit can silently downgrade a
/// real rename/copy into an unrelated `Added`+`Deleted` pair once a
/// changeset is large enough; fixing one generous value removes the repo's
/// own config as a source of that difference. Comfortably above
/// `IngestLimits`'s default `max_files` so a default-sized snapshot's
/// rename/copy detection is never silently skipped.
const GIT_RENAME_LIMIT: u32 = 20_000;

/// Version of the deterministic Git command policy itself (environment
/// allowlist, disabled system/global config and per-user attributes file,
/// disabled external diff/textconv/optional-locks/replace-objects, fixed
/// diff algorithm, rename detection, forced-text diffing, and forced-off
/// diff coloring). Bumped whenever the policy's *shape* changes in a way
/// that could change accepted facts for an unchanged snapshot tuple,
/// independent of the real `git --version` the executing binary reports.
/// `2`: added `force_text_diff` (`--text` on every `git diff`, so a
/// `binary`/`-diff` `.gitattributes` classification can no longer silently
/// empty out `changed_lines`) and `no_replace_objects`
/// (`--no-replace-objects` on every invocation, so a `refs/replace/*` ref
/// either clone happens to carry can no longer substitute a different
/// object for the same requested revision). `3`: added `no_color`
/// (`--no-color` on every `git diff`, so a repo-local `color.ui=always`/
/// `color.diff=always` can no longer inject ANSI escapes into a hunk
/// header -- `changed_lines`'s `@@ ` prefix match would otherwise silently
/// stop matching and empty out the fact).
const GIT_COMMAND_POLICY_VERSION: &str = "3";

/// The deterministic Git command policy's fixed values, bound into
/// `adapter_set_hash` (see `crate::SnapshotIdentities::new`) alongside the
/// real `git`/`cargo`/`syn`/`proc-macro2` tool versions: a future change to
/// any of these constants must visibly change the fingerprint, exactly like
/// a different tool version does.
pub(crate) fn git_command_policy_fingerprint() -> Value {
    json!({
        "version": GIT_COMMAND_POLICY_VERSION,
        "diff_algorithm": GIT_DIFF_ALGORITHM,
        "rename_similarity_percent": GIT_RENAME_SIMILARITY_PERCENT,
        "rename_limit": GIT_RENAME_LIMIT,
        "force_text_diff": true,
        "no_replace_objects": true,
        "no_color": true,
    })
}

/// Version of the deterministic Cargo tool admission policy (below): this
/// module never searches host `PATH`, never spawns `rustup`/`mise`/`asdf`,
/// and never installs or downloads a toolchain. The only source of a
/// `cargo` executable is [`crate::CargoToolAdmission`]: `Disabled` runs no
/// Cargo metadata at all, and `TrustedExecutable(path)` requires `path` to
/// already be an absolute, caller-admitted executable, which is
/// canonicalized and checked to be an existing regular executable file
/// before it is ever spawned. Bumped whenever this policy's *shape* changes
/// in a way that could change whether or which `cargo` a run actually
/// executes for an unchanged `IngestConfig.cargo_admission`, independent of
/// the real `cargo --version` the admitted binary reports.
/// `2`: replaced automatic `PATH`/`rustup` resolution (`same_file` proxy
/// identification, `rustup which cargo`, `RUSTUP_AUTO_INSTALL`) with strict
/// host admission: an audited runtime `rustup` probe can itself touch the
/// toolchain even under a "never install" configuration, so no automatic
/// resolution from inside this crate can be made safe. A caller must admit
/// an already-verified absolute executable instead.
/// `3`: every allow-listed `cargo` subprocess (`cargo --version` and `cargo
/// metadata` alike) now runs through the shared `cargo_command` builder,
/// which clears the child's entire environment (`Command::env_clear()`)
/// before setting only `CARGO_HOME`, `CARGO_NET_OFFLINE`, `CARGO_TERM_COLOR`,
/// and `LC_ALL`. Previously `cargo --version` inherited the calling
/// process's full ambient environment unmodified (including `PATH`,
/// `RUSTUP_TOOLCHAIN`, any `MISE_*` variable, `RUSTC`, `RUSTFLAGS`, and any
/// ambient `CARGO_*`), and `cargo metadata` inherited everything except
/// `CARGO_TARGET_DIR`: either could silently select a different toolchain,
/// target directory, or registry for the identical admitted executable and
/// staged snapshot, depending on whatever shell state happened to invoke
/// this crate. The fixed variable set is now bound directly into this
/// fingerprint below.
const CARGO_RESOLVER_POLICY_VERSION: &str = "3";

/// The deterministic Cargo tool admission policy's fixed values, bound into
/// `adapter_set_hash` (see `crate::SnapshotIdentities::new`) alongside the
/// real `git`/`cargo`/`syn`/`proc-macro2` tool versions and the Git command
/// policy: a future change to any of these constants must visibly change
/// the fingerprint, exactly like a different tool version does. Never
/// includes a host-specific path (no candidate directory, no admitted
/// executable) -- only the fixed policy values themselves, exactly like
/// `git_command_policy_fingerprint`.
pub(crate) fn cargo_resolver_policy_fingerprint() -> Value {
    json!({
        "version": CARGO_RESOLVER_POLICY_VERSION,
        "automatic_path_resolution": false,
        "rustup_invocation": false,
        "admission": "host_absolute_executable",
        "child_environment": "cleared",
        "fixed_env_vars": ["CARGO_HOME", "CARGO_NET_OFFLINE", "CARGO_TERM_COLOR", "LC_ALL"],
    })
}

/// Stable, redacted category for why `cargo --version` -- or the private
/// snapshot staging that must precede it so `cargo --version` and the
/// later `cargo metadata` share the exact same working directory -- could
/// not be determined. `adapter_set_hash` (see
/// `crate::SnapshotIdentities::new`) and a resulting
/// `cargo_metadata_unavailable` limitation's own stable ID are both bound
/// only to this `kind`, never to the randomly-named staged `TempDir` path
/// or to a raw subprocess `stderr` capture: two runs of the identical
/// input staged into two different `TempDir`s must fingerprint and
/// identify identically, and two runs that failed for genuinely different
/// reasons must never collide.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum CargoToolFailureKind {
    /// The private staging directory `cargo --version` and `cargo
    /// metadata` both run from could not be created or populated.
    StagingFailed,
    /// `IngestConfig.cargo_admission` is `Disabled`: no Cargo executable was
    /// admitted for this ingest request, so `cargo --version` was never
    /// attempted.
    NotAdmitted,
    /// `IngestConfig.cargo_admission` is `TrustedExecutable(path)`, but
    /// `path` is not usable as-is: it is not absolute, it could not be
    /// canonicalized to an existing entry, or the canonicalized entry is
    /// not a regular, executable file. Distinct from `Unavailable` below,
    /// which describes a spawn failure of an already-admitted executable.
    AdmittedExecutableInvalid,
    /// The admitted `cargo` executable could not be launched at all.
    Unavailable,
    /// `cargo --version` exceeded the bounded subprocess wall-clock timeout.
    TimedOut,
    /// `cargo --version`'s captured stdout/stderr exceeded the bounded
    /// captured-output cap.
    OutputTooLarge,
    /// `cargo --version` ran and exited with a non-success status.
    NonSuccessExit,
    /// `cargo --version` produced output that is not valid UTF-8.
    NotUtf8,
    /// `cargo --version` exited successfully but produced no output.
    EmptyOutput,
}

impl CargoToolFailureKind {
    /// Stable label folded into `adapter_set_hash` and into a
    /// `cargo_metadata_unavailable` limitation's identity -- part of a
    /// fingerprint two independent runs of the same input must agree on,
    /// so this string must never change meaning once a real run has
    /// observed it.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::StagingFailed => "staging_failed",
            Self::NotAdmitted => "not_admitted",
            Self::AdmittedExecutableInvalid => "admitted_executable_invalid",
            Self::Unavailable => "unavailable",
            Self::TimedOut => "timed_out",
            Self::OutputTooLarge => "output_too_large",
            Self::NonSuccessExit => "non_success_exit",
            Self::NotUtf8 => "not_utf8",
            Self::EmptyOutput => "empty_output",
        }
    }
}

/// A `cargo --version` (or its preceding snapshot-staging) failure: a
/// stable, identity-bearing `kind` plus a human-readable `diagnostic` with
/// the private staged snapshot root -- and its canonicalized form, when it
/// differs -- redacted to the fixed placeholder `<staged-snapshot>`.
/// `diagnostic` is retained only for a human reading the public obstruction
/// text; it is deliberately never folded into `adapter_set_hash` or a
/// limitation's own ID, since a redacted diagnostic still cannot be proven
/// byte-identical across every host/environment the way `kind` can.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CargoToolFailure {
    pub(crate) kind: CargoToolFailureKind,
    pub(crate) diagnostic: String,
}

/// A Git change kind retained by changed-structure mapping.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ChangeKind {
    /// A path exists only in the target tree.
    Added,
    /// A path exists in both trees with changed content or mode.
    Modified,
    /// A path existed only in the base tree.
    Deleted,
    /// Git detected a rename between the bounded commits.
    Renamed,
    /// Git detected a copy between the bounded commits.
    Copied,
    /// Git reported a type change.
    TypeChanged,
}

impl ChangeKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
            Self::Copied => "copied",
            Self::TypeChanged => "type_changed",
        }
    }
}

/// One source-tree change retained without inferring semantic preservation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChangeEntry {
    pub(crate) kind: ChangeKind,
    pub(crate) base_path: String,
    pub(crate) target_path: String,
}

/// The `change` artifact key for one change-family fact. `base_path` is
/// part of this key, so this key -- and everything derived from it -- is
/// the only place `base_revision` is allowed to affect a fact's identity or
/// canonical body; every other accepted fact for the same target revision
/// must stay identical across a different diff base.
pub(crate) fn change_key(change: &ChangeEntry) -> String {
    format!(
        "change:{}:{}:{}",
        change.kind.as_str(),
        change.base_path,
        change.target_path
    )
}

/// Immutable bytes for one regular file in the target Git tree.
#[derive(Clone, Debug)]
pub(crate) struct SnapshotFile {
    pub(crate) path: String,
    pub(crate) content: Vec<u8>,
    pub(crate) content_hash: ContentHash,
    pub(crate) changed_lines: BTreeSet<u64>,
}

impl SnapshotFile {
    pub(crate) fn line_count(&self) -> u64 {
        u64::try_from(self.content.iter().filter(|byte| **byte == b'\n').count() + 1)
            .expect("usize always fits u64 on supported hosts")
    }
}

/// Fully bounded local Git snapshot consumed by all M2 adapters.
pub(crate) struct GitSnapshot {
    pub(crate) repository_root: String,
    pub(crate) repository_identity: String,
    pub(crate) repository_name: String,
    pub(crate) base_revision: String,
    pub(crate) target_revision: String,
    pub(crate) tree_hash: ContentHash,
    pub(crate) config: crate::IngestConfig,
    pub(crate) files: Vec<SnapshotFile>,
    pub(crate) changes: Vec<ChangeEntry>,
    pub(crate) issues: Vec<IssueDraft>,
    pub(crate) adapter_reports: Vec<AdapterReport>,
    pub(crate) capabilities: BTreeMap<String, CapabilityState>,
    pub(crate) capability_sources: BTreeMap<String, BTreeSet<String>>,
    /// The real `git --version` output for the binary this run actually
    /// executed. Always present: every other Git call in this module
    /// already requires a working `git`, so this is not a new requirement.
    pub(crate) git_version: String,
    /// The real `cargo --version` output for the binary this run actually
    /// executed, or a typed [`CargoToolFailure`] describing why it could
    /// not be determined -- never a raw, unclassified `String`, and never
    /// silently downgraded to an undifferentiated `null`/absence. Always
    /// determined from the *exact same* working directory
    /// (`staged_snapshot`, below) `cargo metadata` itself later runs from,
    /// through the same bounded command policy: the admitted `cargo`
    /// executable itself is already fixed (see `cargo_executable`, below)
    /// and never re-resolved based on cwd, but Cargo's own config and
    /// manifest interpretation -- workspace-root discovery, `.cargo/
    /// config.toml` lookup, and path-relative dependency/patch resolution
    /// -- is directory-relative, so running `cargo --version` and `cargo
    /// metadata` from two different staged copies of otherwise-identical
    /// snapshot content could still resolve that context differently even
    /// though the identical binary produced both outputs. A snapshot need
    /// not contain a Cargo project and a host need not have Cargo installed,
    /// so this alone does not fail the whole request; `extract_cargo_metadata`
    /// instead treats `Err` here as an unconditional precondition failure
    /// and never runs (or accepts facts from) `cargo metadata` in that
    /// case, exactly as if Cargo itself were unavailable -- a `cargo
    /// metadata` result can never be attributed to a verified tool
    /// identity.
    pub(crate) cargo_version: Result<String, CargoToolFailure>,
    /// The absolute, canonicalized `cargo` executable this run admitted (see
    /// `resolve_cargo`/`admit_cargo_executable`, and
    /// `crate::CargoToolAdmission`) and reused for both `cargo_version`
    /// above and, later, `cargo metadata` itself (see
    /// `extract_cargo_metadata`) -- `Some` exactly when `cargo_version` is
    /// `Ok`, mirroring `staged_snapshot`'s own invariant. Deliberately never
    /// serialized into `adapter_set_hash`, `ProgramSpace`, or
    /// `ExtractionReport`: the admitted path is host-specific (it can point
    /// anywhere the caller/harness chose to admit it from), so only the
    /// *version string* it reports -- never this path -- is ever bound into
    /// the fingerprint or any other identity/canonical output.
    pub(crate) cargo_executable: Option<PathBuf>,
    /// The private, disposable staging copy of every accepted snapshot
    /// file, used as the shared working directory for both `cargo_version`
    /// above and, later, `cargo metadata` itself (see `extract_cargo_metadata`).
    /// `None` only when staging itself failed, in which case
    /// `cargo_version` is always `Err` for the same reason and this is
    /// never consulted. Kept alive for the whole snapshot's lifetime (not
    /// re-staged per use) so both really do run from the identical
    /// directory, not merely equivalent copies.
    pub(crate) staged_snapshot: Option<TempDir>,
}

impl GitSnapshot {
    pub(crate) fn file_by_path(&self, path: &str) -> Option<&SnapshotFile> {
        self.files.iter().find(|file| file.path == path)
    }

    pub(crate) fn source_by_path(&self) -> BTreeMap<String, ContentHash> {
        self.files
            .iter()
            .map(|file| (file.path.clone(), file.content_hash.clone()))
            .collect()
    }
}

/// Resolves and reads a Git tree without consulting the target working tree.
pub(crate) fn load_snapshot(request: &IngestRequest) -> Result<GitSnapshot, IngestError> {
    if request.repository_identity.trim().is_empty() {
        return Err(IngestError::InvalidRequest(
            "repository_identity must not be empty".to_owned(),
        ));
    }
    let workspace_root = canonical_workspace_root(&request.workspace_root)?;
    let requested_root = canonical_scoped(&workspace_root, &request.repository_root)?;
    let git_root = canonical_git_root(&workspace_root, &requested_root)?;
    if git_root != requested_root {
        return Err(IngestError::RepositoryRootMismatch {
            requested: requested_root,
            actual: git_root,
        });
    }
    let repository_root = crate::path_from_utf8(&git_root)?;
    let repository_name = git_root
        .file_name()
        .and_then(OsStr::to_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            IngestError::InvalidRequest("repository root has no UTF-8 name".to_owned())
        })?;
    // Bound to provenance below (see `SnapshotIdentities::new`), not just
    // logged: the real executing tool identity, not a hardcoded version
    // string, is part of what `adapter_set_hash` attests to. `git` is
    // already unconditionally required by every call below, so a failure
    // here fails the whole request exactly as an early `rev-parse` failure
    // already would. `cargo` is not: a snapshot need not be a Cargo
    // project, and a host need not have Cargo installed at all, matching
    // `cargo_metadata`'s existing optional-capability contract. `cargo
    // --version` is deliberately determined later (see `staged_snapshot`
    // below, after `files` is built), from the same staged working
    // directory `cargo metadata` itself later runs from -- never from
    // `git_root`, whose Cargo config/manifest interpretation (workspace
    // root, `.cargo/config.toml`, path-relative dependencies) need not
    // match the staged snapshot's own.
    let git_version = git_version(&git_root)?;

    let target_revision = resolve_commit(&git_root, &request.target_revision)?;
    let base_revision = resolve_commit(&git_root, &request.base_revision)?;
    let tree_hash_output = run_git(&git_root, GitCommand::TreeHash(&target_revision))?;
    let tree_hash = ContentHash::parse(format!(
        "git:{}",
        std::str::from_utf8(&tree_hash_output)
            .map_err(|_| IngestError::AdapterOutput("Git tree hash is not UTF-8".to_owned()))?
            .trim()
    ))?;

    let tree_entries = parse_tree(&run_git(&git_root, GitCommand::ListTree(&target_revision))?)?;
    let discovered = u64::try_from(tree_entries.len()).expect("usize fits u64");

    // Computed before the tree-entry loop so an excluded entry (symlink,
    // submodule, or other unsupported mode/type) that Git's own diff also
    // reports changed can be linked to that change record, instead of only
    // ever falling back to the whole-snapshot grounding.
    let changes = parse_changes(&run_git(
        &git_root,
        GitCommand::Changes {
            base: &base_revision,
            target: &target_revision,
        },
    )?)?;
    let change_by_target_path = changes
        .iter()
        .map(|change| (change.target_path.clone(), change))
        .collect::<BTreeMap<_, _>>();

    let mut issues = Vec::new();
    let mut regular_paths = Vec::new();
    let mut blob_sizes = BTreeMap::<String, u64>::new();
    let mut excluded: u64 = 0;
    let mut changed_structure_excluded = false;
    for entry in tree_entries {
        match (entry.mode.as_str(), entry.object_type.as_str()) {
            ("100644" | "100755", "blob") => {
                if let Some(size) = entry.size {
                    blob_sizes.insert(entry.path.clone(), size);
                }
                regular_paths.push(entry.path);
            }
            ("120000", "blob") => {
                excluded += 1;
                let changed_entry = change_by_target_path.get(&entry.path).copied();
                changed_structure_excluded |= changed_entry.is_some();
                issues.push(excluded_entry_issue(
                    IngestionObstructionKind::RegionExcluded,
                    ObstructionSeverity::High,
                    "tracked symlink was not followed while reading the immutable Git snapshot"
                        .to_owned(),
                    entry.path,
                    changed_entry,
                ));
            }
            (_, "commit") => {
                excluded += 1;
                let changed_entry = change_by_target_path.get(&entry.path).copied();
                changed_structure_excluded |= changed_entry.is_some();
                issues.push(excluded_entry_issue(
                    IngestionObstructionKind::UnsupportedInput,
                    ObstructionSeverity::Low,
                    "Git submodule entry was not entered while reading the immutable Git snapshot"
                        .to_owned(),
                    entry.path,
                    changed_entry,
                ));
            }
            _ => {
                excluded += 1;
                let changed_entry = change_by_target_path.get(&entry.path).copied();
                changed_structure_excluded |= changed_entry.is_some();
                issues.push(excluded_entry_issue(
                    IngestionObstructionKind::UnsupportedInput,
                    ObstructionSeverity::Low,
                    format!(
                        "Git tree entry mode `{}` type `{}` is outside the regular-file M2 adapter boundary",
                        entry.mode, entry.object_type
                    ),
                    entry.path,
                    changed_entry,
                ));
            }
        }
    }
    regular_paths.sort();
    if regular_paths.len() > request.config.limits.max_files {
        return Err(IngestError::FileLimitExceeded {
            max_files: request.config.limits.max_files,
        });
    }

    let changed_paths = change_by_target_path
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut files = Vec::new();
    for path in &regular_paths {
        // Precheck against the size `ls-tree -l` already reported, so a
        // grossly oversized blob is rejected without spending a `git show`
        // fetch of its full content first.
        if let Some(&size) = blob_sizes.get(path)
            && usize::try_from(size).is_ok_and(|size| size > request.config.limits.max_file_bytes)
        {
            return Err(IngestError::FileTooLarge {
                path: path.clone(),
                actual_bytes: usize::try_from(size).unwrap_or(usize::MAX),
                max_bytes: request.config.limits.max_file_bytes,
            });
        }
        let content = run_git(
            &git_root,
            GitCommand::ShowFile {
                revision: &target_revision,
                path,
            },
        )?;
        if content.len() > request.config.limits.max_file_bytes {
            return Err(IngestError::FileTooLarge {
                path: path.clone(),
                actual_bytes: content.len(),
                max_bytes: request.config.limits.max_file_bytes,
            });
        }
        let changed_lines = if changed_paths.contains(path) {
            changed_lines(&git_root, &base_revision, &target_revision, path)?
        } else {
            BTreeSet::new()
        };
        files.push(SnapshotFile {
            path: path.clone(),
            content_hash: ContentHash::sha256(&content),
            content,
            changed_lines,
        });
    }
    if files.is_empty() {
        return Err(IngestError::InvalidRequest(
            "target revision contains no regular tracked files".to_owned(),
        ));
    }
    // Staged once, here, and reused as-is by `extract_cargo_metadata`
    // later: `cargo --version` and `cargo metadata` must run against the
    // same admitted `cargo` executable and the exact same staged snapshot
    // and working directory (not merely two independently-staged copies of
    // the same content), so no current or future cwd-sensitive `cargo`
    // behavior can make the two calls attribute version and metadata to
    // different toolchains. A staging failure is treated exactly like
    // `cargo --version` itself failing -- `cargo`'s tool identity simply
    // could not be verified -- rather than failing the whole request,
    // matching every other Cargo-is-optional path in this module.
    //
    // `Disabled` short-circuits before ever calling `stage_files`: staging
    // writes the whole accepted snapshot to a private temporary directory on
    // disk purely so a real `cargo` has something to run against, and no
    // Cargo executable will ever be admitted or invoked for this request, so
    // that disk I/O would be pure waste -- and, per this module's "no host
    // contact beyond what git/rust ingestion itself needs" boundary, contact
    // this crate should not make at all when Cargo tooling was never
    // requested. `resolve_cargo` would reach the identical `NotAdmitted`
    // outcome via `staged.path()` anyway; this only skips the unnecessary
    // staging step to reach it.
    let (staged_snapshot, cargo_executable, cargo_version) = if matches!(
        &request.config.cargo_admission,
        crate::CargoToolAdmission::Disabled
    ) {
        (
            None,
            None,
            Err(CargoToolFailure {
                kind: CargoToolFailureKind::NotAdmitted,
                diagnostic: "cargo_admission is Disabled; no Cargo executable was admitted \
                        for this ingest request"
                    .to_owned(),
            }),
        )
    } else {
        match stage_files(&files) {
            Ok(staged) => match resolve_cargo(&request.config.cargo_admission, staged.path()) {
                Ok((executable, version)) => (Some(staged), Some(executable), Ok(version)),
                Err(failure) => (Some(staged), None, Err(failure)),
            },
            Err(error) => (
                None,
                None,
                Err(CargoToolFailure {
                    kind: CargoToolFailureKind::StagingFailed,
                    diagnostic: format!(
                        "could not stage the snapshot to verify cargo's tool identity from \
                             the same working directory `cargo metadata` would use: {error}"
                    ),
                }),
            ),
        }
    };
    // `git_snapshot` covers every accepted regular-file record; any excluded
    // entry (symlink, submodule, other unsupported mode/type) is a real gap
    // in that snapshot, so the capability can never stay `complete` once one
    // exists. `changed_structure` only downgrades when an excluded entry is
    // also one Git's own diff reports changed: that is the only case where
    // the changed-structure mapping is actually missing a fact (an excluded
    // entry that never changed between base and target has nothing to map).
    let mut capabilities = BTreeMap::new();
    capabilities.insert(
        "git_snapshot".to_owned(),
        if excluded > 0 {
            CapabilityState::Partial
        } else {
            CapabilityState::Complete
        },
    );
    capabilities.insert(
        "changed_structure".to_owned(),
        if changed_structure_excluded {
            CapabilityState::Partial
        } else {
            CapabilityState::Complete
        },
    );
    let git_snapshot_source_keys = regular_paths
        .iter()
        .map(|path| format!("file:{path}"))
        .collect::<BTreeSet<_>>();
    let changed_structure_source_keys = changes.iter().map(change_key).collect::<BTreeSet<_>>();
    let status = if issues.is_empty() {
        AdapterStatus::Complete
    } else {
        AdapterStatus::Partial
    };
    Ok(GitSnapshot {
        repository_root,
        repository_identity: request.repository_identity.clone(),
        repository_name,
        base_revision,
        target_revision,
        tree_hash,
        config: request.config.clone(),
        files,
        changes,
        issues,
        adapter_reports: vec![AdapterReport {
            id: "reviewgraphen.ingest.git".to_owned(),
            version: "1".to_owned(),
            status,
            parsed: Some(u64::try_from(regular_paths.len()).expect("usize fits u64")),
            total: Some(discovered),
            excluded: Some(excluded),
            failed: Some(0),
        }],
        capabilities,
        capability_sources: BTreeMap::from([
            ("git_snapshot".to_owned(), git_snapshot_source_keys),
            (
                "changed_structure".to_owned(),
                changed_structure_source_keys,
            ),
        ]),
        git_version,
        cargo_version,
        cargo_executable,
        staged_snapshot,
    })
}

/// Builds one `IssueDraft` for a Git tree entry the M2 adapter boundary
/// excludes (symlink, submodule, or another unsupported mode/type). Always
/// related to `git_snapshot`, since the entry is unconditionally missing
/// from the accepted snapshot; additionally related to, and source-keyed
/// against, `changed_structure`'s own `change:*` fact only when Git's own
/// diff also reports this exact path changed between base and target --
/// that is the only case where the changed-structure mapping actually loses
/// a fact because of the exclusion.
fn excluded_entry_issue(
    kind: IngestionObstructionKind,
    severity: ObstructionSeverity,
    description: String,
    path: String,
    changed_entry: Option<&ChangeEntry>,
) -> IssueDraft {
    let mut related_capabilities = BTreeSet::from(["git_snapshot".to_owned()]);
    let mut source_keys = BTreeSet::new();
    if let Some(change) = changed_entry {
        related_capabilities.insert("changed_structure".to_owned());
        source_keys.insert(change_key(change));
    }
    IssueDraft {
        kind,
        severity,
        description,
        source_keys,
        paths: BTreeSet::from([path]),
        related_capabilities,
    }
}

/// Runs Cargo metadata only against a private, byte-for-byte staged snapshot.
pub(crate) fn extract_cargo_metadata(
    snapshot: &GitSnapshot,
    _snapshot_id: &StableId,
) -> CargoExtraction {
    let mut capabilities = BTreeMap::new();
    // A precondition, checked before anything else: `cargo metadata`'s
    // result is only ever accepted when it can be attributed to a verified
    // `cargo` tool identity (see `SnapshotIdentities::new`'s
    // `tool_versions.cargo`). Running -- and accepting facts from --
    // `cargo metadata` while `cargo --version` itself failed would let a
    // real result sit behind a fingerprint that cannot actually vouch for
    // which `cargo` produced it; that is treated exactly like Cargo being
    // unavailable at all, never as a `complete`/`partial` run with an
    // unattributed provenance gap.
    if let Err(failure) = &snapshot.cargo_version {
        // `description` is built only from the stable `kind` and fixed
        // wording -- never from `failure.diagnostic` -- so it can double as
        // this limitation's own identity input (see `lift`'s
        // `derived_id("limitation", ...)` call) the same way every other
        // obstruction's `description` already does. The raw subprocess
        // diagnostic stays on the internal, non-canonical `CargoToolFailure`
        // value; it is never folded into `ProgramSpace`/`ExtractionReport`.
        capabilities.insert("cargo_metadata".to_owned(), CapabilityState::Missing);
        return CargoExtraction::unavailable(
            AdapterStatus::NotRun,
            capabilities,
            &format!(
                "cargo --version could not be determined ({}), so Cargo metadata was \
                 not run: its result could never be attributed to a verified tool identity",
                failure.kind.as_str(),
            ),
        );
    }
    let cargo_toml = snapshot.file_by_path("Cargo.toml");
    if cargo_toml.is_none() {
        capabilities.insert("cargo_metadata".to_owned(), CapabilityState::Missing);
        return CargoExtraction::unavailable(
            AdapterStatus::NotRun,
            capabilities,
            "snapshot has no root Cargo.toml; Cargo metadata was not run",
        );
    }
    let manifest_violation = snapshot
        .files
        .iter()
        .filter(|file| file.path.ends_with("Cargo.toml"))
        .find_map(|file| {
            let manifest_dir = manifest_directory(&file.path);
            cargo_manifest_containment_violation(&file.content, &manifest_dir)
                .map(|reason| (file.path.clone(), reason))
        });
    if let Some((path, reason)) = manifest_violation {
        capabilities.insert("cargo_metadata".to_owned(), CapabilityState::Missing);
        return CargoExtraction::unavailable(
            AdapterStatus::NotRun,
            capabilities,
            &format!("Cargo metadata was not run because `{path}` {reason}"),
        );
    }
    // `cargo_version` above is `Ok` only when staging already succeeded
    // (see `load_snapshot`), so this is always `Some` here -- the same
    // staged directory `cargo_version` was itself determined from, not a
    // freshly re-staged copy.
    let staged = snapshot
        .staged_snapshot
        .as_ref()
        .expect("staged_snapshot is always Some once cargo_version is Ok (see load_snapshot)");
    // Likewise: the exact same admitted `cargo` executable `cargo_version`
    // was itself determined from, never a freshly re-admitted one -- this
    // must never let `cargo metadata` run against a different binary than
    // the one whose version this run already attested to.
    let executable = snapshot
        .cargo_executable
        .as_ref()
        .expect("cargo_executable is always Some once cargo_version is Ok (see load_snapshot)");
    let result = run_cargo_metadata(executable, staged.path(), "Cargo.toml");
    let metadata = match result {
        Ok(output) => match serde_json::from_slice::<Value>(&output) {
            Ok(value) => value,
            Err(error) => {
                capabilities.insert("cargo_metadata".to_owned(), CapabilityState::Missing);
                return CargoExtraction::unavailable(
                    AdapterStatus::Failed,
                    capabilities,
                    &format!("Cargo metadata emitted invalid JSON: {error}"),
                );
            }
        },
        Err(error) => {
            capabilities.insert("cargo_metadata".to_owned(), CapabilityState::Missing);
            return CargoExtraction::unavailable(
                AdapterStatus::Failed,
                capabilities,
                &format!("Cargo metadata was unavailable: {error}"),
            );
        }
    };
    let Some(packages) = metadata.get("packages").and_then(Value::as_array) else {
        capabilities.insert("cargo_metadata".to_owned(), CapabilityState::Missing);
        return CargoExtraction::unavailable(
            AdapterStatus::Failed,
            capabilities,
            "Cargo metadata JSON contains no packages array",
        );
    };

    let mut artifacts = Vec::new();
    let mut relations = Vec::new();
    let mut issues = Vec::new();
    let mut package_keys = BTreeMap::<(String, String), String>::new();
    // Pass 1: register every accepted local package (and its artifact
    // draft) before resolving any dependency, so a dependency naming a
    // package that appears later in `packages[]` still resolves.
    let mut local_packages = Vec::new();
    for package in packages {
        let Some(name) = package.get("name").and_then(Value::as_str) else {
            continue;
        };
        let version = package
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let manifest_path = package
            .get("manifest_path")
            .and_then(Value::as_str)
            .and_then(|value| relative_staged_path(staged.path(), value));
        let Some(manifest_path) = manifest_path else {
            continue;
        };
        let key = format!("package:{manifest_path}:{name}@{version}");
        package_keys.insert((manifest_path.clone(), name.to_owned()), key.clone());
        local_packages.push(LocalPackage {
            key: key.clone(),
            name: name.to_owned(),
            manifest_path: manifest_path.clone(),
        });
        let mut attributes = Map::new();
        attributes.insert("version".to_owned(), Value::String(version.to_owned()));
        attributes.insert(
            "manifest_path".to_owned(),
            Value::String(manifest_path.clone()),
        );
        artifacts.push(ArtifactDraft {
            key,
            id_kind: "package",
            kind: "package",
            label: name.to_owned(),
            language: Some("rust"),
            location: Some(LocationDraft {
                path: manifest_path.clone(),
                start_line: 1,
                end_line: 1,
                start_column: 1,
                end_column: 1,
            }),
            content_hash: snapshot
                .file_by_path(&manifest_path)
                .map(|file| file.content_hash.clone()),
            attributes,
            source_path: Some(manifest_path.clone()),
            extraction_method: "reviewgraphen.ingest.cargo_metadata.v1",
        });
    }

    // Pass 2: resolve every declared dependency. A dependency with a
    // non-null `source` (registry/git) is unambiguously external. A local
    // (`source: null`) dependency -- typically a `path` dependency -- is
    // accepted as a `depends_on` edge to an existing accepted local
    // package artifact only when it resolves to *exactly one* of them;
    // never to a fabricated external stub, and never guessed when
    // ambiguous or unmatched (a non-workspace-member local path
    // dependency, which `--no-deps` never turns into a `packages[]`
    // entry, correctly falls into this unresolved case too).
    let mut has_unresolved_local_dependency = false;
    for package in packages {
        let (Some(name), Some(manifest_path)) = (
            package.get("name").and_then(Value::as_str),
            package
                .get("manifest_path")
                .and_then(Value::as_str)
                .and_then(|value| relative_staged_path(staged.path(), value)),
        ) else {
            continue;
        };
        let source_key = package_keys
            .get(&(manifest_path.clone(), name.to_owned()))
            .expect("package key is inserted in pass 1")
            .clone();
        let dependencies = package
            .get("dependencies")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for dependency in dependencies {
            let Some(dependency_name) = dependency.get("name").and_then(Value::as_str) else {
                continue;
            };
            let requirement = dependency.get("req").and_then(Value::as_str).unwrap_or("*");
            match resolve_dependency(&dependency, staged.path(), &local_packages) {
                DependencyResolution::External => {
                    let dependency_key =
                        format!("external-package:{manifest_path}:{dependency_name}:{requirement}");
                    artifacts.push(ArtifactDraft {
                        key: dependency_key.clone(),
                        id_kind: "package",
                        kind: "package",
                        label: dependency_name.to_owned(),
                        language: Some("rust"),
                        location: None,
                        content_hash: None,
                        attributes: Map::from_iter([
                            ("external".to_owned(), Value::Bool(true)),
                            (
                                "requirement".to_owned(),
                                Value::String(requirement.to_owned()),
                            ),
                        ]),
                        source_path: Some(manifest_path.clone()),
                        extraction_method: "reviewgraphen.ingest.cargo_metadata.v1",
                    });
                    relations.push(RelationDraft {
                        kind: "depends_on",
                        source_key: source_key.clone(),
                        target_keys: BTreeSet::from([dependency_key]),
                        attributes: Map::from_iter([(
                            "requirement".to_owned(),
                            Value::String(requirement.to_owned()),
                        )]),
                        source_path: Some(manifest_path.clone()),
                        extraction_method: "reviewgraphen.ingest.cargo_metadata.v1",
                    });
                }
                DependencyResolution::Internal(target_key) => {
                    relations.push(RelationDraft {
                        kind: "depends_on",
                        source_key: source_key.clone(),
                        target_keys: BTreeSet::from([target_key]),
                        attributes: Map::from_iter([(
                            "requirement".to_owned(),
                            Value::String(requirement.to_owned()),
                        )]),
                        source_path: Some(manifest_path.clone()),
                        extraction_method: "reviewgraphen.ingest.cargo_metadata.v1",
                    });
                }
                DependencyResolution::Unresolved => {
                    has_unresolved_local_dependency = true;
                    issues.push(IssueDraft {
                        kind: IngestionObstructionKind::RelationUnresolved,
                        severity: ObstructionSeverity::Medium,
                        description: format!(
                            "local Cargo dependency `{dependency_name}` of package `{name}` \
                             did not resolve to exactly one accepted local package; it was \
                             retained as unresolved rather than guessed or treated as external"
                        ),
                        source_keys: BTreeSet::from([source_key.clone()]),
                        paths: BTreeSet::from([manifest_path.clone()]),
                        related_capabilities: BTreeSet::from(["cargo_metadata".to_owned()]),
                    });
                }
            }
        }
    }
    capabilities.insert(
        "cargo_metadata".to_owned(),
        if has_unresolved_local_dependency {
            CapabilityState::Partial
        } else {
            CapabilityState::Complete
        },
    );
    let cargo_source_keys = artifacts
        .iter()
        .filter(|artifact| artifact.id_kind == "package")
        .map(|artifact| artifact.key.clone())
        .collect::<BTreeSet<_>>();
    CargoExtraction {
        artifacts,
        relations,
        issues,
        capabilities,
        capability_sources: BTreeMap::from([("cargo_metadata".to_owned(), cargo_source_keys)]),
        adapter_report: AdapterReport {
            id: "reviewgraphen.ingest.cargo_metadata".to_owned(),
            version: "1".to_owned(),
            status: if has_unresolved_local_dependency {
                AdapterStatus::Partial
            } else {
                AdapterStatus::Complete
            },
            parsed: Some(u64::try_from(packages.len()).expect("usize fits u64")),
            total: Some(u64::try_from(packages.len()).expect("usize fits u64")),
            excluded: None,
            failed: None,
        },
    }
}

/// One accepted local package this Cargo metadata run already produced,
/// used to resolve another package's declared dependency against it.
struct LocalPackage {
    key: String,
    name: String,
    manifest_path: String,
}

/// Resolution outcome for one declared dependency entry.
enum DependencyResolution {
    /// A registry/git dependency (`source` is a non-null string): always
    /// external, regardless of anything else about the entry.
    External,
    /// Resolved to exactly one accepted local package, by its draft key.
    Internal(String),
    /// A local (`source: null`) dependency that did not resolve to exactly
    /// one accepted local package: no name match, an ambiguous multi-match,
    /// or a `path` outside the staged snapshot. Never guessed.
    Unresolved,
}

/// Resolves one Cargo `dependencies[]` entry against the packages this
/// metadata run already accepted. `source` is Cargo's own authoritative
/// external-vs-local signal: non-null means registry/git (external);
/// `null` means path/workspace-local. A local dependency is matched by
/// name among `local_packages`, further narrowed by its `path` field (its
/// declaring directory, resolved relative to the staged snapshot) when
/// present; anything other than exactly one surviving candidate is
/// reported unresolved rather than guessed.
fn resolve_dependency(
    dependency: &Value,
    staged_root: &Path,
    local_packages: &[LocalPackage],
) -> DependencyResolution {
    if dependency.get("source").and_then(Value::as_str).is_some() {
        return DependencyResolution::External;
    }
    let Some(name) = dependency.get("name").and_then(Value::as_str) else {
        return DependencyResolution::Unresolved;
    };
    let mut candidates = local_packages
        .iter()
        .filter(|package| package.name == name)
        .collect::<Vec<_>>();
    if let Some(path) = dependency.get("path").and_then(Value::as_str) {
        let Some(resolved_dir) = relative_staged_path(staged_root, path) else {
            return DependencyResolution::Unresolved;
        };
        candidates.retain(|package| manifest_directory(&package.manifest_path) == resolved_dir);
    }
    match candidates.as_slice() {
        [package] => DependencyResolution::Internal(package.key.clone()),
        _ => DependencyResolution::Unresolved,
    }
}

pub(crate) struct CargoExtraction {
    pub(crate) artifacts: Vec<ArtifactDraft>,
    pub(crate) relations: Vec<RelationDraft>,
    pub(crate) issues: Vec<IssueDraft>,
    pub(crate) capabilities: BTreeMap<String, CapabilityState>,
    pub(crate) capability_sources: BTreeMap<String, BTreeSet<String>>,
    pub(crate) adapter_report: AdapterReport,
}

impl CargoExtraction {
    fn unavailable(
        status: AdapterStatus,
        capabilities: BTreeMap<String, CapabilityState>,
        description: &str,
    ) -> Self {
        Self {
            artifacts: Vec::new(),
            relations: Vec::new(),
            issues: vec![IssueDraft {
                kind: IngestionObstructionKind::CargoMetadataUnavailable,
                severity: ObstructionSeverity::Medium,
                description: description.to_owned(),
                source_keys: BTreeSet::new(),
                paths: BTreeSet::from(["Cargo.toml".to_owned()]),
                related_capabilities: BTreeSet::from(["cargo_metadata".to_owned()]),
            }],
            capabilities,
            capability_sources: BTreeMap::new(),
            adapter_report: AdapterReport {
                id: "reviewgraphen.ingest.cargo_metadata".to_owned(),
                version: "1".to_owned(),
                status,
                parsed: None,
                total: None,
                excluded: None,
                failed: None,
            },
        }
    }
}

#[derive(Debug)]
struct TreeEntry {
    mode: String,
    /// Git object type (`blob`, `commit` for a submodule, or `tree`, though
    /// `-r` recursion never leaves a bare `tree` entry). Retained instead of
    /// rejected: a non-`blob` entry is a typed obstruction, not a fatal
    /// adapter failure -- one Git submodule must not abort ingestion of an
    /// otherwise bounded, valid snapshot.
    object_type: String,
    /// Blob byte size reported by `git ls-tree -l`, when the entry is a
    /// blob (Git reports `-` for a non-blob entry, parsed here as `None`).
    /// Lets a grossly oversized blob be rejected before the separate
    /// `git show` fetch of its full content.
    size: Option<u64>,
    path: String,
}

fn parse_tree(output: &[u8]) -> Result<Vec<TreeEntry>, IngestError> {
    output
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let Some(tab) = entry.iter().position(|byte| *byte == b'\t') else {
                return Err(IngestError::AdapterOutput(
                    "malformed NUL-delimited git ls-tree entry".to_owned(),
                ));
            };
            let (header, raw_path) = (&entry[..tab], &entry[tab + 1..]);
            // `-l` right-pads the size column, so consecutive spaces before
            // it are collapsed rather than split into empty fields.
            let fields = header
                .split(|byte| *byte == b' ')
                .filter(|field| !field.is_empty())
                .collect::<Vec<_>>();
            if fields.len() != 4 {
                return Err(IngestError::AdapterOutput(
                    "malformed git ls-tree entry header".to_owned(),
                ));
            }
            let mode = std::str::from_utf8(fields[0])
                .map_err(|_| IngestError::AdapterOutput("Git mode is not UTF-8".to_owned()))?
                .to_owned();
            let object_type = std::str::from_utf8(fields[1])
                .map_err(|_| IngestError::AdapterOutput("Git object type is not UTF-8".to_owned()))?
                .to_owned();
            let size = std::str::from_utf8(fields[3])
                .map_err(|_| IngestError::AdapterOutput("Git blob size is not UTF-8".to_owned()))?
                .parse::<u64>()
                .ok();
            let path = std::str::from_utf8(raw_path)
                .map_err(|_| IngestError::AdapterOutput("Git path is not UTF-8".to_owned()))?
                .to_owned();
            validate_tree_path(&path)?;
            Ok(TreeEntry {
                mode,
                object_type,
                size,
                path,
            })
        })
        .collect()
}

fn parse_changes(output: &[u8]) -> Result<Vec<ChangeEntry>, IngestError> {
    let entries = output
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    let mut index = 0;
    let mut changes = Vec::new();
    while index < entries.len() {
        let status = std::str::from_utf8(entries[index])
            .map_err(|_| IngestError::AdapterOutput("Git status is not UTF-8".to_owned()))?;
        index += 1;
        let kind = match status.as_bytes().first().copied() {
            Some(b'A') => ChangeKind::Added,
            Some(b'M') => ChangeKind::Modified,
            Some(b'D') => ChangeKind::Deleted,
            Some(b'R') => ChangeKind::Renamed,
            Some(b'C') => ChangeKind::Copied,
            Some(b'T') => ChangeKind::TypeChanged,
            _ => {
                return Err(IngestError::AdapterOutput(format!(
                    "unsupported git name-status entry `{status}`"
                )));
            }
        };
        let first = entries.get(index).ok_or_else(|| {
            IngestError::AdapterOutput("truncated git name-status path".to_owned())
        })?;
        index += 1;
        let first = std::str::from_utf8(first)
            .map_err(|_| IngestError::AdapterOutput("Git path is not UTF-8".to_owned()))?
            .to_owned();
        validate_tree_path(&first)?;
        let (base_path, target_path) = match kind {
            ChangeKind::Renamed | ChangeKind::Copied => {
                let second = entries.get(index).ok_or_else(|| {
                    IngestError::AdapterOutput("truncated Git rename/copy entry".to_owned())
                })?;
                index += 1;
                let second = std::str::from_utf8(second)
                    .map_err(|_| IngestError::AdapterOutput("Git path is not UTF-8".to_owned()))?
                    .to_owned();
                validate_tree_path(&second)?;
                (first, second)
            }
            ChangeKind::Added => (String::new(), first),
            ChangeKind::Deleted => (first, String::new()),
            ChangeKind::Modified | ChangeKind::TypeChanged => (first.clone(), first),
        };
        changes.push(ChangeEntry {
            kind,
            base_path,
            target_path,
        });
    }
    changes.sort_by(|left, right| {
        (
            left.target_path.as_str(),
            left.base_path.as_str(),
            left.kind,
        )
            .cmp(&(
                right.target_path.as_str(),
                right.base_path.as_str(),
                right.kind,
            ))
    });
    Ok(changes)
}

fn changed_lines(
    root: &Path,
    base: &str,
    target: &str,
    path: &str,
) -> Result<BTreeSet<u64>, IngestError> {
    let output = run_git(root, GitCommand::ChangedLines { base, target, path })?;
    let mut result = BTreeSet::new();
    for line in String::from_utf8_lossy(&output).lines() {
        let Some(header) = line.strip_prefix("@@ ") else {
            continue;
        };
        let Some(plus) = header.split_whitespace().find(|part| part.starts_with('+')) else {
            continue;
        };
        let range = plus.trim_start_matches('+');
        let (start, count) = match range.split_once(',') {
            Some((start, count)) => (start, count),
            None => (range, "1"),
        };
        let start = start.parse::<u64>().map_err(|_| {
            IngestError::AdapterOutput("Git diff hunk start is not an integer".to_owned())
        })?;
        let count = count.parse::<u64>().map_err(|_| {
            IngestError::AdapterOutput("Git diff hunk count is not an integer".to_owned())
        })?;
        for line_number in start..start.saturating_add(count) {
            result.insert(line_number);
        }
    }
    Ok(result)
}

fn validate_tree_path(path: &str) -> Result<(), IngestError> {
    let candidate = Path::new(path);
    if path.is_empty()
        || candidate.is_absolute()
        || candidate.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(IngestError::PathEscape {
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn canonical_workspace_root(path: &Path) -> Result<PathBuf, IngestError> {
    fs::canonicalize(path).map_err(|source| IngestError::CommandIo {
        command: "canonicalize workspace root",
        source,
    })
}

fn canonical_scoped(workspace: &Path, candidate: &Path) -> Result<PathBuf, IngestError> {
    let candidate = fs::canonicalize(candidate).map_err(|source| IngestError::CommandIo {
        command: "canonicalize repository root",
        source,
    })?;
    if candidate.starts_with(workspace) {
        Ok(candidate)
    } else {
        Err(IngestError::WorkspaceEscape {
            path: candidate,
            workspace: workspace.to_path_buf(),
        })
    }
}

fn canonical_git_root(workspace: &Path, root: &Path) -> Result<PathBuf, IngestError> {
    let output = run_git(root, GitCommand::Root)?;
    let root_text = std::str::from_utf8(&output)
        .map_err(|_| IngestError::AdapterOutput("Git root is not UTF-8".to_owned()))?
        .trim();
    canonical_scoped(workspace, Path::new(root_text))
}

fn resolve_commit(root: &Path, candidate: &str) -> Result<String, IngestError> {
    if candidate.is_empty() || candidate.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(IngestError::InvalidRequest(
            "Git revision must be non-empty and contain no control bytes".to_owned(),
        ));
    }
    let output = run_git(root, GitCommand::ResolveCommit(candidate))?;
    let resolved = std::str::from_utf8(&output)
        .map_err(|_| IngestError::AdapterOutput("resolved Git revision is not UTF-8".to_owned()))?
        .trim()
        .to_owned();
    if resolved.len() != 40 || !resolved.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(IngestError::AdapterOutput(
            "Git did not return a full hexadecimal commit ID".to_owned(),
        ));
    }
    Ok(resolved.to_ascii_lowercase())
}

enum GitCommand<'a> {
    Root,
    ResolveCommit(&'a str),
    TreeHash(&'a str),
    ListTree(&'a str),
    ShowFile {
        revision: &'a str,
        path: &'a str,
    },
    Changes {
        base: &'a str,
        target: &'a str,
    },
    ChangedLines {
        base: &'a str,
        target: &'a str,
        path: &'a str,
    },
}

/// Builds the one deterministic policy shared by every allow-listed `git`
/// invocation. The child's environment is stripped to `GIT_INHERITED_ENV_VARS`
/// and system/global Git config plus a per-user global gitattributes file
/// are disabled outright, so a snapshot's accepted facts can never depend on
/// the executing host's or user's Git configuration -- only the repository's
/// own `.git/config`/`.gitattributes` (part of the snapshot itself) still
/// apply, and even that is overridden per command wherever it could change
/// which facts are extracted (see the `GitCommand` match arms in `run_git`).
/// Command-specific arguments are appended by the caller.
fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.env_clear();
    for var in GIT_INHERITED_ENV_VARS {
        if let Ok(value) = env::var(var) {
            command.env(var, value);
        }
    }
    command
        .current_dir(root)
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("LC_ALL", "C")
        .arg("--no-pager")
        .arg("--no-optional-locks")
        // A replacement ref (`refs/replace/<object>`, see `git-replace(1)`)
        // transparently substitutes a different object wherever the
        // original is read -- including every `rev-parse`/`ls-tree`/
        // `show`/`diff` call this module makes. Two clones of the exact
        // same real history can disagree only in which replacement refs
        // they happen to carry, silently producing different resolved
        // commits/trees/blobs/diffs from the same requested revision.
        // Disabling replacement outright makes every read resolve the
        // literal requested object, regardless of what replacement refs
        // either clone happens to carry.
        .arg("--no-replace-objects")
        // `GIT_CONFIG_GLOBAL` above only stops Git from reading a *config*
        // file that might point `core.attributesFile` elsewhere; Git
        // separately consults a default per-user attributes file
        // (`$XDG_CONFIG_HOME/git/attributes` or
        // `$HOME/.config/git/attributes`) unconditionally, not only when a
        // config file sets it. Pointing it at `/dev/null` closes that gap;
        // the repository's own tracked `.gitattributes` (real snapshot
        // content, not ambient environment) is unaffected.
        .arg("-c")
        .arg("core.attributesFile=/dev/null");
    command
}

fn run_git(root: &Path, command: GitCommand<'_>) -> Result<Vec<u8>, IngestError> {
    let child = build_git_command(root, command);
    checked_output(
        "git",
        run_with_bounds(
            "git",
            child,
            SUBPROCESS_TIMEOUT,
            SUBPROCESS_MAX_OUTPUT_BYTES,
        ),
    )
}

/// Applies one `GitCommand`'s specific arguments on top of the shared
/// `git_command` policy. Split out from `run_git` so the exact argument
/// list for a given `GitCommand` variant is directly unit-testable without
/// spawning a real subprocess.
fn build_git_command(root: &Path, command: GitCommand<'_>) -> Command {
    let mut child = git_command(root);
    match command {
        GitCommand::Root => {
            child.args(["rev-parse", "--show-toplevel"]);
        }
        GitCommand::ResolveCommit(revision) => {
            child.args(["rev-parse", "--verify", "--quiet", "--end-of-options"]);
            child.arg(format!("{revision}^{{commit}}"));
        }
        GitCommand::TreeHash(revision) => {
            child.args(["rev-parse", "--verify", "--quiet"]);
            child.arg(format!("{revision}^{{tree}}"));
        }
        GitCommand::ListTree(revision) => {
            // `-l` additionally reports each blob's byte size, so oversized
            // blobs can be rejected before spending a `git show` fetch on
            // them (see the blob-size precheck in `load_snapshot`).
            child.args(["ls-tree", "-r", "-l", "-z", "--full-tree", revision, "--"]);
        }
        GitCommand::ShowFile { revision, path } => {
            child.args(["show", "--no-ext-diff", "--no-textconv", "--format="]);
            child.arg(format!("{revision}:{path}"));
        }
        GitCommand::Changes { base, target } => {
            child.args([
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                // Overrides a repo-local `color.ui=always`/`color.diff=always`
                // (never tracked history, so it can differ freely between
                // clones of the identical commits) so this `git diff`'s
                // output can never carry ANSI escapes, matching every other
                // `git diff` invocation in this module.
                "--no-color",
                // Overrides any `binary`/`-diff` `.gitattributes`
                // classification (tracked snapshot content, not ambient
                // environment -- see `--text` on `ChangedLines` below for
                // why the diff *policy* still needs to be fixed) and
                // Git's own content-sniffing heuristic. `--name-status`
                // itself reports the same `A`/`M`/`D`/`R`/`C`/`T` status
                // letters regardless of binary/text classification, so
                // this is a no-op for its own output, but keeps the diff
                // policy uniform across every `git diff` invocation
                // rather than leaving one silently unpinned.
                "--text",
                "--name-status",
                "-z",
            ]);
            // Fixed similarity threshold and detection limit, overriding
            // any repo `diff.renames`/`diff.renameLimit` config: the same
            // base/target pair must report the same `Renamed`/`Copied`
            // facts regardless of the cloned repo's own settings.
            child.arg(format!("--find-renames={GIT_RENAME_SIMILARITY_PERCENT}%"));
            child.arg(format!("--find-copies={GIT_RENAME_SIMILARITY_PERCENT}%"));
            child.arg(format!("-l{GIT_RENAME_LIMIT}"));
            child.args([base, target, "--"]);
        }
        GitCommand::ChangedLines { base, target, path } => {
            child.args(["diff", "--no-ext-diff", "--no-textconv"]);
            // Without this, a repo-local `color.ui=always`/
            // `color.diff=always` (never tracked history, so it can differ
            // freely between clones of the identical commits) wraps the
            // `@@ ...` hunk header in ANSI escapes, so `changed_lines`'s
            // `line.strip_prefix("@@ ")` match would silently stop
            // matching and empty out the fact -- `--no-color` forces every
            // hunk header to stay plain text unconditionally, so this fact
            // never depends on that config.
            child.arg("--no-color");
            // Without this, a path Git (or a `binary`/`-diff`
            // `.gitattributes` declaration) classifies as binary gets
            // "Binary files ... differ" with *no* `@@` hunks at all,
            // silently making `changed_lines` empty despite a real,
            // parseable text change -- `--text` forces every path to be
            // diffed as text unconditionally, so this fact never depends
            // on that classification.
            child.arg("--text");
            child.arg(format!("--diff-algorithm={GIT_DIFF_ALGORITHM}"));
            child.args(["--unified=0", base, target, "--", path]);
        }
    }
    child
}

/// Environment variables set -- and, thanks to `Command::env_clear()`,
/// exclusively set -- on every allow-listed `cargo` subprocess (`cargo
/// --version` and `cargo metadata` alike, see `cargo_version` and
/// `run_cargo_metadata` below). Clearing first means ambient shell state --
/// `PATH`, `RUSTUP_TOOLCHAIN`, any `MISE_*` variable, `RUSTC`, `RUSTFLAGS`,
/// `CARGO_TARGET_DIR`, or any other ambient `CARGO_*` -- can never reach the
/// admitted binary and silently change which toolchain, target directory,
/// or registry it actually uses for an unchanged admitted executable and
/// staged snapshot. `PATH` is deliberately not propagated back in, unlike
/// `git_command`: `cargo` is always invoked by its already-canonicalized
/// absolute path (see `resolve_cargo`/`admit_cargo_executable`), never
/// looked up on `PATH`, so the child process has no need for it.
fn cargo_command(executable: &Path, staged_root: &Path) -> Command {
    let mut command = Command::new(executable);
    command.env_clear();
    command
        .current_dir(staged_root)
        // Keeps Cargo's registry/config/cache writes inside the disposable
        // staged snapshot rather than a real, host-shared CARGO_HOME.
        .env("CARGO_HOME", staged_root.join(".reviewgraphen-cargo-home"))
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TERM_COLOR", "never")
        .env("LC_ALL", "C");
    command
}

fn run_cargo_metadata(
    executable: &Path,
    root: &Path,
    manifest_path: &str,
) -> Result<Vec<u8>, IngestError> {
    let mut child = cargo_command(executable, root);
    // `--offline --no-deps` needs no target repository state.
    child.args([
        "metadata",
        "--offline",
        "--no-deps",
        "--format-version=1",
        "--manifest-path",
        manifest_path,
    ]);
    checked_output(
        "cargo metadata",
        run_with_bounds(
            "cargo metadata",
            child,
            SUBPROCESS_TIMEOUT,
            SUBPROCESS_MAX_OUTPUT_BYTES,
        ),
    )
}

/// The real `git --version` output for the `git` binary this run actually
/// executes, through the same bounded allow-list executor as every other
/// Git call -- never a hardcoded or guessed string, and never silently
/// absorbed into a placeholder on failure.
fn git_version(root: &Path) -> Result<String, IngestError> {
    let mut child = git_command(root);
    child.arg("version");
    command_version("git --version", child)
}

/// Admits the `cargo` executable this run will invoke, per
/// `crate::CargoToolAdmission`, then runs its `--version` from
/// `staged_root` -- the same working directory `cargo metadata` itself
/// later runs from (see `extract_cargo_metadata`). Returns both:
/// `extract_cargo_metadata` must reuse the exact admitted path, never
/// re-admit independently, so the version it attests to can never silently
/// diverge from the binary that actually ran `cargo metadata`.
///
/// This module never searches `PATH` and never spawns `rustup`/`mise`/
/// `asdf`: `Disabled` runs no Cargo metadata at all, and
/// `TrustedExecutable(path)` requires the caller/harness to have already
/// vouched for `path` as a real host `cargo`, outside this crate's own
/// trust boundary -- see docs/20 for the documented reasoning (a runtime
/// `rustup` probe can itself touch the toolchain even under a "never
/// install" configuration, so no automatic resolution from inside this
/// crate can be made safe).
pub(crate) fn resolve_cargo(
    admission: &crate::CargoToolAdmission,
    staged_root: &Path,
) -> Result<(PathBuf, String), CargoToolFailure> {
    let trusted_path = match admission {
        crate::CargoToolAdmission::Disabled => {
            return Err(CargoToolFailure {
                kind: CargoToolFailureKind::NotAdmitted,
                diagnostic: "cargo_admission is Disabled; no Cargo executable was admitted for \
                    this ingest request"
                    .to_owned(),
            });
        }
        crate::CargoToolAdmission::TrustedExecutable(path) => path,
    };
    let executable = admit_cargo_executable(trusted_path)?;
    let version = cargo_version(&executable, staged_root)
        .map_err(|error| cargo_tool_failure(&error, staged_root))?;
    Ok((executable, version))
}

/// Admits `path` -- an already caller-verified absolute `cargo` executable
/// (see `crate::CargoToolAdmission::TrustedExecutable`) -- as the exact
/// binary this run will invoke, without ever consulting `PATH` or spawning
/// `rustup`/`mise`/`asdf`. `path` must be absolute; it is then canonicalized
/// and checked to be an existing regular, executable file. A relative path,
/// a path that fails to canonicalize, or one that resolves to a directory
/// or another non-regular/non-executable entry is a typed
/// `CargoToolFailureKind::AdmittedExecutableInvalid` failure, never trusted
/// as-is.
fn admit_cargo_executable(path: &Path) -> Result<PathBuf, CargoToolFailure> {
    if !path.is_absolute() {
        return Err(CargoToolFailure {
            kind: CargoToolFailureKind::AdmittedExecutableInvalid,
            diagnostic: "admitted cargo executable path must be absolute".to_owned(),
        });
    }
    let canonical = fs::canonicalize(path).map_err(|_| CargoToolFailure {
        kind: CargoToolFailureKind::AdmittedExecutableInvalid,
        diagnostic: "admitted cargo executable path could not be canonicalized to an existing \
            entry"
            .to_owned(),
    })?;
    let metadata = canonical.metadata().map_err(|_| CargoToolFailure {
        kind: CargoToolFailureKind::AdmittedExecutableInvalid,
        diagnostic: "admitted cargo executable path metadata could not be read".to_owned(),
    })?;
    if !metadata.is_file() {
        return Err(CargoToolFailure {
            kind: CargoToolFailureKind::AdmittedExecutableInvalid,
            diagnostic: "admitted cargo executable path is not a regular file".to_owned(),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(CargoToolFailure {
                kind: CargoToolFailureKind::AdmittedExecutableInvalid,
                diagnostic: "admitted cargo executable path is not executable".to_owned(),
            });
        }
    }
    Ok(canonical)
}

/// The real `cargo --version` output for the exact `executable` this run
/// admitted (see `resolve_cargo`), through the same bounded allow-list
/// executor `cargo metadata` already uses. Callers decide how to treat a
/// failure; this function itself never guesses a value.
fn cargo_version(executable: &Path, root: &Path) -> Result<String, IngestError> {
    let mut child = cargo_command(executable, root);
    child.arg("--version");
    command_version("cargo --version", child)
}

/// Classifies a `cargo --version` failure (`error`) into a stable
/// [`CargoToolFailureKind`] plus a redacted [`CargoToolFailure::diagnostic`]:
/// `error`'s own `Display` text with every occurrence of `staged_root` --
/// the private, randomly-named `TempDir` `cargo --version` ran from -- and
/// its canonicalized form (when it differs) replaced by the fixed
/// placeholder `<staged-snapshot>`. A real subprocess failure (for example
/// a `rust-toolchain.toml`-driven toolchain-resolution error) can echo that
/// path verbatim in its stderr; without this, the same input staged into a
/// different `TempDir` on a repeated run would carry a different raw
/// diagnostic and therefore, if that diagnostic ever fed identity, a
/// different fingerprint for no real difference in the underlying failure.
pub(crate) fn cargo_tool_failure(error: &IngestError, staged_root: &Path) -> CargoToolFailure {
    CargoToolFailure {
        kind: cargo_tool_failure_kind(error),
        diagnostic: redact_staged_root(&error.to_string(), staged_root),
    }
}

/// Maps a `cargo --version` failure's `IngestError` shape onto a stable
/// [`CargoToolFailureKind`]. The `AdapterOutput` match relies on
/// `command_version`'s own two fixed message shapes (`` `{command}` output
/// is not UTF-8`` vs `` `{command}` produced no output``), which this crate
/// controls, not on any subprocess-controlled text.
fn cargo_tool_failure_kind(error: &IngestError) -> CargoToolFailureKind {
    match error {
        IngestError::CommandIo { source, .. } if source.kind() == std::io::ErrorKind::TimedOut => {
            CargoToolFailureKind::TimedOut
        }
        IngestError::CommandIo { .. } => CargoToolFailureKind::Unavailable,
        IngestError::OutputLimitExceeded { .. } => CargoToolFailureKind::OutputTooLarge,
        IngestError::CommandFailed { .. } => CargoToolFailureKind::NonSuccessExit,
        IngestError::AdapterOutput(message) if message.contains("is not UTF-8") => {
            CargoToolFailureKind::NotUtf8
        }
        // `command_version`'s only other `AdapterOutput` case is "produced
        // no output".
        IngestError::AdapterOutput(_) => CargoToolFailureKind::EmptyOutput,
        // Every other `IngestError` variant is unreachable from
        // `cargo_version`/`command_version` today; treat it the same as an
        // unlaunchable binary rather than panicking on a future variant.
        _ => CargoToolFailureKind::Unavailable,
    }
}

/// Replaces every occurrence of `root`'s displayed path -- and, when it
/// differs, its canonicalized form -- with the fixed placeholder
/// `<staged-snapshot>`.
fn redact_staged_root(text: &str, root: &Path) -> String {
    let raw = root.display().to_string();
    let mut redacted = text.replace(&raw, "<staged-snapshot>");
    if let Ok(canonical) = root.canonicalize() {
        let canonical = canonical.display().to_string();
        if canonical != raw {
            redacted = redacted.replace(&canonical, "<staged-snapshot>");
        }
    }
    redacted
}

fn command_version(command_name: &'static str, child: Command) -> Result<String, IngestError> {
    let output = checked_output(
        command_name,
        run_with_bounds(
            command_name,
            child,
            SUBPROCESS_TIMEOUT,
            SUBPROCESS_MAX_OUTPUT_BYTES,
        ),
    )?;
    let text = std::str::from_utf8(&output)
        .map_err(|_| IngestError::AdapterOutput(format!("`{command_name}` output is not UTF-8")))?
        .trim();
    if text.is_empty() {
        return Err(IngestError::AdapterOutput(format!(
            "`{command_name}` produced no output"
        )));
    }
    Ok(text.to_owned())
}

fn checked_output(
    command: &'static str,
    output: Result<Output, IngestError>,
) -> Result<Vec<u8>, IngestError> {
    let output = output?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(IngestError::CommandFailed {
            command,
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

/// Runs `command` to completion, bounded by a wall-clock `timeout` and a
/// captured-output cap (`max_output_bytes`, applied independently to stdout
/// and stderr). A process exceeding the timeout is killed and reported as a
/// typed `CommandIo` failure. Reading stdout/stderr happens on dedicated
/// threads so a child that fills its pipe buffer cannot deadlock the
/// polling wait loop; once a stream's captured bytes exceed the cap, that
/// thread stops draining its pipe (a child still writing to a now-full,
/// undrained pipe blocks and is later reaped by the timeout, not held open
/// indefinitely). A stream that actually exceeded the cap fails the whole
/// call with a typed `OutputLimitExceeded` -- it is never returned as a
/// truncated `Ok`, and a reader thread panic is never silently absorbed
/// into an empty result.
fn run_with_bounds(
    command_name: &'static str,
    mut command: Command,
    timeout: Duration,
    max_output_bytes: usize,
) -> Result<Output, IngestError> {
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|source| IngestError::CommandIo {
        command: command_name,
        source,
    })?;
    let mut stdout_pipe = child
        .stdout
        .take()
        .expect("stdout is piped by run_with_bounds");
    let mut stderr_pipe = child
        .stderr
        .take()
        .expect("stderr is piped by run_with_bounds");
    let stdout_reader = thread::spawn(move || read_capped(&mut stdout_pipe, max_output_bytes));
    let stderr_reader = thread::spawn(move || read_capped(&mut stderr_pipe, max_output_bytes));

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(source) => {
                return Err(IngestError::CommandIo {
                    command: command_name,
                    source,
                });
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(IngestError::CommandIo {
                command: command_name,
                source: std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("subprocess exceeded the {timeout:?} bound and was killed"),
                ),
            });
        }
        thread::sleep(Duration::from_millis(20));
    };

    // Join both reader threads unconditionally (the child has already
    // exited or been killed, so neither read can block for long) before
    // deciding on an error, so which stream's outcome is reported never
    // depends on incidental thread-scheduling order.
    let stdout_outcome = stdout_reader.join();
    let stderr_outcome = stderr_reader.join();
    let stdout = capped_read_result(command_name, "stdout", stdout_outcome, max_output_bytes)?;
    let stderr = capped_read_result(command_name, "stderr", stderr_outcome, max_output_bytes)?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Resolves one reader thread's outcome into either its captured bytes or a
/// typed failure: a thread panic becomes a `CommandIo` error (never a
/// silently empty result), an I/O error becomes `CommandIo`, and captured
/// output that actually exceeded `limit` becomes `OutputLimitExceeded` --
/// distinct from a within-bound read, which is trusted as complete.
fn capped_read_result(
    command: &'static str,
    stream: &'static str,
    outcome: std::thread::Result<std::io::Result<CappedRead>>,
    limit: usize,
) -> Result<Vec<u8>, IngestError> {
    let captured = outcome
        .map_err(|_| IngestError::CommandIo {
            command,
            source: std::io::Error::other(format!("{stream} reader thread panicked")),
        })?
        .map_err(|source| IngestError::CommandIo { command, source })?;
    if captured.exceeded {
        return Err(IngestError::OutputLimitExceeded {
            command,
            stream,
            limit,
        });
    }
    Ok(captured.bytes)
}

/// Result of [`read_capped`]: the bytes captured up to (and possibly
/// slightly past) `cap`, and whether the source actually exceeded `cap`.
/// `exceeded` is the load-bearing signal a caller must check before
/// trusting `bytes` -- `bytes` alone cannot distinguish "the stream was
/// exactly this long" from "the stream was longer and got cut off here".
struct CappedRead {
    bytes: Vec<u8>,
    exceeded: bool,
}

/// Reads `reader` to EOF, or until more than `cap` bytes have been
/// captured, whichever comes first. Deliberately does not keep draining
/// past the cap: the buffer stays bounded near `cap` instead of growing to
/// match however much output the child actually produces. Exceeding the
/// cap is reported via `CappedRead::exceeded`, never silently absorbed
/// into a truncated `Ok` -- a caller that ignored it could parse a
/// truncated `ls-tree`/`diff` as if it were complete.
fn read_capped(reader: &mut impl Read, cap: usize) -> std::io::Result<CappedRead> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(CappedRead {
                bytes: buffer,
                exceeded: false,
            });
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > cap {
            return Ok(CappedRead {
                bytes: buffer,
                exceeded: true,
            });
        }
    }
}

fn stage_files(files: &[SnapshotFile]) -> Result<TempDir, IngestError> {
    let staged = tempfile::Builder::new()
        .prefix("reviewgraphen-ingest-")
        .tempdir()
        .map_err(|source| IngestError::CommandIo {
            command: "create private cargo metadata staging directory",
            source,
        })?;
    for file in files {
        let relative = Path::new(&file.path);
        validate_tree_path(&file.path)?;
        let destination = staged.path().join(relative);
        let parent = destination.parent().ok_or_else(|| {
            IngestError::AdapterOutput("staged Git file has no parent directory".to_owned())
        })?;
        fs::create_dir_all(parent).map_err(|source| IngestError::CommandIo {
            command: "stage Git snapshot directory",
            source,
        })?;
        fs::write(&destination, &file.content).map_err(|source| IngestError::CommandIo {
            command: "stage Git snapshot file",
            source,
        })?;
    }
    Ok(staged)
}

/// The snapshot-relative directory a Cargo manifest lives in (`""` for a
/// root `Cargo.toml`, `"crates/foo"` for `crates/foo/Cargo.toml`).
fn manifest_directory(manifest_path: &str) -> String {
    match manifest_path.rsplit_once('/') {
        Some((dir, _)) => dir.to_owned(),
        None => String::new(),
    }
}

/// Statically checks one Cargo manifest for any reference that would expand
/// Cargo's read scope beyond the bounded snapshot, without invoking Cargo:
/// a `path` dependency (in `[dependencies]`, `[dev-dependencies]`,
/// `[build-dependencies]`, any target-specific dependency table, or a
/// `[patch]`/`[replace]` override -- found by walking every table for a
/// `path` key, rather than hand-enumerating each dependency table Cargo
/// supports), `package.workspace`, or a `[workspace] members` entry. Each
/// candidate path is resolved relative to the manifest's own snapshot
/// directory and only rejected if it would actually cross the snapshot
/// root; a sibling-crate reference that stays within the snapshot (the
/// normal shape of a Cargo workspace) is accepted. Returns `Some(reason)`
/// for a real violation *or* an unparseable manifest -- an unparseable
/// manifest cannot be proven safe, so it is treated the same as a proven
/// violation, never as an implicit pass.
fn cargo_manifest_containment_violation(content: &[u8], manifest_dir: &str) -> Option<String> {
    let text = match std::str::from_utf8(content) {
        Ok(text) => text,
        Err(_) => return Some("is not valid UTF-8".to_owned()),
    };
    let table: toml::Table = match text.parse() {
        Ok(table) => table,
        Err(error) => return Some(format!("could not be parsed as TOML: {error}")),
    };
    if let Some(violation) = find_escaping_path(&toml::Value::Table(table.clone()), manifest_dir) {
        return Some(violation);
    }
    if let Some(workspace_root) = table
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("workspace"))
        .and_then(toml::Value::as_str)
        && path_escapes_snapshot(manifest_dir, workspace_root)
    {
        return Some(format!(
            "declares package.workspace = \"{workspace_root}\", which escapes the bounded snapshot"
        ));
    }
    if let Some(members) = table
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
    {
        for member in members {
            if let Some(member) = member.as_str()
                && member_reference_escapes_snapshot(member)
            {
                return Some(format!(
                    "declares workspace.members \"{member}\", which escapes the bounded snapshot"
                ));
            }
        }
    }
    None
}

/// Recursively walks a parsed manifest for any table carrying a string
/// `path` key (the shape Cargo uses for a path dependency, a `[patch]`
/// source override, and a `[replace]` override alike) and validates it.
fn find_escaping_path(value: &toml::Value, manifest_dir: &str) -> Option<String> {
    match value {
        toml::Value::Table(table) => {
            if let Some(path) = table.get("path").and_then(toml::Value::as_str)
                && path_escapes_snapshot(manifest_dir, path)
            {
                return Some(format!(
                    "declares a path reference \"{path}\", which escapes the bounded snapshot"
                ));
            }
            table
                .values()
                .find_map(|item| find_escaping_path(item, manifest_dir))
        }
        toml::Value::Array(items) => items
            .iter()
            .find_map(|item| find_escaping_path(item, manifest_dir)),
        _ => None,
    }
}

/// Resolves `relative` against `manifest_dir` (both `/`-separated,
/// snapshot-relative) and reports whether the result would cross above the
/// snapshot root, or `relative` is itself absolute (POSIX or Windows).
fn path_escapes_snapshot(manifest_dir: &str, relative: &str) -> bool {
    if relative.is_empty() {
        return false;
    }
    if relative.starts_with('/') || relative.starts_with('\\') {
        return true;
    }
    let bytes = relative.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return true;
    }
    let mut stack = manifest_dir
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    for segment in relative.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                if stack.pop().is_none() {
                    return true;
                }
            }
            other => stack.push(other),
        }
    }
    false
}

/// A conservative, glob-agnostic check for a `[workspace] members` entry: a
/// `..` component anywhere (glob or not) is treated as a potential escape,
/// since M2 does not expand glob members to resolve their real target.
fn member_reference_escapes_snapshot(member: &str) -> bool {
    if member.starts_with('/') || member.starts_with('\\') {
        return true;
    }
    let bytes = member.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return true;
    }
    member.split(['/', '\\']).any(|segment| segment == "..")
}

fn relative_staged_path(staging_root: &Path, path: &str) -> Option<String> {
    let canonical_root = fs::canonicalize(staging_root).ok()?;
    let canonical_path = fs::canonicalize(Path::new(path)).ok()?;
    canonical_path
        .strip_prefix(canonical_root)
        .ok()
        .and_then(|value| value.to_str())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod bounded_exec_tests {
    use super::{
        CappedRead, Command, Duration, Instant, capped_read_result, run_with_bounds, thread,
    };
    use crate::IngestError;

    #[test]
    fn a_well_behaved_process_completes_normally_through_the_bounded_wrapper() {
        let mut command = Command::new("printf");
        command.arg("hello");
        let output = run_with_bounds("printf", command, Duration::from_secs(5), 1024)
            .expect("a fast, well-behaved process must complete normally");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"hello");
    }

    #[test]
    fn a_slow_process_is_killed_at_the_timeout_bound_instead_of_awaited_to_completion() {
        let mut command = Command::new("sleep");
        command.arg("5");
        let started = Instant::now();
        let result = run_with_bounds("sleep", command, Duration::from_millis(200), 1024 * 1024);
        assert!(
            result.is_err(),
            "a process exceeding the timeout must be reported as failed, not silently awaited"
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the process must be killed near the timeout bound, not left to run 5s to completion"
        );
    }

    #[test]
    fn unbounded_output_does_not_leave_the_process_running_and_never_returns_a_truncated_ok() {
        // `yes` never exits on its own. Once our reader thread stops
        // draining its pipe past the cap, `yes` either receives SIGPIPE
        // from the now-closed read end (most platforms) or blocks on a
        // full pipe until the outer timeout kills it -- either way it must
        // not be left running, and it must never come back as a truncated
        // `Ok`: the cap was genuinely exceeded, so this must fail closed.
        let command = Command::new("yes");
        let started = Instant::now();
        let result = run_with_bounds("yes", command, Duration::from_millis(300), 4096);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the process must not be left running indefinitely"
        );
        assert!(
            result.is_err(),
            "output that exceeded the cap must never be returned as a truncated Ok: {result:?}"
        );
    }

    #[test]
    fn output_exceeding_the_cap_is_a_typed_error_even_when_the_child_exits_successfully() {
        // A deliberately small cap with a child that writes more than the
        // cap and then exits with status 0 -- the historical bug returned
        // Ok(truncated_bytes) here because the process itself "succeeded".
        let mut command = Command::new("printf");
        command.arg("0123456789abcdef0123456789abcdef");
        let result = run_with_bounds("printf", command, Duration::from_secs(5), 8);
        match result {
            Err(IngestError::OutputLimitExceeded {
                command,
                stream,
                limit,
            }) => {
                assert_eq!(command, "printf");
                assert_eq!(stream, "stdout");
                assert_eq!(limit, 8);
            }
            other => panic!(
                "expected a typed OutputLimitExceeded for a successfully-exiting but \
                 over-cap child, got: {other:?}"
            ),
        }
    }

    #[test]
    fn stderr_exceeding_the_cap_is_also_a_typed_error() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf '0123456789abcdef0123456789abcdef' 1>&2"]);
        let result = run_with_bounds("sh", command, Duration::from_secs(5), 8);
        match result {
            Err(IngestError::OutputLimitExceeded {
                command,
                stream,
                limit,
            }) => {
                assert_eq!(command, "sh");
                assert_eq!(stream, "stderr");
                assert_eq!(limit, 8);
            }
            other => panic!("expected a typed OutputLimitExceeded for stderr, got: {other:?}"),
        }
    }

    #[test]
    fn output_exactly_at_the_cap_boundary_is_accepted() {
        let mut command = Command::new("printf");
        command.arg("01234567");
        let output = run_with_bounds("printf", command, Duration::from_secs(5), 8)
            .expect("output exactly at the cap must be accepted, not treated as exceeded");
        assert_eq!(output.stdout, b"01234567");
    }

    #[test]
    fn a_reader_thread_panic_becomes_a_typed_error_not_an_empty_result() {
        let handle = thread::spawn(|| -> std::io::Result<CappedRead> {
            panic!("simulated reader thread panic");
        });
        let outcome = handle.join();
        match capped_read_result("probe", "stdout", outcome, 1024) {
            Err(IngestError::CommandIo { command, source }) => {
                assert_eq!(command, "probe");
                assert_eq!(source.kind(), std::io::ErrorKind::Other);
            }
            other => panic!(
                "a panicked reader thread must become a typed CommandIo error, never an \
                 empty result: {other:?}"
            ),
        }
    }
}

#[cfg(test)]
mod tool_version_tests {
    use super::{Command, command_version};
    use crate::IngestError;

    #[test]
    fn a_successful_command_returns_its_trimmed_stdout() {
        let mut command = Command::new("printf");
        command.arg("mock-tool version 9.9.9\n");
        let version = command_version("mock-tool --version", command)
            .expect("a well-behaved version command must succeed");
        assert_eq!(version, "mock-tool version 9.9.9");
    }

    #[test]
    fn a_nonzero_exit_is_a_typed_command_failed_error() {
        let command = Command::new("false");
        let error = command_version("mock-tool --version", command)
            .expect_err("a failing version command must fail closed, not return a guessed value");
        assert!(
            matches!(error, IngestError::CommandFailed { .. }),
            "expected CommandFailed, got {error:?}"
        );
    }

    #[test]
    fn empty_stdout_on_success_is_a_typed_adapter_output_error() {
        let command = Command::new("true");
        let error = command_version("mock-tool --version", command)
            .expect_err("empty output must never be treated as a real version string");
        assert!(
            matches!(error, IngestError::AdapterOutput(_)),
            "expected AdapterOutput, got {error:?}"
        );
    }

    #[test]
    fn a_missing_binary_is_a_typed_command_io_error() {
        let command = Command::new("reviewgraphen-test-tool-that-does-not-exist");
        let error = command_version("mock-tool --version", command)
            .expect_err("a missing binary must fail closed, never panic or default silently");
        assert!(
            matches!(error, IngestError::CommandIo { .. }),
            "expected CommandIo, got {error:?}"
        );
    }
}

#[cfg(test)]
mod git_command_policy_tests {
    use super::{GitCommand, Path, build_git_command, git_command};

    fn args_of(command: GitCommand<'_>) -> Vec<String> {
        build_git_command(Path::new("/tmp"), command)
            .get_args()
            .map(|arg| arg.to_str().expect("policy args are UTF-8").to_owned())
            .collect()
    }

    /// Proves the policy at the `Command`-construction level, without
    /// needing a real `git` binary: every environment variable the child
    /// sees is either the allowlisted `PATH` or one of this builder's fixed
    /// overrides -- nothing from the ambient test-runner environment (an
    /// unrelated `GIT_CONFIG_GLOBAL`, locale, credential helper, ...) can
    /// leak through, because `env_clear()` strips it before anything else
    /// is set.
    #[test]
    fn the_built_command_strips_ambient_environment_to_the_allowlist_and_disables_config() {
        let command = git_command(Path::new("/tmp"));
        let mut system_config = None;
        let mut global_config = None;
        let mut locale = None;
        let mut unexpected = Vec::new();
        for (name, value) in command.get_envs() {
            match name.to_str().unwrap_or_default() {
                "PATH" => {}
                "GIT_CONFIG_SYSTEM" => system_config = value,
                "GIT_CONFIG_GLOBAL" => global_config = value,
                "LC_ALL" => locale = value,
                other => unexpected.push(other.to_owned()),
            }
        }
        assert_eq!(
            system_config,
            Some(std::ffi::OsStr::new("/dev/null")),
            "system Git config must be unconditionally disabled"
        );
        assert_eq!(
            global_config,
            Some(std::ffi::OsStr::new("/dev/null")),
            "global Git config must be unconditionally disabled"
        );
        assert_eq!(
            locale,
            Some(std::ffi::OsStr::new("C")),
            "locale must be pinned for stable, non-ambient diagnostic output"
        );
        assert!(
            unexpected.is_empty(),
            "no environment variable beyond the fixed allowlist may reach the child: {unexpected:?}"
        );
    }

    #[test]
    fn the_built_command_disables_pager_optional_locks_and_the_default_attributes_file() {
        let command = git_command(Path::new("/tmp"));
        assert_eq!(command.get_program().to_str(), Some("git"));
        let args = command
            .get_args()
            .map(|arg| arg.to_str().expect("policy args are UTF-8"))
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                "--no-pager",
                "--no-optional-locks",
                "--no-replace-objects",
                "-c",
                "core.attributesFile=/dev/null",
            ]
        );
    }

    #[test]
    fn the_changes_command_disables_ext_diff_and_textconv_and_fixes_rename_detection() {
        let args = args_of(GitCommand::Changes {
            base: "base-rev",
            target: "target-rev",
        });
        assert_eq!(
            args,
            [
                "--no-pager",
                "--no-optional-locks",
                "--no-replace-objects",
                "-c",
                "core.attributesFile=/dev/null",
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--text",
                "--name-status",
                "-z",
                "--find-renames=50%",
                "--find-copies=50%",
                "-l20000",
                "base-rev",
                "target-rev",
                "--",
            ]
        );
    }

    #[test]
    fn the_changed_lines_command_disables_ext_diff_and_textconv_and_fixes_the_diff_algorithm() {
        let args = args_of(GitCommand::ChangedLines {
            base: "base-rev",
            target: "target-rev",
            path: "src/lib.rs",
        });
        assert_eq!(
            args,
            [
                "--no-pager",
                "--no-optional-locks",
                "--no-replace-objects",
                "-c",
                "core.attributesFile=/dev/null",
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--text",
                "--diff-algorithm=myers",
                "--unified=0",
                "base-rev",
                "target-rev",
                "--",
                "src/lib.rs",
            ]
        );
    }

    #[test]
    fn the_show_file_command_disables_ext_diff_and_textconv() {
        let args = args_of(GitCommand::ShowFile {
            revision: "target-rev",
            path: "src/lib.rs",
        });
        assert_eq!(
            args,
            [
                "--no-pager",
                "--no-optional-locks",
                "--no-replace-objects",
                "-c",
                "core.attributesFile=/dev/null",
                "show",
                "--no-ext-diff",
                "--no-textconv",
                "--format=",
                "target-rev:src/lib.rs",
            ]
        );
    }
}

#[cfg(test)]
mod cargo_dependency_resolution_tests {
    use super::{DependencyResolution, LocalPackage, Path, resolve_dependency};
    use serde_json::json;

    fn local(key: &str, name: &str, manifest_path: &str) -> LocalPackage {
        LocalPackage {
            key: key.to_owned(),
            name: name.to_owned(),
            manifest_path: manifest_path.to_owned(),
        }
    }

    #[test]
    fn registry_dependency_is_always_external_even_if_a_same_named_local_package_exists() {
        let packages = [local(
            "package:member-a/Cargo.toml:member-a@0.1.0",
            "member-a",
            "member-a/Cargo.toml",
        )];
        let dependency = json!({
            "name": "member-a",
            "source": "registry+https://github.com/rust-lang/crates.io-index",
            "req": "^0.1",
        });
        assert!(
            matches!(
                resolve_dependency(&dependency, Path::new("/staged"), &packages),
                DependencyResolution::External
            ),
            "cargo's own `source` field is authoritative: a non-null source is always \
             external, regardless of a coincidentally same-named local package"
        );
    }

    #[test]
    fn unambiguous_local_dependency_resolves_to_its_accepted_package() {
        let packages = [local(
            "package:member-a/Cargo.toml:member-a@0.1.0",
            "member-a",
            "member-a/Cargo.toml",
        )];
        let dependency = json!({ "name": "member-a", "source": null, "req": "*" });
        match resolve_dependency(&dependency, Path::new("/staged"), &packages) {
            DependencyResolution::Internal(key) => {
                assert_eq!(key, "package:member-a/Cargo.toml:member-a@0.1.0");
            }
            _ => panic!("expected an internal resolution, got a different outcome"),
        }
    }

    #[test]
    fn unmatched_local_dependency_is_unresolved_not_guessed() {
        let packages: [LocalPackage; 0] = [];
        let dependency = json!({ "name": "not-a-workspace-member", "source": null, "req": "*" });
        assert!(
            matches!(
                resolve_dependency(&dependency, Path::new("/staged"), &packages),
                DependencyResolution::Unresolved
            ),
            "a local dependency naming no accepted package (for example a path dependency \
             to a non-workspace-member crate `--no-deps` never resolved) must never be \
             guessed as external or bound to an unrelated package"
        );
    }

    #[test]
    fn same_named_local_packages_are_not_mis_bound_without_a_disambiguating_path() {
        // Real single-workspace `cargo metadata` cannot itself produce two
        // packages sharing a name; this is a defensive counterexample for
        // `resolve_dependency`'s own matching logic, which must not guess
        // between colliding candidates just because a name matched.
        let packages = [
            local("package:a/Cargo.toml:dup@0.1.0", "dup", "a/Cargo.toml"),
            local("package:b/Cargo.toml:dup@0.2.0", "dup", "b/Cargo.toml"),
        ];
        let dependency = json!({ "name": "dup", "source": null, "req": "*" });
        assert!(
            matches!(
                resolve_dependency(&dependency, Path::new("/staged"), &packages),
                DependencyResolution::Unresolved
            ),
            "an ambiguous same-named local match must never be bound to either candidate"
        );
    }

    #[test]
    fn same_named_local_packages_are_disambiguated_by_an_exact_path_match() {
        let staged = tempfile::tempdir().expect("temporary staged root");
        std::fs::create_dir_all(staged.path().join("a")).expect("staged dir a");
        std::fs::create_dir_all(staged.path().join("b")).expect("staged dir b");
        let packages = [
            local("package:a/Cargo.toml:dup@0.1.0", "dup", "a/Cargo.toml"),
            local("package:b/Cargo.toml:dup@0.2.0", "dup", "b/Cargo.toml"),
        ];
        let dependency_path = staged
            .path()
            .join("b")
            .to_str()
            .expect("staged path is UTF-8")
            .to_owned();
        let dependency =
            json!({ "name": "dup", "source": null, "req": "*", "path": dependency_path });
        match resolve_dependency(&dependency, staged.path(), &packages) {
            DependencyResolution::Internal(key) => {
                assert_eq!(key, "package:b/Cargo.toml:dup@0.2.0");
            }
            _ => panic!("expected the path field to disambiguate to package b"),
        }
    }
}

/// Writes `contents` to `path` and marks it executable (`0o755`). Shared by
/// every test module below that stages a fake `cargo` executable on disk
/// (Unix-only: shell shebangs and permission bits are POSIX-specific).
#[cfg(all(test, unix))]
fn write_test_executable(path: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, contents).expect("write fake executable script");
    let mut permissions = fs::metadata(path)
        .expect("fake executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("set fake executable permissions");
}

/// A fake `cargo` that only understands `metadata`: it reports one local
/// package (`precondition-fixture`, matching `snapshot_with_cargo_version`'s
/// fixture manifest below) whose `manifest_path` is resolved from its own
/// cwd, and fails closed -- like a real `cargo metadata` would -- when the
/// requested `--manifest-path` does not exist under that cwd.
#[cfg(all(test, unix))]
const FAKE_CARGO_METADATA_SCRIPT: &str = r#"#!/bin/sh
manifest="Cargo.toml"
prev=""
for arg in "$@"; do
  if [ "$prev" = "--manifest-path" ]; then
    manifest="$arg"
  fi
  prev="$arg"
done
if [ ! -f "$manifest" ]; then
  echo "error: manifest path \`$manifest\` does not exist" 1>&2
  exit 101
fi
manifest_abs="$(cd "$(dirname "$manifest")" && pwd -P)/$(basename "$manifest")"
cat <<JSON
{"packages":[{"name":"precondition-fixture","version":"0.1.0","manifest_path":"$manifest_abs","dependencies":[]}]}
JSON
"#;

#[cfg(all(test, unix))]
mod cargo_version_precondition_tests {
    use super::{
        AdapterStatus, CapabilityState, CargoToolFailure, CargoToolFailureKind, ContentHash,
        FAKE_CARGO_METADATA_SCRIPT, GitSnapshot, IngestionObstructionKind, SnapshotFile, StableId,
        extract_cargo_metadata, stage_files, write_test_executable,
    };
    use std::collections::{BTreeMap, BTreeSet};

    /// A snapshot whose `Cargo.toml`/`src/lib.rs` are, on their own, a
    /// perfectly ordinary crate `cargo metadata` would happily describe --
    /// isolating `cargo_version` as the only variable under test. `caller`
    /// picks whether `cargo --version` is reported as having succeeded.
    fn snapshot_with_cargo_version(cargo_version: Result<String, CargoToolFailure>) -> GitSnapshot {
        let manifest = b"[package]\nname = \"precondition-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n".to_vec();
        let lib = b"pub fn entry() {}\n".to_vec();
        let files = vec![
            SnapshotFile {
                path: "Cargo.toml".to_owned(),
                content_hash: ContentHash::sha256(&manifest),
                content: manifest,
                changed_lines: BTreeSet::new(),
            },
            SnapshotFile {
                path: "src/lib.rs".to_owned(),
                content_hash: ContentHash::sha256(&lib),
                content: lib,
                changed_lines: BTreeSet::new(),
            },
        ];
        // Mirrors the real `cargo_version: Ok(_) => staged_snapshot: Some(_)`
        // invariant `load_snapshot` upholds (see `extract_cargo_metadata`'s
        // `.expect`): only a real staged copy lets the `Ok` case below
        // actually exercise `cargo metadata` end to end.
        let staged_snapshot = cargo_version
            .is_ok()
            .then(|| stage_files(&files).expect("test staging succeeds"));
        // Mirrors the real `cargo_version: Ok(_) => cargo_executable: Some(_)`
        // invariant too -- and, like the real admitted executable
        // `admit_cargo_executable` hands back, an absolute, canonicalized
        // path to a real, executable regular file, not a bare relative
        // name `Command` would otherwise resolve via a `PATH` lookup this
        // module's production code never performs. Staged inside the same
        // `staged_snapshot` `TempDir` (never a snapshot file itself, so it
        // cannot collide with `snapshot.files`) so its lifetime matches the
        // returned `GitSnapshot`'s.
        let cargo_executable = staged_snapshot.as_ref().map(|staged| {
            let path = staged.path().join("fake-cargo.sh");
            write_test_executable(&path, FAKE_CARGO_METADATA_SCRIPT);
            std::fs::canonicalize(&path).expect("canonicalize fake cargo executable")
        });
        GitSnapshot {
            repository_root: "/repo".to_owned(),
            repository_identity: "reviewgraphen.test/cargo-version-precondition".to_owned(),
            repository_name: "repo".to_owned(),
            base_revision: "0".repeat(40),
            target_revision: "1".repeat(40),
            tree_hash: ContentHash::parse(format!("git:{}", "2".repeat(40)))
                .expect("valid test tree hash"),
            config: crate::IngestConfig::default(),
            files,
            changes: Vec::new(),
            issues: Vec::new(),
            adapter_reports: Vec::new(),
            capabilities: BTreeMap::new(),
            capability_sources: BTreeMap::new(),
            git_version: "git version 2.99.0".to_owned(),
            cargo_version,
            cargo_executable,
            staged_snapshot,
        }
    }

    fn dummy_snapshot_id() -> StableId {
        StableId::parse("snapshot:test").expect("valid test snapshot id")
    }

    #[test]
    fn an_unavailable_cargo_version_blocks_cargo_metadata_even_though_the_manifest_would_otherwise_succeed()
     {
        let snapshot = snapshot_with_cargo_version(Err(CargoToolFailure {
            kind: CargoToolFailureKind::NonSuccessExit,
            diagnostic: "simulated: cargo --version failed even though a `cargo metadata` \
                 wrapper might still succeed"
                .to_owned(),
        }));
        let extraction = extract_cargo_metadata(&snapshot, &dummy_snapshot_id());

        assert_eq!(
            extraction.capabilities.get("cargo_metadata"),
            Some(&CapabilityState::Missing),
            "cargo metadata must never be accepted while cargo's own tool identity is unverified"
        );
        assert_eq!(extraction.adapter_report.status, AdapterStatus::NotRun);
        assert!(
            extraction.artifacts.is_empty(),
            "no package/dependency facts may be accepted without a verified cargo identity"
        );
        assert!(
            extraction.issues.iter().any(|issue| {
                issue.kind == IngestionObstructionKind::CargoMetadataUnavailable
                    && issue.description.contains("cargo --version")
            }),
            "the precondition failure must be retained as a typed obstruction naming the cause"
        );
    }

    #[test]
    fn a_verified_cargo_version_lets_the_same_manifest_produce_accepted_package_facts() {
        // Control proving the fixture above is a genuine counterexample,
        // not a manifest that would have failed anyway: with the identical
        // `Cargo.toml`/`src/lib.rs`, only `cargo_version` differs.
        let snapshot = snapshot_with_cargo_version(Ok("cargo 1.99.0 (test)".to_owned()));
        let extraction = extract_cargo_metadata(&snapshot, &dummy_snapshot_id());

        assert_ne!(
            extraction.capabilities.get("cargo_metadata"),
            Some(&CapabilityState::Missing),
            "the same manifest must not be blocked once cargo's tool identity is verified"
        );
        assert!(
            extraction
                .artifacts
                .iter()
                .any(|artifact| artifact.id_kind == "package"),
            "a verified cargo run over a valid manifest must accept at least one package fact"
        );
    }

    #[test]
    fn cargo_metadata_reads_the_exact_staged_directory_cargo_version_was_determined_from() {
        // `load_snapshot` stages the snapshot exactly once, and both
        // `cargo_version` and (later) `cargo metadata` itself run from
        // that one directory, against the same admitted `cargo`
        // executable -- never two independently-staged copies of the same
        // content, which could disagree if any current or future
        // cwd-sensitive `cargo` behavior attributed version and metadata
        // to different toolchains. Proven here by tampering with
        // the *exact* directory `cargo_version` was already determined
        // from, bypassing `snapshot.files` entirely: if
        // `extract_cargo_metadata` truly reuses that directory rather than
        // re-staging a fresh copy from `snapshot.files` internally (which
        // would silently restore this file and hide the regression), the
        // manifest is really gone and Cargo metadata must fail to find it.
        let snapshot = snapshot_with_cargo_version(Ok("cargo 1.99.0 (test)".to_owned()));
        let staged_path = snapshot
            .staged_snapshot
            .as_ref()
            .expect("Ok cargo_version implies a staged snapshot")
            .path()
            .to_path_buf();
        std::fs::remove_file(staged_path.join("Cargo.toml"))
            .expect("remove Cargo.toml from the already-staged directory");

        let extraction = extract_cargo_metadata(&snapshot, &dummy_snapshot_id());

        assert_eq!(
            extraction.capabilities.get("cargo_metadata"),
            Some(&CapabilityState::Missing),
            "cargo metadata must fail once its manifest is removed from the exact staged \
             directory cargo_version was determined from -- a pass here would mean \
             extract_cargo_metadata re-staged a fresh copy instead of reusing it"
        );
    }

    #[test]
    #[should_panic(expected = "staged_snapshot is always Some once cargo_version is Ok")]
    fn cargo_metadata_extraction_refuses_to_proceed_if_cargo_version_is_ok_without_a_staged_snapshot()
     {
        // A snapshot in this shape can never arise from `load_snapshot`
        // itself (staging always happens immediately before, and only on
        // success does `cargo_version` get determined from it -- see the
        // doc comment on `GitSnapshot::staged_snapshot`). This proves the
        // invariant is actively enforced, not merely assumed: if a future
        // change ever reintroduced the original bug class (determining
        // `cargo_version` from a *different* directory than the one `cargo
        // metadata` itself later stages and runs from), the mismatch would
        // fail loudly here instead of silently attributing a `cargo
        // metadata` result to an unverified working directory.
        let mut snapshot = snapshot_with_cargo_version(Ok("cargo 1.99.0 (test)".to_owned()));
        snapshot.staged_snapshot = None;
        let _ = extract_cargo_metadata(&snapshot, &dummy_snapshot_id());
    }
}

/// Exercises `resolve_cargo`/`admit_cargo_executable` against real
/// `TempDir`-staged fake `cargo` executables (Unix-only: shell shebangs and
/// permission bits are POSIX-specific), independent of whatever real
/// `cargo` the host actually has installed. No test here ever places a
/// `rustup`/`mise`/`asdf` shim on disk or touches `PATH`: strict host
/// admission never consults either, so there is nothing for such a shim to
/// intercept.
#[cfg(all(test, unix))]
mod cargo_admission_tests {
    use super::{
        CargoToolFailureKind, Path, PathBuf, cargo_command, cargo_version, resolve_cargo,
        run_cargo_metadata, write_test_executable,
    };
    use crate::CargoToolAdmission;
    use std::env;
    use std::fs;

    /// A fake `cargo` that only understands `--version`.
    fn fake_cargo_version_script(version_line: &str) -> String {
        r#"#!/bin/sh
if [ "${1:-}" = "--version" ]; then
  echo "__VERSION__"
  exit 0
fi
echo "unsupported fake cargo invocation: $*" 1>&2
exit 2
"#
        .replace("__VERSION__", version_line)
    }

    fn fake_logging_cargo_script(log_path: &Path) -> String {
        r#"#!/bin/sh
{
  echo "ARGV0:$0"
  echo "PWD:$(pwd -P)"
  echo "CARGO_HOME:${CARGO_HOME:-<unset>}"
  echo "CARGO_NET_OFFLINE:${CARGO_NET_OFFLINE:-<unset>}"
  echo "CARGO_TERM_COLOR:${CARGO_TERM_COLOR:-<unset>}"
  echo "LC_ALL:${LC_ALL:-<unset>}"
  echo "PATH:${PATH:-<unset>}"
  echo "RUSTUP_TOOLCHAIN:${RUSTUP_TOOLCHAIN:-<unset>}"
  echo "MISE_TRUSTED_CONFIG_PATHS:${MISE_TRUSTED_CONFIG_PATHS:-<unset>}"
  echo "RUSTC:${RUSTC:-<unset>}"
  echo "RUSTFLAGS:${RUSTFLAGS:-<unset>}"
  echo "CARGO_TARGET_DIR:${CARGO_TARGET_DIR:-<unset>}"
  echo "ARGS:$*"
  echo "---"
} >> "__LOG_PATH__"
if [ "${1:-}" = "--version" ]; then
  echo "cargo 1.99.0-test"
  exit 0
fi
echo "{}"
exit 0
"#
        .replace("__LOG_PATH__", &log_path.display().to_string())
    }

    /// Structural counterpart to
    /// `git_command_policy_tests::the_built_command_strips_ambient_environment_to_the_allowlist_and_disables_config`,
    /// for `cargo_command`: inspects the `Command` builder's own declared
    /// environment map (`Command::get_envs()`) rather than spawning a real
    /// subprocess, so it needs no environment mutation (this workspace
    /// forbids `unsafe_code`, and `std::env::set_var`/`remove_var` require
    /// `unsafe`). This proves no variable beyond the fixed allowlist --
    /// including `PATH`, `RUSTUP_TOOLCHAIN`, any `MISE_*` variable, `RUSTC`,
    /// `RUSTFLAGS`, or `CARGO_TARGET_DIR` -- can ever be declared on the
    /// child, categorically, not only for the specific names this test
    /// happens to enumerate.
    #[test]
    fn cargo_command_declares_only_the_fixed_env_allowlist() {
        let staged_root = Path::new("/tmp/reviewgraphen-cargo-command-test");
        let command = cargo_command(Path::new("/tmp/fake-cargo"), staged_root);
        assert_eq!(command.get_program().to_str(), Some("/tmp/fake-cargo"));
        assert_eq!(command.get_current_dir(), Some(staged_root));

        let mut cargo_home = None;
        let mut net_offline = None;
        let mut term_color = None;
        let mut locale = None;
        let mut unexpected = Vec::new();
        for (name, value) in command.get_envs() {
            match name.to_str().unwrap_or_default() {
                "CARGO_HOME" => cargo_home = value,
                "CARGO_NET_OFFLINE" => net_offline = value,
                "CARGO_TERM_COLOR" => term_color = value,
                "LC_ALL" => locale = value,
                other => unexpected.push(other.to_owned()),
            }
        }
        assert_eq!(
            cargo_home,
            Some(std::ffi::OsStr::new(
                "/tmp/reviewgraphen-cargo-command-test/.reviewgraphen-cargo-home"
            ))
        );
        assert_eq!(net_offline, Some(std::ffi::OsStr::new("true")));
        assert_eq!(term_color, Some(std::ffi::OsStr::new("never")));
        assert_eq!(locale, Some(std::ffi::OsStr::new("C")));
        assert!(
            unexpected.is_empty(),
            "no environment variable beyond the fixed allowlist may reach the child, and PATH \
             is deliberately excluded -- the admitted executable always runs by its already- \
             canonicalized absolute path: {unexpected:?}"
        );
    }

    #[test]
    fn cargo_subprocesses_never_inherit_the_real_ambient_path() {
        let staged_root = tempfile::tempdir().expect("staged root");
        let log_path = staged_root.path().join("invocations.log");
        let cargo_path = staged_root.path().join("fake-cargo.sh");
        write_test_executable(&cargo_path, &fake_logging_cargo_script(&log_path));

        // `PATH` is guaranteed to be ambiently set in any process this test
        // suite runs in; if `cargo_command`'s `env_clear()` regressed and
        // started inheriting the calling process's environment, the fake
        // script below would observe this exact ambient value verbatim.
        // Compared against that real value below, rather than asserting
        // `PATH:<unset>`: `cargo_command` clears `PATH` from the child's
        // declared environment (see
        // `cargo_command_declares_only_the_fixed_env_allowlist`), but a
        // POSIX `/bin/sh` (the fake script's shebang interpreter) is free
        // to synthesize its own implementation-defined default `PATH` once
        // it observes none was inherited, so the fake script's own `$PATH`
        // need not actually read as unset -- only as different from the
        // real ambient sentinel this test process is running under.
        let ambient_path =
            env::var_os("PATH").expect("PATH must be ambiently set in the test process");

        let admission = CargoToolAdmission::TrustedExecutable(cargo_path.clone());
        let (executable, version) = resolve_cargo(&admission, staged_root.path())
            .expect("fake cargo --version succeeds through the real resolve_cargo entry point");
        assert_eq!(version, "cargo 1.99.0-test");
        let _ = run_cargo_metadata(&executable, staged_root.path(), "Cargo.toml")
            .expect("fake cargo metadata succeeds");

        let log = fs::read_to_string(&log_path).expect("read invocation log");
        let invocations = log
            .split("---\n")
            .filter(|entry| !entry.trim().is_empty())
            .collect::<Vec<_>>();
        assert_eq!(
            invocations.len(),
            2,
            "both cargo --version and cargo metadata must have logged: {log}"
        );
        let canonical_cargo_path = fs::canonicalize(&cargo_path).expect("canonicalize fake cargo");
        let canonical_root =
            fs::canonicalize(staged_root.path()).expect("canonicalize staged root");
        assert_eq!(
            executable, canonical_cargo_path,
            "cargo_version and cargo metadata must both run the exact executable resolve_cargo \
             returned"
        );
        for invocation in &invocations {
            assert!(
                invocation.contains(&format!("ARGV0:{}", canonical_cargo_path.display())),
                "both invocations must run the resolved executable: {invocation}"
            );
            assert!(
                invocation.contains(&format!("PWD:{}", canonical_root.display())),
                "both invocations must run from the same staged cwd: {invocation}"
            );
            for (var, expected) in [
                ("CARGO_NET_OFFLINE", "true"),
                ("CARGO_TERM_COLOR", "never"),
                ("LC_ALL", "C"),
            ] {
                assert!(
                    invocation.contains(&format!("{var}:{expected}")),
                    "fixed env var {var} must be set to {expected}: {invocation}"
                );
            }
            assert!(
                invocation.contains(&format!(
                    "CARGO_HOME:{}",
                    staged_root
                        .path()
                        .join(".reviewgraphen-cargo-home")
                        .display()
                )),
                "CARGO_HOME must stay inside the staged snapshot: {invocation}"
            );
            for var in [
                "RUSTUP_TOOLCHAIN",
                "MISE_TRUSTED_CONFIG_PATHS",
                "RUSTC",
                "RUSTFLAGS",
                "CARGO_TARGET_DIR",
            ] {
                assert!(
                    invocation.contains(&format!("{var}:<unset>")),
                    "ambient {var} must never reach the admitted cargo subprocess: {invocation}"
                );
            }
            assert!(
                !invocation.contains(&format!("PATH:{}", ambient_path.to_string_lossy())),
                "the real ambient PATH sentinel must never be inherited into the admitted \
                 cargo subprocess, even though a POSIX /bin/sh may synthesize its own default \
                 PATH once env_clear() leaves PATH unset: {invocation}"
            );
        }
    }

    #[test]
    fn disabled_admission_never_attempts_cargo_version() {
        let staged_root = tempfile::tempdir().expect("staged root");
        let error = resolve_cargo(&CargoToolAdmission::Disabled, staged_root.path())
            .expect_err("Disabled must never resolve a cargo executable");
        assert_eq!(error.kind, CargoToolFailureKind::NotAdmitted);
    }

    #[test]
    fn a_relative_admitted_path_fails_closed() {
        let staged_root = tempfile::tempdir().expect("staged root");
        let admission = CargoToolAdmission::TrustedExecutable(PathBuf::from("cargo"));
        let error = resolve_cargo(&admission, staged_root.path())
            .expect_err("a relative admitted path must never be trusted");
        assert_eq!(error.kind, CargoToolFailureKind::AdmittedExecutableInvalid);
    }

    #[test]
    fn a_nonexistent_admitted_path_fails_closed() {
        let bin_dir = tempfile::tempdir().expect("bin dir");
        let staged_root = tempfile::tempdir().expect("staged root");
        let admission =
            CargoToolAdmission::TrustedExecutable(bin_dir.path().join("does-not-exist"));
        let error = resolve_cargo(&admission, staged_root.path())
            .expect_err("a nonexistent admitted path must never be trusted");
        assert_eq!(error.kind, CargoToolFailureKind::AdmittedExecutableInvalid);
    }

    #[test]
    fn a_directory_admitted_path_fails_closed() {
        let bin_dir = tempfile::tempdir().expect("bin dir");
        let directory = bin_dir.path().join("cargo-looking-directory");
        fs::create_dir(&directory).expect("bogus directory target");
        let staged_root = tempfile::tempdir().expect("staged root");
        let admission = CargoToolAdmission::TrustedExecutable(directory);
        let error = resolve_cargo(&admission, staged_root.path())
            .expect_err("a directory admitted path must never be trusted");
        assert_eq!(error.kind, CargoToolFailureKind::AdmittedExecutableInvalid);
    }

    #[test]
    fn a_non_executable_admitted_file_fails_closed() {
        let bin_dir = tempfile::tempdir().expect("bin dir");
        let cargo_path = bin_dir.path().join("cargo");
        fs::write(&cargo_path, fake_cargo_version_script("cargo 1.99.0-test"))
            .expect("write non-executable fake cargo");
        let staged_root = tempfile::tempdir().expect("staged root");
        let admission = CargoToolAdmission::TrustedExecutable(cargo_path);
        let error = resolve_cargo(&admission, staged_root.path())
            .expect_err("a non-executable admitted path must never be trusted");
        assert_eq!(error.kind, CargoToolFailureKind::AdmittedExecutableInvalid);
    }

    #[test]
    fn a_valid_trusted_executable_is_admitted_and_its_real_version_is_reported() {
        let bin_dir = tempfile::tempdir().expect("bin dir");
        let cargo_path = bin_dir.path().join("cargo");
        write_test_executable(&cargo_path, &fake_cargo_version_script("cargo 1.99.0-test"));
        let staged_root = tempfile::tempdir().expect("staged root");
        let admission = CargoToolAdmission::TrustedExecutable(cargo_path.clone());
        let (executable, version) = resolve_cargo(&admission, staged_root.path())
            .expect("a valid absolute, executable, regular-file path must be admitted");
        assert_eq!(
            executable,
            fs::canonicalize(&cargo_path).expect("canonicalize fake cargo")
        );
        assert_eq!(version, "cargo 1.99.0-test");
    }

    #[test]
    fn cargo_version_and_cargo_metadata_run_the_same_admitted_executable_from_the_same_cwd() {
        let staged_root = tempfile::tempdir().expect("staged root");
        let log_path = staged_root.path().join("invocations.log");
        let cargo_path = staged_root.path().join("fake-cargo.sh");
        write_test_executable(&cargo_path, &fake_logging_cargo_script(&log_path));

        let version =
            cargo_version(&cargo_path, staged_root.path()).expect("fake cargo --version succeeds");
        assert_eq!(version, "cargo 1.99.0-test");

        let _ = run_cargo_metadata(&cargo_path, staged_root.path(), "Cargo.toml")
            .expect("fake cargo metadata succeeds");

        let log = fs::read_to_string(&log_path).expect("read invocation log");
        let invocations = log
            .split("---\n")
            .filter(|entry| !entry.trim().is_empty())
            .collect::<Vec<_>>();
        assert_eq!(
            invocations.len(),
            2,
            "both invocations must have logged: {log}"
        );
        let canonical_cargo_path = fs::canonicalize(&cargo_path).expect("canonicalize fake cargo");
        let canonical_root =
            fs::canonicalize(staged_root.path()).expect("canonicalize staged root");
        for invocation in &invocations {
            assert!(
                invocation.contains(&format!("ARGV0:{}", cargo_path.display()))
                    || invocation.contains(&format!("ARGV0:{}", canonical_cargo_path.display())),
                "both invocations must run the exact resolved executable: {invocation}"
            );
            assert!(
                invocation.contains(&format!("PWD:{}", canonical_root.display())),
                "both invocations must run from the same staged cwd: {invocation}"
            );
        }
        let metadata_invocation = invocations[1];
        assert!(metadata_invocation.contains("CARGO_NET_OFFLINE:true"));
        assert!(metadata_invocation.contains("CARGO_TERM_COLOR:never"));
        assert!(
            metadata_invocation.contains(&format!(
                "CARGO_HOME:{}",
                staged_root
                    .path()
                    .join(".reviewgraphen-cargo-home")
                    .display()
            )),
            "cargo metadata must keep CARGO_HOME inside the staged snapshot: {metadata_invocation}"
        );
        assert!(
            metadata_invocation.contains(
                "ARGS:metadata --offline --no-deps --format-version=1 --manifest-path Cargo.toml"
            ),
            "cargo metadata must keep --offline and its other fixed flags: {metadata_invocation}"
        );
    }
}
