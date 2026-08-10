use crate::context::{ContextProjectionAdmission, context_domain_error};
use crate::{
    BuiltContextProjection, ContentHash, Decision, DecisionAdmission, DomainError, Evidence,
    EvidenceAdmission, EvidenceBinding, EvidenceSnapshotAdmission, Finding, MvpRulePack,
    Obligation, ObligationLifecycle, ProgramSpace, Result, ReviewAggregate, ReviewClaim,
    ReviewContextEnvelope, ReviewPlan, StableId, TrustedHumanAdmission, UniverseDescriptor,
    Verification, canonical_json,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, value::RawValue};
use std::collections::BTreeMap;
use std::io::Write as _;

/// The immutable wire contract carried by every envelope in a stream.
///
/// V1 remains import-only so its historical hashes can be replayed exactly;
/// all newly-created logs use V2.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EventContractVersion {
    /// Pre-M3 event vocabulary and legacy aggregate genesis hash.
    V1,
    /// M3 contract with typed genesis and source registration payloads.
    V2,
}

impl EventContractVersion {
    /// Exact persisted schema tag.
    #[must_use]
    pub const fn schema(self) -> &'static str {
        match self {
            Self::V1 => "reviewgraphen.review_event.v1",
            Self::V2 => "reviewgraphen.review_event.v2",
        }
    }

    fn parse(schema: &str) -> Result<Self> {
        match schema {
            "reviewgraphen.review_event.v1" => Ok(Self::V1),
            "reviewgraphen.review_event.v2" => Ok(Self::V2),
            _ => Err(DomainError::EventSequence(
                "unsupported event schema".to_owned(),
            )),
        }
    }
}

/// Explicit genesis material required to validate a durable event stream.
/// V2 must receive the exact canonical snapshot bytes whose CAS is named by
/// the first manifest; V1 retains only its historical aggregate hash.
#[derive(Clone, Copy, Debug)]
pub enum EventStreamGenesis<'a> {
    V1(&'a ContentHash),
    V2(&'a [u8]),
}

const SYSTEM_ACTOR: &str = "reviewgraphen-core@1";
const RUN_GENESIS_SCHEMA: &str = "reviewgraphen.run_genesis.v1";
const MAX_D1_EVENT_LINE_BYTES: usize = 1_048_576;

/// Typed, canonical state from which a v2 run begins. It is deliberately not
/// `ReviewAggregate` serialization: only a pristine baseline belongs here.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunGenesisSnapshot {
    schema: String,
    program_space: ProgramSpace,
    universe: UniverseDescriptor,
    obligations: Vec<Obligation>,
}

impl RunGenesisSnapshot {
    fn from_aggregate(aggregate: &ReviewAggregate) -> Result<Self> {
        aggregate.validate_pristine_for_event_log()?;
        Ok(Self {
            schema: RUN_GENESIS_SCHEMA.to_owned(),
            program_space: aggregate.program().clone(),
            universe: aggregate.universe().clone(),
            obligations: aggregate.obligations().cloned().collect(),
        })
    }

    /// Canonical CAS bytes for the v2 genesis object.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        canonical_json(self)
    }

    /// The SHA-256 CAS identity of the canonical genesis bytes.
    pub fn canonical_hash(&self) -> Result<ContentHash> {
        Ok(ContentHash::sha256(&self.canonical_bytes()?))
    }

    /// Strictly decodes canonical v2 genesis bytes and rebuilds the aggregate
    /// through its normal validation boundary. This rejects unknown fields,
    /// noncanonical encodings, malformed ProgramSpace records, and any
    /// inconsistent universe/obligation tuple.
    pub fn from_canonical_bytes(input: &[u8]) -> Result<Self> {
        let snapshot: Self =
            serde_json::from_slice(input).map_err(|error| DomainError::Json(error.to_string()))?;
        if snapshot.schema != RUN_GENESIS_SCHEMA || snapshot.canonical_bytes()? != input {
            return Err(DomainError::Validation(
                "run genesis snapshot must use the supported canonical schema".to_owned(),
            ));
        }
        let aggregate = snapshot.rebuild_aggregate()?;
        aggregate.validate_pristine_for_event_log()?;
        Ok(snapshot)
    }

    /// Reconstructs exactly the pristine aggregate represented by this DTO.
    pub fn rebuild_aggregate(&self) -> Result<ReviewAggregate> {
        if self.schema != RUN_GENESIS_SCHEMA {
            return Err(DomainError::Validation(
                "unsupported run genesis snapshot schema".to_owned(),
            ));
        }
        let mut previous = None;
        for obligation in &self.obligations {
            obligation.validate_full()?;
            if previous
                .as_ref()
                .is_some_and(|id: &StableId| id >= obligation.id())
            {
                return Err(DomainError::Validation(
                    "run genesis obligations must be strictly ordered and unique by StableId"
                        .to_owned(),
                ));
            }
            previous = Some(obligation.id().clone());
        }
        let (expected_universe, mut expected_obligations) =
            MvpRulePack::synthesize(&self.program_space)?.into_parts();
        expected_obligations.sort_by(|left, right| left.id().cmp(right.id()));
        if expected_universe != self.universe || expected_obligations != self.obligations {
            return Err(DomainError::Validation(
                "run genesis obligations and universe must equal deterministic MVP re-synthesis"
                    .to_owned(),
            ));
        }
        let aggregate = ReviewAggregate::new(
            self.program_space.clone(),
            self.universe.clone(),
            self.obligations.clone(),
        )?;
        aggregate.validate_pristine_for_event_log()?;
        Ok(aggregate)
    }

    /// Program facts committed by this baseline.
    #[must_use]
    pub fn program_space(&self) -> &ProgramSpace {
        &self.program_space
    }

    /// Versioned obligation denominator committed by this baseline.
    #[must_use]
    pub fn universe(&self) -> &UniverseDescriptor {
        &self.universe
    }

    /// Obligations in canonical StableId order.
    #[must_use]
    pub fn obligations(&self) -> &[Obligation] {
        &self.obligations
    }
}

/// Required handling class for a registered immutable artifact.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactSensitivity {
    CanonicalState,
    WorkspaceSource,
    Sensitive,
}

/// Closed provenance for an artifact registration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ArtifactSource {
    RunGenesis {
        run_id: StableId,
    },
    SnapshotIngest {
        run_id: StableId,
        snapshot_id: StableId,
        adapter_id: String,
    },
    ReviewerExecution {
        run_id: StableId,
        execution_id: StableId,
        reviewer_id: String,
    },
}

/// Contextual registration of one content-addressed artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRegistered {
    run_id: StableId,
    registration_id: StableId,
    cas_hash: ContentHash,
    media_type: String,
    size: u64,
    sensitivity: ArtifactSensitivity,
    source: ArtifactSource,
}

impl ArtifactRegistered {
    /// Constructs a registration after requiring its explicit sensitivity and
    /// stable registration namespace.
    pub fn new(
        run_id: StableId,
        registration_id: StableId,
        cas_hash: ContentHash,
        media_type: impl Into<String>,
        size: u64,
        sensitivity: ArtifactSensitivity,
        source: ArtifactSource,
    ) -> Result<Self> {
        let media_type = media_type.into();
        if run_id.kind() != "run"
            || registration_id.kind() != "registration"
            || media_type.trim().is_empty()
        {
            return Err(DomainError::Validation(
                "artifact registration requires a registration ID and non-empty media type"
                    .to_owned(),
            ));
        }
        if registration_id
            != Self::derived_id(&run_id, &cas_hash, &media_type, sensitivity, &source)?
        {
            return Err(DomainError::Validation(
                "artifact registration ID must bind CAS metadata and source".to_owned(),
            ));
        }
        if !valid_artifact_source_sensitivity(&source, sensitivity) {
            return Err(DomainError::Validation(
                "artifact registration source and sensitivity must form a closed pair".to_owned(),
            ));
        }
        if !artifact_source_matches_run(&source, &run_id) {
            return Err(DomainError::Validation(
                "artifact registration source must bind the enclosing run".to_owned(),
            ));
        }
        Ok(Self {
            run_id,
            registration_id,
            cas_hash,
            media_type,
            size,
            sensitivity,
            source,
        })
    }

    fn derived_id(
        run_id: &StableId,
        cas_hash: &ContentHash,
        media_type: &str,
        sensitivity: ArtifactSensitivity,
        source: &ArtifactSource,
    ) -> Result<StableId> {
        let source =
            serde_json::to_value(source).map_err(|error| DomainError::Json(error.to_string()))?;
        StableId::derived(
            "registration",
            &BTreeMap::from([
                ("run_id".to_owned(), Value::String(run_id.to_string())),
                ("cas_hash".to_owned(), Value::String(cas_hash.to_string())),
                (
                    "media_type".to_owned(),
                    Value::String(media_type.to_owned()),
                ),
                (
                    "sensitivity".to_owned(),
                    Value::String(
                        match sensitivity {
                            ArtifactSensitivity::CanonicalState => "canonical_state",
                            ArtifactSensitivity::WorkspaceSource => "workspace_source",
                            ArtifactSensitivity::Sensitive => "sensitive",
                        }
                        .to_owned(),
                    ),
                ),
                ("source".to_owned(), source),
            ]),
        )
    }

