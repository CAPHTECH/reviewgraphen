//! Bounded, deterministic ingestion for local Git-backed Rust repositories.
//!
//! This crate deliberately stops at accepted `ProgramSpace` facts.  It does
//! not execute target code, expand macros, infer dispatch targets, or turn a
//! missing fact into an absence claim.

mod git;
mod rust;

use reviewgraphen_core::{
    self as rg_core, ContentHash, DomainError, ProgramSpace, SnapshotSourceBundle,
    SnapshotSourceEntry, StableId, canonical_json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub use git::ChangeKind;

/// Versioned identifier for the public ingestion report shape.
pub const EXTRACTION_REPORT_SCHEMA: &str = "reviewgraphen.extraction_report.v1";

/// Closed schema identifier for one v2 unresolved call occurrence.
pub const INGESTION_OBSTRUCTION_V2_SCHEMA: &str = "reviewgraphen.ingestion_obstruction.v2";

/// Versioned identifier for the isolated v2 ingestion sidecar.
pub const INGESTION_REPORT_V2_SCHEMA: &str = "reviewgraphen.ingestion_report.v2";

/// Versioned identifier for the distinct global v2 limitation.
pub const INGESTION_GLOBAL_LIMITATION_V2_SCHEMA: &str =
    "reviewgraphen.ingestion_global_limitation.v2";

const INGESTION_V2_PROJECTION_EXTRACTOR_ID: &str = "reviewgraphen.ingest.rust-call-enumeration@2";

/// The exact `syn` version this crate was actually built against, read from
/// the workspace `Cargo.lock` at compile time by `build.rs` -- never a
/// string copied by hand, which could silently drift from the real pin.
const SYN_VERSION: &str = env!("REVIEWGRAPHEN_INGEST_SYN_VERSION");

/// The exact `proc-macro2` version this crate was actually built against,
/// read the same way as `SYN_VERSION`. `syn`'s `Span`s -- and this crate's
/// derived `Location`s and symbol IDs -- are backed directly by
/// `proc_macro2::Span`, so a different `proc-macro2` resolution changes
/// span/location computation, not just an unrelated transitive dependency.
const PROC_MACRO2_VERSION: &str = env!("REVIEWGRAPHEN_INGEST_PROC_MACRO2_VERSION");

/// Exact locked `quote` version. Rust semantic anchors use `ToTokens`, so a
/// quote upgrade is an extractor-identity change even when syn is unchanged.
const QUOTE_VERSION: &str = env!("REVIEWGRAPHEN_INGEST_QUOTE_VERSION");

/// Bounded resource limits for one local ingestion request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestLimits {
    /// Maximum tracked regular files read from the requested Git tree.
    pub max_files: usize,
    /// Maximum byte size of one tracked regular file.
    pub max_file_bytes: usize,
}

#[cfg(test)]
mod shared_path_contract_tests {
    use super::normalized_repository_path;

    #[test]
    fn ingest_path_obeys_shared_snapshot_relative_contract() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/snapshot-relative-path-contract.v1.json"
        )))
        .expect("shared path contract fixture");
        assert_eq!(
            fixture["schema"],
            "reviewgraphen.snapshot_relative_path_contract_cases.v1"
        );
        for case in fixture["common_cases"].as_array().expect("common cases") {
            let path = case["path"].as_str().expect("case path");
            let valid = case["valid"].as_bool().expect("case validity");
            assert_eq!(
                normalized_repository_path(path),
                valid,
                "shared path contract case {path:?}"
            );
        }
    }
}

impl Default for IngestLimits {
    fn default() -> Self {
        Self {
            max_files: 20_000,
            max_file_bytes: 2 * 1024 * 1024,
        }
    }
}

/// Host trust boundary for the Cargo executable `cargo_metadata` may invoke.
///
/// M2 never searches `PATH`, never spawns `rustup`/`mise`/`asdf`, and never
/// attempts to install or download a toolchain: an audited runtime `rustup`
/// probe can itself touch the toolchain even under a "never install"
/// configuration, so no automatic resolution can be made safe from inside
/// this crate. A caller that wants Cargo metadata must instead admit an
/// already-verified, absolute host `cargo` executable explicitly -- that
/// admission, performed by the caller/harness outside this crate, is the
/// trust boundary, not anything this module resolves on its own.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CargoToolAdmission {
    /// Cargo metadata is never attempted. `cargo_metadata` is reported
    /// `missing` (`NotRun`) with a typed `cargo_metadata_unavailable`
    /// obstruction naming the cause, exactly as if Cargo were unavailable
    /// on the host at all.
    #[default]
    Disabled,
    /// The caller/harness has already admitted this absolute path as a
    /// real, host-installed `cargo` executable, outside this crate's own
    /// trust boundary. It must be absolute; it is canonicalized and
    /// checked to be an existing regular, executable file before use (a
    /// relative, nonexistent, or directory path is a typed failure, never
    /// silently accepted), and the identical resolved path is reused for
    /// both `cargo --version` and `cargo metadata`, from the same staged
    /// snapshot working directory.
    TrustedExecutable(PathBuf),
}

/// Deterministic configuration for the M2 adapter set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestConfig {
    /// Static bounds applied before source bytes are parsed.
    pub limits: IngestLimits,
    /// Code-review profile identity retained in the v1 ProgramSpace contract.
    pub profile_id: String,
    /// Code-review profile version retained in the v1 ProgramSpace contract.
    pub profile_version: String,
    /// Rule-set identity supplied to the core; M2 does not reinterpret it.
    pub rule_set_hash: ContentHash,
    /// Policy identity supplied to the core; M2 does not reinterpret it.
    pub policy_version: String,
    /// Host trust boundary for the Cargo executable `cargo_metadata` may
    /// invoke. Defaults to `Disabled`: no Cargo executable is ever
    /// resolved or spawned unless the caller explicitly admits one.
    pub cargo_admission: CargoToolAdmission,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            limits: IngestLimits::default(),
            profile_id: "code-review".to_owned(),
            profile_version: "1".to_owned(),
            rule_set_hash: ContentHash::parse("sha256:3333333333333333")
                .expect("M2 static rule-set hash is valid"),
            policy_version: "default-evidence@1".to_owned(),
            cargo_admission: CargoToolAdmission::default(),
        }
    }
}

/// A bounded request to ingest one immutable local Git revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestRequest {
    /// The outer local workspace allowed to contain the target repository.
    pub workspace_root: PathBuf,
    /// Local Git repository root. It must be within `workspace_root`.
    pub repository_root: PathBuf,
    /// Caller-supplied stable repository identity (for example a remote URL
    /// or a logical repository name). Unlike `repository_root`, it does not
    /// depend on the local clone path, so the derived repository/snapshot
    /// IDs and provenance stay identical across clones of the same
    /// repository at different filesystem locations.
    pub repository_identity: String,
    /// Base commit used only for deterministic changed-structure mapping.
    pub base_revision: String,
    /// Target commit whose tree is extracted into ProgramSpace.
    pub target_revision: String,
    /// Versioned adapter configuration.
    pub config: IngestConfig,
}

impl IngestRequest {
    /// Builds an M2 request without resolving paths or executing commands.
    #[must_use]
    pub fn new(
        workspace_root: impl Into<PathBuf>,
        repository_root: impl Into<PathBuf>,
        repository_identity: impl Into<String>,
        base_revision: impl Into<String>,
        target_revision: impl Into<String>,
    ) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            repository_root: repository_root.into(),
            repository_identity: repository_identity.into(),
            base_revision: base_revision.into(),
            target_revision: target_revision.into(),
            config: IngestConfig::default(),
        }
    }
}

/// Capability completeness reported by an adapter or fact family.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    /// The bounded adapter completely covered its declared input subset.
    Complete,
    /// The adapter returned facts while retaining explicit unknown regions.
    Partial,
    /// The adapter cannot provide this fact family for the snapshot.
    Missing,
    /// The adapter did not establish a completeness result.
    Unknown,
}

impl CapabilityState {
    const fn as_core(self) -> rg_core::CapabilityState {
        match self {
            Self::Complete => rg_core::CapabilityState::Complete,
            Self::Partial => rg_core::CapabilityState::Partial,
            Self::Missing => rg_core::CapabilityState::Missing,
            Self::Unknown => rg_core::CapabilityState::Unknown,
        }
    }
}

/// Stable adapter status in the public ingestion report.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterStatus {
    /// The adapter completed its declared bounded scope.
    Complete,
    /// The adapter completed with explicitly recorded losses.
    Partial,
    /// The adapter was attempted but could not return its declared facts.
    Failed,
    /// The adapter was deliberately not run because a safety precondition failed.
    NotRun,
}

impl AdapterStatus {
    const fn as_core(self) -> rg_core::AdapterStatus {
        match self {
            Self::Complete => rg_core::AdapterStatus::Complete,
            Self::Partial => rg_core::AdapterStatus::Partial,
            Self::Failed => rg_core::AdapterStatus::Failed,
            Self::NotRun => rg_core::AdapterStatus::NotRun,
        }
    }
}

/// One bounded adapter's completeness declaration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AdapterReport {
    /// Versioned adapter identifier.
    pub id: String,
    /// Adapter implementation version.
    pub version: String,
    /// Completion state for this snapshot.
    pub status: AdapterStatus,
    /// Number of accepted (successfully handled) inputs, when meaningful.
    pub parsed: Option<u64>,
    /// Number of discovered inputs, when meaningful. `parsed <= total`.
    pub total: Option<u64>,
    /// Of `total`, the number deliberately excluded by a declared bound
    /// (for example a symlink or a Git submodule entry), when meaningful.
    /// Every excluded input is retained as a typed [`IngestionObstruction`].
    pub excluded: Option<u64>,
    /// Of `total`, the number the adapter attempted but could not handle
    /// (for example a Rust parse failure), when meaningful. Distinct from
    /// `excluded`: a failure was attempted and did not succeed, an
    /// exclusion was never attempted because a bound ruled it out first.
    /// `parsed + excluded + failed == total` when all four are present.
    pub failed: Option<u64>,
}

/// An explicit obstruction/unknown retained by M2 extraction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestionObstruction {
    /// Stable core limitation identifier.
    pub id: StableId,
    /// Typed category; never a free-form error-only diagnostic.
    pub kind: IngestionObstructionKind,
    /// Severity describes review impact, not program correctness.
    pub severity: ObstructionSeverity,
    /// Deterministic explanatory text.
    pub description: String,
    /// Affected ProgramSpace source IDs where available.
    pub source_ids: BTreeSet<StableId>,
    /// Source paths retained for consumers that cannot yet dereference IDs.
    pub paths: BTreeSet<String>,
}

/// Closed v2 serialization of one located unresolved call occurrence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestionObstructionV2 {
    /// Versioned closed record discriminator.
    schema: String,
    /// Stable obstruction ID whose preimage binds every field except
    /// `description`.
    id: StableId,
    /// Typed extraction-loss category.
    kind: IngestionObstructionKind,
    /// Review impact, not a program-correctness assertion.
    severity: ObstructionSeverity,
    /// Deterministic rendering of the typed record fields.
    description: String,
    /// Sorted, nonempty accepted source IDs.
    source_ids: BTreeSet<StableId>,
    /// Exact normalized source path as a singleton set.
    paths: BTreeSet<String>,
    /// Exactly `["direct_calls"]` for every occurrence.
    related_capabilities: BTreeSet<String>,
    /// Closed call syntax family.
    call_kind: CallKind,
    /// Closed unresolved-call reason.
    reason: CallObstructionReason,
    /// Exact inclusive source span.
    span: IngestionObstructionSpan,
    /// Snapshot source ID.
    snapshot_id: StableId,
    /// Extractor contract identifier.
    extractor_id: String,
}

/// The global direct-call limitation is deliberately a different closed type
/// from a located occurrence; it has no path or span.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestionGlobalLimitationV2 {
    schema: String,
    id: StableId,
    snapshot_id: StableId,
    projection_extractor_id: String,
    kind: String,
    severity: ObstructionSeverity,
    description: String,
    source_ids: BTreeSet<StableId>,
    related_capabilities: BTreeSet<String>,
    latent_occurrence_count: LatentOccurrenceCount,
}

/// Validated, isolated v2 ingestion sidecar. Private fields prevent an
/// unchecked deserializer or legacy builder from constructing it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestionReportV2 {
    schema: String,
    report_id: StableId,
    snapshot_id: StableId,
    target_revision: String,
    legacy_program_space_sha256: ContentHash,
    legacy_extraction_report_sha256: ContentHash,
    projection_extractor_id: String,
    located_call_occurrences: Vec<IngestionObstructionV2>,
    global_direct_calls_limitation: IngestionGlobalLimitationV2,
}

/// The closed syntactic family of a v2 unresolved call occurrence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallKind {
    /// A direct `ExprCall` occurrence.
    Direct,
    /// A method-call occurrence whose dispatch target was not resolved.
    Method,
    /// A macro invocation whose expansion was not inspected.
    MacroInvocation,
}

impl CallKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Method => "method",
            Self::MacroInvocation => "macro_invocation",
        }
    }
}

/// The closed reason vocabulary for a v2 unresolved call occurrence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallObstructionReason {
    /// A direct call did not use a path expression.
    DirectNonPath,
    /// A direct path expression did not contain a path segment.
    DirectEmptyPath,
    /// A direct call name was shadowed by a local binding.
    DirectShadowedBinding,
    /// A direct call occurred in a scope with an unresolved import shape.
    DirectUnresolvedScope,
    /// Syntactic lookup found no accepted target.
    DirectTargetCountZero,
    /// Syntactic lookup found more than one accepted target.
    DirectTargetCountMultiple,
    /// Method dispatch was deliberately not resolved.
    MethodDispatchUnresolved,
    /// Macro expansion was deliberately not inspected.
    MacroExpansionUnresolved,
}

impl CallObstructionReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::DirectNonPath => "direct_non_path",
            Self::DirectEmptyPath => "direct_empty_path",
            Self::DirectShadowedBinding => "direct_shadowed_binding",
            Self::DirectUnresolvedScope => "direct_unresolved_scope",
            Self::DirectTargetCountZero => "direct_target_count_zero",
            Self::DirectTargetCountMultiple => "direct_target_count_multiple",
            Self::MethodDispatchUnresolved => "method_dispatch_unresolved",
            Self::MacroExpansionUnresolved => "macro_expansion_unresolved",
        }
    }
}

/// Exact inclusive source coordinates retained for a v2 occurrence record.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct IngestionObstructionSpan {
    /// Normalized repository-relative source path.
    pub path: String,
    /// One-based inclusive start line.
    pub start_line: u64,
    /// One-based inclusive end line.
    pub end_line: u64,
    /// One-based inclusive start column.
    pub start_column: u64,
    /// One-based inclusive end column.
    pub end_column: u64,
}

/// The only permitted latent call-cardinality state for the current Rust
/// macro-expansion boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LatentOccurrenceCount {
    /// Expansion can contain an unbounded, unobserved number of call sites.
    Unknown,
}

impl LatentOccurrenceCount {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
        }
    }
}

/// Categories of analysis loss intentionally retained by the M2 contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestionObstructionKind {
    /// A file could not be parsed as Rust.
    ParseFailure,
    /// A macro invocation was observed but was not expanded or resolved.
    MacroExpansionUnresolved,
    /// A method/trait/dynamic dispatch target was not resolved.
    DynamicDispatchUnresolved,
    /// A syntactic relation could not be mapped to an accepted target fact.
    RelationUnresolved,
    /// A repository entry or region was deliberately excluded by a bound.
    RegionExcluded,
    /// Cargo metadata could not safely be collected from the immutable snapshot.
    CargoMetadataUnavailable,
    /// The source input is outside the bounded M2 contract.
    UnsupportedInput,
    /// A deliberately conservative unknown that does not fit a stronger class.
    Unknown,
}

impl IngestionObstructionKind {
    const fn as_core(self) -> rg_core::LimitationKind {
        match self {
            Self::ParseFailure => rg_core::LimitationKind::ParseFailure,
            Self::MacroExpansionUnresolved
            | Self::DynamicDispatchUnresolved
            | Self::RelationUnresolved => rg_core::LimitationKind::UnresolvedRelation,
            Self::RegionExcluded => rg_core::LimitationKind::ExcludedRegion,
            Self::CargoMetadataUnavailable => rg_core::LimitationKind::CapabilityMissing,
            Self::UnsupportedInput => rg_core::LimitationKind::UnsupportedInput,
            Self::Unknown => rg_core::LimitationKind::Unknown,
        }
    }
}

/// Descriptive severity for an ingestion obstruction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObstructionSeverity {
    /// Informational retained loss.
    Info,
    /// Low review impact.
    Low,
    /// Medium review impact.
    Medium,
    /// High review impact.
    High,
    /// Critical review impact.
    Critical,
}

impl ObstructionSeverity {
    const fn as_core(self) -> rg_core::Severity {
        match self {
            Self::Info => rg_core::Severity::Info,
            Self::Low => rg_core::Severity::Low,
            Self::Medium => rg_core::Severity::Medium,
            Self::High => rg_core::Severity::High,
            Self::Critical => rg_core::Severity::Critical,
        }
    }
}

