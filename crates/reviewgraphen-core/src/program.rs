use crate::{ContentHash, DomainError, Result, ReviewStatus, StableId};
use serde::de::{MapAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
#[cfg(test)]
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
thread_local! {
    static PROGRAM_SPACE_GENERIC_SERIALIZE_CALLS: Cell<u64> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_program_space_generic_serialize_calls() {
    PROGRAM_SPACE_GENERIC_SERIALIZE_CALLS.with(|calls| calls.set(0));
}

#[cfg(test)]
pub(crate) fn program_space_generic_serialize_calls() -> u64 {
    PROGRAM_SPACE_GENERIC_SERIALIZE_CALLS.with(Cell::get)
}

/// Provenance for a deterministic fact or evidence record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceRef {
    /// Origin category (`fixture`, `tool`, `human`, and so on).
    kind: String,
    /// Stable external locator.
    locator: String,
    /// Optional revision at the origin.
    revision: Option<String>,
    /// Optional content hash of the origin.
    content_hash: Option<ContentHash>,
    /// Optional origin-local identity.
    source_local_id: Option<String>,
}

impl SourceRef {
    fn allocated_bytes(&self) -> usize {
        self.kind
            .capacity()
            .saturating_add(self.locator.capacity())
            .saturating_add(self.revision.as_ref().map_or(0, String::capacity))
            .saturating_add(
                self.content_hash
                    .as_ref()
                    .map_or(0, ContentHash::allocated_bytes),
            )
            .saturating_add(self.source_local_id.as_ref().map_or(0, String::capacity))
    }
    /// Creates a source reference after enforcing the input-contract source kinds.
    pub fn new(
        kind: impl Into<String>,
        locator: impl Into<String>,
        revision: Option<String>,
        content_hash: Option<ContentHash>,
        source_local_id: Option<String>,
    ) -> Result<Self> {
        let kind = kind.into();
        let locator = locator.into();
        require_enum(
            &kind,
            &[
                "git", "file", "tool", "human", "model", "external", "fixture", "custom",
            ],
            "source.kind",
        )?;
        ensure_non_empty(&kind, "source.kind")?;
        ensure_non_empty(&locator, "source.locator")?;
        Ok(Self {
            kind,
            locator,
            revision,
            content_hash,
            source_local_id,
        })
    }

    /// Source kind from the declared input vocabulary.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Stable external source locator.
    #[must_use]
    pub fn locator(&self) -> &str {
        &self.locator
    }

    /// Optional content hash of the origin, when the adapter recorded one.
    #[must_use]
    pub fn content_hash(&self) -> Option<&ContentHash> {
        self.content_hash.as_ref()
    }

    /// Optional origin-local identity, such as a repo-relative path.
    #[must_use]
    pub fn source_local_id(&self) -> Option<&str> {
        self.source_local_id.as_deref()
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let _ = Self::new(
            self.kind.clone(),
            self.locator.clone(),
            self.revision.clone(),
            self.content_hash.clone(),
            self.source_local_id.clone(),
        )?;
        Ok(())
    }
}

/// Extraction provenance kept separate from program facts themselves.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Provenance {
    /// Input source.
    source: SourceRef,
    /// Deterministic extractor or manual adapter identity.
    extraction_method: String,
    /// Optional tool version.
    tool_version: Option<String>,
    /// Confidence is descriptive only and has no acceptance authority.
    confidence: Option<f64>,
    /// Review state of the record.
    review_status: ReviewStatus,
}

impl Provenance {
    fn allocated_bytes(&self) -> usize {
        self.source
            .allocated_bytes()
            .saturating_add(self.extraction_method.capacity())
            .saturating_add(self.tool_version.as_ref().map_or(0, String::capacity))
    }
    /// Creates provenance eligible for a canonical Program fact or review evidence.
    pub fn accepted_deterministic(
        source: SourceRef,
        extraction_method: impl Into<String>,
        tool_version: Option<String>,
        confidence: Option<f64>,
    ) -> Result<Self> {
        let extraction_method = extraction_method.into();
        ensure_non_empty(&extraction_method, "provenance.extraction_method")?;
        if confidence.is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value)) {
            return Err(DomainError::Validation(
                "provenance confidence must be finite and between 0 and 1".to_owned(),
            ));
        }
        if !matches!(
            source.kind(),
            "git" | "file" | "tool" | "external" | "fixture"
        ) {
            return Err(DomainError::Validation(
                "canonical evidence provenance must have a deterministic source authority"
                    .to_owned(),
            ));
        }
        Ok(Self {
            source,
            extraction_method,
            tool_version,
            confidence,
            review_status: ReviewStatus::Accepted,
        })
    }

    /// Source authority for this observation.
    #[must_use]
    pub fn source(&self) -> &SourceRef {
        &self.source
    }

    /// Deterministic extraction method.
    #[must_use]
    pub fn extraction_method(&self) -> &str {
        &self.extraction_method
    }

    /// Review status, fixed to accepted for canonical facts.
    #[must_use]
    pub const fn review_status(&self) -> ReviewStatus {
        self.review_status
    }

    pub(crate) fn validate_for_canonical_evidence(&self) -> Result<()> {
        let _ = Self::accepted_deterministic(
            self.source.clone(),
            self.extraction_method.clone(),
            self.tool_version.clone(),
            self.confidence,
        )?;
        if self.review_status != ReviewStatus::Accepted {
            return Err(DomainError::Validation(
                "canonical evidence provenance must be accepted".to_owned(),
            ));
        }
        self.source.validate()
    }
}

/// Shared descriptive severity for invariants and extraction limitations.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Informational retained detail.
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

/// Completeness state for one named extraction capability.
///
/// `Complete` and `Partial` are deliberately distinct: only `Complete` fully
/// satisfies a rule's capability requirement. `Partial` still lets already
/// resolved facts produce grounded, targeted obligations, but it must not be
/// silently treated as if the capability were fully available.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    /// The adapter completely covered its declared input subset.
    Complete,
    /// The adapter returned facts while retaining explicit unknown regions.
    Partial,
    /// The adapter cannot provide this fact family for the snapshot.
    Missing,
    /// The adapter did not establish a completeness result.
    Unknown,
}

/// Bounded completion state for one adapter run.
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

/// Typed category for an extraction limitation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitationKind {
    /// A required capability is entirely unavailable for the snapshot.
    CapabilityMissing,
    /// A source region could not be parsed.
    ParseFailure,
    /// A syntactic or semantic relation could not be resolved.
    UnresolvedRelation,
    /// A repository entry or region was deliberately excluded by a bound.
    ExcludedRegion,
    /// A projection or extraction step lost information.
    ProjectionLoss,
    /// Policy restricted extraction of an otherwise available fact.
    PolicyRestriction,
    /// The input was outside the bounded extractor contract.
    UnsupportedInput,
    /// A deliberately conservative unknown that does not fit a stronger class.
    Unknown,
}

/// A language-neutral accepted ProgramSpace artifact.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Artifact {
    /// Stable artifact ID.
    pub id: StableId,
    /// Profile-defined kind such as `function`, `event`, or `test`.
    pub kind: String,
    /// Human-readable label.
    pub label: String,
    /// Optional implementation language.
    pub language: Option<String>,
    /// Optional source location.
    pub location: Option<Location>,
    /// Optional source content hash.
    pub content_hash: Option<ContentHash>,
    /// Adapter-specific factual attributes, with sorted keys.
    pub attributes: BTreeMap<String, Value>,
    /// Deterministic provenance.
    pub provenance: Provenance,
}

const ARTIFACT_KINDS: &[&str] = &[
    "repository",
    "snapshot",
    "file",
    "module",
    "package",
    "class",
    "type",
    "function",
    "method",
    "field",
    "route",
    "event",
    "state",
    "api",
    "database",
    "config",
    "permission",
    "test",
    "requirement",
    "policy",
    "owner",
    "external_service",
    "custom",
];

impl Artifact {
    /// Constructs and validates one accepted ProgramSpace artifact. This is the
    /// single validation boundary shared by the JSON adapter input path and any
    /// typed `ProgramSpaceBuilder` caller.
    // Each parameter is a distinct field of the accepted, schema-shaped
    // artifact contract validated here; grouping them into a struct would
    // change this stable public boundary rather than reduce arity.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: StableId,
        kind: impl Into<String>,
        label: impl Into<String>,
        language: Option<String>,
        location: Option<Location>,
        content_hash: Option<ContentHash>,
        attributes: BTreeMap<String, Value>,
        provenance: Provenance,
    ) -> Result<Self> {
        let kind = kind.into();
        require_enum(&kind, ARTIFACT_KINDS, "artifact.kind")?;
        let label = label.into();
        ensure_non_empty(&label, "artifact.label")?;
        if let Some(location) = &location {
            location.validate()?;
        }
        Ok(Self {
            id,
            kind,
            label,
            language,
            location,
            content_hash,
            attributes,
            provenance,
        })
    }

    /// Re-validates an already-constructed artifact, including a nested
    /// [`Location`]. This is the boundary [`ProgramSpaceBuilder::build`] uses
    /// so an `Artifact` assembled via its public struct-literal fields
    /// (bypassing [`Self::new`]) cannot smuggle an invalid `kind`, an empty
    /// `label`, or a malformed `location` into a validated `ProgramSpace`.
    pub(crate) fn validate(&self) -> Result<()> {
        let _ = Self::new(
            self.id.clone(),
            self.kind.clone(),
            self.label.clone(),
            self.language.clone(),
            self.location.clone(),
            self.content_hash.clone(),
            self.attributes.clone(),
            self.provenance.clone(),
        )?;
        Ok(())
    }
}

/// A source location that remains evidence, rather than an identifier by itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Location {
    /// Path relative to the snapshot root.
    pub path: String,
    /// Optional one-based start line.
    pub start_line: Option<u64>,
    /// Optional one-based end line.
    pub end_line: Option<u64>,
    /// Optional one-based start column.
    pub start_column: Option<u64>,
    /// Optional one-based end column.
    pub end_column: Option<u64>,
    /// Optional referenced symbol.
    pub symbol_id: Option<StableId>,
}

impl Location {
    /// Constructs and validates a source location: a normalized,
    /// workspace-relative snapshot path and a 1-based, all-or-none, ordered
    /// range.
    pub fn new(
        path: impl Into<String>,
        start_line: Option<u64>,
        end_line: Option<u64>,
        start_column: Option<u64>,
        end_column: Option<u64>,
        symbol_id: Option<StableId>,
    ) -> Result<Self> {
        let path = path.into();
        validate_snapshot_relative_path(&path)?;

        for (field, value) in [
            ("location.start_line", start_line),
            ("location.end_line", end_line),
            ("location.start_column", start_column),
            ("location.end_column", end_column),
        ] {
            if value == Some(0) {
                return Err(DomainError::Validation(format!(
                    "{field} must be at least 1"
                )));
            }
        }
        if start_line.is_none() != end_line.is_none() {
            return Err(DomainError::Validation(
                "location.start_line and location.end_line must both be present or both be absent"
                    .to_owned(),
            ));
        }
        if start_column.is_none() != end_column.is_none() {
            return Err(DomainError::Validation(
                "location.start_column and location.end_column must both be present or both be absent"
                    .to_owned(),
            ));
        }
        if start_line.is_none() && start_column.is_some() {
            return Err(DomainError::Validation(
                "location column range requires a corresponding line range".to_owned(),
            ));
        }
        if let (Some(start), Some(end)) = (start_line, end_line)
            && end < start
        {
            return Err(DomainError::Validation(
                "location.end_line must not precede location.start_line".to_owned(),
            ));
        }
        if start_line.is_some()
            && start_line == end_line
            && let (Some(start), Some(end)) = (start_column, end_column)
            && end < start
        {
            return Err(DomainError::Validation(
                "location.end_column must not precede location.start_column on the same line"
                    .to_owned(),
            ));
        }
        Ok(Self {
            path,
            start_line,
            end_line,
            start_column,
            end_column,
            symbol_id,
        })
    }

    /// Re-validates an already-constructed location. This is the boundary
    /// used by [`ProgramSpaceBuilder::build`] so a `Location` assembled via
    /// its public struct-literal fields (bypassing [`Self::new`]) cannot
    /// smuggle an absolute path, a `..` segment, or a malformed range into a
    /// validated `ProgramSpace`.
    pub(crate) fn validate(&self) -> Result<()> {
        let _ = Self::new(
            self.path.clone(),
            self.start_line,
            self.end_line,
            self.start_column,
            self.end_column,
            self.symbol_id.clone(),
        )?;
        Ok(())
    }
}