    #[must_use]
    pub fn registration_id(&self) -> &StableId {
        &self.registration_id
    }
    #[must_use]
    pub fn run_id(&self) -> &StableId {
        &self.run_id
    }
    #[must_use]
    pub fn cas_hash(&self) -> &ContentHash {
        &self.cas_hash
    }
    #[must_use]
    pub const fn sensitivity(&self) -> ArtifactSensitivity {
        self.sensitivity
    }
    #[must_use]
    pub fn source(&self) -> &ArtifactSource {
        &self.source
    }
    #[must_use]
    pub fn media_type(&self) -> &str {
        &self.media_type
    }
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.size
    }

    fn validate(&self) -> Result<()> {
        let rebuilt = Self::new(
            self.run_id.clone(),
            self.registration_id.clone(),
            self.cas_hash.clone(),
            self.media_type.clone(),
            self.size,
            self.sensitivity,
            self.source.clone(),
        )?;
        let valid_source = valid_artifact_source_sensitivity(&self.source, self.sensitivity)
            && artifact_source_matches_run(&self.source, &self.run_id);
        if rebuilt != *self || !is_cas_hash(&self.cas_hash) || !valid_source {
            return Err(DomainError::Validation(
                "artifact registration requires an exact CAS hash and closed source/sensitivity pair"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

fn valid_artifact_source_sensitivity(
    source: &ArtifactSource,
    sensitivity: ArtifactSensitivity,
) -> bool {
    match (source, sensitivity) {
        (ArtifactSource::RunGenesis { run_id }, ArtifactSensitivity::CanonicalState) => {
            run_id.kind() == "run"
        }
        (
            ArtifactSource::SnapshotIngest {
                run_id,
                snapshot_id,
                adapter_id,
            },
            ArtifactSensitivity::WorkspaceSource,
        ) => {
            run_id.kind() == "run"
                && snapshot_id.kind() == "snapshot"
                && !adapter_id.trim().is_empty()
        }
        (
            ArtifactSource::ReviewerExecution {
                run_id,
                execution_id,
                reviewer_id,
            },
            ArtifactSensitivity::Sensitive,
        ) => {
            run_id.kind() == "run"
                && execution_id.kind() == "execution"
                && !reviewer_id.trim().is_empty()
        }
        _ => false,
    }
}

fn artifact_source_matches_run(source: &ArtifactSource, run_id: &StableId) -> bool {
    match source {
        ArtifactSource::RunGenesis {
            run_id: source_run_id,
        }
        | ArtifactSource::SnapshotIngest {
            run_id: source_run_id,
            ..
        }
        | ArtifactSource::ReviewerExecution {
            run_id: source_run_id,
            ..
        } => source_run_id == run_id,
    }
}

/// The mandatory first v2 event, binding a run to its typed baseline CAS
/// artifact and snapshot/profile provenance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunGenesisManifest {
    run_id: StableId,
    event_contract_version: String,
    genesis_artifact: ArtifactRegistered,
    repository_identity: String,
    snapshot_id: StableId,
    profile_id: String,
    profile_version: String,
}

impl RunGenesisManifest {
    /// Builds the sole v2 run-genesis manifest.
    pub fn new(
        run_id: StableId,
        genesis_artifact: ArtifactRegistered,
        repository_identity: impl Into<String>,
        snapshot_id: StableId,
        profile_id: impl Into<String>,
        profile_version: impl Into<String>,
    ) -> Result<Self> {
        let manifest = Self {
            run_id,
            event_contract_version: EventContractVersion::V2.schema().to_owned(),
            genesis_artifact,
            repository_identity: repository_identity.into(),
            snapshot_id,
            profile_id: profile_id.into(),
            profile_version: profile_version.into(),
        };
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<()> {
        if self.run_id.kind() != "run"
            || self.event_contract_version != EventContractVersion::V2.schema()
            || self.repository_identity.trim().is_empty()
            || self.profile_id.trim().is_empty()
            || self.profile_version.trim().is_empty()
            || self.genesis_artifact.media_type != "application/json"
            || !matches!(
                &self.genesis_artifact.source,
                ArtifactSource::RunGenesis { run_id } if run_id == &self.run_id
            )
        {
            return Err(DomainError::Validation(
                "invalid v2 run genesis manifest".to_owned(),
            ));
        }
        self.genesis_artifact.validate()
    }

    fn validate_against_genesis(
        &self,
        run_id: &StableId,
        snapshot: &RunGenesisSnapshot,
        bytes: &[u8],
    ) -> Result<()> {
        self.validate()?;
        let size = u64::try_from(bytes.len())
            .map_err(|_| DomainError::Validation("genesis bytes do not fit u64".to_owned()))?;
        if self.run_id != *run_id
            || self.genesis_artifact.run_id != *run_id
            || self.genesis_artifact.cas_hash != ContentHash::sha256(bytes)
            || self.genesis_artifact.size != size
            || self.repository_identity != snapshot.program_space.repository_identity()
            || self.snapshot_id != *snapshot.program_space.snapshot_id()
            || self.profile_id != snapshot.program_space.profile_id()
            || self.profile_version != snapshot.program_space.profile_version()
            || !matches!(
                &self.genesis_artifact.source,
                ArtifactSource::RunGenesis { run_id: source_run_id } if source_run_id == run_id
            )
        {
            return Err(DomainError::Validation(
                "run genesis manifest must exactly bind verified genesis bytes and provenance"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn genesis_artifact(&self) -> &ArtifactRegistered {
        &self.genesis_artifact
    }
    #[must_use]
    pub fn run_id(&self) -> &StableId {
        &self.run_id
    }
    #[must_use]
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
}

fn validate_v2_genesis_contract(
    run_id: &StableId,
    initial: &ReviewAggregate,
    bytes: &[u8],
    manifest: &RunGenesisManifest,
) -> Result<RunGenesisSnapshot> {
    let snapshot = RunGenesisSnapshot::from_canonical_bytes(bytes)?;
    let rebuilt = snapshot.rebuild_aggregate()?;
    if canonical_json(&rebuilt)? != canonical_json(initial)? {
        return Err(DomainError::Validation(
            "verified run genesis bytes do not reconstruct the supplied initial aggregate"
                .to_owned(),
        ));
    }
    manifest.validate_against_genesis(run_id, &snapshot, bytes)?;
    Ok(snapshot)
}

fn validate_v2_genesis_envelope(
    run_id: &StableId,
    initial: &ReviewAggregate,
    bytes: &[u8],
    envelope: &EventEnvelope,
) -> Result<RunGenesisSnapshot> {
    if envelope.genesis_hash != ContentHash::sha256(bytes) {
        return Err(DomainError::EventSequence(
            "v2 genesis envelope hash must equal the verified canonical genesis CAS".to_owned(),
        ));
    }
    let payload = decode_canonical_payload(envelope.payload.get())?;
    let PersistedPayload::RunGenesisManifest(manifest) = payload else {
        return Err(DomainError::EventSequence(
            "v2 sequence one must carry RunGenesisManifest".to_owned(),
        ));
    };
    validate_v2_genesis_contract(run_id, initial, bytes, &manifest)
}

/// One persisted source entry, referring to a prior registration rather than
/// embedding workspace bytes in the event stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotSourceRecordEntry {
    artifact_id: StableId,
    path: String,
    content_hash: ContentHash,
    registration_id: StableId,
    cas_hash: ContentHash,
    line_count: u64,
}

impl SnapshotSourceRecordEntry {
    /// Creates one immutable source-registration reference.
    pub fn new(
        artifact_id: StableId,
        path: impl Into<String>,
        content_hash: ContentHash,
        registration_id: StableId,
        cas_hash: ContentHash,
        line_count: u64,
    ) -> Result<Self> {
        let entry = Self {
            artifact_id,
            path: path.into(),
            content_hash,
            registration_id,
            cas_hash,
            line_count,
        };
        if entry.path.is_empty()
            || entry.registration_id.kind() != "registration"
            || !is_cas_hash(&entry.content_hash)
            || !is_cas_hash(&entry.cas_hash)
            || entry.content_hash != entry.cas_hash
            || entry.line_count == 0
        {
            return Err(DomainError::Validation(
                "invalid snapshot source registration entry".to_owned(),
            ));
        }
        Ok(entry)
    }
}

/// Exact source-registration projection for one accepted snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotSourcesRecorded {
    snapshot_id: StableId,
    entries: Vec<SnapshotSourceRecordEntry>,
}

impl SnapshotSourcesRecorded {
    /// Constructs a path-ordered exact source record set.
    pub fn new(snapshot_id: StableId, entries: Vec<SnapshotSourceRecordEntry>) -> Result<Self> {
        let sources = Self {
            snapshot_id,
            entries,
        };
        sources.validate_shape()?;
        Ok(sources)
    }

    fn validate_shape(&self) -> Result<()> {
        let mut paths = BTreeMap::new();
        let mut artifacts = BTreeMap::new();
        let mut previous = None;
        for entry in &self.entries {
            if entry.path.is_empty()
                || !is_cas_hash(&entry.content_hash)
                || !is_cas_hash(&entry.cas_hash)
                || entry.content_hash != entry.cas_hash
                || entry.line_count == 0
            {
                return Err(DomainError::Validation(
                    "snapshot source entries require non-empty paths and SHA-256 CAS hashes"
                        .to_owned(),
                ));
            }
            if previous
                .as_ref()
                .is_some_and(|path: &String| path >= &entry.path)
                || paths.insert(entry.path.clone(), ()).is_some()
                || artifacts.insert(entry.artifact_id.clone(), ()).is_some()
            {
                return Err(DomainError::Validation(
                    "snapshot source entries must be uniquely ordered by path".to_owned(),
                ));
            }
            previous = Some(entry.path.clone());
        }
        Ok(())
    }

    #[must_use]
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }

    #[must_use]
    pub fn entries(&self) -> &[SnapshotSourceRecordEntry] {
        &self.entries
    }
}

impl SnapshotSourceRecordEntry {
    #[must_use]
    pub fn artifact_id(&self) -> &StableId {
        &self.artifact_id
    }
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    #[must_use]
    pub fn content_hash(&self) -> &ContentHash {
        &self.content_hash
    }
    #[must_use]
    pub fn registration_id(&self) -> &StableId {
        &self.registration_id
    }
    #[must_use]
    pub fn cas_hash(&self) -> &ContentHash {
        &self.cas_hash
    }
    #[must_use]
    pub const fn line_count(&self) -> u64 {
        self.line_count
    }
}

fn is_cas_hash(hash: &ContentHash) -> bool {
    let value = hash.to_string();
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Internal exact append point for non-serializable event admissions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StreamPosition {
    pub(crate) run_id: StableId,
    pub(crate) genesis_hash: ContentHash,
    pub(crate) tail_hash: ContentHash,
    pub(crate) sequence: u64,
}

/// An opaque, locally admitted command for an append-only M1 event log.
///
/// It deliberately has no JSON deserializer. Persisted records are decoded by
/// a private DTO at replay time, where their trust capabilities are checked
/// again. This prevents a caller from forging an accepting decision or an
/// exact evidence admission by deserializing a public payload enum.
#[derive(Clone, Debug)]
pub struct EventCommand {
    payload: PersistedPayload,
    admission: CommandAdmission,
}

#[derive(Clone, Debug)]
enum CommandAdmission {
    None,
    Evidence(EvidenceAdmission),
    Binding(EvidenceBindingAdmission),
    Verification(VerificationAdmission),
    Decision(DecisionAdmission),
    ContextProjection(ContextProjectionAdmission),
}

#[derive(Default)]
struct MatchedAdmissions {
    evidence: Option<EvidenceAdmission>,
    binding: Option<EvidenceBindingAdmission>,
    verification: Option<VerificationAdmission>,
    decision: Option<DecisionAdmission>,
    context_projection: Option<PositionedContextProjectionAdmission>,
}

/// A private byte admission sealed to one exact persisted event position.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PositionedContextProjectionAdmission {
    run_id: StableId,
    genesis_hash: ContentHash,
    previous_event_hash: ContentHash,
    sequence: u64,
    event_id: StableId,
    projection: ContextProjectionAdmission,
}

impl PositionedContextProjectionAdmission {
    fn seal(
        event: &EventEnvelope,
        context: &ReviewContextEnvelope,
        projection: ContextProjectionAdmission,
    ) -> Result<Self> {
        if !projection.matches_projection(context) {
            return Err(DomainError::Validation(
                "context projection admission does not match the persisted envelope".to_owned(),
            ));
        }
        Ok(Self {
            run_id: event.run_id.clone(),
            genesis_hash: event.genesis_hash.clone(),
            previous_event_hash: event.previous_event_hash.clone(),
            sequence: event.sequence,
            event_id: event.id.clone(),
            projection,
        })
    }

    fn matches(
        &self,
        event: &EventEnvelope,
        context: &ReviewContextEnvelope,
        aggregate: &ReviewAggregate,
    ) -> Result<bool> {
        Ok(self.matches_position(event) && self.projection.matches(context, aggregate)?)
    }

    fn matches_position(&self, event: &EventEnvelope) -> bool {
        self.run_id == event.run_id
            && self.genesis_hash == event.genesis_hash
            && self.previous_event_hash == event.previous_event_hash
            && self.sequence == event.sequence
            && self.event_id == event.id
    }
}

/// Opaque, non-serializable authorization for one exact claim/evidence binding
/// at one immutable event-log genesis and append position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceBindingAdmission {
    run_id: StableId,
    genesis_hash: ContentHash,
    tail_hash: ContentHash,
    sequence: u64,
    binding_id: StableId,
    digest: ContentHash,
}

/// Opaque, non-serializable authorization for one exact, canonical verifier
/// result in one immutable event-log genesis and append position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationAdmission {
    run_id: StableId,
    genesis_hash: ContentHash,
    tail_hash: ContentHash,
    sequence: u64,
    verification_id: StableId,
    digest: ContentHash,
}

/// Non-serializable host admissions required to replay authority-bearing events.
///
/// Each token is exact, run/genesis/body-bound, and bound to the mint-time
/// chain tail plus expected next sequence. Empty admissions are appropriate
/// only for streams with no such events.
#[derive(Clone, Debug, Default)]
pub struct EventAdmissions {
    evidence: Vec<EvidenceAdmission>,
    bindings: Vec<EvidenceBindingAdmission>,
    verifications: Vec<VerificationAdmission>,
    decisions: Vec<DecisionAdmission>,
    context_projections: Vec<PositionedContextProjectionAdmission>,
}

impl EventAdmissions {
    /// Builds a host-supplied admission set for imported events.
    #[must_use]
    pub fn new(evidence: Vec<EvidenceAdmission>, decisions: Vec<DecisionAdmission>) -> Self {
        Self {
            evidence,
            bindings: Vec::new(),
            verifications: Vec::new(),
            decisions,
            context_projections: Vec::new(),
        }
    }

    /// Consumes independently rebuilt projections paired with the exact
    /// imported events they authorize, sealing each private byte admission to
    /// that event's run, genesis, predecessor, sequence, and event ID.
    pub fn with_context_projections(
        mut self,
        projections: Vec<(EventEnvelope, BuiltContextProjection)>,
    ) -> Result<Self> {
        for (event, built) in projections {
            let (built_context, projection) = built.into_parts();
            let payload = decode_canonical_payload(event.payload.get())?;
            let PersistedPayload::ContextEnvelopeProjected(event_context) = payload else {
                return Err(DomainError::Validation(
                    "context projection admission must be paired with a context event".to_owned(),
                ));
            };
            if built_context
                .canonical_bytes()
                .map_err(context_domain_error)?
                != event_context
                    .canonical_bytes()
                    .map_err(context_domain_error)?
            {
                return Err(DomainError::Validation(
                    "rebuilt context projection does not equal its paired event payload".to_owned(),
                ));
            }
            self.context_projections
                .push(PositionedContextProjectionAdmission::seal(
                    &event,
                    &event_context,
                    projection,
                )?);
        }
        Ok(self)
    }

    fn with_context_admissions(
        mut self,
        admissions: Vec<PositionedContextProjectionAdmission>,
    ) -> Self {
        self.context_projections = admissions;
        self
    }

    /// Adds the exact non-serializable admissions retained by locally recorded
    /// binding and verification events before a stream is imported again.
    #[must_use]
    pub fn with_trace_admissions(
        mut self,
        bindings: Vec<EvidenceBindingAdmission>,
        verifications: Vec<VerificationAdmission>,
    ) -> Self {
        self.bindings = bindings;
        self.verifications = verifications;
        self
    }

    fn evidence_for(
        &self,
        run_id: &StableId,
        genesis_hash: &ContentHash,
        tail_hash: &ContentHash,
        sequence: u64,
        evidence: &Evidence,
    ) -> Option<EvidenceAdmission> {
        self.evidence
            .iter()
            .find(|admission| {
                admission.matches(run_id, genesis_hash, tail_hash, sequence, evidence)
            })
            .cloned()
    }

    fn binding_for(
        &self,
        run_id: &StableId,
        genesis_hash: &ContentHash,
        tail_hash: &ContentHash,
        sequence: u64,
        binding: &EvidenceBinding,
    ) -> Option<EvidenceBindingAdmission> {
        self.bindings
            .iter()
            .find(|admission| admission.matches(run_id, genesis_hash, tail_hash, sequence, binding))
            .cloned()
    }

    fn verification_for(
        &self,
        run_id: &StableId,
        genesis_hash: &ContentHash,
        tail_hash: &ContentHash,
        sequence: u64,
        verification: &Verification,
    ) -> Option<VerificationAdmission> {
        self.verifications
            .iter()
            .find(|admission| {
                admission.matches(run_id, genesis_hash, tail_hash, sequence, verification)
            })
            .cloned()
    }

    fn decision_for(
        &self,
        position: &StreamPosition,
        universe_id: &StableId,
        closure_digest: &ContentHash,
        decision: &Decision,
    ) -> Option<DecisionAdmission> {
        self.decisions
            .iter()
            .find(|admission| {
                decision.matches_decision_admission(
                    position,
                    universe_id,
                    closure_digest,
                    admission,
                )
            })
            .cloned()
    }

    fn context_projection_for(
        &self,
        event: &EventEnvelope,
        envelope: &ReviewContextEnvelope,
        aggregate: &ReviewAggregate,
    ) -> Result<Option<PositionedContextProjectionAdmission>> {
        for admission in &self.context_projections {
            if admission.matches(event, envelope, aggregate)? {
                return Ok(Some(admission.clone()));
            }
        }
        Ok(None)
    }
}

impl EvidenceBindingAdmission {
    fn new(
        run_id: StableId,
        genesis_hash: ContentHash,
        tail_hash: ContentHash,
        sequence: u64,
        binding: &EvidenceBinding,
    ) -> Result<Self> {
        Ok(Self {
            run_id,
            genesis_hash,
            tail_hash,
            sequence,
            binding_id: binding.id().clone(),
            digest: ContentHash::sha256(&canonical_json(binding)?),
        })
    }

    fn matches(
        &self,
        run_id: &StableId,
        genesis_hash: &ContentHash,
        tail_hash: &ContentHash,
        sequence: u64,
        binding: &EvidenceBinding,
    ) -> bool {
        self.run_id == *run_id
            && self.genesis_hash == *genesis_hash
            && self.tail_hash == *tail_hash
            && self.sequence == sequence
            && self.binding_id == *binding.id()
            && canonical_json(binding).is_ok_and(|bytes| ContentHash::sha256(&bytes) == self.digest)
    }
}

impl VerificationAdmission {
    fn new(
        run_id: StableId,
        genesis_hash: ContentHash,
        tail_hash: ContentHash,
        sequence: u64,
        verification: &Verification,
    ) -> Result<Self> {
        Ok(Self {
            run_id,
            genesis_hash,
            tail_hash,
            sequence,
            verification_id: verification.id().clone(),
            digest: ContentHash::sha256(&canonical_json(verification)?),
        })
    }

    fn matches(
        &self,
        run_id: &StableId,
        genesis_hash: &ContentHash,
        tail_hash: &ContentHash,
        sequence: u64,
        verification: &Verification,
    ) -> bool {
        self.run_id == *run_id
            && self.genesis_hash == *genesis_hash
            && self.tail_hash == *tail_hash
            && self.sequence == sequence
            && self.verification_id == *verification.id()
            && canonical_json(verification)
                .is_ok_and(|bytes| ContentHash::sha256(&bytes) == self.digest)
    }
}

impl EventCommand {
    /// Requests a lifecycle transition. Completion has no trust implication.
    #[must_use]
    pub fn obligation_transition(obligation_id: StableId, next: ObligationLifecycle) -> Self {
        Self {
            payload: PersistedPayload::ObligationTransition {
                obligation_id,
                next,
            },
            admission: CommandAdmission::None,
        }
    }

    /// Adds a new, necessarily proposed and unreviewed claim.
    #[must_use]
    pub fn claim_proposed(claim: ReviewClaim) -> Self {
        Self {
            payload: PersistedPayload::ClaimProposed(claim),
            admission: CommandAdmission::None,
        }
    }

    /// Records exact snapshot-bound evidence using a run-bound admission.
    #[must_use]
    pub fn evidence_recorded(evidence: Evidence, admission: EvidenceAdmission) -> Self {
        Self {
            payload: PersistedPayload::EvidenceRecorded(Box::new(evidence)),
            admission: CommandAdmission::Evidence(admission),
        }
    }

    /// Binds one separately recorded evidence item to one claim.
    #[must_use]
    pub fn evidence_bound(binding: EvidenceBinding, admission: EvidenceBindingAdmission) -> Self {
        Self {
            payload: PersistedPayload::EvidenceBound(binding),
            admission: CommandAdmission::Binding(admission),
        }
    }

    /// Records an explicit verifier result. The log derives and persists its
    /// freshness before hashing the event.
    #[must_use]
    pub fn verification_recorded(
        verification: Verification,
        admission: VerificationAdmission,
    ) -> Self {
        Self {
            payload: PersistedPayload::VerificationRecorded(verification),
            admission: CommandAdmission::Verification(admission),
        }
    }

    /// Records an exact, run-bound trusted human decision.
    #[must_use]
    pub fn decision_recorded(decision: Decision, admission: DecisionAdmission) -> Self {
        Self {
            payload: PersistedPayload::DecisionRecorded(decision),
            admission: CommandAdmission::Decision(admission),
        }
    }

    /// Adds a report finding; aggregate validation determines its status.
    #[must_use]
    pub fn finding_recorded(finding: Finding) -> Self {
        Self {
            payload: PersistedPayload::FindingRecorded(finding),
            admission: CommandAdmission::None,
        }
    }

    /// Registers a persisted artifact in a v2 stream.
    #[must_use]
    pub fn artifact_registered(registration: ArtifactRegistered) -> Self {
        Self {
            payload: PersistedPayload::ArtifactRegistered(registration),
            admission: CommandAdmission::None,
        }
    }

    /// Records source registrations for one accepted snapshot in a v2 stream.
    #[must_use]
    pub fn snapshot_sources_recorded(sources: SnapshotSourcesRecorded) -> Self {
        Self {
            payload: PersistedPayload::SnapshotSourcesRecorded(sources),
            admission: CommandAdmission::None,
        }
    }

    /// Records one deterministic, aggregate-bound review plan in a v2 stream.
    #[must_use]
    pub fn review_plan_recorded(plan: ReviewPlan) -> Self {
        Self {
            payload: PersistedPayload::ReviewPlanRecorded(plan),
            admission: CommandAdmission::None,
        }
    }

    /// Records one byte-reverified context projection. A metadata-only
    /// envelope cannot be supplied here because the builder result is consumed
    /// together with its private runtime admission.
    #[must_use]
    pub fn context_envelope_projected(projection: BuiltContextProjection) -> Self {
        let (envelope, admission) = projection.into_parts();
        Self {
            payload: PersistedPayload::ContextEnvelopeProjected(envelope),
            admission: CommandAdmission::ContextProjection(admission),
        }
    }
}

/// The closed persisted event vocabulary. It is private so it can never be
/// constructed from untrusted JSON through a public typed API.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum PersistedPayload {
    ObligationTransition {
        obligation_id: StableId,
        next: ObligationLifecycle,
    },
    ClaimProposed(ReviewClaim),
    EvidenceRecorded(Box<Evidence>),
    EvidenceBound(EvidenceBinding),
    VerificationRecorded(Verification),
    DecisionRecorded(Decision),
    FindingRecorded(Finding),
    RunGenesisManifest(RunGenesisManifest),
    ArtifactRegistered(ArtifactRegistered),
    SnapshotSourcesRecorded(SnapshotSourcesRecorded),
    ReviewPlanRecorded(ReviewPlan),
    ContextEnvelopeProjected(ReviewContextEnvelope),
}

impl PersistedPayload {
    fn unreconciled_kind_and_id(&self) -> Option<(UnreconciledRecordKind, &StableId)> {
        match self {
            Self::EvidenceRecorded(value) => Some((UnreconciledRecordKind::Evidence, value.id())),
            Self::EvidenceBound(value) => {
                Some((UnreconciledRecordKind::EvidenceBinding, value.id()))
            }
            Self::VerificationRecorded(value) => {
                Some((UnreconciledRecordKind::Verification, value.id()))
            }
            Self::DecisionRecorded(value) => Some((UnreconciledRecordKind::Decision, value.id())),
            _ => None,
        }
    }

    fn is_v2_only(&self) -> bool {
        matches!(
            self,
            Self::RunGenesisManifest(_)
                | Self::ArtifactRegistered(_)
                | Self::SnapshotSourcesRecorded(_)
                | Self::ReviewPlanRecorded(_)
                | Self::ContextEnvelopeProjected(_)
        )
    }

    fn is_d1_bounded(&self) -> bool {
        matches!(
            self,
            Self::ReviewPlanRecorded(_) | Self::ContextEnvelopeProjected(_)
        )
    }

    fn actor(&self) -> &str {
        match self {
            Self::DecisionRecorded(decision) => decision.actor(),
            _ => SYSTEM_ACTOR,
        }
    }

    fn validate_shape(&self) -> Result<()> {
        match self {
            Self::ObligationTransition { .. } => Ok(()),
            Self::ClaimProposed(claim) => claim.validate_initial(),
            Self::EvidenceRecorded(evidence) => evidence.validate_for_review_event(),
            Self::EvidenceBound(binding) => {
                if binding
                    .scope()
                    .get("property_id")
                    .is_none_or(String::is_empty)
                {
                    return Err(DomainError::Validation(
                        "evidence binding requires a non-empty property_id scope".to_owned(),
                    ));
                }
                Ok(())
            }
            Self::VerificationRecorded(verification) => verification.validate_event_shape(),
            Self::DecisionRecorded(decision) => decision.validate_event_admission(),
            Self::FindingRecorded(_) => Ok(()),
            Self::RunGenesisManifest(manifest) => manifest.validate(),
            Self::ArtifactRegistered(registration) => registration.validate(),
            Self::SnapshotSourcesRecorded(sources) => sources.validate_shape(),
            Self::ReviewPlanRecorded(plan) => {
                let _ = plan.canonical_bytes()?;
                Ok(())
            }
            Self::ContextEnvelopeProjected(envelope) => envelope
                .canonical_bytes()
                .map(|_| ())
                .map_err(context_domain_error),
        }
    }

    fn validate_for_enclosing_run(&self, run_id: &StableId) -> Result<()> {
        match self {
            Self::ArtifactRegistered(registration) if registration.run_id() != run_id => {
                Err(DomainError::Validation(
                    "artifact registration must bind the enclosing event run".to_owned(),
                ))
            }
            Self::RunGenesisManifest(manifest)
                if manifest.run_id() != run_id
                    || manifest.genesis_artifact().run_id() != run_id =>
            {
                Err(DomainError::Validation(
                    "genesis manifest and nested artifact must bind the enclosing event run"
                        .to_owned(),
                ))
            }
            _ => Ok(()),
        }
    }
}

struct BoundedEventJson {
    bytes: Vec<u8>,
    max: usize,
    overflow: Option<usize>,
}

impl BoundedEventJson {
    fn new(max: usize) -> Self {
        Self {
            bytes: Vec::new(),
            max,
            overflow: None,
        }
    }

    fn push(&mut self, bytes: &[u8]) -> Result<()> {
        self.write_all(bytes).map_err(|error| {
            if let Some(observed) = self.overflow {
                DomainError::Incomplete {
                    operation: "D1 canonical event JSONL",
                    limit: MAX_D1_EVENT_LINE_BYTES,
                    observed: observed.saturating_add(1),
                }
            } else {
                DomainError::CanonicalJson(error.to_string())
            }
        })
    }

    fn string(&mut self, value: &str) -> Result<()> {
        serde_json::to_writer(&mut *self, value).map_err(|error| {
            if let Some(observed) = self.overflow {
                DomainError::Incomplete {
                    operation: "D1 canonical event JSONL",
                    limit: MAX_D1_EVENT_LINE_BYTES,
                    observed: observed.saturating_add(1),
                }
            } else {
                DomainError::CanonicalJson(error.to_string())
            }
        })
    }

    fn number(&mut self, value: u64) -> Result<()> {
        write!(self, "{value}").map_err(|error| DomainError::CanonicalJson(error.to_string()))
    }

    fn finish(self) -> Result<Vec<u8>> {
        if let Some(observed) = self.overflow {
            return Err(DomainError::Incomplete {
                operation: "D1 canonical event JSONL",
                limit: MAX_D1_EVENT_LINE_BYTES,
                observed: observed.saturating_add(1),
            });
        }
        Ok(self.bytes)
    }
}

impl std::io::Write for BoundedEventJson {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let observed = self.bytes.len().saturating_add(bytes.len());
        if observed > self.max {
            self.overflow = Some(observed);
            return Err(std::io::Error::other(
                "bounded event JSON exceeded its limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn d1_payload_bytes(kind: &str, data: &[u8]) -> Result<Vec<u8>> {
    let mut out = BoundedEventJson::new(MAX_D1_EVENT_LINE_BYTES - 1);
    out.push(b"{\"data\":")?;
    out.push(data)?;
    out.push(b",\"type\":")?;
    out.string(kind)?;
    out.push(b"}")?;
    out.finish()
}

fn payload_canonical_bytes(payload: &PersistedPayload) -> Result<Vec<u8>> {
    match payload {
        PersistedPayload::ReviewPlanRecorded(plan) => {
            d1_payload_bytes("review_plan_recorded", &plan.canonical_bytes()?)
        }
        PersistedPayload::ContextEnvelopeProjected(envelope) => d1_payload_bytes(
            "context_envelope_projected",
            &envelope.canonical_bytes().map_err(context_domain_error)?,
        ),
        _ => canonical_json(payload),
    }
}

fn raw_payload(bytes: Vec<u8>) -> Result<Box<RawValue>> {
    let text =
        String::from_utf8(bytes).map_err(|error| DomainError::CanonicalJson(error.to_string()))?;
    RawValue::from_string(text).map_err(|error| DomainError::Json(error.to_string()))
}

fn validate_stream_payload_position(
    version: EventContractVersion,
    sequence: u64,
    payload: &PersistedPayload,
) -> Result<()> {
    if version == EventContractVersion::V2
        && (sequence == 1) != matches!(payload, PersistedPayload::RunGenesisManifest(_))
    {
        return Err(DomainError::EventSequence(
            "v2 streams require RunGenesisManifest exactly at sequence one".to_owned(),
        ));
    }
    Ok(())
}

fn reject_v2_legacy_execution_payload(
    version: EventContractVersion,
    payload: &PersistedPayload,
) -> Result<()> {
    if version == EventContractVersion::V2
        && matches!(
            payload,
            PersistedPayload::ClaimProposed(_)
                | PersistedPayload::ObligationTransition {
                    next: ObligationLifecycle::Completed,
                    ..
                }
        )
    {
        return Err(DomainError::Validation(
            "v2 requires the Unit D atomic execution record before claims or completed obligations"
                .to_owned(),
        ));
    }
    Ok(())
}

fn reject_duplicate_d1_record(
    aggregate: &ReviewAggregate,
    payload: &PersistedPayload,
) -> Result<()> {
    let duplicate = match payload {
        PersistedPayload::ReviewPlanRecorded(plan) => aggregate.review_plan(plan.id()).is_some(),
        PersistedPayload::ContextEnvelopeProjected(context) => {
            aggregate.context_envelope(context.id()).is_some()
        }
        _ => false,
    };
    if duplicate {
        let id = match payload {
            PersistedPayload::ReviewPlanRecorded(plan) => plan.id().clone(),
            PersistedPayload::ContextEnvelopeProjected(context) => context.id().clone(),
            _ => unreachable!("duplicate is true only for D1 records"),
        };
        return Err(DomainError::IdCollision { id });
    }
    Ok(())
}

/// A validated event envelope binding a closed persisted payload to one logical
/// run and to the canonical genesis aggregate of that run.
#[derive(Clone, Debug, Serialize)]
pub struct EventEnvelope {
    schema: String,
    id: StableId,
    run_id: StableId,
    genesis_hash: ContentHash,
    sequence: u64,
    actor: String,
    logical_time: u64,
    payload: Box<RawValue>,
    payload_hash: ContentHash,
    previous_event_hash: ContentHash,
    event_hash: ContentHash,
}

impl EventEnvelope {
    // Each field is independently hash-bound. Grouping them would obscure
    // the selected schema input at call sites and risks a mismatched tuple.
    #[allow(clippy::too_many_arguments)]
    fn new(
        version: EventContractVersion,
        run_id: StableId,
        genesis_hash: ContentHash,
        sequence: u64,
        actor: impl Into<String>,
        logical_time: u64,
        previous_event_hash: ContentHash,
        payload: PersistedPayload,
    ) -> Result<Self> {
        payload.validate_shape()?;
        let actor = actor.into();
        let payload_bytes = payload_canonical_bytes(&payload)?;
        let payload_hash = ContentHash::sha256(&payload_bytes);
        let payload = raw_payload(payload_bytes)?;
        let id = event_id(
            version.schema(),
            &run_id,
            &genesis_hash,
            sequence,
            &actor,
            logical_time,
            &payload_hash,
            &previous_event_hash,
        )?;
        let event_hash = envelope_hash(EventHashInput {
            schema: version.schema(),
            id: &id,
            run_id: &run_id,
            genesis_hash: &genesis_hash,
            sequence,
            actor: &actor,
            logical_time,
            payload_hash: &payload_hash,
            previous_event_hash: &previous_event_hash,
        })?;
        let envelope = Self {
            schema: version.schema().to_owned(),
            id,
            run_id,
            genesis_hash,
            sequence,
            actor,
            logical_time,
            payload,
            payload_hash,
            previous_event_hash,
            event_hash,
        };
        envelope.validate()?;
        if envelope.payload_is_d1()? {
            let _ = envelope.canonical_bytes()?;
        }
        Ok(envelope)
    }

    #[cfg(test)]
    pub(crate) fn next_context_duplicate_for_test(
        &self,
        context: ReviewContextEnvelope,
    ) -> Result<Self> {
        let sequence = self.sequence.checked_add(1).ok_or_else(|| {
            DomainError::EventSequence("event sequence overflow in test fixture".to_owned())
        })?;
        Self::new(
            EventContractVersion::parse(&self.schema)?,
            self.run_id.clone(),
            self.genesis_hash.clone(),
            sequence,
            SYSTEM_ACTOR,
            sequence,
            self.event_hash.clone(),
            PersistedPayload::ContextEnvelopeProjected(context),
        )
    }

    /// Imports and structurally validates one JSON envelope. Applying it still
    /// requires replay with its matching run-bound decision admissions.
    pub fn from_json_slice(input: &[u8]) -> Result<Self> {
        serde_json::from_slice(input).map_err(|error| DomainError::Json(error.to_string()))
    }

    /// Validates deterministic envelope bindings and every nested untrusted
    /// DTO before the record can enter a replay stream.
    pub fn validate(&self) -> Result<()> {
        let version = EventContractVersion::parse(&self.schema)?;
        if self.run_id.kind() != "run" || self.sequence == 0 || self.logical_time != self.sequence {
            return Err(DomainError::EventSequence(
                "event requires a run ID, one-based sequence, and matching logical time".to_owned(),
            ));
        }
        if self.actor.is_empty() {
            return Err(DomainError::EventSequence(
                "event actor must be non-empty".to_owned(),
            ));
        }
        let payload = decode_canonical_payload(self.payload.get())?;
        if version == EventContractVersion::V1 && payload.is_v2_only() {
            return Err(DomainError::EventSequence(
                "v1 stream cannot contain v2-only payloads".to_owned(),
            ));
        }
        validate_stream_payload_position(version, self.sequence, &payload)?;
        if let PersistedPayload::RunGenesisManifest(manifest) = &payload
            && (manifest.run_id != self.run_id
                || manifest.genesis_artifact.cas_hash != self.genesis_hash)
        {
            return Err(DomainError::EventSequence(
                "v2 genesis manifest must bind its envelope run ID and genesis hash".to_owned(),
            ));
        }
        if self.actor != payload.actor() {
            return Err(DomainError::EventSequence(
                "event actor must match the typed payload authority".to_owned(),
            ));
        }
        let expected_payload_bytes = payload_canonical_bytes(&payload)?;
        let actual_payload_bytes = self.payload.get().as_bytes();
        if actual_payload_bytes != expected_payload_bytes {
            return Err(DomainError::EventSequence(
                "persisted event payload must equal its canonical closed representation".to_owned(),
            ));
        }
        let expected_hash = ContentHash::sha256(&expected_payload_bytes);
        if self.payload_hash != expected_hash {
            return Err(DomainError::EventSequence(
                "event payload hash does not match payload".to_owned(),
            ));
        }
        let expected_id = event_id(
            version.schema(),
            &self.run_id,
            &self.genesis_hash,
            self.sequence,
            &self.actor,
            self.logical_time,
            &self.payload_hash,
            &self.previous_event_hash,
        )?;
        if self.id != expected_id {
            return Err(DomainError::EventSequence(
                "event ID does not bind its envelope".to_owned(),
            ));
        }
        let expected_event_hash = envelope_hash(EventHashInput {
            schema: version.schema(),
            id: &self.id,
            run_id: &self.run_id,
            genesis_hash: &self.genesis_hash,
            sequence: self.sequence,
            actor: &self.actor,
            logical_time: self.logical_time,
            payload_hash: &self.payload_hash,
            previous_event_hash: &self.previous_event_hash,
        })?;
        if self.event_hash != expected_event_hash {
            return Err(DomainError::EventSequence(
                "event hash does not bind its envelope and predecessor".to_owned(),
            ));
        }
        if payload.is_d1_bounded() {
            let _ = self.canonical_bytes()?;
        }
        Ok(())
    }

    fn payload_is_d1(&self) -> Result<bool> {
        Ok(decode_canonical_payload(self.payload.get())?.is_d1_bounded())
    }

    /// Exact canonical event bytes. D1 payloads are written through a bounded
    /// writer whose limit reserves the mandatory trailing LF byte.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        let payload = decode_canonical_payload(self.payload.get())?;
        if !payload.is_d1_bounded() {
            return canonical_json(self);
        }
        let payload_bytes = payload_canonical_bytes(&payload)?;
        let mut out = BoundedEventJson::new(MAX_D1_EVENT_LINE_BYTES - 1);
        out.push(b"{\"actor\":")?;
        out.string(&self.actor)?;
        out.push(b",\"event_hash\":")?;
        out.string(&self.event_hash.to_string())?;
        out.push(b",\"genesis_hash\":")?;
        out.string(&self.genesis_hash.to_string())?;
        out.push(b",\"id\":")?;
        out.string(&self.id.to_string())?;
        out.push(b",\"logical_time\":")?;
        out.number(self.logical_time)?;
        out.push(b",\"payload\":")?;
        out.push(&payload_bytes)?;
        out.push(b",\"payload_hash\":")?;
        out.string(&self.payload_hash.to_string())?;
        out.push(b",\"previous_event_hash\":")?;
        out.string(&self.previous_event_hash.to_string())?;
        out.push(b",\"run_id\":")?;
        out.string(&self.run_id.to_string())?;
        out.push(b",\"schema\":")?;
        out.string(&self.schema)?;
        out.push(b",\"sequence\":")?;
        out.number(self.sequence)?;
        out.push(b"}")?;
        let bytes = out.finish()?;
        let line_length = bytes.len().checked_add(1).ok_or(DomainError::Incomplete {
            operation: "D1 canonical event JSONL",
            limit: MAX_D1_EVENT_LINE_BYTES,
            observed: usize::MAX,
        })?;
        if line_length > MAX_D1_EVENT_LINE_BYTES {
            return Err(DomainError::Incomplete {
                operation: "D1 canonical event JSONL",
                limit: MAX_D1_EVENT_LINE_BYTES,
                observed: line_length,
            });
        }
        Ok(bytes)
    }

    /// Validates a contiguous prefix, including the requested run even for an
    /// empty stream and the exact initial aggregate hash for every event.
    pub fn validate_sequence(
        version: EventContractVersion,
        run_id: &StableId,
        genesis_hash: &ContentHash,
        events: &[EventEnvelope],
    ) -> Result<()> {
        if run_id.kind() != "run" {
            return Err(DomainError::EventSequence(
                "event stream requires a run ID even when empty".to_owned(),
            ));
        }
        let mut previous_event_hash = event_chain_genesis_hash(run_id, genesis_hash)?;
        for (index, event) in events.iter().enumerate() {
            let expected = u64::try_from(index).map_err(|_| {
                DomainError::EventSequence("event count does not fit u64".to_owned())
            })? + 1;
            event.validate()?;
            let event_version = EventContractVersion::parse(&event.schema)?;
            if event_version != version {
                return Err(DomainError::EventSequence(
                    "event stream schema must match its explicit contract version".to_owned(),
                ));
            }
            if &event.run_id != run_id
                || &event.genesis_hash != genesis_hash
                || event.sequence != expected
                || event.previous_event_hash != previous_event_hash
            {
                return Err(DomainError::EventSequence(
                    "event run IDs, genesis hashes, sequences, and hashes must form one contiguous prefix"
                    .to_owned(),
                ));
            }
            previous_event_hash = event.event_hash.clone();
        }
        Ok(())
    }

    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    #[must_use]
    pub fn run_id(&self) -> &StableId {
        &self.run_id
    }

    /// Canonical initial aggregate binding for the complete event stream.
    #[must_use]
    pub fn genesis_hash(&self) -> &ContentHash {
        &self.genesis_hash
    }

    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub fn actor(&self) -> &str {
        &self.actor
    }

    #[must_use]
    pub const fn logical_time(&self) -> u64 {
        self.logical_time
    }

    #[must_use]
    pub fn payload_hash(&self) -> &ContentHash {
        &self.payload_hash
    }

    /// Hash of the immediately preceding event, or the deterministic genesis
    /// sentinel for the first event in a stream.
    #[must_use]
    pub fn previous_event_hash(&self) -> &ContentHash {
        &self.previous_event_hash
    }

    /// Hash binding this event's canonical envelope and predecessor.
    #[must_use]
    pub fn event_hash(&self) -> &ContentHash {
        &self.event_hash
    }

    /// Validates one complete, confirmed stream prefix and exposes only
    /// typed payloads. Store code must not inspect the private JSON payload.
    pub fn validated_view<'a>(
        version: EventContractVersion,
        run_id: &StableId,
        genesis: EventStreamGenesis<'_>,
        events: &'a [EventEnvelope],
    ) -> Result<ValidatedEventView<'a>> {
        let (genesis_hash, snapshot) = match (version, genesis) {
            (EventContractVersion::V1, EventStreamGenesis::V1(hash)) => (hash.clone(), None),
            (EventContractVersion::V2, EventStreamGenesis::V2(bytes)) => {
                let snapshot = RunGenesisSnapshot::from_canonical_bytes(bytes)?;
                (ContentHash::sha256(bytes), Some(snapshot))
            }
            _ => {
                return Err(DomainError::EventSequence(
                    "event stream genesis material must match its explicit contract version"
                        .to_owned(),
                ));
            }
        };
        Self::validate_sequence(version, run_id, &genesis_hash, events)?;
        if let (EventContractVersion::V2, Some(first), Some(snapshot)) =
            (version, events.first(), snapshot.as_ref())
        {
            let initial = snapshot.rebuild_aggregate()?;
            let bytes = snapshot.canonical_bytes()?;
            validate_v2_genesis_envelope(run_id, &initial, &bytes, first)?;
        }
        let events = events
            .iter()
            .map(|envelope| {
                Ok(ValidatedEvent {
                    envelope,
                    payload: decoded_payload(decode_canonical_payload(envelope.payload.get())?),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ValidatedEventView {
            version,
            run_id: run_id.clone(),
            genesis_hash,
            snapshot,
            events,
        })
    }
}

/// Typed, hash-chain-validated event prefix for durable projections.
#[derive(Clone, Debug)]
pub struct ValidatedEventView<'a> {
    version: EventContractVersion,
    run_id: StableId,
    genesis_hash: ContentHash,
    snapshot: Option<RunGenesisSnapshot>,
    events: Vec<ValidatedEvent<'a>>,
}

impl<'a> ValidatedEventView<'a> {
    #[must_use]
    pub const fn event_contract_version(&self) -> EventContractVersion {
        self.version
    }
    #[must_use]
    pub fn events(&self) -> &[ValidatedEvent<'a>] {
        &self.events
    }

    #[must_use]
    pub fn genesis_hash(&self) -> &ContentHash {
        &self.genesis_hash
    }
    #[must_use]
    pub fn run_id(&self) -> &StableId {
        &self.run_id
    }

    fn validate_initial(&self, initial: &ReviewAggregate) -> Result<()> {
        initial.validate_pristine_for_event_log()?;
        match (self.version, &self.snapshot) {
            (EventContractVersion::V1, None) => {
                if self.genesis_hash != ContentHash::sha256(&canonical_json(initial)?) {
                    return Err(DomainError::Validation(
                        "offline v1 projection initial aggregate does not match its genesis hash"
                            .to_owned(),
                    ));
                }
            }
            (EventContractVersion::V2, Some(snapshot)) => {
                let bytes = snapshot.canonical_bytes()?;
                let rebuilt = snapshot.rebuild_aggregate()?;
                if self.genesis_hash != ContentHash::sha256(&bytes)
                    || canonical_json(&rebuilt)? != canonical_json(initial)?
                {
                    return Err(DomainError::Validation(
                        "offline v2 projection initial aggregate does not match verified genesis bytes"
                            .to_owned(),
                    ));
                }
            }
            _ => {
                return Err(DomainError::Validation(
                    "validated event view has inconsistent genesis material".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// A validated envelope paired with its decoded closed payload.
#[derive(Clone, Debug)]
pub struct ValidatedEvent<'a> {
    envelope: &'a EventEnvelope,
    payload: DecodedPayload,
}

impl<'a> ValidatedEvent<'a> {
    #[must_use]
    pub fn envelope(&self) -> &'a EventEnvelope {
        self.envelope
    }
    #[must_use]
    pub fn payload(&self) -> &DecodedPayload {
        &self.payload
    }
}

/// Store-facing typed metadata. It intentionally excludes raw JSON.
#[derive(Clone, Debug)]
pub enum DecodedPayload {
    ObligationTransition {
        obligation_id: StableId,
        next: ObligationLifecycle,
    },
    ClaimProposed(ReviewClaim),
    EvidenceRecorded(Evidence),
    EvidenceBound(EvidenceBinding),
    VerificationRecorded(Verification),
    DecisionRecorded(Decision),
    FindingRecorded(Finding),
    RunGenesisManifest(RunGenesisManifest),
    ArtifactRegistered(ArtifactRegistered),
    SnapshotSourcesRecorded(SnapshotSourcesRecorded),
    ReviewPlanRecorded(ReviewPlan),
    ContextEnvelopeProjected(ReviewContextEnvelope),
}

fn decoded_payload(payload: PersistedPayload) -> DecodedPayload {
    match payload {
        PersistedPayload::ObligationTransition {
            obligation_id,
            next,
        } => DecodedPayload::ObligationTransition {
            obligation_id,
            next,
        },
        PersistedPayload::ClaimProposed(value) => DecodedPayload::ClaimProposed(value),
        PersistedPayload::EvidenceRecorded(value) => DecodedPayload::EvidenceRecorded(*value),
        PersistedPayload::EvidenceBound(value) => DecodedPayload::EvidenceBound(value),
        PersistedPayload::VerificationRecorded(value) => {
            DecodedPayload::VerificationRecorded(value)
        }
        PersistedPayload::DecisionRecorded(value) => DecodedPayload::DecisionRecorded(value),
        PersistedPayload::FindingRecorded(value) => DecodedPayload::FindingRecorded(value),
        PersistedPayload::RunGenesisManifest(value) => DecodedPayload::RunGenesisManifest(value),
        PersistedPayload::ArtifactRegistered(value) => DecodedPayload::ArtifactRegistered(value),
        PersistedPayload::SnapshotSourcesRecorded(value) => {
            DecodedPayload::SnapshotSourcesRecorded(value)
        }
        PersistedPayload::ReviewPlanRecorded(value) => DecodedPayload::ReviewPlanRecorded(value),
        PersistedPayload::ContextEnvelopeProjected(value) => {
            DecodedPayload::ContextEnvelopeProjected(value)
        }
    }
}

/// The kind of a record retained by an offline projection without making it
/// part of the accepted aggregate.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum UnreconciledRecordKind {
    /// Separately stored evidence.
    Evidence,
    /// A claim-to-evidence link.
    EvidenceBinding,
    /// A verifier outcome.
    Verification,
    /// A human authority decision.
    Decision,
}

/// Safe, typed index metadata for a record which has not been reconciled into
/// accepted aggregate state.  The record body remains private to the
/// projection so callers cannot mistake it for an accepted review fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnreconciledRecordMetadata {
    kind: UnreconciledRecordKind,
    id: StableId,
    body_hash: ContentHash,
    authority_reconciled: bool,
}

impl UnreconciledRecordMetadata {
    /// Persisted record kind.
    #[must_use]
    pub fn kind(&self) -> UnreconciledRecordKind {
        self.kind
    }

    /// Stable record identifier.
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    /// Hash of the canonical persisted payload body.
    #[must_use]
    pub fn body_hash(&self) -> &ContentHash {
        &self.body_hash
    }

    /// Always false until an explicit reconciliation path is added.
    #[must_use]
    pub const fn authority_reconciled(&self) -> bool {
        self.authority_reconciled
    }
}

/// Authority-free finding metadata projected against the private authority
/// shadow. It is intentionally distinct from unreconciled authority records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectedFindingMetadata {
    id: StableId,
    body_hash: ContentHash,
}

/// Metadata-only context projection retained during offline replay. The
/// envelope body stays private and never enters the live aggregate map.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectedContextEnvelopeMetadata {
    id: StableId,
    projection_hash: ContentHash,
    body_hash: ContentHash,
}

impl ProjectedContextEnvelopeMetadata {
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    #[must_use]
    pub fn projection_hash(&self) -> &ContentHash {
        &self.projection_hash
    }

    #[must_use]
    pub fn body_hash(&self) -> &ContentHash {
        &self.body_hash
    }
}

impl ProjectedFindingMetadata {
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    #[must_use]
    pub fn body_hash(&self) -> &ContentHash {
        &self.body_hash
    }
}

/// Semantic projection state for offline indexing. Authority-bearing records
/// are validated against a private shadow overlay, never replayed into the
/// accepted aggregate because an offline store cannot mint their admissions.
#[derive(Clone, Debug)]
pub struct OfflineProjectionState {
    version: EventContractVersion,
    run_id: StableId,
    genesis_hash: ContentHash,
    next_sequence: u64,
    previous_event_hash: ContentHash,
    expected_events: Vec<OfflineExpectedEvent>,
    aggregate: ReviewAggregate,
    unreconciled_records: BTreeMap<StableId, PersistedPayload>,
    unreconciled_metadata: BTreeMap<StableId, UnreconciledRecordMetadata>,
    projected_findings: BTreeMap<StableId, Finding>,
    projected_finding_metadata: BTreeMap<StableId, ProjectedFindingMetadata>,
    projected_context_metadata: BTreeMap<StableId, ProjectedContextEnvelopeMetadata>,
    unreconciled_order: Vec<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OfflineExpectedEvent {
    id: StableId,
    event_hash: ContentHash,
}

impl OfflineProjectionState {
    /// Seeds offline semantic validation only from a view whose immutable
    /// genesis has already been verified at the durable boundary.
    pub fn new(view: &ValidatedEventView<'_>, initial: ReviewAggregate) -> Result<Self> {
        view.validate_initial(&initial)?;
        Ok(Self {
            version: view.version,
            run_id: view.run_id.clone(),
            genesis_hash: view.genesis_hash.clone(),
            next_sequence: 1,
            previous_event_hash: event_chain_genesis_hash(&view.run_id, &view.genesis_hash)?,
            expected_events: view
                .events
                .iter()
                .map(|event| OfflineExpectedEvent {
                    id: event.envelope.id.clone(),
                    event_hash: event.envelope.event_hash.clone(),
                })
                .collect(),
            aggregate: initial,
            unreconciled_records: BTreeMap::new(),
            unreconciled_metadata: BTreeMap::new(),
            projected_findings: BTreeMap::new(),
            projected_finding_metadata: BTreeMap::new(),
            projected_context_metadata: BTreeMap::new(),
            unreconciled_order: Vec::new(),
        })
    }

    /// Applies the next exact validated-view event atomically. Returns `true`
    /// only when an authority-free event is applied to the accepted aggregate.
    /// Returns `false` when either an authority-bearing event is retained as
    /// unreconciled shadow metadata, or an authority-free `FindingRecorded`
    /// is validated against that shadow and retained as projected-finding
    /// metadata. Neither `false` case promotes state into the accepted
    /// aggregate.
    pub fn apply(&mut self, event: &ValidatedEvent<'_>) -> Result<bool> {
        let envelope = event.envelope();
        let expected = self
            .expected_events
            .get(usize::try_from(self.next_sequence - 1).map_err(|_| {
                DomainError::EventSequence(
                    "offline projection sequence does not fit usize".to_owned(),
                )
            })?)
            .ok_or_else(|| {
                DomainError::EventSequence(
                    "offline projection cannot apply an event beyond its validated view".to_owned(),
                )
            })?;
        if envelope.id != expected.id || envelope.event_hash != expected.event_hash {
            return Err(DomainError::EventSequence(
                "offline projection event is not the expected validated-view event".to_owned(),
            ));
        }
        if envelope.run_id != self.run_id
            || envelope.genesis_hash != self.genesis_hash
            || envelope.sequence != self.next_sequence
            || envelope.previous_event_hash != self.previous_event_hash
        {
            return Err(DomainError::EventSequence(
                "offline projection event does not continue its validated view cursor".to_owned(),
            ));
        }
        if EventContractVersion::parse(&event.envelope().schema)? != self.version {
            return Err(DomainError::EventSequence(
                "offline projection event schema does not match its validated view contract"
                    .to_owned(),
            ));
        }
        let payload = match event.payload() {
            DecodedPayload::EvidenceRecorded(value) => {
                return self.apply_unreconciled(
                    PersistedPayload::EvidenceRecorded(Box::new(value.clone())),
                    envelope,
                    event.envelope().actor(),
                );
            }
            DecodedPayload::EvidenceBound(value) => {
                return self.apply_unreconciled(
                    PersistedPayload::EvidenceBound(value.clone()),
                    envelope,
                    event.envelope().actor(),
                );
            }
            DecodedPayload::VerificationRecorded(value) => {
                return self.apply_unreconciled(
                    PersistedPayload::VerificationRecorded(value.clone()),
                    envelope,
                    event.envelope().actor(),
                );
            }
            DecodedPayload::DecisionRecorded(value) => {
                return self.apply_unreconciled(
                    PersistedPayload::DecisionRecorded(value.clone()),
                    envelope,
                    event.envelope().actor(),
                );
            }
            DecodedPayload::ObligationTransition {
                obligation_id,
                next,
            } => PersistedPayload::ObligationTransition {
                obligation_id: obligation_id.clone(),
                next: *next,
            },
            DecodedPayload::ClaimProposed(value) => PersistedPayload::ClaimProposed(value.clone()),
            DecodedPayload::FindingRecorded(value) => {
                return self.apply_projected_finding(
                    value.clone(),
                    envelope,
                    event.envelope().actor(),
                );
            }
            DecodedPayload::RunGenesisManifest(value) => {
                PersistedPayload::RunGenesisManifest(value.clone())
            }
            DecodedPayload::ArtifactRegistered(value) => {
                PersistedPayload::ArtifactRegistered(value.clone())
            }
            DecodedPayload::SnapshotSourcesRecorded(value) => {
                PersistedPayload::SnapshotSourcesRecorded(value.clone())
            }
            DecodedPayload::ReviewPlanRecorded(value) => {
                PersistedPayload::ReviewPlanRecorded(value.clone())
            }
            DecodedPayload::ContextEnvelopeProjected(value) => {
                return self.apply_projected_context(value.clone(), envelope);
            }
        };
        reject_v2_legacy_execution_payload(self.version, &payload)?;
        reject_duplicate_d1_record(&self.aggregate, &payload)?;
        payload.validate_for_enclosing_run(&self.run_id)?;
        let mut next = self.aggregate.clone();
        apply(&mut next, &payload, event.envelope().actor(), &self.run_id)?;
        next.validate()?;
        self.aggregate = next;
        self.advance_offline_cursor(envelope)?;
        Ok(true)
    }

    fn advance_offline_cursor(&mut self, envelope: &EventEnvelope) -> Result<()> {
        self.previous_event_hash = envelope.event_hash.clone();
        self.next_sequence = self.next_sequence.checked_add(1).ok_or_else(|| {
            DomainError::EventSequence("offline projection sequence overflow".to_owned())
        })?;
        Ok(())
    }

    fn apply_unreconciled(
        &mut self,
        payload: PersistedPayload,
        envelope: &EventEnvelope,
        actor: &str,
    ) -> Result<bool> {
        reject_v2_legacy_execution_payload(self.version, &payload)?;
        payload.validate_shape()?;
        payload.validate_for_enclosing_run(&self.run_id)?;
        let (kind, id) = payload
            .unreconciled_kind_and_id()
            .map(|(kind, id)| (kind, id.clone()))
            .ok_or_else(|| {
                DomainError::Validation("offline shadow received a non-shadow payload".to_owned())
            })?;
        if self.unreconciled_records.contains_key(&id) || self.projected_findings.contains_key(&id)
        {
            return Err(DomainError::Validation(format!(
                "offline shadow record ID collision: {id}"
            )));
        }
        let mut candidate = self.shadow_candidate()?;
        apply(&mut candidate, &payload, actor, &self.run_id)?;
        candidate.validate()?;
        let metadata = UnreconciledRecordMetadata {
            kind,
            id: id.clone(),
            body_hash: ContentHash::sha256(&canonical_json(&payload)?),
            authority_reconciled: false,
        };
        self.unreconciled_records.insert(id.clone(), payload);
        self.unreconciled_metadata.insert(id.clone(), metadata);
        self.unreconciled_order.push(id.clone());
        self.advance_offline_cursor(envelope)?;
        Ok(false)
    }

    fn apply_projected_finding(
        &mut self,
        finding: Finding,
        envelope: &EventEnvelope,
        actor: &str,
    ) -> Result<bool> {
        let id = finding.id().clone();
        if self.unreconciled_records.contains_key(&id) || self.projected_findings.contains_key(&id)
        {
            return Err(DomainError::Validation(format!(
                "offline shadow record ID collision: {id}"
            )));
        }
        let payload = PersistedPayload::FindingRecorded(finding.clone());
        let mut candidate = self.shadow_candidate()?;
        apply(&mut candidate, &payload, actor, &self.run_id)?;
        candidate.validate()?;
        self.projected_finding_metadata.insert(
            id.clone(),
            ProjectedFindingMetadata {
                id: id.clone(),
                body_hash: ContentHash::sha256(&canonical_json(&payload)?),
            },
        );
        self.projected_findings.insert(id.clone(), finding);
        self.unreconciled_order.push(id);
        self.advance_offline_cursor(envelope)?;
        Ok(false)
    }

    fn apply_projected_context(
        &mut self,
        context: ReviewContextEnvelope,
        envelope: &EventEnvelope,
    ) -> Result<bool> {
        context.validate_for_event(&self.aggregate)?;
        let id = context.id().clone();
        if self.projected_context_metadata.contains_key(&id) {
            return Err(DomainError::IdCollision { id });
        }
        let bytes = context.canonical_bytes().map_err(context_domain_error)?;
        self.projected_context_metadata.insert(
            id.clone(),
            ProjectedContextEnvelopeMetadata {
                id: id.clone(),
                projection_hash: context.projection_hash().clone(),
                body_hash: ContentHash::sha256(&bytes),
            },
        );
        self.advance_offline_cursor(envelope)?;
        Ok(false)
    }

    fn shadow_candidate(&self) -> Result<ReviewAggregate> {
        let mut candidate = self.aggregate.clone();
        for id in &self.unreconciled_order {
            if let Some(payload) = self.unreconciled_records.get(id) {
                apply(&mut candidate, payload, payload.actor(), &self.run_id)?;
            } else if let Some(finding) = self.projected_findings.get(id) {
                apply(
                    &mut candidate,
                    &PersistedPayload::FindingRecorded(finding.clone()),
                    SYSTEM_ACTOR,
                    &self.run_id,
                )?;
            } else {
                return Err(DomainError::Validation(
                    "offline shadow order references a missing record".to_owned(),
                ));
            }
        }
        candidate.validate()?;
        Ok(candidate)
    }

    #[must_use]
    pub fn aggregate(&self) -> &ReviewAggregate {
        &self.aggregate
    }

    /// Returns metadata only; no unreconciled authority record is exposed as
    /// accepted aggregate state.
    #[must_use]
    pub fn unreconciled_records(&self) -> Vec<&UnreconciledRecordMetadata> {
        self.unreconciled_order
            .iter()
            .filter_map(|id| self.unreconciled_metadata.get(id))
            .collect()
    }

    /// Returns authority-free findings validated against the private shadow.
    #[must_use]
    pub fn projected_findings(&self) -> Vec<&ProjectedFindingMetadata> {
        self.unreconciled_order
            .iter()
            .filter_map(|id| self.projected_finding_metadata.get(id))
            .collect()
    }

    /// Returns metadata only. No offline replay path exposes a live context
    /// envelope or a context admission.
    #[must_use]
    pub fn projected_context_envelopes(&self) -> Vec<&ProjectedContextEnvelopeMetadata> {
        self.projected_context_metadata.values().collect()
    }

    /// Whether every event in the bound validated view has been applied.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        usize::try_from(self.next_sequence - 1)
            .is_ok_and(|count| count == self.expected_events.len())
    }

    /// The hash at the projection cursor; on completion this equals the
    /// validated view's tail event hash (or its chain-genesis hash when empty).
    #[must_use]
    pub fn tail_hash(&self) -> &ContentHash {
        &self.previous_event_hash
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEventEnvelope {
    schema: String,
    id: StableId,
    run_id: StableId,
    genesis_hash: ContentHash,
    sequence: u64,
    actor: String,
    logical_time: u64,
    payload: Box<RawValue>,
    payload_hash: ContentHash,
    previous_event_hash: ContentHash,
    event_hash: ContentHash,
}

impl<'de> Deserialize<'de> for EventEnvelope {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawEventEnvelope::deserialize(deserializer)?;
        let envelope = Self {
            schema: raw.schema,
            id: raw.id,
            run_id: raw.run_id,
            genesis_hash: raw.genesis_hash,
            sequence: raw.sequence,
            actor: raw.actor,
            logical_time: raw.logical_time,
            payload: raw.payload,
            payload_hash: raw.payload_hash,
            previous_event_hash: raw.previous_event_hash,
            event_hash: raw.event_hash,
        };
        envelope.validate().map_err(serde::de::Error::custom)?;
        Ok(envelope)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPayloadHeader<'a> {
    #[serde(rename = "type")]
    kind: String,
    #[serde(borrow)]
    data: &'a RawValue,
}

fn decode_payload(input: &str) -> Result<PersistedPayload> {
    let raw: RawPayloadHeader<'_> =
        serde_json::from_str(input).map_err(|error| DomainError::Json(error.to_string()))?;
    if matches!(
        raw.kind.as_str(),
        "review_plan_recorded" | "context_envelope_projected"
    ) {
        return match raw.kind.as_str() {
            "review_plan_recorded" => Ok(PersistedPayload::ReviewPlanRecorded(
                ReviewPlan::from_event_bytes(raw.data.get().as_bytes())?,
            )),
            "context_envelope_projected" => Ok(PersistedPayload::ContextEnvelopeProjected(
                ReviewContextEnvelope::from_event_bytes(raw.data.get().as_bytes())
                    .map_err(context_domain_error)?,
            )),
            _ => unreachable!("closed D1 kind checked"),
        };
    }
    let data: Value = serde_json::from_str(raw.data.get())
        .map_err(|error| DomainError::Json(error.to_string()))?;
    match raw.kind.as_str() {
        "obligation_transition" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct RawTransition {
                obligation_id: StableId,
                next: ObligationLifecycle,
            }
            let data: RawTransition = serde_json::from_value(data)
                .map_err(|error| DomainError::Json(error.to_string()))?;
            Ok(PersistedPayload::ObligationTransition {
                obligation_id: data.obligation_id,
                next: data.next,
            })
        }
        "claim_proposed" => Ok(PersistedPayload::ClaimProposed(
            ReviewClaim::from_event_value(data)?,
        )),
        "evidence_recorded" => Ok(PersistedPayload::EvidenceRecorded(Box::new(
            Evidence::from_event_value(data)?,
        ))),
        "evidence_bound" => Ok(PersistedPayload::EvidenceBound(
            EvidenceBinding::from_event_value(data)?,
        )),
        "verification_recorded" => Ok(PersistedPayload::VerificationRecorded(
            Verification::from_event_value(data)?,
        )),
        "decision_recorded" => Ok(PersistedPayload::DecisionRecorded(
            Decision::from_event_value(data)?,
        )),
        "finding_recorded" => Ok(PersistedPayload::FindingRecorded(
            Finding::from_event_value(data)?,
        )),
        "run_genesis_manifest" => Ok(PersistedPayload::RunGenesisManifest(
            serde_json::from_value(data).map_err(|error| DomainError::Json(error.to_string()))?,
        )),
        "artifact_registered" => Ok(PersistedPayload::ArtifactRegistered(
            serde_json::from_value(data).map_err(|error| DomainError::Json(error.to_string()))?,
        )),
        "snapshot_sources_recorded" => Ok(PersistedPayload::SnapshotSourcesRecorded(
            serde_json::from_value(data).map_err(|error| DomainError::Json(error.to_string()))?,
        )),
        _ => Err(DomainError::EventSequence(
            "unknown persisted event payload type".to_owned(),
        )),
    }
}

fn decode_canonical_payload(input: &str) -> Result<PersistedPayload> {
    let payload = decode_payload(input)?;
    let canonical_raw = input.as_bytes();
    let canonical_typed = payload_canonical_bytes(&payload)?;
    if canonical_raw != canonical_typed {
        return Err(DomainError::EventSequence(
            "persisted event payload must equal the canonical closed payload representation"
                .to_owned(),
        ));
    }
    Ok(payload)
}

/// A validated immutable event. Its payload remains private; consumers may
/// retain the envelope and replay it through the validation boundary.
#[derive(Clone, Debug)]
pub struct Event {
    envelope: EventEnvelope,
    evidence_admission: Option<EvidenceAdmission>,
    binding_admission: Option<EvidenceBindingAdmission>,
    verification_admission: Option<VerificationAdmission>,
    decision_admission: Option<DecisionAdmission>,
    context_projection_admission: Option<PositionedContextProjectionAdmission>,
}

impl Event {
    #[must_use]
    pub fn envelope(&self) -> &EventEnvelope {
        &self.envelope
    }

    /// Retained non-serializable admission for exact evidence replay.
    #[must_use]
    pub fn evidence_admission(&self) -> Option<&EvidenceAdmission> {
        self.evidence_admission.as_ref()
    }

    /// Retained non-serializable admission for exact binding replay.
    #[must_use]
    pub fn binding_admission(&self) -> Option<&EvidenceBindingAdmission> {
        self.binding_admission.as_ref()
    }

    /// Retained non-serializable admission for exact verification replay.
    #[must_use]
    pub fn verification_admission(&self) -> Option<&VerificationAdmission> {
        self.verification_admission.as_ref()
    }

    /// Retained non-serializable admission for exact decision replay.
    #[must_use]
    pub fn decision_admission(&self) -> Option<&DecisionAdmission> {
        self.decision_admission.as_ref()
    }
}

/// Append-only, in-memory event log that deterministically replays a prefix
/// only when it shares the canonical initial aggregate/genesis hash.
#[derive(Clone, Debug)]
pub struct EventLog {
    version: EventContractVersion,
    read_only: bool,
    run_id: StableId,
    genesis_hash: ContentHash,
    initial: ReviewAggregate,
    aggregate: ReviewAggregate,
    events: Vec<Event>,
    tail_hash: ContentHash,
}

impl EventLog {
    /// Starts an empty log from a validated initial aggregate.
    pub fn new(run_id: StableId, initial: ReviewAggregate) -> Result<Self> {
        Self::new_v2(run_id, initial)
    }

    /// Starts a newly minted v2 stream with its mandatory genesis manifest.
    pub fn new_v2(run_id: StableId, initial: ReviewAggregate) -> Result<Self> {
        let mut log = Self::new_with_version(EventContractVersion::V2, run_id, initial)?;
        log.append_v2_genesis_manifest()?;
        Ok(log)
    }

    /// Initializes the legacy hash boundary used only while importing a
    /// pre-existing v1 prefix. The returned log is read-only: v1 event minting
    /// is available only to crate-local test scaffolding. New callers must use
    /// [`Self::new_v2`].
    pub fn new_v1_for_import(run_id: StableId, initial: ReviewAggregate) -> Result<Self> {
        let mut log = Self::new_with_version(EventContractVersion::V1, run_id, initial)?;
        log.read_only = true;
        Ok(log)
    }

    /// Builds an editable v1 log exclusively for crate-local compatibility
    /// tests. This cannot be reached through the public library API.
    #[cfg(test)]
    pub(crate) fn new_v1_for_test(run_id: StableId, initial: ReviewAggregate) -> Result<Self> {
        Self::new_with_version(EventContractVersion::V1, run_id, initial)
    }

    fn new_with_version(
        version: EventContractVersion,
        run_id: StableId,
        initial: ReviewAggregate,
    ) -> Result<Self> {
        if run_id.kind() != "run" {
            return Err(DomainError::EventSequence(
                "event stream requires a run ID even when empty".to_owned(),
            ));
        }
        initial.validate()?;
        initial.validate_pristine_for_event_log()?;
        if version == EventContractVersion::V2 {
            let snapshot = RunGenesisSnapshot::from_aggregate(&initial)?;
            let bytes = snapshot.canonical_bytes()?;
            let rebuilt = RunGenesisSnapshot::from_canonical_bytes(&bytes)?.rebuild_aggregate()?;
            if canonical_json(&rebuilt)? != canonical_json(&initial)? {
                return Err(DomainError::Validation(
                    "v2 genesis snapshot does not reconstruct the supplied aggregate".to_owned(),
                ));
            }
        }
        let genesis_hash = match version {
            EventContractVersion::V1 => ContentHash::sha256(&canonical_json(&initial)?),
            EventContractVersion::V2 => {
                RunGenesisSnapshot::from_aggregate(&initial)?.canonical_hash()?
            }
        };
        let tail_hash = event_chain_genesis_hash(&run_id, &genesis_hash)?;
        Ok(Self {
            version,
            read_only: false,
            run_id,
            genesis_hash,
            aggregate: initial.clone(),
            initial,
            events: Vec::new(),
            tail_hash,
        })
    }

    /// Mints exact evidence authority only through a parsed ProgramSpace's
    /// snapshot admission and this log's run/genesis boundary. Stale evidence
    /// may still be retained for audit, but cannot later satisfy fresh sign-off.
    pub fn admit_evidence(
        &self,
        snapshot_admission: &EvidenceSnapshotAdmission,
        evidence: &Evidence,
    ) -> Result<EvidenceAdmission> {
        self.require_writable()?;
        if evidence.snapshot_id() == self.aggregate.program().snapshot_id()
            && evidence
                .target_ids()
                .iter()
                .any(|target| !self.aggregate.program().known_ids().contains(target))
        {
            return Err(DomainError::DanglingReference {
                owner: "evidence",
                owner_id: evidence.id().clone(),
                reference: evidence
                    .target_ids()
                    .iter()
                    .find(|target| !self.aggregate.program().known_ids().contains(*target))
                    .expect("target existence was checked")
                    .clone(),
            });
        }
        snapshot_admission.admit_for_event_log(
            self.run_id.clone(),
            self.genesis_hash.clone(),
            self.tail_hash.clone(),
            self.next_sequence()?,
            evidence,
        )
    }

    /// Mints exact authority for a binding after validating its current claim,
    /// evidence, target, property, context, and mode connection.
    pub fn admit_evidence_binding(
        &self,
        binding: &EvidenceBinding,
    ) -> Result<EvidenceBindingAdmission> {
        self.require_writable()?;
        self.aggregate.validate_binding_for_event(binding)?;
        EvidenceBindingAdmission::new(
            self.run_id.clone(),
            self.genesis_hash.clone(),
            self.tail_hash.clone(),
            self.next_sequence()?,
            binding,
        )
    }

    /// Mints exact authority for a canonical passed/failed verification after
    /// deriving freshness from the aggregate's evidence records.
    pub fn admit_verification(&self, verification: Verification) -> Result<VerificationAdmission> {
        self.require_writable()?;
        let verification = self.aggregate.normalize_verification(verification)?;
        self.aggregate
            .validate_verification_for_event(&verification)?;
        VerificationAdmission::new(
            self.run_id.clone(),
            self.genesis_hash.clone(),
            self.tail_hash.clone(),
            self.next_sequence()?,
            &verification,
        )
    }

    /// Mints an authenticated human admission bound to this exact current
    /// aggregate closure, obligation universe, and immutable genesis.
    pub fn admit_decision(
        &self,
        human: &TrustedHumanAdmission,
        decision: &Decision,
    ) -> Result<DecisionAdmission> {
        self.require_writable()?;
        let closure_digest = self
            .aggregate
            .decision_closure_digest(&self.genesis_hash, decision)?;
        human.admit_decision_for_event_log(
            self.stream_position()?,
            self.aggregate.universe().id().clone(),
            closure_digest,
            decision,
        )
    }

    /// Appends one locally admitted command atomically.
    pub fn append(&mut self, command: EventCommand) -> Result<&Event> {
        self.require_writable()?;
        let (payload, admission) = self.normalized_payload(command)?;
        let sequence = self.next_sequence()?;
        validate_stream_payload_position(self.version, sequence, &payload)?;
        reject_v2_legacy_execution_payload(self.version, &payload)?;
        reject_duplicate_d1_record(&self.aggregate, &payload)?;
        payload.validate_for_enclosing_run(&self.run_id)?;
        match (&payload, &admission) {
            (
                PersistedPayload::EvidenceRecorded(evidence),
                CommandAdmission::Evidence(capability),
            ) if capability.matches(
                &self.run_id,
                &self.genesis_hash,
                &self.tail_hash,
                sequence,
                evidence,
            ) => {}
            (PersistedPayload::EvidenceRecorded(_), _) => {
                return Err(DomainError::Validation(
                    "recording evidence requires an exact snapshot-bound run admission".to_owned(),
                ));
            }
            (PersistedPayload::EvidenceBound(binding), CommandAdmission::Binding(capability))
                if capability.matches(
                    &self.run_id,
                    &self.genesis_hash,
                    &self.tail_hash,
                    sequence,
                    binding,
                ) => {}
            (PersistedPayload::EvidenceBound(_), _) => {
                return Err(DomainError::Validation(
                    "recording an evidence binding requires an exact run/genesis-bound admission"
                        .to_owned(),
                ));
            }
            (
                PersistedPayload::VerificationRecorded(verification),
                CommandAdmission::Verification(capability),
            ) if capability.matches(
                &self.run_id,
                &self.genesis_hash,
                &self.tail_hash,
                sequence,
                verification,
            ) => {}
            (PersistedPayload::VerificationRecorded(_), _) => {
                return Err(DomainError::Validation(
                    "recording a verification requires an exact run/genesis-bound admission"
                        .to_owned(),
                ));
            }
            (
                PersistedPayload::DecisionRecorded(decision),
                CommandAdmission::Decision(capability),
            ) if self.decision_admission_matches(decision, capability)? => {}
            (PersistedPayload::DecisionRecorded(_), _) => {
                return Err(DomainError::Validation(
                    "recording a decision requires an exact run-bound trusted admission".to_owned(),
                ));
            }
            (
                PersistedPayload::ContextEnvelopeProjected(envelope),
                CommandAdmission::ContextProjection(capability),
            ) if capability.matches(envelope, &self.aggregate)? => {}
            (PersistedPayload::ContextEnvelopeProjected(_), _) => {
                return Err(DomainError::Validation(
                    "recording a context envelope requires a byte-reverified builder result"
                        .to_owned(),
                ));
            }
            _ => {}
        }
        let actor = payload.actor().to_owned();
        let envelope = EventEnvelope::new(
            self.version,
            self.run_id.clone(),
            self.genesis_hash.clone(),
            sequence,
            actor,
            sequence,
            self.tail_hash.clone(),
            payload,
        )?;
        let (
            evidence_admission,
            binding_admission,
            verification_admission,
            decision_admission,
            context_projection_admission,
        ) = match admission {
            CommandAdmission::Evidence(admission) => (Some(admission), None, None, None, None),
            CommandAdmission::Binding(admission) => (None, Some(admission), None, None, None),
            CommandAdmission::Verification(admission) => (None, None, Some(admission), None, None),
            CommandAdmission::Decision(admission) => (None, None, None, Some(admission), None),
            CommandAdmission::ContextProjection(admission) => {
                let payload = decode_canonical_payload(envelope.payload.get())?;
                let PersistedPayload::ContextEnvelopeProjected(context) = payload else {
                    return Err(DomainError::Validation(
                        "context admission cannot seal a non-context event".to_owned(),
                    ));
                };
                let positioned =
                    PositionedContextProjectionAdmission::seal(&envelope, &context, admission)?;
                (None, None, None, None, Some(positioned))
            }
            CommandAdmission::None => (None, None, None, None, None),
        };
        self.append_envelope(
            envelope,
            evidence_admission,
            binding_admission,
            verification_admission,
            decision_admission,
            context_projection_admission,
        )
    }

    /// Replays exact in-process events from the same initial aggregate.
    pub fn replay(
        version: EventContractVersion,
        run_id: StableId,
        initial: ReviewAggregate,
        events: &[Event],
    ) -> Result<Self> {
        let envelopes = events
            .iter()
            .map(|event| event.envelope.clone())
            .collect::<Vec<_>>();
        let evidence = events
            .iter()
            .filter_map(|event| event.evidence_admission.clone())
            .collect::<Vec<_>>();
        let bindings = events
            .iter()
            .filter_map(|event| event.binding_admission.clone())
            .collect::<Vec<_>>();
        let verifications = events
            .iter()
            .filter_map(|event| event.verification_admission.clone())
            .collect::<Vec<_>>();
        let decisions = events
            .iter()
            .filter_map(|event| event.decision_admission.clone())
            .collect::<Vec<_>>();
        let context_projections = events
            .iter()
            .filter_map(|event| event.context_projection_admission.clone())
            .collect::<Vec<_>>();
        Self::replay_envelopes(
            version,
            run_id,
            initial,
            &envelopes,
            &EventAdmissions::new(evidence, decisions)
                .with_trace_admissions(bindings, verifications)
                .with_context_admissions(context_projections),
        )
    }

    /// Replays JSON-imported envelopes only with matching exact host admissions.
    pub fn replay_envelopes(
        version: EventContractVersion,
        run_id: StableId,
        initial: ReviewAggregate,
        envelopes: &[EventEnvelope],
        admissions: &EventAdmissions,
    ) -> Result<Self> {
        let mut log = Self::new_with_version(version, run_id, initial)?;
        EventEnvelope::validate_sequence(version, &log.run_id, &log.genesis_hash, envelopes)?;
        if version == EventContractVersion::V2
            && let Some(first) = envelopes.first()
        {
            let bytes = RunGenesisSnapshot::from_aggregate(&log.initial)?.canonical_bytes()?;
            validate_v2_genesis_envelope(&log.run_id, &log.initial, &bytes, first)?;
        }
        for envelope in envelopes {
            let payload = decode_canonical_payload(envelope.payload.get())?;
            let admissions = log.admissions_for(envelope, &payload, admissions)?;
            log.append_envelope(
                envelope.clone(),
                admissions.evidence,
                admissions.binding,
                admissions.verification,
                admissions.decision,
                admissions.context_projection,
            )?;
        }
        if version == EventContractVersion::V1 {
            log.read_only = true;
        }
        Ok(log)
    }

    /// Continues a previously replayed prefix with the next imported suffix.
    /// The suffix must preserve this log's run and canonical genesis bindings.
    pub fn resume_envelopes(
        &mut self,
        envelopes: &[EventEnvelope],
        admissions: &EventAdmissions,
    ) -> Result<()> {
        self.require_writable()?;
        let mut next = self.clone();
        if next.version == EventContractVersion::V2
            && next.events.is_empty()
            && let Some(first) = envelopes.first()
        {
            let bytes = RunGenesisSnapshot::from_aggregate(&next.initial)?.canonical_bytes()?;
            validate_v2_genesis_envelope(&next.run_id, &next.initial, &bytes, first)?;
        }
        for envelope in envelopes {
            let payload = decode_canonical_payload(envelope.payload.get())?;
            let admissions = next.admissions_for(envelope, &payload, admissions)?;
            next.append_envelope(
                envelope.clone(),
                admissions.evidence,
                admissions.binding,
                admissions.verification,
                admissions.decision,
                admissions.context_projection,
            )?;
        }
        *self = next;
        Ok(())
    }

    fn append_envelope(
        &mut self,
        envelope: EventEnvelope,
        evidence_admission: Option<EvidenceAdmission>,
        binding_admission: Option<EvidenceBindingAdmission>,
        verification_admission: Option<VerificationAdmission>,
        decision_admission: Option<DecisionAdmission>,
        context_projection_admission: Option<PositionedContextProjectionAdmission>,
    ) -> Result<&Event> {
        self.require_writable()?;
        let expected_sequence = self.next_sequence()?;
        envelope.validate()?;
        if EventContractVersion::parse(&envelope.schema)? != self.version
            || envelope.run_id != self.run_id
            || envelope.genesis_hash != self.genesis_hash
            || envelope.sequence != expected_sequence
            || envelope.previous_event_hash != self.tail_hash
        {
            return Err(DomainError::EventSequence(
                "event run ID, genesis hash, and sequence must continue this log".to_owned(),
            ));
        }
        let payload = decode_canonical_payload(envelope.payload.get())?;
        validate_stream_payload_position(self.version, expected_sequence, &payload)?;
        reject_v2_legacy_execution_payload(self.version, &payload)?;
        reject_duplicate_d1_record(&self.aggregate, &payload)?;
        payload.validate_for_enclosing_run(&self.run_id)?;
        if let PersistedPayload::EvidenceRecorded(evidence) = &payload
            && !evidence_admission.as_ref().is_some_and(|admission| {
                admission.matches(
                    &self.run_id,
                    &self.genesis_hash,
                    &self.tail_hash,
                    expected_sequence,
                    evidence,
                )
            })
        {
            return Err(DomainError::Validation(
                "evidence event lacks an exact snapshot-bound run admission".to_owned(),
            ));
        }
        if let PersistedPayload::EvidenceBound(binding) = &payload
            && !binding_admission.as_ref().is_some_and(|admission| {
                admission.matches(
                    &self.run_id,
                    &self.genesis_hash,
                    &self.tail_hash,
                    expected_sequence,
                    binding,
                )
            })
        {
            return Err(DomainError::Validation(
                "evidence binding event lacks an exact run/genesis-bound admission".to_owned(),
            ));
        }
        if let PersistedPayload::VerificationRecorded(verification) = &payload
            && !verification_admission.as_ref().is_some_and(|admission| {
                admission.matches(
                    &self.run_id,
                    &self.genesis_hash,
                    &self.tail_hash,
                    expected_sequence,
                    verification,
                )
            })
        {
            return Err(DomainError::Validation(
                "verification event lacks an exact run/genesis-bound admission".to_owned(),
            ));
        }
        if let PersistedPayload::DecisionRecorded(decision) = &payload
            && !decision_admission.as_ref().is_some_and(|admission| {
                self.decision_admission_matches(decision, admission)
                    .is_ok_and(|matched| matched)
            })
        {
            return Err(DomainError::Validation(
                "decision event lacks an exact run-bound trusted admission".to_owned(),
            ));
        }
        if let PersistedPayload::ContextEnvelopeProjected(context) = &payload
            && !context_projection_admission
                .as_ref()
                .is_some_and(|admission| {
                    admission
                        .matches(&envelope, context, &self.aggregate)
                        .is_ok_and(|matched| matched)
                })
        {
            return Err(DomainError::Validation(
                "context event lacks its exact byte-reverified admission".to_owned(),
            ));
        }
        let mut next = self.aggregate.clone();
        apply(&mut next, &payload, envelope.actor(), &self.run_id)?;
        self.aggregate = next;
        self.events.push(Event {
            envelope,
            evidence_admission,
            binding_admission,
            verification_admission,
            decision_admission,
            context_projection_admission,
        });
        self.tail_hash = self
            .events
            .last()
            .map(|event| event.envelope.event_hash.clone())
            .ok_or_else(|| {
                DomainError::EventSequence("append did not retain the committed event".to_owned())
            })?;
        self.events.last().ok_or_else(|| {
            DomainError::EventSequence("append did not retain the committed event".to_owned())
        })
    }

    fn normalized_payload(
        &self,
        command: EventCommand,
    ) -> Result<(PersistedPayload, CommandAdmission)> {
        let payload = match command.payload {
            PersistedPayload::VerificationRecorded(verification) => {
                PersistedPayload::VerificationRecorded(
                    self.aggregate.normalize_verification(verification)?,
                )
            }
            payload => payload,
        };
        Ok((payload, command.admission))
    }

    fn require_writable(&self) -> Result<()> {
        if self.read_only {
            return Err(DomainError::Validation(
                "legacy v1 event logs are import/replay-only and cannot mint or append events"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn admissions_for(
        &self,
        event: &EventEnvelope,
        payload: &PersistedPayload,
        admissions: &EventAdmissions,
    ) -> Result<MatchedAdmissions> {
        let evidence = match payload {
            PersistedPayload::EvidenceRecorded(evidence) => admissions
                .evidence_for(
                    &self.run_id,
                    &self.genesis_hash,
                    &self.tail_hash,
                    self.next_sequence()?,
                    evidence,
                )
                .ok_or_else(|| {
                    DomainError::Validation(
                        "imported evidence lacks an exact snapshot-bound run admission".to_owned(),
                    )
                })
                .map(Some)?,
            _ => None,
        };
        let binding = match payload {
            PersistedPayload::EvidenceBound(binding) => admissions
                .binding_for(
                    &self.run_id,
                    &self.genesis_hash,
                    &self.tail_hash,
                    self.next_sequence()?,
                    binding,
                )
                .ok_or_else(|| {
                    DomainError::Validation(
                        "imported evidence binding lacks an exact run/genesis-bound admission"
                            .to_owned(),
                    )
                })
                .map(Some)?,
            _ => None,
        };
        let verification = match payload {
            PersistedPayload::VerificationRecorded(verification) => admissions
                .verification_for(
                    &self.run_id,
                    &self.genesis_hash,
                    &self.tail_hash,
                    self.next_sequence()?,
                    verification,
                )
                .ok_or_else(|| {
                    DomainError::Validation(
                        "imported verification lacks an exact run/genesis-bound admission"
                            .to_owned(),
                    )
                })
                .map(Some)?,
            _ => None,
        };
        let decision = match payload {
            PersistedPayload::DecisionRecorded(decision) => {
                let closure = self
                    .aggregate
                    .decision_closure_digest(&self.genesis_hash, decision)?;
                admissions
                    .decision_for(
                        &self.stream_position()?,
                        self.aggregate.universe().id(),
                        &closure,
                        decision,
                    )
                    .ok_or_else(|| {
                        DomainError::Validation(
                            "imported decision lacks an exact run/genesis/closure-bound trusted admission"
                                .to_owned(),
                        )
                    })
                    .map(Some)?
            }
            _ => None,
        };
        let context_projection = match payload {
            PersistedPayload::ContextEnvelopeProjected(context) => admissions
                .context_projection_for(event, context, &self.aggregate)?
                .ok_or_else(|| {
                    DomainError::Validation(
                        "imported context envelope lacks an exact byte-reverified admission"
                            .to_owned(),
                    )
                })
                .map(Some)?,
            _ => None,
        };
        Ok(MatchedAdmissions {
            evidence,
            binding,
            verification,
            decision,
            context_projection,
        })
    }

    fn decision_admission_matches(
        &self,
        decision: &Decision,
        admission: &DecisionAdmission,
    ) -> Result<bool> {
        let closure = self
            .aggregate
            .decision_closure_digest(&self.genesis_hash, decision)?;
        Ok(decision.matches_decision_admission(
            &self.stream_position()?,
            self.aggregate.universe().id(),
            &closure,
            admission,
        ))
    }

    #[must_use]
    pub fn aggregate(&self) -> &ReviewAggregate {
        &self.aggregate
    }

    #[must_use]
    pub fn initial(&self) -> &ReviewAggregate {
        &self.initial
    }

    /// The canonical typed baseline committed by this v2 stream.
    pub fn run_genesis_snapshot(&self) -> Result<RunGenesisSnapshot> {
        if self.version != EventContractVersion::V2 {
            return Err(DomainError::Validation(
                "legacy v1 streams do not carry a typed run genesis snapshot".to_owned(),
            ));
        }
        RunGenesisSnapshot::from_aggregate(&self.initial)
    }

    /// Stable run binding required when minting exact replay admissions.
    #[must_use]
    pub fn run_id(&self) -> &StableId {
        &self.run_id
    }

    /// The one schema contract governing this entire stream.
    #[must_use]
    pub const fn event_contract_version(&self) -> EventContractVersion {
        self.version
    }

    #[must_use]
    pub fn genesis_hash(&self) -> &ContentHash {
        &self.genesis_hash
    }

    /// Current chain tip; an empty log uses the deterministic genesis sentinel.
    #[must_use]
    pub fn tail_hash(&self) -> &ContentHash {
        &self.tail_hash
    }

    #[must_use]
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    #[must_use]
    pub fn envelopes(&self) -> impl ExactSizeIterator<Item = &EventEnvelope> {
        self.events.iter().map(Event::envelope)
    }

    fn next_sequence(&self) -> Result<u64> {
        u64::try_from(self.events.len())
            .map_err(|_| DomainError::EventSequence("event count does not fit u64".to_owned()))?
            .checked_add(1)
            .ok_or_else(|| DomainError::EventSequence("event count does not fit u64".to_owned()))
    }

    fn stream_position(&self) -> Result<StreamPosition> {
        Ok(StreamPosition {
            run_id: self.run_id.clone(),
            genesis_hash: self.genesis_hash.clone(),
            tail_hash: self.tail_hash.clone(),
            sequence: self.next_sequence()?,
        })
    }

    fn append_v2_genesis_manifest(&mut self) -> Result<()> {
        let snapshot = RunGenesisSnapshot::from_aggregate(&self.initial)?;
        let bytes = snapshot.canonical_bytes()?;
        let source = ArtifactSource::RunGenesis {
            run_id: self.run_id.clone(),
        };
        let registration_id = ArtifactRegistered::derived_id(
            &self.run_id,
            &self.genesis_hash,
            "application/json",
            ArtifactSensitivity::CanonicalState,
            &source,
        )?;
        let registration = ArtifactRegistered::new(
            self.run_id.clone(),
            registration_id,
            self.genesis_hash.clone(),
            "application/json",
            u64::try_from(bytes.len()).map_err(|_| {
                DomainError::EventSequence("genesis bytes do not fit u64".to_owned())
            })?,
            ArtifactSensitivity::CanonicalState,
            source,
        )?;
        let manifest = RunGenesisManifest::new(
            self.run_id.clone(),
            registration,
            self.initial.program().repository_identity(),
            self.initial.program().snapshot_id().clone(),
            self.initial.program().profile_id(),
            self.initial.program().profile_version(),
        )?;
        let envelope = EventEnvelope::new(
            EventContractVersion::V2,
            self.run_id.clone(),
            self.genesis_hash.clone(),
            1,
            SYSTEM_ACTOR,
            1,
            self.tail_hash.clone(),
            PersistedPayload::RunGenesisManifest(manifest),
        )?;
        self.append_envelope(envelope, None, None, None, None, None)?;
        let envelope = self
            .events
            .first()
            .ok_or_else(|| {
                DomainError::EventSequence("missing appended v2 genesis event".to_owned())
            })?
            .envelope();
        validate_v2_genesis_envelope(&self.run_id, &self.initial, &bytes, envelope)?;
        Ok(())
    }
}

// The event identity intentionally binds every envelope component, including
// its selected schema, so legacy v1 and minted v2 IDs cannot alias.
#[allow(clippy::too_many_arguments)]
fn event_id(
    schema: &str,
    run_id: &StableId,
    genesis_hash: &ContentHash,
    sequence: u64,
    actor: &str,
    logical_time: u64,
    payload_hash: &ContentHash,
    previous_event_hash: &ContentHash,
) -> Result<StableId> {
    let bindings = BTreeMap::from([
        ("actor".to_owned(), Value::String(actor.to_owned())),
        (
            "genesis_hash".to_owned(),
            Value::String(genesis_hash.to_string()),
        ),
        (
            "logical_time".to_owned(),
            Value::Number(serde_json::Number::from(logical_time)),
        ),
        (
            "payload_hash".to_owned(),
            Value::String(payload_hash.to_string()),
        ),
        (
            "previous_event_hash".to_owned(),
            Value::String(previous_event_hash.to_string()),
        ),
        ("run".to_owned(), Value::String(run_id.to_string())),
        ("schema".to_owned(), Value::String(schema.to_owned())),
        (
            "sequence".to_owned(),
            Value::Number(serde_json::Number::from(sequence)),
        ),
    ]);
    StableId::derived("event", &bindings)
}

struct EventHashInput<'a> {
    schema: &'a str,
    id: &'a StableId,
    run_id: &'a StableId,
    genesis_hash: &'a ContentHash,
    sequence: u64,
    actor: &'a str,
    logical_time: u64,
    payload_hash: &'a ContentHash,
    previous_event_hash: &'a ContentHash,
}

fn envelope_hash(input: EventHashInput<'_>) -> Result<ContentHash> {
    let bindings = BTreeMap::from([
        ("actor".to_owned(), Value::String(input.actor.to_owned())),
        ("event_id".to_owned(), Value::String(input.id.to_string())),
        (
            "genesis_hash".to_owned(),
            Value::String(input.genesis_hash.to_string()),
        ),
        (
            "logical_time".to_owned(),
            Value::Number(serde_json::Number::from(input.logical_time)),
        ),
        (
            "payload_hash".to_owned(),
            Value::String(input.payload_hash.to_string()),
        ),
        (
            "previous_event_hash".to_owned(),
            Value::String(input.previous_event_hash.to_string()),
        ),
        ("run".to_owned(), Value::String(input.run_id.to_string())),
        ("schema".to_owned(), Value::String(input.schema.to_owned())),
        (
            "sequence".to_owned(),
            Value::Number(serde_json::Number::from(input.sequence)),
        ),
    ]);
    Ok(ContentHash::sha256(&canonical_json(&bindings)?))
}

fn event_chain_genesis_hash(run_id: &StableId, genesis_hash: &ContentHash) -> Result<ContentHash> {
    let sentinel = BTreeMap::from([
        (
            "domain".to_owned(),
            Value::String("reviewgraphen.event_chain_genesis.v1".to_owned()),
        ),
        (
            "genesis_hash".to_owned(),
            Value::String(genesis_hash.to_string()),
        ),
        ("run".to_owned(), Value::String(run_id.to_string())),
    ]);
    Ok(ContentHash::sha256(&canonical_json(&sentinel)?))
}

fn apply(
    aggregate: &mut ReviewAggregate,
    payload: &PersistedPayload,
    actor: &str,
    expected_run_id: &StableId,
) -> Result<()> {
    match payload {
        PersistedPayload::ObligationTransition {
            obligation_id,
            next,
        } => aggregate.transition_obligation(obligation_id, *next),
        PersistedPayload::ClaimProposed(claim) => aggregate.add_claim(claim.clone()),
        PersistedPayload::EvidenceRecorded(evidence) => {
            aggregate.add_evidence(evidence.as_ref().clone())
        }
        PersistedPayload::EvidenceBound(binding) => aggregate.bind_evidence(binding.clone()),
        PersistedPayload::VerificationRecorded(verification) => {
            aggregate.add_verification(verification.clone())
        }
        PersistedPayload::DecisionRecorded(decision) => {
            aggregate.record_decision(decision.clone(), actor)
        }
        PersistedPayload::FindingRecorded(finding) => aggregate.add_finding(finding.clone()),
        PersistedPayload::RunGenesisManifest(manifest) => {
            aggregate.record_genesis_manifest(expected_run_id, manifest.clone())
        }
        PersistedPayload::ArtifactRegistered(registration) => {
            aggregate.register_artifact(expected_run_id, registration.clone())
        }
        PersistedPayload::SnapshotSourcesRecorded(sources) => {
            aggregate.record_snapshot_sources(sources.clone())
        }
        PersistedPayload::ReviewPlanRecorded(plan) => aggregate.record_review_plan(plan.clone()),
        PersistedPayload::ContextEnvelopeProjected(envelope) => {
            aggregate.record_context_envelope(envelope.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ClaimPolarity, DecisionOutcome, EvidenceDetails, EvidenceRelation, FindingStatus,
        FindingTrace, MvpRulePack, PlanBudget, ProgramSpace, Provenance, SourceRef,
        VerificationOutcome, canonical_json, plan,
    };
    use std::collections::{BTreeMap, BTreeSet};

    const FIXTURE: &[u8] =
        include_bytes!("../../../examples/double-submit-payment/program-space.json");

    fn id(value: &str) -> StableId {
        StableId::parse(value).expect("test identifier")
    }

    fn aggregate() -> ReviewAggregate {
        let program = ProgramSpace::from_json_slice(FIXTURE).expect("fixture program");
        let (universe, obligations) = MvpRulePack::synthesize(&program)
            .expect("fixture synthesis")
            .into_parts();
        ReviewAggregate::new(program, universe, obligations).expect("fixture aggregate")
    }

    fn forged_envelope(
        version: EventContractVersion,
        run_id: StableId,
        genesis_hash: ContentHash,
        sequence: u64,
        previous_event_hash: ContentHash,
        payload: PersistedPayload,
    ) -> EventEnvelope {
        let actor = payload.actor().to_owned();
        let payload_bytes = payload_canonical_bytes(&payload).expect("canonical payload JSON");
        let payload_hash = ContentHash::sha256(&payload_bytes);
        let payload = raw_payload(payload_bytes).expect("raw payload JSON");
        let id = event_id(
            version.schema(),
            &run_id,
            &genesis_hash,
            sequence,
            &actor,
            sequence,
            &payload_hash,
            &previous_event_hash,
        )
        .expect("event ID");
        let event_hash = envelope_hash(EventHashInput {
            schema: version.schema(),
            id: &id,
            run_id: &run_id,
            genesis_hash: &genesis_hash,
            sequence,
            actor: &actor,
            logical_time: sequence,
            payload_hash: &payload_hash,
            previous_event_hash: &previous_event_hash,
        })
        .expect("event hash");
        EventEnvelope {
            schema: version.schema().to_owned(),
            id,
            run_id,
            genesis_hash,
            sequence,
            actor,
            logical_time: sequence,
            payload,
            payload_hash,
            previous_event_hash,
            event_hash,
        }
    }

    fn v2_log(run: &str) -> EventLog {
        EventLog::new(id(run), aggregate()).expect("v2 log")
    }

    #[test]
    fn d1_raw_decode_is_strict_and_duplicate_plan_replay_is_atomic() {
        let initial = aggregate();
        let run_id = id("run:d1-duplicate-plan");
        let mut log = EventLog::new(run_id.clone(), initial.clone()).unwrap();
        let review_plan = plan(log.aggregate(), PlanBudget::new(16, 2).unwrap()).unwrap();

        let mut plan_text = String::from_utf8(review_plan.canonical_bytes().unwrap()).unwrap();
        assert_eq!(plan_text.pop(), Some('}'));
        let unknown_payload = format!(
            "{{\"data\":{plan_text},\"unknown\":null}},\"type\":\"review_plan_recorded\"}}"
        );
        assert!(decode_payload(&unknown_payload).is_err());
        let duplicate_header = format!(
            "{{\"data\":{}}},\"type\":\"review_plan_recorded\",\"type\":\"review_plan_recorded\"}}",
            review_plan
                .canonical_bytes()
                .map(String::from_utf8)
                .unwrap()
                .unwrap()
        );
        assert!(decode_payload(&duplicate_header).is_err());
        let oversized_context = format!(
            "{{\"data\":{{\"padding\":\"{}\"}},\"type\":\"context_envelope_projected\"}}",
            "x".repeat(786_432)
        );
        assert!(matches!(
            decode_payload(&oversized_context),
            Err(DomainError::Incomplete {
                operation: "context envelope canonical bytes",
                limit: 786_432,
                observed,
            }) if observed > 786_432
        ));

        log.append(EventCommand::review_plan_recorded(review_plan.clone()))
            .unwrap();
        let duplicate = EventEnvelope::new(
            EventContractVersion::V2,
            run_id.clone(),
            log.genesis_hash().clone(),
            log.next_sequence().unwrap(),
            SYSTEM_ACTOR,
            log.next_sequence().unwrap(),
            log.tail_hash().clone(),
            PersistedPayload::ReviewPlanRecorded(review_plan),
        )
        .unwrap();
        let mut envelopes = log.envelopes().cloned().collect::<Vec<_>>();
        envelopes.push(duplicate.clone());

        assert!(matches!(
            EventLog::replay_envelopes(
                EventContractVersion::V2,
                run_id.clone(),
                initial.clone(),
                &envelopes,
                &EventAdmissions::default(),
            ),
            Err(DomainError::IdCollision { .. })
        ));

        let mut resumed = EventLog::replay_envelopes(
            EventContractVersion::V2,
            run_id.clone(),
            initial.clone(),
            &envelopes[..envelopes.len() - 1],
            &EventAdmissions::default(),
        )
        .unwrap();
        let resumed_tail = resumed.tail_hash().clone();
        let resumed_count = resumed.events().len();
        assert!(matches!(
            resumed.resume_envelopes(&[duplicate], &EventAdmissions::default()),
            Err(DomainError::IdCollision { .. })
        ));
        assert_eq!(resumed.tail_hash(), &resumed_tail);
        assert_eq!(resumed.events().len(), resumed_count);
        assert_eq!(resumed.aggregate().review_plans().count(), 1);

        let genesis = RunGenesisSnapshot::from_aggregate(&initial)
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let view = EventEnvelope::validated_view(
            EventContractVersion::V2,
            &run_id,
            EventStreamGenesis::V2(&genesis),
            &envelopes,
        )
        .unwrap();
        let mut offline = OfflineProjectionState::new(&view, initial).unwrap();
        for event in &view.events()[..view.events().len() - 1] {
            offline.apply(event).unwrap();
        }
        let offline_tail = offline.tail_hash().clone();
        assert!(matches!(
            offline.apply(view.events().last().unwrap()),
            Err(DomainError::IdCollision { .. })
        ));
        assert_eq!(offline.tail_hash(), &offline_tail);
        assert_eq!(offline.aggregate().review_plans().count(), 1);
        assert!(!offline.is_complete());
    }

    #[test]
    fn positioned_context_admission_rejects_every_event_position_splice() {
        let log = v2_log("run:context-position");
        let event = log.envelopes().next().unwrap().clone();
        let admission = PositionedContextProjectionAdmission {
            run_id: event.run_id.clone(),
            genesis_hash: event.genesis_hash.clone(),
            previous_event_hash: event.previous_event_hash.clone(),
            sequence: event.sequence,
            event_id: event.id.clone(),
            projection: ContextProjectionAdmission {
                manifest_digest: ContentHash::sha256(b"position-test-manifest"),
                envelope_id: id("context-envelope:position-test"),
                projection_hash: ContentHash::sha256(b"position-test-projection"),
                sources: Vec::new(),
            },
        };
        assert!(admission.matches_position(&event));

        let mut wrong_run = event.clone();
        wrong_run.run_id = id("run:other-context-position");
        assert!(!admission.matches_position(&wrong_run));
        let mut wrong_genesis = event.clone();
        wrong_genesis.genesis_hash = ContentHash::sha256(b"other genesis");
        assert!(!admission.matches_position(&wrong_genesis));
        let mut wrong_tail = event.clone();
        wrong_tail.previous_event_hash = ContentHash::sha256(b"other tail");
        assert!(!admission.matches_position(&wrong_tail));
        let mut wrong_sequence = event.clone();
        wrong_sequence.sequence += 1;
        assert!(!admission.matches_position(&wrong_sequence));
        let mut wrong_event = event;
        wrong_event.id = id("event:other-context-position");
        assert!(!admission.matches_position(&wrong_event));
    }

    #[test]
    fn d1_outer_jsonl_writer_accepts_exact_mebibyte_and_refuses_plus_one() {
        let mut exact = BoundedEventJson::new(MAX_D1_EVENT_LINE_BYTES - 1);
        exact
            .push(&vec![b'x'; MAX_D1_EVENT_LINE_BYTES - 1])
            .unwrap();
        let exact = exact.finish().unwrap();
        assert_eq!(exact.len() + 1, MAX_D1_EVENT_LINE_BYTES);

        let mut over = BoundedEventJson::new(MAX_D1_EVENT_LINE_BYTES - 1);
        assert!(matches!(
            over.push(&vec![b'x'; MAX_D1_EVENT_LINE_BYTES]),
            Err(DomainError::Incomplete {
                operation: "D1 canonical event JSONL",
                limit: MAX_D1_EVENT_LINE_BYTES,
                observed,
            }) if observed == MAX_D1_EVENT_LINE_BYTES + 1
        ));
    }

    fn planned_payload(log: &EventLog) -> PersistedPayload {
        PersistedPayload::ObligationTransition {
            obligation_id: log
                .aggregate()
                .obligations()
                .next()
                .expect("obligation")
                .id()
                .clone(),
            next: ObligationLifecycle::Planned,
        }
    }

    #[test]
    fn v2_manifest_is_required_once_at_sequence_one_and_schema_cannot_mix() {
        let log = v2_log("run:v2-manifest-contract");
        let manifest = log.events[0].envelope.clone();
        let missing = forged_envelope(
            EventContractVersion::V2,
            log.run_id.clone(),
            log.genesis_hash.clone(),
            1,
            manifest.previous_event_hash.clone(),
            planned_payload(&log),
        );
        assert!(missing.validate().is_err(), "v2 sequence one needs genesis");
        assert!(
            EventEnvelope::validate_sequence(
                EventContractVersion::V2,
                &log.run_id,
                &log.genesis_hash,
                &[missing],
            )
            .is_err()
        );

        let manifest_payload = decode_canonical_payload(manifest.payload.get()).unwrap();
        let duplicate = forged_envelope(
            EventContractVersion::V2,
            log.run_id.clone(),
            log.genesis_hash.clone(),
            2,
            manifest.event_hash.clone(),
            manifest_payload,
        );
        assert!(
            duplicate.validate().is_err(),
            "manifest cannot be non-first"
        );
        assert!(
            EventEnvelope::validate_sequence(
                EventContractVersion::V2,
                &log.run_id,
                &log.genesis_hash,
                &[manifest.clone(), duplicate],
            )
            .is_err()
        );

        let mixed = forged_envelope(
            EventContractVersion::V1,
            log.run_id.clone(),
            log.genesis_hash.clone(),
            2,
            manifest.event_hash.clone(),
            planned_payload(&log),
        );
        assert!(mixed.validate().is_ok());
        assert!(
            EventEnvelope::validate_sequence(
                EventContractVersion::V2,
                &log.run_id,
                &log.genesis_hash,
                &[manifest, mixed],
            )
            .is_err()
        );
    }

    #[test]
    fn verified_genesis_bytes_reject_tampered_tuple_and_noncanonical_obligations() {
        let log = v2_log("run:verified-genesis");
        let snapshot = log.run_genesis_snapshot().unwrap();
        let (expected_universe, mut expected_obligations) =
            MvpRulePack::synthesize(snapshot.program_space())
                .unwrap()
                .into_parts();
        expected_obligations.sort_by(|left, right| left.id().cmp(right.id()));
        assert_eq!(snapshot.universe(), &expected_universe);
        assert_eq!(snapshot.obligations(), expected_obligations.as_slice());
        let bytes = snapshot.canonical_bytes().unwrap();
        let manifest = match decode_canonical_payload(log.events[0].envelope.payload.get()).unwrap()
        {
            PersistedPayload::RunGenesisManifest(value) => value,
            _ => unreachable!("v2 first event is manifest"),
        };
        for pointer in [
            "/run_id",
            "/repository_identity",
            "/snapshot_id",
            "/profile_id",
            "/profile_version",
            "/event_contract_version",
            "/genesis_artifact/cas_hash",
            "/genesis_artifact/media_type",
            "/genesis_artifact/size",
            "/genesis_artifact/sensitivity",
            "/genesis_artifact/registration_id",
            "/genesis_artifact/source/run_id",
        ] {
            let mut value = serde_json::to_value(&manifest).unwrap();
            let replacement = match pointer {
                "/run_id" | "/genesis_artifact/source/run_id" => {
                    Value::String("run:other".to_owned())
                }
                "/repository_identity" | "/profile_id" | "/profile_version" => {
                    Value::String("other".to_owned())
                }
                "/snapshot_id" => Value::String("snapshot:other".to_owned()),
                "/event_contract_version" => {
                    Value::String("reviewgraphen.review_event.v1".to_owned())
                }
                "/genesis_artifact/cas_hash" => {
                    Value::String(ContentHash::sha256(b"other").to_string())
                }
                "/genesis_artifact/media_type" => Value::String("text/plain".to_owned()),
                "/genesis_artifact/size" => Value::Number(serde_json::Number::from(0)),
                "/genesis_artifact/sensitivity" => Value::String("sensitive".to_owned()),
                "/genesis_artifact/registration_id" => {
                    Value::String("registration:other".to_owned())
                }
                _ => unreachable!(),
            };
            *value.pointer_mut(pointer).unwrap() = replacement;
            let tampered: RunGenesisManifest = serde_json::from_value(value).unwrap();
            assert!(
                tampered
                    .validate_against_genesis(log.run_id(), &snapshot, &bytes)
                    .is_err()
            );
        }

        let mut unknown = serde_json::from_slice::<Value>(&bytes).unwrap();
        unknown["unknown"] = Value::Bool(true);
        assert!(
            RunGenesisSnapshot::from_canonical_bytes(&canonical_json(&unknown).unwrap()).is_err()
        );
        let mut misordered = serde_json::from_slice::<Value>(&bytes).unwrap();
        misordered["obligations"].as_array_mut().unwrap().swap(0, 1);
        assert!(
            RunGenesisSnapshot::from_canonical_bytes(&canonical_json(&misordered).unwrap())
                .is_err()
        );
        let mut duplicate = serde_json::from_slice::<Value>(&bytes).unwrap();
        let first = duplicate["obligations"][0].clone();
        duplicate["obligations"].as_array_mut().unwrap().push(first);
        assert!(
            RunGenesisSnapshot::from_canonical_bytes(&canonical_json(&duplicate).unwrap()).is_err()
        );
        let mut malformed = serde_json::from_slice::<Value>(&bytes).unwrap();
        malformed["obligations"][0]["id"] = Value::String("not-an-id".to_owned());
        assert!(
            RunGenesisSnapshot::from_canonical_bytes(&canonical_json(&malformed).unwrap()).is_err()
        );
        let mut noncanonical = bytes.clone();
        noncanonical.insert(0, b' ');
        assert!(RunGenesisSnapshot::from_canonical_bytes(&noncanonical).is_err());
    }

    #[test]
    fn source_record_shapes_and_artifact_source_pairs_fail_closed() {
        let cas = ContentHash::sha256(b"source");
        assert!(
            SnapshotSourceRecordEntry::new(
                id("file:one"),
                "src/one.rs",
                cas.clone(),
                id("registration:one"),
                cas.clone(),
                0,
            )
            .is_err()
        );
        assert!(
            SnapshotSourceRecordEntry::new(
                id("file:one"),
                "src/one.rs",
                cas.clone(),
                id("registration:one"),
                ContentHash::sha256(b"other"),
                1,
            )
            .is_err()
        );
        let wrong_source = ArtifactSource::SnapshotIngest {
            run_id: id("run:fixture"),
            snapshot_id: id("snapshot:fixture"),
            adapter_id: "adapter".to_owned(),
        };
        let registration_id = ArtifactRegistered::derived_id(
            &id("run:fixture"),
            &cas,
            "text/plain",
            ArtifactSensitivity::CanonicalState,
            &wrong_source,
        )
        .unwrap();
        assert!(
            ArtifactRegistered::new(
                id("run:fixture"),
                registration_id,
                cas,
                "text/plain",
                6,
                ArtifactSensitivity::CanonicalState,
                wrong_source,
            )
            .is_err()
        );
    }

    #[test]
    fn v1_event_hash_remains_a_historical_golden() {
        let mut log = EventLog::new_v1_for_test(id("run:v1-golden"), aggregate()).unwrap();
        let obligation = log.aggregate().obligations().next().unwrap().id().clone();
        log.append(EventCommand::obligation_transition(
            obligation,
            ObligationLifecycle::Planned,
        ))
        .unwrap();
        let envelope = log.events[0].envelope();
        assert_eq!(envelope.schema, EventContractVersion::V1.schema());
        assert_eq!(
            envelope.event_hash.to_string(),
            "sha256:088674a66c49436fe9b654ea3d47a434b511c31a635357d947bafb87f41bb021"
        );
    }

    #[test]
    fn snapshot_source_records_require_exact_registered_snapshot_files_and_are_idempotent() {
        let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
        let bytes_by_path = BTreeMap::from([
            ("src/checkout_controller.rs", b"checkout\n".to_vec()),
            ("src/payment_repository.rs", b"repository\n".to_vec()),
        ]);
        for artifact in input["artifacts"].as_array_mut().unwrap() {
            if artifact["kind"] == "file" {
                let path = artifact["location"]["path"].as_str().unwrap();
                artifact["content_hash"] = Value::String(
                    ContentHash::sha256(bytes_by_path.get(path).unwrap()).to_string(),
                );
            }
        }
        let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let mut aggregate = ReviewAggregate::new(program, universe, obligations).unwrap();
        let snapshot_id = aggregate.program().snapshot_id().clone();
        let mut entries = Vec::new();
        let file_artifacts = aggregate
            .program()
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
            .cloned()
            .collect::<Vec<_>>();
        for artifact in file_artifacts {
            let path = artifact.location.as_ref().unwrap().path.clone();
            let hash = artifact.content_hash.clone().unwrap();
            let source = ArtifactSource::SnapshotIngest {
                run_id: id("run:source-records"),
                snapshot_id: snapshot_id.clone(),
                adapter_id: "fixture-adapter".to_owned(),
            };
            let registration = ArtifactRegistered::new(
                id("run:source-records"),
                ArtifactRegistered::derived_id(
                    &id("run:source-records"),
                    &hash,
                    "text/plain",
                    ArtifactSensitivity::WorkspaceSource,
                    &source,
                )
                .unwrap(),
                hash.clone(),
                "text/plain",
                1,
                ArtifactSensitivity::WorkspaceSource,
                source,
            )
            .unwrap();
            let registration_id = registration.registration_id().clone();
            aggregate
                .register_artifact(&id("run:source-records"), registration)
                .unwrap();
            entries.push(
                SnapshotSourceRecordEntry::new(
                    artifact.id,
                    path,
                    hash.clone(),
                    registration_id,
                    hash,
                    1,
                )
                .unwrap(),
            );
        }
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        let sources = SnapshotSourcesRecorded::new(snapshot_id.clone(), entries.clone()).unwrap();
        aggregate.record_snapshot_sources(sources.clone()).unwrap();
        aggregate.record_snapshot_sources(sources).unwrap();

        let mut changed_line = entries.clone();
        let changed = changed_line[0].clone();
        changed_line[0] = SnapshotSourceRecordEntry::new(
            changed.artifact_id().clone(),
            changed.path(),
            changed.content_hash().clone(),
            changed.registration_id().clone(),
            changed.cas_hash().clone(),
            2,
        )
        .unwrap();
        assert!(matches!(
            aggregate.record_snapshot_sources(
                SnapshotSourcesRecorded::new(snapshot_id.clone(), changed_line).unwrap()
            ),
            Err(DomainError::IdCollision { .. })
        ));

        let mut missing = entries.clone();
        missing.pop();
        assert!(
            aggregate
                .record_snapshot_sources(
                    SnapshotSourcesRecorded::new(snapshot_id.clone(), missing).unwrap()
                )
                .is_err()
        );
        let mut extra = entries.clone();
        extra.push(
            SnapshotSourceRecordEntry::new(
                id("file:extra"),
                "src/z.rs",
                ContentHash::sha256(b"z"),
                entries[0].registration_id().clone(),
                ContentHash::sha256(b"z"),
                1,
            )
            .unwrap(),
        );
        assert!(
            aggregate
                .record_snapshot_sources(
                    SnapshotSourcesRecorded::new(snapshot_id.clone(), extra).unwrap()
                )
                .is_err()
        );
        let mut duplicate_path = entries.clone();
        duplicate_path.push(entries[0].clone());
        assert!(SnapshotSourcesRecorded::new(snapshot_id.clone(), duplicate_path).is_err());

        let dangling = SnapshotSourceRecordEntry::new(
            entries[0].artifact_id().clone(),
            entries[0].path(),
            entries[0].content_hash().clone(),
            id("registration:missing"),
            entries[0].cas_hash().clone(),
            1,
        )
        .unwrap();
        let mut dangling_entries = entries.clone();
        dangling_entries[0] = dangling;
        assert!(
            aggregate
                .record_snapshot_sources(
                    SnapshotSourcesRecorded::new(snapshot_id, dangling_entries).unwrap()
                )
                .is_err()
        );
    }

    #[test]
    fn offline_v2_rejects_legacy_claims_and_completed_transitions() {
        let log = v2_log("run:offline-v2-legacy");
        let manifest = log.events[0].envelope.clone();
        let obligation = log.aggregate().obligations().next().unwrap().id().clone();
        let claim = ReviewClaim::propose_ai(
            id("claim:offline-v2-legacy"),
            id("execution:fixture"),
            BTreeSet::from([obligation.clone()]),
            crate::ClaimPolarity::IssuePresent,
            "v2 must reject legacy claim records",
            BTreeSet::from([id("function:checkout-submit")]),
            None,
        )
        .unwrap();
        let claim_event = forged_envelope(
            EventContractVersion::V2,
            log.run_id.clone(),
            log.genesis_hash.clone(),
            2,
            manifest.event_hash.clone(),
            PersistedPayload::ClaimProposed(claim),
        );
        let claim_envelopes = [manifest.clone(), claim_event];
        let genesis_bytes = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let claim_view = EventEnvelope::validated_view(
            EventContractVersion::V2,
            &log.run_id,
            EventStreamGenesis::V2(&genesis_bytes),
            &claim_envelopes,
        )
        .expect("structurally valid view");
        let mut claim_projection = OfflineProjectionState::new(&claim_view, aggregate()).unwrap();
        assert!(claim_projection.apply(&claim_view.events()[0]).unwrap());
        assert!(claim_projection.apply(&claim_view.events()[1]).is_err());

        let completed_event = forged_envelope(
            EventContractVersion::V2,
            log.run_id.clone(),
            log.genesis_hash.clone(),
            2,
            manifest.event_hash.clone(),
            PersistedPayload::ObligationTransition {
                obligation_id: obligation,
                next: ObligationLifecycle::Completed,
            },
        );
        let completed_envelopes = [manifest, completed_event];
        let completed_view = EventEnvelope::validated_view(
            EventContractVersion::V2,
            &log.run_id,
            EventStreamGenesis::V2(&genesis_bytes),
            &completed_envelopes,
        )
        .expect("structurally valid view");
        let mut completed_projection =
            OfflineProjectionState::new(&completed_view, aggregate()).unwrap();
        assert!(
            completed_projection
                .apply(&completed_view.events()[0])
                .unwrap()
        );
        assert!(
            completed_projection
                .apply(&completed_view.events()[1])
                .is_err()
        );
    }

    #[test]
    fn artifact_registration_must_bind_the_enclosing_event_run() {
        let log = v2_log("run:artifact-owner-a");
        let manifest = log.events[0].envelope.clone();
        let source = ArtifactSource::ReviewerExecution {
            run_id: id("run:artifact-owner-b"),
            execution_id: id("execution:fixture"),
            reviewer_id: "fixture-reviewer".to_owned(),
        };
        let cas = ContentHash::sha256(b"foreign artifact");
        let foreign = ArtifactRegistered::new(
            id("run:artifact-owner-b"),
            ArtifactRegistered::derived_id(
                &id("run:artifact-owner-b"),
                &cas,
                "text/plain",
                ArtifactSensitivity::Sensitive,
                &source,
            )
            .unwrap(),
            cas,
            "text/plain",
            16,
            ArtifactSensitivity::Sensitive,
            source,
        )
        .unwrap();
        let foreign_event = forged_envelope(
            EventContractVersion::V2,
            log.run_id.clone(),
            log.genesis_hash.clone(),
            2,
            manifest.event_hash.clone(),
            PersistedPayload::ArtifactRegistered(foreign),
        );
        let envelopes = [manifest, foreign_event];
        let bytes = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let view = EventEnvelope::validated_view(
            EventContractVersion::V2,
            log.run_id(),
            EventStreamGenesis::V2(&bytes),
            &envelopes,
        )
        .unwrap();
        assert!(
            EventLog::replay_envelopes(
                EventContractVersion::V2,
                log.run_id.clone(),
                aggregate(),
                &envelopes,
                &EventAdmissions::default(),
            )
            .is_err()
        );
        let mut projection = OfflineProjectionState::new(&view, aggregate()).unwrap();
        assert!(projection.apply(&view.events()[0]).unwrap());
        assert!(projection.apply(&view.events()[1]).is_err());
        assert_eq!(projection.tail_hash(), &envelopes[0].event_hash);
        assert!(!projection.is_complete());
    }

    #[test]
    fn offline_projection_is_bound_to_the_exact_validated_view_events() {
        let mut log = EventLog::new_v1_for_test(id("run:offline-exact-view"), aggregate()).unwrap();
        let obligation = log.aggregate().obligations().next().unwrap().id().clone();
        log.append(EventCommand::obligation_transition(
            obligation.clone(),
            ObligationLifecycle::Planned,
        ))
        .unwrap();
        let expected_envelopes = log.envelopes().cloned().collect::<Vec<_>>();
        let expected_view = EventEnvelope::validated_view(
            EventContractVersion::V1,
            log.run_id(),
            EventStreamGenesis::V1(log.genesis_hash()),
            &expected_envelopes,
        )
        .unwrap();
        let fork = forged_envelope(
            EventContractVersion::V1,
            log.run_id.clone(),
            log.genesis_hash.clone(),
            1,
            expected_envelopes[0].previous_event_hash.clone(),
            PersistedPayload::ObligationTransition {
                obligation_id: obligation,
                next: ObligationLifecycle::InProgress,
            },
        );
        let fork_envelopes = [fork];
        let fork_view = EventEnvelope::validated_view(
            EventContractVersion::V1,
            log.run_id(),
            EventStreamGenesis::V1(log.genesis_hash()),
            &fork_envelopes,
        )
        .unwrap();
        let mut projection = OfflineProjectionState::new(&expected_view, aggregate()).unwrap();
        assert!(projection.apply(&fork_view.events()[0]).is_err());
        assert_eq!(
            projection.tail_hash(),
            &expected_envelopes[0].previous_event_hash
        );
        assert!(!projection.is_complete());
        assert!(projection.apply(&expected_view.events()[0]).unwrap());
        assert!(projection.is_complete());
        assert_eq!(projection.tail_hash(), &expected_envelopes[0].event_hash);
        assert!(projection.apply(&expected_view.events()[0]).is_err());
    }

    #[test]
    fn offline_authority_metadata_must_validate_before_it_is_unreconciled() {
        let initial = aggregate();
        let run_id = id("run:offline-dangling-authority");
        let legacy =
            EventLog::new_with_version(EventContractVersion::V1, run_id.clone(), initial.clone())
                .unwrap();
        let source = SourceRef::new(
            "tool",
            "fixture-verifier@1",
            Some("1".to_owned()),
            None,
            None,
        )
        .unwrap();
        let provenance = Provenance::accepted_deterministic(
            source,
            "fixture.verifier.v1",
            Some("1".to_owned()),
            Some(1.0),
        )
        .unwrap();
        let evidence = Evidence::new(
            id("evidence:offline-dangling"),
            "static_fact",
            BTreeSet::from([id("function:missing")]),
            EvidenceDetails::new(None, None, BTreeMap::new()),
            provenance.clone(),
            initial.program().evidence_snapshot_admission(),
        )
        .unwrap();
        let envelope = forged_envelope(
            EventContractVersion::V1,
            run_id,
            legacy.genesis_hash.clone(),
            1,
            legacy.tail_hash.clone(),
            PersistedPayload::EvidenceRecorded(Box::new(evidence)),
        );
        let envelopes = [envelope];
        let view = EventEnvelope::validated_view(
            EventContractVersion::V1,
            legacy.run_id(),
            EventStreamGenesis::V1(legacy.genesis_hash()),
            &envelopes,
        )
        .expect("shape-valid envelope view");
        let mut projection = OfflineProjectionState::new(&view, initial.clone()).unwrap();
        assert!(projection.apply(&view.events()[0]).is_err());
        assert_eq!(projection.aggregate().claims().count(), 0);

        let accepted_target = Evidence::new(
            id("evidence:offline-shadow"),
            "static_fact",
            BTreeSet::from([id("function:checkout-submit")]),
            EvidenceDetails::new(None, None, BTreeMap::new()),
            provenance,
            projection
                .aggregate()
                .program()
                .evidence_snapshot_admission(),
        )
        .unwrap();
        let accepted_envelope = forged_envelope(
            EventContractVersion::V1,
            legacy.run_id.clone(),
            legacy.genesis_hash.clone(),
            1,
            legacy.tail_hash.clone(),
            PersistedPayload::EvidenceRecorded(Box::new(accepted_target)),
        );
        let accepted_envelopes = [accepted_envelope];
        let accepted_view = EventEnvelope::validated_view(
            EventContractVersion::V1,
            legacy.run_id(),
            EventStreamGenesis::V1(legacy.genesis_hash()),
            &accepted_envelopes,
        )
        .unwrap();
        let mut accepted_projection = OfflineProjectionState::new(&accepted_view, initial).unwrap();
        assert!(
            !accepted_projection
                .apply(&accepted_view.events()[0])
                .unwrap()
        );
        let metadata = accepted_projection.unreconciled_records();
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].kind(), UnreconciledRecordKind::Evidence);
        assert_eq!(metadata[0].id(), &id("evidence:offline-shadow"));
        assert!(!metadata[0].authority_reconciled());
        assert_eq!(accepted_projection.aggregate().claims().count(), 0);
    }

    #[test]
    fn offline_shadow_replays_the_complete_authority_chain_without_accepting_it() {
        let initial = aggregate();
        let observed = initial.program().clone();
        let mut log =
            EventLog::new_v1_for_test(id("run:offline-shadow-chain"), initial.clone()).unwrap();
        let obligation = log
            .aggregate()
            .obligations()
            .find(|item| item.target_kind() == "node")
            .unwrap()
            .id()
            .clone();
        let claim = ReviewClaim::propose_ai(
            id("claim:offline-shadow-chain"),
            id("execution:fixture"),
            BTreeSet::from([obligation]),
            ClaimPolarity::IssuePresent,
            "offline shadow chain",
            BTreeSet::from([id("state:checkout-loading")]),
            Some(1.0),
        )
        .unwrap();
        let claim_id = claim.id().clone();
        log.append(EventCommand::claim_proposed(claim)).unwrap();
        let evidence = Evidence::new(
            id("evidence:offline-shadow-chain"),
            "static_fact",
            BTreeSet::from([id("function:checkout-submit")]),
            EvidenceDetails::new(None, None, BTreeMap::new()),
            Provenance::accepted_deterministic(
                SourceRef::new(
                    "tool",
                    "fixture-verifier@1",
                    Some("1".to_owned()),
                    None,
                    None,
                )
                .unwrap(),
                "fixture.verifier.v1",
                Some("1".to_owned()),
                Some(1.0),
            )
            .unwrap(),
            observed.evidence_snapshot_admission(),
        )
        .unwrap();
        let evidence_id = evidence.id().clone();
        let evidence_admission = log
            .admit_evidence(&observed.evidence_snapshot_admission(), &evidence)
            .unwrap();
        log.append(EventCommand::evidence_recorded(
            evidence,
            evidence_admission,
        ))
        .unwrap();
        let binding = EvidenceBinding::new(
            id("binding:offline-shadow-chain"),
            claim_id.clone(),
            evidence_id.clone(),
            EvidenceRelation::Reproduces,
            BTreeMap::from([(
                "property_id".to_owned(),
                "async.concurrent_reentry".to_owned(),
            )]),
        )
        .unwrap();
        let binding_admission = log.admit_evidence_binding(&binding).unwrap();
        log.append(EventCommand::evidence_bound(binding, binding_admission))
            .unwrap();
        let verification = Verification::new(
            id("verification:offline-shadow-chain"),
            claim_id.clone(),
            VerificationOutcome::Passed,
            "fixture-verifier@1",
            BTreeSet::from([evidence_id.clone()]),
        )
        .unwrap();
        let verification_id = verification.id().clone();
        let verification_admission = log.admit_verification(verification.clone()).unwrap();
        log.append(EventCommand::verification_recorded(
            verification,
            verification_admission,
        ))
        .unwrap();
        let human =
            TrustedHumanAdmission::from_trusted_host("human:reviewer", "reviewer:fixture").unwrap();
        let decision = Decision::human(
            id("decision:offline-shadow-chain"),
            claim_id.clone(),
            DecisionOutcome::Accept,
            human.clone(),
            "joined trace is accepted",
            BTreeSet::from([
                claim_id.clone(),
                evidence_id.clone(),
                verification_id.clone(),
            ]),
        )
        .unwrap();
        let decision_admission = log.admit_decision(&human, &decision).unwrap();
        log.append(EventCommand::decision_recorded(
            decision,
            decision_admission,
        ))
        .unwrap();
        log.append(EventCommand::finding_recorded(Finding::new(
            id("finding:offline-shadow-chain"),
            claim_id.clone(),
            FindingStatus::Accepted,
            FindingTrace::new(
                BTreeSet::from([evidence_id.clone()]),
                BTreeSet::from([verification_id.clone()]),
                Some(id("decision:offline-shadow-chain")),
                BTreeSet::from([id("state:checkout-loading")]),
            ),
        )))
        .unwrap();

        let envelopes = log.envelopes().cloned().collect::<Vec<_>>();
        let view = EventEnvelope::validated_view(
            EventContractVersion::V1,
            log.run_id(),
            EventStreamGenesis::V1(log.genesis_hash()),
            &envelopes,
        )
        .unwrap();
        let mut projection = OfflineProjectionState::new(&view, initial.clone()).unwrap();
        assert!(projection.apply(&view.events()[0]).unwrap());
        for event in &view.events()[1..] {
            assert!(!projection.apply(event).unwrap());
        }
        let metadata = projection.unreconciled_records();
        assert_eq!(
            metadata.iter().map(|item| item.kind()).collect::<Vec<_>>(),
            vec![
                UnreconciledRecordKind::Evidence,
                UnreconciledRecordKind::EvidenceBinding,
                UnreconciledRecordKind::Verification,
                UnreconciledRecordKind::Decision,
            ]
        );
        assert_eq!(
            metadata
                .iter()
                .map(|item| item.id().clone())
                .collect::<Vec<_>>(),
            vec![
                id("evidence:offline-shadow-chain"),
                id("binding:offline-shadow-chain"),
                id("verification:offline-shadow-chain"),
                id("decision:offline-shadow-chain"),
            ]
        );
        for (item, event) in metadata.iter().zip(&view.events()[1..5]) {
            assert_eq!(item.body_hash(), &event.envelope().payload_hash);
            assert!(!item.authority_reconciled());
        }
        let findings = projection.projected_findings();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id(), &id("finding:offline-shadow-chain"));
        assert_eq!(
            findings[0].body_hash(),
            &view.events()[5].envelope().payload_hash
        );
        assert_eq!(projection.aggregate().claims().count(), 1);
        assert_eq!(projection.aggregate().bindings().count(), 0);
        assert_eq!(projection.aggregate().verifications().count(), 0);
        assert_eq!(projection.aggregate().decisions().count(), 0);
        assert_eq!(projection.aggregate().findings().count(), 0);

        let claim_envelope = envelopes[0].clone();
        let bad_payloads = vec![
            PersistedPayload::EvidenceBound(
                EvidenceBinding::new(
                    id("binding:offline-dangling"),
                    claim_id.clone(),
                    id("evidence:missing"),
                    EvidenceRelation::Supports,
                    BTreeMap::from([(
                        "property_id".to_owned(),
                        "async.concurrent_reentry".to_owned(),
                    )]),
                )
                .unwrap(),
            ),
            PersistedPayload::VerificationRecorded(
                Verification::new(
                    id("verification:offline-dangling"),
                    claim_id.clone(),
                    VerificationOutcome::Passed,
                    "fixture-verifier@1",
                    BTreeSet::from([id("evidence:missing")]),
                )
                .unwrap(),
            ),
            PersistedPayload::DecisionRecorded(
                Decision::human(
                    id("decision:offline-dangling"),
                    claim_id.clone(),
                    DecisionOutcome::Accept,
                    human,
                    "missing joined trace",
                    BTreeSet::from([claim_id.clone(), id("evidence:missing")]),
                )
                .unwrap(),
            ),
            PersistedPayload::FindingRecorded(Finding::new(
                id("finding:offline-dangling"),
                claim_id,
                FindingStatus::Accepted,
                FindingTrace::new(
                    BTreeSet::from([id("evidence:missing")]),
                    BTreeSet::from([id("verification:missing")]),
                    Some(id("decision:missing")),
                    BTreeSet::from([id("state:checkout-loading")]),
                ),
            )),
        ];
        for payload in bad_payloads {
            let invalid = forged_envelope(
                EventContractVersion::V1,
                log.run_id.clone(),
                log.genesis_hash.clone(),
                2,
                claim_envelope.event_hash.clone(),
                payload,
            );
            let invalid_envelopes = [claim_envelope.clone(), invalid];
            let invalid_view = EventEnvelope::validated_view(
                EventContractVersion::V1,
                log.run_id(),
                EventStreamGenesis::V1(log.genesis_hash()),
                &invalid_envelopes,
            )
            .unwrap();
            let mut invalid_projection =
                OfflineProjectionState::new(&invalid_view, initial.clone()).unwrap();
            assert!(invalid_projection.apply(&invalid_view.events()[0]).unwrap());
            assert!(invalid_projection.apply(&invalid_view.events()[1]).is_err());
            assert!(invalid_projection.unreconciled_records().is_empty());
            assert_eq!(invalid_projection.aggregate().claims().count(), 1);
            assert_eq!(invalid_projection.tail_hash(), &claim_envelope.event_hash);
            assert!(!invalid_projection.is_complete());
        }
    }
}
