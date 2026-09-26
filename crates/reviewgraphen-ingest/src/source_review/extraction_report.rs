//! Canonical extraction-ledger contracts for G1-S5.
//!
//! The design makes extraction admission a source
//! comparison against the native basis, not a report-owned digest recomputation.

use crate::typescript::payload::TypeScriptPayload;
use reviewgraphen_core::source_review::admitted_source::AdmittedSourceBundleV1;
use reviewgraphen_core::source_review::basis::SourceReviewBasisV1;
use reviewgraphen_core::source_review::ids::{AccountingMismatch, SourceFileId, SourceRange};
use reviewgraphen_core::source_review::reasons::ReasonSet;
use reviewgraphen_core::source_review::registry::TypeScriptRegistryBinding;

/// The six mutually exclusive file inventory outcomes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum FileOutcome {
    Parsed,
    ParseFailed,
    NonTargetExtension,
    ProfileExcluded,
    UnreadBound,
    UnsupportedEntry,
}

/// A basis-bound file ledger record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileRecord {
    pub file_id: SourceFileId,
    pub outcome: FileOutcome,
    pub bytes_read: bool,
    pub byte_count: Option<u64>,
    pub reasons: ReasonSet,
    pub latent_callable_count: Option<u64>,
}

/// A raw syntax catalogue row submitted to runtime admission.
///
/// Its payload is typed but is not a validation capability. Runtime A2/A3
/// rebuild every role from an admitted reconstruction context before accepting
/// this row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxRecord {
    pub file_id: SourceFileId,
    pub range: SourceRange,
    pub payload: TypeScriptPayload,
}

/// A submitted extraction report before source validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractionReport {
    pub registry_binding: TypeScriptRegistryBinding,
    pub files: Vec<FileRecord>,
    pub syntax: Vec<SyntaxRecord>,
    pub inventory_complete: bool,
}

/// The adapter's independently rebuilt canonical draft, kept distinct from the
/// supplied report so an owner cannot validate itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalExtractionDraft {
    report: ExtractionReport,
}

/// Extraction reconstruction or canonical comparison failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtractionError {
    AccountingMismatch(AccountingMismatch),
    BasisIncompatible,
    /// Canonical extraction reconstruction is not implemented yet
    /// (TypeScript I2); callers get a typed refusal instead of a panic.
    NotImplemented,
}

impl std::fmt::Display for ExtractionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccountingMismatch(mismatch) => {
                write!(formatter, "extraction accounting mismatch: {mismatch}")
            }
            Self::BasisIncompatible => formatter.write_str("extraction basis is incompatible"),
            Self::NotImplemented => {
                formatter.write_str("canonical extraction reconstruction is not implemented")
            }
        }
    }
}

impl std::error::Error for ExtractionError {}

/// Reconstructs the canonical extraction draft from the admitted basis and its
/// source bundle. This output is deliberately distinct from a submitted report.
pub fn rebuild_canonical_extraction(
    _basis: &SourceReviewBasisV1,
    _sources: &AdmittedSourceBundleV1,
) -> Result<CanonicalExtractionDraft, ExtractionError> {
    Err(ExtractionError::NotImplemented)
}
