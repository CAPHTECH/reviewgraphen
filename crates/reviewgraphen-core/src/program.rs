use crate::{ContentHash, DomainError, Result, ReviewStatus, StableId};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

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
    pub severity: String,
    /// Optional expected verification approach.
    pub verification_mode: Option<String>,
    /// Deterministic provenance.
    pub provenance: Provenance,
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
    pub kind: String,
    /// Explanation of the capability boundary.
    pub description: String,
    /// Descriptive severity.
    pub severity: String,
    /// Sorted affected IDs.
    pub source_ids: BTreeSet<StableId>,
}

/// Extractor descriptors and limits associated with accepted facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Extraction {
    /// Hash of the full adapter set.
    pub adapter_set_hash: ContentHash,
    /// Deterministically sorted adapter descriptors.
    pub adapters: Vec<AdapterDescriptor>,
    /// Capability state by capability name.
    pub capabilities: BTreeMap<String, String>,
    /// Explicit limitations, never silently erased.
    pub limitations: Vec<Limitation>,
}

/// One manual or tool adapter result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AdapterDescriptor {
    /// Adapter identity.
    pub id: String,
    /// Adapter version.
    pub version: String,
    /// `complete`, `partial`, `failed`, or `not_run`.
    pub status: String,
    /// Optional parsed count.
    pub parsed: Option<u64>,
    /// Optional total count.
    pub total: Option<u64>,
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

