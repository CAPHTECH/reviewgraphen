use reviewgraphen_core::{ContentHash, StableId};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RustInclusiveLineColumnRange {
    start_line: u64,
    start_column: u64,
    end_line: u64,
    end_column: u64,
}

impl RustInclusiveLineColumnRange {
    pub(crate) fn new(sl: u64, sc: u64, el: u64, ec: u64) -> Option<Self> {
        (sl > 0 && sc > 0 && el > 0 && ec > 0 && (sl, sc) <= (el, ec)).then_some(Self {
            start_line: sl,
            start_column: sc,
            end_line: el,
            end_column: ec,
        })
    }

    pub(crate) const fn start_line(self) -> u64 {
        self.start_line
    }
    pub(crate) const fn start_column(self) -> u64 {
        self.start_column
    }
    pub(crate) const fn end_line(self) -> u64 {
        self.end_line
    }
    pub(crate) const fn end_column(self) -> u64 {
        self.end_column
    }

    pub(super) fn value(self) -> Value {
        json!({"start_line":self.start_line,"start_column":self.start_column,
               "end_line":self.end_line,"end_column":self.end_column})
    }

    pub(super) fn overlaps(self, other: Self) -> bool {
        (self.start_line, self.start_column) <= (other.end_line, other.end_column)
            && (other.start_line, other.start_column) <= (self.end_line, self.end_column)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum RustLhsKindV1 {
    Identifier,
    Field,
    Index,
    Dereference,
    Tuple,
    Pattern,
    Other,
}
impl RustLhsKindV1 {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Identifier => "identifier",
            Self::Field => "field",
            Self::Index => "index",
            Self::Dereference => "dereference",
            Self::Tuple => "tuple",
            Self::Pattern => "pattern",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum RawKey {
    Declaration {
        occurrence: RustInclusiveLineColumnRange,
        logical_name: String,
    },
    Containment {
        parent: RustInclusiveLineColumnRange,
        child: RustInclusiveLineColumnRange,
        parent_logical_name: String,
        child_logical_name: String,
    },
    Assignment {
        occurrence: RustInclusiveLineColumnRange,
        lhs: RustInclusiveLineColumnRange,
        lhs_kind: RustLhsKindV1,
        lhs_name: Option<String>,
        owner_syntax: Option<RustInclusiveLineColumnRange>,
        owner_logical_name: Option<String>,
    },
    TestAttribute {
        occurrence: RustInclusiveLineColumnRange,
        function_syntax: RustInclusiveLineColumnRange,
        owner_logical_name: String,
    },
}

impl RawKey {
    pub(super) fn contract(&self) -> &'static str {
        match self {
            Self::Declaration { .. } => "R-declaration@2",
            Self::Containment { .. } => "R-containment@2",
            Self::Assignment { .. } => "R-write@2",
            Self::TestAttribute { .. } => "R-test-marker@2",
        }
    }
    pub(super) fn syntax(&self) -> Value {
        match self {
            Self::Declaration {
                occurrence,
                logical_name,
            } => {
                json!({"kind":"free_function","occurrence":occurrence.value(),"logical_name":logical_name})
            }
            Self::Containment {
                parent,
                child,
                parent_logical_name,
                child_logical_name,
            } => json!({"kind":"inline_module_direct_function","parent":parent.value(),
                    "child":child.value(),"parent_logical_name":parent_logical_name,
                    "child_logical_name":child_logical_name}),
            Self::Assignment {
                occurrence,
                lhs,
                lhs_kind,
                owner_syntax,
                owner_logical_name,
                ..
            } => json!({"kind":"assignment","occurrence":occurrence.value(),"lhs":lhs.value(),
                    "lhs_kind":lhs_kind.label(),"owner_syntax":owner_syntax.map(|r|r.value()),
                    "owner_logical_name":owner_logical_name}),
            Self::TestAttribute {
                occurrence,
                function_syntax,
                owner_logical_name,
            } => json!({"kind":"exact_test_attribute","occurrence":occurrence.value(),
                    "function_syntax":function_syntax.value(),"owner_logical_name":owner_logical_name}),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RawRustFileV1 {
    pub(super) declarations: Vec<RawKey>,
    pub(super) containment: Vec<RawKey>,
    pub(super) assignments: Vec<RawKey>,
    pub(super) test_attributes: Vec<RawKey>,
    pub(super) census: Vec<RawKey>,
    pub(super) exclusions: Vec<RustExclusionV1>,
    pub(super) limitations: Vec<RustLimitationV1>,
    pub(super) legacy_first_writes: BTreeMap<String, RustInclusiveLineColumnRange>,
    pub(super) accepted_locations: BTreeMap<RawKey, RustInclusiveLineColumnRange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RustExclusionV1 {
    pub(super) path: String,
    pub(super) range: RustInclusiveLineColumnRange,
    pub(super) kind: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RustLimitationV1 {
    pub(super) path: String,
    pub(super) range: RustInclusiveLineColumnRange,
    pub(super) kind: &'static str,
    pub(super) latent_occurrence_count: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RawRustParseObstructionV1 {
    InvalidUtf8,
    ParseFailed,
    InvalidRange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum G3AccountingError {
    MissingOccurrence { key: String },
    UnexpectedOccurrence { key: String },
    DoubleDisposition { key: String },
    DuplicateOccurrence { key: String },
    DuplicateFile { path: String },
    BindingMismatch { path: String },
    IdentityCollision { id: StableId },
    IdDerivation { domain: &'static str },
    GitExclusionMismatch { path: String },
}

/// A Git exclusion has unknown latent constructs, not a known zero count.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GitLatentOccurrenceCountV1 {
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RustGitExclusionRefV1 {
    pub(super) path: String,
    pub(super) entry_kind: crate::git::GitExcludedEntryKind,
    pub(super) kind: crate::IngestionObstructionKind,
    pub(super) legacy_obstruction_id: StableId,
    pub(super) latent_occurrence_count: GitLatentOccurrenceCountV1,
}

#[allow(dead_code)] // The private getters are exercised by the frozen G3 acceptance.
impl RustGitExclusionRefV1 {
    pub(crate) fn new(
        path: String,
        entry_kind: crate::git::GitExcludedEntryKind,
        kind: crate::IngestionObstructionKind,
        legacy_obstruction_id: StableId,
    ) -> Self {
        Self {
            path,
            entry_kind,
            kind,
            legacy_obstruction_id,
            latent_occurrence_count: GitLatentOccurrenceCountV1::Unknown,
        }
    }

    pub(crate) fn path(&self) -> &str {
        &self.path
    }
    pub(crate) fn entry_kind(&self) -> crate::git::GitExcludedEntryKind {
        self.entry_kind
    }
    pub(crate) fn kind(&self) -> crate::IngestionObstructionKind {
        self.kind
    }
    pub(crate) fn legacy_obstruction_id(&self) -> &StableId {
        &self.legacy_obstruction_id
    }
    pub(crate) fn latent_occurrence_count(&self) -> GitLatentOccurrenceCountV1 {
        self.latent_occurrence_count
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RustFileBindingV1 {
    pub(super) snapshot_id: StableId,
    pub(super) file_id: StableId,
    pub(super) canonical_path: String,
    pub(super) source_hash: ContentHash,
}
impl RustFileBindingV1 {
    pub(crate) fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    pub(crate) fn file_id(&self) -> &StableId {
        &self.file_id
    }
    pub(crate) fn canonical_path(&self) -> &str {
        &self.canonical_path
    }
    pub(crate) fn source_hash(&self) -> &ContentHash {
        &self.source_hash
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RustFactRefV1 {
    Artifact {
        id: StableId,
    },
    Relation {
        id: StableId,
        source_id: StableId,
        target_ids: BTreeSet<StableId>,
    },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RustG3ReasonV1 {
    UnsupportedAssignmentLhs,
    UnsupportedOwner,
    MissingAcceptedFact,
    AmbiguousAcceptedFact,
    AcceptedLocationMismatch,
    OverlappingOccurrence,
}
impl RustG3ReasonV1 {
    pub(super) const fn label(&self) -> &'static str {
        match self {
            Self::UnsupportedAssignmentLhs => "unsupported_assignment_lhs@2",
            Self::UnsupportedOwner => "unsupported_owner@2",
            Self::MissingAcceptedFact => "missing_accepted_fact@2",
            Self::AmbiguousAcceptedFact => "ambiguous_accepted_fact@2",
            Self::AcceptedLocationMismatch => "accepted_location_mismatch@2",
            Self::OverlappingOccurrence => "overlapping_occurrence@2",
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RustG3OutcomeV1 {
    ExistingFact(RustFactRefV1),
    Obstructed {
        obstruction_id: StableId,
        reason: RustG3ReasonV1,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RustG3RowV1 {
    pub(super) id: StableId,
    pub(super) source: RustFileBindingV1,
    pub(super) key: RawKey,
    pub(super) outcome: RustG3OutcomeV1,
}
#[allow(dead_code)] // The admitted rows are first read by the frozen acceptance and later G3 consumers.
impl RustG3RowV1 {
    pub(crate) fn id(&self) -> &StableId {
        &self.id
    }
    pub(crate) fn source(&self) -> &RustFileBindingV1 {
        &self.source
    }
    pub(crate) fn outcome(&self) -> &RustG3OutcomeV1 {
        &self.outcome
    }
    pub(crate) fn occurrence(&self) -> RustInclusiveLineColumnRange {
        match &self.key {
            RawKey::Declaration { occurrence, .. }
            | RawKey::Assignment { occurrence, .. }
            | RawKey::TestAttribute { occurrence, .. } => *occurrence,
            RawKey::Containment { child, .. } => *child,
        }
    }
    pub(crate) fn parent_range(&self) -> RustInclusiveLineColumnRange {
        match &self.key {
            RawKey::Containment { parent, .. } => *parent,
            _ => self.occurrence(),
        }
    }
    pub(crate) fn child_range(&self) -> RustInclusiveLineColumnRange {
        self.occurrence()
    }
    pub(crate) fn lhs_range(&self) -> RustInclusiveLineColumnRange {
        match &self.key {
            RawKey::Assignment { lhs, .. } => *lhs,
            _ => self.occurrence(),
        }
    }
    pub(crate) fn lhs_kind(&self) -> RustLhsKindV1 {
        match &self.key {
            RawKey::Assignment { lhs_kind, .. } => *lhs_kind,
            _ => RustLhsKindV1::Other,
        }
    }
    pub(crate) fn function_syntax_range(&self) -> RustInclusiveLineColumnRange {
        match &self.key {
            RawKey::TestAttribute {
                function_syntax, ..
            } => *function_syntax,
            _ => self.occurrence(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RustG3ObstructionV1 {
    pub(super) id: StableId,
    pub(super) occurrence_id: StableId,
    pub(super) source: RustFileBindingV1,
    pub(super) reason: String,
    pub(super) detail: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RustFileOutcomeV1 {
    Observed,
    ParseObstructed {
        reason: &'static str,
        latent: &'static str,
    },
    NonRustExcluded,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RustFileDispositionV1 {
    pub(super) source: RustFileBindingV1,
    pub(super) outcome: RustFileOutcomeV1,
}
#[allow(dead_code)] // The source binding is read by the frozen G3 acceptance.
impl RustFileDispositionV1 {
    pub(crate) fn source(&self) -> &RustFileBindingV1 {
        &self.source
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RustFilePartitionV1 {
    pub(super) id: StableId,
    pub(super) files: Vec<RustFileDispositionV1>,
}
#[allow(dead_code)]
impl RustFilePartitionV1 {
    pub(crate) fn id(&self) -> &StableId {
        &self.id
    }
    pub(crate) fn files(&self) -> &[RustFileDispositionV1] {
        &self.files
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RustConstructPartitionV1 {
    pub(super) id: StableId,
    pub(super) occurrence_ids: BTreeMap<String, Vec<StableId>>,
    pub(super) success_ids: BTreeMap<String, Vec<StableId>>,
    pub(super) obstruction_ids: BTreeMap<String, Vec<StableId>>,
    pub(super) exclusions: Vec<RustExclusionV1>,
    pub(super) limitations: Vec<RustLimitationV1>,
}
#[allow(dead_code)]
impl RustConstructPartitionV1 {
    pub(crate) fn id(&self) -> &StableId {
        &self.id
    }
    pub(crate) fn exclusions(&self) -> &[RustExclusionV1] {
        &self.exclusions
    }
    pub(crate) fn limitations(&self) -> &[RustLimitationV1] {
        &self.limitations
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RustG3BatchV1 {
    pub(super) snapshot_id: StableId,
    pub(super) git_exclusion_refs: Vec<RustGitExclusionRefV1>,
    pub(super) declarations: Vec<RustG3RowV1>,
    pub(super) containment: Vec<RustG3RowV1>,
    pub(super) writes: Vec<RustG3RowV1>,
    pub(super) test_markers: Vec<RustG3RowV1>,
    pub(super) file_partition: RustFilePartitionV1,
    pub(super) construct_partition: RustConstructPartitionV1,
    pub(super) obstructions: Vec<RustG3ObstructionV1>,
}
#[allow(dead_code)]
impl RustG3BatchV1 {
    pub(crate) fn git_exclusion_refs(&self) -> &[RustGitExclusionRefV1] {
        &self.git_exclusion_refs
    }
    pub(crate) fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    pub(crate) fn declarations(&self) -> &[RustG3RowV1] {
        &self.declarations
    }
    pub(crate) fn containment(&self) -> &[RustG3RowV1] {
        &self.containment
    }
    pub(crate) fn writes(&self) -> &[RustG3RowV1] {
        &self.writes
    }
    pub(crate) fn test_markers(&self) -> &[RustG3RowV1] {
        &self.test_markers
    }
    pub(crate) fn file_partition(&self) -> &RustFilePartitionV1 {
        &self.file_partition
    }
    pub(crate) fn construct_partition(&self) -> &RustConstructPartitionV1 {
        &self.construct_partition
    }
    pub(crate) fn obstructions(&self) -> &[RustG3ObstructionV1] {
        &self.obstructions
    }
    pub(crate) fn obstruction(&self, id: &StableId) -> Option<&RustG3ObstructionV1> {
        self.obstructions.iter().find(|row| &row.id == id)
    }
}
