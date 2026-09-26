//! Closed source-admission facade for TypeScript I2.
//!
//! `SourceValidated` is defined in the private `sealed` module and is publicly
//! re-exported only from this facade. The seven functions below are the entire
//! public capability-producing surface; raw reconstruction helpers in core,
//! ingest, and runtime never mint this type.

use reviewgraphen_core::source_review::admitted_source::AdmittedSourceBundleV1;
use reviewgraphen_core::source_review::basis::{
    SourceFileOutcome, SourceReviewBasisV1, SourceReviewFileV1, SourceReviewSyntaxV1,
    SourceSyntaxRole,
};
use reviewgraphen_core::source_review::ids::{
    AccountingMismatch, CanonicalFileKey, SnapshotBinding, SourceFileId, SourceHash,
    registry_tuple_hash,
};
use reviewgraphen_core::source_review::registry::{
    TypeScriptRegistryBinding, typescript_registry_binding, validate_typescript_binding,
};
use reviewgraphen_core::typescript::profile::{
    TypeScriptPathClassification, classify_typescript_v1_path,
};
use reviewgraphen_ingest::source_review::extraction_report::{ExtractionReport, SyntaxRecord};
use reviewgraphen_ingest::typescript::payload::{
    PayloadDraft, PayloadError, TypeScriptPayload, TypeScriptPayloadSourceViewV1,
    encode_payload_draft, rebuild_payload_from_source,
};
use reviewgraphen_ingest::typescript::syntax::parse_typescript;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) mod g3_observe;

/// The request-side bounds A0 checks before walking a literal Git tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceAdmissionBoundsV1 {
    pub max_files: usize,
    pub max_file_bytes: usize,
    pub max_total_source_bytes: usize,
}

/// Raw repository and snapshot selection submitted to A0.
///
/// Every field is a claim or a requested bound. A0 reads the repository,
/// commit, tree, and registry facts itself and compares this value; no field is
/// Git authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceAdmissionRequestV1 {
    pub workspace_admission_root: PathBuf,
    pub repository_admission_root: PathBuf,
    pub repository_identity: String,
    pub base_commit_oid: String,
    pub base_tree_oid: String,
    pub target_commit_oid: String,
    pub target_tree_oid: String,
    pub registry_binding: TypeScriptRegistryBinding,
    pub bounds: SourceAdmissionBoundsV1,
}

/// The claimed Git entry class retained for a submitted inventory row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceInventoryEntryKindV1 {
    Blob,
    Symlink,
    Submodule,
}

/// The fixed profile decision for one inventory entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceProfileClaimV1 {
    Included,
    Excluded {
        reasons: Vec<String>,
        primary_reason: String,
    },
    NonTargetExtension,
}

/// One submitted target-tree inventory claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceInventoryClaimV1 {
    pub path: String,
    pub object_oid: String,
    pub mode: u32,
    pub entry_kind: SourceInventoryEntryKindV1,
    pub profile: SourceProfileClaimV1,
    pub outcome: SourceFileOutcome,
    pub read: SourceReadClaimV1,
}

/// One discovered non-root tree entry, retained outside `target_inventory`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceTreeEntryClaimV1 {
    pub path: String,
    pub object_oid: String,
    pub mode: u32,
    pub parent_tree_oid: String,
}

/// The read state of an inventory entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceReadClaimV1 {
    NotRead {
        reason: SourceNotReadReasonV1,
    },
    Complete {
        byte_count: u64,
        source_hash: SourceHash,
    },
    Failed {
        byte_count: u64,
        prefix_hash: SourceHash,
        cause: SourceIoFailureV1,
    },
}

/// A reason an entry was not read at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceNotReadReasonV1 {
    NonTargetExtension,
    ProfileExcluded,
    UnsupportedEntry,
    FileCountBound,
    FileByteBound,
    TotalSourceByteBound,
}

/// A concrete read failure. `TotalSourceByteBound` distinguishes a budget
/// interruption after a read has begun from `NotRead` before the read begins
///.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceIoFailureV1 {
    MissingObject,
    Io,
    Timeout,
    OutputBound,
    TotalSourceByteBound,
}

/// Exact material obtained from one target read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceReadMaterialClaimV1 {
    pub path: String,
    pub bytes: Vec<u8>,
    pub hash: SourceHash,
    pub extent: SourceReadExtentV1,
}

/// Whether raw read material is a complete blob or an interrupted prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceReadExtentV1 {
    FullBlob,
    InterruptedPrefix,
}

/// A target-tree ancestor which could not be expanded completely.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnexpandedAncestorClaimV1 {
    pub path: String,
    pub object_oid: String,
    pub cause: SourceAncestorCauseV1,
}

/// The concrete obstruction preventing one ancestor expansion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceAncestorCauseV1 {
    TreeReadFailed(SourceIoFailureV1),
    Symlink,
    Submodule,
}

/// One raw base-side claim. A0 derives the exact C set from target resolution;
/// this vector cannot remove a comparison path or turn unread into absent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaseEntryClaimV1 {
    pub path: String,
    pub state: BaseEntryStateV1,
}

/// A checked tree proof retained for a base lookup path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceTreeProofV1 {
    pub path: String,
    pub tree_oid: String,
}

/// A complete base blob retained with its object identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BaseBlobClaimV1 {
    pub object_oid: String,
    pub mode: u32,
    pub bytes: Vec<u8>,
    pub source_hash: SourceHash,
}

/// A caller's claimed base-side availability for one A0 comparison path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BaseEntryStateV1 {
    Present {
        ancestors: Vec<SourceTreeProofV1>,
        blob: BaseBlobClaimV1,
    },
    Absent {
        ancestors: Vec<SourceTreeProofV1>,
        missing_path: String,
    },
    Unavailable {
        ancestors: Vec<SourceTreeProofV1>,
        obstruction: BaseUnavailableClaimV1,
    },
}

/// A base lookup which cannot prove a present/absent ordinary blob.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BaseUnavailableClaimV1 {
    UnexpandedAncestor(UnexpandedAncestorClaimV1),
    NonRegularEntry {
        path: String,
        object_oid: String,
        mode: u32,
    },
    NotRead {
        object_oid: String,
        mode: u32,
        reason: SourceNotReadReasonV1,
    },
    ReadFailed {
        object_oid: String,
        mode: u32,
        partial: SourceReadMaterialClaimV1,
        cause: SourceIoFailureV1,
    },
    ParseFailed(BaseBlobClaimV1),
}

/// All submitted source material that A0 compares with its literal Git rebuild.
///
/// `target_basis` and the read-material projection are submitted I1 projections, not an
/// alternative source authority. The A0 implementation must reject missing,
/// extra, or same-key-different material before constructing a context
///.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceAdmissionMaterialV1 {
    pub target_basis: SourceReviewBasisV1,
    pub target_inventory: Vec<SourceInventoryClaimV1>,
    pub target_tree_entries: Vec<SourceTreeEntryClaimV1>,
    pub inventory_complete: bool,
    pub unexpanded_ancestors: Vec<UnexpandedAncestorClaimV1>,
    pub target_read_material: Vec<SourceReadMaterialClaimV1>,
    pub base_entries: Vec<BaseEntryClaimV1>,
}

/// The one raw input to A0. It is a submission DTO, never a context or a
/// `SourceValidated` capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceAdmissionSubmissionV1 {
    pub request: SourceAdmissionRequestV1,
    pub material: SourceAdmissionMaterialV1,
}

/// Raw A1 payload input. `locator` is checked only after the context has
/// rebuilt the canonical file set, so it cannot narrow the source denominator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayloadSubmission {
    pub locator: SourceFileId,
    pub draft: PayloadDraft,
}

/// A0 failure before a reconstruction context exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceSnapshotSideV1 {
    Base,
    Target,
}

/// A root Git object class whose literal read is required for admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceRootObjectKindV1 {
    Commit,
    Tree,
}

/// One typed projection mismatch between the raw submission and A0's rebuild.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceMaterialMismatchV1 {
    pub projection: SourceProjectionV1,
    pub missing_keys: Vec<String>,
    pub extra_keys: Vec<String>,
    pub mismatched_keys: Vec<String>,
    pub expected_digest: SourceHash,
    pub actual_digest: SourceHash,
}

/// Material projections that A0 compares independently and in this order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceProjectionV1 {
    Inventory,
    TreeEntries,
    InventoryComplete,
    UnexpandedAncestors,
    ReadMaterial,
    BasisBinding,
    BasisFiles,
    BasisSyntax,
    BaseEntries,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceInputError {
    InvalidRequest,
    InvalidBounds,
    RepositoryUnavailable,
    RepositoryIdentityMismatch,
    RegistryMismatch,
    RootObjectUnavailable {
        side: SourceSnapshotSideV1,
        kind: SourceRootObjectKindV1,
    },
    InvalidGitObject {
        side: SourceSnapshotSideV1,
        object_oid: String,
    },
    CommitTreeMismatch {
        side: SourceSnapshotSideV1,
        expected: String,
        submitted: String,
    },
    SourceMismatch(SourceMaterialMismatchV1),
    ReconstructionFailed,
    /// The target needs exact-pair (C) reconstruction, which this A0 does not
    /// perform yet: a parsed target file contains at least one Call-role syntax
    /// record. No context is created. Removed or narrowed by slice V1b
    ///.
    ExactPairReconstructionUnsupported,
    /// The repository identity could not be derived from the literal target
    /// history (a missing or unreadable ancestor commit, or the policy's
    /// traversal bound was reached). No context is created
    ///.
    RepositoryIdentityUnavailable,
}

impl fmt::Display for SourceInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SourceInputError {}

/// The source-selected domain retained only inside A0's canonical material.
///
/// Every member is copied from literal Git reads and checked policy, rather
/// than retaining a caller's raw request as an authority.  It deliberately
/// has no accessor: the seven context metadata accessors below are the whole
/// public observation surface.
#[derive(Debug, Eq, PartialEq)]
struct AdmittedSourceDomainV1 {
    workspace_admission_root: PathBuf,
    repository_admission_root: PathBuf,
    repository_identity: String,
    base_commit_oid: String,
    base_tree_oid: String,
    target_commit_oid: String,
    target_tree_oid: String,
    registry_binding: TypeScriptRegistryBinding,
    bounds: SourceAdmissionBoundsV1,
}