/// Rejects a `location.path` that is not a normalized, workspace-relative
/// snapshot path: empty, absolute (POSIX or Windows drive/UNC), or containing
/// a `.` or `..` segment.
fn validate_snapshot_relative_path(path: &str) -> Result<()> {
    if path.is_empty() {
        return Err(DomainError::EmptyField {
            field: "location.path",
        });
    }
    if path.contains('\0') {
        return Err(DomainError::Validation(
            "location.path must not contain a NUL character".to_owned(),
        ));
    }
    // A relative `location.path` is a portable, `/`-delimited snapshot path.
    // A literal `\` is rejected outright rather than normalized as an
    // alternate separator: silently accepting it would let one snapshot path
    // mean two different things depending on the platform that later resolves
    // it against a workspace root.
    if path.contains('\\') {
        return Err(DomainError::Validation(
            "location.path must not contain `\\`; use `/` as the only path separator".to_owned(),
        ));
    }
    if path.starts_with('/') {
        return Err(DomainError::Validation(
            "location.path must be workspace-relative, not absolute".to_owned(),
        ));
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(DomainError::Validation(
            "location.path must not use a Windows drive prefix".to_owned(),
        ));
    }
    for segment in path.split('/') {
        match segment {
            "" => {
                return Err(DomainError::Validation(
                    "location.path must not contain empty segments".to_owned(),
                ));
            }
            "." => {
                return Err(DomainError::Validation(
                    "location.path must not contain `.` segments".to_owned(),
                ));
            }
            ".." => {
                return Err(DomainError::Validation(
                    "location.path must not contain `..` segments".to_owned(),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// A typed relation in ProgramSpace.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Relation {
    /// Stable relation ID.
    pub id: StableId,
    /// Profile-defined relation kind.
    pub kind: String,
    /// Relation origin.
    pub source_id: StableId,
    /// Sorted target IDs.
    pub target_ids: BTreeSet<StableId>,
    /// Whether relation direction is meaningful.
    pub directed: bool,
    /// Adapter-specific factual attributes.
    pub attributes: BTreeMap<String, Value>,
    /// Deterministic provenance.
    pub provenance: Provenance,
}

impl Relation {
    /// Constructs and validates one accepted ProgramSpace relation.
    pub fn new(
        id: StableId,
        kind: impl Into<String>,
        source_id: StableId,
        target_ids: BTreeSet<StableId>,
        directed: bool,
        attributes: BTreeMap<String, Value>,
        provenance: Provenance,
    ) -> Result<Self> {
        let kind = kind.into();
        ensure_non_empty(&kind, "relation.kind")?;
        if target_ids.is_empty() {
            return Err(DomainError::EmptyField {
                field: "relation.target_ids",
            });
        }
        Ok(Self {
            id,
            kind,
            source_id,
            target_ids,
            directed,
            attributes,
            provenance,
        })
    }

    /// Re-validates an already-constructed relation. This is the boundary
    /// [`ProgramSpaceBuilder::build`] uses so a `Relation` assembled via its
    /// public struct-literal fields (bypassing [`Self::new`]) cannot smuggle
    /// an empty `kind` or an empty `target_ids` into a validated
    /// `ProgramSpace`.
    pub(crate) fn validate(&self) -> Result<()> {
        let _ = Self::new(
            self.id.clone(),
            self.kind.clone(),
            self.source_id.clone(),
            self.target_ids.clone(),
            self.directed,
            self.attributes.clone(),
            self.provenance.clone(),
        )?;
        Ok(())
    }
}

/// A named local review context; it is not a claim or a decision.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReviewContext {
    /// Stable context ID.
    pub id: StableId,
    /// Profile-defined context kind.
    pub kind: String,
    /// Human-readable label.
    pub label: String,
    /// Sorted member IDs.
    pub member_ids: BTreeSet<StableId>,
    /// Adapter-specific attributes.
    pub attributes: BTreeMap<String, Value>,
    /// Deterministic provenance.
    pub provenance: Provenance,
}

impl ReviewContext {
    /// Constructs and validates one accepted ProgramSpace review context.
    pub fn new(
        id: StableId,
        kind: impl Into<String>,
        label: impl Into<String>,
        member_ids: BTreeSet<StableId>,
        attributes: BTreeMap<String, Value>,
        provenance: Provenance,
    ) -> Result<Self> {
        let kind = kind.into();
        ensure_non_empty(&kind, "context.kind")?;
        let label = label.into();
        ensure_non_empty(&label, "context.label")?;
        Ok(Self {
            id,
            kind,
            label,
            member_ids,
            attributes,
            provenance,
        })
    }

    /// Re-validates an already-constructed review context. This is the
    /// boundary [`ProgramSpaceBuilder::build`] uses so a `ReviewContext`
    /// assembled via its public struct-literal fields (bypassing
    /// [`Self::new`]) cannot smuggle an empty `kind` or `label` into a
    /// validated `ProgramSpace`.
    pub(crate) fn validate(&self) -> Result<()> {
        let _ = Self::new(
            self.id.clone(),
            self.kind.clone(),
            self.label.clone(),
            self.member_ids.clone(),
            self.attributes.clone(),
            self.provenance.clone(),
        )?;
        Ok(())
    }
}

/// A declared property that applies over a program scope.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Invariant {
    /// Stable invariant ID.
    pub id: StableId,
    /// Versioned property name.
    pub property_id: String,
    /// Human-readable scope statement.
    pub description: String,
    /// Sorted scope IDs.
    pub scope_ids: BTreeSet<StableId>,
    /// Severity is descriptive and not an acceptance state.
    pub severity: Severity,
    /// Optional expected verification approach.
    pub verification_mode: Option<String>,
    /// Deterministic provenance.
    pub provenance: Provenance,
}

impl Invariant {
    /// Constructs and validates one accepted ProgramSpace invariant.
    pub fn new(
        id: StableId,
        property_id: impl Into<String>,
        description: impl Into<String>,
        scope_ids: BTreeSet<StableId>,
        severity: Severity,
        verification_mode: Option<String>,
        provenance: Provenance,
    ) -> Result<Self> {
        let property_id = property_id.into();
        ensure_non_empty(&property_id, "invariant.property_id")?;
        let description = description.into();
        ensure_non_empty(&description, "invariant.description")?;
        if scope_ids.is_empty() {
            return Err(DomainError::EmptyField {
                field: "invariant.scope_ids",
            });
        }
        Ok(Self {
            id,
            property_id,
            description,
            scope_ids,
            severity,
            verification_mode,
            provenance,
        })
    }

    /// Re-validates an already-constructed invariant. This is the boundary
    /// [`ProgramSpaceBuilder::build`] uses so an `Invariant` assembled via its
    /// public struct-literal fields (bypassing [`Self::new`]) cannot smuggle
    /// an empty `property_id`, `description`, or `scope_ids` into a validated
    /// `ProgramSpace`.
    pub(crate) fn validate(&self) -> Result<()> {
        let _ = Self::new(
            self.id.clone(),
            self.property_id.clone(),
            self.description.clone(),
            self.scope_ids.clone(),
            self.severity,
            self.verification_mode.clone(),
            self.provenance.clone(),
        )?;
        Ok(())
    }
}

/// Existing evidence imported alongside ProgramSpace facts.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Evidence {
    /// Stable evidence ID.
    id: StableId,
    /// Evidence kind.
    kind: String,
    /// Sorted IDs the evidence is about.
    target_ids: BTreeSet<StableId>,
    /// Optional external artifact reference.
    artifact_ref: Option<String>,
    /// Optional content hash.
    content_hash: Option<ContentHash>,
    /// Evidence metadata, never an implicit verification outcome.
    attributes: BTreeMap<String, Value>,
    /// Provenance of the observation.
    provenance: Provenance,
    /// Snapshot at which this observation was collected. This is not part of
    /// the legacy ProgramSpace input document; the adapter attaches it.
    snapshot_id: StableId,
}

/// Opaque admission that binds newly recorded evidence to one accepted
/// ProgramSpace snapshot. Only a parsed `ProgramSpace` can mint it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceSnapshotAdmission {
    snapshot_id: StableId,
}

/// Opaque authority for one exact evidence record at one review-stream position.
///
/// This is minted only by a snapshot admission obtained from a validated
/// `ProgramSpace`; it is deliberately not serializable or deserializable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceAdmission {
    run_id: StableId,
    genesis_hash: ContentHash,
    tail_hash: ContentHash,
    sequence: u64,
    snapshot_id: StableId,
    evidence_id: StableId,
    digest: ContentHash,
}

impl EvidenceAdmission {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.run_id.allocated_bytes()
            + self.genesis_hash.allocated_bytes()
            + self.tail_hash.allocated_bytes()
            + self.snapshot_id.allocated_bytes()
            + self.evidence_id.allocated_bytes()
            + self.digest.allocated_bytes()
    }
}

impl EvidenceSnapshotAdmission {
    pub(crate) fn admits(&self, evidence: &Evidence) -> bool {
        self.snapshot_id == evidence.snapshot_id
    }

    /// Mints an exact evidence admission only for a specific event-log genesis
    /// and expected append position.
    ///
    /// This stays crate-private because an event log, rather than a bare
    /// snapshot token, owns the run's immutable genesis binding. A stale
    /// observation may still be admitted and retained for audit; freshness is
    /// derived later when it is used by a verification or decision.
    pub(crate) fn admit_for_event_log(
        &self,
        run_id: StableId,
        genesis_hash: ContentHash,
        tail_hash: ContentHash,
        sequence: u64,
        evidence: &Evidence,
    ) -> Result<EvidenceAdmission> {
        if run_id.kind() != "run" || sequence == 0 || !self.admits(evidence) {
            return Err(DomainError::Validation(
                "evidence admission requires its observed snapshot, body, and review run"
                    .to_owned(),
            ));
        }
        Ok(EvidenceAdmission {
            run_id,
            genesis_hash,
            tail_hash,
            sequence,
            snapshot_id: self.snapshot_id.clone(),
            evidence_id: evidence.id.clone(),
            digest: ContentHash::sha256(&crate::canonical_json(evidence)?),
        })
    }
}

impl EvidenceAdmission {
    pub(crate) fn matches(
        &self,
        run_id: &StableId,
        genesis_hash: &ContentHash,
        tail_hash: &ContentHash,
        sequence: u64,
        evidence: &Evidence,
    ) -> bool {
        self.run_id == *run_id
            && self.genesis_hash == *genesis_hash
            && self.tail_hash == *tail_hash
            && self.sequence == sequence
            && self.snapshot_id == evidence.snapshot_id
            && self.evidence_id == evidence.id
            && crate::canonical_json(evidence)
                .is_ok_and(|bytes| ContentHash::sha256(&bytes) == self.digest)
    }
}

/// Optional material associated with an evidence observation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EvidenceDetails {
    artifact_ref: Option<String>,
    content_hash: Option<ContentHash>,
    attributes: BTreeMap<String, Value>,
}

impl EvidenceDetails {
    /// Creates optional evidence material without granting trust or freshness.
    #[must_use]
    pub fn new(
        artifact_ref: Option<String>,
        content_hash: Option<ContentHash>,
        attributes: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            artifact_ref,
            content_hash,
            attributes,
        }
    }
}

impl Evidence {
    pub(crate) fn allocated_bytes(&self) -> usize {
        fn value_bytes(value: &Value) -> usize {
            match value {
                Value::String(value) => value.capacity(),
                Value::Array(values) => values
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Value>())
                    .saturating_add(values.iter().map(value_bytes).sum::<usize>()),
                Value::Object(values) => values.iter().fold(
                    values
                        .len()
                        .saturating_mul(std::mem::size_of::<(String, Value)>()),
                    |total, (key, value)| {
                        total
                            .saturating_add(key.capacity())
                            .saturating_add(value_bytes(value))
                    },
                ),
                _ => 0,
            }
        }
        let targets = self
            .target_ids
            .len()
            .saturating_mul(std::mem::size_of::<StableId>())
            .saturating_add(
                self.target_ids
                    .iter()
                    .map(StableId::allocated_bytes)
                    .sum::<usize>(),
            );
        let attributes = self.attributes.iter().fold(
            self.attributes
                .len()
                .saturating_mul(std::mem::size_of::<(String, Value)>()),
            |total, (key, value)| {
                total
                    .saturating_add(key.capacity())
                    .saturating_add(value_bytes(value))
            },
        );
        self.id
            .allocated_bytes()
            .saturating_add(self.kind.capacity())
            .saturating_add(targets)
            .saturating_add(self.artifact_ref.as_ref().map_or(0, String::capacity))
            .saturating_add(
                self.content_hash
                    .as_ref()
                    .map_or(0, ContentHash::allocated_bytes),
            )
            .saturating_add(attributes)
            .saturating_add(self.provenance.allocated_bytes())
            .saturating_add(self.snapshot_id.allocated_bytes())
    }
    /// Creates review-event evidence bound to the snapshot where it was observed.
    pub fn new(
        id: StableId,
        kind: impl Into<String>,
        target_ids: BTreeSet<StableId>,
        details: EvidenceDetails,
        provenance: Provenance,
        admission: EvidenceSnapshotAdmission,
    ) -> Result<Self> {
        let kind = kind.into();
        ensure_non_empty(&kind, "evidence.kind")?;
        if target_ids.is_empty() {
            return Err(DomainError::EmptyField {
                field: "evidence.target_ids",
            });
        }
        provenance.validate_for_canonical_evidence()?;
        Ok(Self {
            id,
            kind,
            target_ids,
            artifact_ref: details.artifact_ref,
            content_hash: details.content_hash,
            attributes: details.attributes,
            provenance,
            snapshot_id: admission.snapshot_id,
        })
    }

    /// Constructs one imported ProgramSpace-input evidence fact. Unlike
    /// [`Self::new`], an empty target set is allowed here: the legacy input
    /// contract permits importing historical evidence without a current
    /// target, but such a record still cannot enter a review event because
    /// [`Self::new`] requires targets.
    // Each parameter is a distinct field of the legacy ProgramSpace-input
    // evidence contract validated here; grouping them into a struct would
    // change this stable public boundary rather than reduce arity.
    #[allow(clippy::too_many_arguments)]
    pub fn for_program_space(
        id: StableId,
        kind: impl Into<String>,
        target_ids: BTreeSet<StableId>,
        artifact_ref: Option<String>,
        content_hash: Option<ContentHash>,
        attributes: BTreeMap<String, Value>,
        provenance: Provenance,
        snapshot_id: StableId,
    ) -> Result<Self> {
        let kind = kind.into();
        ensure_non_empty(&kind, "evidence.kind")?;
        Ok(Self {
            id,
            kind,
            target_ids,
            artifact_ref,
            content_hash,
            attributes,
            provenance,
            snapshot_id,
        })
    }

    /// Re-validates an already-constructed ProgramSpace evidence fact against
    /// the [`Self::for_program_space`] contract. This is the boundary
    /// [`ProgramSpaceBuilder::build`] uses so re-validation stays identical to
    /// construction even if the internal shape changes.
    pub(crate) fn validate_for_program_space(&self) -> Result<()> {
        let _ = Self::for_program_space(
            self.id.clone(),
            self.kind.clone(),
            self.target_ids.clone(),
            self.artifact_ref.clone(),
            self.content_hash.clone(),
            self.attributes.clone(),
            self.provenance.clone(),
            self.snapshot_id.clone(),
        )?;
        Ok(())
    }

    /// Stable evidence ID.
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    /// Evidence mode used to match an obligation requirement.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Program subjects observed by the evidence.
    #[must_use]
    pub fn target_ids(&self) -> &BTreeSet<StableId> {
        &self.target_ids
    }

    /// Deterministic observation provenance.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Snapshot on which this evidence was observed.
    #[must_use]
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }

    pub(crate) fn validate_for_review_event(&self) -> Result<()> {
        let _ = Self::new(
            self.id.clone(),
            self.kind.clone(),
            self.target_ids.clone(),
            EvidenceDetails::new(
                self.artifact_ref.clone(),
                self.content_hash.clone(),
                self.attributes.clone(),
            ),
            self.provenance.clone(),
            EvidenceSnapshotAdmission {
                snapshot_id: self.snapshot_id.clone(),
            },
        )?;
        self.provenance.validate_for_canonical_evidence()
    }

    pub(crate) fn from_event_value(value: Value) -> Result<Self> {
        let raw: RawEventEvidence =
            serde_json::from_value(value).map_err(|error| DomainError::Json(error.to_string()))?;
        let target_ids = event_ids(raw.target_ids, "evidence.target_ids")?;
        let source = SourceRef::new(
            raw.provenance.source.kind,
            raw.provenance.source.locator,
            raw.provenance.source.revision,
            raw.provenance
                .source
                .content_hash
                .map(ContentHash::parse)
                .transpose()?,
            raw.provenance.source.source_local_id,
        )?;
        if raw.provenance.review_status != ReviewStatus::Accepted {
            return Err(DomainError::Validation(
                "review-event evidence requires accepted deterministic provenance".to_owned(),
            ));
        }
        let provenance = Provenance::accepted_deterministic(
            source,
            raw.provenance.extraction_method,
            raw.provenance.tool_version,
            raw.provenance.confidence,
        )?;
        let evidence = Self {
            id: raw.id,
            kind: raw.kind,
            target_ids,
            artifact_ref: raw.artifact_ref,
            content_hash: raw.content_hash,
            attributes: raw.attributes,
            provenance,
            snapshot_id: raw.snapshot_id,
        };
        evidence.validate_for_review_event()?;
        Ok(evidence)
    }
}

