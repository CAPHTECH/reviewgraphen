//! Source-validated N/D/G materialization contracts for G1-S6.
//!
//! The design makes this a bidirectional
//! closure check: a count or a self-derived partition is never sufficient.

use super::basis::SourceSyntaxRole;
use super::ids::{
    AccountingMismatch, DeclarationId, DeclarationPairId, ObligationId, SnapshotBinding,
    SourceFileId, SourceRange,
};
use super::reasons::{DReasonSetV1, NodeObstructionReasonSetV1, ObligationDeferReasonSetV1};
use super::registry::TypeScriptRegistryBinding;
use crate::typescript::rules::{CoverageLayer, PropertyId, RuleId, TargetKind};
use std::collections::{BTreeMap, BTreeSet};

/// All allowed materialization targets; a target is identity-backed, not a key
/// string.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ObligationTarget {
    Node(DeclarationId),
    Relation(DeclarationPairId),
    SnapshotGap(SnapshotBinding),
}

impl ObligationTarget {
    /// Returns the key admitted in `ObligationPreimage`'s `target_key` field.
    /// It is UTF-8 `canonical_json` text with domain
    /// `source_review_obligation_target.v1`: Node is
    /// `{"domain":"source_review_obligation_target.v1","kind":"node",
    /// "node_key": DECLARATION_KEY}`, Relation is
    /// `{"domain":"source_review_obligation_target.v1","kind":"relation",
    /// "pair_key": PAIR_KEY}`, and SnapshotGap is
    /// `{"domain":"source_review_obligation_target.v1","kind":"snapshot_gap",
    /// "snapshot_binding": SNAPSHOT_BINDING}`. The nested values are typed
    /// canonical keys (or the admitted binding), never declaration names
    ///.
    #[must_use]
    pub fn canonical_key(&self) -> String {
        let value = match self {
            Self::Node(declaration) => serde_json::json!({
                "domain": "source_review_obligation_target.v1",
                "kind": "node",
                "node_key": declaration.canonical_key(),
            }),
            Self::Relation(pair) => serde_json::json!({
                "domain": "source_review_obligation_target.v1",
                "kind": "relation",
                "pair_key": pair.canonical_key(),
            }),
            Self::SnapshotGap(binding) => serde_json::json!({
                "domain": "source_review_obligation_target.v1",
                "kind": "snapshot_gap",
                "snapshot_binding": binding.as_str(),
            }),
        };
        String::from_utf8(
            crate::canonical_json(&value).expect("fixed obligation target is canonical JSON"),
        )
        .expect("canonical JSON is UTF-8")
    }
}

/// A syntax-record identity retained as an obligation support witness.
///
/// This is a source-position key, not a display label or an artifact digest.
/// A0's fixed adapter must create it only from the admitted target draft
///.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SyntaxKey {
    file_id: SourceFileId,
    range: SourceRange,
    role: SourceSyntaxRole,
}

impl SyntaxKey {
    /// Binds a syntax witness to its source file, UTF-8 byte range, and role.
    #[must_use]
    pub fn from_source(file_id: SourceFileId, range: SourceRange, role: SourceSyntaxRole) -> Self {
        Self {
            file_id,
            range,
            role,
        }
    }
}

/// A source-derived endpoint used where an obligation supports a callable or
/// relation endpoint rather than one syntax envelope.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct BasisEndpointId(DeclarationId);

impl BasisEndpointId {
    /// Binds an endpoint witness to the declaration identity reconstructed by A0.
    #[must_use]
    pub fn from_declaration(declaration: DeclarationId) -> Self {
        Self(declaration)
    }
}

/// The closed witness-key union allowed in an obligation preimage.
///
/// `SyntaxKey` carries source syntax support; `BasisEndpointId` carries a
/// source-derived endpoint. Neither arm accepts a free-form key, report digest,
/// or later run artifact.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum SourceWitnessKey {
    SyntaxKey(SyntaxKey),
    BasisEndpointId(BasisEndpointId),
}

/// The complete canonical preimage exposed with each materialized obligation.
/// It is public so acceptance can compare a fixture-derived value rather than
/// compare an implementation constant to itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObligationPreimage {
    pub registry_binding: TypeScriptRegistryBinding,
    pub snapshot_binding: SnapshotBinding,
    pub rule_id: RuleId,
    pub property_id: PropertyId,
    pub origin_rule_id: Option<RuleId>,
    pub target: ObligationTarget,
    pub source_witnesses: BTreeSet<SourceWitnessKey>,
}

