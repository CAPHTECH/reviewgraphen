//! G3 observations bound exclusively inside the literal-Git A0 admission.
//! These private descriptive rows are not an A1 seal or a serialized fact set.

mod partition;

use super::{
    ReconstructionProvenanceV1, SourceReadClaimV1, SourceReadExtentV1,
    rebuild_payload_rows_from_provenance,
};
use reviewgraphen_core::StableId;
use reviewgraphen_core::source_review::basis::{SourceFileOutcome, SourceSyntaxRole};
use reviewgraphen_core::source_review::ids::{
    CanonicalFileKey, SnapshotBinding, SourceFileId, SourceHash, SourceRange, SyntaxKeyV1,
    registry_tuple_hash,
};
use reviewgraphen_core::source_review::reasons::{RecordOutcomeV1, TypeScriptSyntaxKind};
use reviewgraphen_ingest::typescript::g3_syntax::{
    RawTypeScriptExclusionV1, RawTypeScriptFileV1, RawTypeScriptKeyV1,
    RawTypeScriptParseObstructionV1, TypeScriptLhsKindV1, parse_g3_syntax,
};
use reviewgraphen_ingest::typescript::payload::{
    SyntaxRole, TypeScriptOutcomeV1, TypeScriptPayload, TypeScriptPayloadData,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const PROFILE: &str = "typescript.g3-admission@2";
const EXTRACTOR: &str = "tree-sitter-typescript.g3@2";
const OBSERVATION_DOMAIN: &str = "reviewgraphen.g3.typescript.observation.v2";
const OBSTRUCTION_DOMAIN: &str = "reviewgraphen.g3.typescript.obstruction.v2";
const PARTITION_DOMAIN: &str = "reviewgraphen.g3.typescript.partition.v2";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum G3AccountingError {
    PayloadReconstruction,
    MissingFile,
    DuplicateFile,
    BindingMismatch,
    DuplicateAcceptedSyntax,
    ParserDisagreement(RawTypeScriptParseObstructionV1),
    MissingOccurrence,
    UnexpectedOccurrence,
    DuplicateOccurrence,
    DoubleDisposition,
    IdCollision,
    Derivation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TypeScriptFileBindingV1 {
    snapshot_binding: SnapshotBinding,
    file_id: SourceFileId,
    canonical_path: String,
    source_hash: SourceHash,
}

impl TypeScriptFileBindingV1 {
    pub(crate) fn snapshot_binding(&self) -> &SnapshotBinding {
        &self.snapshot_binding
    }
    pub(crate) fn file_id(&self) -> &SourceFileId {
        &self.file_id
    }
    pub(crate) fn canonical_path(&self) -> &str {
        &self.canonical_path
    }
    pub(crate) fn source_hash(&self) -> &SourceHash {
        &self.source_hash
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TypeScriptG3ReasonV1 {
    UnsupportedAssignmentLhs(TypeScriptLhsKindV1),
    OverlappingOccurrence(Vec<StableId>),
    MissingAcceptedSyntax,
    Nonmember,
}

impl TypeScriptG3ReasonV1 {
    fn literal(&self) -> &'static str {
        match self {
            Self::UnsupportedAssignmentLhs(_) => "unsupported_assignment_lhs@2",
            Self::OverlappingOccurrence(_) => "overlapping_occurrence@2",
            Self::MissingAcceptedSyntax => "missing_accepted_syntax@2",
            Self::Nonmember => "nonmember@2",
        }
    }
    fn detail(&self) -> Value {
        match self {
            Self::UnsupportedAssignmentLhs(kind) => json!({"lhs_kind": kind.wire_literal()}),
            Self::OverlappingOccurrence(ids) => json!({"overlapping_occurrence_ids":
                ids.iter().map(StableId::as_str).collect::<Vec<_>>() }),
            Self::MissingAcceptedSyntax | Self::Nonmember => Value::Null,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TypeScriptG3SourceObservationV1 {
    IdentifierAssignment,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TestFrameworkUnknownCodeV1 {
    TestFrameworkSemanticsUnaccepted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TypeScriptG3OutcomeV1 {
    ExistingSyntaxMembership {
        scope_key: SyntaxKeyV1,
        callable_key: SyntaxKeyV1,
    },
    SourceObservation(TypeScriptG3SourceObservationV1),
    Unknown(TestFrameworkUnknownCodeV1),
    Obstructed {
        obstruction_id: StableId,
        reason: TypeScriptG3ReasonV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TypeScriptG3ContainmentV1 {
    id: StableId,
    binding: TypeScriptFileBindingV1,
    parent_range: SourceRange,
    child_range: SourceRange,
    scope_key: Option<SyntaxKeyV1>,
    callable_key: Option<SyntaxKeyV1>,
    outcome: TypeScriptG3OutcomeV1,
}
impl TypeScriptG3ContainmentV1 {
    pub(crate) fn id(&self) -> &StableId {
        &self.id
    }
    pub(crate) fn binding(&self) -> &TypeScriptFileBindingV1 {
        &self.binding
    }
    pub(crate) fn parent_range(&self) -> SourceRange {
        self.parent_range
    }
    pub(crate) fn child_range(&self) -> SourceRange {
        self.child_range
    }
    pub(crate) fn scope_key(&self) -> Option<&SyntaxKeyV1> {
        self.scope_key.as_ref()
    }
    pub(crate) fn callable_key(&self) -> Option<&SyntaxKeyV1> {
        self.callable_key.as_ref()
    }
    pub(crate) fn outcome(&self) -> &TypeScriptG3OutcomeV1 {
        &self.outcome
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TypeScriptG3WriteV1 {
    id: StableId,
    binding: TypeScriptFileBindingV1,
    occurrence_range: SourceRange,
    lhs_range: SourceRange,
    lhs_kind: TypeScriptLhsKindV1,
    outcome: TypeScriptG3OutcomeV1,
}
impl TypeScriptG3WriteV1 {
    pub(crate) fn id(&self) -> &StableId {
        &self.id
    }
    pub(crate) fn binding(&self) -> &TypeScriptFileBindingV1 {
        &self.binding
    }
    pub(crate) fn occurrence_range(&self) -> SourceRange {
        self.occurrence_range
    }
    pub(crate) fn lhs_range(&self) -> SourceRange {
        self.lhs_range
    }
    pub(crate) fn lhs_kind(&self) -> &TypeScriptLhsKindV1 {
        &self.lhs_kind
    }
    pub(crate) fn outcome(&self) -> &TypeScriptG3OutcomeV1 {
        &self.outcome
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TypeScriptG3TestMarkerCandidateV1 {
    id: StableId,
    binding: TypeScriptFileBindingV1,
    occurrence_range: SourceRange,
    call_key: Option<SyntaxKeyV1>,
    outcome: TypeScriptG3OutcomeV1,
}
impl TypeScriptG3TestMarkerCandidateV1 {
    pub(crate) fn id(&self) -> &StableId {
        &self.id
    }
    pub(crate) fn binding(&self) -> &TypeScriptFileBindingV1 {
        &self.binding
    }
    pub(crate) fn occurrence_range(&self) -> SourceRange {
        self.occurrence_range
    }
    pub(crate) fn call_key(&self) -> Option<&SyntaxKeyV1> {
        self.call_key.as_ref()
    }
    pub(crate) fn outcome(&self) -> &TypeScriptG3OutcomeV1 {
        &self.outcome
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct G3FilePartitionV1 {
    canonical_path: String,
    file_id: SourceFileId,
    source_hash: Option<SourceHash>,
    outcome: SourceFileOutcome,
    obstruction_id: Option<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct G3ExclusionV1 {
    binding: TypeScriptFileBindingV1,
    syntax: RawTypeScriptExclusionV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct G3ConstructPartitionV1 {
    id: StableId,
    occurrence_ids: Vec<StableId>,
    success_ids: Vec<StableId>,
    obstruction_ids: Vec<StableId>,
    per_file_contracts: Vec<G3FileContractPartitionV1>,
    exclusions: Vec<G3ExclusionV1>,
    latent: Vec<G3LatentV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct G3FileContractPartitionV1 {
    binding: TypeScriptFileBindingV1,
    contract: &'static str,
    raw_keys: Vec<RawTypeScriptKeyV1>,
    success_keys: Vec<RawTypeScriptKeyV1>,
    obstruction_keys: Vec<RawTypeScriptKeyV1>,
    occurrence_ids: Vec<StableId>,
    success_ids: Vec<StableId>,
    obstruction_ids: Vec<StableId>,
    exclusion_keys: Vec<RawTypeScriptExclusionV1>,
}
impl G3ConstructPartitionV1 {
    pub(crate) fn exclusions(&self) -> &[G3ExclusionV1] {
        &self.exclusions
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum G3LatentV1 {
    FileConstructs {
        file_id: SourceFileId,
        canonical_path: String,
        source_hash: Option<SourceHash>,
        outcome: SourceFileOutcome,
        obstruction_id: StableId,
        count: G3LatentCardinalityV1,
    },
    UnexpandedAncestorDescendants {
        path: String,
        object_oid: String,
        cause: String,
        count: G3LatentCardinalityV1,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum G3LatentCardinalityV1 {
    Unknown,
}

fn latent_preimage(row: &G3LatentV1) -> Value {
    match row {
        G3LatentV1::FileConstructs {
            file_id,
            canonical_path,
            source_hash,
            outcome,
            obstruction_id,
            count: G3LatentCardinalityV1::Unknown,
        } => json!({
            "kind":"file_constructs", "file_id":file_id.canonical_key(),
            "canonical_source_path":canonical_path,
            "source_hash":source_hash.as_ref().map(SourceHash::wire_literal),
            "outcome":format!("{outcome:?}"), "obstruction_id":obstruction_id.as_str(),
            "count":"unknown",
        }),
        G3LatentV1::UnexpandedAncestorDescendants {
            path,
            object_oid,
            cause,
            count: G3LatentCardinalityV1::Unknown,
        } => json!({
            "kind":"unexpanded_ancestor_descendants", "path":path,
            "object_oid":object_oid, "cause":cause, "count":"unknown",
        }),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TypeScriptG3ObstructionV1 {
    id: StableId,
    observation_id: StableId,
    reason: String,
    detail: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TypeScriptG3BatchV1 {
    snapshot_binding: SnapshotBinding,
    containment: Vec<TypeScriptG3ContainmentV1>,
    writes: Vec<TypeScriptG3WriteV1>,
    test_marker_candidates: Vec<TypeScriptG3TestMarkerCandidateV1>,
    file_partition: Vec<G3FilePartitionV1>,
    construct_partition: G3ConstructPartitionV1,
    obstructions: BTreeMap<StableId, TypeScriptG3ObstructionV1>,
}
impl TypeScriptG3BatchV1 {
    pub(crate) fn snapshot_binding(&self) -> &SnapshotBinding {
        &self.snapshot_binding
    }
    pub(crate) fn containment(&self) -> &[TypeScriptG3ContainmentV1] {
        &self.containment
    }
    pub(crate) fn writes(&self) -> &[TypeScriptG3WriteV1] {
        &self.writes
    }
    pub(crate) fn test_marker_candidates(&self) -> &[TypeScriptG3TestMarkerCandidateV1] {
        &self.test_marker_candidates
    }
    pub(crate) fn file_partition(&self) -> &[G3FilePartitionV1] {
        &self.file_partition
    }
    pub(crate) fn construct_partition(&self) -> &G3ConstructPartitionV1 {
        &self.construct_partition
    }
    pub(crate) fn obstructions(&self) -> &BTreeMap<StableId, TypeScriptG3ObstructionV1> {
        &self.obstructions
    }
    pub(crate) fn obstruction(&self, id: &StableId) -> Option<&TypeScriptG3ObstructionV1> {
        self.obstructions.get(id)
    }
}

fn range(range: SourceRange) -> Value {
    json!({"start":range.start(),"end":range.end()})
}

fn raw_key(key: &RawTypeScriptKeyV1) -> Value {
    match key {
        RawTypeScriptKeyV1::Containment(row) => json!({
            "kind":"file_direct_callable", "parent":range(row.parent_range),
            "child":range(row.child_range), "child_kind":row.child_kind.wire_literal(),
        }),
        RawTypeScriptKeyV1::Assignment(row) => json!({
            "kind":"assignment_expression", "occurrence":range(row.occurrence_range),
            "lhs":range(row.lhs_range), "lhs_kind":row.lhs_kind.wire_literal(),
        }),
        RawTypeScriptKeyV1::Call(row) => json!({
            "kind":"call_expression", "occurrence":range(row.occurrence_range),
        }),
    }
}

fn identity(
    p: &ReconstructionProvenanceV1,
    binding: &TypeScriptFileBindingV1,
    contract: &'static str,
    syntax: Value,
) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("version".into(), json!(2)),
        ("contract".into(), json!(contract)),
        ("profile".into(), json!(PROFILE)),
        ("extractor".into(), json!(EXTRACTOR)),
        (
            "snapshot".into(),
            json!({"kind":"ts_a0","value":p.snapshot_binding.as_str()}),
        ),
        (
            "registry".into(),
            json!({"registry_hash":p.registry_binding.registry_hash,
            "tuple_hash":registry_tuple_hash(&p.registry_binding)}),
        ),
        ("file_id".into(), json!(binding.file_id.canonical_key())),
        (
            "canonical_source_path".into(),
            json!(binding.canonical_path),
        ),
        (
            "source_hash".into(),
            json!(binding.source_hash.wire_literal()),
        ),
        ("coordinate".into(), json!("ts_utf8_byte_half_open")),
        ("syntax".into(), syntax),
    ])
}

fn stable(domain: &str, preimage: &BTreeMap<String, Value>) -> Result<StableId, G3AccountingError> {
    StableId::derived(domain, preimage).map_err(|_| G3AccountingError::Derivation)
}

fn insert_obstruction(
    batch: &mut TypeScriptG3BatchV1,
    preimage: &BTreeMap<String, Value>,
    observation_id: &StableId,
    reason: &str,
    detail: Value,
) -> Result<StableId, G3AccountingError> {
    let mut preimage = preimage.clone();
    preimage.insert("observation_id".into(), json!(observation_id.as_str()));
    preimage.insert("reason".into(), json!(reason));
    preimage.insert("reason_detail".into(), detail.clone());
    let id = stable(OBSTRUCTION_DOMAIN, &preimage)?;
    let row = TypeScriptG3ObstructionV1 {
        id: id.clone(),
        observation_id: observation_id.clone(),
        reason: reason.to_owned(),
        detail,
    };
    if batch.obstructions.insert(id.clone(), row).is_some() {
        return Err(G3AccountingError::IdCollision);
    }
    Ok(id)
}

type A1Index = BTreeMap<
    (SourceFileId, SyntaxRole, TypeScriptSyntaxKind, SourceRange),
    (SyntaxKeyV1, TypeScriptPayload),
>;

fn rebuilt_index(p: &ReconstructionProvenanceV1) -> Result<A1Index, G3AccountingError> {
    let mut index = BTreeMap::new();
    let mut keys = BTreeSet::new();
    for (file_id, rows) in rebuild_payload_rows_from_provenance(p)
        .map_err(|_| G3AccountingError::PayloadReconstruction)?
    {
        for row in rows {
            let role = match row.role {
                SyntaxRole::Scope => SourceSyntaxRole::Scope,
                SyntaxRole::Callable => SourceSyntaxRole::Callable,
                SyntaxRole::Call => SourceSyntaxRole::Call,
                SyntaxRole::Binding => SourceSyntaxRole::Binding,
                SyntaxRole::Surface => SourceSyntaxRole::Surface,
            };
            let key = SyntaxKeyV1::derive_from_source(
                &p.registry_binding,
                &p.snapshot_binding,
                &file_id,
                role,
                &row.kind,
                row.range,
            );
            if !keys.insert(key.wire_literal().to_owned())
                || index
                    .insert(
                        (file_id.clone(), row.role, row.kind.clone(), row.range),
                        (key, row),
                    )
                    .is_some()
            {
                return Err(G3AccountingError::DuplicateAcceptedSyntax);
            }
        }
    }
    Ok(index)
}

fn accepted<'a>(
    index: &'a A1Index,
    binding: &TypeScriptFileBindingV1,
    role: SyntaxRole,
    kind: &TypeScriptSyntaxKind,
    span: SourceRange,
) -> Option<&'a (SyntaxKeyV1, TypeScriptPayload)> {
    index.get(&(binding.file_id.clone(), role, kind.clone(), span))
}

fn overlap(left: SourceRange, right: SourceRange) -> bool {
    left.start() < right.end() && right.start() < left.end()
}

fn file_binding(
    p: &ReconstructionProvenanceV1,
    path: &str,
) -> Result<TypeScriptFileBindingV1, G3AccountingError> {
    let source = p
        .source_material
        .target_sources
        .file_by_canonical_basis_path(path)
        .ok_or(G3AccountingError::MissingFile)?;
    let canonical_path = p
        .source_material
        .target_sources
        .canonical_basis_path(&source.file_id)
        .ok_or(G3AccountingError::BindingMismatch)?;
    if canonical_path != path || SourceHash::from_source_bytes(&source.bytes) != source.source_hash
    {
        return Err(G3AccountingError::BindingMismatch);
    }
    Ok(TypeScriptFileBindingV1 {
        snapshot_binding: p.snapshot_binding.clone(),
        file_id: source.file_id.clone(),
        canonical_path: canonical_path.to_owned(),
        source_hash: source.source_hash.clone(),
    })
}

pub(super) fn observe(
    p: &ReconstructionProvenanceV1,
) -> Result<TypeScriptG3BatchV1, G3AccountingError> {
    let index = rebuilt_index(p)?;
    let material = &p.source_material;
    let mut batch = TypeScriptG3BatchV1 {
        snapshot_binding: p.snapshot_binding.clone(),
        containment: Vec::new(),
        writes: Vec::new(),
        test_marker_candidates: Vec::new(),
        file_partition: Vec::new(),
        construct_partition: G3ConstructPartitionV1 {
            id: stable(
                PARTITION_DOMAIN,
                &BTreeMap::from([("version".into(), json!(2))]),
            )?,
            occurrence_ids: Vec::new(),
            success_ids: Vec::new(),
            obstruction_ids: Vec::new(),
            per_file_contracts: Vec::new(),
            exclusions: Vec::new(),
            latent: Vec::new(),
        },
        obstructions: BTreeMap::new(),
    };
    let mut seen_files = BTreeSet::new();
    let expected = material
        .target_basis
        .files
        .iter()
        .filter(|row| row.outcome == SourceFileOutcome::Parsed)
        .map(|row| row.path.clone())
        .collect::<BTreeSet<_>>();
    let actual = material
        .target_sources
        .files()
        .iter()
        .map(|source| {
            material
                .target_sources
                .canonical_basis_path(&source.file_id)
                .map(str::to_owned)
                .ok_or(G3AccountingError::BindingMismatch)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if expected != actual || actual.len() != material.target_sources.files().len() {
        return Err(G3AccountingError::BindingMismatch);
    }
    for file in &material.target_basis.files {
        if !seen_files.insert(file.path.clone()) {
            return Err(G3AccountingError::DuplicateFile);
        }
        let file_id = SourceFileId::from_basis_file_key(
            CanonicalFileKey::from_basis_path(&file.path)
                .map_err(|_| G3AccountingError::BindingMismatch)?,
        );
        let inventory = material
            .target_inventory
            .iter()
            .find(|row| row.path == file.path)
            .ok_or(G3AccountingError::MissingFile)?;
        if inventory.outcome != file.outcome
            || material
                .target_inventory
                .iter()
                .filter(|row| row.path == file.path)
                .count()
                != 1
        {
            return Err(G3AccountingError::BindingMismatch);
        }
        let source_hash = match &inventory.read {
            SourceReadClaimV1::Complete { source_hash, .. } => Some(source_hash.clone()),
            SourceReadClaimV1::Failed { .. } | SourceReadClaimV1::NotRead { .. } => None,
        };
        let mut file_obstruction_id = None;
        if file.outcome == SourceFileOutcome::Parsed {
            let binding = file_binding(p, &file.path)?;
            if binding.file_id != file_id || source_hash.as_ref() != Some(&binding.source_hash) {
                return Err(G3AccountingError::BindingMismatch);
            }
            let source = material
                .target_sources
                .file(&file_id)
                .ok_or(G3AccountingError::MissingFile)?;
            let raw =
                parse_g3_syntax(&source.bytes).map_err(G3AccountingError::ParserDisagreement)?;
            partition::verify_raw(&raw)?;
            bind_parsed(p, &binding, &raw, &index, &mut batch)?;
        } else if file.outcome == SourceFileOutcome::ParseFailed {
            let read = material
                .target_read_material
                .iter()
                .find(|row| row.path == file.path && row.extent == SourceReadExtentV1::FullBlob)
                .ok_or(G3AccountingError::MissingFile)?;
            if source_hash.as_ref() != Some(&read.hash)
                || SourceHash::from_source_bytes(&read.bytes) != read.hash
            {
                return Err(G3AccountingError::BindingMismatch);
            }
            let parse_error = parse_g3_syntax(&read.bytes)
                .err()
                .ok_or(G3AccountingError::BindingMismatch)?;
            let reason = match parse_error {
                RawTypeScriptParseObstructionV1::InvalidUtf8 => "invalid_utf8@2",
                RawTypeScriptParseObstructionV1::InvalidRange => "invalid_range@2",
                RawTypeScriptParseObstructionV1::ParseFailed
                | RawTypeScriptParseObstructionV1::ParserUnavailable => "parse_failed@2",
            };
            file_obstruction_id = Some(obstruct_file(
                p,
                &file_id,
                &file.path,
                source_hash.as_ref(),
                json!({"kind":"unparsed_file"}),
                reason,
                &mut batch,
            )?);
        } else if matches!(
            file.outcome,
            SourceFileOutcome::UnreadBound | SourceFileOutcome::UnsupportedEntry
        ) {
            file_obstruction_id = Some(obstruct_file(
                p,
                &file_id,
                &file.path,
                None,
                json!({"kind":"unread_file", "outcome":format!("{:?}",file.outcome),
                    "read_extent":format!("{:?}",inventory.read),"object_oid":inventory.object_oid}),
                "missing_accepted_syntax@2",
                &mut batch,
            )?);
        }
        if matches!(
            file.outcome,
            SourceFileOutcome::ParseFailed
                | SourceFileOutcome::UnreadBound
                | SourceFileOutcome::UnsupportedEntry
        ) {
            batch
                .construct_partition
                .latent
                .push(G3LatentV1::FileConstructs {
                    file_id: file_id.clone(),
                    canonical_path: file.path.clone(),
                    source_hash: source_hash.clone(),
                    outcome: file.outcome,
                    obstruction_id: file_obstruction_id
                        .clone()
                        .ok_or(G3AccountingError::MissingFile)?,
                    count: G3LatentCardinalityV1::Unknown,
                });
        }
        batch.file_partition.push(G3FilePartitionV1 {
            canonical_path: file.path.clone(),
            file_id,
            source_hash,
            outcome: file.outcome,
            obstruction_id: file_obstruction_id,
        });
    }
    if seen_files.len() != material.target_inventory.len()
        || !material
            .target_inventory
            .iter()
            .all(|row| seen_files.contains(&row.path))
    {
        return Err(G3AccountingError::MissingFile);
    }
    for ancestor in &material.unexpanded_ancestors {
        // These are known ancestors with unknown latent descendant cardinality;
        // their source-witness identity stays distinct from unseen file IDs.
        batch
            .construct_partition
            .latent
            .push(G3LatentV1::UnexpandedAncestorDescendants {
                path: ancestor.path.clone(),
                object_oid: ancestor.object_oid.clone(),
                cause: format!("{:?}", ancestor.cause),
                count: G3LatentCardinalityV1::Unknown,
            });
    }
    finish_partition(p, &mut batch)?;
    Ok(batch)
}

fn obstruct_file(
    p: &ReconstructionProvenanceV1,
    file_id: &SourceFileId,
    path: &str,
    hash: Option<&SourceHash>,
    syntax: Value,
    reason: &str,
    batch: &mut TypeScriptG3BatchV1,
) -> Result<StableId, G3AccountingError> {
    let preimage = BTreeMap::from([
        ("version".into(), json!(2)),
        ("contract".into(), json!("TS-file-accounting@2")),
        ("profile".into(), json!(PROFILE)),
        ("extractor".into(), json!(EXTRACTOR)),
        (
            "snapshot".into(),
            json!({"kind":"ts_a0","value":p.snapshot_binding.as_str()}),
        ),
        (
            "registry".into(),
            json!({"registry_hash":p.registry_binding.registry_hash,
            "tuple_hash":registry_tuple_hash(&p.registry_binding)}),
        ),
        ("file_id".into(), json!(file_id.canonical_key())),
        ("canonical_source_path".into(), json!(path)),
        (
            "source_hash".into(),
            json!(hash.map(SourceHash::wire_literal)),
        ),
        ("coordinate".into(), json!("unlocated_file")),
        ("syntax".into(), syntax),
    ]);
    let observation_id = stable(OBSERVATION_DOMAIN, &preimage)?;
    insert_obstruction(batch, &preimage, &observation_id, reason, Value::Null)
}

fn bind_parsed(
    p: &ReconstructionProvenanceV1,
    binding: &TypeScriptFileBindingV1,
    raw: &RawTypeScriptFileV1,
    index: &A1Index,
    batch: &mut TypeScriptG3BatchV1,
) -> Result<(), G3AccountingError> {
    let mut successes = Vec::new();
    let mut obstructed = Vec::new();
    let mut ids = BTreeMap::<RawTypeScriptKeyV1, StableId>::new();
    for pair in &raw.containment {
        let syntax = json!({"kind":"file_direct_callable", "parent":range(pair.parent_range),
            "child":range(pair.child_range),"child_kind":pair.child_kind.wire_literal()});
        let preimage = identity(p, binding, "TS-containment@2", syntax);
        let id = stable(OBSERVATION_DOMAIN, &preimage)?;
        ids.insert(RawTypeScriptKeyV1::Containment(pair.clone()), id.clone());
        let scope_kind = TypeScriptSyntaxKind::parse_wire("program")
            .map_err(|_| G3AccountingError::Derivation)?;
        let scope = accepted(
            index,
            binding,
            SyntaxRole::Scope,
            &scope_kind,
            pair.parent_range,
        );
        let callable = accepted(
            index,
            binding,
            SyntaxRole::Callable,
            &pair.child_kind,
            pair.child_range,
        );
        let (scope_key, callable_key, outcome) = match (scope, callable) {
            (Some((scope_key, scope_row)), Some((callable_key, _))) => {
                let member = matches!((&scope_row.outcome, &scope_row.data),
                    (TypeScriptOutcomeV1::Scope(RecordOutcomeV1::Recorded), TypeScriptPayloadData::Scope(data))
                    if data.member_keys.contains(callable_key));
                if member {
                    successes.push(RawTypeScriptKeyV1::Containment(pair.clone()));
                    (
                        Some(scope_key.clone()),
                        Some(callable_key.clone()),
                        TypeScriptG3OutcomeV1::ExistingSyntaxMembership {
                            scope_key: scope_key.clone(),
                            callable_key: callable_key.clone(),
                        },
                    )
                } else {
                    let reason = TypeScriptG3ReasonV1::Nonmember;
                    let obstruction_id = insert_obstruction(
                        batch,
                        &preimage,
                        &id,
                        reason.literal(),
                        reason.detail(),
                    )?;
                    obstructed.push(RawTypeScriptKeyV1::Containment(pair.clone()));
                    (
                        Some(scope_key.clone()),
                        Some(callable_key.clone()),
                        TypeScriptG3OutcomeV1::Obstructed {
                            obstruction_id,
                            reason,
                        },
                    )
                }
            }
            _ => {
                let reason = TypeScriptG3ReasonV1::MissingAcceptedSyntax;
                let obstruction_id =
                    insert_obstruction(batch, &preimage, &id, reason.literal(), reason.detail())?;
                obstructed.push(RawTypeScriptKeyV1::Containment(pair.clone()));
                (
                    scope.map(|(key, _)| key.clone()),
                    callable.map(|(key, _)| key.clone()),
                    TypeScriptG3OutcomeV1::Obstructed {
                        obstruction_id,
                        reason,
                    },
                )
            }
        };
        let is_obstructed = matches!(outcome, TypeScriptG3OutcomeV1::Obstructed { .. });
        batch.containment.push(TypeScriptG3ContainmentV1 {
            id: id.clone(),
            binding: binding.clone(),
            parent_range: pair.parent_range,
            child_range: pair.child_range,
            scope_key,
            callable_key,
            outcome,
        });
        batch.construct_partition.occurrence_ids.push(id.clone());
        if is_obstructed {
            batch.construct_partition.obstruction_ids.push(id);
        } else {
            batch.construct_partition.success_ids.push(id);
        }
    }
    let assignments = raw
        .assignments
        .iter()
        .map(|row| {
            let preimage = identity(
                p,
                binding,
                "TS-write@2",
                json!({"kind":"assignment_expression",
            "occurrence":range(row.occurrence_range), "lhs":range(row.lhs_range),
            "lhs_kind":row.lhs_kind.wire_literal()}),
            );
            let id = stable(OBSERVATION_DOMAIN, &preimage)?;
            Ok::<_, G3AccountingError>((row, preimage, id))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (row, preimage, id) in &assignments {
        ids.insert(RawTypeScriptKeyV1::Assignment((*row).clone()), id.clone());
        let mut overlaps = assignments
            .iter()
            .filter(|(other, _, _)| {
                other.occurrence_range != row.occurrence_range
                    && overlap(other.occurrence_range, row.occurrence_range)
            })
            .map(|(_, _, id)| id.clone())
            .collect::<Vec<_>>();
        overlaps.sort();
        let reason = if !overlaps.is_empty() {
            Some(TypeScriptG3ReasonV1::OverlappingOccurrence(overlaps))
        } else if row.lhs_kind != TypeScriptLhsKindV1::Identifier {
            Some(TypeScriptG3ReasonV1::UnsupportedAssignmentLhs(row.lhs_kind))
        } else {
            None
        };
        let outcome = if let Some(reason) = reason {
            let obstruction_id =
                insert_obstruction(batch, preimage, id, reason.literal(), reason.detail())?;
            obstructed.push(RawTypeScriptKeyV1::Assignment((*row).clone()));
            batch.construct_partition.obstruction_ids.push(id.clone());
            TypeScriptG3OutcomeV1::Obstructed {
                obstruction_id,
                reason,
            }
        } else {
            successes.push(RawTypeScriptKeyV1::Assignment((*row).clone()));
            batch.construct_partition.success_ids.push(id.clone());
            TypeScriptG3OutcomeV1::SourceObservation(
                TypeScriptG3SourceObservationV1::IdentifierAssignment,
            )
        };
        batch.writes.push(TypeScriptG3WriteV1 {
            id: id.clone(),
            binding: binding.clone(),
            occurrence_range: row.occurrence_range,
            lhs_range: row.lhs_range,
            lhs_kind: row.lhs_kind,
            outcome,
        });
        batch.construct_partition.occurrence_ids.push(id.clone());
    }
    for call in &raw.calls {
        let preimage = identity(
            p,
            binding,
            "TS-test-marker-candidate@2",
            json!({"kind":"call_expression","occurrence":range(call.occurrence_range)}),
        );
        let id = stable(OBSERVATION_DOMAIN, &preimage)?;
        ids.insert(RawTypeScriptKeyV1::Call(call.clone()), id.clone());
        let accepted = accepted(
            index,
            binding,
            SyntaxRole::Call,
            &call.kind,
            call.occurrence_range,
        );
        let (call_key, outcome) = if let Some((key, _)) = accepted {
            let _unknown_obstruction_id = insert_obstruction(
                batch,
                &preimage,
                &id,
                "test_framework_semantics_unaccepted@2",
                Value::Null,
            )?;
            (
                Some(key.clone()),
                TypeScriptG3OutcomeV1::Unknown(
                    TestFrameworkUnknownCodeV1::TestFrameworkSemanticsUnaccepted,
                ),
            )
        } else {
            let reason = TypeScriptG3ReasonV1::MissingAcceptedSyntax;
            let obstruction_id =
                insert_obstruction(batch, &preimage, &id, reason.literal(), reason.detail())?;
            (
                None,
                TypeScriptG3OutcomeV1::Obstructed {
                    obstruction_id,
                    reason,
                },
            )
        };
        obstructed.push(RawTypeScriptKeyV1::Call(call.clone()));
        batch.construct_partition.occurrence_ids.push(id.clone());
        batch.construct_partition.obstruction_ids.push(id.clone());
        batch
            .test_marker_candidates
            .push(TypeScriptG3TestMarkerCandidateV1 {
                id,
                binding: binding.clone(),
                occurrence_range: call.occurrence_range,
                call_key,
                outcome,
            });
    }
    for excluded in &raw.exclusions {
        batch.construct_partition.exclusions.push(G3ExclusionV1 {
            binding: binding.clone(),
            syntax: excluded.clone(),
        });
    }
    partition::verify_bound(raw, &successes, &obstructed)?;
    for (contract, belongs) in [
        ("TS-containment@2", 0_u8),
        ("TS-write@2", 1_u8),
        ("TS-test-marker-candidate@2", 2_u8),
    ] {
        let matches_contract = |key: &RawTypeScriptKeyV1| match key {
            RawTypeScriptKeyV1::Containment(_) => belongs == 0,
            RawTypeScriptKeyV1::Assignment(_) => belongs == 1,
            RawTypeScriptKeyV1::Call(_) => belongs == 2,
        };
        let mut raw_keys = raw
            .census
            .iter()
            .filter(|key| matches_contract(key))
            .cloned()
            .collect::<Vec<_>>();
        let mut success_keys = successes
            .iter()
            .filter(|key| matches_contract(key))
            .cloned()
            .collect::<Vec<_>>();
        let mut obstruction_keys = obstructed
            .iter()
            .filter(|key| matches_contract(key))
            .cloned()
            .collect::<Vec<_>>();
        raw_keys.sort();
        success_keys.sort();
        obstruction_keys.sort();
        let mut occurrence_ids = raw_keys
            .iter()
            .map(|key| {
                ids.get(key)
                    .cloned()
                    .ok_or(G3AccountingError::MissingOccurrence)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut success_ids = success_keys
            .iter()
            .map(|key| {
                ids.get(key)
                    .cloned()
                    .ok_or(G3AccountingError::MissingOccurrence)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut obstruction_ids = obstruction_keys
            .iter()
            .map(|key| {
                ids.get(key)
                    .cloned()
                    .ok_or(G3AccountingError::MissingOccurrence)
            })
            .collect::<Result<Vec<_>, _>>()?;
        occurrence_ids.sort();
        success_ids.sort();
        obstruction_ids.sort();
        let mut exclusion_keys = if belongs == 0 {
            raw.exclusions.iter().filter(|row| matches!(row.reason,
            reviewgraphen_ingest::typescript::g3_syntax::RawTypeScriptExclusionReasonV1::NestedCallable |
            reviewgraphen_ingest::typescript::g3_syntax::RawTypeScriptExclusionReasonV1::UnsupportedCallableForm))
            .cloned().collect::<Vec<_>>()
        } else if belongs == 1 {
            raw.exclusions.iter().filter(|row| matches!(row.reason,
            reviewgraphen_ingest::typescript::g3_syntax::RawTypeScriptExclusionReasonV1::CompoundAssignment |
            reviewgraphen_ingest::typescript::g3_syntax::RawTypeScriptExclusionReasonV1::UpdateExpression))
            .cloned().collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        exclusion_keys.sort();
        batch
            .construct_partition
            .per_file_contracts
            .push(G3FileContractPartitionV1 {
                binding: binding.clone(),
                contract,
                raw_keys,
                success_keys,
                obstruction_keys,
                occurrence_ids,
                success_ids,
                obstruction_ids,
                exclusion_keys,
            });
    }
    Ok(())
}

fn finish_partition(
    p: &ReconstructionProvenanceV1,
    batch: &mut TypeScriptG3BatchV1,
) -> Result<(), G3AccountingError> {
    let part = &mut batch.construct_partition;
    if part.occurrence_ids.iter().collect::<BTreeSet<_>>().len() != part.occurrence_ids.len() {
        return Err(G3AccountingError::IdCollision);
    }
    part.occurrence_ids.sort();
    part.success_ids.sort();
    part.obstruction_ids.sort();
    part.exclusions.sort_by(|a, b| {
        (
            &a.binding.canonical_path,
            a.syntax.range,
            &a.syntax.syntax_kind,
        )
            .cmp(&(
                &b.binding.canonical_path,
                b.syntax.range,
                &b.syntax.syntax_kind,
            ))
    });
    part.latent
        .sort_by_key(|row| latent_preimage(row).to_string());
    let files = batch
        .file_partition
        .iter()
        .map(|file| {
            json!({
                "file_id":file.file_id.canonical_key(), "canonical_source_path":file.canonical_path,
                "source_hash":file.source_hash.as_ref().map(SourceHash::wire_literal),
                "outcome":format!("{:?}",file.outcome),
                "obstruction_id":file.obstruction_id.as_ref().map(StableId::as_str),
            })
        })
        .collect::<Vec<_>>();
    part.per_file_contracts.sort_by(|left, right| {
        (&left.binding.canonical_path, left.contract)
            .cmp(&(&right.binding.canonical_path, right.contract))
    });
    let contracts = part.per_file_contracts.iter().map(|row| json!({
        "contract":row.contract, "file_id":row.binding.file_id.canonical_key(),
        "canonical_source_path":row.binding.canonical_path,
        "raw_keys":row.raw_keys.iter().map(raw_key).collect::<Vec<_>>(),
        "success_keys":row.success_keys.iter().map(raw_key).collect::<Vec<_>>(),
        "obstruction_keys":row.obstruction_keys.iter().map(raw_key).collect::<Vec<_>>(),
        "occurrence_ids":row.occurrence_ids.iter().map(StableId::as_str).collect::<Vec<_>>(),
        "success_ids":row.success_ids.iter().map(StableId::as_str).collect::<Vec<_>>(),
        "obstruction_ids":row.obstruction_ids.iter().map(StableId::as_str).collect::<Vec<_>>(),
        "exclusion_keys":row.exclusion_keys.iter().map(|key| json!({
            "range":range(key.range), "kind":key.syntax_kind, "reason":format!("{:?}",key.reason)
        })).collect::<Vec<_>>(),
    })).collect::<Vec<_>>();
    let mut excluded = part
        .exclusions
        .iter()
        .map(|row| {
            json!({
                "path":row.binding.canonical_path,"range":range(row.syntax.range),
                "kind":row.syntax.syntax_kind,"reason":format!("{:?}",row.syntax.reason)
            })
        })
        .collect::<Vec<_>>();
    excluded.sort_by_key(|item| item.to_string());
    let preimage = BTreeMap::from([
        ("version".into(), json!(2)),
        ("profile".into(), json!(PROFILE)),
        ("extractor".into(), json!(EXTRACTOR)),
        (
            "snapshot".into(),
            json!({"kind":"ts_a0","value":p.snapshot_binding.as_str()}),
        ),
        (
            "registry".into(),
            json!({"registry_hash":p.registry_binding.registry_hash,
            "tuple_hash":registry_tuple_hash(&p.registry_binding)}),
        ),
        ("files".into(), json!(files)),
        (
            "inventory_complete".into(),
            json!(p.source_material.inventory_complete),
        ),
        ("contracts".into(), json!(contracts)),
        ("exclusion_keys".into(), json!(excluded)),
        (
            "latent".into(),
            json!(part.latent.iter().map(latent_preimage).collect::<Vec<_>>()),
        ),
    ]);
    part.id = stable(PARTITION_DOMAIN, &preimage)?;
    Ok(())
}

#[cfg(test)]
mod latent_tests {
    use super::super::{SourceAdmissionBoundsV1, admit_typescript_revision_pair};
    use super::*;
    use std::{fs, path::Path, process::Command};

    fn git(root: &Path, args: &[&str]) -> String {
        let mut command = Command::new("git");
        command
            .env_clear()
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("LC_ALL", "C")
            .args(args);
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        let output = command.output().expect("fixture Git invocation");
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn parsed_empty_has_zero_constructs_while_nonparsed_files_have_bound_unknown_counts() {
        let workspace = tempfile::tempdir().expect("fixture workspace");
        let repository = workspace.path().join("repository");
        fs::create_dir(&repository).unwrap();
        git(&repository, &["init", "--quiet"]);
        fs::write(repository.join("README.md"), b"base\n").unwrap();
        git(&repository, &["add", "."]);
        let commit = [
            "-c",
            "user.name=G3 fixture",
            "-c",
            "user.email=g3@example.invalid",
            "commit",
            "--quiet",
            "-m",
        ];
        let mut base_args = commit.to_vec();
        base_args.push("base");
        git(&repository, &base_args);
        let base = git(&repository, &["rev-parse", "HEAD"]);
        fs::create_dir(repository.join("src")).unwrap();
        let malformed = b"let = ;\n";
        fs::write(repository.join("src/broken.ts"), malformed).unwrap();
        fs::write(repository.join("src/empty.ts"), b"").unwrap();
        fs::write(
            repository.join("src/unread.ts"),
            b"export const unread = 1;\n",
        )
        .unwrap();
        git(&repository, &["add", "."]);
        let mut target_args = commit.to_vec();
        target_args.push("target");
        git(&repository, &target_args);
        let target = git(&repository, &["rev-parse", "HEAD"]);

        let (_, context) = admit_typescript_revision_pair(
            workspace.path().to_owned(),
            repository,
            base,
            target,
            SourceAdmissionBoundsV1 {
                max_files: 2,
                max_file_bytes: 1024,
                max_total_source_bytes: 4096,
            },
        )
        .expect("real A0 literal Git admission");
        let batch = context
            .g3_observations()
            .expect("nonparsed files are accounted");
        let files = batch.file_partition();
        let empty = files
            .iter()
            .find(|row| row.canonical_path == "src/empty.ts")
            .unwrap();
        assert_eq!(empty.outcome, SourceFileOutcome::Parsed);
        assert_eq!(empty.source_hash, Some(SourceHash::from_source_bytes(b"")));
        assert_eq!(
            batch
                .construct_partition
                .per_file_contracts
                .iter()
                .filter(
                    |row| row.binding.canonical_path == "src/empty.ts" && row.raw_keys.is_empty()
                )
                .count(),
            3
        );
        assert!(
            !batch
                .construct_partition
                .latent
                .iter()
                .any(|row| matches!(row,
            G3LatentV1::FileConstructs { canonical_path, .. } if canonical_path == "src/empty.ts"))
        );

        let latent = &batch.construct_partition.latent;
        assert_eq!(
            latent.len(),
            2,
            "no unexpanded ancestor and no latent count on parsed empty"
        );
        for (path, outcome, hash) in [
            (
                "src/broken.ts",
                SourceFileOutcome::ParseFailed,
                Some(SourceHash::from_source_bytes(malformed)),
            ),
            ("src/unread.ts", SourceFileOutcome::UnreadBound, None),
        ] {
            let partition = files.iter().find(|row| row.canonical_path == path).unwrap();
            let expected_file_id =
                SourceFileId::from_basis_file_key(CanonicalFileKey::from_basis_path(path).unwrap());
            assert_eq!(partition.file_id, expected_file_id);
            assert_eq!(partition.outcome, outcome);
            assert_eq!(partition.source_hash, hash);
            let row = latent
                .iter()
                .find(|row| {
                    matches!(row,
                G3LatentV1::FileConstructs { canonical_path, .. } if canonical_path == path)
                })
                .unwrap();
            assert!(matches!(row, G3LatentV1::FileConstructs {
                file_id, source_hash, outcome: actual, obstruction_id, count: G3LatentCardinalityV1::Unknown,
                ..
            } if file_id == &expected_file_id && source_hash == &hash && *actual == outcome
                && Some(obstruction_id) == partition.obstruction_id.as_ref()));
            let preimage = latent_preimage(row);
            assert_eq!(preimage["kind"], "file_constructs");
            assert_eq!(preimage["file_id"], expected_file_id.canonical_key());
            assert_eq!(preimage["canonical_source_path"], path);
            assert_eq!(
                preimage["source_hash"],
                json!(hash.as_ref().map(SourceHash::wire_literal))
            );
            assert_eq!(preimage["count"], "unknown");
        }
        assert!(!latent.iter().any(|row| matches!(row,
            G3LatentV1::FileConstructs { canonical_path, .. } if canonical_path == "README.md")));
    }
}