fn event_ids(values: Vec<StableId>, field: &'static str) -> Result<BTreeSet<StableId>> {
    let mut result = BTreeSet::new();
    for id in values {
        if !result.insert(id.clone()) {
            return Err(DomainError::Validation(format!(
                "{field} must not contain duplicate IDs"
            )));
        }
    }
    if result.is_empty() {
        return Err(DomainError::EmptyField { field });
    }
    Ok(result)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEventSourceRef {
    kind: String,
    locator: String,
    revision: Option<String>,
    content_hash: Option<String>,
    source_local_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEventProvenance {
    source: RawEventSourceRef,
    extraction_method: String,
    tool_version: Option<String>,
    confidence: Option<f64>,
    review_status: ReviewStatus,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEventEvidence {
    id: StableId,
    kind: String,
    target_ids: Vec<StableId>,
    artifact_ref: Option<String>,
    content_hash: Option<ContentHash>,
    #[serde(default)]
    attributes: BTreeMap<String, Value>,
    provenance: RawEventProvenance,
    snapshot_id: StableId,
}

/// A declared loss when a projection or extraction omits information.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InformationLoss {
    /// Loss category.
    pub kind: String,
    /// Why information was omitted or collapsed.
    pub reason: String,
    /// Whether the loss can be recovered by a named operation.
    pub recoverable: bool,
    /// Optional recovery path.
    pub recoverable_via: Option<String>,
    /// Sources affected by the loss.
    pub source_ids: BTreeSet<StableId>,
}

/// A capability limitation retained as an explicit record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Limitation {
    /// Stable limitation ID.
    pub id: StableId,
    /// Limitation category.
    pub kind: LimitationKind,
    /// Explanation of the capability boundary.
    pub description: String,
    /// Descriptive severity.
    pub severity: Severity,
    /// Sorted affected IDs.
    pub source_ids: BTreeSet<StableId>,
    /// Named `extraction.capabilities` entries this limitation qualifies.
    /// Empty when the limitation is not tied to one named capability.
    #[serde(default)]
    pub related_capabilities: BTreeSet<String>,
}

impl Limitation {
    /// Constructs and validates one extraction limitation record.
    pub fn new(
        id: StableId,
        kind: LimitationKind,
        description: impl Into<String>,
        severity: Severity,
        source_ids: BTreeSet<StableId>,
        related_capabilities: BTreeSet<String>,
    ) -> Result<Self> {
        let description = description.into();
        ensure_non_empty(&description, "limitation.description")?;
        if source_ids.is_empty() {
            return Err(DomainError::EmptyField {
                field: "limitation.source_ids",
            });
        }
        for capability in &related_capabilities {
            ensure_non_empty(capability, "limitation.related_capabilities")?;
        }
        Ok(Self {
            id,
            kind,
            description,
            severity,
            source_ids,
            related_capabilities,
        })
    }

    /// Re-validates an already-constructed limitation. This is the boundary
    /// [`ProgramSpaceBuilder::build`] uses so a `Limitation` assembled via its
    /// public struct-literal fields (bypassing [`Self::new`]) cannot smuggle
    /// an empty `description` or an empty `related_capabilities` entry into a
    /// validated `ProgramSpace`.
    pub(crate) fn validate(&self) -> Result<()> {
        let _ = Self::new(
            self.id.clone(),
            self.kind,
            self.description.clone(),
            self.severity,
            self.source_ids.clone(),
            self.related_capabilities.clone(),
        )?;
        Ok(())
    }
}

/// Named extraction capability, together with the accepted facts it is
/// grounded in.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CapabilityDeclaration {
    /// Completeness state for this capability.
    pub state: CapabilityState,
    /// Non-empty set of IDs this declaration is grounded in.
    pub source_ids: BTreeSet<StableId>,
}

impl CapabilityDeclaration {
    /// Constructs and validates one capability declaration.
    pub fn new(state: CapabilityState, source_ids: BTreeSet<StableId>) -> Result<Self> {
        if source_ids.is_empty() {
            return Err(DomainError::EmptyField {
                field: "capability.source_ids",
            });
        }
        Ok(Self { state, source_ids })
    }

    /// Re-validates an already-constructed capability declaration. This is
    /// the boundary [`Extraction::revalidated`] uses so a
    /// `CapabilityDeclaration` assembled via its public struct-literal
    /// fields (bypassing [`Self::new`]) cannot smuggle an empty `source_ids`
    /// into a validated `ProgramSpace`.
    pub(crate) fn validate(&self) -> Result<()> {
        let _ = Self::new(self.state, self.source_ids.clone())?;
        Ok(())
    }
}

/// Extractor descriptors and limits associated with accepted facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Extraction {
    /// Hash of the full adapter set.
    pub adapter_set_hash: ContentHash,
    /// Deterministically sorted adapter descriptors.
    pub adapters: Vec<AdapterDescriptor>,
    /// Capability declaration by capability name.
    pub capabilities: BTreeMap<String, CapabilityDeclaration>,
    /// Explicit limitations, never silently erased.
    pub limitations: Vec<Limitation>,
}

impl Extraction {
    /// Constructs and validates the extraction declaration. This is the single
    /// boundary that rejects duplicate adapter identities and completeness
    /// contradictions between a capability's declared state and its related
    /// limitations, instead of accepting them silently.
    pub fn new(
        adapter_set_hash: ContentHash,
        mut adapters: Vec<AdapterDescriptor>,
        capabilities: BTreeMap<String, CapabilityDeclaration>,
        mut limitations: Vec<Limitation>,
    ) -> Result<Self> {
        if adapters.is_empty() {
            return Err(DomainError::EmptyField {
                field: "extraction.adapters",
            });
        }
        adapters.sort_by(|left, right| left.id.cmp(&right.id));
        let mut seen_adapter_ids = BTreeSet::new();
        for adapter in &adapters {
            adapter.validate()?;
            if !seen_adapter_ids.insert(adapter.id.clone()) {
                return Err(DomainError::Validation(format!(
                    "duplicate extraction adapter id `{}`",
                    adapter.id
                )));
            }
        }
        limitations.sort_by(|left, right| left.id.cmp(&right.id));
        for limitation in &limitations {
            limitation.validate()?;
            for capability in &limitation.related_capabilities {
                if !capabilities.contains_key(capability) {
                    return Err(DomainError::Validation(format!(
                        "limitation `{}` relates to undeclared capability `{capability}`",
                        limitation.id
                    )));
                }
            }
        }
        for (capability, declaration) in &capabilities {
            ensure_non_empty(capability, "extraction.capabilities key")?;
            declaration.validate()?;
            let related = limitations
                .iter()
                .filter(|item| item.related_capabilities.contains(capability))
                .collect::<Vec<_>>();
            match declaration.state {
                CapabilityState::Complete => {
                    if !related.is_empty() {
                        return Err(DomainError::Validation(format!(
                            "capability `{capability}` is declared complete but has a related unresolved limitation"
                        )));
                    }
                }
                CapabilityState::Partial => {
                    if related.is_empty() {
                        return Err(DomainError::Validation(format!(
                            "capability `{capability}` is declared partial but has no related limitation"
                        )));
                    }
                    if related
                        .iter()
                        .any(|item| item.kind == LimitationKind::CapabilityMissing)
                    {
                        return Err(DomainError::Validation(format!(
                            "capability `{capability}` is declared partial but a related limitation claims it is entirely missing"
                        )));
                    }
                }
                CapabilityState::Missing => {
                    if !related
                        .iter()
                        .any(|item| item.kind == LimitationKind::CapabilityMissing)
                    {
                        return Err(DomainError::Validation(format!(
                            "capability `{capability}` is declared missing but has no related `capability_missing` limitation"
                        )));
                    }
                }
                CapabilityState::Unknown => {
                    if related.is_empty() {
                        return Err(DomainError::Validation(format!(
                            "capability `{capability}` is declared unknown but has no related limitation"
                        )));
                    }
                }
            }
        }
        Ok(Self {
            adapter_set_hash,
            adapters,
            capabilities,
            limitations,
        })
    }

    /// Re-validates an already-constructed extraction declaration, including
    /// every nested adapter and limitation, and returns the same canonically
    /// ordered form [`Self::new`] would have produced. This is the boundary
    /// [`ProgramSpaceBuilder::new`] and [`ProgramSpaceBuilder::build`] both
    /// use, so an `Extraction` assembled via its public struct-literal fields
    /// (bypassing [`Self::new`], or holding out-of-order or struct-literal
    /// adapters/limitations) cannot smuggle a per-item contradiction, a
    /// cross-field contradiction, or an unsorted `adapters`/`limitations`
    /// order into a validated `ProgramSpace`. Unlike a `&self` check, this
    /// consumes `self` and returns the normalized replacement so the
    /// re-validated (sorted) clone is never silently discarded.
    pub(crate) fn revalidated(self) -> Result<Self> {
        for adapter in &self.adapters {
            adapter.validate()?;
        }
        for declaration in self.capabilities.values() {
            declaration.validate()?;
        }
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        Self::new(
            self.adapter_set_hash,
            self.adapters,
            self.capabilities,
            self.limitations,
        )
    }
}

/// Declared, structured information loss from an explicit v1→v2
/// `Extraction` migration (see ADR 0011 §7). This is never silently
/// absorbed into an otherwise-normal v2 value; every entry names the exact
/// capability or limitation it concerns and the replacement data assigned
/// on its behalf.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MigrationLoss {
    /// A v1 capability declared no source trace at all; migration
    /// conservatively assigned it the snapshot as its only honest source.
    CapabilitySourceBackfill {
        /// Deterministic loss ID.
        id: StableId,
        /// Migrated capability name.
        capability: String,
        /// Migrated capability state.
        state: CapabilityState,
        /// `source_ids` assigned by migration.
        assigned_source_ids: BTreeSet<StableId>,
    },
    /// A deterministic limitation was synthesized to satisfy the v2
    /// non-`complete` cross-field contract for a migrated capability.
    SynthesizedLimitation {
        /// Deterministic loss ID.
        id: StableId,
        /// Migrated capability name.
        capability: String,
        /// Migrated capability state.
        state: CapabilityState,
        /// ID of the synthesized limitation.
        limitation_id: StableId,
        /// Kind of the synthesized limitation.
        limitation_kind: LimitationKind,
    },
    /// Records the original v1 `source_ids` of every carried-over
    /// limitation, whether empty or not, since migration never infers an
    /// association the v1 record did not declare. An empty set here is
    /// paired with a separate [`Self::LimitationSourceBackfill`] entry for
    /// the same `limitation_id`; a nonempty set is carried through
    /// unchanged and is not itself a loss beyond this declaration.
    CarriedLimitationTrace {
        /// Deterministic loss ID.
        id: StableId,
        /// ID of the carried-over limitation.
        limitation_id: StableId,
        /// The limitation's original v1 `source_ids`, possibly empty.
        original_source_ids: BTreeSet<StableId>,
    },
    /// A carried-over v1 limitation's `source_ids` was conservatively
    /// backfilled to the snapshot ID because the v1 record supplied none.
    LimitationSourceBackfill {
        /// Deterministic loss ID.
        id: StableId,
        /// ID of the carried-over limitation.
        limitation_id: StableId,
        /// `source_ids` assigned by migration.
        assigned_source_ids: BTreeSet<StableId>,
    },
}

impl MigrationLoss {
    /// Deterministic loss ID, present on every variant.
    #[must_use]
    pub fn id(&self) -> &StableId {
        match self {
            Self::CapabilitySourceBackfill { id, .. }
            | Self::SynthesizedLimitation { id, .. }
            | Self::CarriedLimitationTrace { id, .. }
            | Self::LimitationSourceBackfill { id, .. } => id,
        }
    }
}

/// Structured record of an explicit v1→v2 `Extraction` migration (see
/// ADR 0011 §7). Returned alongside the migrated `Extraction` so every
/// backfill and synthesized limitation stays visible to the caller instead
/// of being silently folded into an otherwise-normal v2 value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MigrationRecord {
    schema: String,
    id: StableId,
    source_schema: String,
    target_schema: String,
    snapshot_id: StableId,
    losses: Vec<MigrationLoss>,
}

impl MigrationRecord {
    /// Fixed record schema discriminator.
    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// Deterministic migration record ID.
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    /// Schema discriminator the migrated record originally declared.
    #[must_use]
    pub fn source_schema(&self) -> &str {
        &self.source_schema
    }

    /// Schema discriminator the migrated `Extraction` now satisfies.
    #[must_use]
    pub fn target_schema(&self) -> &str {
        &self.target_schema
    }

    /// Snapshot the migrated record belongs to.
    #[must_use]
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }

    /// Ordered, structured information loss declared by this migration.
    #[must_use]
    pub fn losses(&self) -> &[MigrationLoss] {
        &self.losses
    }
}

/// One manual or tool adapter result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AdapterDescriptor {
    /// Adapter identity.
    pub id: String,
    /// Adapter version.
    pub version: String,
    /// Completion state for this snapshot.
    pub status: AdapterStatus,
    /// Optional parsed count.
    pub parsed: Option<u64>,
    /// Optional total count.
    pub total: Option<u64>,
}

impl AdapterDescriptor {
    /// Constructs and validates one adapter completeness declaration.
    pub fn new(
        id: impl Into<String>,
        version: impl Into<String>,
        status: AdapterStatus,
        parsed: Option<u64>,
        total: Option<u64>,
    ) -> Result<Self> {
        let id = id.into();
        ensure_non_empty(&id, "adapter.id")?;
        let version = version.into();
        ensure_non_empty(&version, "adapter.version")?;
        if parsed
            .zip(total)
            .is_some_and(|(parsed, total)| parsed > total)
        {
            return Err(DomainError::Validation(
                "adapter parsed count must not exceed total".to_owned(),
            ));
        }
        if status == AdapterStatus::Complete
            && parsed
                .zip(total)
                .is_some_and(|(parsed, total)| parsed != total)
        {
            return Err(DomainError::Validation(
                "an adapter reporting complete status must have parsed == total when both counts are present"
                    .to_owned(),
            ));
        }
        if status == AdapterStatus::NotRun && (parsed.is_some() || total.is_some()) {
            return Err(DomainError::Validation(
                "an adapter reporting not_run status must not report parsed/total counts"
                    .to_owned(),
            ));
        }
        Ok(Self {
            id,
            version,
            status,
            parsed,
            total,
        })
    }