/// A materialized obligation and the exact preimage that produced its ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Obligation {
    pub id: ObligationId,
    pub preimage: ObligationPreimage,
    pub target_kind: TargetKind,
    pub coverage_layer: CoverageLayer,
    pub status: ObligationStatus,
    /// Bound-policy defer reasons. Materialized first-cohort obligations carry
    /// `Some(non_empty)`; the token vocabulary is not closed here.
    pub deferred_reason: Option<ObligationDeferReasonSetV1>,
}

/// Enumeration mode for this cohort; all source-validated obligations defer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum ObligationStatus {
    Planned,
    Deferred,
}

/// D input already keyed by declaration-backed caller/callee identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DSynthPair {
    pub pair_id: DeclarationPairId,
    pub eligible: bool,
    pub reasons: DReasonSetV1,
}

/// Submitted N/D/G candidates. All keys are typed IDs so a name collision
/// cannot form, block, or change an obligation.
///
/// This public struct is deliberately not a source-validation capability:
/// callers can construct it, but must pass it to the runtime ingestion
/// runtime A5 admission together with source-validated extraction/D facts
/// before it can reach the runtime facade's D1 derivation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SynthesizeInput {
    pub binding: TypeScriptRegistryBinding,
    pub snapshot_binding: SnapshotBinding,
    pub eligible_nodes: BTreeSet<DeclarationId>,
    pub blocked_nodes: BTreeMap<DeclarationId, NodeObstructionReasonSetV1>,
    pub d_pairs: Vec<DSynthPair>,
    pub d_partial: bool,
    /// Source syntax and endpoint witnesses supporting the submitted Node/D
    /// candidates. A5 compares this complete set with the A3/A4 reconstruction.
    pub source_witness_keys: BTreeSet<SourceWitnessKey>,
    /// Source-backed limitation witnesses for the one snapshot-global D gap.
    /// These are keys, not report hashes or free-form reason text.
    pub d_global_limitation_keys: BTreeSet<SourceWitnessKey>,
}

/// The source-validated N/D/G set plus both directions of every accounting
/// partition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObligationSet {
    pub obligations: Vec<Obligation>,
    pub planned_ids: BTreeSet<ObligationId>,
    pub deferred_ids: BTreeSet<ObligationId>,
    pub blocked_nodes: BTreeMap<DeclarationId, NodeObstructionReasonSetV1>,
    pub ineligible_d_pairs: BTreeMap<DeclarationPairId, DReasonSetV1>,
    pub snapshot_gaps: BTreeSet<SnapshotBinding>,
    pub eligible_nodes: BTreeSet<DeclarationId>,
    pub eligible_d_pairs: BTreeSet<DeclarationPairId>,
    pub d_partial: bool,
}

/// The complete externally submitted side of an S6 closure comparison.
///
/// This is deliberately distinct from [`ObligationSet`], which is produced
/// only after source validation. A validator must compare every record's
/// preimage, target kind, and status as well as both directions of the N/D/G
/// partitions. Checking IDs or a digest alone is insufficient.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObligationClosureSubmission {
    pub records: Vec<Obligation>,
    pub planned_ids: BTreeSet<ObligationId>,
    pub deferred_ids: BTreeSet<ObligationId>,
    pub eligible_nodes: BTreeSet<DeclarationId>,
    pub blocked_nodes: BTreeMap<DeclarationId, NodeObstructionReasonSetV1>,
    pub eligible_d_pairs: BTreeSet<DeclarationPairId>,
    pub ineligible_d_pairs: BTreeMap<DeclarationPairId, DReasonSetV1>,
    pub snapshot_gaps: BTreeSet<SnapshotBinding>,
    pub d_partial: bool,
}

/// The two independently reconstructed sides that must close exactly.
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Reserved private A6 comparison diagnostic; never public admission state.
pub(crate) struct ObligationClosure {
    pub expected_materialized: BTreeSet<ObligationId>,
    pub actual_materialized: BTreeSet<ObligationId>,
    pub expected_gap_targets: BTreeSet<SnapshotBinding>,
    pub actual_gap_targets: BTreeSet<SnapshotBinding>,
}

/// Synthesis failed because a registry-coupled partition cannot be materialized.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SynthesisError {
    AccountingMismatch(AccountingMismatch),
    RegistryRuleMismatch,
}

impl std::fmt::Display for SynthesisError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccountingMismatch(mismatch) => {
                write!(formatter, "synthesis accounting mismatch: {mismatch}")
            }
            Self::RegistryRuleMismatch => {
                formatter.write_str("synthesis rule does not match the registered arm")
            }
        }
    }
}

impl std::error::Error for SynthesisError {}