/// The typed native drafts rebuilt from admitted I1 source material.
///
/// The catalogue and report remain raw values internally.  They exist so the
/// A1--A4 reconstructions can compare source rebuilds without ever receiving
/// a submitted payload or report as their reconstruction input.
#[derive(Debug, Eq, PartialEq)]
struct NativeSourceDraftV1 {
    syntax_catalog: Vec<SyntaxRecord>,
    extraction_report: ExtractionReport,
}

/// The internal, complete canonical view used only for A0's raw comparison.
///
/// It contains the snapshot/policy binding, complete target inventory and
/// read evidence, and every C base variant. It never becomes a public report or
/// a caller-supplied second comparison value.
#[derive(Debug, Eq, PartialEq)]
struct SourceMaterialCanonicalViewV1 {
    admitted_domain: AdmittedSourceDomainV1,
    target_inventory: Vec<SourceInventoryClaimV1>,
    target_tree_entries: Vec<SourceTreeEntryClaimV1>,
    inventory_complete: bool,
    unexpanded_ancestors: Vec<UnexpandedAncestorClaimV1>,
    target_read_material: Vec<SourceReadMaterialClaimV1>,
    target_basis: SourceReviewBasisV1,
    target_sources: AdmittedSourceBundleV1,
    native_draft: NativeSourceDraftV1,
    base_entries: Vec<BaseEntryClaimV1>,
    source_material_binding: SourceHash,
    native_basis_digest: SourceHash,
}

/// Private provenance retained by a reconstruction context.
#[derive(Debug, Eq, PartialEq)]
struct ReconstructionProvenanceV1 {
    snapshot_binding: SnapshotBinding,
    registry_binding: TypeScriptRegistryBinding,
    base_commit_oid: String,
    base_tree_oid: String,
    target_commit_oid: String,
    target_tree_oid: String,
    bounds: SourceAdmissionBoundsV1,
    source_material: SourceMaterialCanonicalViewV1,
}

/// The immutable A0 output used by A1--A4.
///
/// It has no public constructor, fields, conversion, clone, default, map, or
/// caller-provided source loader. The accessors are read-only metadata only;
/// internal source material remains sealed.
#[derive(Debug, Eq, PartialEq)]
pub struct ReconstructionContextV1 {
    provenance: ReconstructionProvenanceV1,
    g3_observations: Result<g3_observe::TypeScriptG3BatchV1, g3_observe::G3AccountingError>,
}

/// Opaque payload rows rebuilt from one private A0 context. Only this module
/// constructs or opens the rows; the parent product module can only ask for
/// metadata and return the entire token for batch sealing.
pub(super) struct A0RebuiltPayloadCatalogueV1 {
    rows: Vec<(SourceFileId, Vec<TypeScriptPayload>)>,
}

impl A0RebuiltPayloadCatalogueV1 {
    pub(super) fn parsed_file_keys(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|(file_id, _)| file_id.canonical_key())
            .collect()
    }

    pub(super) fn parsed_file_count(&self) -> usize {
        self.rows.len()
    }

    fn into_rows(self) -> Vec<(SourceFileId, Vec<TypeScriptPayload>)> {
        self.rows
    }
}

/// The only complete target view passed to the TypeScript payload resolver.
/// It has no public constructor or accessor and borrows the private A0
/// provenance, so a caller cannot turn a submitted inventory into a complete
/// traversal claim.
struct A0PayloadSourceView<'a> {
    sources: &'a AdmittedSourceBundleV1,
    target_basis: &'a SourceReviewBasisV1,
    inventory_complete: bool,
}

impl TypeScriptPayloadSourceViewV1 for A0PayloadSourceView<'_> {
    fn source_bundle(&self) -> &AdmittedSourceBundleV1 {
        self.sources
    }

    fn target_outcome_for_path(&self, canonical_path: &str) -> Option<SourceFileOutcome> {
        self.target_basis
            .files
            .iter()
            .find(|file| file.path == canonical_path)
            .map(|file| file.outcome)
    }

    fn target_inventory_complete(&self) -> bool {
        self.inventory_complete
    }
}

impl ReconstructionContextV1 {
    /// Private admission's immutable Git-byte G3 observations. An accounting
    /// failure remains an error, never an empty successful batch.
    pub(crate) fn g3_observations(
        &self,
    ) -> Result<&g3_observe::TypeScriptG3BatchV1, &g3_observe::G3AccountingError> {
        self.g3_observations.as_ref()
    }

    /// Fail-closed G3 integrity gate of the live TS route. Refuses an accounting error of the batch
    /// (missing/unexpected/duplicate occurrence, double disposition, ID
    /// collision, binding mismatch, …) and any internal inconsistency between
    /// the rows and this admission: foreign snapshot, a row bound to a file,
    /// ID or hash other than the admitted target source, a duplicate
    /// observation ID or exclusion, a range outside its admitted file or LHS
    /// outside its occurrence, a membership whose keys differ from the row's,
    /// or an obstructed outcome whose obstruction row is absent. Per-file and
    /// per-occurrence obstructions are ordinary accounted rows, never a
    /// refusal. It reads no bytes beyond this admission and emits nothing.
    pub(crate) fn check_g3_integrity(&self) -> Result<(), &'static str> {
        use g3_observe::{TypeScriptFileBindingV1, TypeScriptG3OutcomeV1, TypeScriptG3ReasonV1};
        let observations = self.g3_observations();
        let batch = observations.map_err(|_| "G3 accounting inconsistency")?;
        let snapshot = &self.provenance.snapshot_binding;
        if batch.snapshot_binding() != snapshot {
            return Err("G3 snapshot binding");
        }
        let sources = &self.provenance.source_material.target_sources;
        let admitted_length = |binding: &TypeScriptFileBindingV1| -> Result<u64, &'static str> {
            let source = sources
                .file_by_canonical_basis_path(binding.canonical_path())
                .ok_or("G3 row bound to an unadmitted file")?;
            if binding.snapshot_binding() != snapshot
                || *binding.file_id() != source.file_id
                || *binding.source_hash() != source.source_hash
                || SourceHash::from_source_bytes(&source.bytes) != source.source_hash
            {
                return Err("G3 file binding");
            }
            u64::try_from(source.bytes.len()).map_err(|_| "G3 file length")
        };
        let obstruction_present = |outcome: &TypeScriptG3OutcomeV1| match outcome {
            TypeScriptG3OutcomeV1::Obstructed { obstruction_id, .. } => {
                batch.obstruction(obstruction_id).is_some()
            }
            _ => true,
        };
        let mut ids = BTreeSet::new();
        for row in batch.containment() {
            let length = admitted_length(row.binding())?;
            let membership = match row.outcome() {
                TypeScriptG3OutcomeV1::ExistingSyntaxMembership {
                    scope_key,
                    callable_key,
                } => row.scope_key() == Some(scope_key) && row.callable_key() == Some(callable_key),
                _ => true,
            };
            if !ids.insert(row.id().clone())
                || row.parent_range().end() > length
                || row.child_range().end() > length
                || !membership
                || !obstruction_present(row.outcome())
            {
                return Err("G3 containment row");
            }
        }
        for row in batch.writes() {
            let length = admitted_length(row.binding())?;
            let (occurrence, lhs) = (row.occurrence_range(), row.lhs_range());
            let lhs_reason = match row.outcome() {
                TypeScriptG3OutcomeV1::Obstructed {
                    reason: TypeScriptG3ReasonV1::UnsupportedAssignmentLhs(kind),
                    ..
                } => kind == row.lhs_kind(),
                _ => true,
            };
            if !ids.insert(row.id().clone())
                || occurrence.end() > length
                || lhs.start() < occurrence.start()
                || lhs.end() > occurrence.end()
                || !lhs_reason
                || !obstruction_present(row.outcome())
            {
                return Err("G3 write row");
            }
        }
        for row in batch.test_marker_candidates() {
            let length = admitted_length(row.binding())?;
            // An accepted call syntax key exists exactly for the Unknown outcome.
            let keyed = matches!(row.outcome(), TypeScriptG3OutcomeV1::Unknown(_));
            if !ids.insert(row.id().clone())
                || row.occurrence_range().end() > length
                || row.call_key().is_some() != keyed
                || !obstruction_present(row.outcome())
            {
                return Err("G3 test-marker candidate row");
            }
        }
        // Every obstructed containment/write row and every test-marker
        // candidate inserted exactly one distinct obstruction row.
        let obstructed = |outcome: &TypeScriptG3OutcomeV1| {
            matches!(outcome, TypeScriptG3OutcomeV1::Obstructed { .. })
        };
        let accounted = batch
            .containment()
            .iter()
            .filter(|row| obstructed(row.outcome()))
            .count()
            + batch
                .writes()
                .iter()
                .filter(|row| obstructed(row.outcome()))
                .count()
            + batch.test_marker_candidates().len();
        if batch.obstructions().len() < accounted {
            return Err("G3 obstruction rows missing");
        }
        let mut files = BTreeSet::new();
        for row in batch.file_partition() {
            if !files.insert(format!("{row:?}")) {
                return Err("G3 duplicate file partition row");
            }
        }
        let mut exclusions = BTreeSet::new();
        for exclusion in batch.construct_partition().exclusions() {
            if !exclusions.insert(format!("{exclusion:?}")) {
                return Err("G3 duplicate exclusion");
            }
        }
        Ok(())
    }

    /// Returns the A0-verified snapshot binding without exposing its source
    /// material or any mutable authority.
    #[must_use]
    pub fn snapshot_binding(&self) -> &SnapshotBinding {
        &self.provenance.snapshot_binding
    }

    /// Returns the A0-verified registry binding without exposing a context
    /// constructor or a raw source projection.
    #[must_use]
    pub fn registry_binding(&self) -> &TypeScriptRegistryBinding {
        &self.provenance.registry_binding
    }

    /// Returns the literal base commit A0 read.
    #[must_use]
    pub fn base_commit_oid(&self) -> &str {
        &self.provenance.base_commit_oid
    }

    /// Returns the literal base tree A0 read.
    #[must_use]
    pub fn base_tree_oid(&self) -> &str {
        &self.provenance.base_tree_oid
    }

    /// Returns the literal target commit A0 read.
    #[must_use]
    pub fn target_commit_oid(&self) -> &str {
        &self.provenance.target_commit_oid
    }

    /// Returns the literal target tree that A0 read from the selected target
    /// commit. It is used by the replace-ref acceptance fixture.
    #[must_use]
    pub fn target_tree_oid(&self) -> &str {
        &self.provenance.target_tree_oid
    }

    /// Returns A0-checked caller bounds without exposing source material.
    #[must_use]
    pub fn bounds(&self) -> &SourceAdmissionBoundsV1 {
        &self.provenance.bounds
    }

    /// Rebuilds the complete A1 payload catalogue through the private A0
    /// carrier. The opaque result is the only input accepted by the private
    /// batch sealer; no caller draft, bundle, or completeness claim crosses
    /// this boundary.
    pub(super) fn rebuild_payload_catalogue_from_a0(
        &self,
    ) -> Result<A0RebuiltPayloadCatalogueV1, PayloadError> {
        rebuild_payload_rows_from_provenance(&self.provenance)
            .map(|rows| A0RebuiltPayloadCatalogueV1 { rows })
    }

    /// Seals each already A0-rebuilt row without accepting any caller draft.
    /// This is runtime-private and consumes the opaque catalogue, so only
    /// `rebuild_payload_catalogue_from_a0` can provide its payloads.
    pub(super) fn seal_rebuilt_payload_catalogue_from_a0(
        &self,
        catalogue: A0RebuiltPayloadCatalogueV1,
    ) -> Vec<(SourceFileId, SourceValidated<TypeScriptPayload>)> {
        let provenance = self.payload_validation_provenance();
        catalogue
            .into_rows()
            .into_iter()
            .flat_map(|(file_id, payloads)| {
                payloads.into_iter().map({
                    let provenance = provenance.clone();
                    move |payload| {
                        (
                            file_id.clone(),
                            SourceValidated::from_reconstructed(payload, provenance.clone()),
                        )
                    }
                })
            })
            .collect()
    }

    /// Rebuilds raw payloads for the externally callable raw-submission
    /// comparison. This intentionally remains separate from product batch
    /// sealing, which consumes the opaque A0 catalogue above.
    fn rebuild_payloads_from_a0(
        &self,
    ) -> Result<Vec<(SourceFileId, Vec<TypeScriptPayload>)>, PayloadError> {
        self.rebuild_payload_catalogue_from_a0()
            .map(A0RebuiltPayloadCatalogueV1::into_rows)
    }

    fn payload_validation_provenance(&self) -> SourceValidationProvenanceV1 {
        SourceValidationProvenanceV1 {
            snapshot_binding: self.provenance.snapshot_binding.clone(),
            registry_binding: self.provenance.registry_binding.clone(),
            source_material_binding: self
                .provenance
                .source_material
                .source_material_binding
                .clone(),
            native_basis_digest: self.provenance.source_material.native_basis_digest.clone(),
            owner_binding: SourceValidationOwnerV1::Payload,
        }
    }
}