    /// Re-validates an already-constructed adapter descriptor. This is the
    /// boundary [`ProgramSpaceBuilder::build`] uses so an `AdapterDescriptor`
    /// assembled via its public struct-literal fields (bypassing
    /// [`Self::new`]) cannot smuggle an empty `id`/`version`, an inverted
    /// `parsed`/`total` pair, or a status-inconsistent count into a validated
    /// `ProgramSpace`.
    pub(crate) fn validate(&self) -> Result<()> {
        let _ = Self::new(
            self.id.clone(),
            self.version.clone(),
            self.status,
            self.parsed,
            self.total,
        )?;
        Ok(())
    }
}

/// Plain repository identity used by [`ProgramSpaceBuilder`]. The identity is
/// explicit and stable; a local filesystem root is metadata only and is never
/// part of canonical identity derivation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryDescriptor {
    /// Stable repository ID.
    pub id: StableId,
    /// Human-readable repository name.
    pub name: String,
    /// Optional local root, retained as non-canonical metadata only.
    pub root: Option<String>,
    /// Optional stable repository URI (for example a remote clone URL).
    pub uri: Option<String>,
}

impl RepositoryDescriptor {
    /// Re-validates an already-constructed repository descriptor. This is the
    /// single contract [`ProgramSpaceBuilder::new`] and
    /// [`ProgramSpaceBuilder::build`] both enforce, so a `RepositoryDescriptor`
    /// assembled via its public struct-literal fields cannot smuggle an empty
    /// `name` into a validated `ProgramSpace`.
    pub(crate) fn validate(&self) -> Result<()> {
        ensure_non_empty(&self.name, "repository.name")
    }
}

/// Plain snapshot descriptor used by [`ProgramSpaceBuilder`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotDescriptor {
    /// Stable snapshot ID.
    pub id: StableId,
    /// Base revision used only for deterministic changed-structure mapping.
    pub base_revision: String,
    /// Target revision whose tree is captured by this snapshot.
    pub target_revision: String,
    /// Tree hash of the target revision.
    pub tree_hash: ContentHash,
    /// Whether the workspace had uncommitted changes.
    pub dirty: bool,
    /// Optional input snapshot timestamp, retained without reinterpretation.
    pub created_at: Option<String>,
}

impl SnapshotDescriptor {
    /// Re-validates an already-constructed snapshot descriptor. This is the
    /// single contract [`ProgramSpaceBuilder::new`] and
    /// [`ProgramSpaceBuilder::build`] both enforce, so a `SnapshotDescriptor`
    /// assembled via its public struct-literal fields cannot smuggle an empty
    /// `base_revision`/`target_revision` into a validated `ProgramSpace`.
    pub(crate) fn validate(&self) -> Result<()> {
        ensure_non_empty(&self.base_revision, "snapshot.base_revision")?;
        ensure_non_empty(&self.target_revision, "snapshot.target_revision")?;
        Ok(())
    }
}

/// Plain profile descriptor used by [`ProgramSpaceBuilder`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileDescriptor {
    /// Profile identity.
    pub id: String,
    /// Profile version.
    pub version: String,
    /// Hash of the selected rule pack.
    pub rule_set_hash: ContentHash,
    /// Versioned policy identity.
    pub policy_version: String,
}

impl ProfileDescriptor {
    /// Re-validates an already-constructed profile descriptor. This is the
    /// single contract [`ProgramSpaceBuilder::new`] and
    /// [`ProgramSpaceBuilder::build`] both enforce, so a `ProfileDescriptor`
    /// assembled via its public struct-literal fields cannot smuggle an empty
    /// `id`/`version`/`policy_version` into a validated `ProgramSpace`.
    pub(crate) fn validate(&self) -> Result<()> {
        ensure_non_empty(&self.id, "profile.id")?;
        ensure_non_empty(&self.version, "profile.version")?;
        ensure_non_empty(&self.policy_version, "profile.policy_version")?;
        Ok(())
    }
}

/// Typed lift boundary from adapter facts to a validated [`ProgramSpace`].
///
/// This is the single place that performs ID generation and dedup contracts
/// (via the caller-supplied [`StableId`]s), canonical ordering, reference
/// validation, provenance/completeness validation, and completeness
/// contradiction checks. Both the legacy JSON adapter input path
/// (`ProgramSpace::from_json_slice`) and any typed extractor funnel through
/// this exact boundary; there is no second, ad-hoc construction path.
pub struct ProgramSpaceBuilder {
    source: SourceRef,
    repository: RepositoryDescriptor,
    snapshot: SnapshotDescriptor,
    profile: ProfileDescriptor,
    extraction: Extraction,
    artifacts: Vec<Artifact>,
    relations: Vec<Relation>,
    contexts: Vec<ReviewContext>,
    invariants: Vec<Invariant>,
    evidence: Vec<Evidence>,
}

impl ProgramSpaceBuilder {
    /// Starts a builder for exactly one ProgramSpace snapshot.
    pub fn new(
        source: SourceRef,
        repository: RepositoryDescriptor,
        snapshot: SnapshotDescriptor,
        profile: ProfileDescriptor,
        extraction: Extraction,
    ) -> Result<Self> {
        if source.kind() == "model" {
            return Err(DomainError::Validation(
                "ProgramSpace input source cannot be a model".to_owned(),
            ));
        }
        repository.validate()?;
        snapshot.validate()?;
        profile.validate()?;
        let extraction = extraction.revalidated()?;
        Ok(Self {
            source,
            repository,
            snapshot,
            profile,
            extraction,
            artifacts: Vec::new(),
            relations: Vec::new(),
            contexts: Vec::new(),
            invariants: Vec::new(),
            evidence: Vec::new(),
        })
    }

    /// Appends one accepted artifact.
    #[must_use]
    pub fn with_artifact(mut self, artifact: Artifact) -> Self {
        self.artifacts.push(artifact);
        self
    }

    /// Appends accepted artifacts.
    #[must_use]
    pub fn with_artifacts(mut self, artifacts: impl IntoIterator<Item = Artifact>) -> Self {
        self.artifacts.extend(artifacts);
        self
    }

    /// Appends one accepted relation.
    #[must_use]
    pub fn with_relation(mut self, relation: Relation) -> Self {
        self.relations.push(relation);
        self
    }

    /// Appends accepted relations.
    #[must_use]
    pub fn with_relations(mut self, relations: impl IntoIterator<Item = Relation>) -> Self {
        self.relations.extend(relations);
        self
    }

    /// Appends one accepted review context.
    #[must_use]
    pub fn with_context(mut self, context: ReviewContext) -> Self {
        self.contexts.push(context);
        self
    }

    /// Appends accepted review contexts.
    #[must_use]
    pub fn with_contexts(mut self, contexts: impl IntoIterator<Item = ReviewContext>) -> Self {
        self.contexts.extend(contexts);
        self
    }

    /// Appends one accepted invariant.
    #[must_use]
    pub fn with_invariant(mut self, invariant: Invariant) -> Self {
        self.invariants.push(invariant);
        self
    }

    /// Appends accepted invariants.
    #[must_use]
    pub fn with_invariants(mut self, invariants: impl IntoIterator<Item = Invariant>) -> Self {
        self.invariants.extend(invariants);
        self
    }

    /// Appends one imported evidence fact.
    #[must_use]
    pub fn with_evidence_item(mut self, evidence: Evidence) -> Self {
        self.evidence.push(evidence);
        self
    }

    /// Appends imported evidence facts.
    #[must_use]
    pub fn with_evidence(mut self, evidence: impl IntoIterator<Item = Evidence>) -> Self {
        self.evidence.extend(evidence);
        self
    }

    /// Validates and assembles the final `ProgramSpace`.
    pub fn build(self) -> Result<ProgramSpace> {
        let Self {
            source,
            repository,
            snapshot,
            profile,
            extraction,
            mut artifacts,
            mut relations,
            mut contexts,
            mut invariants,
            mut evidence,
        } = self;

        if artifacts.is_empty() {
            return Err(DomainError::EmptyField { field: "artifacts" });
        }

        // `build` is the real validation boundary: a caller can assemble any
        // of these records through their public struct-literal fields,
        // bypassing the constructor that normally enforces the record's
        // invariants. Re-run that exact same contract here so construction
        // path never determines whether an invalid record is accepted, and
        // replace `extraction` with the normalized clone `revalidated`
        // returns instead of discarding it, so an out-of-order struct-literal
        // `Extraction` still produces the same canonical `ProgramSpace` bytes
        // as one built through `Extraction::new`.
        repository.validate()?;
        snapshot.validate()?;
        profile.validate()?;
        let extraction = extraction.revalidated()?;
        for artifact in &artifacts {
            artifact.validate()?;
        }
        for relation in &relations {
            relation.validate()?;
        }
        for context in &contexts {
            context.validate()?;
        }
        for invariant in &invariants {
            invariant.validate()?;
        }
        for item in &evidence {
            item.validate_for_program_space()?;
        }

        artifacts.sort_by(|left, right| left.id.cmp(&right.id));
        relations.sort_by(|left, right| left.id.cmp(&right.id));
        contexts.sort_by(|left, right| left.id.cmp(&right.id));
        invariants.sort_by(|left, right| left.id.cmp(&right.id));
        evidence.sort_by(|left, right| left.id.cmp(&right.id));

        let mut ids = BTreeSet::from([repository.id.clone(), snapshot.id.clone()]);
        for id in artifacts
            .iter()
            .map(|item| &item.id)
            .chain(relations.iter().map(|item| &item.id))
            .chain(contexts.iter().map(|item| &item.id))
            .chain(invariants.iter().map(|item| &item.id))
            .chain(evidence.iter().map(|item| &item.id))
            .chain(extraction.limitations.iter().map(|item| &item.id))
        {
            if !ids.insert(id.clone()) {
                return Err(DomainError::IdCollision { id: id.clone() });
            }
        }

        let artifact_ids = artifacts
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>();
        for artifact in &artifacts {
            if let Some(symbol_id) = artifact
                .location
                .as_ref()
                .and_then(|location| location.symbol_id.as_ref())
            {
                validate_reference("location", &artifact.id, &artifact_ids, symbol_id)?;
                let symbol = artifacts
                    .iter()
                    .find(|candidate| candidate.id == *symbol_id)
                    .ok_or_else(|| DomainError::DanglingReference {
                        owner: "location",
                        owner_id: artifact.id.clone(),
                        reference: symbol_id.clone(),
                    })?;
                if !is_symbol_kind(&symbol.kind) {
                    return Err(DomainError::Validation(
                        "location.symbol_id must refer to a symbol artifact".to_owned(),
                    ));
                }
            }
        }
        let relation_ids = relations
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>();
        let relation_target_ids = artifact_ids
            .union(&relation_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        for relation in &relations {
            validate_reference("relation", &relation.id, &artifact_ids, &relation.source_id)?;
            for target_id in &relation.target_ids {
                validate_reference("relation", &relation.id, &relation_target_ids, target_id)?;
            }
        }
        let context_ids = contexts
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>();
        let context_member_ids = relation_target_ids.clone();
        for context in &contexts {
            for member_id in &context.member_ids {
                validate_reference("context", &context.id, &context_member_ids, member_id)?;
            }
        }
        let invariant_ids = invariants
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>();
        let invariant_scope_artifacts = artifacts
            .iter()
            .filter(|artifact| matches!(artifact.kind.as_str(), "requirement" | "policy"))
            .map(|artifact| artifact.id.clone())
            .collect::<BTreeSet<_>>();
        let invariant_scope_ids = context_ids
            .union(&invariant_scope_artifacts)
            .cloned()
            .collect::<BTreeSet<_>>();
        for invariant in &invariants {
            for scope_id in &invariant.scope_ids {
                validate_reference("invariant", &invariant.id, &invariant_scope_ids, scope_id)?;
            }
        }
        let evidence_target_ids = relation_target_ids
            .union(&invariant_ids)
            .cloned()
            .collect::<BTreeSet<_>>();
        for item in &evidence {
            for target_id in &item.target_ids {
                validate_reference("evidence", &item.id, &evidence_target_ids, target_id)?;
            }
        }
        validate_extraction_source_traces(&extraction, &ids)?;

        validate_source_identity_consistency(&artifacts, &relations, &contexts, &invariants)?;
        let _ = invariant_ids;

        Ok(ProgramSpace {
            schema: PROGRAM_SPACE_SCHEMA_V2.to_owned(),
            source,
            repository_id: repository.id,
            repository_name: repository.name,
            repository_root: repository.root,
            repository_uri: repository.uri,
            snapshot_id: snapshot.id,
            base_revision: snapshot.base_revision,
            target_revision: snapshot.target_revision,
            tree_hash: snapshot.tree_hash,
            dirty: snapshot.dirty,
            snapshot_created_at: snapshot.created_at,
            profile_id: profile.id,
            profile_version: profile.version,
            rule_set_hash: profile.rule_set_hash,
            policy_version: profile.policy_version,
            artifacts,
            relations,
            contexts,
            invariants,
            evidence,
            extraction,
        })
    }
}

/// Validates every `extraction.limitations[].source_ids` and
/// `extraction.capabilities[].source_ids` entry against the full known
/// ProgramSpace ID set, and rejects a limitation whose source set contains
/// its own ID.
fn validate_extraction_source_traces(
    extraction: &Extraction,
    ids: &BTreeSet<StableId>,
) -> Result<()> {
    for limitation in &extraction.limitations {
        for source_id in &limitation.source_ids {
            validate_reference("limitation", &limitation.id, ids, source_id)?;
        }
    }
    for (capability, declaration) in &extraction.capabilities {
        for source_id in &declaration.source_ids {
            if !ids.contains(source_id) {
                return Err(DomainError::Validation(format!(
                    "capability `{capability}` has dangling source `{source_id}`"
                )));
            }
        }
    }
    for limitation in &extraction.limitations {
        if limitation.source_ids.contains(&limitation.id) {
            return Err(DomainError::Validation(format!(
                "limitation `{}` has a self-referential source",
                limitation.id
            )));
        }
    }

    let limitation_ids = extraction
        .limitations
        .iter()
        .map(|limitation| limitation.id.clone())
        .collect::<BTreeSet<_>>();
    let mut depends_on = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    let mut dependents = BTreeMap::<StableId, BTreeSet<StableId>>::new();
    for limitation in &extraction.limitations {
        let deps = limitation
            .source_ids
            .iter()
            .filter(|source_id| limitation_ids.contains(*source_id))
            .cloned()
            .collect::<BTreeSet<_>>();
        for dep in &deps {
            dependents
                .entry(dep.clone())
                .or_default()
                .insert(limitation.id.clone());
        }
        depends_on.insert(limitation.id.clone(), deps);
    }

    let mut remaining_out_degree = depends_on
        .iter()
        .map(|(id, deps)| (id.clone(), deps.len()))
        .collect::<BTreeMap<_, _>>();
    let mut ready = remaining_out_degree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut remaining = limitation_ids.clone();
    while let Some(id) = ready.iter().next().cloned() {
        ready.remove(&id);
        remaining.remove(&id);
        if let Some(affected) = dependents.get(&id) {
            for dependent in affected {
                if let Some(degree) = remaining_out_degree.get_mut(dependent) {
                    *degree -= 1;
                    if *degree == 0 {
                        ready.insert(dependent.clone());
                    }
                }
            }
        }
    }
    if let Some(cycle_id) = remaining.iter().next() {
        return Err(DomainError::Validation(format!(
            "limitation source cycle includes `{cycle_id}`"
        )));
    }

    let mut grounded = extraction
        .limitations
        .iter()
        .filter(|limitation| {
            limitation
                .source_ids
                .iter()
                .any(|source_id| !limitation_ids.contains(source_id))
        })
        .map(|limitation| limitation.id.clone())
        .collect::<BTreeSet<_>>();
    let mut frontier = grounded.clone();
    while let Some(id) = frontier.iter().next().cloned() {
        frontier.remove(&id);
        if let Some(affected) = dependents.get(&id) {
            for dependent in affected {
                if grounded.insert(dependent.clone()) {
                    frontier.insert(dependent.clone());
                }
            }
        }
    }
    if let Some(ungrounded_id) = limitation_ids.iter().find(|id| !grounded.contains(*id)) {
        return Err(DomainError::Validation(format!(
            "limitation `{ungrounded_id}` is not grounded in a non-limitation fact"
        )));
    }

    Ok(())
}

/// Rejects facts that claim the same origin (source kind, locator, and
/// origin-local identity) while disagreeing about that origin's content hash.
/// A single source position cannot honestly produce two different byte
/// observations within one snapshot.
fn validate_source_identity_consistency(
    artifacts: &[Artifact],
    relations: &[Relation],
    contexts: &[ReviewContext],
    invariants: &[Invariant],
) -> Result<()> {
    let mut seen: BTreeMap<(String, String, Option<String>), ContentHash> = BTreeMap::new();
    let sources = artifacts
        .iter()
        .map(|item| &item.provenance)
        .chain(relations.iter().map(|item| &item.provenance))
        .chain(contexts.iter().map(|item| &item.provenance))
        .chain(invariants.iter().map(|item| &item.provenance))
        .map(Provenance::source);
    for source in sources {
        let Some(content_hash) = source.content_hash.as_ref() else {
            continue;
        };
        let key = (
            source.kind.clone(),
            source.locator.clone(),
            source.source_local_id.clone(),
        );
        match seen.get(&key) {
            Some(existing) if existing != content_hash => {
                return Err(DomainError::Validation(format!(
                    "source identity `{}` (`{}`) reports conflicting content hashes",
                    key.1,
                    key.2.as_deref().unwrap_or("")
                )));
            }
            Some(_) => {}
            None => {
                seen.insert(key, content_hash.clone());
            }
        }
    }
    Ok(())
}

/// Accepted, language-neutral input facts for exactly one snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct ProgramSpace {
    schema: String,
    source: SourceRef,
    repository_id: StableId,
    repository_name: String,
    repository_root: Option<String>,
    repository_uri: Option<String>,
    snapshot_id: StableId,
    base_revision: String,
    target_revision: String,
    tree_hash: ContentHash,
    dirty: bool,
    snapshot_created_at: Option<String>,
    profile_id: String,
    profile_version: String,
    rule_set_hash: ContentHash,
    policy_version: String,
    artifacts: Vec<Artifact>,
    relations: Vec<Relation>,
    contexts: Vec<ReviewContext>,
    invariants: Vec<Invariant>,
    evidence: Vec<Evidence>,
    extraction: Extraction,
}