/// Versioned completeness and obstruction output for one ingestion run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExtractionReport {
    schema: &'static str,
    /// Snapshot bound to every accepted fact and limitation.
    pub snapshot_id: StableId,
    /// Resolved immutable target commit.
    pub target_revision: String,
    /// Hash of the complete deterministic adapter/configuration set.
    pub adapter_set_hash: ContentHash,
    /// Deterministically ordered adapter declarations.
    pub adapters: Vec<AdapterReport>,
    /// Explicit state for each limited M2 fact family.
    pub capabilities: BTreeMap<String, CapabilityState>,
    /// Explicit unknowns and safety restrictions.
    pub obstructions: Vec<IngestionObstruction>,
}

impl ExtractionReport {
    /// The versioned report schema identifier.
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        self.schema
    }
}

/// Successful M2 result, retaining both accepted facts and their completeness.
#[derive(Clone, Debug, PartialEq)]
pub struct IngestResult {
    /// Parsed and core-validated ProgramSpace input facts.
    pub program_space: ProgramSpace,
    /// Adapter completeness and unknown regions for the same snapshot.
    pub extraction_report: ExtractionReport,
}

/// Successful source-retaining ingest result. Source bytes are a validated
/// snapshot handoff, separate from accepted ProgramSpace facts and evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct IngestWithSourcesResult {
    /// Parsed and core-validated ProgramSpace facts.
    pub program_space: ProgramSpace,
    /// Adapter completeness and unknown regions for the same snapshot.
    pub extraction_report: ExtractionReport,
    /// Exact source bytes for every accepted file artifact in this snapshot.
    pub source_bundle: SnapshotSourceBundle,
}

/// The only public route that couples unchanged legacy ingestion with the
/// separately validated v2 call-enumeration sidecar.
#[derive(Clone, Debug, PartialEq)]
pub struct IngestResultV2 {
    pub legacy: IngestResult,
    pub ingestion_report_v2: IngestionReportV2,
}

/// Source-retaining counterpart to [`IngestResultV2`].
#[derive(Clone, Debug, PartialEq)]
pub struct IngestWithSourcesResultV2 {
    pub legacy: IngestWithSourcesResult,
    pub ingestion_report_v2: IngestionReportV2,
}

impl IngestResult {
    /// Returns byte-stable JSON for the ProgramSpace plus its M2 report.
    pub fn canonical_output(&self) -> Result<Vec<u8>, IngestError> {
        canonical_json(&CanonicalOutput {
            program_space: &self.program_space,
            extraction_report: &self.extraction_report,
        })
        .map_err(IngestError::from)
    }
}

impl IngestionReportV2 {
    /// Canonical standalone sidecar bytes. There is intentionally no combined
    /// v1/v2 serializer.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, IngestError> {
        canonical_json(self).map_err(IngestError::from)
    }

    #[must_use]
    pub fn report_id(&self) -> &StableId {
        &self.report_id
    }

    #[must_use]
    pub fn located_call_occurrences(&self) -> &[IngestionObstructionV2] {
        &self.located_call_occurrences
    }

    #[must_use]
    pub fn global_direct_calls_limitation(&self) -> &IngestionGlobalLimitationV2 {
        &self.global_direct_calls_limitation
    }
}

impl IngestionObstructionV2 {
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn call_kind(&self) -> CallKind {
        self.call_kind
    }
    #[must_use]
    pub fn reason(&self) -> CallObstructionReason {
        self.reason
    }
    #[must_use]
    pub fn span(&self) -> &IngestionObstructionSpan {
        &self.span
    }
}

impl IngestionGlobalLimitationV2 {
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn latent_occurrence_count(&self) -> LatentOccurrenceCount {
        self.latent_occurrence_count
    }
}

#[derive(Serialize)]
struct CanonicalOutput<'a> {
    program_space: &'a ProgramSpace,
    extraction_report: &'a ExtractionReport,
}

/// Typed failures at the local-ingestion security and I/O boundary.
#[derive(Debug, Error)]
pub enum IngestError {
    /// The request cannot establish a safe bounded input.
    #[error("invalid ingestion request: {0}")]
    InvalidRequest(String),
    /// A canonical target lies outside the explicitly allowed workspace.
    #[error("path `{path}` escapes workspace scope `{workspace}`")]
    WorkspaceEscape { path: PathBuf, workspace: PathBuf },
    /// A target is not exactly the local repository root requested by the caller.
    #[error("requested repository root `{requested}` resolves to Git root `{actual}")]
    RepositoryRootMismatch { requested: PathBuf, actual: PathBuf },
    /// A Git tree path would escape its snapshot root.
    #[error("Git tree path escapes the snapshot root: `{path}`")]
    PathEscape { path: String },
    /// A symlink was encountered where immutable regular file bytes are required.
    #[error("Git tree contains a symlink that cannot be followed: `{path}`")]
    SymlinkEscape { path: String },
    /// An allow-listed child process could not be launched.
    #[error("failed to run allow-listed {command}: {source}")]
    CommandIo {
        command: &'static str,
        #[source]
        source: std::io::Error,
    },
    /// An allow-listed child process returned a non-success status.
    #[error("allow-listed {command} failed with {status}: {stderr}")]
    CommandFailed {
        command: &'static str,
        status: i32,
        stderr: String,
    },
    /// An allow-listed child process's captured stdout or stderr exceeded
    /// the configured bound, even though the process itself exited
    /// successfully. The captured bytes are never returned truncated: this
    /// is a hard failure of the whole run, not a partial/best-effort result.
    #[error(
        "allow-listed {command} {stream} exceeded the {limit}-byte captured-output bound; \
         its output was truncated and cannot be trusted, so the run fails closed instead of \
         parsing a partial result"
    )]
    OutputLimitExceeded {
        command: &'static str,
        stream: &'static str,
        limit: usize,
    },
    /// A bounded adapter returned malformed data.
    #[error("adapter output is malformed: {0}")]
    AdapterOutput(String),
    /// A target file exceeded the explicitly configured byte limit.
    #[error("Git blob `{path}` is {actual_bytes} bytes, above the {max_bytes} byte bound")]
    FileTooLarge {
        path: String,
        actual_bytes: usize,
        max_bytes: usize,
    },
    /// The configured file limit was reached before all tracked regular files could be read.
    #[error("Git tree contains more than the configured {max_files} regular-file bound")]
    FileLimitExceeded { max_files: usize },
    /// The aggregate source handoff exceeded its caller-provided byte limit.
    #[error(
        "snapshot source bytes total {actual_total_source_bytes}, above the {max_total_source_bytes} byte bound"
    )]
    SourceBundleTooLarge {
        max_total_source_bytes: u64,
        actual_total_source_bytes: u64,
    },
    /// Source bytes do not exactly match the ProgramSpace file artifact they
    /// claim to retain. The whole ingest fails; no partial bundle is exposed.
    #[error("invalid snapshot source bundle entry `{artifact_id}`: {reason}")]
    InvalidSourceBundle {
        artifact_id: StableId,
        reason: String,
    },
    /// Core validation rejected data that an adapter attempted to lift.
    #[error(transparent)]
    Core(#[from] DomainError),
    /// JSON emitted by an allow-listed adapter was invalid.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Ingests exactly one immutable local Git target revision without a network request.
///
/// The only child executables are private, fixed `git` read commands and the
/// fixed `cargo metadata --offline --no-deps` invocation. Target repository
/// bytes are read through Git; target files are never written or executed.
pub fn ingest(request: &IngestRequest) -> Result<IngestResult, IngestError> {
    let result = ingest_pipeline(request, None, false)?;
    Ok(result.ingest)
}

/// Ingests one immutable snapshot and retains the exact already-read source
/// bytes, subject to an aggregate caller-owned source budget.
pub fn ingest_with_sources(
    request: &IngestRequest,
    max_total_source_bytes: u64,
) -> Result<IngestWithSourcesResult, IngestError> {
    let result = ingest_pipeline(request, Some(max_total_source_bytes), false)?;
    let source_bundle = result
        .source_bundle
        .expect("source-retaining pipeline always produces a source bundle");
    Ok(IngestWithSourcesResult {
        program_space: result.ingest.program_space,
        extraction_report: result.ingest.extraction_report,
        source_bundle,
    })
}

/// Ingests the unchanged legacy v1 product and its isolated v2 sidecar.
pub fn ingest_v2(request: &IngestRequest) -> Result<IngestResultV2, IngestError> {
    let result = ingest_pipeline(request, None, true)?;
    Ok(IngestResultV2 {
        legacy: result.ingest,
        ingestion_report_v2: result
            .ingestion_report_v2
            .expect("v2 pipeline always constructs a sidecar"),
    })
}

/// Source-retaining v2 ingestion with the same legacy source-budget contract.
pub fn ingest_with_sources_v2(
    request: &IngestRequest,
    max_total_source_bytes: u64,
) -> Result<IngestWithSourcesResultV2, IngestError> {
    let result = ingest_pipeline(request, Some(max_total_source_bytes), true)?;
    let source_bundle = result
        .source_bundle
        .expect("source-retaining pipeline always produces a source bundle");
    Ok(IngestWithSourcesResultV2 {
        legacy: IngestWithSourcesResult {
            program_space: result.ingest.program_space,
            extraction_report: result.ingest.extraction_report,
            source_bundle,
        },
        ingestion_report_v2: result
            .ingestion_report_v2
            .expect("v2 pipeline always constructs a sidecar"),
    })
}

struct IngestPipelineResult {
    ingest: IngestResult,
    source_bundle: Option<SnapshotSourceBundle>,
    ingestion_report_v2: Option<IngestionReportV2>,
}

/// Shared private pipeline for source-retaining and ordinary ingestion.
fn ingest_pipeline(
    request: &IngestRequest,
    max_total_source_bytes: Option<u64>,
    include_v2: bool,
) -> Result<IngestPipelineResult, IngestError> {
    validate_config(&request.config)?;
    let snapshot = git::load_snapshot(request, max_total_source_bytes)?;
    let identities = SnapshotIdentities::new(
        &snapshot,
        &request.config,
        SYN_VERSION,
        PROC_MACRO2_VERSION,
        QUOTE_VERSION,
        &git::git_command_policy_fingerprint(),
        &git::cargo_resolver_policy_fingerprint(),
    )?;

    let mut drafts = Vec::new();
    let mut issues = snapshot.issues.clone();
    let mut adapter_reports = snapshot.adapter_reports.clone();
    let mut capabilities = snapshot.capabilities.clone();
    let mut capability_sources = snapshot.capability_sources.clone();

    for file in &snapshot.files {
        drafts.push(file_artifact_draft(file));
    }

    let rust = rust::extract(&snapshot, &identities.snapshot_id);
    adapter_reports.push(rust.adapter_report);
    capabilities.extend(rust.capabilities);
    capability_sources.extend(rust.capability_sources);
    issues.extend(rust.issues);
    drafts.extend(rust.artifacts);
    let mut relation_drafts = rust.relations;
    let rust_anchor_drafts = rust.symbol_anchors;
    let v2_obstruction_drafts = rust.v2_obstructions;

    let cargo = git::extract_cargo_metadata(&snapshot, &identities.snapshot_id);
    adapter_reports.push(cargo.adapter_report);
    capabilities.extend(cargo.capabilities);
    capability_sources.extend(cargo.capability_sources);
    issues.extend(cargo.issues);
    drafts.extend(cargo.artifacts);
    relation_drafts.extend(cargo.relations);

    for change in &snapshot.changes {
        let key = git::change_key(change);
        let mut attributes = Map::new();
        attributes.insert(
            "change_kind".to_owned(),
            Value::String(change.kind.as_str().to_owned()),
        );
        attributes.insert(
            "base_path".to_owned(),
            Value::String(change.base_path.clone()),
        );
        attributes.insert(
            "target_path".to_owned(),
            Value::String(change.target_path.clone()),
        );
        // The change-family artifact is the one record in a snapshot whose
        // whole existence is already base-relative (its key carries
        // `base_path`, and it exists at all only because Git's own diff
        // reported this path changed), so it is the only record that can
        // carry the base-relative `changed` fact without making a
        // same-ID artifact's canonical body depend on the diff base. Every
        // consumer that asks "was this symbol changed?" -- obligation
        // synthesis included -- reads it from here, through the
        // change-family `contains` edges built below, never from the
        // symbol's own attributes.
        attributes.insert("changed".to_owned(), Value::Bool(true));
        // `changed_lines` is comparison-specific (it depends on
        // `base_revision`), so it lives only on this change-family artifact,
        // never on the target file/function/method/type record it describes:
        // those records keep the same ID *and* the same canonical body no
        // matter which base a caller diffs against.
        let target_file = snapshot.file_by_path(&change.target_path);
        if let Some(changed_lines) = target_file
            .map(|file| &file.changed_lines)
            .filter(|lines| !lines.is_empty())
        {
            attributes.insert(
                "changed_lines".to_owned(),
                Value::Array(
                    changed_lines
                        .iter()
                        .map(|line| Value::Number((*line).into()))
                        .collect(),
                ),
            );
        }
        drafts.push(ArtifactDraft {
            key: key.clone(),
            id_kind: "change",
            kind: "custom",
            label: format!("{} {}", change.kind.as_str(), change.target_path),
            language: None,
            location: None,
            content_hash: None,
            attributes,
            source_path: change
                .target_path
                .is_empty()
                .then_some(change.base_path.clone())
                .or_else(|| Some(change.target_path.clone())),
            extraction_method: "reviewgraphen.ingest.git.changed_structure.v1",
        });
        if let Some(file) = target_file {
            relation_drafts.push(RelationDraft {
                kind: "changed_by",
                source_key: format!("file:{}", change.target_path),
                target_keys: vec![key.clone()],
                attributes: Map::new(),
                source_path: Some(change.target_path.clone()),
                extraction_method: "reviewgraphen.ingest.git.changed_structure.v1",
            });
            // Every other accepted record at this path (function, method,
            // type, test, state, ...) links to the change through this
            // relation pair too, rather than carrying its own base-relative
            // `changed`/`changed_lines` attribute.
            for draft in &drafts {
                if draft.id_kind == "file" {
                    continue;
                }
                let Some(location) = &draft.location else {
                    continue;
                };
                if location.path != change.target_path
                    || !intersects_changed(&file.changed_lines, location)
                {
                    continue;
                }
                relation_drafts.push(RelationDraft {
                    kind: "changed_by",
                    source_key: draft.key.clone(),
                    target_keys: vec![key.clone()],
                    attributes: Map::new(),
                    source_path: Some(change.target_path.clone()),
                    extraction_method: "reviewgraphen.ingest.git.changed_structure.v1",
                });
                // The same membership, in the direction a consumer reading
                // containment travels: the change-family artifact contains
                // exactly the accepted records whose source region it
                // changed. This is what carries `changed` down to an
                // individual function/method/type/test/state record --
                // `changed_by` alone points the other way, so nothing
                // walking down from the change could reach them. Both edges
                // are change-family facts, are the only relations whose
                // existence depends on `base_revision`, and are excluded
                // from the base-invariance contract for exactly that reason
                // (see docs/20_m2_ingestion_contract.md).
                relation_drafts.push(RelationDraft {
                    kind: "contains",
                    source_key: key.clone(),
                    target_keys: vec![draft.key.clone()],
                    attributes: Map::new(),
                    source_path: Some(change.target_path.clone()),
                    extraction_method: "reviewgraphen.ingest.git.changed_structure.v1",
                });
            }
        }
    }

    let lifted = lift(
        &snapshot,
        &identities,
        LiftInputs {
            drafts,
            relation_drafts,
            rust_anchor_drafts,
            issues,
            adapter_reports,
            capabilities,
            capability_sources,
        },
    )?;
    let ingestion_report_v2 = if include_v2 {
        let legacy_index = V2LegacyValidationIndex::new(&lifted)?;
        let report = build_ingestion_report_v2(
            &snapshot,
            &identities,
            &lifted,
            &legacy_index,
            v2_obstruction_drafts,
        )?;
        // Admission is a second immutable-snapshot reconstruction, not a
        // trust decision over the first in-memory record collection.
        let rebuilt_obstructions =
            rust::extract(&snapshot, &identities.snapshot_id).v2_obstructions;
        let rebuilt = build_ingestion_report_v2(
            &snapshot,
            &identities,
            &lifted,
            &legacy_index,
            rebuilt_obstructions,
        )?;
        // The closed report DTO derives field-wise equality over every field
        // it serializes. Comparing the typed reconstructions is therefore
        // exactly equivalent to comparing their canonical encodings, without
        // allocating two very large temporary JSON buffers.
        if report != rebuilt {
            return Err(IngestError::AdapterOutput(
                "v2 ingestion sidecar did not reproduce from the immutable snapshot".to_owned(),
            ));
        }
        Some(report)
    } else {
        None
    };
    let source_bundle = max_total_source_bytes
        .map(|_| source_bundle_from_snapshot(&snapshot, &lifted.program_space))
        .transpose()?;
    Ok(IngestPipelineResult {
        ingest: lifted,
        source_bundle,
        ingestion_report_v2,
    })
}

struct V2LegacyValidationIndex {
    program_hash: ContentHash,
    extraction_hash: ContentHash,
    known_ids: BTreeSet<StableId>,
    accepted_file_paths: BTreeSet<String>,
}

impl V2LegacyValidationIndex {
    fn new(legacy: &IngestResult) -> Result<Self, IngestError> {
        Ok(Self {
            program_hash: ContentHash::sha256(&canonical_json(&legacy.program_space)?),
            extraction_hash: ContentHash::sha256(&canonical_json(&legacy.extraction_report)?),
            known_ids: legacy.program_space.known_ids(),
            accepted_file_paths: legacy
                .program_space
                .artifacts()
                .iter()
                .filter(|artifact| artifact.kind == "file")
                .filter_map(|artifact| {
                    artifact
                        .location
                        .as_ref()
                        .map(|location| location.path.clone())
                })
                .collect(),
        })
    }
}

struct V2OccurrenceSourceIndex {
    line_lengths_by_path: BTreeMap<String, Vec<usize>>,
}

impl V2OccurrenceSourceIndex {
    fn new(snapshot: &git::GitSnapshot, drafts: &[V2IngestionObstructionDraft]) -> Self {
        let occurrence_paths = drafts
            .iter()
            .filter_map(|draft| draft.call_occurrence.as_ref())
            .map(|occurrence| occurrence.span.path.as_str())
            .collect::<BTreeSet<_>>();
        let line_lengths_by_path = snapshot
            .files
            .iter()
            .filter(|file| occurrence_paths.contains(file.path.as_str()))
            .map(|file| (file.path.clone(), v2_line_lengths(&file.content)))
            .collect();
        Self {
            line_lengths_by_path,
        }
    }

    fn line_lengths(&self, path: &str) -> Option<&[usize]> {
        self.line_lengths_by_path.get(path).map(Vec::as_slice)
    }
}

fn v2_line_lengths(bytes: &[u8]) -> Vec<usize> {
    bytes
        .split(|byte| *byte == b'\n')
        .map(<[u8]>::len)
        .collect()
}

fn build_ingestion_report_v2(
    snapshot: &git::GitSnapshot,
    identities: &SnapshotIdentities,
    legacy: &IngestResult,
    legacy_index: &V2LegacyValidationIndex,
    drafts: Vec<V2IngestionObstructionDraft>,
) -> Result<IngestionReportV2, IngestError> {
    let occurrence_source_index = V2OccurrenceSourceIndex::new(snapshot, &drafts);
    let source_id_index =
        build_v2_source_id_index(&drafts, &legacy_index.known_ids, &identities.snapshot_id)?;
    let direct_sources = legacy
        .program_space
        .extraction()
        .capabilities
        .get("direct_calls")
        .ok_or_else(|| IngestError::AdapterOutput("missing direct_calls capability".to_owned()))?
        .source_ids
        .clone();
    if direct_sources.is_empty() {
        return Err(IngestError::AdapterOutput(
            "v2 global direct_calls limitation requires accepted sources".to_owned(),
        ));
    }

    let mut occurrences = Vec::new();
    let mut globals = Vec::new();
    for draft in drafts {
        match (&draft.call_occurrence, draft.latent_occurrence_count) {
            (Some(occurrence), None) => {
                let source_ids = resolve_v2_source_ids(&draft.source_keys, &source_id_index)?;
                validate_occurrence_draft(
                    &occurrence_source_index,
                    &draft,
                    occurrence,
                    &source_ids,
                )?;
                let span = IngestionObstructionSpan {
                    path: occurrence.span.path.clone(),
                    start_line: occurrence.span.start_line,
                    end_line: occurrence.span.end_line,
                    start_column: occurrence.span.start_column,
                    end_column: occurrence.span.end_column,
                };
                let description = render_v2_call_occurrence_description(occurrence);
                let id = derived_id(
                    "ingestion-obstruction",
                    [
                        (
                            "schema",
                            Value::String(INGESTION_OBSTRUCTION_V2_SCHEMA.to_owned()),
                        ),
                        (
                            "snapshot_id",
                            Value::String(identities.snapshot_id.to_string()),
                        ),
                        (
                            "extractor_id",
                            Value::String(occurrence.extractor_id.to_owned()),
                        ),
                        ("kind", Value::String(format!("{:?}", draft.kind))),
                        ("severity", Value::String(format!("{:?}", draft.severity))),
                        ("source_ids", stable_id_values(&source_ids)),
                        ("paths", string_set_values(&draft.paths)),
                        (
                            "related_capabilities",
                            string_set_values(&draft.related_capabilities),
                        ),
                        (
                            "call_kind",
                            Value::String(occurrence.call_kind.as_str().to_owned()),
                        ),
                        (
                            "reason",
                            Value::String(occurrence.reason.as_str().to_owned()),
                        ),
                        ("span", span_value(&span)),
                    ],
                )?;
                occurrences.push(IngestionObstructionV2 {
                    schema: INGESTION_OBSTRUCTION_V2_SCHEMA.to_owned(),
                    id,
                    kind: draft.kind,
                    severity: draft.severity,
                    description,
                    source_ids,
                    paths: draft.paths,
                    related_capabilities: draft.related_capabilities,
                    call_kind: occurrence.call_kind,
                    reason: occurrence.reason,
                    span,
                    snapshot_id: identities.snapshot_id.clone(),
                    extractor_id: occurrence.extractor_id.to_owned(),
                });
            }
            (None, Some(LatentOccurrenceCount::Unknown)) => globals.push(draft),
            _ => {
                return Err(IngestError::AdapterOutput(
                    "v2 draft must be exactly one located occurrence or the global limitation"
                        .to_owned(),
                ));
            }
        }
    }
    if globals.len() != 1 {
        return Err(IngestError::AdapterOutput(
            "v2 sidecar requires exactly one global direct_calls limitation".to_owned(),
        ));
    }
    let global_draft = globals.pop().expect("length checked");
    if global_draft.kind != IngestionObstructionKind::RelationUnresolved
        || global_draft.severity != ObstructionSeverity::Info
        || global_draft.related_capabilities != BTreeSet::from(["direct_calls".to_owned()])
    {
        return Err(IngestError::AdapterOutput(
            "v2 global direct_calls limitation violates its closed contract".to_owned(),
        ));
    }
    let global_sources = direct_sources;
    let global_description = render_v2_global_description();
    let global_id = derived_id(
        "ingestion-limitation",
        [
            (
                "schema",
                Value::String(INGESTION_GLOBAL_LIMITATION_V2_SCHEMA.to_owned()),
            ),
            (
                "snapshot_id",
                Value::String(identities.snapshot_id.to_string()),
            ),
            (
                "projection_extractor_id",
                Value::String(INGESTION_V2_PROJECTION_EXTRACTOR_ID.to_owned()),
            ),
            (
                "kind",
                Value::String("global_direct_calls_limitation".to_owned()),
            ),
            ("severity", Value::String("Info".to_owned())),
            ("source_ids", stable_id_values(&global_sources)),
            (
                "related_capabilities",
                Value::Array(vec![Value::String("direct_calls".to_owned())]),
            ),
            (
                "latent_occurrence_count",
                Value::String("unknown".to_owned()),
            ),
        ],
    )?;
    let global = IngestionGlobalLimitationV2 {
        schema: INGESTION_GLOBAL_LIMITATION_V2_SCHEMA.to_owned(),
        id: global_id,
        snapshot_id: identities.snapshot_id.clone(),
        projection_extractor_id: INGESTION_V2_PROJECTION_EXTRACTOR_ID.to_owned(),
        kind: "global_direct_calls_limitation".to_owned(),
        severity: ObstructionSeverity::Info,
        description: global_description,
        source_ids: global_sources,
        related_capabilities: BTreeSet::from(["direct_calls".to_owned()]),
        latent_occurrence_count: LatentOccurrenceCount::Unknown,
    };
    occurrences.sort_by(|left, right| left.id.cmp(&right.id));
    let program_hash = legacy_index.program_hash.clone();
    let extraction_hash = legacy_index.extraction_hash.clone();
    let report_id = derived_id(
        "ingestion-report",
        [
            (
                "schema",
                Value::String(INGESTION_REPORT_V2_SCHEMA.to_owned()),
            ),
            (
                "snapshot_id",
                Value::String(identities.snapshot_id.to_string()),
            ),
            (
                "target_revision",
                Value::String(legacy.program_space.target_revision().to_owned()),
            ),
            (
                "legacy_program_space_sha256",
                Value::String(program_hash.to_string()),
            ),
            (
                "legacy_extraction_report_sha256",
                Value::String(extraction_hash.to_string()),
            ),
            (
                "projection_extractor_id",
                Value::String(INGESTION_V2_PROJECTION_EXTRACTOR_ID.to_owned()),
            ),
            (
                "located_call_occurrences",
                stable_id_values(&occurrences.iter().map(|value| value.id.clone()).collect()),
            ),
            (
                "global_direct_calls_limitation",
                Value::String(global.id.to_string()),
            ),
        ],
    )?;
    let report = IngestionReportV2 {
        schema: INGESTION_REPORT_V2_SCHEMA.to_owned(),
        report_id,
        snapshot_id: identities.snapshot_id.clone(),
        target_revision: legacy.program_space.target_revision().to_owned(),
        legacy_program_space_sha256: program_hash,
        legacy_extraction_report_sha256: extraction_hash,
        projection_extractor_id: INGESTION_V2_PROJECTION_EXTRACTOR_ID.to_owned(),
        located_call_occurrences: occurrences,
        global_direct_calls_limitation: global,
    };
    validate_ingestion_report_v2_with_index(&report, legacy, legacy_index)?;
    Ok(report)
}

fn stable_id_values(ids: &BTreeSet<StableId>) -> Value {
    Value::Array(ids.iter().map(|id| Value::String(id.to_string())).collect())
}

fn string_set_values(values: &BTreeSet<String>) -> Value {
    Value::Array(values.iter().cloned().map(Value::String).collect())
}

fn span_value(span: &IngestionObstructionSpan) -> Value {
    Value::Object(Map::from_iter([
        ("path".to_owned(), Value::String(span.path.clone())),
        (
            "start_line".to_owned(),
            Value::Number(span.start_line.into()),
        ),
        ("end_line".to_owned(), Value::Number(span.end_line.into())),
        (
            "start_column".to_owned(),
            Value::Number(span.start_column.into()),
        ),
        (
            "end_column".to_owned(),
            Value::Number(span.end_column.into()),
        ),
    ]))
}

fn build_v2_source_id_index(
    drafts: &[V2IngestionObstructionDraft],
    known_ids: &BTreeSet<StableId>,
    snapshot_id: &StableId,
) -> Result<BTreeMap<String, StableId>, IngestError> {
    drafts
        .iter()
        .flat_map(|draft| draft.source_keys.iter())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|key| {
            let id = derived_id(
                v2_source_key_kind(key)?,
                [
                    ("snapshot", Value::String(snapshot_id.to_string())),
                    ("key", Value::String(key.clone())),
                ],
            )?;
            if !known_ids.contains(&id) {
                return Err(IngestError::AdapterOutput(
                    "v2 source key did not resolve to an accepted source ID".to_owned(),
                ));
            }
            Ok((key.clone(), id))
        })
        .collect()
}