/// One unchanged A1 reconstruction expression shared by the existing v5 path
/// and the private G3 join. Its complete-inventory view belongs to A0.
fn rebuild_payload_rows_from_provenance(
    provenance: &ReconstructionProvenanceV1,
) -> Result<Vec<(SourceFileId, Vec<TypeScriptPayload>)>, PayloadError> {
    let material = &provenance.source_material;
    let source_view = A0PayloadSourceView {
        sources: &material.target_sources,
        target_basis: &material.target_basis,
        inventory_complete: material.inventory_complete,
    };
    source_view
        .source_bundle()
        .files()
        .iter()
        .map(|source| {
            rebuild_payload_from_source(
                &source_view,
                &provenance.registry_binding,
                &provenance.snapshot_binding,
                source.file_id.clone(),
            )
            .map(|payloads| (source.file_id.clone(), payloads))
        })
        .collect::<Result<Vec<_>, _>>()
}

/// The private provenance attached to every capability.
///
/// It binds the A0 snapshot, registry, source-material binding, native basis
/// digest, and owner binding. Implementations must carry the same binding from
/// A3 through A4, A5, D1, and A6.
#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceValidationProvenanceV1 {
    snapshot_binding: SnapshotBinding,
    registry_binding: TypeScriptRegistryBinding,
    source_material_binding: SourceHash,
    native_basis_digest: SourceHash,
    owner_binding: SourceValidationOwnerV1,
}

/// The concrete source-owned value a capability provenance binds.
///
/// This private tag prevents A3/A4/A5/D1/A6 provenance from degenerating into
/// an empty marker while keeping the public capability vocabulary unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Constructed by concrete A1--A5/D1 bodies in the implementation slice.
enum SourceValidationOwnerV1 {
    Payload,
    Catalog,
    Extraction,
    Ingestion,
    SynthesisInput,
    ObligationSet,
}

mod sealed {
    use super::SourceValidationProvenanceV1;

    /// A source-reconstructed value that only this facade may create.
    ///
    /// Fields are private; this type deliberately implements neither `Clone`,
    /// `Copy`, `Default`, `From`, `Into`, mapping, mutable dereference, nor a
    /// public constructor. `T` is limited by the facade's seven concrete
    /// producers, and never recursively contains this capability
    ///.
    #[derive(Debug, Eq, PartialEq)]
    pub struct SourceValidated<T> {
        value: T,
        provenance: SourceValidationProvenanceV1,
    }

    impl<T> SourceValidated<T> {
        /// Constructs one capability only for the concrete A1--A5 and D1
        /// admissions in this crate. This is deliberately visible to the
        /// enclosing facade, but not to downstream crates; it is not a public
        /// generic mint.
        pub(super) fn from_reconstructed(
            _value: T,
            _provenance: SourceValidationProvenanceV1,
        ) -> Self {
            Self {
                value: _value,
                provenance: _provenance,
            }
        }

        /// Borrows the reconstructed value without exposing its provenance for
        /// mutation or creating a second capability.
        #[must_use]
        #[allow(
            clippy::should_implement_trait,
            reason = "an AsRef implementation would add a public generic trait surface to this sealed capability"
        )]
        pub fn as_ref(&self) -> &T {
            &self.value
        }

        /// Consumes the capability and returns only its raw value. The caller
        /// cannot turn that value back into `SourceValidated` without A1--A6
        /// or D1.
        #[must_use]
        pub fn into_inner(self) -> T {
            self.value
        }
    }
}

pub use sealed::SourceValidated;

/// Opens the one source authority context for A1--A4.
///
/// A0 must read literal commit, tree, and blob objects from the admitted
/// repository and rebuild all eight comparison rows without treating submitted
/// material as authority. Its literal object reads must ignore `refs/replace`
/// and environment replacement settings. It selects full commit/tree OIDs
/// directly and performs no graft-history traversal; the mere existence of a
/// graft is not a distinct rejection condition.
#[allow(
    clippy::result_large_err,
    reason = "the frozen public A0 error contract carries detailed source mismatches unboxed"
)]
pub fn admit_reconstruction_context(
    submitted: SourceAdmissionSubmissionV1,
) -> Result<ReconstructionContextV1, SourceInputError> {
    let source_material = load_literal_git_objects(&submitted.request, IdentityAdmission::Verify)?;
    compare_source_material(&source_material, &submitted.material)?;
    let domain = &source_material.admitted_domain;
    let snapshot_binding =
        SnapshotBinding::from_admitted_binding(&snapshot_binding_preimage(domain));
    let provenance = ReconstructionProvenanceV1 {
        snapshot_binding,
        registry_binding: domain.registry_binding.clone(),
        base_commit_oid: domain.base_commit_oid.clone(),
        base_tree_oid: domain.base_tree_oid.clone(),
        target_commit_oid: domain.target_commit_oid.clone(),
        target_tree_oid: domain.target_tree_oid.clone(),
        bounds: domain.bounds.clone(),
        source_material,
    };
    let g3_observations = g3_observe::observe(&provenance);
    Ok(ReconstructionContextV1 {
        provenance,
        g3_observations,
    })
}

/// Builds one raw A0 submission by reading literal Git source selected by
/// `request`. This is an input stimulus, not an admission: it returns neither
/// a context nor a `SourceValidated` value, and A0 must read all objects again
/// before it accepts the result.
#[allow(
    clippy::result_large_err,
    reason = "the frozen public A0 error contract carries detailed source mismatches unboxed"
)]
pub fn build_source_admission_submission_from_git(
    request: SourceAdmissionRequestV1,
) -> Result<SourceAdmissionSubmissionV1, SourceInputError> {
    let source_material = load_literal_git_objects(&request, IdentityAdmission::RawSubmission)?;
    Ok(SourceAdmissionSubmissionV1 {
        request,
        material: SourceAdmissionMaterialV1 {
            target_basis: source_material.target_basis.clone(),
            target_inventory: source_material.target_inventory.clone(),
            target_tree_entries: source_material.target_tree_entries.clone(),
            inventory_complete: source_material.inventory_complete,
            unexpanded_ancestors: source_material.unexpanded_ancestors.clone(),
            target_read_material: source_material.target_read_material.clone(),
            base_entries: source_material.base_entries.clone(),
        },
    })
}

/// Builds the current product's raw A0 comparison submission from literal Git
/// objects, then immediately seals it through A0. The request is synthesized
/// from literal commit/tree identities; product callers cannot provide source
/// material, inventory, or traversal state.
#[allow(
    clippy::result_large_err,
    reason = "the frozen public A0 error contract carries detailed source mismatches unboxed"
)]
pub fn admit_typescript_revision_pair(
    workspace_admission_root: PathBuf,
    repository_admission_root: PathBuf,
    base_commit_oid: String,
    target_commit_oid: String,
    bounds: SourceAdmissionBoundsV1,
) -> Result<(SourceAdmissionSubmissionV1, ReconstructionContextV1), SourceInputError> {
    let base_tree_oid = read_commit_tree(
        &repository_admission_root,
        &base_commit_oid,
        SourceSnapshotSideV1::Base,
    )?;
    let target_tree_oid = read_commit_tree(
        &repository_admission_root,
        &target_commit_oid,
        SourceSnapshotSideV1::Target,
    )?;
    let repository_identity =
        derive_target_root_set_identity(&repository_admission_root, &target_commit_oid)?;
    let request = SourceAdmissionRequestV1 {
        workspace_admission_root,
        repository_admission_root,
        repository_identity,
        base_commit_oid,
        base_tree_oid,
        target_commit_oid,
        target_tree_oid,
        registry_binding: typescript_registry_binding(),
        bounds,
    };
    let submission = build_source_admission_submission_from_git(request)?;
    let context = admit_reconstruction_context(submission.clone())?;
    Ok((submission, context))
}

/// Reads literal Git objects for A0 after repository admission.
///
/// The implementation must use the established Git command policy with
/// replacement objects disabled and an environment that does not inherit
/// `GIT_REPLACE_REF_BASE`. It uses literal object IDs rather than a
/// graft-dependent history traversal; it does not reject a repository merely
/// because graft files exist. This internal seam accepts no caller closure or
/// provider.
#[derive(Clone, Copy, Eq, PartialEq)]
enum IdentityAdmission {
    Verify,
    RawSubmission,
}