pub(crate) struct ProgramSpaceStreamingRef<'a>(&'a ProgramSpace);

struct SourceRefStreamingRef<'a>(&'a SourceRef);

impl Serialize for SourceRefStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("SourceRef", 5)?;
        if let Some(content_hash) = &value.content_hash {
            state.serialize_field("content_hash", content_hash)?;
        }
        state.serialize_field("kind", &value.kind)?;
        state.serialize_field("locator", &value.locator)?;
        if let Some(revision) = &value.revision {
            state.serialize_field("revision", revision)?;
        }
        if let Some(source_local_id) = &value.source_local_id {
            state.serialize_field("source_local_id", source_local_id)?;
        }
        state.end()
    }
}

struct ProvenanceStreamingRef<'a>(&'a Provenance);

impl Serialize for ProvenanceStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("Provenance", 5)?;
        if let Some(confidence) = value.confidence {
            state.serialize_field("confidence", &confidence)?;
        }
        state.serialize_field("extraction_method", &value.extraction_method)?;
        state.serialize_field("review_status", &value.review_status)?;
        state.serialize_field("source", &SourceRefStreamingRef(&value.source))?;
        if let Some(tool_version) = &value.tool_version {
            state.serialize_field("tool_version", tool_version)?;
        }
        state.end()
    }
}

struct LocationStreamingRef<'a>(&'a Location);

impl Serialize for LocationStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("Location", 6)?;
        if let Some(end_column) = value.end_column {
            state.serialize_field("end_column", &end_column)?;
        }
        if let Some(end_line) = value.end_line {
            state.serialize_field("end_line", &end_line)?;
        }
        state.serialize_field("path", &value.path)?;
        if let Some(start_column) = value.start_column {
            state.serialize_field("start_column", &start_column)?;
        }
        if let Some(start_line) = value.start_line {
            state.serialize_field("start_line", &start_line)?;
        }
        if let Some(symbol_id) = &value.symbol_id {
            state.serialize_field("symbol_id", symbol_id)?;
        }
        state.end()
    }
}

struct ArtifactStreamingRef<'a>(&'a Artifact);
struct ArtifactSequence<'a>(&'a [Artifact]);

impl Serialize for ArtifactStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("Artifact", 8)?;
        state.serialize_field("attributes", &value.attributes)?;
        if let Some(content_hash) = &value.content_hash {
            state.serialize_field("content_hash", content_hash)?;
        }
        state.serialize_field("id", &value.id)?;
        state.serialize_field("kind", &value.kind)?;
        state.serialize_field("label", &value.label)?;
        if let Some(language) = &value.language {
            state.serialize_field("language", language)?;
        }
        if let Some(location) = &value.location {
            state.serialize_field("location", &LocationStreamingRef(location))?;
        }
        state.serialize_field("provenance", &ProvenanceStreamingRef(&value.provenance))?;
        state.end()
    }
}

impl Serialize for ArtifactSequence<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&ArtifactStreamingRef(value))?;
        }
        sequence.end()
    }
}

struct RelationStreamingRef<'a>(&'a Relation);
struct RelationSequence<'a>(&'a [Relation]);
impl Serialize for RelationStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("Relation", 7)?;
        state.serialize_field("attributes", &value.attributes)?;
        state.serialize_field("directed", &value.directed)?;
        state.serialize_field("id", &value.id)?;
        state.serialize_field("kind", &value.kind)?;
        state.serialize_field("provenance", &ProvenanceStreamingRef(&value.provenance))?;
        state.serialize_field("source_id", &value.source_id)?;
        state.serialize_field("target_ids", &value.target_ids)?;
        state.end()
    }
}
impl Serialize for RelationSequence<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&RelationStreamingRef(value))?;
        }
        sequence.end()
    }
}

struct ContextStreamingRef<'a>(&'a ReviewContext);
struct ContextSequence<'a>(&'a [ReviewContext]);
impl Serialize for ContextStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("ReviewContext", 6)?;
        state.serialize_field("attributes", &value.attributes)?;
        state.serialize_field("id", &value.id)?;
        state.serialize_field("kind", &value.kind)?;
        state.serialize_field("label", &value.label)?;
        state.serialize_field("member_ids", &value.member_ids)?;
        state.serialize_field("provenance", &ProvenanceStreamingRef(&value.provenance))?;
        state.end()
    }
}
impl Serialize for ContextSequence<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&ContextStreamingRef(value))?;
        }
        sequence.end()
    }
}

struct InvariantStreamingRef<'a>(&'a Invariant);
struct InvariantSequence<'a>(&'a [Invariant]);
impl Serialize for InvariantStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("Invariant", 7)?;
        state.serialize_field("description", &value.description)?;
        state.serialize_field("id", &value.id)?;
        state.serialize_field("property_id", &value.property_id)?;
        state.serialize_field("provenance", &ProvenanceStreamingRef(&value.provenance))?;
        state.serialize_field("scope_ids", &value.scope_ids)?;
        state.serialize_field("severity", &value.severity)?;
        if let Some(mode) = &value.verification_mode {
            state.serialize_field("verification_mode", mode)?;
        }
        state.end()
    }
}
impl Serialize for InvariantSequence<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&InvariantStreamingRef(value))?;
        }
        sequence.end()
    }
}

struct ProgramEvidenceStreamingRef<'a>(&'a Evidence);

impl Serialize for ProgramEvidenceStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("Evidence", 7)?;
        if let Some(artifact_ref) = &value.artifact_ref {
            state.serialize_field("artifact_ref", artifact_ref)?;
        }
        state.serialize_field("attributes", &value.attributes)?;
        if let Some(content_hash) = &value.content_hash {
            state.serialize_field("content_hash", content_hash)?;
        }
        state.serialize_field("id", &value.id)?;
        state.serialize_field("kind", &value.kind)?;
        state.serialize_field("provenance", &ProvenanceStreamingRef(&value.provenance))?;
        state.serialize_field("target_ids", &value.target_ids)?;
        state.end()
    }
}

struct ProgramEvidenceSequence<'a>(&'a [Evidence]);

impl Serialize for ProgramEvidenceSequence<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for evidence in self.0 {
            sequence.serialize_element(&ProgramEvidenceStreamingRef(evidence))?;
        }
        sequence.end()
    }
}

struct AdapterStreamingRef<'a>(&'a AdapterDescriptor);
struct AdapterSequence<'a>(&'a [AdapterDescriptor]);
impl Serialize for AdapterStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("AdapterDescriptor", 5)?;
        state.serialize_field("id", &value.id)?;
        if let Some(parsed) = value.parsed {
            state.serialize_field("parsed", &parsed)?;
        }
        state.serialize_field("status", &value.status)?;
        if let Some(total) = value.total {
            state.serialize_field("total", &total)?;
        }
        state.serialize_field("version", &value.version)?;
        state.end()
    }
}
impl Serialize for AdapterSequence<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&AdapterStreamingRef(value))?;
        }
        sequence.end()
    }
}

struct CapabilityStreamingRef<'a>(&'a CapabilityDeclaration);
struct CapabilityMapStreamingRef<'a>(&'a BTreeMap<String, CapabilityDeclaration>);
impl Serialize for CapabilityStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("CapabilityDeclaration", 2)?;
        state.serialize_field("source_ids", &self.0.source_ids)?;
        state.serialize_field("state", &self.0.state)?;
        state.end()
    }
}
impl Serialize for CapabilityMapStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0 {
            map.serialize_entry(key, &CapabilityStreamingRef(value))?;
        }
        map.end()
    }
}

struct LimitationStreamingRef<'a>(&'a Limitation);
struct LimitationSequence<'a>(&'a [Limitation]);
impl Serialize for LimitationStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("Limitation", 6)?;
        state.serialize_field("description", &value.description)?;
        state.serialize_field("id", &value.id)?;
        state.serialize_field("kind", &value.kind)?;
        state.serialize_field("related_capabilities", &value.related_capabilities)?;
        state.serialize_field("severity", &value.severity)?;
        state.serialize_field("source_ids", &value.source_ids)?;
        state.end()
    }
}
impl Serialize for LimitationSequence<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            sequence.serialize_element(&LimitationStreamingRef(value))?;
        }
        sequence.end()
    }
}

struct ExtractionStreamingRef<'a>(&'a Extraction);
impl Serialize for ExtractionStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("Extraction", 4)?;
        state.serialize_field("adapter_set_hash", &value.adapter_set_hash)?;
        state.serialize_field("adapters", &AdapterSequence(&value.adapters))?;
        state.serialize_field(
            "capabilities",
            &CapabilityMapStreamingRef(&value.capabilities),
        )?;
        state.serialize_field("limitations", &LimitationSequence(&value.limitations))?;
        state.end()
    }
}

struct ProgramProfileStreamingRef<'a>(&'a ProgramSpace);
impl Serialize for ProgramProfileStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("Profile", 4)?;
        state.serialize_field("id", &value.profile_id)?;
        state.serialize_field("policy_version", &value.policy_version)?;
        state.serialize_field("rule_set_hash", &value.rule_set_hash)?;
        state.serialize_field("version", &value.profile_version)?;
        state.end()
    }
}

struct ProgramRepositoryStreamingRef<'a>(&'a ProgramSpace);
impl Serialize for ProgramRepositoryStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("Repository", 4)?;
        state.serialize_field("id", &value.repository_id)?;
        state.serialize_field("name", &value.repository_name)?;
        if let Some(root) = &value.repository_root {
            state.serialize_field("root", root)?;
        }
        if let Some(uri) = &value.repository_uri {
            state.serialize_field("uri", uri)?;
        }
        state.end()
    }
}

struct ProgramSnapshotStreamingRef<'a>(&'a ProgramSpace);
impl Serialize for ProgramSnapshotStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("Snapshot", 6)?;
        state.serialize_field("base_revision", &value.base_revision)?;
        if let Some(created_at) = &value.snapshot_created_at {
            state.serialize_field("created_at", created_at)?;
        }
        state.serialize_field("dirty", &value.dirty)?;
        state.serialize_field("id", &value.snapshot_id)?;
        state.serialize_field("target_revision", &value.target_revision)?;
        state.serialize_field("tree_hash", &value.tree_hash)?;
        state.end()
    }
}

impl Serialize for ProgramSpaceStreamingRef<'_> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value = self.0;
        let mut state = serializer.serialize_struct("ProgramSpace", 11)?;
        state.serialize_field("artifacts", &ArtifactSequence(&value.artifacts))?;
        state.serialize_field("contexts", &ContextSequence(&value.contexts))?;
        state.serialize_field("evidence", &ProgramEvidenceSequence(&value.evidence))?;
        state.serialize_field("extraction", &ExtractionStreamingRef(&value.extraction))?;
        state.serialize_field("invariants", &InvariantSequence(&value.invariants))?;
        state.serialize_field("profile", &ProgramProfileStreamingRef(value))?;
        state.serialize_field("relations", &RelationSequence(&value.relations))?;
        state.serialize_field("repository", &ProgramRepositoryStreamingRef(value))?;
        state.serialize_field("schema", &value.schema)?;
        state.serialize_field("snapshot", &ProgramSnapshotStreamingRef(value))?;
        state.serialize_field("source", &SourceRefStreamingRef(&value.source))?;
        state.end()
    }
}