/// Serializes only the checked-in ProgramSpace input contract. Internal
/// snapshot bindings on evidence remain available to the review aggregate but
/// are not invented as fields in the v1 input schema.
impl Serialize for ProgramSpace {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
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

impl ProgramSpace {
    /// Parses the supported v1 manual JSON adapter input and validates references.
    pub fn from_json_slice(input: &[u8]) -> Result<Self> {
        let raw: RawProgramSpace =
            serde_json::from_slice(input).map_err(|error| DomainError::Json(error.to_string()))?;
        Self::try_from(raw)
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
        if raw.schema != "reviewgraphen.program_space.input.v1" {
            return Err(DomainError::Validation(format!(
                "unsupported ProgramSpace schema `{}`",
                raw.schema
            )));
        }
        let source: SourceRef = raw.source.try_into()?;
        if source.kind() == "model" {
            return Err(DomainError::Validation(
                "ProgramSpace input source cannot be a model".to_owned(),
            ));
        }
        let repository_id = StableId::parse(raw.repository.id)?;
        let snapshot_id = StableId::parse(raw.snapshot.id)?;
        let tree_hash = ContentHash::parse(raw.snapshot.tree_hash)?;
        let rule_set_hash = ContentHash::parse(raw.profile.rule_set_hash)?;
        let extraction: Extraction = raw.extraction.try_into()?;
        ensure_non_empty(&raw.repository.name, "repository.name")?;
        ensure_non_empty(&raw.snapshot.base_revision, "snapshot.base_revision")?;
        ensure_non_empty(&raw.snapshot.target_revision, "snapshot.target_revision")?;
        ensure_non_empty(&raw.profile.id, "profile.id")?;
        ensure_non_empty(&raw.profile.version, "profile.version")?;
        ensure_non_empty(&raw.profile.policy_version, "profile.policy_version")?;

        let mut artifacts = raw
            .artifacts
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Artifact>>>()?;
        if artifacts.is_empty() {
            return Err(DomainError::EmptyField { field: "artifacts" });
        }
        let mut relations = raw
            .relations
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Relation>>>()?;
        let mut contexts = raw
            .contexts
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<ReviewContext>>>()?;
        let mut invariants = raw
            .invariants
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<Invariant>>>()?;
        let mut evidence = raw
            .evidence
            .into_iter()
            .map(|item| Evidence::from_input(item, snapshot_id.clone()))
            .collect::<Result<Vec<Evidence>>>()?;

        artifacts.sort_by(|left, right| left.id.cmp(&right.id));
        relations.sort_by(|left, right| left.id.cmp(&right.id));
        contexts.sort_by(|left, right| left.id.cmp(&right.id));
        invariants.sort_by(|left, right| left.id.cmp(&right.id));
        evidence.sort_by(|left, right| left.id.cmp(&right.id));

        let mut ids = BTreeSet::from([repository_id.clone(), snapshot_id.clone()]);
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
        for limitation in &extraction.limitations {
            for source_id in &limitation.source_ids {
                validate_reference("limitation", &limitation.id, &ids, source_id)?;
            }
        }

        Ok(Self {
            schema: raw.schema,
            source,
            repository_id,
            repository_name: raw.repository.name,
            repository_root: raw.repository.root,
            repository_uri: raw.repository.uri,
            snapshot_id,
            base_revision: raw.snapshot.base_revision,
            target_revision: raw.snapshot.target_revision,
            tree_hash,
            dirty: raw.snapshot.dirty,
            snapshot_created_at: raw.snapshot.created_at,
            profile_id: raw.profile.id,
            profile_version: raw.profile.version,
            rule_set_hash,
            policy_version: raw.profile.policy_version,
            artifacts,
            relations,
            contexts,
            invariants,
            evidence,
            extraction,
        })
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
        ensure_non_empty(&raw.path, "location.path")?;
        for (field, value) in [
            ("location.start_line", raw.start_line),
            ("location.end_line", raw.end_line),
            ("location.start_column", raw.start_column),
            ("location.end_column", raw.end_column),
        ] {
            if value == Some(0) {
                return Err(DomainError::Validation(format!(
                    "{field} must be at least 1"
                )));
            }
        }
        Ok(Self {
            path: raw.path,
            start_line: raw.start_line,
            end_line: raw.end_line,
            start_column: raw.start_column,
            end_column: raw.end_column,
            symbol_id: raw.symbol_id.map(StableId::parse).transpose()?,
        })
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
        require_enum(
            &raw.kind,
            &[
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
            ],
            "artifact.kind",
        )?;
        ensure_non_empty(&raw.kind, "artifact.kind")?;
        ensure_non_empty(&raw.label, "artifact.label")?;
        Ok(Self {
            id: StableId::parse(raw.id)?,
            kind: raw.kind,
            label: raw.label,
            language: raw.language,
            location: raw.location.map(TryInto::try_into).transpose()?,
            content_hash: raw.content_hash.map(ContentHash::parse).transpose()?,
            attributes: raw.attributes,
            provenance: provenance(raw.provenance)?,
        })
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
        ensure_non_empty(&raw.kind, "relation.kind")?;
        let target_ids = ids(raw.target_ids)?;
        if target_ids.is_empty() {
            return Err(DomainError::EmptyField {
                field: "relation.target_ids",
            });
        }
        Ok(Self {
            id: StableId::parse(raw.id)?,
            kind: raw.kind,
            source_id: StableId::parse(raw.source_id)?,
            target_ids,
            directed: raw.directed,
            attributes: raw.attributes,
            provenance: provenance(raw.provenance)?,
        })
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
        ensure_non_empty(&raw.kind, "context.kind")?;
        ensure_non_empty(&raw.label, "context.label")?;
        Ok(Self {
            id: StableId::parse(raw.id)?,
            kind: raw.kind,
            label: raw.label,
            member_ids: ids(raw.member_ids)?,
            attributes: raw.attributes,
            provenance: provenance(raw.provenance)?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInvariant {
    id: String,
    property_id: String,
    description: String,
    scope_ids: Vec<String>,
    severity: String,
    verification_mode: Option<String>,
    provenance: RawProvenance,
}

impl TryFrom<RawInvariant> for Invariant {
    type Error = DomainError;

    fn try_from(raw: RawInvariant) -> Result<Self> {
        ensure_non_empty(&raw.property_id, "invariant.property_id")?;
        ensure_non_empty(&raw.description, "invariant.description")?;
        let scope_ids = ids(raw.scope_ids)?;
        if scope_ids.is_empty() {
            return Err(DomainError::EmptyField {
                field: "invariant.scope_ids",
            });
        }
        require_enum(
            &raw.severity,
            &["info", "low", "medium", "high", "critical"],
            "invariant.severity",
        )?;
        Ok(Self {
            id: StableId::parse(raw.id)?,
            property_id: raw.property_id,
            description: raw.description,
            scope_ids,
            severity: raw.severity,
            verification_mode: raw.verification_mode,
            provenance: provenance(raw.provenance)?,
        })
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
        // The input schema allows an empty target set for imported historical
        // evidence. Such a record remains a Program fact but cannot enter a
        // review event because `Evidence::new` requires targets.
        let kind = raw.kind;
        ensure_non_empty(&kind, "evidence.kind")?;
        Ok(Self {
            id: StableId::parse(raw.id)?,
            kind,
            target_ids: ids(raw.target_ids)?,
            artifact_ref: raw.artifact_ref,
            content_hash: raw.content_hash.map(ContentHash::parse).transpose()?,
            attributes: raw.attributes,
            provenance: provenance(raw.provenance)?,
            snapshot_id,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExtraction {
    adapter_set_hash: String,
    adapters: Vec<RawAdapterDescriptor>,
    capabilities: BTreeMap<String, String>,
    limitations: Vec<RawLimitation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAdapterDescriptor {
    id: String,
    version: String,
    status: String,
    parsed: Option<u64>,
    total: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLimitation {
    id: String,
    kind: String,
    description: String,
    severity: String,
    #[serde(default)]
    source_ids: Vec<String>,
}

impl TryFrom<RawExtraction> for Extraction {
    type Error = DomainError;

    fn try_from(raw: RawExtraction) -> Result<Self> {
        if raw.adapters.is_empty() {
            return Err(DomainError::EmptyField {
                field: "extraction.adapters",
            });
        }
        let mut adapters = raw
            .adapters
            .into_iter()
            .map(|raw| {
                ensure_non_empty(&raw.id, "adapter.id")?;
                ensure_non_empty(&raw.version, "adapter.version")?;
                require_enum(
                    &raw.status,
                    &["complete", "partial", "failed", "not_run"],
                    "adapter.status",
                )?;
                if raw
                    .parsed
                    .zip(raw.total)
                    .is_some_and(|(parsed, total)| parsed > total)
                {
                    return Err(DomainError::Validation(
                        "adapter parsed count must not exceed total".to_owned(),
                    ));
                }
                Ok(AdapterDescriptor {
                    id: raw.id,
                    version: raw.version,
                    status: raw.status,
                    parsed: raw.parsed,
                    total: raw.total,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        for capability in raw.capabilities.values() {
            require_enum(
                capability,
                &["complete", "partial", "missing", "unknown"],
                "extraction.capabilities",
            )?;
        }
        adapters.sort_by(|left, right| left.id.cmp(&right.id));
        let mut limitations = raw
            .limitations
            .into_iter()
            .map(|raw| {
                ensure_non_empty(&raw.kind, "limitation.kind")?;
                ensure_non_empty(&raw.description, "limitation.description")?;
                require_enum(
                    &raw.kind,
                    &[
                        "capability_missing",
                        "parse_failure",
                        "unresolved_relation",
                        "excluded_region",
                        "projection_loss",
                        "policy_restriction",
                        "unsupported_input",
                        "unknown",
                    ],
                    "limitation.kind",
                )?;
                require_enum(
                    &raw.severity,
                    &["info", "low", "medium", "high", "critical"],
                    "limitation.severity",
                )?;
                Ok(Limitation {
                    id: StableId::parse(raw.id)?,
                    kind: raw.kind,
                    description: raw.description,
                    severity: raw.severity,
                    source_ids: ids(raw.source_ids)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        limitations.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(Self {
            adapter_set_hash: ContentHash::parse(raw.adapter_set_hash)?,
            adapters,
            capabilities: raw.capabilities,
            limitations,
        })
    }
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