#[allow(
    clippy::result_large_err,
    reason = "the frozen A0 error contract carries detailed source mismatches unboxed"
)]
fn load_literal_git_objects(
    request: &SourceAdmissionRequestV1,
    identity_admission: IdentityAdmission,
) -> Result<SourceMaterialCanonicalViewV1, SourceInputError> {
    let (workspace_admission_root, repository_admission_root) = admit_repository(request)?;
    if validate_typescript_binding(&request.registry_binding).is_err() {
        return Err(SourceInputError::RegistryMismatch);
    }
    if request.bounds.max_files == 0
        || request.bounds.max_file_bytes == 0
        || request.bounds.max_total_source_bytes == 0
    {
        return Err(SourceInputError::InvalidBounds);
    }

    let base_tree_oid = read_commit_tree(
        &repository_admission_root,
        &request.base_commit_oid,
        SourceSnapshotSideV1::Base,
    )?;
    check_claimed_tree(
        SourceSnapshotSideV1::Base,
        &base_tree_oid,
        &request.base_tree_oid,
    )?;
    read_root_tree(
        &repository_admission_root,
        &base_tree_oid,
        SourceSnapshotSideV1::Base,
    )?;

    let target_tree_oid = read_commit_tree(
        &repository_admission_root,
        &request.target_commit_oid,
        SourceSnapshotSideV1::Target,
    )?;
    check_claimed_tree(
        SourceSnapshotSideV1::Target,
        &target_tree_oid,
        &request.target_tree_oid,
    )?;
    read_root_tree(
        &repository_admission_root,
        &target_tree_oid,
        SourceSnapshotSideV1::Target,
    )?;

    let repository_identity = match identity_admission {
        IdentityAdmission::Verify => {
            validate_repository_identity_claim(&request.repository_identity)?;
            let derived = derive_target_root_set_identity(
                &repository_admission_root,
                &request.target_commit_oid,
            )?;
            if derived != request.repository_identity {
                return Err(SourceInputError::RepositoryIdentityMismatch);
            }
            derived
        }
        IdentityAdmission::RawSubmission => String::new(),
    };

    let mut tree_entries = Vec::new();
    let mut inventory = Vec::new();
    let mut traversal = TargetTreeTraversal::complete();
    collect_target_tree(
        &repository_admission_root,
        &target_tree_oid,
        "",
        &mut tree_entries,
        &mut inventory,
        &mut traversal,
    )?;
    tree_entries.sort_by(|left, right| left.path.cmp(&right.path));
    inventory.sort_by(|left, right| left.path.cmp(&right.path));
    traversal.sort();

    let mut target_inventory = Vec::with_capacity(inventory.len());
    let mut target_read_material = Vec::new();
    let mut parsed_sources = Vec::new();
    let mut syntax_roles = Vec::new();
    let mut read_files = 0_usize;
    let mut total_source_bytes = 0_usize;
    for entry in inventory {
        let classification = classify_typescript_v1_path(&entry.path)
            .map_err(|_| SourceInputError::InvalidRequest)?;
        let (profile, outcome, read) = match (classification, entry.entry_kind) {
            (_, kind) if kind != SourceInventoryEntryKindV1::Blob => (
                profile_claim_for_path(&entry.path)?,
                SourceFileOutcome::UnsupportedEntry,
                SourceReadClaimV1::NotRead {
                    reason: SourceNotReadReasonV1::UnsupportedEntry,
                },
            ),
            (TypeScriptPathClassification::NonTargetExtension, _) => (
                SourceProfileClaimV1::NonTargetExtension,
                SourceFileOutcome::NonTargetExtension,
                SourceReadClaimV1::NotRead {
                    reason: SourceNotReadReasonV1::NonTargetExtension,
                },
            ),
            (TypeScriptPathClassification::ProfileExcluded(reason), _) => (
                SourceProfileClaimV1::Excluded {
                    reasons: vec![reason.to_owned()],
                    primary_reason: reason.to_owned(),
                },
                SourceFileOutcome::ProfileExcluded,
                SourceReadClaimV1::NotRead {
                    reason: SourceNotReadReasonV1::ProfileExcluded,
                },
            ),
            (TypeScriptPathClassification::Included, _)
                if read_files >= request.bounds.max_files =>
            {
                (
                    SourceProfileClaimV1::Included,
                    SourceFileOutcome::UnreadBound,
                    SourceReadClaimV1::NotRead {
                        reason: SourceNotReadReasonV1::FileCountBound,
                    },
                )
            }
            (TypeScriptPathClassification::Included, _) => {
                let byte_count = literal_object_size(&repository_admission_root, &entry.object_oid);
                if let Err(cause) = byte_count {
                    let prefix = Vec::new();
                    target_read_material.push(SourceReadMaterialClaimV1 {
                        path: entry.path.clone(),
                        hash: SourceHash::from_source_bytes(&prefix),
                        bytes: prefix,
                        extent: SourceReadExtentV1::InterruptedPrefix,
                    });
                    (
                        SourceProfileClaimV1::Included,
                        SourceFileOutcome::UnreadBound,
                        SourceReadClaimV1::Failed {
                            byte_count: 0,
                            prefix_hash: SourceHash::from_source_bytes(&[]),
                            cause,
                        },
                    )
                } else if byte_count.expect("handled error above") > request.bounds.max_file_bytes {
                    (
                        SourceProfileClaimV1::Included,
                        SourceFileOutcome::UnreadBound,
                        SourceReadClaimV1::NotRead {
                            reason: SourceNotReadReasonV1::FileByteBound,
                        },
                    )
                } else if total_source_bytes
                    .saturating_add(byte_count.expect("handled error above"))
                    > request.bounds.max_total_source_bytes
                {
                    (
                        SourceProfileClaimV1::Included,
                        SourceFileOutcome::UnreadBound,
                        SourceReadClaimV1::NotRead {
                            reason: SourceNotReadReasonV1::TotalSourceByteBound,
                        },
                    )
                } else {
                    let object =
                        match read_literal_object(&repository_admission_root, &entry.object_oid) {
                            Ok(object) => object,
                            Err(LiteralObjectReadError::Unavailable(cause)) => {
                                let prefix = Vec::new();
                                target_read_material.push(SourceReadMaterialClaimV1 {
                                    path: entry.path.clone(),
                                    hash: SourceHash::from_source_bytes(&prefix),
                                    bytes: prefix,
                                    extent: SourceReadExtentV1::InterruptedPrefix,
                                });
                                target_inventory.push(SourceInventoryClaimV1 {
                                    path: entry.path,
                                    object_oid: entry.object_oid,
                                    mode: entry.mode,
                                    entry_kind: entry.entry_kind,
                                    profile: SourceProfileClaimV1::Included,
                                    outcome: SourceFileOutcome::UnreadBound,
                                    read: SourceReadClaimV1::Failed {
                                        byte_count: 0,
                                        prefix_hash: SourceHash::from_source_bytes(&[]),
                                        cause,
                                    },
                                });
                                continue;
                            }
                            Err(LiteralObjectReadError::Invalid) => {
                                return Err(SourceInputError::InvalidGitObject {
                                    side: SourceSnapshotSideV1::Target,
                                    object_oid: entry.object_oid.clone(),
                                });
                            }
                        };
                    if object.kind != "blob" {
                        return Err(SourceInputError::InvalidGitObject {
                            side: SourceSnapshotSideV1::Target,
                            object_oid: entry.object_oid.clone(),
                        });
                    }
                    read_files += 1;
                    total_source_bytes += object.body.len();
                    let source_hash = SourceHash::from_source_bytes(&object.body);
                    let parsed = parse_typescript(&entry.path, &object.body);
                    let outcome = if parsed.is_parsed() {
                        syntax_roles.push((
                            entry.path.clone(),
                            if parsed.callables.is_empty() {
                                vec![SourceSyntaxRole::Scope]
                            } else {
                                vec![SourceSyntaxRole::Callable, SourceSyntaxRole::Scope]
                            },
                        ));
                        parsed_sources.push((
                            entry.path.clone(),
                            object.body.clone(),
                            source_hash.clone(),
                        ));
                        SourceFileOutcome::Parsed
                    } else {
                        SourceFileOutcome::ParseFailed
                    };
                    target_read_material.push(SourceReadMaterialClaimV1 {
                        path: entry.path.clone(),
                        bytes: object.body.clone(),
                        hash: source_hash.clone(),
                        extent: SourceReadExtentV1::FullBlob,
                    });
                    (
                        SourceProfileClaimV1::Included,
                        outcome,
                        SourceReadClaimV1::Complete {
                            byte_count: u64::try_from(object.body.len())
                                .map_err(|_| SourceInputError::InvalidBounds)?,
                            source_hash,
                        },
                    )
                }
            }
        };
        target_inventory.push(SourceInventoryClaimV1 {
            path: entry.path,
            object_oid: entry.object_oid,
            mode: entry.mode,
            entry_kind: entry.entry_kind,
            profile,
            outcome,
            read,
        });
    }

    let target_basis = SourceReviewBasisV1::new(
        typescript_registry_binding(),
        target_inventory
            .iter()
            .map(|entry| SourceReviewFileV1 {
                path: entry.path.clone(),
                language: typescript_registry_binding().tuple.language,
                outcome: entry.outcome,
            })
            .collect(),
        syntax_roles
            .into_iter()
            .flat_map(|(path, roles)| {
                roles.into_iter().map(move |role| SourceReviewSyntaxV1 {
                    file_path: path.clone(),
                    role,
                })
            })
            .collect(),
    )
    .map_err(|_| SourceInputError::ReconstructionFailed)?;
    let target_sources = AdmittedSourceBundleV1::new(
        &target_basis,
        parsed_sources
            .into_iter()
            .map(|(path, bytes, source_hash)| {
                let file_key = CanonicalFileKey::from_basis_path(&path)
                    .expect("target paths were admitted before source-bundle construction");
                reviewgraphen_core::source_review::admitted_source::AdmittedSourceFileV1 {
                    file_id: SourceFileId::from_basis_file_key(file_key),
                    bytes,
                    source_hash,
                }
            })
            .collect(),
    )
    .map_err(|_| SourceInputError::ReconstructionFailed)?;

    let admitted_domain = AdmittedSourceDomainV1 {
        workspace_admission_root,
        repository_admission_root,
        repository_identity,
        base_commit_oid: request.base_commit_oid.clone(),
        base_tree_oid,
        target_commit_oid: request.target_commit_oid.clone(),
        target_tree_oid,
        registry_binding: typescript_registry_binding(),
        bounds: request.bounds.clone(),
    };
    let source_material_binding = source_hash_for_debug(&(
        &target_inventory,
        &tree_entries,
        traversal.inventory_complete,
        &traversal.unexpanded_ancestors,
        &target_read_material,
    ));
    let native_basis_digest = source_hash_for_debug(&target_basis);
    let native_draft = NativeSourceDraftV1 {
        syntax_catalog: Vec::new(),
        extraction_report: ExtractionReport {
            registry_binding: typescript_registry_binding(),
            files: Vec::new(),
            syntax: Vec::new(),
            inventory_complete: traversal.inventory_complete,
        },
    };
    Ok(SourceMaterialCanonicalViewV1 {
        admitted_domain,
        target_inventory,
        target_tree_entries: tree_entries,
        inventory_complete: traversal.inventory_complete,
        unexpanded_ancestors: traversal.unexpanded_ancestors,
        target_read_material,
        target_basis,
        target_sources,
        native_draft,
        base_entries: Vec::new(),
        source_material_binding,
        native_basis_digest,
    })
}