/// Serializes only the checked-in ProgramSpace input contract. Internal
/// snapshot bindings on evidence remain available to the review aggregate but
/// are not invented as fields in the v1 input schema.
impl Serialize for ProgramSpace {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[cfg(test)]
        PROGRAM_SPACE_GENERIC_SERIALIZE_CALLS.with(|calls| calls.set(calls.get() + 1));
        let evidence = self
            .evidence
            .iter()
            .map(|item| {
                serde_json::json!({
                    "id": item.id,
                    "kind": item.kind,
                    "target_ids": item.target_ids,
                    "artifact_ref": item.artifact_ref,
                    "content_hash": item.content_hash,
                    "attributes": item.attributes,
                    "provenance": item.provenance,
                })
            })
            .collect::<Vec<_>>();
        let mut document = serde_json::json!({
            "schema": self.schema,
            "source": self.source,
            "repository": {
                "id": self.repository_id,
                "name": self.repository_name,
                "root": self.repository_root,
                "uri": self.repository_uri,
            },
            "snapshot": {
                "id": self.snapshot_id,
                "base_revision": self.base_revision,
                "target_revision": self.target_revision,
                "tree_hash": self.tree_hash,
                "dirty": self.dirty,
                "created_at": self.snapshot_created_at,
            },
            "profile": {
                "id": self.profile_id,
                "version": self.profile_version,
                "rule_set_hash": self.rule_set_hash,
                "policy_version": self.policy_version,
            },
            "artifacts": self.artifacts,
            "relations": self.relations,
            "contexts": self.contexts,
            "invariants": self.invariants,
            "evidence": evidence,
            "extraction": self.extraction,
        });
        omit_program_structural_nulls(&mut document);
        document.serialize(serializer)
    }
}

/// Omits only optional fields defined by the ProgramSpace input contract.
///
/// Accepted `attributes` are arbitrary JSON facts: a `null` inside them is a
/// meaningful value and must survive a ProgramSpace round trip.  Do not turn
/// this into a recursive "remove nulls" pass.
fn omit_program_structural_nulls(document: &mut Value) {
    let Some(root) = document.as_object_mut() else {
        return;
    };
    omit_null_fields(root, &["source"]);
    if let Some(source) = root.get_mut("source") {
        omit_source_ref_nulls(source);
    }
    if let Some(repository) = root.get_mut("repository") {
        omit_object_nulls(repository, &["root", "uri"]);
    }
    if let Some(snapshot) = root.get_mut("snapshot") {
        omit_object_nulls(snapshot, &["created_at"]);
    }
    for collection in [
        "artifacts",
        "relations",
        "contexts",
        "invariants",
        "evidence",
    ] {
        let Some(items) = root.get_mut(collection).and_then(Value::as_array_mut) else {
            continue;
        };
        for item in items {
            omit_object_nulls(
                item,
                &[
                    "language",
                    "location",
                    "content_hash",
                    "verification_mode",
                    "artifact_ref",
                ],
            );
            if let Some(location) = item.get_mut("location") {
                omit_object_nulls(
                    location,
                    &[
                        "start_line",
                        "end_line",
                        "start_column",
                        "end_column",
                        "symbol_id",
                    ],
                );
            }
            if let Some(provenance) = item.get_mut("provenance") {
                omit_object_nulls(provenance, &["tool_version", "confidence"]);
                if let Some(source) = provenance.get_mut("source") {
                    omit_source_ref_nulls(source);
                }
            }
        }
    }
    if let Some(extraction) = root.get_mut("extraction")
        && let Some(adapters) = extraction.get_mut("adapters").and_then(Value::as_array_mut)
    {
        for adapter in adapters {
            omit_object_nulls(adapter, &["parsed", "total"]);
        }
    }
}

fn omit_source_ref_nulls(value: &mut Value) {
    omit_object_nulls(value, &["revision", "content_hash", "source_local_id"]);
}

fn omit_object_nulls(value: &mut Value, fields: &[&str]) {
    if let Some(object) = value.as_object_mut() {
        omit_null_fields(object, fields);
    }
}

fn omit_null_fields(object: &mut serde_json::Map<String, Value>, fields: &[&str]) {
    for field in fields {
        if object.get(*field).is_some_and(Value::is_null) {
            object.remove(*field);
        }
    }
}

/// Current `ProgramSpace` manual JSON adapter input schema discriminator.
const PROGRAM_SPACE_SCHEMA_V2: &str = "reviewgraphen.program_space.input.v2";

/// Superseded `ProgramSpace` manual JSON adapter input schema discriminator;
/// recognized only to return a typed [`DomainError::MigrationRequired`].
const PROGRAM_SPACE_SCHEMA_V1: &str = "reviewgraphen.program_space.input.v1";

/// Fixed [`MigrationRecord`] schema discriminator.
const MIGRATION_RECORD_SCHEMA: &str = "reviewgraphen.program_space.migration.v1";

impl ProgramSpace {
    pub(crate) fn streaming_ref(&self) -> ProgramSpaceStreamingRef<'_> {
        ProgramSpaceStreamingRef(self)
    }

    pub(crate) fn allocated_bytes(&self) -> usize {
        fn ids(values: &BTreeSet<StableId>) -> usize {
            values.len() * std::mem::size_of::<StableId>()
                + values.iter().map(StableId::allocated_bytes).sum::<usize>()
        }
        fn strings(values: &BTreeSet<String>) -> usize {
            values.len() * std::mem::size_of::<String>()
                + values.iter().map(String::capacity).sum::<usize>()
        }
        fn json(value: &Value) -> usize {
            match value {
                Value::String(value) => value.capacity(),
                Value::Array(values) => {
                    values.capacity() * std::mem::size_of::<Value>()
                        + values.iter().map(json).sum::<usize>()
                }
                Value::Object(values) => {
                    values.len() * std::mem::size_of::<(String, Value)>()
                        + values
                            .iter()
                            .map(|(key, value)| key.capacity() + json(value))
                            .sum::<usize>()
                }
                _ => 0,
            }
        }
        fn attributes(values: &BTreeMap<String, Value>) -> usize {
            values.len() * std::mem::size_of::<(String, Value)>()
                + values
                    .iter()
                    .map(|(key, value)| key.capacity() + json(value))
                    .sum::<usize>()
        }
        let artifacts = self.artifacts.capacity() * std::mem::size_of::<Artifact>()
            + self
                .artifacts
                .iter()
                .map(|value| {
                    value.id.allocated_bytes()
                        + value.kind.capacity()
                        + value.label.capacity()
                        + value.language.as_ref().map_or(0, String::capacity)
                        + value.location.as_ref().map_or(0, |location| {
                            location.path.capacity()
                                + location
                                    .symbol_id
                                    .as_ref()
                                    .map_or(0, StableId::allocated_bytes)
                        })
                        + value
                            .content_hash
                            .as_ref()
                            .map_or(0, ContentHash::allocated_bytes)
                        + attributes(&value.attributes)
                        + value.provenance.allocated_bytes()
                })
                .sum::<usize>();
        let relations = self.relations.capacity() * std::mem::size_of::<Relation>()
            + self
                .relations
                .iter()
                .map(|value| {
                    value.id.allocated_bytes()
                        + value.kind.capacity()
                        + value.source_id.allocated_bytes()
                        + ids(&value.target_ids)
                        + attributes(&value.attributes)
                        + value.provenance.allocated_bytes()
                })
                .sum::<usize>();
        let contexts = self.contexts.capacity() * std::mem::size_of::<ReviewContext>()
            + self
                .contexts
                .iter()
                .map(|value| {
                    value.id.allocated_bytes()
                        + value.kind.capacity()
                        + value.label.capacity()
                        + ids(&value.member_ids)
                        + attributes(&value.attributes)
                        + value.provenance.allocated_bytes()
                })
                .sum::<usize>();
        let invariants = self.invariants.capacity() * std::mem::size_of::<Invariant>()
            + self
                .invariants
                .iter()
                .map(|value| {
                    value.id.allocated_bytes()
                        + value.property_id.capacity()
                        + value.description.capacity()
                        + ids(&value.scope_ids)
                        + value.verification_mode.as_ref().map_or(0, String::capacity)
                        + value.provenance.allocated_bytes()
                })
                .sum::<usize>();
        let evidence = self.evidence.capacity() * std::mem::size_of::<Evidence>()
            + self
                .evidence
                .iter()
                .map(Evidence::allocated_bytes)
                .sum::<usize>();
        let extraction = self.extraction.adapter_set_hash.allocated_bytes()
            + self.extraction.adapters.capacity() * std::mem::size_of::<AdapterDescriptor>()
            + self
                .extraction
                .adapters
                .iter()
                .map(|value| value.id.capacity() + value.version.capacity())
                .sum::<usize>()
            + self.extraction.capabilities.len()
                * std::mem::size_of::<(String, CapabilityDeclaration)>()
            + self
                .extraction
                .capabilities
                .iter()
                .map(|(key, value)| key.capacity() + ids(&value.source_ids))
                .sum::<usize>()
            + self.extraction.limitations.capacity() * std::mem::size_of::<Limitation>()
            + self
                .extraction
                .limitations
                .iter()
                .map(|value| {
                    value.id.allocated_bytes()
                        + value.description.capacity()
                        + ids(&value.source_ids)
                        + strings(&value.related_capabilities)
                })
                .sum::<usize>();
        self.schema.capacity()
            + self.source.allocated_bytes()
            + self.repository_id.allocated_bytes()
            + self.repository_name.capacity()
            + self.repository_root.as_ref().map_or(0, String::capacity)
            + self.repository_uri.as_ref().map_or(0, String::capacity)
            + self.snapshot_id.allocated_bytes()
            + self.base_revision.capacity()
            + self.target_revision.capacity()
            + self.tree_hash.allocated_bytes()
            + self
                .snapshot_created_at
                .as_ref()
                .map_or(0, String::capacity)
            + self.profile_id.capacity()
            + self.profile_version.capacity()
            + self.rule_set_hash.allocated_bytes()
            + self.policy_version.capacity()
            + artifacts
            + relations
            + contexts
            + invariants
            + evidence
            + extraction
    }

    /// Parses the supported v2 manual JSON adapter input and validates
    /// references. The top-level `schema` discriminator is probed first, so
    /// a v1 document reports a typed migration requirement and an unknown
    /// or malformed discriminator reports a typed unsupported-schema error,
    /// instead of both falling through to a generic v2 shape-mismatch error.
    pub fn from_json_slice(input: &[u8]) -> Result<Self> {
        let value: Value =
            serde_json::from_slice(input).map_err(|error| DomainError::Json(error.to_string()))?;
        match value.get("schema").and_then(Value::as_str) {
            Some(PROGRAM_SPACE_SCHEMA_V2) => {
                let raw: RawProgramSpace = serde_json::from_slice(input)
                    .map_err(|error| DomainError::Json(error.to_string()))?;
                Self::try_from(raw)
            }
            Some(PROGRAM_SPACE_SCHEMA_V1) => Err(DomainError::MigrationRequired {
                detected: PROGRAM_SPACE_SCHEMA_V1.to_owned(),
                required: PROGRAM_SPACE_SCHEMA_V2.to_owned(),
            }),
            Some(other) => Err(DomainError::UnsupportedSchema {
                detected: Some(other.to_owned()),
            }),
            None => Err(DomainError::UnsupportedSchema { detected: None }),
        }
    }

    /// Stable snapshot ID bound to every generated obligation.
    #[must_use]
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }

    /// Repository identity for an external report adapter.
    #[must_use]
    pub fn repository_id(&self) -> &StableId {
        &self.repository_id
    }

    /// Canonical top-level source whose content hash binds host repository
    /// authority to the exact ingested ProgramSpace snapshot.
    pub(crate) fn repository_source(&self) -> &SourceRef {
        &self.source
    }

    /// Optional repository root retained from the input contract.
    #[must_use]
    pub fn repository_root(&self) -> Option<&str> {
        self.repository_root.as_deref()
    }

    /// Optional repository URI retained from the input contract.
    #[must_use]
    pub fn repository_uri(&self) -> Option<&str> {
        self.repository_uri.as_deref()
    }

    /// Fixed base revision of this ProgramSpace snapshot.
    #[must_use]
    pub fn base_revision(&self) -> &str {
        &self.base_revision
    }

    /// Fixed target revision of this ProgramSpace snapshot.
    #[must_use]
    pub fn target_revision(&self) -> &str {
        &self.target_revision
    }

    /// Optional input snapshot timestamp retained without reinterpretation.
    #[must_use]
    pub fn snapshot_created_at(&self) -> Option<&str> {
        self.snapshot_created_at.as_deref()
    }

    /// Creates the only public admission for evidence recorded against this
    /// validated ProgramSpace snapshot.
    #[must_use]
    pub fn evidence_snapshot_admission(&self) -> EvidenceSnapshotAdmission {
        EvidenceSnapshotAdmission {
            snapshot_id: self.snapshot_id.clone(),
        }
    }

    /// Normalized profile spelling compatible with the obligation schema.
    #[must_use]
    pub fn profile_key(&self) -> String {
        format!("{}@{}", self.profile_id, self.profile_version)
    }

    /// Profile identity retained by this immutable snapshot.
    #[must_use]
    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    /// Profile version retained by this immutable snapshot.
    #[must_use]
    pub fn profile_version(&self) -> &str {
        &self.profile_version
    }

    /// Canonical repository identity retained in snapshot provenance.
    #[must_use]
    pub fn repository_identity(&self) -> &str {
        self.repository_uri
            .as_deref()
            .unwrap_or_else(|| self.source.locator())
    }

    /// Hash of the selected rule pack.
    #[must_use]
    pub fn rule_set_hash(&self) -> &ContentHash {
        &self.rule_set_hash
    }

    /// Hash of the deterministic extractor/adaptor set.
    #[must_use]
    pub fn extractor_set_hash(&self) -> &ContentHash {
        &self.extraction.adapter_set_hash
    }

    /// Versioned policy identity.
    #[must_use]
    pub fn policy_version(&self) -> &str {
        &self.policy_version
    }

    /// Accepted artifacts in stable ID order.
    #[must_use]
    pub fn artifacts(&self) -> &[Artifact] {
        &self.artifacts
    }

    /// Accepted relations in stable ID order.
    #[must_use]
    pub fn relations(&self) -> &[Relation] {
        &self.relations
    }

    /// Accepted contexts in stable ID order.
    #[must_use]
    pub fn contexts(&self) -> &[ReviewContext] {
        &self.contexts
    }

    /// Accepted invariants in stable ID order.
    #[must_use]
    pub fn invariants(&self) -> &[Invariant] {
        &self.invariants
    }

    /// Imported evidence facts. These retain a separate namespace from
    /// Program fact IDs and are seeded into a review aggregate explicitly.
    #[must_use]
    pub fn evidence(&self) -> &[Evidence] {
        &self.evidence
    }

    /// Extraction declaration including retained limitations.
    #[must_use]
    pub fn extraction(&self) -> &Extraction {
        &self.extraction
    }

    /// Returns program-fact IDs only. Evidence is deliberately not mixed into
    /// this set; use [`Self::known_evidence_ids`] for that namespace.
    #[must_use]
    pub fn known_ids(&self) -> BTreeSet<StableId> {
        let mut ids = BTreeSet::from([self.repository_id.clone(), self.snapshot_id.clone()]);
        for artifact in &self.artifacts {
            ids.insert(artifact.id.clone());
        }
        for relation in &self.relations {
            ids.insert(relation.id.clone());
        }
        for context in &self.contexts {
            ids.insert(context.id.clone());
        }
        for invariant in &self.invariants {
            ids.insert(invariant.id.clone());
        }
        for limitation in &self.extraction.limitations {
            ids.insert(limitation.id.clone());
        }
        ids
    }

    /// Returns imported evidence IDs only.
    #[must_use]
    pub fn known_evidence_ids(&self) -> BTreeSet<StableId> {
        self.evidence.iter().map(|item| item.id().clone()).collect()
    }

    /// Returns an artifact by ID.
    #[must_use]
    pub fn artifact(&self, id: &StableId) -> Option<&Artifact> {
        self.artifacts.iter().find(|artifact| &artifact.id == id)
    }

    /// Returns a relation by ID.
    #[must_use]
    pub fn relation(&self, id: &StableId) -> Option<&Relation> {
        self.relations.iter().find(|relation| &relation.id == id)
    }

    /// Returns an invariant by ID.
    #[must_use]
    pub fn invariant(&self, id: &StableId) -> Option<&Invariant> {
        self.invariants.iter().find(|invariant| &invariant.id == id)
    }
}

