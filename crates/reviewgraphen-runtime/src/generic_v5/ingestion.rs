//! Source-validated D aggregation contracts for G1-S5.
//!
//! The design requires pair aggregation to
//! preserve identity-bound callsites, all reasons, and the registered primary
//! selection without relying on input order.

use reviewgraphen_core::ContentHash;
use reviewgraphen_core::source_review::ids::{
    AccountingMismatch, CallerId, CallsiteId, ChangeWitnessRef, DeclarationId, DeclarationPairId,
    SnapshotBinding,
};
use reviewgraphen_core::source_review::reasons::{CallReason, ReasonSet, ResolutionKind};
use reviewgraphen_core::source_review::registry::TypeScriptRegistryBinding;
use std::collections::{BTreeMap, BTreeSet};

/// A source-validated relative or local callsite ready for D aggregation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPair {
    pub caller_id: CallerId,
    pub callee_id: DeclarationId,
    pub callsite_key: CallsiteId,
    pub resolution_kind: ResolutionKind,
    pub callsite_reasons: ReasonSet,
    pub change_witness_refs: BTreeSet<ChangeWitnessRef>,
    pub visibility_witness_refs: BTreeSet<ChangeWitnessRef>,
    pub changed: bool,
    pub callee_public: bool,
}

/// The reason set associated with one original callsite. This is intentionally
/// retained after pair aggregation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallsiteReasonSet {
    pub callsite_key: CallsiteId,
    pub reasons: ReasonSet,
}

/// One D pair. It exposes both every callsite's reason set and the primary
/// reason chosen from their union, so input order cannot change the result
///.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DPair {
    pub pair_id: DeclarationPairId,
    pub caller_id: CallerId,
    pub callee_id: DeclarationId,
    pub resolution_kinds: BTreeSet<ResolutionKind>,
    pub callsite_reasons: BTreeMap<CallsiteId, ReasonSet>,
    pub eligible: bool,
    pub ineligible_reasons: ReasonSet,
    pub primary_reason: Option<CallReason>,
    pub change_witness_refs: BTreeSet<ChangeWitnessRef>,
    pub visibility_witness_refs: BTreeSet<ChangeWitnessRef>,
}

/// The D partition tracks typed pair identity sets; the digest is derived only
/// after canonical source validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DPartition {
    pub eligible: BTreeSet<DeclarationPairId>,
    pub ineligible: BTreeSet<DeclarationPairId>,
    pub unknown: Option<u64>,
    /// Digest of the sorted declaration-pair identity set; never an input-order
    /// digest.
    pub digest: ContentHash,
}

impl DPartition {
    /// Returns the only preimage for [`DPartition::digest`]: the JSON value
    /// `{"declaration_pair_keys": PAIR_KEYS, "domain":
    /// "source_review_d_partition.v1"}`, where `PAIR_KEYS` is the sorted,
    /// duplicate-free union of `eligible` and `ineligible`, each value from
    /// [`DeclarationPairId::canonical_key`]. `unknown` has no fabricated pair
    /// ID and is therefore not represented in this identity-set digest. The
    /// digest is `ContentHash::sha256(canonical_json(&value))`; pair names,
    /// display labels, and input order are never encoded.
    #[cfg(reviewgraphen_unimplemented_contracts)]
    #[must_use]
    pub fn canonical_digest_preimage(&self) -> serde_json::Value {
        todo!("I2 skeleton: implementation must expose the D partition digest preimage")
    }
}

/// A submitted extraction-owner binding for an ingestion report.
///
/// This is raw caller material, not a capability. Runtime A4 compares it with
/// the provenance embedded in its reconstruction context before producing any
/// `SourceValidated<IngestionReport>`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractionBindingV1 {
    pub snapshot_binding: SnapshotBinding,
    pub registry_binding: TypeScriptRegistryBinding,
    pub extraction_digest: ContentHash,
}

/// A submitted ingestion report. It contains no extraction capability or
/// duplicated extraction rows; its binding is compared by runtime A4.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestionReport {
    pub registry_binding: TypeScriptRegistryBinding,
    pub extraction_binding: ExtractionBindingV1,
    pub d_pairs: Vec<DPair>,
    pub d_partition: DPartition,
}

/// The raw A4 submitted side. Runtime reconstructs both `resolved_calls` and
/// `report` from the immutable A0 context; neither field is source authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestionSubmission {
    pub resolved_calls: Vec<ResolvedPair>,
    pub report: IngestionReport,
}

/// Aggregation failed because one source-derived partition or registry binding
/// cannot be reconciled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IngestionError {
    AccountingMismatch(AccountingMismatch),
    MixedArm,
}

impl std::fmt::Display for IngestionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccountingMismatch(mismatch) => {
                write!(formatter, "ingestion accounting mismatch: {mismatch}")
            }
            Self::MixedArm => formatter.write_str("ingestion mixes registry arms"),
        }
    }
}

impl std::error::Error for IngestionError {}

/// Aggregates callsites into declaration-ID pairs. The implementation must
/// reject duplicate [`CallsiteId`] values and same-pair conflicting source
/// attributes before aggregation, union all reason sets before selecting
/// [`DPair::primary_reason`], and sort only canonical sets, never use input
/// order.
#[cfg(reviewgraphen_unimplemented_contracts)]
pub fn aggregate_pairs(_pairs: Vec<ResolvedPair>) -> Result<Vec<DPair>, IngestionError> {
    todo!("I2 skeleton: implementation must aggregate every callsite reason set")
}

/// Computes a raw D partition from one already-checked pair set.
///
/// This pure set calculation validates no source input and cannot mint a
/// capability. Source reconstruction belongs exclusively to runtime A4
///.
#[cfg(reviewgraphen_unimplemented_contracts)]
pub fn d_partition(_pairs: &[DPair]) -> Result<DPartition, IngestionError> {
    todo!("I2 skeleton: implementation must compute the raw D identity partition")
}

/// Validates an externally supplied registry binding before any ingestion owner
/// accepts it. The five binding fields are positional and coupled; swapping two
/// known hashes is rejected instead of being treated as an unordered bag
///.
#[cfg(reviewgraphen_unimplemented_contracts)]
pub fn validate_ingestion_registry_binding(
    _binding: TypeScriptRegistryBinding,
) -> Result<TypeScriptRegistryBinding, IngestionError> {
    todo!("I2 skeleton: implementation must validate positional registry binding fields")
}