struct LiteralGitObject {
    kind: String,
    body: Vec<u8>,
}

struct LiteralInventoryEntry {
    path: String,
    object_oid: String,
    mode: u32,
    entry_kind: SourceInventoryEntryKindV1,
}

struct LiteralTreeEntry {
    mode: u32,
    entry_kind: Option<SourceInventoryEntryKindV1>,
    object_oid: String,
    name: String,
}

const IDENTITY_MAX_COMMITS: usize = 100_000;
const IDENTITY_MAX_COMMIT_BYTES: usize = 64 * 1024 * 1024;
const LITERAL_GIT_TIMEOUT: Duration = Duration::from_secs(30);
const LITERAL_GIT_MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const LITERAL_GIT_POLL_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiteralGitCommandError {
    Io,
    Timeout,
    OutputBound,
    Exit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiteralObjectReadError {
    Unavailable(SourceIoFailureV1),
    Invalid,
}

#[derive(Debug)]
struct TargetTreeTraversal {
    inventory_complete: bool,
    unexpanded_ancestors: Vec<UnexpandedAncestorClaimV1>,
}

impl TargetTreeTraversal {
    fn complete() -> Self {
        Self {
            inventory_complete: true,
            unexpanded_ancestors: Vec::new(),
        }
    }

    fn record_tree_failure(&mut self, path: String, object_oid: String, cause: SourceIoFailureV1) {
        self.inventory_complete = false;
        self.unexpanded_ancestors.push(UnexpandedAncestorClaimV1 {
            path,
            object_oid,
            cause: SourceAncestorCauseV1::TreeReadFailed(cause),
        });
    }

    fn record_nonregular(
        &mut self,
        path: String,
        object_oid: String,
        entry_kind: SourceInventoryEntryKindV1,
    ) {
        let cause = match entry_kind {
            SourceInventoryEntryKindV1::Symlink => SourceAncestorCauseV1::Symlink,
            SourceInventoryEntryKindV1::Submodule => SourceAncestorCauseV1::Submodule,
            SourceInventoryEntryKindV1::Blob => return,
        };
        self.unexpanded_ancestors.push(UnexpandedAncestorClaimV1 {
            path,
            object_oid,
            cause,
        });
    }

    fn sort(&mut self) {
        self.unexpanded_ancestors.sort_by(|left, right| {
            (&left.path, &left.object_oid, format!("{:?}", left.cause)).cmp(&(
                &right.path,
                &right.object_oid,
                format!("{:?}", right.cause),
            ))
        });
        self.unexpanded_ancestors.dedup();
    }
}

#[allow(
    clippy::result_large_err,
    reason = "the frozen A0 error contract carries detailed source mismatches unboxed"
)]
fn admit_repository(
    request: &SourceAdmissionRequestV1,
) -> Result<(PathBuf, PathBuf), SourceInputError> {
    if request.repository_identity.is_empty()
        || !is_full_object_oid(&request.base_commit_oid)
        || !is_full_object_oid(&request.target_commit_oid)
        || !is_full_object_oid(&request.base_tree_oid)
        || !is_full_object_oid(&request.target_tree_oid)
    {
        return Err(SourceInputError::InvalidRequest);
    }
    let workspace = request
        .workspace_admission_root
        .canonicalize()
        .map_err(|_| SourceInputError::RepositoryUnavailable)?;
    let repository = request
        .repository_admission_root
        .canonicalize()
        .map_err(|_| SourceInputError::RepositoryUnavailable)?;
    if !repository.starts_with(&workspace) {
        return Err(SourceInputError::InvalidRequest);
    }
    let git_root = literal_git_text(&repository, &["rev-parse", "--show-toplevel"], &[])
        .map_err(|_| SourceInputError::RepositoryUnavailable)?;
    let git_root = PathBuf::from(git_root)
        .canonicalize()
        .map_err(|_| SourceInputError::RepositoryUnavailable)?;
    if git_root != repository {
        return Err(SourceInputError::RepositoryUnavailable);
    }
    Ok((workspace, repository))
}

#[allow(clippy::result_large_err)]
fn validate_repository_identity_claim(value: &str) -> Result<(), SourceInputError> {
    let identity = value
        .strip_prefix("git.target-root-set@1:")
        .ok_or(SourceInputError::InvalidRequest)?;
    let (object_format, roots) = identity
        .split_once(':')
        .ok_or(SourceInputError::InvalidRequest)?;
    if roots.is_empty() || !matches!(object_format, "sha1" | "sha256") {
        return Err(SourceInputError::InvalidRequest);
    }
    let roots = roots.split(',').collect::<Vec<_>>();
    if roots
        .iter()
        .any(|root| !is_object_oid_for_format(root, object_format))
        || roots.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(SourceInputError::InvalidRequest);
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
fn derive_target_root_set_identity(
    root: &Path,
    target_commit_oid: &str,
) -> Result<String, SourceInputError> {
    let object_format = literal_git_text(root, &["rev-parse", "--show-object-format"], &[])
        .map_err(|_| SourceInputError::RepositoryIdentityUnavailable)?;
    if !matches!(object_format.as_str(), "sha1" | "sha256") {
        return Err(SourceInputError::RepositoryIdentityUnavailable);
    }

    let mut pending = vec![target_commit_oid.to_owned()];
    let mut visited = BTreeSet::new();
    let mut roots = BTreeSet::new();
    let mut bytes_read = 0_usize;
    while let Some(commit_oid) = pending.pop() {
        if !visited.insert(commit_oid.clone()) {
            continue;
        }
        if visited.len() > IDENTITY_MAX_COMMITS {
            return Err(SourceInputError::RepositoryIdentityUnavailable);
        }
        let object = match read_literal_object(root, &commit_oid) {
            Ok(object) => object,
            Err(LiteralObjectReadError::Unavailable(_)) => {
                return Err(SourceInputError::RepositoryIdentityUnavailable);
            }
            Err(LiteralObjectReadError::Invalid) => {
                return Err(SourceInputError::InvalidGitObject {
                    side: SourceSnapshotSideV1::Target,
                    object_oid: commit_oid,
                });
            }
        };
        if object.kind != "commit" {
            return Err(SourceInputError::InvalidGitObject {
                side: SourceSnapshotSideV1::Target,
                object_oid: commit_oid,
            });
        }
        bytes_read = bytes_read
            .checked_add(object.body.len())
            .ok_or(SourceInputError::RepositoryIdentityUnavailable)?;
        if bytes_read > IDENTITY_MAX_COMMIT_BYTES {
            return Err(SourceInputError::RepositoryIdentityUnavailable);
        }
        let parents = commit_parent_oids(&object.body, &object_format).ok_or(
            SourceInputError::InvalidGitObject {
                side: SourceSnapshotSideV1::Target,
                object_oid: commit_oid.clone(),
            },
        )?;
        if parents.is_empty() {
            roots.insert(commit_oid);
        } else {
            pending.extend(parents);
        }
    }
    if roots.is_empty() {
        return Err(SourceInputError::RepositoryIdentityUnavailable);
    }
    Ok(format!(
        "git.target-root-set@1:{object_format}:{}",
        roots.into_iter().collect::<Vec<_>>().join(",")
    ))
}

fn commit_parent_oids(body: &[u8], object_format: &str) -> Option<Vec<String>> {
    let mut has_tree = false;
    let mut parents = Vec::new();
    for line in body
        .split(|byte| *byte == b'\n')
        .take_while(|line| !line.is_empty())
    {
        if let Some(tree) = line.strip_prefix(b"tree ") {
            if has_tree || !is_object_oid_for_format(std::str::from_utf8(tree).ok()?, object_format)
            {
                return None;
            }
            has_tree = true;
        } else if let Some(parent) = line.strip_prefix(b"parent ") {
            let parent = std::str::from_utf8(parent).ok()?;
            if !is_object_oid_for_format(parent, object_format) {
                return None;
            }
            parents.push(parent.to_owned());
        }
    }
    has_tree.then_some(parents)
}

fn snapshot_binding_preimage(domain: &AdmittedSourceDomainV1) -> String {
    let binding = &domain.registry_binding;
    let tuple = &binding.tuple;
    let tuple_hash = registry_tuple_hash(binding);
    [
        ("repository_identity", domain.repository_identity.as_str()),
        ("base_commit_oid", domain.base_commit_oid.as_str()),
        ("base_tree_oid", domain.base_tree_oid.as_str()),
        ("target_commit_oid", domain.target_commit_oid.as_str()),
        ("target_tree_oid", domain.target_tree_oid.as_str()),
        ("registry_id", binding.registry_id.as_str()),
        ("registry_hash", binding.registry_hash.as_str()),
        ("arm_id", binding.arm_id.as_str()),
        ("arm_hash", binding.arm_hash.as_str()),
        ("tuple_hash", tuple_hash.as_str()),
        ("profile_id", tuple.profile_id.as_str()),
        ("profile_version", tuple.profile_version.as_str()),
        ("language", tuple.language.as_str()),
        ("producer_id", tuple.producer_id.as_str()),
        ("extractor_set_hash", tuple.extractor_set_hash.as_str()),
        ("rule_set_hash", tuple.rule_set_hash.as_str()),
        ("projection_id", tuple.projection_id.as_str()),
    ]
    .into_iter()
    .map(|(name, value)| format!("{name}={}:{}", value.len(), value))
    .collect::<Vec<_>>()
    .join(";")
}

#[allow(
    clippy::result_large_err,
    reason = "the frozen A0 error contract carries detailed source mismatches unboxed"
)]
fn check_claimed_tree(
    side: SourceSnapshotSideV1,
    expected: &str,
    submitted: &str,
) -> Result<(), SourceInputError> {
    if expected == submitted {
        Ok(())
    } else {
        Err(SourceInputError::CommitTreeMismatch {
            side,
            expected: expected.to_owned(),
            submitted: submitted.to_owned(),
        })
    }
}