impl<'de> Deserialize<'de> for ProgramSpace {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawProgramSpace::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

impl TryFrom<RawProgramSpace> for ProgramSpace {
    type Error = DomainError;

    fn try_from(raw: RawProgramSpace) -> Result<Self> {
        if raw.schema != PROGRAM_SPACE_SCHEMA_V2 {
            return Err(DomainError::UnsupportedSchema {
                detected: Some(raw.schema),
            });
        }
        let source: SourceRef = raw.source.try_into()?;
        let repository = RepositoryDescriptor {
            id: StableId::parse(raw.repository.id)?,
            name: raw.repository.name,
            root: raw.repository.root,
            uri: raw.repository.uri,
        };
        let snapshot = SnapshotDescriptor {
            id: StableId::parse(raw.snapshot.id)?,
            base_revision: raw.snapshot.base_revision,
            target_revision: raw.snapshot.target_revision,
            tree_hash: ContentHash::parse(raw.snapshot.tree_hash)?,
            dirty: raw.snapshot.dirty,
            created_at: raw.snapshot.created_at,
        };
        let profile = ProfileDescriptor {
            id: raw.profile.id,
            version: raw.profile.version,
            rule_set_hash: ContentHash::parse(raw.profile.rule_set_hash)?,
            policy_version: raw.profile.policy_version,
        };
        let extraction: Extraction = raw.extraction.try_into()?;

        let artifacts = raw
            .artifacts
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Artifact>>>()?;
        let relations = raw
            .relations
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Relation>>>()?;
        let contexts = raw
            .contexts
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<ReviewContext>>>()?;
        let invariants = raw
            .invariants
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Invariant>>>()?;
        let snapshot_id = snapshot.id.clone();
        let evidence = raw
            .evidence
            .into_iter()
            .map(|item| Evidence::from_input(item, snapshot_id.clone()))
            .collect::<Result<Vec<Evidence>>>()?;

        ProgramSpaceBuilder::new(source, repository, snapshot, profile, extraction)?
            .with_artifacts(artifacts)
            .with_relations(relations)
            .with_contexts(contexts)
            .with_invariants(invariants)
            .with_evidence(evidence)
            .build()
    }
}

fn validate_reference(
    owner: &'static str,
    owner_id: &StableId,
    known: &BTreeSet<StableId>,
    reference: &StableId,
) -> Result<()> {
    if known.contains(reference) {
        Ok(())
    } else {
        Err(DomainError::DanglingReference {
            owner,
            owner_id: owner_id.clone(),
            reference: reference.clone(),
        })
    }
}

fn ensure_non_empty(value: &str, field: &'static str) -> Result<()> {
    if value.is_empty() {
        Err(DomainError::EmptyField { field })
    } else {
        Ok(())
    }
}

fn is_symbol_kind(kind: &str) -> bool {
    matches!(kind, "class" | "type" | "function" | "method" | "field")
}

fn require_enum(value: &str, allowed: &[&str], field: &'static str) -> Result<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(DomainError::Validation(format!(
            "{field} has unsupported value `{value}`"
        )))
    }
}

fn ids(values: Vec<String>) -> Result<BTreeSet<StableId>> {
    let mut ids = BTreeSet::new();
    for value in values {
        let id = StableId::parse(value)?;
        if !ids.insert(id.clone()) {
            return Err(DomainError::IdCollision { id });
        }
    }
    Ok(ids)
}