fn resolve_v2_source_ids(
    keys: &BTreeSet<String>,
    source_id_index: &BTreeMap<String, StableId>,
) -> Result<BTreeSet<StableId>, IngestError> {
    if keys.is_empty() {
        return Err(IngestError::AdapterOutput(
            "v2 source keys cannot be empty".to_owned(),
        ));
    }
    keys.iter()
        .map(|key| {
            source_id_index.get(key).cloned().ok_or_else(|| {
                IngestError::AdapterOutput(
                    "v2 source key did not resolve to an accepted source ID".to_owned(),
                )
            })
        })
        .collect()
}

fn v2_source_key_kind(key: &str) -> Result<&str, IngestError> {
    match key.split(':').next() {
        Some("file") => Ok("file"),
        Some("function") => Ok("function"),
        Some("method") => Ok("method"),
        Some("test") => Ok("test"),
        Some("module") => Ok("module"),
        _ => Err(IngestError::AdapterOutput(format!(
            "v2 source key has no accepted artifact kind: {key}"
        ))),
    }
}

fn validate_occurrence_draft(
    source_index: &V2OccurrenceSourceIndex,
    draft: &V2IngestionObstructionDraft,
    occurrence: &CallOccurrenceDraft,
    source_ids: &BTreeSet<StableId>,
) -> Result<(), IngestError> {
    let expected_kind = match occurrence.call_kind {
        CallKind::Direct => IngestionObstructionKind::RelationUnresolved,
        CallKind::Method => IngestionObstructionKind::DynamicDispatchUnresolved,
        CallKind::MacroInvocation => IngestionObstructionKind::MacroExpansionUnresolved,
    };
    if draft.kind != expected_kind
        || draft.severity != ObstructionSeverity::Medium
        || draft.related_capabilities != BTreeSet::from(["direct_calls".to_owned()])
        || draft.paths != BTreeSet::from([occurrence.span.path.clone()])
        || source_ids.is_empty()
        || occurrence.extractor_id != INGESTION_V2_PROJECTION_EXTRACTOR_ID
    {
        return Err(IngestError::AdapterOutput(
            "v2 occurrence violates its closed contract".to_owned(),
        ));
    }
    let line_lengths = source_index
        .line_lengths(&occurrence.span.path)
        .ok_or_else(|| {
            IngestError::AdapterOutput("v2 occurrence path is not an admitted source".to_owned())
        })?;
    validate_span_in_line_lengths(&occurrence.span, line_lengths)
}