#[allow(
    clippy::result_large_err,
    reason = "the frozen A0 error contract carries detailed source mismatches unboxed"
)]
fn read_commit_tree(
    root: &Path,
    commit_oid: &str,
    side: SourceSnapshotSideV1,
) -> Result<String, SourceInputError> {
    let object = match read_literal_object(root, commit_oid) {
        Ok(object) => object,
        Err(LiteralObjectReadError::Unavailable(_)) => {
            return Err(SourceInputError::RootObjectUnavailable {
                side,
                kind: SourceRootObjectKindV1::Commit,
            });
        }
        Err(LiteralObjectReadError::Invalid) => {
            return Err(SourceInputError::InvalidGitObject {
                side,
                object_oid: commit_oid.to_owned(),
            });
        }
    };
    if object.kind != "commit" {
        return Err(SourceInputError::InvalidGitObject {
            side,
            object_oid: commit_oid.to_owned(),
        });
    }
    commit_tree_oid(&object.body).ok_or(SourceInputError::InvalidGitObject {
        side,
        object_oid: commit_oid.to_owned(),
    })
}

#[allow(
    clippy::result_large_err,
    reason = "the frozen A0 error contract carries detailed source mismatches unboxed"
)]
fn read_root_tree(
    root: &Path,
    tree_oid: &str,
    side: SourceSnapshotSideV1,
) -> Result<(), SourceInputError> {
    let object = match read_literal_object(root, tree_oid) {
        Ok(object) => object,
        Err(LiteralObjectReadError::Unavailable(_)) => {
            return Err(SourceInputError::RootObjectUnavailable {
                side,
                kind: SourceRootObjectKindV1::Tree,
            });
        }
        Err(LiteralObjectReadError::Invalid) => {
            return Err(SourceInputError::InvalidGitObject {
                side,
                object_oid: tree_oid.to_owned(),
            });
        }
    };
    if object.kind == "tree" {
        Ok(())
    } else {
        Err(SourceInputError::InvalidGitObject {
            side,
            object_oid: tree_oid.to_owned(),
        })
    }
}

#[allow(
    clippy::result_large_err,
    reason = "the frozen A0 error contract carries detailed source mismatches unboxed"
)]
fn collect_target_tree(
    root: &Path,
    tree_oid: &str,
    prefix: &str,
    tree_entries: &mut Vec<SourceTreeEntryClaimV1>,
    inventory: &mut Vec<LiteralInventoryEntry>,
    traversal: &mut TargetTreeTraversal,
) -> Result<(), SourceInputError> {
    let object = match read_literal_object(root, tree_oid) {
        Ok(object) => object,
        Err(LiteralObjectReadError::Unavailable(cause)) if !prefix.is_empty() => {
            traversal.record_tree_failure(prefix.to_owned(), tree_oid.to_owned(), cause);
            return Ok(());
        }
        Err(LiteralObjectReadError::Unavailable(_)) => {
            return Err(SourceInputError::RootObjectUnavailable {
                side: SourceSnapshotSideV1::Target,
                kind: SourceRootObjectKindV1::Tree,
            });
        }
        Err(LiteralObjectReadError::Invalid) => {
            return Err(SourceInputError::InvalidGitObject {
                side: SourceSnapshotSideV1::Target,
                object_oid: tree_oid.to_owned(),
            });
        }
    };
    if object.kind != "tree" {
        return Err(SourceInputError::InvalidGitObject {
            side: SourceSnapshotSideV1::Target,
            object_oid: tree_oid.to_owned(),
        });
    }
    for LiteralTreeEntry {
        mode,
        entry_kind: kind,
        object_oid,
        name,
    } in literal_tree_entries(root, tree_oid)?
    {
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        match kind {
            Some(entry_kind) => {
                traversal.record_nonregular(path.clone(), object_oid.clone(), entry_kind);
                inventory.push(LiteralInventoryEntry {
                    path,
                    object_oid,
                    mode,
                    entry_kind,
                });
            }
            None => {
                tree_entries.push(SourceTreeEntryClaimV1 {
                    path: path.clone(),
                    object_oid: object_oid.clone(),
                    mode,
                    parent_tree_oid: tree_oid.to_owned(),
                });
                collect_target_tree(root, &object_oid, &path, tree_entries, inventory, traversal)?;
            }
        }
    }
    Ok(())
}

#[allow(
    clippy::result_large_err,
    reason = "the frozen A0 error contract carries detailed source mismatches unboxed"
)]
fn literal_tree_entries(
    root: &Path,
    tree_oid: &str,
) -> Result<Vec<LiteralTreeEntry>, SourceInputError> {
    let output = literal_git_bytes(root, &["ls-tree", "-z", "--full-tree", tree_oid], &[])
        .map_err(|_| SourceInputError::InvalidGitObject {
            side: SourceSnapshotSideV1::Target,
            object_oid: tree_oid.to_owned(),
        })?;
    output
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let tab = entry.iter().position(|byte| *byte == b'\t').ok_or(
                SourceInputError::InvalidGitObject {
                    side: SourceSnapshotSideV1::Target,
                    object_oid: tree_oid.to_owned(),
                },
            )?;
            let (metadata, name_with_tab) = entry.split_at(tab);
            let name = &name_with_tab[1..];
            let metadata =
                std::str::from_utf8(metadata).map_err(|_| SourceInputError::InvalidGitObject {
                    side: SourceSnapshotSideV1::Target,
                    object_oid: tree_oid.to_owned(),
                })?;
            let name =
                std::str::from_utf8(name).map_err(|_| SourceInputError::InvalidGitObject {
                    side: SourceSnapshotSideV1::Target,
                    object_oid: tree_oid.to_owned(),
                })?;
            if name.is_empty() || name.contains('/') {
                return Err(SourceInputError::InvalidGitObject {
                    side: SourceSnapshotSideV1::Target,
                    object_oid: tree_oid.to_owned(),
                });
            }
            let mut fields = metadata.split_whitespace();
            let mode = fields.next().ok_or(SourceInputError::InvalidGitObject {
                side: SourceSnapshotSideV1::Target,
                object_oid: tree_oid.to_owned(),
            })?;
            let object_type = fields.next().ok_or(SourceInputError::InvalidGitObject {
                side: SourceSnapshotSideV1::Target,
                object_oid: tree_oid.to_owned(),
            })?;
            let object_oid = fields.next().ok_or(SourceInputError::InvalidGitObject {
                side: SourceSnapshotSideV1::Target,
                object_oid: tree_oid.to_owned(),
            })?;
            if fields.next().is_some() || !is_full_object_oid(object_oid) {
                return Err(SourceInputError::InvalidGitObject {
                    side: SourceSnapshotSideV1::Target,
                    object_oid: tree_oid.to_owned(),
                });
            }
            let mode =
                u32::from_str_radix(mode, 8).map_err(|_| SourceInputError::InvalidGitObject {
                    side: SourceSnapshotSideV1::Target,
                    object_oid: tree_oid.to_owned(),
                })?;
            let entry_kind = match (mode, object_type) {
                (0o040000, "tree") => None,
                (0o120000, "blob") => Some(SourceInventoryEntryKindV1::Symlink),
                (0o160000, "commit") => Some(SourceInventoryEntryKindV1::Submodule),
                (0o100644 | 0o100755, "blob") => Some(SourceInventoryEntryKindV1::Blob),
                _ => {
                    return Err(SourceInputError::InvalidGitObject {
                        side: SourceSnapshotSideV1::Target,
                        object_oid: object_oid.to_owned(),
                    });
                }
            };
            Ok(LiteralTreeEntry {
                mode,
                entry_kind,
                object_oid: object_oid.to_owned(),
                name: name.to_owned(),
            })
        })
        .collect()
}

#[allow(
    clippy::result_large_err,
    reason = "the frozen A0 error contract carries detailed source mismatches unboxed"
)]
fn profile_claim_for_path(path: &str) -> Result<SourceProfileClaimV1, SourceInputError> {
    match classify_typescript_v1_path(path).map_err(|_| SourceInputError::InvalidRequest)? {
        TypeScriptPathClassification::Included => Ok(SourceProfileClaimV1::Included),
        TypeScriptPathClassification::NonTargetExtension => {
            Ok(SourceProfileClaimV1::NonTargetExtension)
        }
        TypeScriptPathClassification::ProfileExcluded(reason) => {
            Ok(SourceProfileClaimV1::Excluded {
                reasons: vec![reason.to_owned()],
                primary_reason: reason.to_owned(),
            })
        }
    }
}

#[allow(
    clippy::result_large_err,
    reason = "the frozen A0 error contract carries detailed source mismatches unboxed"
)]
fn compare_source_material(
    expected: &SourceMaterialCanonicalViewV1,
    submitted: &SourceAdmissionMaterialV1,
) -> Result<(), SourceInputError> {
    compare_keyed(
        SourceProjectionV1::Inventory,
        &expected.target_inventory,
        &submitted.target_inventory,
        |entry| entry.path.clone(),
    )?;
    compare_keyed(
        SourceProjectionV1::TreeEntries,
        &expected.target_tree_entries,
        &submitted.target_tree_entries,
        |entry| entry.path.clone(),
    )?;
    if expected.inventory_complete != submitted.inventory_complete {
        return Err(SourceInputError::SourceMismatch(singleton_mismatch(
            SourceProjectionV1::InventoryComplete,
            expected.inventory_complete,
            submitted.inventory_complete,
            "inventory_complete",
        )));
    }
    compare_keyed(
        SourceProjectionV1::UnexpandedAncestors,
        &expected.unexpanded_ancestors,
        &submitted.unexpanded_ancestors,
        |entry| entry.path.clone(),
    )?;
    compare_keyed(
        SourceProjectionV1::ReadMaterial,
        &expected.target_read_material,
        &submitted.target_read_material,
        |entry| entry.path.clone(),
    )?;
    if expected.target_basis.binding != submitted.target_basis.binding {
        return Err(SourceInputError::SourceMismatch(singleton_mismatch(
            SourceProjectionV1::BasisBinding,
            &expected.target_basis.binding,
            &submitted.target_basis.binding,
            "binding",
        )));
    }
    compare_keyed(
        SourceProjectionV1::BasisFiles,
        &expected.target_basis.files,
        &submitted.target_basis.files,
        |entry| entry.path.clone(),
    )?;
    compare_keyed(
        SourceProjectionV1::BasisSyntax,
        &expected.target_basis.syntax,
        &submitted.target_basis.syntax,
        |entry| format!("{}:{:?}", entry.file_path, entry.role),
    )?;
    compare_keyed(
        SourceProjectionV1::BaseEntries,
        &expected.base_entries,
        &submitted.base_entries,
        |entry| entry.path.clone(),
    )
}

