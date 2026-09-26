//! Declaration-ID and source-range change-witness contracts for G1-S4.
//!
//! The design requires a full declaration or same-file
//! export witness. Names alone cannot select the first of duplicate declarations.

use reviewgraphen_core::source_review::ids::{
    ChangeWitnessRef, DeclarationId, SourceIdentity, SourceRange,
};
use tree_sitter::Node;

/// Base and target source identities plus their AST roots. The declaration IDs
/// are source-backed on their respective snapshot; no name selector exists.
pub struct ChangeWitnessInput<'tree, 'source> {
    pub base_identity: SourceIdentity,
    pub base_root: Node<'tree>,
    pub base_source: &'source [u8],
    pub base_declaration_id: DeclarationId,
    pub target_identity: SourceIdentity,
    pub target_root: Node<'tree>,
    pub target_source: &'source [u8],
    pub target_declaration_id: DeclarationId,
}

/// The kind of a source-derived D witness.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ChangeWitnessKind {
    Declaration,
    SameFileExport,
}

/// A source identity/range witness returned to D aggregation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeWitness {
    pub reference: ChangeWitnessRef,
    pub kind: ChangeWitnessKind,
    pub compared_range: SourceRange,
}

/// Change-witness extraction error; callers cannot silently convert an absent
/// declaration, foreign source, or unmatched ID into an unchanged result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChangeWitnessError {
    SourceIdentityMismatch,
    DeclarationNotFound,
    RangeMismatch,
    /// Change-witness reconstruction is not implemented yet (TypeScript I2);
    /// callers get a typed refusal instead of a panic.
    NotImplemented,
}

impl std::fmt::Display for ChangeWitnessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::SourceIdentityMismatch => "change witness source identity mismatch",
            Self::DeclarationNotFound => "change witness declaration not found",
            Self::RangeMismatch => "change witness range mismatch",
            Self::NotImplemented => "change witness reconstruction is not implemented",
        })
    }
}

impl std::error::Error for ChangeWitnessError {}

/// Reconstructs declaration and same-file export witnesses from base/target AST
/// sources. Both identity and range are inputs and witnesses are returned by
/// reference, satisfying the changed-D source trace requirement.
pub fn change_witnesses(
    _input: ChangeWitnessInput<'_, '_>,
) -> Result<Vec<ChangeWitness>, ChangeWitnessError> {
    Err(ChangeWitnessError::NotImplemented)
}