fn provenance(raw: RawProvenance) -> Result<Provenance> {
    let source: SourceRef = raw.source.try_into()?;
    if raw.review_status != ReviewStatus::Accepted {
        return Err(DomainError::Validation(
            "ProgramSpace accepts only deterministic accepted facts; human or model review output belongs in ReviewSpace"
                .to_owned(),
        ));
    }
    Provenance::accepted_deterministic(
        source,
        raw.extraction_method,
        raw.tool_version,
        raw.confidence,
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProgramSpace {
    schema: String,
    source: RawSourceRef,
    repository: RawRepository,
    snapshot: RawSnapshot,
    profile: RawProfile,
    artifacts: Vec<RawArtifact>,
    relations: Vec<RawRelation>,
    contexts: Vec<RawContext>,
    invariants: Vec<RawInvariant>,
    evidence: Vec<RawEvidence>,
    extraction: RawExtraction,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSourceRef {
    kind: String,
    locator: String,
    revision: Option<String>,
    content_hash: Option<String>,
    source_local_id: Option<String>,
}

impl TryFrom<RawSourceRef> for SourceRef {
    type Error = DomainError;

    fn try_from(raw: RawSourceRef) -> Result<Self> {
        SourceRef::new(
            raw.kind,
            raw.locator,
            raw.revision,
            raw.content_hash.map(ContentHash::parse).transpose()?,
            raw.source_local_id,
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRepository {
    id: String,
    name: String,
    root: Option<String>,
    uri: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSnapshot {
    id: String,
    base_revision: String,
    target_revision: String,
    tree_hash: String,
    dirty: bool,
    created_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    id: String,
    version: String,
    rule_set_hash: String,
    policy_version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvenance {
    source: RawSourceRef,
    extraction_method: String,
    tool_version: Option<String>,
    confidence: Option<f64>,
    review_status: ReviewStatus,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLocation {
    path: String,
    start_line: Option<u64>,
    end_line: Option<u64>,
    start_column: Option<u64>,
    end_column: Option<u64>,
    symbol_id: Option<String>,
}

impl TryFrom<RawLocation> for Location {
    type Error = DomainError;

    fn try_from(raw: RawLocation) -> Result<Self> {
        Location::new(
            raw.path,
            raw.start_line,
            raw.end_line,
            raw.start_column,
            raw.end_column,
            raw.symbol_id.map(StableId::parse).transpose()?,
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArtifact {
    id: String,
    kind: String,
    label: String,
    language: Option<String>,
    location: Option<RawLocation>,
    content_hash: Option<String>,
    #[serde(default)]
    attributes: BTreeMap<String, Value>,
    provenance: RawProvenance,
}

impl TryFrom<RawArtifact> for Artifact {
    type Error = DomainError;

    fn try_from(raw: RawArtifact) -> Result<Self> {
        Artifact::new(
            StableId::parse(raw.id)?,
            raw.kind,
            raw.label,
            raw.language,
            raw.location.map(TryInto::try_into).transpose()?,
            raw.content_hash.map(ContentHash::parse).transpose()?,
            raw.attributes,
            provenance(raw.provenance)?,
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRelation {
    id: String,
    kind: String,
    source_id: String,
    target_ids: Vec<String>,
    directed: bool,
    #[serde(default)]
    attributes: BTreeMap<String, Value>,
    provenance: RawProvenance,
}

impl TryFrom<RawRelation> for Relation {
    type Error = DomainError;

    fn try_from(raw: RawRelation) -> Result<Self> {
        Relation::new(
            StableId::parse(raw.id)?,
            raw.kind,
            StableId::parse(raw.source_id)?,
            ids(raw.target_ids)?,
            raw.directed,
            raw.attributes,
            provenance(raw.provenance)?,
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContext {
    id: String,
    kind: String,
    label: String,
    member_ids: Vec<String>,
    #[serde(default)]
    attributes: BTreeMap<String, Value>,
    provenance: RawProvenance,
}

impl TryFrom<RawContext> for ReviewContext {
    type Error = DomainError;

    fn try_from(raw: RawContext) -> Result<Self> {
        ReviewContext::new(
            StableId::parse(raw.id)?,
            raw.kind,
            raw.label,
            ids(raw.member_ids)?,
            raw.attributes,
            provenance(raw.provenance)?,
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInvariant {
    id: String,
    property_id: String,
    description: String,
    scope_ids: Vec<String>,
    severity: Severity,
    verification_mode: Option<String>,
    provenance: RawProvenance,
}

impl TryFrom<RawInvariant> for Invariant {
    type Error = DomainError;

    fn try_from(raw: RawInvariant) -> Result<Self> {
        Invariant::new(
            StableId::parse(raw.id)?,
            raw.property_id,
            raw.description,
            ids(raw.scope_ids)?,
            raw.severity,
            raw.verification_mode,
            provenance(raw.provenance)?,
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEvidence {
    id: String,
    kind: String,
    target_ids: Vec<String>,
    artifact_ref: Option<String>,
    content_hash: Option<String>,
    #[serde(default)]
    attributes: BTreeMap<String, Value>,
    provenance: RawProvenance,
}

impl Evidence {
    fn from_input(raw: RawEvidence, snapshot_id: StableId) -> Result<Self> {
        Evidence::for_program_space(
            StableId::parse(raw.id)?,
            raw.kind,
            ids(raw.target_ids)?,
            raw.artifact_ref,
            raw.content_hash.map(ContentHash::parse).transpose()?,
            raw.attributes,
            provenance(raw.provenance)?,
            snapshot_id,
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExtraction {
    adapter_set_hash: String,
    adapters: Vec<RawAdapterDescriptor>,
    capabilities: RawCapabilities,
    limitations: Vec<RawLimitation>,
}

/// Wraps `extraction.capabilities` so a repeated JSON object key is rejected
/// instead of silently keeping the last occurrence, which is the default
/// `serde_json` map-deserialization behavior.
struct RawCapabilities(BTreeMap<String, RawCapabilityDeclaration>);

impl<'de> Deserialize<'de> for RawCapabilities {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RawCapabilitiesVisitor;

        impl<'de> Visitor<'de> for RawCapabilitiesVisitor {
            type Value = RawCapabilities;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a map of capability name to capability declaration")
            }

            fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut capabilities = BTreeMap::new();
                while let Some((key, value)) =
                    map.next_entry::<String, RawCapabilityDeclaration>()?
                {
                    if capabilities.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate capability key `{key}`"
                        )));
                    }
                    capabilities.insert(key, value);
                }
                Ok(RawCapabilities(capabilities))
            }
        }

        deserializer.deserialize_map(RawCapabilitiesVisitor)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCapabilityDeclaration {
    state: CapabilityState,
    source_ids: Vec<String>,
}

impl TryFrom<RawCapabilityDeclaration> for CapabilityDeclaration {
    type Error = DomainError;

    fn try_from(raw: RawCapabilityDeclaration) -> Result<Self> {
        CapabilityDeclaration::new(raw.state, ids(raw.source_ids)?)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAdapterDescriptor {
    id: String,
    version: String,
    status: AdapterStatus,
    parsed: Option<u64>,
    total: Option<u64>,
}

impl TryFrom<RawAdapterDescriptor> for AdapterDescriptor {
    type Error = DomainError;

    fn try_from(raw: RawAdapterDescriptor) -> Result<Self> {
        AdapterDescriptor::new(raw.id, raw.version, raw.status, raw.parsed, raw.total)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLimitation {
    id: String,
    kind: LimitationKind,
    description: String,
    severity: Severity,
    #[serde(default)]
    source_ids: Vec<String>,
    #[serde(default)]
    related_capabilities: Vec<String>,
}

impl TryFrom<RawLimitation> for Limitation {
    type Error = DomainError;

    fn try_from(raw: RawLimitation) -> Result<Self> {
        Limitation::new(
            StableId::parse(raw.id)?,
            raw.kind,
            raw.description,
            raw.severity,
            ids(raw.source_ids)?,
            raw.related_capabilities.into_iter().collect(),
        )
    }
}

impl TryFrom<RawExtraction> for Extraction {
    type Error = DomainError;

    fn try_from(raw: RawExtraction) -> Result<Self> {
        let adapters = raw
            .adapters
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<AdapterDescriptor>>>()?;
        let limitations = raw
            .limitations
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Limitation>>>()?;
        let capabilities = raw
            .capabilities
            .0
            .into_iter()
            .map(|(name, declaration)| Ok((name, CapabilityDeclaration::try_from(declaration)?)))
            .collect::<Result<BTreeMap<String, CapabilityDeclaration>>>()?;
        Extraction::new(
            ContentHash::parse(raw.adapter_set_hash)?,
            adapters,
            capabilities,
            limitations,
        )
    }
}

/// Preserved `reviewgraphen.program_space.input.v1` top-level shape (see
/// ADR 0011 §1/§7). Parsed only by the explicit v1→v2 migration path, never
/// by the normal v2 parse path. Reuses every top-level child type
/// [`RawProgramSpace`] does except `extraction`, which the v1 and v2
/// schemas disagree about.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProgramSpaceV1 {
    schema: String,
    source: RawSourceRef,
    repository: RawRepository,
    snapshot: RawSnapshot,
    profile: RawProfile,
    artifacts: Vec<RawArtifact>,
    relations: Vec<RawRelation>,
    contexts: Vec<RawContext>,
    invariants: Vec<RawInvariant>,
    evidence: Vec<RawEvidence>,
    extraction: RawExtractionV1,
}

/// Preserved v1 `extraction` shape: v1 adapters are schema-identical to v2
/// ([`RawAdapterDescriptor`] is reused), but v1 `capabilities` values are a
/// bare completeness-state string and v1 `limitations` carry no
/// `related_capabilities`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExtractionV1 {
    adapter_set_hash: String,
    adapters: Vec<RawAdapterDescriptor>,
    capabilities: RawCapabilitiesV1,
    limitations: Vec<RawLimitationV1>,
}

/// Wraps preserved-v1 `extraction.capabilities` so a repeated JSON object
/// key is rejected instead of silently keeping the last occurrence, the
/// same contract [`RawCapabilities`] enforces for v2.
struct RawCapabilitiesV1(BTreeMap<String, CapabilityState>);

impl<'de> Deserialize<'de> for RawCapabilitiesV1 {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RawCapabilitiesV1Visitor;

        impl<'de> Visitor<'de> for RawCapabilitiesV1Visitor {
            type Value = RawCapabilitiesV1;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a map of capability name to v1 capability state")
            }

            fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut capabilities = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, CapabilityState>()? {
                    if capabilities.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate capability key `{key}`"
                        )));
                    }
                    capabilities.insert(key, value);
                }
                Ok(RawCapabilitiesV1(capabilities))
            }
        }

        deserializer.deserialize_map(RawCapabilitiesV1Visitor)
    }
}

/// Preserved v1 `limitation` shape. Unlike [`RawLimitation`], this must not
/// accept `related_capabilities`: the preserved v1 schema never declared
/// that property, so `deny_unknown_fields` correctly rejects it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLimitationV1 {
    id: String,
    kind: LimitationKind,
    description: String,
    severity: Severity,
    #[serde(default)]
    source_ids: Vec<String>,
}

/// Converts one deterministic binding value to canonical JSON for
/// [`StableId::derived`]. Enum values serialize through their existing
/// `Serialize` implementation rather than a hand-written string mapping.
fn migration_binding(value: &impl Serialize) -> Result<Value> {
    serde_json::to_value(value).map_err(|error| DomainError::CanonicalJson(error.to_string()))
}

/// Deterministic ID for one [`MigrationLoss`], derived from the fixed
/// migration record schema, the loss kind, the snapshot, and every semantic
/// field of that loss. `fields` keys must not collide with the three fixed
/// keys this function adds.
fn migration_loss_id(
    kind: &str,
    snapshot_id: &StableId,
    mut fields: BTreeMap<String, Value>,
) -> Result<StableId> {
    fields.insert(
        "migration_schema".to_owned(),
        Value::String(MIGRATION_RECORD_SCHEMA.to_owned()),
    );
    fields.insert("loss_kind".to_owned(), Value::String(kind.to_owned()));
    fields.insert(
        "snapshot_id".to_owned(),
        Value::String(snapshot_id.to_string()),
    );
    StableId::derived("migration_loss", &fields)
}

/// Deterministic ID for one limitation synthesized to justify a migrated
/// non-`complete` capability, derived from the source schema, the capability
/// name, and its declared state.
fn synthesized_limitation_id(capability: &str, state: CapabilityState) -> Result<StableId> {
    let bindings = BTreeMap::from([
        (
            "source_schema".to_owned(),
            Value::String(PROGRAM_SPACE_SCHEMA_V1.to_owned()),
        ),
        (
            "capability".to_owned(),
            Value::String(capability.to_owned()),
        ),
        ("state".to_owned(), migration_binding(&state)?),
    ]);
    StableId::derived("limitation", &bindings)
}

/// The kind and deterministic description of the limitation synthesized to
/// justify a migrated non-`complete` capability, or `None` for `complete`
/// (which requires no related limitation).
fn synthesized_limitation_shape(
    capability: &str,
    state: CapabilityState,
) -> Option<(LimitationKind, String)> {
    match state {
        CapabilityState::Complete => None,
        CapabilityState::Partial => Some((
            LimitationKind::ProjectionLoss,
            format!(
                "Migrated `{capability}` capability declared `partial` in \
                 {PROGRAM_SPACE_SCHEMA_V1}; its original source trace could not be \
                 recovered from the v1 record."
            ),
        )),
        CapabilityState::Missing => Some((
            LimitationKind::CapabilityMissing,
            format!(
                "Migrated `{capability}` capability declared `missing` in \
                 {PROGRAM_SPACE_SCHEMA_V1}; the v1 record carried no source trace for it."
            ),
        )),
        CapabilityState::Unknown => Some((
            LimitationKind::Unknown,
            format!(
                "Migrated `{capability}` capability declared `unknown` in \
                 {PROGRAM_SPACE_SCHEMA_V1}; the v1 record never established a \
                 completeness result for it."
            ),
        )),
    }
}

/// Migrates a preserved v1 `extraction` declaration into a v2 [`Extraction`]
/// plus its structured [`MigrationLoss`] entries (see ADR 0011 §7). Every
/// migrated capability's `source_ids` is conservatively backfilled to
/// `[snapshot_id]`; a non-`complete` capability additionally gets one
/// deterministic synthesized limitation satisfying the v2 cross-field
/// contract. Every carried-over v1 limitation keeps its original shape with
/// no `related_capabilities`; an empty v1 `source_ids` is likewise
/// backfilled to `[snapshot_id]`, while a nonempty one — dangling or not —
/// is carried through unchanged and never repaired here.
fn migrate_extraction_v1(
    raw: RawExtractionV1,
    snapshot_id: &StableId,
) -> Result<(Extraction, Vec<MigrationLoss>)> {
    let adapters = raw
        .adapters
        .into_iter()
        .map(TryInto::try_into)
        .collect::<Result<Vec<AdapterDescriptor>>>()?;

    let mut losses = Vec::new();
    let mut capabilities = BTreeMap::new();
    let mut limitations = Vec::new();

    for (capability, state) in raw.capabilities.0 {
        let assigned_source_ids = BTreeSet::from([snapshot_id.clone()]);
        let backfill_loss_id = migration_loss_id(
            "capability_source_backfill",
            snapshot_id,
            BTreeMap::from([
                ("capability".to_owned(), migration_binding(&capability)?),
                ("state".to_owned(), migration_binding(&state)?),
                (
                    "assigned_source_ids".to_owned(),
                    migration_binding(&assigned_source_ids)?,
                ),
            ]),
        )?;
        losses.push(MigrationLoss::CapabilitySourceBackfill {
            id: backfill_loss_id,
            capability: capability.clone(),
            state,
            assigned_source_ids: assigned_source_ids.clone(),
        });

        if let Some((limitation_kind, description)) =
            synthesized_limitation_shape(&capability, state)
        {
            let limitation_id = synthesized_limitation_id(&capability, state)?;
            limitations.push(Limitation::new(
                limitation_id.clone(),
                limitation_kind,
                description,
                Severity::Info,
                BTreeSet::from([snapshot_id.clone()]),
                BTreeSet::from([capability.clone()]),
            )?);

            let synthesized_loss_id = migration_loss_id(
                "synthesized_limitation",
                snapshot_id,
                BTreeMap::from([
                    ("capability".to_owned(), migration_binding(&capability)?),
                    ("state".to_owned(), migration_binding(&state)?),
                    (
                        "limitation_id".to_owned(),
                        migration_binding(&limitation_id)?,
                    ),
                    (
                        "limitation_kind".to_owned(),
                        migration_binding(&limitation_kind)?,
                    ),
                ]),
            )?;
            losses.push(MigrationLoss::SynthesizedLimitation {
                id: synthesized_loss_id,
                capability: capability.clone(),
                state,
                limitation_id,
                limitation_kind,
            });
        }

        capabilities.insert(
            capability,
            CapabilityDeclaration::new(state, assigned_source_ids)?,
        );
    }

    for raw_limitation in raw.limitations {
        let limitation_id = StableId::parse(raw_limitation.id)?;
        let original_source_ids = ids(raw_limitation.source_ids)?;

        let trace_loss_id = migration_loss_id(
            "carried_limitation_trace",
            snapshot_id,
            BTreeMap::from([
                (
                    "limitation_id".to_owned(),
                    migration_binding(&limitation_id)?,
                ),
                (
                    "original_source_ids".to_owned(),
                    migration_binding(&original_source_ids)?,
                ),
            ]),
        )?;
        losses.push(MigrationLoss::CarriedLimitationTrace {
            id: trace_loss_id,
            limitation_id: limitation_id.clone(),
            original_source_ids: original_source_ids.clone(),
        });

        let source_ids = if original_source_ids.is_empty() {
            let assigned_source_ids = BTreeSet::from([snapshot_id.clone()]);
            let backfill_loss_id = migration_loss_id(
                "limitation_source_backfill",
                snapshot_id,
                BTreeMap::from([
                    (
                        "limitation_id".to_owned(),
                        migration_binding(&limitation_id)?,
                    ),
                    (
                        "assigned_source_ids".to_owned(),
                        migration_binding(&assigned_source_ids)?,
                    ),
                ]),
            )?;
            losses.push(MigrationLoss::LimitationSourceBackfill {
                id: backfill_loss_id,
                limitation_id: limitation_id.clone(),
                assigned_source_ids: assigned_source_ids.clone(),
            });
            assigned_source_ids
        } else {
            original_source_ids
        };

        limitations.push(Limitation::new(
            limitation_id,
            raw_limitation.kind,
            raw_limitation.description,
            raw_limitation.severity,
            source_ids,
            BTreeSet::new(),
        )?);
    }

    losses.sort_by(|left, right| left.id().cmp(right.id()));

    let extraction = Extraction::new(
        ContentHash::parse(raw.adapter_set_hash)?,
        adapters,
        capabilities,
        limitations,
    )?;

    Ok((extraction, losses))
}

/// Deterministic [`MigrationRecord`] ID, derived from the fixed record
/// schema, the source and target schema discriminators, the snapshot, and
/// the ID-ordered set of loss IDs it carries.
fn migration_record_id(snapshot_id: &StableId, losses: &[MigrationLoss]) -> Result<StableId> {
    let loss_ids = losses.iter().map(MigrationLoss::id).collect::<Vec<_>>();
    let bindings = BTreeMap::from([
        (
            "record_schema".to_owned(),
            Value::String(MIGRATION_RECORD_SCHEMA.to_owned()),
        ),
        (
            "source_schema".to_owned(),
            Value::String(PROGRAM_SPACE_SCHEMA_V1.to_owned()),
        ),
        (
            "target_schema".to_owned(),
            Value::String(PROGRAM_SPACE_SCHEMA_V2.to_owned()),
        ),
        (
            "snapshot_id".to_owned(),
            Value::String(snapshot_id.to_string()),
        ),
        ("loss_ids".to_owned(), migration_binding(&loss_ids)?),
    ]);
    StableId::derived("migration", &bindings)
}

/// Explicit v1→v2 `ProgramSpace` migration entry point (ADR 0011 §7). Never
/// invoked by the normal parse path: [`ProgramSpace::from_json_slice`]
/// returns a typed [`DomainError::MigrationRequired`] for v1 input instead
/// of migrating it implicitly. The migrated facts and `Extraction` still
/// pass through the same [`ProgramSpaceBuilder::build`] contract every
/// other construction path uses, so a v1 record that is dangling, cyclic,
/// or otherwise invalid beyond what migration repairs fails closed; no
/// `MigrationRecord` is returned on failure.
pub fn migrate_program_space_v1_to_v2(input: &[u8]) -> Result<(ProgramSpace, MigrationRecord)> {
    let probe: Value =
        serde_json::from_slice(input).map_err(|error| DomainError::Json(error.to_string()))?;
    match probe.get("schema").and_then(Value::as_str) {
        Some(PROGRAM_SPACE_SCHEMA_V1) => {}
        Some(other) => {
            return Err(DomainError::UnsupportedSchema {
                detected: Some(other.to_owned()),
            });
        }
        None => return Err(DomainError::UnsupportedSchema { detected: None }),
    }

    let raw: RawProgramSpaceV1 =
        serde_json::from_slice(input).map_err(|error| DomainError::Json(error.to_string()))?;
    // Defensive re-check: the schema probe above already rejected anything
    // but a v1 discriminator before this strict v1-shaped deserialize ran,
    // so this is unreachable in practice, not the primary guard.
    if raw.schema != PROGRAM_SPACE_SCHEMA_V1 {
        return Err(DomainError::UnsupportedSchema {
            detected: Some(raw.schema),
        });
    }

    let source: SourceRef = raw.source.try_into()?;
    let repository = RepositoryDescriptor {
        id: StableId::parse(raw.repository.id)?,
        name: raw.repository.name,
        root: raw.repository.root,
        uri: raw.repository.uri,
    };
    let snapshot = SnapshotDescriptor {
        id: StableId::parse(raw.snapshot.id)?,
        base_revision: raw.snapshot.base_revision,
        target_revision: raw.snapshot.target_revision,
        tree_hash: ContentHash::parse(raw.snapshot.tree_hash)?,
        dirty: raw.snapshot.dirty,
        created_at: raw.snapshot.created_at,
    };
    let profile = ProfileDescriptor {
        id: raw.profile.id,
        version: raw.profile.version,
        rule_set_hash: ContentHash::parse(raw.profile.rule_set_hash)?,
        policy_version: raw.profile.policy_version,
    };
    let snapshot_id = snapshot.id.clone();
    let (extraction, mut losses) = migrate_extraction_v1(raw.extraction, &snapshot_id)?;

    let artifacts = raw
        .artifacts
        .into_iter()
        .map(TryInto::try_into)
        .collect::<Result<Vec<Artifact>>>()?;
    let relations = raw
        .relations
        .into_iter()
        .map(TryInto::try_into)
        .collect::<Result<Vec<Relation>>>()?;
    let contexts = raw
        .contexts
        .into_iter()
        .map(TryInto::try_into)
        .collect::<Result<Vec<ReviewContext>>>()?;
    let invariants = raw
        .invariants
        .into_iter()
        .map(TryInto::try_into)
        .collect::<Result<Vec<Invariant>>>()?;
    let evidence = raw
        .evidence
        .into_iter()
        .map(|item| Evidence::from_input(item, snapshot_id.clone()))
        .collect::<Result<Vec<Evidence>>>()?;

    let program_space =
        ProgramSpaceBuilder::new(source, repository, snapshot, profile, extraction)?
            .with_artifacts(artifacts)
            .with_relations(relations)
            .with_contexts(contexts)
            .with_invariants(invariants)
            .with_evidence(evidence)
            .build()?;

    losses.sort_by(|left, right| left.id().cmp(right.id()));
    let record = MigrationRecord {
        schema: MIGRATION_RECORD_SCHEMA.to_owned(),
        id: migration_record_id(&snapshot_id, &losses)?,
        source_schema: PROGRAM_SPACE_SCHEMA_V1.to_owned(),
        target_schema: PROGRAM_SPACE_SCHEMA_V2.to_owned(),
        snapshot_id,
        losses,
    };

    Ok((program_space, record))
}

pub(crate) fn attribute_bool(attributes: &BTreeMap<String, Value>, key: &str) -> bool {
    attributes
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn attribute_string<'a>(
    attributes: &'a BTreeMap<String, Value>,
    key: &str,
) -> Option<&'a str> {
    attributes.get(key).and_then(Value::as_str)
}