#[allow(
    clippy::result_large_err,
    reason = "the frozen A0 error contract carries detailed source mismatches unboxed"
)]
fn compare_keyed<T: fmt::Debug>(
    projection: SourceProjectionV1,
    expected: &[T],
    submitted: &[T],
    key: impl Fn(&T) -> String,
) -> Result<(), SourceInputError> {
    let (expected, expected_duplicates) = keyed_debug(expected, &key);
    let (submitted, submitted_duplicates) = keyed_debug(submitted, &key);
    let missing_keys = expected
        .keys()
        .filter(|key| !submitted.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    let extra_keys = submitted
        .keys()
        .filter(|key| !expected.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    let mut mismatched_keys = expected_duplicates;
    mismatched_keys.extend(submitted_duplicates);
    mismatched_keys.extend(expected.iter().filter_map(|(key, value)| {
        submitted
            .get(key)
            .filter(|other| *other != value)
            .map(|_| key.clone())
    }));
    mismatched_keys.sort();
    mismatched_keys.dedup();
    if missing_keys.is_empty() && extra_keys.is_empty() && mismatched_keys.is_empty() {
        return Ok(());
    }
    Err(SourceInputError::SourceMismatch(SourceMaterialMismatchV1 {
        projection,
        missing_keys,
        extra_keys,
        mismatched_keys,
        expected_digest: source_hash_for_debug(&expected),
        actual_digest: source_hash_for_debug(&submitted),
    }))
}

fn keyed_debug<T: fmt::Debug>(
    records: &[T],
    key: &impl Fn(&T) -> String,
) -> (BTreeMap<String, String>, Vec<String>) {
    let mut values = BTreeMap::new();
    let mut duplicates = Vec::new();
    for record in records {
        let key = key(record);
        if values.insert(key.clone(), format!("{record:?}")).is_some() {
            duplicates.push(key);
        }
    }
    (values, duplicates)
}

fn singleton_mismatch<T: fmt::Debug>(
    projection: SourceProjectionV1,
    expected: T,
    submitted: T,
    key: &str,
) -> SourceMaterialMismatchV1 {
    SourceMaterialMismatchV1 {
        projection,
        missing_keys: Vec::new(),
        extra_keys: Vec::new(),
        mismatched_keys: vec![key.to_owned()],
        expected_digest: source_hash_for_debug(&expected),
        actual_digest: source_hash_for_debug(&submitted),
    }
}

fn source_hash_for_debug(value: &impl fmt::Debug) -> SourceHash {
    SourceHash::from_source_bytes(format!("{value:?}").as_bytes())
}

fn read_literal_object(root: &Path, oid: &str) -> Result<LiteralGitObject, LiteralObjectReadError> {
    let output = literal_git_bytes(
        root,
        &["cat-file", "--batch"],
        format!("{oid}\n").as_bytes(),
    )
    .map_err(command_error_as_object_read)?;
    let newline = output
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or(LiteralObjectReadError::Invalid)?;
    let header =
        std::str::from_utf8(&output[..newline]).map_err(|_| LiteralObjectReadError::Invalid)?;
    let mut fields = header.split_whitespace();
    let actual_oid = fields.next().ok_or(LiteralObjectReadError::Invalid)?;
    let kind = fields.next().ok_or(LiteralObjectReadError::Invalid)?;
    if actual_oid == oid && kind == "missing" && fields.next().is_none() {
        return Err(LiteralObjectReadError::Unavailable(
            SourceIoFailureV1::MissingObject,
        ));
    }
    let size = fields
        .next()
        .ok_or(LiteralObjectReadError::Invalid)?
        .parse::<usize>()
        .map_err(|_| LiteralObjectReadError::Invalid)?;
    if fields.next().is_some() || actual_oid != oid || !matches!(kind, "commit" | "tree" | "blob") {
        return Err(LiteralObjectReadError::Invalid);
    }
    let body_start = newline
        .checked_add(1)
        .ok_or(LiteralObjectReadError::Invalid)?;
    let body_end = body_start
        .checked_add(size)
        .ok_or(LiteralObjectReadError::Invalid)?;
    if output.get(body_end) != Some(&b'\n') {
        return Err(LiteralObjectReadError::Invalid);
    }
    let body = output
        .get(body_start..body_end)
        .ok_or(LiteralObjectReadError::Invalid)?
        .to_vec();
    let actual_hash = literal_git_text(root, &["hash-object", "-t", kind, "--stdin"], &body)
        .map_err(command_error_as_object_read)?;
    if actual_hash != oid {
        return Err(LiteralObjectReadError::Invalid);
    }
    Ok(LiteralGitObject {
        kind: kind.to_owned(),
        body,
    })
}

fn literal_object_size(root: &Path, oid: &str) -> Result<usize, SourceIoFailureV1> {
    match literal_git_text(root, &["cat-file", "-s", oid], &[]) {
        Ok(text) => text.parse::<usize>().map_err(|_| SourceIoFailureV1::Io),
        Err(LiteralGitCommandError::Exit) => Err(SourceIoFailureV1::MissingObject),
        Err(error) => Err(command_error_as_source_io(error)),
    }
}

fn commit_tree_oid(body: &[u8]) -> Option<String> {
    let line = body
        .split(|byte| *byte == b'\n')
        .take_while(|line| !line.is_empty())
        .find(|line| line.starts_with(b"tree "))?;
    let tree = std::str::from_utf8(line.strip_prefix(b"tree ")?).ok()?;
    is_full_object_oid(tree).then(|| tree.to_owned())
}

fn literal_git_text(
    root: &Path,
    args: &[&str],
    input: &[u8],
) -> Result<String, LiteralGitCommandError> {
    let bytes = literal_git_bytes(root, args, input)?;
    std::str::from_utf8(&bytes)
        .map(|text| text.trim().to_owned())
        .map_err(|_| LiteralGitCommandError::Io)
}

fn literal_git_bytes(
    root: &Path,
    args: &[&str],
    input: &[u8],
) -> Result<Vec<u8>, LiteralGitCommandError> {
    let mut command = Command::new("git");
    command.env_clear();
    if let Some(path) = env::var_os("PATH") {
        command.env("PATH", path);
    }
    command
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("LC_ALL", "C")
        .arg("--no-pager")
        .arg("--no-optional-locks")
        .arg("--no-replace-objects")
        .arg("-c")
        .arg("core.attributesFile=/dev/null")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|_| LiteralGitCommandError::Io)?;
    let mut stdin = child.stdin.take().ok_or(LiteralGitCommandError::Io)?;
    let mut stdout = child.stdout.take().ok_or(LiteralGitCommandError::Io)?;
    let mut stderr = child.stderr.take().ok_or(LiteralGitCommandError::Io)?;
    let stdout_reader =
        thread::spawn(move || read_capped(&mut stdout, LITERAL_GIT_MAX_OUTPUT_BYTES));
    let stderr_reader =
        thread::spawn(move || read_capped(&mut stderr, LITERAL_GIT_MAX_OUTPUT_BYTES));
    stdin
        .write_all(input)
        .map_err(|_| LiteralGitCommandError::Io)?;
    drop(stdin);

    let deadline = Instant::now() + LITERAL_GIT_TIMEOUT;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => return Err(LiteralGitCommandError::Io),
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            break child.wait().map_err(|_| LiteralGitCommandError::Io)?;
        }
        thread::sleep(LITERAL_GIT_POLL_INTERVAL);
    };
    let stdout = finish_capped_read(stdout_reader)?;
    let stderr = finish_capped_read(stderr_reader)?;
    if timed_out {
        return Err(LiteralGitCommandError::Timeout);
    }
    if stdout.exceeded || stderr.exceeded {
        return Err(LiteralGitCommandError::OutputBound);
    }
    if !status.success() {
        return Err(LiteralGitCommandError::Exit);
    }
    Ok(stdout.bytes)
}

struct CappedRead {
    bytes: Vec<u8>,
    exceeded: bool,
}

fn read_capped(reader: &mut impl Read, limit: usize) -> std::io::Result<CappedRead> {
    let mut bytes = Vec::new();
    let mut exceeded = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let available = limit.saturating_sub(bytes.len());
        if read > available {
            exceeded = true;
        }
        bytes.extend_from_slice(&buffer[..read.min(available)]);
    }
    Ok(CappedRead { bytes, exceeded })
}

fn finish_capped_read(
    reader: thread::JoinHandle<std::io::Result<CappedRead>>,
) -> Result<CappedRead, LiteralGitCommandError> {
    reader
        .join()
        .map_err(|_| LiteralGitCommandError::Io)?
        .map_err(|_| LiteralGitCommandError::Io)
}

fn command_error_as_source_io(error: LiteralGitCommandError) -> SourceIoFailureV1 {
    match error {
        LiteralGitCommandError::Timeout => SourceIoFailureV1::Timeout,
        LiteralGitCommandError::OutputBound => SourceIoFailureV1::OutputBound,
        LiteralGitCommandError::Io | LiteralGitCommandError::Exit => SourceIoFailureV1::Io,
    }
}

fn command_error_as_object_read(error: LiteralGitCommandError) -> LiteralObjectReadError {
    LiteralObjectReadError::Unavailable(command_error_as_source_io(error))
}