fn validate_span_in_line_lengths(
    span: &LocationDraft,
    line_lengths: &[usize],
) -> Result<(), IngestError> {
    if span.start_line == 0
        || span.end_line < span.start_line
        || span.start_column == 0
        || span.end_column == 0
        || (span.start_line == span.end_line && span.end_column < span.start_column)
    {
        return Err(IngestError::AdapterOutput(
            "v2 occurrence span is not inclusive".to_owned(),
        ));
    }
    let line = |number: u64| {
        line_lengths.get(usize::try_from(number.saturating_sub(1)).unwrap_or(usize::MAX))
    };
    let Some(start) = line(span.start_line) else {
        return Err(IngestError::AdapterOutput(
            "v2 occurrence start lies outside accepted source".to_owned(),
        ));
    };
    let Some(end) = line(span.end_line) else {
        return Err(IngestError::AdapterOutput(
            "v2 occurrence end lies outside accepted source".to_owned(),
        ));
    };
    if span.start_column > u64::try_from(*start + 1).expect("usize fits u64")
        || span.end_column > u64::try_from(*end).expect("usize fits u64")
    {
        return Err(IngestError::AdapterOutput(
            "v2 occurrence columns lie outside accepted source".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod v2_occurrence_performance_tests {
    use super::*;

    #[test]
    fn large_source_is_indexed_once_for_many_occurrence_validations() {
        const LINE_COUNT: usize = 82_401;
        const OCCURRENCE_COUNT: usize = 321_099;

        let mut bytes = Vec::with_capacity(LINE_COUNT * 41);
        for line in 0..LINE_COUNT {
            bytes.extend_from_slice(b"0123456789012345678901234567890123456789");
            if line + 1 != LINE_COUNT {
                bytes.push(b'\n');
            }
        }
        let line_lengths = v2_line_lengths(&bytes);
        assert_eq!(line_lengths.len(), LINE_COUNT);
        assert_eq!(line_lengths.iter().sum::<usize>(), LINE_COUNT * 40);

        let span = LocationDraft {
            path: "large.rs".to_owned(),
            start_line: LINE_COUNT as u64,
            end_line: LINE_COUNT as u64,
            start_column: 1,
            end_column: 40,
        };
        for _ in 0..OCCURRENCE_COUNT {
            validate_span_in_line_lengths(&span, &line_lengths)
                .expect("an indexed span remains valid");
        }

        // The per-occurrence API receives only the O(lines) index, not the
        // 3.4 MiB source bytes, making a source-byte rescan inside this loop
        // structurally impossible.
        assert_eq!(line_lengths.len(), LINE_COUNT);
    }
}

fn render_v2_call_occurrence_description(occurrence: &CallOccurrenceDraft) -> String {
    format!(
        "direct_calls unresolved occurrence: call_kind={}; reason={}; path={}; span={}:{}-{}:{}",
        occurrence.call_kind.as_str(),
        occurrence.reason.as_str(),
        occurrence.span.path,
        occurrence.span.start_line,
        occurrence.span.start_column,
        occurrence.span.end_line,
        occurrence.span.end_column
    )
}

fn render_v2_global_description() -> String {
    "direct_calls enumeration is incomplete; macro latent occurrence count is unknown".to_owned()
}

/// Decodes only canonical, closed v2 sidecar bytes and revalidates every
/// semantic binding against the unchanged legacy pair. No unchecked serde
/// value can be obtained as an [`IngestionReportV2`].
pub fn decode_and_validate_ingestion_report_v2(
    bytes: &[u8],
    legacy_program_space: &ProgramSpace,
    legacy_extraction_report: &ExtractionReport,
) -> Result<IngestionReportV2, IngestError> {
    let value: Value = serde_json::from_slice(bytes)?;
    if canonical_json(&value)? != bytes {
        return Err(IngestError::AdapterOutput(
            "v2 ingestion report is not canonical JSON".to_owned(),
        ));
    }
    let raw: RawIngestionReportV2 = serde_json::from_value(value)?;
    let legacy = IngestResult {
        program_space: legacy_program_space.clone(),
        extraction_report: legacy_extraction_report.clone(),
    };
    let report = raw.into_validated()?;
    validate_ingestion_report_v2(&report, &legacy)?;
    Ok(report)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIngestionReportV2 {
    schema: String,
    report_id: StableId,
    snapshot_id: StableId,
    target_revision: String,
    legacy_program_space_sha256: ContentHash,
    legacy_extraction_report_sha256: ContentHash,
    projection_extractor_id: String,
    located_call_occurrences: Vec<RawIngestionObstructionV2>,
    global_direct_calls_limitation: RawIngestionGlobalLimitationV2,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIngestionObstructionV2 {
    schema: String,
    id: StableId,
    kind: IngestionObstructionKind,
    severity: ObstructionSeverity,
    description: String,
    source_ids: Vec<StableId>,
    paths: Vec<String>,
    related_capabilities: Vec<String>,
    call_kind: CallKind,
    reason: CallObstructionReason,
    span: RawIngestionObstructionSpan,
    snapshot_id: StableId,
    extractor_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIngestionObstructionSpan {
    path: String,
    start_line: u64,
    end_line: u64,
    start_column: u64,
    end_column: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIngestionGlobalLimitationV2 {
    schema: String,
    id: StableId,
    snapshot_id: StableId,
    projection_extractor_id: String,
    kind: String,
    severity: ObstructionSeverity,
    description: String,
    source_ids: Vec<StableId>,
    related_capabilities: Vec<String>,
    latent_occurrence_count: LatentOccurrenceCount,
}

impl RawIngestionReportV2 {
    fn into_validated(self) -> Result<IngestionReportV2, IngestError> {
        let occurrences = self
            .located_call_occurrences
            .into_iter()
            .map(RawIngestionObstructionV2::into_validated)
            .collect::<Result<Vec<_>, _>>()?;
        if !strictly_sorted(occurrences.iter().map(|item| &item.id)) {
            return Err(IngestError::AdapterOutput(
                "v2 occurrence IDs must be strictly sorted".to_owned(),
            ));
        }
        Ok(IngestionReportV2 {
            schema: self.schema,
            report_id: self.report_id,
            snapshot_id: self.snapshot_id,
            target_revision: self.target_revision,
            legacy_program_space_sha256: self.legacy_program_space_sha256,
            legacy_extraction_report_sha256: self.legacy_extraction_report_sha256,
            projection_extractor_id: self.projection_extractor_id,
            located_call_occurrences: occurrences,
            global_direct_calls_limitation: self.global_direct_calls_limitation.into_validated()?,
        })
    }
}

impl RawIngestionObstructionV2 {
    fn into_validated(self) -> Result<IngestionObstructionV2, IngestError> {
        Ok(IngestionObstructionV2 {
            schema: self.schema,
            id: self.id,
            kind: self.kind,
            severity: self.severity,
            description: self.description,
            source_ids: ordered_set(self.source_ids, "v2 occurrence source_ids")?,
            paths: ordered_set(self.paths, "v2 occurrence paths")?,
            related_capabilities: ordered_set(
                self.related_capabilities,
                "v2 occurrence related_capabilities",
            )?,
            call_kind: self.call_kind,
            reason: self.reason,
            span: IngestionObstructionSpan {
                path: self.span.path,
                start_line: self.span.start_line,
                end_line: self.span.end_line,
                start_column: self.span.start_column,
                end_column: self.span.end_column,
            },
            snapshot_id: self.snapshot_id,
            extractor_id: self.extractor_id,
        })
    }
}

impl RawIngestionGlobalLimitationV2 {
    fn into_validated(self) -> Result<IngestionGlobalLimitationV2, IngestError> {
        Ok(IngestionGlobalLimitationV2 {
            schema: self.schema,
            id: self.id,
            snapshot_id: self.snapshot_id,
            projection_extractor_id: self.projection_extractor_id,
            kind: self.kind,
            severity: self.severity,
            description: self.description,
            source_ids: ordered_set(self.source_ids, "v2 global source_ids")?,
            related_capabilities: ordered_set(
                self.related_capabilities,
                "v2 global related_capabilities",
            )?,
            latent_occurrence_count: self.latent_occurrence_count,
        })
    }
}

fn strictly_sorted<'a, T: Ord + 'a>(values: impl IntoIterator<Item = &'a T>) -> bool {
    values
        .into_iter()
        .collect::<Vec<_>>()
        .windows(2)
        .all(|pair| pair[0] < pair[1])
}

fn ordered_set<T: Ord>(values: Vec<T>, field: &str) -> Result<BTreeSet<T>, IngestError> {
    if !strictly_sorted(values.iter()) {
        return Err(IngestError::AdapterOutput(format!(
            "{field} must be strictly sorted and duplicate-free"
        )));
    }
    Ok(values.into_iter().collect())
}

fn validate_ingestion_report_v2(
    report: &IngestionReportV2,
    legacy: &IngestResult,
) -> Result<(), IngestError> {
    let legacy_index = V2LegacyValidationIndex::new(legacy)?;
    validate_ingestion_report_v2_with_index(report, legacy, &legacy_index)
}

fn validate_ingestion_report_v2_with_index(
    report: &IngestionReportV2,
    legacy: &IngestResult,
    legacy_index: &V2LegacyValidationIndex,
) -> Result<(), IngestError> {
    let snapshot_id = legacy.program_space.snapshot_id();
    if report.schema != INGESTION_REPORT_V2_SCHEMA
        || report.snapshot_id != *snapshot_id
        || report.target_revision != legacy.program_space.target_revision()
        || report.projection_extractor_id != INGESTION_V2_PROJECTION_EXTRACTOR_ID
        || legacy.extraction_report.snapshot_id != *snapshot_id
        || legacy.extraction_report.target_revision != legacy.program_space.target_revision()
    {
        return Err(IngestError::AdapterOutput(
            "v2 report identity mismatch".to_owned(),
        ));
    }
    if report.legacy_program_space_sha256 != legacy_index.program_hash
        || report.legacy_extraction_report_sha256 != legacy_index.extraction_hash
    {
        return Err(IngestError::AdapterOutput(
            "v2 report legacy hash mismatch".to_owned(),
        ));
    }
    let direct_sources = legacy
        .program_space
        .extraction()
        .capabilities
        .get("direct_calls")
        .ok_or_else(|| {
            IngestError::AdapterOutput("legacy report lacks direct_calls capability".to_owned())
        })?
        .source_ids
        .clone();
    validate_global_v2(
        &report.global_direct_calls_limitation,
        snapshot_id,
        &direct_sources,
    )?;
    let mut occurrence_ids = BTreeSet::new();
    let mut previous = None;
    for occurrence in &report.located_call_occurrences {
        if previous
            .as_ref()
            .is_some_and(|id: &StableId| id >= &occurrence.id)
        {
            return Err(IngestError::AdapterOutput(
                "v2 occurrence order is not canonical".to_owned(),
            ));
        }
        previous = Some(occurrence.id.clone());
        validate_occurrence_v2(
            occurrence,
            snapshot_id,
            &legacy_index.known_ids,
            &legacy_index.accepted_file_paths,
        )?;
        occurrence_ids.insert(occurrence.id.clone());
    }
    let expected_report_id = derived_id(
        "ingestion-report",
        [
            (
                "schema",
                Value::String(INGESTION_REPORT_V2_SCHEMA.to_owned()),
            ),
            ("snapshot_id", Value::String(snapshot_id.to_string())),
            (
                "target_revision",
                Value::String(legacy.program_space.target_revision().to_owned()),
            ),
            (
                "legacy_program_space_sha256",
                Value::String(report.legacy_program_space_sha256.to_string()),
            ),
            (
                "legacy_extraction_report_sha256",
                Value::String(report.legacy_extraction_report_sha256.to_string()),
            ),
            (
                "projection_extractor_id",
                Value::String(INGESTION_V2_PROJECTION_EXTRACTOR_ID.to_owned()),
            ),
            (
                "located_call_occurrences",
                stable_id_values(&occurrence_ids),
            ),
            (
                "global_direct_calls_limitation",
                Value::String(report.global_direct_calls_limitation.id.to_string()),
            ),
        ],
    )?;
    if report.report_id != expected_report_id {
        return Err(IngestError::AdapterOutput(
            "v2 report ID preimage mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn validate_global_v2(
    global: &IngestionGlobalLimitationV2,
    snapshot_id: &StableId,
    direct_sources: &BTreeSet<StableId>,
) -> Result<(), IngestError> {
    if global.schema != INGESTION_GLOBAL_LIMITATION_V2_SCHEMA
        || global.snapshot_id != *snapshot_id
        || global.projection_extractor_id != INGESTION_V2_PROJECTION_EXTRACTOR_ID
        || global.kind != "global_direct_calls_limitation"
        || global.severity != ObstructionSeverity::Info
        || global.related_capabilities != BTreeSet::from(["direct_calls".to_owned()])
        || global.latent_occurrence_count != LatentOccurrenceCount::Unknown
        || global.source_ids != *direct_sources
        || global.source_ids.is_empty()
        || global.description.as_bytes() != render_v2_global_description().as_bytes()
    {
        return Err(IngestError::AdapterOutput(
            "v2 global limitation validation failed".to_owned(),
        ));
    }
    let expected_id = derived_id(
        "ingestion-limitation",
        [
            (
                "schema",
                Value::String(INGESTION_GLOBAL_LIMITATION_V2_SCHEMA.to_owned()),
            ),
            ("snapshot_id", Value::String(snapshot_id.to_string())),
            (
                "projection_extractor_id",
                Value::String(INGESTION_V2_PROJECTION_EXTRACTOR_ID.to_owned()),
            ),
            (
                "kind",
                Value::String("global_direct_calls_limitation".to_owned()),
            ),
            ("severity", Value::String("Info".to_owned())),
            ("source_ids", stable_id_values(&global.source_ids)),
            (
                "related_capabilities",
                string_set_values(&global.related_capabilities),
            ),
            (
                "latent_occurrence_count",
                Value::String(global.latent_occurrence_count.as_str().to_owned()),
            ),
        ],
    )?;
    if global.id != expected_id {
        return Err(IngestError::AdapterOutput(
            "v2 global limitation ID preimage mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn validate_occurrence_v2(
    occurrence: &IngestionObstructionV2,
    snapshot_id: &StableId,
    known_ids: &BTreeSet<StableId>,
    accepted_file_paths: &BTreeSet<String>,
) -> Result<(), IngestError> {
    let expected_kind = match occurrence.call_kind {
        CallKind::Direct => IngestionObstructionKind::RelationUnresolved,
        CallKind::Method => IngestionObstructionKind::DynamicDispatchUnresolved,
        CallKind::MacroInvocation => IngestionObstructionKind::MacroExpansionUnresolved,
    };
    if occurrence.schema != INGESTION_OBSTRUCTION_V2_SCHEMA
        || occurrence.snapshot_id != *snapshot_id
        || occurrence.extractor_id != INGESTION_V2_PROJECTION_EXTRACTOR_ID
        || occurrence.kind != expected_kind
        || occurrence.severity != ObstructionSeverity::Medium
        || occurrence.related_capabilities.len() != 1
        || !occurrence.related_capabilities.contains("direct_calls")
        || occurrence.source_ids.is_empty()
        || !occurrence
            .source_ids
            .iter()
            .all(|id| known_ids.contains(id))
        || occurrence.paths.len() != 1
        || !occurrence.paths.contains(&occurrence.span.path)
        || !normalized_repository_path(&occurrence.span.path)
        || occurrence.description.as_bytes() != render_occurrence_from_span(occurrence).as_bytes()
    {
        return Err(IngestError::AdapterOutput(
            "v2 occurrence validation failed".to_owned(),
        ));
    }
    if !accepted_file_paths.contains(&occurrence.span.path)
        || occurrence.span.start_line == 0
        || occurrence.span.end_line < occurrence.span.start_line
        || occurrence.span.start_column == 0
        || occurrence.span.end_column == 0
        || (occurrence.span.start_line == occurrence.span.end_line
            && occurrence.span.end_column < occurrence.span.start_column)
    {
        return Err(IngestError::AdapterOutput(
            "v2 occurrence span is outside accepted source".to_owned(),
        ));
    }
    let expected_id = derived_id(
        "ingestion-obstruction",
        [
            (
                "schema",
                Value::String(INGESTION_OBSTRUCTION_V2_SCHEMA.to_owned()),
            ),
            ("snapshot_id", Value::String(snapshot_id.to_string())),
            (
                "extractor_id",
                Value::String(occurrence.extractor_id.clone()),
            ),
            ("kind", Value::String(format!("{:?}", occurrence.kind))),
            (
                "severity",
                Value::String(format!("{:?}", occurrence.severity)),
            ),
            ("source_ids", stable_id_values(&occurrence.source_ids)),
            ("paths", string_set_values(&occurrence.paths)),
            (
                "related_capabilities",
                string_set_values(&occurrence.related_capabilities),
            ),
            (
                "call_kind",
                Value::String(occurrence.call_kind.as_str().to_owned()),
            ),
            (
                "reason",
                Value::String(occurrence.reason.as_str().to_owned()),
            ),
            ("span", span_value(&occurrence.span)),
        ],
    )?;
    if occurrence.id != expected_id {
        return Err(IngestError::AdapterOutput(
            "v2 occurrence ID preimage mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn render_occurrence_from_span(occurrence: &IngestionObstructionV2) -> String {
    format!(
        "direct_calls unresolved occurrence: call_kind={}; reason={}; path={}; span={}:{}-{}:{}",
        occurrence.call_kind.as_str(),
        occurrence.reason.as_str(),
        occurrence.span.path,
        occurrence.span.start_line,
        occurrence.span.start_column,
        occurrence.span.end_line,
        occurrence.span.end_column
    )
}

fn normalized_repository_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn source_bundle_from_snapshot(
    snapshot: &git::GitSnapshot,
    program_space: &ProgramSpace,
) -> Result<SnapshotSourceBundle, IngestError> {
    let file_artifacts = program_space
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
        .filter_map(|artifact| {
            artifact
                .location
                .as_ref()
                .map(|location| (location.path.as_str(), artifact))
        })
        .collect::<BTreeMap<_, _>>();
    let entries = snapshot
        .files
        .iter()
        .map(|file| {
            let artifact = file_artifacts.get(file.path.as_str()).ok_or_else(|| {
                IngestError::InvalidSourceBundle {
                    artifact_id: program_space.snapshot_id().clone(),
                    reason: format!(
                        "accepted ProgramSpace has no file artifact for `{}`",
                        file.path
                    ),
                }
            })?;
            Ok(SnapshotSourceEntry::new(
                artifact.id.clone(),
                file.path.clone(),
                file.content_hash.clone(),
                ContentHash::sha256(&file.content),
                file.content.clone(),
            ))
        })
        .collect::<Result<Vec<_>, IngestError>>()?;
    SnapshotSourceBundle::new(program_space, entries).map_err(|error| match error {
        DomainError::InvalidSnapshotSourceBundle {
            artifact_id,
            reason,
        } => IngestError::InvalidSourceBundle {
            artifact_id,
            reason,
        },
        other => IngestError::Core(other),
    })
}

fn validate_config(config: &IngestConfig) -> Result<(), IngestError> {
    if config.limits.max_files == 0 || config.limits.max_file_bytes == 0 {
        return Err(IngestError::InvalidRequest(
            "max_files and max_file_bytes must both be positive".to_owned(),
        ));
    }
    for (field, value) in [
        ("profile_id", &config.profile_id),
        ("profile_version", &config.profile_version),
        ("policy_version", &config.policy_version),
    ] {
        if value.is_empty() {
            return Err(IngestError::InvalidRequest(format!(
                "{field} must not be empty"
            )));
        }
    }
    Ok(())
}

struct SnapshotIdentities {
    repository_id: StableId,
    snapshot_id: StableId,
    adapter_set_hash: ContentHash,
}

impl SnapshotIdentities {
    /// `syn_version`/`proc_macro2_version`/`quote_version` are parameters
    /// (always the corresponding locked constants at the real call site in
    /// `ingest()`) rather than read directly from those constants here, so
    /// `adapter_set_hash`'s sensitivity to a different compiled-in tool
    /// version is directly testable against this typed intermediate,
    /// exactly like its sensitivity to `snapshot.git_version`/
    /// `snapshot.cargo_version`.
    fn new(
        snapshot: &git::GitSnapshot,
        config: &IngestConfig,
        syn_version: &str,
        proc_macro2_version: &str,
        quote_version: &str,
        git_command_policy: &Value,
        cargo_resolver_policy: &Value,
    ) -> Result<Self, IngestError> {
        let repository_id = derived_id(
            "repository",
            [(
                "identity",
                Value::String(snapshot.repository_identity.clone()),
            )],
        )?;
        // `base_revision` is deliberately excluded: it only bounds the
        // changed-structure diff mapping, not the target tree's identity.
        // Including it would mint a different snapshot/artifact/relation ID
        // set for the exact same target tree merely because a caller chose
        // a different diff base, contradicting the M2 promise that facts are
        // bound to `(workspace scope, repository root, target revision,
        // configuration)`.
        let snapshot_id = derived_id(
            "snapshot",
            [
                ("repository", Value::String(repository_id.to_string())),
                (
                    "target_revision",
                    Value::String(snapshot.target_revision.clone()),
                ),
                ("tree_hash", Value::String(snapshot.tree_hash.to_string())),
            ],
        )?;
        // Bound alongside the fixed adapter contract-version strings and
        // `limits` (the only `IngestConfig` field that affects what gets
        // parsed -- `profile_id`/`profile_version`/`rule_set_hash`/
        // `policy_version` are pass-through identity already recorded
        // separately in `ProfileDescriptor`, never consulted by any
        // adapter): the *real* tool versions this run actually executed,
        // never guessed or hardcoded. `git`/`cargo` come from the bounded
        // executor at request time; `syn`/`proc-macro2` are fixed at
        // compile time from the workspace `Cargo.lock`. `cargo` is
        // structured, never a bare `null`, so an unavailable *kind* is
        // itself part of the fingerprint rather than indistinguishable
        // from "no opinion" -- and `extract_cargo_metadata` never runs (or
        // accepts facts from) `cargo metadata` at all while it is `Err`, so
        // a `null`/absent `cargo` version can never coexist with accepted
        // `cargo_metadata` facts. A failure's *kind* (see
        // `git::CargoToolFailureKind`), never its raw diagnostic, human
        // text, or the private staged `TempDir` path it may have echoed, is
        // what's bound here: two ingests with the same environment and the
        // same input therefore still hash identically even when they staged
        // into two different `TempDir`s (none of the four inputs depend on
        // the clone's filesystem path); a different host, toolchain, or
        // `limits` value -- or a genuinely different failure kind -- still
        // changes `adapter_set_hash`, which is the point of binding them
        // here. `git_command_policy` (see
        // `git::git_command_policy_fingerprint`) is bound the same way: it
        // is not itself a tool *version*, but it is exactly as load-bearing
        // -- every allow-listed `git` call is forced through one fixed,
        // repo/host-config-independent policy (disabled system/global
        // config, fixed diff algorithm and rename detection, disabled
        // external diff/textconv), so a future change to that policy's
        // *shape* must be visible in the fingerprint too, not silently
        // absorbed. `cargo_resolver_policy` (see
        // `git::cargo_resolver_policy_fingerprint`) is bound the same way,
        // for the same reason: the strict host-admission policy (no `PATH`
        // search, no `rustup`/`mise`/`asdf` spawn, a caller-admitted
        // absolute executable path admitted and reused as-is) decides
        // *whether and which* `cargo` binary a run actually executes, so a
        // future change to that policy's shape must be visible in the
        // fingerprint too. Only the fixed policy values are bound, exactly
        // like `git_command_policy` -- never the host-specific admitted
        // executable path itself (see `git::GitSnapshot::cargo_executable`'s
        // own doc comment for why that path is deliberately excluded from
        // every identity/canonical output; only `cargo_tool_version`'s
        // `version` string, below, is load-bearing here).
        let cargo_tool_version = match &snapshot.cargo_version {
            Ok(version) => json!({ "available": true, "version": version }),
            Err(failure) => {
                json!({ "available": false, "unavailable_kind": failure.kind.as_str() })
            }
        };
        let adapter_set_hash = ContentHash::sha256(&canonical_json(&json!({
            "contract": "reviewgraphen-ingest@1",
            "git_adapter": "reviewgraphen.ingest.git@1",
            "rust_adapter": "reviewgraphen.ingest.rust-syn@1",
            "rust_responsibility_signals": "reviewgraphen.ingest.rust-responsibility-signals@1",
            "cargo_adapter": "reviewgraphen.ingest.cargo-metadata@1",
            "limits": config.limits,
            "tool_versions": {
                "git": snapshot.git_version,
                "cargo": cargo_tool_version,
                "syn": syn_version,
                "proc_macro2": proc_macro2_version,
                "quote": quote_version,
            },
            "git_command_policy": git_command_policy,
            "cargo_resolver_policy": cargo_resolver_policy,
        }))?);
        Ok(Self {
            repository_id,
            snapshot_id,
            adapter_set_hash,
        })
    }
}

#[derive(Clone)]
pub(crate) struct ArtifactDraft {
    pub(crate) key: String,
    pub(crate) id_kind: &'static str,
    pub(crate) kind: &'static str,
    pub(crate) label: String,
    pub(crate) language: Option<&'static str>,
    pub(crate) location: Option<LocationDraft>,
    pub(crate) content_hash: Option<ContentHash>,
    pub(crate) attributes: Map<String, Value>,
    pub(crate) source_path: Option<String>,
    pub(crate) extraction_method: &'static str,
}

#[derive(Clone)]
pub(crate) struct RelationDraft {
    pub(crate) kind: &'static str,
    pub(crate) source_key: String,
    /// Extractor-declared endpoint sequence. Lift validates uniqueness while
    /// retaining this order separately from the relation's set domain.
    pub(crate) target_keys: Vec<String>,
    pub(crate) attributes: Map<String, Value>,
    pub(crate) source_path: Option<String>,
    pub(crate) extraction_method: &'static str,
}

#[derive(Clone, Debug)]
pub(crate) struct LocationDraft {
    pub(crate) path: String,
    pub(crate) start_line: u64,
    pub(crate) end_line: u64,
    pub(crate) start_column: u64,
    pub(crate) end_column: u64,
}

#[derive(Clone)]
pub(crate) struct IssueDraft {
    pub(crate) kind: IngestionObstructionKind,
    pub(crate) severity: ObstructionSeverity,
    pub(crate) description: String,
    pub(crate) source_keys: BTreeSet<String>,
    pub(crate) paths: BTreeSet<String>,
    /// Named `extraction.capabilities` entries this limitation justifies.
    /// Empty when the limitation is not tied to one named capability.
    pub(crate) related_capabilities: BTreeSet<String>,
}

#[derive(Clone)]
pub(crate) struct V2IngestionObstructionDraft {
    pub(crate) kind: IngestionObstructionKind,
    pub(crate) severity: ObstructionSeverity,
    #[allow(dead_code)]
    pub(crate) description: String,
    pub(crate) source_keys: BTreeSet<String>,
    pub(crate) paths: BTreeSet<String>,
    pub(crate) related_capabilities: BTreeSet<String>,
    pub(crate) call_occurrence: Option<CallOccurrenceDraft>,
    pub(crate) latent_occurrence_count: Option<LatentOccurrenceCount>,
}

#[derive(Clone)]
pub(crate) struct CallOccurrenceDraft {
    pub(crate) call_kind: CallKind,
    pub(crate) reason: CallObstructionReason,
    pub(crate) span: LocationDraft,
    pub(crate) extractor_id: &'static str,
}

/// Whether a location falls within a base-comparison's changed-line set.
/// Only ever consulted while building the change family's own
/// `changed_by`/`contains` relations: the result must never be folded into
/// a target artifact's own attributes, since that would make the same-ID
/// artifact's canonical body depend on `base_revision`.
fn intersects_changed(changed_lines: &BTreeSet<u64>, location: &LocationDraft) -> bool {
    changed_lines
        .range(location.start_line..=location.end_line)
        .next()
        .is_some()
}

fn file_artifact_draft(file: &git::SnapshotFile) -> ArtifactDraft {
    let mut attributes = Map::new();
    attributes.insert("tracked_by_git".to_owned(), Value::Bool(true));
    ArtifactDraft {
        key: format!("file:{}", file.path),
        id_kind: "file",
        kind: "file",
        label: file.path.clone(),
        language: language_for_path(&file.path),
        location: Some(LocationDraft {
            path: file.path.clone(),
            start_line: 1,
            end_line: file.line_count().max(1),
            start_column: 1,
            end_column: 1,
        }),
        content_hash: Some(file.content_hash.clone()),
        attributes,
        source_path: Some(file.path.clone()),
        extraction_method: "reviewgraphen.ingest.git.snapshot.v1",
    }
}

fn language_for_path(path: &str) -> Option<&'static str> {
    if path.ends_with(".rs") {
        Some("rust")
    } else if path.ends_with(".toml") {
        Some("toml")
    } else {
        None
    }
}

/// The accumulated typed intermediate every adapter (git, rust, cargo)
/// contributes to before [`lift`] builds the validated core types from it.
struct LiftInputs {
    drafts: Vec<ArtifactDraft>,
    relation_drafts: Vec<RelationDraft>,
    rust_anchor_drafts: BTreeMap<String, rg_core::RustSymbolAnchorV1>,
    issues: Vec<IssueDraft>,
    adapter_reports: Vec<AdapterReport>,
    capabilities: BTreeMap<String, CapabilityState>,
    capability_sources: BTreeMap<String, BTreeSet<String>>,
}

/// Native `reviewgraphen.program_space.input.v3` producer: every accepted
/// artifact, relation, capability, and limitation is built once, directly as
/// a validated core type, from the same [`ArtifactDraft`]/[`RelationDraft`]/
/// [`IssueDraft`] typed intermediate the [`ExtractionReport`] is also built
/// from. There is no intermediate v1 JSON document and no migration step.
fn lift(
    snapshot: &git::GitSnapshot,
    identities: &SnapshotIdentities,
    inputs: LiftInputs,
) -> Result<IngestResult, IngestError> {
    let LiftInputs {
        drafts,
        relation_drafts,
        rust_anchor_drafts,
        issues,
        mut adapter_reports,
        mut capabilities,
        capability_sources,
    } = inputs;
    capabilities
        .entry("changed_structure".to_owned())
        .or_insert(CapabilityState::Complete);

    let mut id_by_key = BTreeMap::<String, StableId>::new();
    let source_for_path = snapshot.source_by_path();

    let mut artifacts = Vec::<rg_core::Artifact>::new();
    for draft in drafts {
        if id_by_key.contains_key(&draft.key) {
            // Repeated syntactic imports and equivalent metadata references
            // denote the same accepted ProgramSpace fact. The relation facts
            // retain each source occurrence; do not manufacture an ID clash.
            continue;
        }
        let id = derived_id(
            draft.id_kind,
            [
                (
                    "snapshot",
                    Value::String(identities.snapshot_id.to_string()),
                ),
                ("key", Value::String(draft.key.clone())),
            ],
        )?;
        id_by_key.insert(draft.key.clone(), id.clone());
        let provenance = provenance(
            snapshot,
            draft.source_path.as_deref(),
            draft.extraction_method,
            source_for_path.get(draft.source_path.as_deref().unwrap_or_default()),
        )?;
        let location = draft.location.map(core_location).transpose()?;
        artifacts.push(rg_core::Artifact::new(
            id,
            draft.kind,
            draft.label,
            draft.language.map(ToOwned::to_owned),
            location,
            draft.content_hash,
            core_attributes(draft.attributes),
            provenance,
        )?);
    }

    let mut relation_by_id = BTreeMap::<StableId, rg_core::Relation>::new();
    let mut relation_target_order = BTreeMap::<StableId, Vec<StableId>>::new();
    for draft in relation_drafts {
        let Some(source_id) = id_by_key.get(&draft.source_key) else {
            return Err(IngestError::AdapterOutput(format!(
                "relation `{}` has unknown source key `{}`",
                draft.kind, draft.source_key
            )));
        };
        let mut target_ids = BTreeSet::new();
        let mut ordered_target_ids = Vec::new();
        for target_key in &draft.target_keys {
            let Some(target_id) = id_by_key.get(target_key) else {
                return Err(IngestError::AdapterOutput(format!(
                    "relation `{}` has unknown target key `{target_key}`",
                    draft.kind
                )));
            };
            if !target_ids.insert(target_id.clone()) {
                return Err(IngestError::AdapterOutput(format!(
                    "relation `{}` repeats target key `{target_key}`",
                    draft.kind
                )));
            }
            ordered_target_ids.push(target_id.clone());
        }
        let relation_id = derived_id(
            "relation",
            [
                (
                    "snapshot",
                    Value::String(identities.snapshot_id.to_string()),
                ),
                ("kind", Value::String(draft.kind.to_owned())),
                ("source", Value::String(source_id.to_string())),
                (
                    "targets",
                    Value::Array(
                        ordered_target_ids
                            .iter()
                            .map(|id| Value::String(id.to_string()))
                            .collect(),
                    ),
                ),
                ("attributes", Value::Object(draft.attributes.clone())),
            ],
        )?;
        let provenance = provenance(
            snapshot,
            draft.source_path.as_deref(),
            draft.extraction_method,
            source_for_path.get(draft.source_path.as_deref().unwrap_or_default()),
        )?;
        let relation = rg_core::Relation::new(
            relation_id.clone(),
            draft.kind,
            source_id.clone(),
            target_ids,
            true,
            core_attributes(draft.attributes),
            provenance,
        )?;
        match relation_by_id.get(&relation_id) {
            Some(existing) if existing != &relation => {
                return Err(IngestError::Core(DomainError::IdCollision {
                    id: relation_id,
                }));
            }
            Some(_) => {}
            None => {
                relation_target_order.insert(relation_id.clone(), ordered_target_ids);
                relation_by_id.insert(relation_id, relation);
            }
        }
    }

    let mut limitation_by_id = BTreeMap::<StableId, rg_core::Limitation>::new();
    let mut obstruction_by_id = BTreeMap::<StableId, IngestionObstruction>::new();
    for issue in issues {
        let source_ids = resolve_known_source_ids(
            &issue.source_keys,
            &id_by_key,
            &identities.snapshot_id,
            |key| {
                IngestError::AdapterOutput(format!(
                    "limitation `{:?}` has unknown source key `{key}`",
                    issue.kind
                ))
            },
        )?;
        let source_values = Value::Array(
            source_ids
                .iter()
                .map(|value| Value::String(value.to_string()))
                .collect(),
        );
        let path_values = Value::Array(issue.paths.iter().cloned().map(Value::String).collect());
        let capability_values = Value::Array(
            issue
                .related_capabilities
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        );
        let id = derived_id(
            "limitation",
            [
                (
                    "snapshot",
                    Value::String(identities.snapshot_id.to_string()),
                ),
                ("kind", Value::String(format!("{:?}", issue.kind))),
                ("description", Value::String(issue.description.clone())),
                ("sources", source_values),
                ("paths", path_values),
                ("related_capabilities", capability_values),
            ],
        )?;
        if !limitation_by_id.contains_key(&id) {
            limitation_by_id.insert(
                id.clone(),
                rg_core::Limitation::new(
                    id.clone(),
                    issue.kind.as_core(),
                    issue.description.clone(),
                    issue.severity.as_core(),
                    source_ids.clone(),
                    issue.related_capabilities.clone(),
                )?,
            );
            obstruction_by_id.insert(
                id.clone(),
                IngestionObstruction {
                    id,
                    kind: issue.kind,
                    severity: issue.severity,
                    description: issue.description,
                    source_ids,
                    paths: issue.paths,
                },
            );
        }
    }

    let no_declared_sources = BTreeSet::new();
    let mut capability_declarations = BTreeMap::<String, rg_core::CapabilityDeclaration>::new();
    for (name, state) in &capabilities {
        let source_ids = resolve_known_source_ids(
            capability_sources.get(name).unwrap_or(&no_declared_sources),
            &id_by_key,
            &identities.snapshot_id,
            |key| {
                IngestError::AdapterOutput(format!(
                    "capability `{name}` has unknown source key `{key}`"
                ))
            },
        )?;
        capability_declarations.insert(
            name.clone(),
            rg_core::CapabilityDeclaration::new(state.as_core(), source_ids)?,
        );
    }

    adapter_reports.sort_by(|left, right| left.id.cmp(&right.id));
    let core_adapters = adapter_reports
        .iter()
        .map(|adapter| {
            rg_core::AdapterDescriptor::new(
                adapter.id.clone(),
                adapter.version.clone(),
                adapter.status.as_core(),
                adapter.parsed,
                adapter.total,
            )
        })
        .collect::<rg_core::Result<Vec<_>>>()?;

    let extraction = rg_core::Extraction::new(
        identities.adapter_set_hash.clone(),
        core_adapters,
        capability_declarations,
        limitation_by_id.into_values().collect(),
    )?;

    let source = rg_core::SourceRef::new(
        "git",
        snapshot.repository_identity.clone(),
        Some(snapshot.target_revision.clone()),
        Some(snapshot.tree_hash.clone()),
        Some("tree".to_owned()),
    )?;
    let repository = rg_core::RepositoryDescriptor {
        id: identities.repository_id.clone(),
        name: snapshot.repository_name.clone(),
        root: Some(snapshot.repository_root.clone()),
        uri: None,
    };
    let snapshot_descriptor = rg_core::SnapshotDescriptor {
        id: identities.snapshot_id.clone(),
        base_revision: snapshot.base_revision.clone(),
        target_revision: snapshot.target_revision.clone(),
        tree_hash: snapshot.tree_hash.clone(),
        dirty: false,
        created_at: None,
    };
    let profile = rg_core::ProfileDescriptor {
        id: snapshot.config.profile_id.clone(),
        version: snapshot.config.profile_version.clone(),
        rule_set_hash: snapshot.config.rule_set_hash.clone(),
        policy_version: snapshot.config.policy_version.clone(),
    };

    let program_space = rg_core::ProgramSpaceBuilder::new(
        source,
        repository,
        snapshot_descriptor,
        profile,
        extraction,
    )?
    .with_artifacts(artifacts)
    .with_relations(relation_by_id.into_values())
    .with_incremental_facts(rg_core::IncrementalFactsV1::new_with_relation_target_order(
        rg_core::GitRevisionClosureV1::new(
            snapshot.base_revision.clone(),
            snapshot.base_tree_hash.clone(),
            snapshot.target_revision.clone(),
            snapshot.tree_hash.clone(),
        )?,
        SYN_VERSION,
        rust_anchor_drafts
            .into_iter()
            .map(|(key, anchor)| {
                id_by_key
                    .get(&key)
                    .cloned()
                    .map(|id| (id, anchor))
                    .ok_or_else(|| {
                        IngestError::AdapterOutput(format!(
                            "Rust anchor has unknown artifact key `{key}`"
                        ))
                    })
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?,
        relation_target_order,
    )?)
    .build()?;

    let mut obstructions = obstruction_by_id.into_values().collect::<Vec<_>>();
    obstructions.sort_by(|left, right| left.id.cmp(&right.id));

    let extraction_report = ExtractionReport {
        schema: EXTRACTION_REPORT_SCHEMA,
        snapshot_id: identities.snapshot_id.clone(),
        target_revision: snapshot.target_revision.clone(),
        adapter_set_hash: identities.adapter_set_hash.clone(),
        adapters: adapter_reports,
        capabilities,
        obstructions,
    };
    Ok(IngestResult {
        program_space,
        extraction_report,
    })
}

fn core_location(location: LocationDraft) -> Result<rg_core::Location, IngestError> {
    rg_core::Location::new(
        location.path,
        Some(location.start_line),
        Some(location.end_line),
        Some(location.start_column),
        Some(location.end_column),
        None,
    )
    .map_err(IngestError::from)
}

fn core_attributes(attributes: Map<String, Value>) -> BTreeMap<String, Value> {
    attributes.into_iter().collect()
}

fn provenance(
    snapshot: &git::GitSnapshot,
    source_path: Option<&str>,
    extraction_method: &str,
    content_hash: Option<&ContentHash>,
) -> Result<rg_core::Provenance, IngestError> {
    let source = rg_core::SourceRef::new(
        "git",
        snapshot.repository_identity.clone(),
        Some(snapshot.target_revision.clone()),
        Some(content_hash.unwrap_or(&snapshot.tree_hash).clone()),
        source_path.map(ToOwned::to_owned),
    )?;
    rg_core::Provenance::accepted_deterministic(
        source,
        extraction_method,
        Some(env!("CARGO_PKG_VERSION").to_owned()),
        Some(1.0),
    )
    .map_err(IngestError::from)
}

/// Resolves a limitation's or capability declaration's own `source_keys`
/// into `StableId`s, exactly as strictly as a relation draft's `source_key`/
/// `target_keys` already are: an empty key set is the deliberate "grounded
/// in the whole snapshot" case (for example a symlink excluded before any
/// artifact existed for it) and falls back to `snapshot_id`, but a
/// *non-empty* key set that names even one key absent from `id_by_key`
/// means an adapter emitted a key nothing ever drafted -- silently dropping
/// just that key (the old `filter_map` behavior) would produce a
/// still-non-empty, still-"valid"-looking trace that quietly omits the
/// broken key instead of surfacing the bug.
fn resolve_known_source_ids(
    keys: &BTreeSet<String>,
    id_by_key: &BTreeMap<String, StableId>,
    snapshot_id: &StableId,
    unknown_key_error: impl Fn(&str) -> IngestError,
) -> Result<BTreeSet<StableId>, IngestError> {
    if keys.is_empty() {
        return Ok(BTreeSet::from([snapshot_id.clone()]));
    }
    keys.iter()
        .map(|key| {
            id_by_key
                .get(key)
                .cloned()
                .ok_or_else(|| unknown_key_error(key))
        })
        .collect()
}

fn derived_id<const N: usize>(
    kind: &str,
    bindings: [(&'static str, Value); N],
) -> Result<StableId, IngestError> {
    let bindings = bindings
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect::<BTreeMap<_, _>>();
    StableId::derived(kind, &bindings).map_err(IngestError::from)
}

fn path_from_utf8(value: &Path) -> Result<String, IngestError> {
    value.to_str().map(ToOwned::to_owned).ok_or_else(|| {
        IngestError::InvalidRequest(format!("path `{}` is not UTF-8", value.display()))
    })
}

/// Exercises `lift()`'s limitation/capability source-key resolution
/// directly against the typed adapter intermediate (`ArtifactDraft`,
/// `IssueDraft`, `LiftInputs`), independent of any real `git`/`cargo`
/// adapter. A `RelationDraft`'s `source_key`/`target_keys` were already
/// fail-closed before this fix (an unknown key is always a hard
/// `IngestError::AdapterOutput`, never silently dropped); these tests prove
/// `IssueDraft.source_keys` (-> `Limitation.source_ids`) and
/// `capability_sources` (-> `CapabilityDeclaration.source_ids`) now behave
/// the same way, while the deliberate empty-key whole-snapshot fallback and
/// ordinary all-known-key resolution both still work unchanged. There is no
/// analogous case for an `ArtifactDraft` itself: it never resolves another
/// draft's key, only its own.
#[cfg(test)]
mod source_key_resolution_tests {
    use super::*;

    fn snapshot() -> git::GitSnapshot {
        git::GitSnapshot {
            repository_root: "/repo".to_owned(),
            repository_identity: "reviewgraphen.test/source-key-resolution".to_owned(),
            repository_name: "repo".to_owned(),
            base_revision: "0".repeat(40),
            base_tree_hash: ContentHash::parse(format!("git:{}", "3".repeat(40)))
                .expect("valid test base tree hash"),
            target_revision: "1".repeat(40),
            tree_hash: ContentHash::parse(format!("git:{}", "2".repeat(40)))
                .expect("valid test tree hash"),
            config: IngestConfig::default(),
            files: Vec::new(),
            changes: Vec::new(),
            issues: Vec::new(),
            adapter_reports: Vec::new(),
            capabilities: BTreeMap::new(),
            capability_sources: BTreeMap::new(),
            git_version: "git version 2.99.0".to_owned(),
            cargo_version: Ok("cargo 1.99.0".to_owned()),
            cargo_executable: Some(PathBuf::from("cargo")),
            staged_snapshot: None,
        }
    }

    fn identities_for(snapshot: &git::GitSnapshot) -> SnapshotIdentities {
        SnapshotIdentities::new(
            snapshot,
            &snapshot.config,
            "syn 0.0.0-test",
            "proc-macro2 0.0.0-test",
            "quote 0.0.0-test",
            &git::git_command_policy_fingerprint(),
            &git::cargo_resolver_policy_fingerprint(),
        )
        .expect("identities derive")
    }

    fn known_artifact_draft(key: &str) -> ArtifactDraft {
        ArtifactDraft {
            key: key.to_owned(),
            id_kind: "file",
            kind: "file",
            label: key.to_owned(),
            language: None,
            location: None,
            content_hash: None,
            attributes: Map::new(),
            source_path: None,
            extraction_method: "reviewgraphen.ingest.test.v1",
        }
    }

    fn base_inputs() -> LiftInputs {
        LiftInputs {
            drafts: vec![known_artifact_draft("file:known.rs")],
            relation_drafts: Vec::new(),
            rust_anchor_drafts: BTreeMap::new(),
            issues: Vec::new(),
            adapter_reports: vec![AdapterReport {
                id: "reviewgraphen.ingest.test".to_owned(),
                version: "1".to_owned(),
                status: AdapterStatus::Complete,
                parsed: Some(1),
                total: Some(1),
                excluded: Some(0),
                failed: Some(0),
            }],
            capabilities: BTreeMap::new(),
            capability_sources: BTreeMap::new(),
        }
    }

    fn test_issue(source_keys: BTreeSet<String>) -> IssueDraft {
        IssueDraft {
            kind: IngestionObstructionKind::RelationUnresolved,
            severity: ObstructionSeverity::Low,
            description: "test issue".to_owned(),
            source_keys,
            paths: BTreeSet::new(),
            related_capabilities: BTreeSet::new(),
        }
    }

    fn snapshot_with_rust_source() -> git::GitSnapshot {
        let mut snapshot = snapshot();
        let content = b"pub fn caller() { nowhere(); }\n".to_vec();
        snapshot.files = vec![git::SnapshotFile {
            path: "known.rs".to_owned(),
            content_hash: ContentHash::sha256(&content),
            content,
            changed_lines: BTreeSet::from([1]),
        }];
        snapshot
    }

    fn v2_occurrence_draft(
        reason: CallObstructionReason,
        start_column: u64,
    ) -> V2IngestionObstructionDraft {
        V2IngestionObstructionDraft {
            kind: IngestionObstructionKind::RelationUnresolved,
            severity: ObstructionSeverity::Medium,
            description: String::new(),
            source_keys: BTreeSet::from(["file:known.rs".to_owned()]),
            paths: BTreeSet::from(["known.rs".to_owned()]),
            related_capabilities: BTreeSet::from(["direct_calls".to_owned()]),
            call_occurrence: Some(CallOccurrenceDraft {
                call_kind: CallKind::Direct,
                reason,
                span: LocationDraft {
                    path: "known.rs".to_owned(),
                    start_line: 1,
                    end_line: 1,
                    start_column,
                    end_column: start_column + 8,
                },
                extractor_id: INGESTION_V2_PROJECTION_EXTRACTOR_ID,
            }),
            latent_occurrence_count: None,
        }
    }

    fn v2_global_draft() -> V2IngestionObstructionDraft {
        V2IngestionObstructionDraft {
            kind: IngestionObstructionKind::RelationUnresolved,
            severity: ObstructionSeverity::Info,
            description: String::new(),
            source_keys: BTreeSet::from(["file:known.rs".to_owned()]),
            paths: BTreeSet::from(["known.rs".to_owned()]),
            related_capabilities: BTreeSet::from(["direct_calls".to_owned()]),
            call_occurrence: None,
            latent_occurrence_count: Some(LatentOccurrenceCount::Unknown),
        }
    }

    #[test]
    fn reordered_v2_draft_vector_retains_occurrence_ids_and_canonical_bytes() {
        let snapshot = snapshot_with_rust_source();
        let identities = identities_for(&snapshot);
        let mut inputs = base_inputs();
        inputs.drafts = snapshot.files.iter().map(file_artifact_draft).collect();
        inputs
            .capabilities
            .insert("direct_calls".to_owned(), CapabilityState::Partial);
        inputs.capability_sources.insert(
            "direct_calls".to_owned(),
            BTreeSet::from(["file:known.rs".to_owned()]),
        );
        inputs.issues.push(IssueDraft {
            kind: IngestionObstructionKind::RelationUnresolved,
            severity: ObstructionSeverity::Info,
            description: "legacy direct_calls limitation".to_owned(),
            source_keys: BTreeSet::from(["file:known.rs".to_owned()]),
            paths: BTreeSet::from(["known.rs".to_owned()]),
            related_capabilities: BTreeSet::from(["direct_calls".to_owned()]),
        });
        let legacy = lift(&snapshot, &identities, inputs).expect("legacy lift succeeds");
        let legacy_index =
            V2LegacyValidationIndex::new(&legacy).expect("legacy validation index builds");
        let first = v2_occurrence_draft(CallObstructionReason::DirectTargetCountZero, 19);
        let second = v2_occurrence_draft(CallObstructionReason::DirectNonPath, 1);
        let forward = build_ingestion_report_v2(
            &snapshot,
            &identities,
            &legacy,
            &legacy_index,
            vec![first.clone(), second.clone(), v2_global_draft()],
        )
        .expect("forward v2 build succeeds");
        let reversed = build_ingestion_report_v2(
            &snapshot,
            &identities,
            &legacy,
            &legacy_index,
            vec![v2_global_draft(), second, first],
        )
        .expect("reversed v2 build succeeds");
        let ids = |report: &IngestionReportV2| {
            report
                .located_call_occurrences()
                .iter()
                .map(|item| item.id().clone())
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(ids(&forward), ids(&reversed));
        assert_eq!(
            forward.canonical_bytes().expect("forward bytes"),
            reversed.canonical_bytes().expect("reversed bytes")
        );
    }

    #[test]
    fn issue_with_only_unknown_source_keys_fails_closed() {
        let snapshot = snapshot();
        let identities = identities_for(&snapshot);
        let mut inputs = base_inputs();
        inputs
            .issues
            .push(test_issue(BTreeSet::from(["file:missing.rs".to_owned()])));

        let error =
            lift(&snapshot, &identities, inputs).expect_err("unknown source key must fail closed");
        assert!(
            matches!(error, IngestError::AdapterOutput(_)),
            "expected AdapterOutput, got {error:?}"
        );
    }

    #[test]
    fn issue_with_one_unknown_key_among_multiple_fails_closed() {
        let snapshot = snapshot();
        let identities = identities_for(&snapshot);
        let mut inputs = base_inputs();
        inputs.issues.push(test_issue(BTreeSet::from([
            "file:known.rs".to_owned(),
            "file:missing.rs".to_owned(),
        ])));

        let error = lift(&snapshot, &identities, inputs)
            .expect_err("a single unknown key among many must still fail closed");
        assert!(
            matches!(error, IngestError::AdapterOutput(_)),
            "expected AdapterOutput, got {error:?}"
        );
    }

    #[test]
    fn issue_with_empty_source_keys_still_falls_back_to_the_snapshot() {
        let snapshot = snapshot();
        let identities = identities_for(&snapshot);
        let mut inputs = base_inputs();
        inputs.issues.push(test_issue(BTreeSet::new()));

        let result = lift(&snapshot, &identities, inputs)
            .expect("an empty source-key set is the deliberate whole-snapshot fallback");
        let limitation = result
            .program_space
            .extraction()
            .limitations
            .first()
            .expect("one limitation");
        assert_eq!(
            limitation.source_ids,
            BTreeSet::from([identities.snapshot_id.clone()])
        );
    }

    #[test]
    fn issue_with_all_known_source_keys_resolves_normally() {
        let snapshot = snapshot();
        let identities = identities_for(&snapshot);
        let mut inputs = base_inputs();
        inputs
            .issues
            .push(test_issue(BTreeSet::from(["file:known.rs".to_owned()])));

        let result =
            lift(&snapshot, &identities, inputs).expect("a known source key must resolve normally");
        let limitation = result
            .program_space
            .extraction()
            .limitations
            .first()
            .expect("one limitation");
        assert_eq!(limitation.source_ids.len(), 1);
        assert_ne!(
            limitation.source_ids.iter().next(),
            Some(&identities.snapshot_id),
            "a real source key must resolve to its own ID, not the whole-snapshot fallback"
        );
    }

    #[test]
    fn capability_with_only_unknown_source_keys_fails_closed() {
        let snapshot = snapshot();
        let identities = identities_for(&snapshot);
        let mut inputs = base_inputs();
        inputs
            .capabilities
            .insert("ast".to_owned(), CapabilityState::Complete);
        inputs.capability_sources.insert(
            "ast".to_owned(),
            BTreeSet::from(["file:missing.rs".to_owned()]),
        );

        let error = lift(&snapshot, &identities, inputs)
            .expect_err("unknown capability source key must fail closed");
        assert!(
            matches!(error, IngestError::AdapterOutput(_)),
            "expected AdapterOutput, got {error:?}"
        );
    }

    #[test]
    fn capability_with_one_unknown_key_among_multiple_fails_closed() {
        let snapshot = snapshot();
        let identities = identities_for(&snapshot);
        let mut inputs = base_inputs();
        inputs
            .capabilities
            .insert("ast".to_owned(), CapabilityState::Complete);
        inputs.capability_sources.insert(
            "ast".to_owned(),
            BTreeSet::from(["file:known.rs".to_owned(), "file:missing.rs".to_owned()]),
        );

        let error = lift(&snapshot, &identities, inputs)
            .expect_err("a single unknown key among many must still fail closed");
        assert!(
            matches!(error, IngestError::AdapterOutput(_)),
            "expected AdapterOutput, got {error:?}"
        );
    }

    #[test]
    fn capability_with_no_declared_source_keys_falls_back_to_the_snapshot() {
        let snapshot = snapshot();
        let identities = identities_for(&snapshot);
        let mut inputs = base_inputs();
        inputs
            .capabilities
            .insert("ast".to_owned(), CapabilityState::Complete);
        // No `capability_sources` entry at all for `"ast"`.

        let result = lift(&snapshot, &identities, inputs)
            .expect("no declared source keys is the deliberate whole-snapshot fallback");
        let declaration = &result.program_space.extraction().capabilities["ast"];
        assert_eq!(
            declaration.source_ids,
            BTreeSet::from([identities.snapshot_id.clone()])
        );
    }

    #[test]
    fn capability_with_all_known_source_keys_resolves_normally() {
        let snapshot = snapshot();
        let identities = identities_for(&snapshot);
        let mut inputs = base_inputs();
        inputs
            .capabilities
            .insert("ast".to_owned(), CapabilityState::Complete);
        inputs.capability_sources.insert(
            "ast".to_owned(),
            BTreeSet::from(["file:known.rs".to_owned()]),
        );

        let result = lift(&snapshot, &identities, inputs)
            .expect("known capability source keys must resolve normally");
        let declaration = &result.program_space.extraction().capabilities["ast"];
        assert_eq!(declaration.source_ids.len(), 1);
        assert_ne!(
            declaration.source_ids.iter().next(),
            Some(&identities.snapshot_id),
            "a real source key must resolve to its own ID, not the whole-snapshot fallback"
        );
    }

    #[test]
    fn relation_endpoint_order_changes_accepted_identity_and_body() {
        let snapshot = snapshot();
        let identities = identities_for(&snapshot);
        let lift_with_order = |target_keys: Vec<String>| {
            let mut inputs = base_inputs();
            inputs.drafts.extend([
                known_artifact_draft("file:a.rs"),
                known_artifact_draft("file:b.rs"),
            ]);
            inputs.relation_drafts.push(RelationDraft {
                kind: "ordered_test",
                source_key: "file:known.rs".to_owned(),
                target_keys,
                attributes: Map::new(),
                source_path: None,
                extraction_method: "reviewgraphen.ingest.test.v1",
            });
            lift(&snapshot, &identities, inputs).expect("ordered relation lifts")
        };

        let forward = lift_with_order(vec!["file:a.rs".to_owned(), "file:b.rs".to_owned()]);
        let repeated = lift_with_order(vec!["file:a.rs".to_owned(), "file:b.rs".to_owned()]);
        let reversed = lift_with_order(vec!["file:b.rs".to_owned(), "file:a.rs".to_owned()]);
        let forward_relation = &forward.program_space.relations()[0];
        let reversed_relation = &reversed.program_space.relations()[0];

        assert_eq!(
            canonical_json(&forward.program_space).unwrap(),
            canonical_json(&repeated.program_space).unwrap(),
            "the same declared endpoint order is deterministic"
        );
        assert_ne!(forward_relation.id, reversed_relation.id);
        assert_ne!(
            canonical_json(forward_relation).unwrap(),
            canonical_json(reversed_relation).unwrap(),
            "swapping a hyperedge sequence must change the accepted relation body"
        );
        assert_eq!(
            forward_relation.target_ids, reversed_relation.target_ids,
            "the unordered membership domain remains separate from endpoint order"
        );
    }
}

/// Exercises `SnapshotIdentities::new`'s `adapter_set_hash` derivation
/// directly against the typed `git::GitSnapshot`/`IngestConfig`
/// intermediate: it must actually be sensitive to the real `git`/`cargo`/
/// `syn`/`proc-macro2` tool versions (including a *structured* `cargo`
/// unavailable reason, not an undifferentiated `null`) and to every
/// `IngestLimits` field, stable across repeated derivation from the same
/// inputs (determinism) and across a snapshot that only differs in
/// host-independent fields (clone-path independence, covered end-to-end in
/// `tests/m2.rs`), and untouched by profile/policy identity that no
/// adapter ever consults while parsing. `syn_version`/`proc_macro2_version`
/// are passed to `hash_for` rather than read from the real
/// `SYN_VERSION`/`PROC_MACRO2_VERSION` constants, so their sensitivity is
/// directly testable the same way as the runtime `git`/`cargo` fields.
#[cfg(test)]
mod adapter_set_hash_tests {
    use super::*;

    const DEFAULT_SYN_VERSION: &str = "syn 0.0.0-test";
    const DEFAULT_PROC_MACRO2_VERSION: &str = "proc-macro2 0.0.0-test";
    const DEFAULT_QUOTE_VERSION: &str = "quote 0.0.0-test";

    fn snapshot(
        config: IngestConfig,
        git_version: &str,
        cargo_version: Result<&str, git::CargoToolFailureKind>,
    ) -> git::GitSnapshot {
        git::GitSnapshot {
            repository_root: "/repo".to_owned(),
            repository_identity: "reviewgraphen.test/adapter-set-hash".to_owned(),
            repository_name: "repo".to_owned(),
            base_revision: "0".repeat(40),
            base_tree_hash: ContentHash::parse(format!("git:{}", "3".repeat(40)))
                .expect("valid test base tree hash"),
            target_revision: "1".repeat(40),
            tree_hash: ContentHash::parse(format!("git:{}", "2".repeat(40)))
                .expect("valid test tree hash"),
            config,
            files: Vec::new(),
            changes: Vec::new(),
            issues: Vec::new(),
            adapter_reports: Vec::new(),
            capabilities: BTreeMap::new(),
            capability_sources: BTreeMap::new(),
            git_version: git_version.to_owned(),
            cargo_executable: cargo_version.is_ok().then(|| PathBuf::from("cargo")),
            cargo_version: cargo_version.map(ToOwned::to_owned).map_err(|kind| {
                git::CargoToolFailure {
                    kind,
                    diagnostic: "test diagnostic".to_owned(),
                }
            }),
            staged_snapshot: None,
        }
    }

    fn default_snapshot() -> git::GitSnapshot {
        snapshot(
            IngestConfig::default(),
            "git version 2.43.0",
            Ok("cargo 1.75.0"),
        )
    }

    fn hash_for(snapshot: &git::GitSnapshot) -> ContentHash {
        hash_for_tool_versions(
            snapshot,
            DEFAULT_SYN_VERSION,
            DEFAULT_PROC_MACRO2_VERSION,
            DEFAULT_QUOTE_VERSION,
        )
    }

    fn hash_for_tool_versions(
        snapshot: &git::GitSnapshot,
        syn_version: &str,
        proc_macro2_version: &str,
        quote_version: &str,
    ) -> ContentHash {
        hash_for_all(
            snapshot,
            syn_version,
            proc_macro2_version,
            quote_version,
            &git::git_command_policy_fingerprint(),
            &git::cargo_resolver_policy_fingerprint(),
        )
    }

    fn hash_for_all(
        snapshot: &git::GitSnapshot,
        syn_version: &str,
        proc_macro2_version: &str,
        quote_version: &str,
        git_command_policy: &Value,
        cargo_resolver_policy: &Value,
    ) -> ContentHash {
        SnapshotIdentities::new(
            snapshot,
            &snapshot.config,
            syn_version,
            proc_macro2_version,
            quote_version,
            git_command_policy,
            cargo_resolver_policy,
        )
        .expect("identities derive")
        .adapter_set_hash
    }

    #[test]
    fn identical_inputs_produce_an_identical_adapter_set_hash() {
        assert_eq!(hash_for(&default_snapshot()), hash_for(&default_snapshot()));
    }

    #[test]
    fn a_different_git_version_changes_the_adapter_set_hash() {
        let changed = snapshot(
            IngestConfig::default(),
            "git version 9.9.9",
            Ok("cargo 1.75.0"),
        );
        assert_ne!(hash_for(&default_snapshot()), hash_for(&changed));
    }

    #[test]
    fn a_different_cargo_version_changes_the_adapter_set_hash() {
        let changed = snapshot(
            IngestConfig::default(),
            "git version 2.43.0",
            Ok("cargo 9.9.9"),
        );
        assert_ne!(hash_for(&default_snapshot()), hash_for(&changed));
    }

    #[test]
    fn an_unavailable_cargo_version_changes_the_adapter_set_hash_and_never_panics() {
        let changed = snapshot(
            IngestConfig::default(),
            "git version 2.43.0",
            Err(git::CargoToolFailureKind::Unavailable),
        );
        assert_ne!(hash_for(&default_snapshot()), hash_for(&changed));
    }

    #[test]
    fn a_different_unavailable_cargo_failure_kind_changes_the_adapter_set_hash() {
        // The failure *kind* -- not just availability, and never a raw
        // diagnostic string -- is part of the fingerprint: a spawn failure
        // and a non-UTF-8-output failure are both "unavailable", but they
        // are not the same fact.
        let spawn_failure = snapshot(
            IngestConfig::default(),
            "git version 2.43.0",
            Err(git::CargoToolFailureKind::Unavailable),
        );
        let output_failure = snapshot(
            IngestConfig::default(),
            "git version 2.43.0",
            Err(git::CargoToolFailureKind::NotUtf8),
        );
        assert_ne!(hash_for(&spawn_failure), hash_for(&output_failure));
    }

    #[test]
    fn the_same_cargo_failure_kind_hashes_identically_even_with_a_different_diagnostic() {
        // Two runs of the identical input, staged into two different
        // `TempDir`s, can carry two different (already-redacted) diagnostic
        // strings for the exact same underlying failure -- `adapter_set_hash`
        // must not care, since only `kind` is folded into it.
        let first = snapshot(
            IngestConfig::default(),
            "git version 2.43.0",
            Err(git::CargoToolFailureKind::NonSuccessExit),
        );
        let mut second = snapshot(
            IngestConfig::default(),
            "git version 2.43.0",
            Err(git::CargoToolFailureKind::NonSuccessExit),
        );
        second.cargo_version = Err(git::CargoToolFailure {
            kind: git::CargoToolFailureKind::NonSuccessExit,
            diagnostic: "a completely different diagnostic string".to_owned(),
        });
        assert_eq!(hash_for(&first), hash_for(&second));
    }

    #[test]
    fn a_different_max_files_limit_changes_the_adapter_set_hash() {
        let mut config = IngestConfig::default();
        config.limits.max_files += 1;
        let changed = snapshot(config, "git version 2.43.0", Ok("cargo 1.75.0"));
        assert_ne!(hash_for(&default_snapshot()), hash_for(&changed));
    }

    #[test]
    fn a_different_max_file_bytes_limit_changes_the_adapter_set_hash() {
        let mut config = IngestConfig::default();
        config.limits.max_file_bytes += 1;
        let changed = snapshot(config, "git version 2.43.0", Ok("cargo 1.75.0"));
        assert_ne!(hash_for(&default_snapshot()), hash_for(&changed));
    }

    #[test]
    fn a_different_syn_version_changes_the_adapter_set_hash() {
        let snapshot = default_snapshot();
        assert_ne!(
            hash_for_tool_versions(
                &snapshot,
                DEFAULT_SYN_VERSION,
                DEFAULT_PROC_MACRO2_VERSION,
                DEFAULT_QUOTE_VERSION,
            ),
            hash_for_tool_versions(
                &snapshot,
                "syn 9.9.9-test",
                DEFAULT_PROC_MACRO2_VERSION,
                DEFAULT_QUOTE_VERSION,
            )
        );
    }

    #[test]
    fn a_different_proc_macro2_version_changes_the_adapter_set_hash() {
        let snapshot = default_snapshot();
        assert_ne!(
            hash_for_tool_versions(
                &snapshot,
                DEFAULT_SYN_VERSION,
                DEFAULT_PROC_MACRO2_VERSION,
                DEFAULT_QUOTE_VERSION,
            ),
            hash_for_tool_versions(
                &snapshot,
                DEFAULT_SYN_VERSION,
                "proc-macro2 9.9.9-test",
                DEFAULT_QUOTE_VERSION,
            )
        );
    }

    #[test]
    fn a_different_quote_version_changes_the_adapter_set_hash() {
        let snapshot = default_snapshot();
        assert_ne!(
            hash_for_tool_versions(
                &snapshot,
                DEFAULT_SYN_VERSION,
                DEFAULT_PROC_MACRO2_VERSION,
                DEFAULT_QUOTE_VERSION,
            ),
            hash_for_tool_versions(
                &snapshot,
                DEFAULT_SYN_VERSION,
                DEFAULT_PROC_MACRO2_VERSION,
                "quote 9.9.9-test",
            )
        );
    }

    #[test]
    fn a_different_git_command_policy_changes_the_adapter_set_hash() {
        let snapshot = default_snapshot();
        assert_ne!(
            hash_for_all(
                &snapshot,
                DEFAULT_SYN_VERSION,
                DEFAULT_PROC_MACRO2_VERSION,
                DEFAULT_QUOTE_VERSION,
                &git::git_command_policy_fingerprint(),
                &git::cargo_resolver_policy_fingerprint(),
            ),
            hash_for_all(
                &snapshot,
                DEFAULT_SYN_VERSION,
                DEFAULT_PROC_MACRO2_VERSION,
                DEFAULT_QUOTE_VERSION,
                &json!({ "version": "9.9.9-test" }),
                &git::cargo_resolver_policy_fingerprint(),
            ),
            "a different deterministic Git command policy must change the fingerprint, \
             exactly like a different tool version does"
        );
    }

    #[test]
    fn a_different_cargo_resolver_policy_changes_the_adapter_set_hash() {
        let snapshot = default_snapshot();
        assert_ne!(
            hash_for_all(
                &snapshot,
                DEFAULT_SYN_VERSION,
                DEFAULT_PROC_MACRO2_VERSION,
                DEFAULT_QUOTE_VERSION,
                &git::git_command_policy_fingerprint(),
                &git::cargo_resolver_policy_fingerprint(),
            ),
            hash_for_all(
                &snapshot,
                DEFAULT_SYN_VERSION,
                DEFAULT_PROC_MACRO2_VERSION,
                DEFAULT_QUOTE_VERSION,
                &git::git_command_policy_fingerprint(),
                &json!({ "version": "9.9.9-test" }),
            ),
            "a different deterministic Cargo tool admission policy must change the \
             fingerprint, exactly like a different tool version does"
        );
    }

    #[test]
    fn profile_and_policy_identity_do_not_affect_the_adapter_set_hash() {
        let config = IngestConfig {
            profile_id: "a-different-profile".to_owned(),
            profile_version: "2".to_owned(),
            policy_version: "a-different-policy@2".to_owned(),
            ..IngestConfig::default()
        };
        let changed = snapshot(config, "git version 2.43.0", Ok("cargo 1.75.0"));
        assert_eq!(hash_for(&default_snapshot()), hash_for(&changed));
    }

    #[test]
    fn the_syn_version_constant_is_the_real_workspace_pin_not_a_placeholder() {
        // `Cargo.toml` pins `syn = "=2.0.119"`; this must match the value
        // `build.rs` actually extracted from the locked dependency graph,
        // not an empty string or a value neither file agrees with. This
        // assertion is expected to need updating whenever that pin moves.
        assert_eq!(SYN_VERSION, "2.0.119");
    }

    #[test]
    fn the_proc_macro2_version_constant_is_the_real_workspace_pin_not_a_placeholder() {
        // `Cargo.lock` locks exactly one `proc-macro2` package, currently
        // `1.0.107`; this must match the value `build.rs` actually
        // extracted, not an empty string or a value neither file agrees
        // with. This assertion is expected to need updating whenever that
        // resolution moves.
        assert_eq!(PROC_MACRO2_VERSION, "1.0.107");
    }

    #[test]
    fn the_quote_version_constant_is_the_real_workspace_pin_not_a_placeholder() {
        assert_eq!(QUOTE_VERSION, "1.0.47");
    }
}

/// Exercises the full `extract_cargo_metadata` -> `lift` pipeline `ingest()`
/// itself uses (bypassing the real `git`/`cargo` subprocesses, exactly like
/// `source_key_resolution_tests`) to prove `CargoToolFailure`'s identity
/// contract end to end: a `cargo --version` failure's `adapter_set_hash`,
/// its `cargo_metadata_unavailable` limitation ID, and the run's full
/// `canonical_output()` bytes must all agree across two runs of the
/// identical input that merely staged into two different `TempDir`s (and
/// so, for a real failure that can echo its cwd in stderr, could otherwise
/// carry two different raw diagnostics), and even across two runs whose raw
/// diagnostics are deliberately, unredactably different for the same
/// `kind` -- because `diagnostic` is never folded into `description`, the
/// limitation's identity input, at all -- and must all disagree once the
/// failure *kind* genuinely differs.
#[cfg(test)]
mod cargo_failure_identity_tests {
    use super::*;

    fn base_snapshot(cargo_version: Result<String, git::CargoToolFailure>) -> git::GitSnapshot {
        // `ProgramSpace` requires at least one accepted artifact, so this
        // fixture carries one otherwise-inert tracked file -- identical
        // across every snapshot this module builds -- purely to satisfy
        // that precondition; it plays no role in what's under test here.
        let content = b"[package]\n".to_vec();
        let file = git::SnapshotFile {
            path: "Cargo.toml".to_owned(),
            content_hash: ContentHash::sha256(&content),
            content,
            changed_lines: BTreeSet::new(),
        };
        git::GitSnapshot {
            repository_root: "/repo".to_owned(),
            repository_identity: "reviewgraphen.test/cargo-failure-identity".to_owned(),
            repository_name: "repo".to_owned(),
            base_revision: "0".repeat(40),
            base_tree_hash: ContentHash::parse(format!("git:{}", "3".repeat(40)))
                .expect("valid test base tree hash"),
            target_revision: "1".repeat(40),
            tree_hash: ContentHash::parse(format!("git:{}", "2".repeat(40)))
                .expect("valid test tree hash"),
            config: IngestConfig::default(),
            files: vec![file],
            changes: Vec::new(),
            issues: Vec::new(),
            adapter_reports: Vec::new(),
            capabilities: BTreeMap::new(),
            capability_sources: BTreeMap::new(),
            git_version: "git version 2.43.0".to_owned(),
            cargo_executable: cargo_version.is_ok().then(|| PathBuf::from("cargo")),
            cargo_version,
            staged_snapshot: None,
        }
    }

    fn failing_snapshot(kind: git::CargoToolFailureKind, diagnostic: &str) -> git::GitSnapshot {
        base_snapshot(Err(git::CargoToolFailure {
            kind,
            diagnostic: diagnostic.to_owned(),
        }))
    }

    fn identities_for(snapshot: &git::GitSnapshot) -> SnapshotIdentities {
        SnapshotIdentities::new(
            snapshot,
            &snapshot.config,
            "syn 0.0.0-test",
            "proc-macro2 0.0.0-test",
            "quote 0.0.0-test",
            &git::git_command_policy_fingerprint(),
            &git::cargo_resolver_policy_fingerprint(),
        )
        .expect("identities derive")
    }

    /// Runs exactly the `extract_cargo_metadata` -> `lift` sequence
    /// `ingest()` runs, without a real Git/Cargo subprocess.
    fn lift_cargo_failure(
        snapshot: &git::GitSnapshot,
        identities: &SnapshotIdentities,
    ) -> IngestResult {
        let cargo = git::extract_cargo_metadata(snapshot, &identities.snapshot_id);
        let drafts = snapshot.files.iter().map(file_artifact_draft).collect();
        lift(
            snapshot,
            identities,
            LiftInputs {
                drafts,
                relation_drafts: Vec::new(),
                rust_anchor_drafts: BTreeMap::new(),
                issues: cargo.issues,
                adapter_reports: vec![cargo.adapter_report],
                capabilities: cargo.capabilities,
                capability_sources: cargo.capability_sources,
            },
        )
        .expect("a cargo-version-failure snapshot always lifts successfully")
    }

    fn cargo_metadata_unavailable_limitation(result: &IngestResult) -> &IngestionObstruction {
        result
            .extraction_report
            .obstructions
            .iter()
            .find(|obstruction| {
                obstruction.kind == IngestionObstructionKind::CargoMetadataUnavailable
            })
            .expect("a cargo --version failure always records this obstruction")
    }

    /// Builds a `CargoToolFailure` the way `load_snapshot` really does
    /// (`git::cargo_tool_failure`), from a simulated `cargo --version`
    /// non-success exit whose stderr echoes `staged_root` -- exactly what a
    /// real `rust-toolchain.toml`-driven toolchain-resolution error would
    /// do -- so the redaction this test relies on is the real production
    /// code path, not a hand-simulated stand-in.
    fn non_success_exit_failure(staged_root: &Path) -> git::CargoToolFailure {
        let error = IngestError::CommandFailed {
            command: "cargo --version",
            status: 101,
            stderr: format!(
                "error: could not find `rust-toolchain.toml` in `{}` or any parent directory",
                staged_root.display()
            ),
        };
        git::cargo_tool_failure(&error, staged_root)
    }

    #[test]
    fn the_same_cargo_version_failure_from_two_different_staged_tempdirs_hashes_and_identifies_identically()
     {
        // Two separate `TempDir`s, exactly like two separate `ingest()`
        // runs of the identical input would stage into (see
        // `git::stage_files`): the only thing that can differ between the
        // two simulated `cargo --version` failures below is this random
        // path, which `git::cargo_tool_failure` must redact away.
        let first_root = tempfile::tempdir().expect("first staged root");
        let second_root = tempfile::tempdir().expect("second staged root");
        let first_failure = non_success_exit_failure(first_root.path());
        let second_failure = non_success_exit_failure(second_root.path());
        assert_eq!(
            first_failure.diagnostic, second_failure.diagnostic,
            "redaction must remove the only difference between the two staged roots' diagnostics"
        );

        let first = base_snapshot(Err(first_failure));
        let second = base_snapshot(Err(second_failure));

        let first_identities = identities_for(&first);
        let second_identities = identities_for(&second);
        assert_eq!(
            first_identities.adapter_set_hash, second_identities.adapter_set_hash,
            "the same failure kind must hash identically regardless of which staged TempDir's \
             diagnostic text a run happened to carry"
        );

        let first_result = lift_cargo_failure(&first, &first_identities);
        let second_result = lift_cargo_failure(&second, &second_identities);
        assert_eq!(
            cargo_metadata_unavailable_limitation(&first_result).id,
            cargo_metadata_unavailable_limitation(&second_result).id,
            "the limitation ID must not depend on the TempDir-specific diagnostic either"
        );
        assert_eq!(
            first_result
                .canonical_output()
                .expect("first run's canonical output"),
            second_result
                .canonical_output()
                .expect("second run's canonical output"),
            "the full canonical output must be byte-identical across the two runs"
        );
    }

    #[test]
    fn the_same_cargo_failure_kind_with_deliberately_different_diagnostics_matches_completely() {
        // Unlike the `TempDir`-redaction test above (where the two
        // diagnostics are only different in a path both runs redact away to
        // the exact same text), these two diagnostics are genuinely
        // different subprocess stderr content for the same failure `kind`.
        // `description` must never be built from `diagnostic`, so this must
        // still produce the same `Limitation` (not only the same ID), the
        // same `adapter_set_hash`, and byte-identical `canonical_output()`.
        let first = failing_snapshot(
            git::CargoToolFailureKind::NonSuccessExit,
            "error: could not find `rust-toolchain.toml` in any parent directory",
        );
        let second = failing_snapshot(
            git::CargoToolFailureKind::NonSuccessExit,
            "error: linker `cc` not found: No such file or directory (os error 2)",
        );

        let first_identities = identities_for(&first);
        let second_identities = identities_for(&second);
        assert_eq!(
            first_identities.adapter_set_hash, second_identities.adapter_set_hash,
            "adapter_set_hash must not depend on the raw subprocess diagnostic"
        );

        let first_result = lift_cargo_failure(&first, &first_identities);
        let second_result = lift_cargo_failure(&second, &second_identities);
        assert_eq!(
            cargo_metadata_unavailable_limitation(&first_result),
            cargo_metadata_unavailable_limitation(&second_result),
            "the full limitation -- id, description, and every other field, not only the id \
             -- must be identical when only the raw subprocess diagnostic differs"
        );
        assert_eq!(
            first_result
                .canonical_output()
                .expect("first run's canonical output"),
            second_result
                .canonical_output()
                .expect("second run's canonical output"),
            "the full canonical output must be byte-identical regardless of diagnostic text"
        );
    }

    #[test]
    fn a_different_cargo_version_failure_kind_changes_the_hash_and_limitation_id() {
        let unavailable = failing_snapshot(
            git::CargoToolFailureKind::Unavailable,
            "cargo: command not found",
        );
        let not_utf8 = failing_snapshot(
            git::CargoToolFailureKind::NotUtf8,
            "cargo: command not found",
        );

        let unavailable_identities = identities_for(&unavailable);
        let not_utf8_identities = identities_for(&not_utf8);
        assert_ne!(
            unavailable_identities.adapter_set_hash,
            not_utf8_identities.adapter_set_hash
        );

        let unavailable_result = lift_cargo_failure(&unavailable, &unavailable_identities);
        let not_utf8_result = lift_cargo_failure(&not_utf8, &not_utf8_identities);
        assert_ne!(
            cargo_metadata_unavailable_limitation(&unavailable_result).id,
            cargo_metadata_unavailable_limitation(&not_utf8_result).id,
        );
        assert_ne!(
            unavailable_result
                .canonical_output()
                .expect("unavailable run's canonical output"),
            not_utf8_result
                .canonical_output()
                .expect("not-utf8 run's canonical output"),
        );
    }
}

/// Proves `GitSnapshot::cargo_executable`'s own doc-comment claim end to
/// end: the admitted `cargo` binary's absolute path is host-specific and
/// deliberately excluded from `adapter_set_hash`/`canonical_output()` --
/// only the *version string* it reports (and the fixed
/// `cargo_resolver_policy`) is bound into identity. Two genuinely different
/// fake `cargo` executables at two different absolute paths (never the same
/// `TempDir`, unlike `cargo_failure_identity_tests`'s redaction tests) that
/// happen to report byte-identical `cargo --version` output and identical
/// (here: identically *rejected*, "no packages array") `cargo metadata`
/// output must still produce the same `adapter_set_hash` and
/// byte-identical `canonical_output()`.
#[cfg(test)]
mod cargo_executable_path_independence_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    /// A fake `cargo` that reports a fixed `--version` string and, for
    /// anything else (here: `metadata ...`), a syntactically valid but
    /// packages-less JSON object -- real `cargo metadata`'s own well-formed
    /// output shape when it is called against a manifest that resolves to
    /// no packages, so `extract_cargo_metadata` takes its ordinary
    /// `capability_missing`/`Failed` path rather than a JSON-parse error.
    fn fake_cargo_script() -> &'static str {
        "#!/bin/sh\nif [ \"${1:-}\" = \"--version\" ]; then\n  echo \"cargo 1.75.0-test\"\n  exit 0\nfi\necho '{}'\nexit 0\n"
    }

    /// Initial execution plus at most seven retries; sleeps total at most 35ms.
    const TEST_EXEC_RETRY_LIMIT: usize = 7;
    const TEST_EXEC_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(5);
    const TEST_EXEC_PROBE_ARGUMENT: &str = "--reviewgraphen-test-exec-probe";

    fn write_executable(path: &Path, contents: &str) -> std::io::Result<()> {
        let body = contents.strip_prefix("#!/bin/sh\n").ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "fake executable must use the expected /bin/sh shebang",
            )
        })?;
        std::fs::write(
            path,
            format!(
                "#!/bin/sh\nif [ \"${{1:-}}\" = \"{TEST_EXEC_PROBE_ARGUMENT}\" ]; then\n  exit 0\nfi\n{body}"
            ),
        )?;
        let mut permissions = std::fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)?;
        wait_for_test_executable(path)
    }

    fn wait_for_test_executable(path: &Path) -> std::io::Result<()> {
        for retry in 0..=TEST_EXEC_RETRY_LIMIT {
            match std::process::Command::new(path)
                .arg(TEST_EXEC_PROBE_ARGUMENT)
                .status()
            {
                Ok(status) if status.success() => return Ok(()),
                Ok(status) => {
                    return Err(std::io::Error::other(format!(
                        "test executable probe exited unsuccessfully with {status}"
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    if retry == TEST_EXEC_RETRY_LIMIT {
                        return Err(error);
                    }
                    std::thread::sleep(TEST_EXEC_RETRY_DELAY);
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("bounded test executable retry either succeeds or returns its typed error")
    }

    /// Stages a private snapshot directory carrying the same `Cargo.toml`
    /// `snapshot`'s own `files` declares, mirroring what `git::stage_files`
    /// does for a real ingest run -- `run_cargo_metadata` reads from disk,
    /// independent of the in-memory `GitSnapshot::files`.
    fn staged_snapshot_with_cargo_toml(content: &[u8]) -> TempDir {
        let staged = tempfile::tempdir().expect("staged snapshot dir");
        std::fs::write(staged.path().join("Cargo.toml"), content).expect("write staged Cargo.toml");
        staged
    }

    /// Resolves a fresh fake `cargo` at its own, unique absolute path (a
    /// fresh `TempDir` per call), admits it via the real `resolve_cargo`
    /// entry point, and runs `extract_cargo_metadata` against a `GitSnapshot`
    /// built around it -- the exact `load_snapshot`/`extract_cargo_metadata`
    /// sequence a real `ingest()` run uses, never a hand-simulated stand-in.
    fn ingest_result_with_independent_fake_cargo() -> (IngestResult, PathBuf) {
        let cargo_toml_content =
            b"[package]\nname = \"m2-fixture\"\nversion = \"0.1.0\"\n".to_vec();
        let bin_dir = tempfile::tempdir().expect("fake cargo bin dir");
        let cargo_path = bin_dir.path().join("cargo");
        write_executable(&cargo_path, fake_cargo_script())
            .expect("fake cargo executable becomes exec-ready within the bounded retry");
        let staged = staged_snapshot_with_cargo_toml(&cargo_toml_content);
        let (executable, version) = git::resolve_cargo(
            &CargoToolAdmission::TrustedExecutable(cargo_path),
            staged.path(),
        )
        .expect("fake cargo --version succeeds through the real resolve_cargo entry point");
        let executable_path = executable.clone();

        let file = git::SnapshotFile {
            path: "Cargo.toml".to_owned(),
            content_hash: ContentHash::sha256(&cargo_toml_content),
            content: cargo_toml_content,
            changed_lines: BTreeSet::new(),
        };
        let snapshot = git::GitSnapshot {
            repository_root: "/repo".to_owned(),
            repository_identity: "reviewgraphen.test/cargo-executable-path-independence".to_owned(),
            repository_name: "repo".to_owned(),
            base_revision: "0".repeat(40),
            base_tree_hash: ContentHash::parse(format!("git:{}", "3".repeat(40)))
                .expect("valid test base tree hash"),
            target_revision: "1".repeat(40),
            tree_hash: ContentHash::parse(format!("git:{}", "2".repeat(40)))
                .expect("valid test tree hash"),
            config: IngestConfig::default(),
            files: vec![file],
            changes: Vec::new(),
            issues: Vec::new(),
            adapter_reports: Vec::new(),
            capabilities: BTreeMap::new(),
            capability_sources: BTreeMap::new(),
            git_version: "git version 2.43.0".to_owned(),
            cargo_executable: Some(executable),
            cargo_version: Ok(version),
            staged_snapshot: Some(staged),
        };
        let identities = SnapshotIdentities::new(
            &snapshot,
            &snapshot.config,
            "syn 0.0.0-test",
            "proc-macro2 0.0.0-test",
            "quote 0.0.0-test",
            &git::git_command_policy_fingerprint(),
            &git::cargo_resolver_policy_fingerprint(),
        )
        .expect("identities derive");
        let cargo = git::extract_cargo_metadata(&snapshot, &identities.snapshot_id);
        let drafts = snapshot.files.iter().map(file_artifact_draft).collect();
        let result = lift(
            &snapshot,
            &identities,
            LiftInputs {
                drafts,
                relation_drafts: Vec::new(),
                rust_anchor_drafts: BTreeMap::new(),
                issues: cargo.issues,
                adapter_reports: vec![cargo.adapter_report],
                capabilities: cargo.capabilities,
                capability_sources: cargo.capability_sources,
            },
        )
        .expect("lift succeeds");
        (result, executable_path)
    }

    #[test]
    fn different_admitted_cargo_paths_with_identical_output_hash_and_canonicalize_identically() {
        let (first, first_executable) = ingest_result_with_independent_fake_cargo();
        let (second, second_executable) = ingest_result_with_independent_fake_cargo();

        assert_ne!(
            first_executable, second_executable,
            "the two runs must have admitted genuinely different absolute cargo paths for this \
             test to be meaningful"
        );
        assert_eq!(
            first.extraction_report.capabilities["cargo_metadata"],
            CapabilityState::Missing,
            "both runs must have actually exercised extract_cargo_metadata's real \
             no-packages-array path, not merely skipped it"
        );
        assert_eq!(
            first.extraction_report.adapter_set_hash, second.extraction_report.adapter_set_hash,
            "the admitted cargo executable's own absolute path is host-specific and must never \
             affect adapter_set_hash -- only the reported version string and the fixed \
             cargo_resolver_policy may"
        );
        assert_eq!(
            first
                .canonical_output()
                .expect("first run's canonical output"),
            second
                .canonical_output()
                .expect("second run's canonical output"),
            "two runs admitting genuinely different cargo binaries that report identical \
             output must produce byte-identical canonical_output()"
        );
    }
}