fn is_full_object_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_object_oid_for_format(value: &str, object_format: &str) -> bool {
    let expected_length = match object_format {
        "sha1" => 40,
        "sha256" => 64,
        _ => return false,
    };
    value.len() == expected_length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// A1 compares one submitted payload with the payload rebuilt from `ctx`.
/// The locator is selected only after canonical reconstruction and cannot be a
/// source fact.
pub fn validate_payload_from_source(
    _ctx: &ReconstructionContextV1,
    _submitted: PayloadSubmission,
) -> Result<SourceValidated<TypeScriptPayload>, PayloadError> {
    let canonical_payloads = _ctx.rebuild_payloads_from_a0()?;

    let expected_payloads = canonical_payloads
        .iter()
        .find(|(file_id, _)| *file_id == _submitted.locator)
        .map(|(_, payloads)| payloads.as_slice())
        .ok_or_else(|| payload_source_mismatch(&[], &_submitted.draft))?;
    let expected = expected_payloads
        .iter()
        .find(|payload| encode_payload_draft(payload) == _submitted.draft)
        .cloned()
        .ok_or_else(|| payload_source_mismatch(expected_payloads, &_submitted.draft))?;

    Ok(SourceValidated::from_reconstructed(
        expected,
        _ctx.payload_validation_provenance(),
    ))
}

fn payload_source_mismatch(
    expected_payloads: &[TypeScriptPayload],
    submitted: &PayloadDraft,
) -> PayloadError {
    let Some(expected) = expected_payloads
        .iter()
        .min_by_key(|payload| payload_difference_count(payload, submitted))
    else {
        return PayloadError::SourceMismatch(AccountingMismatch {
            missing: vec!["source.locator".to_owned()],
            extra: vec!["submitted.locator".to_owned()],
        });
    };
    let expected = encode_payload_draft(expected);
    let fields = payload_difference_fields(&expected, submitted);
    PayloadError::SourceMismatch(AccountingMismatch {
        missing: fields
            .iter()
            .map(|field| format!("source.{field}"))
            .collect(),
        extra: fields
            .iter()
            .map(|field| format!("submitted.{field}"))
            .collect(),
    })
}

fn payload_difference_count(payload: &TypeScriptPayload, submitted: &PayloadDraft) -> usize {
    let expected = encode_payload_draft(payload);
    payload_difference_fields(&expected, submitted).len()
}

fn payload_difference_fields(
    expected: &PayloadDraft,
    submitted: &PayloadDraft,
) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if expected.role != submitted.role {
        fields.push("role");
    }
    if expected.kind != submitted.kind {
        fields.push("kind");
    }
    if expected.range != submitted.range {
        fields.push("range");
    }
    if expected.outcome != submitted.outcome {
        fields.push("outcome");
    }
    if expected.reasons != submitted.reasons {
        fields.push("reasons");
    }
    if expected.primary_reason != submitted.primary_reason {
        fields.push("primary_reason");
    }
    if expected.refs != submitted.refs {
        fields.push("refs");
    }
    if expected.descriptor_id != submitted.descriptor_id {
        fields.push("descriptor_id");
    }
    if expected.descriptor_hash != submitted.descriptor_hash {
        fields.push("descriptor_hash");
    }
    if expected.data != submitted.data {
        fields.push("data");
    }
    fields
}

/// A2 compares the submitted complete syntax catalogue with A0's five-role
/// reconstruction; submitted rows cannot narrow its inventory.
#[cfg(reviewgraphen_unimplemented_contracts)]
pub fn validate_catalog_from_basis(
    _ctx: &ReconstructionContextV1,
    _submitted: Vec<SyntaxRecord>,
) -> Result<SourceValidated<Vec<SyntaxRecord>>, AccountingMismatch> {
    todo!("I2 skeleton: implementation must admit the complete syntax catalogue")
}

/// A3 compares the full raw extraction report with the reconstruction from A0.
/// Count, digest, and schema agreement alone never suffice.
#[cfg(reviewgraphen_unimplemented_contracts)]
pub fn validate_extraction_from_basis(
    _ctx: &ReconstructionContextV1,
    _submitted: ExtractionReport,
) -> Result<SourceValidated<ExtractionReport>, AccountingMismatch> {
    todo!("I2 skeleton: implementation must admit the full extraction report")
}

/// A4 reconstructs every resolved call and D aggregate from A0, verifies that
/// `extraction` carries the same provenance, then compares `submitted`
///.
#[cfg(reviewgraphen_unimplemented_contracts)]
pub fn validate_ingestion_from_sources(
    _ctx: &ReconstructionContextV1,
    _extraction: &SourceValidated<ExtractionReport>,
    _submitted: IngestionSubmission,
) -> Result<SourceValidated<IngestionReport>, IngestionError> {
    todo!("I2 skeleton: implementation must admit ingestion from A0 and A3")
}

/// A5 derives canonical N/D/G input solely from A3 and A4 values with equal
/// provenance, then compares the raw submission.
#[cfg(reviewgraphen_unimplemented_contracts)]
pub fn validate_synthesis_input(
    _extraction: &SourceValidated<ExtractionReport>,
    _ingestion: &SourceValidated<IngestionReport>,
    _submitted: SynthesizeInput,
) -> Result<SourceValidated<SynthesizeInput>, AccountingMismatch> {
    todo!("I2 skeleton: implementation must admit synthesis input from A3 and A4")
}

/// D1 is the only concrete capability-preserving derivation. It accepts no
/// callback, generic mapper, raw output, or extra snapshot.
#[cfg(reviewgraphen_unimplemented_contracts)]
pub fn synthesize(
    _input: SourceValidated<SynthesizeInput>,
) -> Result<SourceValidated<ObligationSet>, SynthesisError> {
    todo!("I2 skeleton: implementation must derive obligations from admitted synthesis input")
}

/// A6 compares a submitted full closure with a D1 (or earlier A6) expected
/// closure. It checks records and both partition directions before returning
/// the already source-derived expected capability.
#[cfg(reviewgraphen_unimplemented_contracts)]
pub fn validate_obligation_closure(
    _expected: SourceValidated<ObligationSet>,
    _submitted: ObligationClosureSubmission,
) -> Result<SourceValidated<ObligationSet>, AccountingMismatch> {
    todo!("I2 skeleton: implementation must admit a full obligation closure")
}

#[cfg(test)]
mod snapshot_binding_preimage_tests {
    use super::*;
    use std::path::PathBuf;

    fn admitted_domain() -> AdmittedSourceDomainV1 {
        AdmittedSourceDomainV1 {
            workspace_admission_root: PathBuf::from("/test-workspace"),
            repository_admission_root: PathBuf::from("/test-repository"),
            repository_identity: "test-root-set".into(),
            base_commit_oid: "test-base-commit".into(),
            base_tree_oid: "test-base-tree".into(),
            target_commit_oid: "test-target-commit".into(),
            target_tree_oid: "test-target-tree".into(),
            registry_binding: typescript_registry_binding(),
            bounds: SourceAdmissionBoundsV1 {
                max_files: 1,
                max_file_bytes: 1,
                max_total_source_bytes: 1,
            },
        }
    }

    fn snapshot_binding(domain: &AdmittedSourceDomainV1) -> SnapshotBinding {
        SnapshotBinding::from_admitted_binding(&snapshot_binding_preimage(domain))
    }

    #[test]
    fn snapshot_binding_changes_for_each_design_input_element() {
        let baseline_domain = admitted_domain();
        let baseline_binding = snapshot_binding(&baseline_domain);

        for element in [
            "repository_identity",
            "base_commit_oid",
            "base_tree_oid",
            "target_commit_oid",
            "target_tree_oid",
            "registry_hash",
            "tuple_hash",
            "extractor_set_hash",
            "rule_set_hash",
        ] {
            let mut variant = admitted_domain();
            match element {
                "repository_identity" => variant.repository_identity.push_str("-variant"),
                "base_commit_oid" => variant.base_commit_oid.push_str("-variant"),
                "base_tree_oid" => variant.base_tree_oid.push_str("-variant"),
                "target_commit_oid" => variant.target_commit_oid.push_str("-variant"),
                "target_tree_oid" => variant.target_tree_oid.push_str("-variant"),
                "registry_hash" => variant.registry_binding.registry_hash.push_str("-variant"),
                "tuple_hash" => variant
                    .registry_binding
                    .tuple
                    .projection_id
                    .push_str("-variant"),
                "extractor_set_hash" => variant
                    .registry_binding
                    .tuple
                    .extractor_set_hash
                    .push_str("-variant"),
                "rule_set_hash" => variant
                    .registry_binding
                    .tuple
                    .rule_set_hash
                    .push_str("-variant"),
                _ => unreachable!("test table contains only the design's binding elements"),
            }

            let variant_preimage = snapshot_binding_preimage(&variant);
            if element == "tuple_hash" {
                let tuple_hash = registry_tuple_hash(&variant.registry_binding);
                assert!(
                    variant_preimage.contains(&format!(
                        "tuple_hash={}:{}",
                        tuple_hash.len(),
                        tuple_hash
                    )),
                    "{element} must be represented in the snapshot-binding preimage"
                );
            }
            assert_ne!(
                baseline_binding,
                SnapshotBinding::from_admitted_binding(&variant_preimage),
                "snapshot binding must change when only {element} changes"
            );
        }
    }

    #[test]
    fn snapshot_binding_preimage_names_every_registry_hash_element() {
        // The design lists registry_hash, tuple_hash, extractor_set_hash and
        // rule_set_hash. extractor_set_hash and rule_set_hash also feed tuple_hash,
        // so a binding-inequality check alone cannot notice one of them being
        // dropped from the preimage; assert each is present by name and value.
        let domain = admitted_domain();
        let preimage = snapshot_binding_preimage(&domain);
        let binding = &domain.registry_binding;
        let tuple_hash = registry_tuple_hash(binding);
        for (name, value) in [
            ("registry_hash", binding.registry_hash.as_str()),
            ("tuple_hash", tuple_hash.as_str()),
            (
                "extractor_set_hash",
                binding.tuple.extractor_set_hash.as_str(),
            ),
            ("rule_set_hash", binding.tuple.rule_set_hash.as_str()),
        ] {
            assert!(
                preimage.contains(&format!("{name}={}:{}", value.len(), value)),
                "snapshot binding preimage must contain {name}"
            );
        }
    }

    #[test]
    fn registry_tuple_hash_matches_the_frozen_registry_literal() {
        // `tuple_hash` is the SHA-256 of the canonical bytes of the
        // seven-field tuple object. The frozen literal is the same value used by
        // the frozen CLI acceptance (typescript_v1_acceptance.rs TUPLE_HASH) and
        // schemas/reviewgraphen.extraction_report.v2.example.json.
        let binding = reviewgraphen_core::source_review::registry::typescript_registry_binding();
        assert_eq!(
            registry_tuple_hash(&binding),
            "sha256:30b5b7ed971d9bad50164563cef086488781f36870e0c12863aa023bcf186a55"
        );
    }

    #[test]
    fn snapshot_binding_is_deterministic_for_the_same_admitted_domain() {
        let domain = admitted_domain();

        assert_eq!(
            snapshot_binding(&domain),
            snapshot_binding(&domain),
            "snapshot binding must be deterministic for the same admitted domain"
        );
    }
}
