use crate::context::{ContextProjectionAdmission, context_domain_error};
use crate::execution::{
    MAX_D2_WORKING_BYTES, ReviewExecutionRecorded, ReviewerRawClosure, preflight_d2_decode_working,
};
use crate::{
    BuiltContextProjection, ContentHash, Decision, DecisionAdmission, DomainError, Evidence,
    EvidenceAdmission, EvidenceBinding, EvidenceSnapshotAdmission, ExecutionClaimV2,
    ExecutionRecord, Finding, LegacyClaimV1, MvpRulePack, Obligation, ObligationLifecycle,
    ProgramSpace, Result, ReviewAggregate, ReviewClaim, ReviewContextEnvelope, ReviewPlan,
    StableId, TrustedHumanAdmission, UniverseDescriptor, ValidatedExecutionBundle, Verification,
    canonical_json, canonical_json_value,
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
    /// Compact certificate created only by strict canonical-genesis decode.
    /// Durable readers use it to avoid rebuilding the full baseline merely
    /// to re-check the sequence-one manifest.
    V2Verified(&'a VerifiedV2Genesis),
}

const SYSTEM_ACTOR: &str = "reviewgraphen-core@1";
const RUN_GENESIS_SCHEMA: &str = "reviewgraphen.run_genesis.v1";
const MAX_D1_EVENT_LINE_BYTES: usize = 1_048_576;
const MAX_EVENT_JSON_DEPTH: usize = 128;
const MAX_EVENT_JSON_VALUES: usize = 65_536;
const MAX_GENESIS_JSON_DEPTH: usize = 128;
const MAX_GENESIS_JSON_VALUES: usize = 1_048_576;

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

/// Immutable, compact witness that canonical V2 genesis bytes were strictly
/// decoded and rebuilt through the pristine aggregate boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedV2Genesis {
    run_id: StableId,
    genesis_hash: ContentHash,
    byte_len: u64,
    repository_identity: String,
    snapshot_id: StableId,
    profile_id: String,
    profile_version: String,
    pristine_aggregate_hash: ContentHash,
}

impl VerifiedV2Genesis {
    /// Creates a certificate only after the complete canonical genesis and
    /// deterministic baseline reconstruction have both succeeded.
    pub fn from_canonical_bytes(run_id: &StableId, input: &[u8]) -> Result<Self> {
        let snapshot = RunGenesisSnapshot::from_canonical_bytes(input)?;
        let aggregate = snapshot.rebuild_aggregate()?;
        Ok(Self {
            run_id: run_id.clone(),
            genesis_hash: ContentHash::sha256(input),
            byte_len: u64::try_from(input.len())
                .map_err(|_| DomainError::Validation("genesis bytes do not fit u64".to_owned()))?,
            repository_identity: snapshot.program_space.repository_identity().to_owned(),
            snapshot_id: snapshot.program_space.snapshot_id().clone(),
            profile_id: snapshot.program_space.profile_id().to_owned(),
            profile_version: snapshot.program_space.profile_version().to_owned(),
            pristine_aggregate_hash: ContentHash::sha256(&canonical_json(&aggregate)?),
        })
    }

    #[must_use]
    pub fn run_id(&self) -> &StableId {
        &self.run_id
    }

    #[must_use]
    pub fn genesis_hash(&self) -> &ContentHash {
        &self.genesis_hash
    }

    /// Requested bytes retained by the compact verified-genesis certificate,
    /// including its fixed Arc payload layout and backing-string capacities.
    #[must_use]
    pub fn allocated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            .saturating_add(self.run_id.allocated_bytes())
            .saturating_add(self.genesis_hash.allocated_bytes())
            .saturating_add(self.repository_identity.capacity())
            .saturating_add(self.snapshot_id.allocated_bytes())
            .saturating_add(self.profile_id.capacity())
            .saturating_add(self.profile_version.capacity())
            .saturating_add(self.pristine_aggregate_hash.allocated_bytes())
    }

    /// Confirms that a reconstructed pristine aggregate is the one certified
    /// by these canonical genesis bytes without retaining those bytes again.
    pub fn validate_pristine_aggregate(&self, aggregate: &ReviewAggregate) -> Result<()> {
        aggregate.validate_pristine_for_event_log()?;
        if ContentHash::sha256(&canonical_json(aggregate)?) != self.pristine_aggregate_hash {
            return Err(DomainError::Validation(
                "offline v2 projection initial aggregate does not match verified genesis bytes"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn validate_manifest(&self, run_id: &StableId, manifest: &RunGenesisManifest) -> Result<()> {
        manifest.validate()?;
        if &self.run_id != run_id
            || manifest.run_id != *run_id
            || manifest.genesis_artifact.run_id != *run_id
            || manifest.genesis_artifact.cas_hash != self.genesis_hash
            || manifest.genesis_artifact.size != self.byte_len
            || manifest.repository_identity != self.repository_identity
            || manifest.snapshot_id != self.snapshot_id
            || manifest.profile_id != self.profile_id
            || manifest.profile_version != self.profile_version
            || !matches!(
                &manifest.genesis_artifact.source,
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

    /// Index-only bounded decode seam. The structural cap is operational and
    /// deliberately does not alter the canonical genesis schema.
    pub fn from_canonical_bytes_for_index(input: &[u8]) -> Result<Self> {
        preflight_genesis_json_structure(input)?;
        Self::from_canonical_bytes(input)
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
    const MAX_D2_REGISTRATION_IDENTITY_BYTES: usize = 4_096;

    pub(crate) fn allocated_bytes(&self) -> usize {
        let source = match &self.source {
            ArtifactSource::RunGenesis { run_id } => run_id.allocated_bytes(),
            ArtifactSource::SnapshotIngest {
                run_id,
                snapshot_id,
                adapter_id,
            } => run_id
                .allocated_bytes()
                .saturating_add(snapshot_id.allocated_bytes())
                .saturating_add(adapter_id.capacity()),
            ArtifactSource::ReviewerExecution {
                run_id,
                execution_id,
                reviewer_id,
            } => run_id
                .allocated_bytes()
                .saturating_add(execution_id.allocated_bytes())
                .saturating_add(reviewer_id.capacity()),
        };
        self.run_id
            .allocated_bytes()
            .saturating_add(self.registration_id.allocated_bytes())
            .saturating_add(self.cas_hash.allocated_bytes())
            .saturating_add(self.media_type.capacity())
            .saturating_add(source)
    }
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

    /// Constructs the exact sensitive registration required before a D2
    /// reviewer execution event can be admitted.
    pub fn reviewer_execution(
        run_id: StableId,
        execution_id: StableId,
        reviewer_id: impl Into<String>,
        cas_hash: ContentHash,
        media_type: impl Into<String>,
        size: u64,
    ) -> Result<Self> {
        let reviewer_id = reviewer_id.into();
        let media_type = media_type.into();
        let source = ArtifactSource::ReviewerExecution {
            run_id: run_id.clone(),
            execution_id,
            reviewer_id,
        };
        let registration_id = Self::derived_id(
            &run_id,
            &cas_hash,
            &media_type,
            ArtifactSensitivity::Sensitive,
            &source,
        )?;
        Self::new(
            run_id,
            registration_id,
            cas_hash,
            media_type,
            size,
            ArtifactSensitivity::Sensitive,
            source,
        )
    }

    pub(crate) fn derived_id(
        run_id: &StableId,
        cas_hash: &ContentHash,
        media_type: &str,
        sensitivity: ArtifactSensitivity,
        source: &ArtifactSource,
    ) -> Result<StableId> {
        if let ArtifactSource::ReviewerExecution {
            run_id: source_run_id,
            execution_id,
            reviewer_id,
        } = source
        {
            #[derive(Serialize)]
            struct ReviewerSourceIdentity<'a> {
                execution_id: &'a StableId,
                kind: &'static str,
                reviewer_id: &'a str,
                run_id: &'a StableId,
            }

            #[derive(Serialize)]
            struct ReviewerRegistrationIdentity<'a> {
                cas_hash: &'a ContentHash,
                media_type: &'a str,
                run_id: &'a StableId,
                sensitivity: &'static str,
                source: ReviewerSourceIdentity<'a>,
            }

            if media_type.len() > 256 || reviewer_id.len() > 256 {
                return Err(DomainError::Incomplete {
                    operation: "D2 registration identity string bytes",
                    limit: 256,
                    observed: media_type.len().max(reviewer_id.len()),
                });
            }
            let identity = ReviewerRegistrationIdentity {
                cas_hash,
                media_type,
                run_id,
                sensitivity: match sensitivity {
                    ArtifactSensitivity::CanonicalState => "canonical_state",
                    ArtifactSensitivity::WorkspaceSource => "workspace_source",
                    ArtifactSensitivity::Sensitive => "sensitive",
                },
                source: ReviewerSourceIdentity {
                    execution_id,
                    kind: "reviewer_execution",
                    reviewer_id,
                    run_id: source_run_id,
                },
            };
            let bytes = crate::execution::bounded_json(
                &identity,
                Self::MAX_D2_REGISTRATION_IDENTITY_BYTES,
                "D2 registration identity",
            )?;
            return StableId::parse(format!("registration:{}", ContentHash::sha256(&bytes)));
        }
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
    fn allocated_bytes(&self) -> usize {
        self.run_id
            .allocated_bytes()
            .saturating_add(self.event_contract_version.capacity())
            .saturating_add(self.genesis_artifact.allocated_bytes())
            .saturating_add(self.repository_identity.capacity())
            .saturating_add(self.snapshot_id.allocated_bytes())
            .saturating_add(self.profile_id.capacity())
            .saturating_add(self.profile_version.capacity())
    }
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
    fn allocated_bytes(&self) -> usize {
        self.snapshot_id
            .allocated_bytes()
            .saturating_add(
                self.entries
                    .capacity()
                    .saturating_mul(std::mem::size_of::<SnapshotSourceRecordEntry>()),
            )
            .saturating_add(self.entries.iter().fold(0_usize, |total, entry| {
                total
                    .saturating_add(entry.artifact_id.allocated_bytes())
                    .saturating_add(entry.path.capacity())
                    .saturating_add(entry.content_hash.allocated_bytes())
                    .saturating_add(entry.registration_id.allocated_bytes())
                    .saturating_add(entry.cas_hash.allocated_bytes())
            }))
    }
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
    ReviewerRaw(ReviewerRawClosure),
}

#[derive(Default)]
struct MatchedAdmissions {
    evidence: Option<EvidenceAdmission>,
    binding: Option<EvidenceBindingAdmission>,
    verification: Option<VerificationAdmission>,
    decision: Option<DecisionAdmission>,
    context_projection: Option<PositionedContextProjectionAdmission>,
    reviewer_raw: Option<ReviewerRawClosure>,
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
    reviewer_raw: Vec<ReviewerRawClosure>,
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
            reviewer_raw: Vec::new(),
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

    /// Adds byte-derived, authority-free closure for imported D2 execution
    /// events. The bytes are consumed only to recheck hash and size; no
    /// reviewer capability or acceptance authority is minted.
    pub fn with_reviewer_artifacts(
        mut self,
        artifacts: Vec<(EventEnvelope, Vec<u8>)>,
    ) -> Result<Self> {
        for (event, raw_bytes) in artifacts {
            if raw_bytes.len() > crate::execution::MAX_D2_RAW_REVIEWER_BYTES {
                return Err(DomainError::Incomplete {
                    operation: "D2 raw reviewer bytes",
                    limit: crate::execution::MAX_D2_RAW_REVIEWER_BYTES,
                    observed: raw_bytes.len(),
                });
            }
            let payload = decode_canonical_payload(event.payload.get())?;
            let PersistedPayload::ReviewExecutionRecorded(recorded) = payload else {
                return Err(DomainError::Validation(
                    "reviewer artifact bytes must be paired with a D2 execution event".to_owned(),
                ));
            };
            let closure = ReviewerRawClosure::from_bytes(&recorded, &raw_bytes)?;
            self.reviewer_raw.push(closure);
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

    fn with_reviewer_raw(mut self, reviewer_raw: Vec<ReviewerRawClosure>) -> Self {
        self.reviewer_raw = reviewer_raw;
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

    fn reviewer_raw_for(&self, recorded: &ReviewExecutionRecorded) -> Option<ReviewerRawClosure> {
        self.reviewer_raw
            .iter()
            .find(|closure| closure.matches(recorded))
            .cloned()
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
    #[cfg(test)]
    pub(crate) fn claim_proposed(claim: ReviewClaim) -> Self {
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

    /// Records one validated fake/no-tools D2 execution and all of its claims
    /// in the only atomic v2 payload that can introduce D2 claim state.
    #[must_use]
    pub fn review_execution_recorded(bundle: ValidatedExecutionBundle) -> Self {
        let (recorded, raw_closure) = bundle.into_parts();
        Self {
            payload: PersistedPayload::ReviewExecutionRecorded(recorded),
            admission: CommandAdmission::ReviewerRaw(raw_closure),
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
    ClaimProposed(LegacyClaimV1),
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
    ReviewExecutionRecorded(ReviewExecutionRecorded),
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
            Self::ReviewExecutionRecorded(value) => Some((
                UnreconciledRecordKind::ReviewExecution,
                value.execution.id(),
            )),
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
                | Self::ReviewExecutionRecorded(_)
        )
    }

    fn is_d1_bounded(&self) -> bool {
        matches!(
            self,
            Self::ReviewPlanRecorded(_)
                | Self::ContextEnvelopeProjected(_)
                | Self::ReviewExecutionRecorded(_)
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
            Self::ReviewExecutionRecorded(recorded) => recorded.validate_shape(),
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

#[derive(Clone, Copy)]
enum JsonFrame {
    Array(JsonArrayState),
    Object(JsonObjectState),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum JsonArrayState {
    ValueOrEnd,
    CommaOrEnd,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum JsonObjectState {
    KeyOrEnd,
    Colon,
    Value,
    CommaOrEnd,
}

/// Performs the fixed structural admission required for every persisted event
/// JSON document. The scanner intentionally does not allocate or decide
/// duplicate-key semantics; serde and the closed typed DTOs retain those
/// responsibilities after this resource check succeeds.
pub(crate) fn preflight_event_json_structure(input: &[u8]) -> Result<()> {
    preflight_json_structure(
        input,
        "index event JSON structure",
        MAX_EVENT_JSON_DEPTH,
        MAX_EVENT_JSON_VALUES,
    )
}

pub fn preflight_index_genesis_json_structure(input: &[u8]) -> Result<()> {
    preflight_json_structure(
        input,
        "index genesis JSON structure",
        MAX_GENESIS_JSON_DEPTH,
        MAX_GENESIS_JSON_VALUES,
    )
}

fn preflight_genesis_json_structure(input: &[u8]) -> Result<()> {
    preflight_index_genesis_json_structure(input)
}

/// Counts every object member and array element without constructing a JSON
/// value tree. The fixed 128-entry stack is sufficient for both contracts, so
/// successful scans retain no owned allocation.
fn preflight_json_structure(
    input: &[u8],
    operation: &'static str,
    max_depth: usize,
    max_values: usize,
) -> Result<()> {
    let mut stack = [JsonFrame::Array(JsonArrayState::ValueOrEnd); MAX_EVENT_JSON_DEPTH];
    let mut stack_len = 0_usize;
    let mut values = 0_usize;
    let mut root_started = false;
    let mut root_complete = false;
    let mut index = 0_usize;

    while index < input.len() {
        if input[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if root_complete {
            return invalid_json_structure();
        }

        match input[index] {
            b'{' | b'[' => {
                accept_json_value(
                    &mut stack,
                    stack_len,
                    &mut values,
                    &mut root_started,
                    operation,
                    max_values,
                )?;
                let next_depth = stack_len.saturating_add(1);
                if next_depth > max_depth {
                    return Err(DomainError::Incomplete {
                        operation,
                        limit: max_depth,
                        observed: next_depth,
                    });
                }
                stack[stack_len] = if input[index] == b'{' {
                    JsonFrame::Object(JsonObjectState::KeyOrEnd)
                } else {
                    JsonFrame::Array(JsonArrayState::ValueOrEnd)
                };
                stack_len = next_depth;
                index += 1;
            }
            b'}' => {
                let Some(JsonFrame::Object(state)) =
                    stack_len.checked_sub(1).map(|position| stack[position])
                else {
                    return invalid_json_structure();
                };
                if !matches!(
                    state,
                    JsonObjectState::KeyOrEnd | JsonObjectState::CommaOrEnd
                ) {
                    return invalid_json_structure();
                }
                stack_len -= 1;
                if stack_len == 0 {
                    root_complete = true;
                }
                index += 1;
            }
            b']' => {
                let Some(JsonFrame::Array(state)) =
                    stack_len.checked_sub(1).map(|position| stack[position])
                else {
                    return invalid_json_structure();
                };
                if !matches!(
                    state,
                    JsonArrayState::ValueOrEnd | JsonArrayState::CommaOrEnd
                ) {
                    return invalid_json_structure();
                }
                stack_len -= 1;
                if stack_len == 0 {
                    root_complete = true;
                }
                index += 1;
            }
            b',' => {
                let Some(frame) = stack_len
                    .checked_sub(1)
                    .map(|position| &mut stack[position])
                else {
                    return invalid_json_structure();
                };
                match frame {
                    JsonFrame::Array(state) if *state == JsonArrayState::CommaOrEnd => {
                        *state = JsonArrayState::ValueOrEnd;
                    }
                    JsonFrame::Object(state) if *state == JsonObjectState::CommaOrEnd => {
                        *state = JsonObjectState::KeyOrEnd;
                    }
                    _ => return invalid_json_structure(),
                }
                index += 1;
            }
            b':' => {
                let Some(JsonFrame::Object(state)) = stack_len
                    .checked_sub(1)
                    .map(|position| &mut stack[position])
                else {
                    return invalid_json_structure();
                };
                if *state != JsonObjectState::Colon {
                    return invalid_json_structure();
                }
                *state = JsonObjectState::Value;
                index += 1;
            }
            b'"' => {
                let string_end = scan_json_string(input, index)?;
                let is_object_key = matches!(
                    stack_len.checked_sub(1).map(|position| &stack[position]),
                    Some(JsonFrame::Object(JsonObjectState::KeyOrEnd))
                );
                if is_object_key {
                    if let JsonFrame::Object(state) = &mut stack[stack_len - 1] {
                        *state = JsonObjectState::Colon;
                    }
                } else {
                    accept_json_value(
                        &mut stack,
                        stack_len,
                        &mut values,
                        &mut root_started,
                        operation,
                        max_values,
                    )?;
                    if stack_len == 0 {
                        root_complete = true;
                    }
                }
                index = string_end;
            }
            _ => {
                let value_end = scan_json_primitive(input, index)?;
                accept_json_value(
                    &mut stack,
                    stack_len,
                    &mut values,
                    &mut root_started,
                    operation,
                    max_values,
                )?;
                if stack_len == 0 {
                    root_complete = true;
                }
                index = value_end;
            }
        }
    }

    if !root_started || !root_complete || stack_len != 0 {
        return invalid_json_structure();
    }
    Ok(())
}

fn accept_json_value(
    stack: &mut [JsonFrame; MAX_EVENT_JSON_DEPTH],
    stack_len: usize,
    values: &mut usize,
    root_started: &mut bool,
    operation: &'static str,
    max_values: usize,
) -> Result<()> {
    if stack_len == 0 {
        if *root_started {
            return invalid_json_structure();
        }
        *root_started = true;
        return Ok(());
    }
    match &mut stack[stack_len - 1] {
        JsonFrame::Array(state) if *state == JsonArrayState::ValueOrEnd => {
            *state = JsonArrayState::CommaOrEnd;
        }
        JsonFrame::Object(state) if *state == JsonObjectState::Value => {
            *state = JsonObjectState::CommaOrEnd;
        }
        _ => return invalid_json_structure(),
    }
    *values = values.saturating_add(1);
    if *values > max_values {
        return Err(DomainError::Incomplete {
            operation,
            limit: max_values,
            observed: *values,
        });
    }
    Ok(())
}

fn scan_json_string(input: &[u8], mut index: usize) -> Result<usize> {
    debug_assert_eq!(input[index], b'"');
    index += 1;
    while index < input.len() {
        match input[index] {
            b'"' => return Ok(index + 1),
            b'\\' => {
                index = index.saturating_add(2);
            }
            _ => index += 1,
        }
    }
    invalid_json_structure()
}

fn scan_json_primitive(input: &[u8], mut index: usize) -> Result<usize> {
    let start = index;
    while index < input.len()
        && !input[index].is_ascii_whitespace()
        && !matches!(input[index], b',' | b']' | b'}' | b':' | b'{' | b'[' | b'"')
    {
        index += 1;
    }
    if index == start {
        return invalid_json_structure();
    }
    Ok(index)
}

fn invalid_json_structure<T>() -> Result<T> {
    Err(DomainError::Json("invalid JSON structure".to_owned()))
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

    fn new_reserved(max: usize, capacity: usize) -> Result<Self> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| DomainError::Incomplete {
                operation: "index canonical event destination",
                limit: capacity,
                observed: capacity,
            })?;
        Ok(Self {
            bytes,
            max,
            overflow: None,
        })
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
        PersistedPayload::ReviewExecutionRecorded(recorded) => {
            d1_payload_bytes("review_execution_recorded", &recorded.canonical_bytes()?)
        }
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
    if version == EventContractVersion::V2 && matches!(payload, PersistedPayload::ClaimProposed(_))
    {
        return Err(DomainError::Validation(
            "v2 requires the Unit D atomic execution record before claims or completed obligations"
                .to_owned(),
        ));
    }
    Ok(())
}

fn validate_v2_completed_transition(
    version: EventContractVersion,
    aggregate: &ReviewAggregate,
    payload: &PersistedPayload,
) -> Result<()> {
    if version == EventContractVersion::V2
        && let PersistedPayload::ObligationTransition {
            obligation_id,
            next: ObligationLifecycle::Completed,
        } = payload
        && !aggregate.has_structured_execution(obligation_id)
    {
        return Err(DomainError::Validation(
            "v2 completed lifecycle requires an earlier structured D2 execution".to_owned(),
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
    /// Heap capacity retained by this decoded envelope, excluding the inline
    /// `EventEnvelope` value charged by its owning vector.
    #[must_use]
    pub fn allocated_bytes(&self) -> usize {
        self.schema.capacity()
            + self.id.allocated_bytes()
            + self.run_id.allocated_bytes()
            + self.genesis_hash.allocated_bytes()
            + self.actor.capacity()
            + self.payload.get().len()
            + self.payload_hash.allocated_bytes()
            + self.previous_event_hash.allocated_bytes()
            + self.event_hash.allocated_bytes()
    }

    /// Declared closed event-contract version after envelope validation.
    pub fn contract_version(&self) -> Result<EventContractVersion> {
        EventContractVersion::parse(&self.schema)
    }

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
        Self::from_json_slice_with_d2_working_limit(input, MAX_D2_WORKING_BYTES)
    }

    fn from_json_slice_with_d2_working_limit(input: &[u8], working_limit: usize) -> Result<Self> {
        if input
            .windows(b"review_execution_recorded".len())
            .any(|window| window == b"review_execution_recorded")
        {
            let line_bytes = input.len().checked_add(1).ok_or(DomainError::Incomplete {
                operation: "D2 canonical event JSONL",
                limit: MAX_D1_EVENT_LINE_BYTES,
                observed: usize::MAX,
            })?;
            if line_bytes > MAX_D1_EVENT_LINE_BYTES {
                return Err(DomainError::Incomplete {
                    operation: "D2 canonical event JSONL",
                    limit: MAX_D1_EVENT_LINE_BYTES,
                    observed: line_bytes,
                });
            }
            preflight_event_json_structure(input)?;
            preflight_d2_decode_working(input, working_limit)?;
        }
        serde_json::from_slice(input).map_err(|error| DomainError::Json(error.to_string()))
    }

    /// Index-only bounded import seam. The structural cap is operational and
    /// does not change which historical envelopes are canonical.
    pub fn from_json_slice_for_index(input: &[u8]) -> Result<Self> {
        preflight_event_json_structure(input)?;
        Self::from_json_slice(input)
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

    /// Canonical writer used only by the bounded derived-index scanner. The
    /// destination request is made at the caller-admitted capacity before
    /// serialization; serde/value-tree transients remain core-owned.
    pub fn canonical_bytes_for_index(&self, capacity: usize) -> Result<Vec<u8>> {
        let payload = decode_canonical_payload(self.payload.get())?;
        if payload.is_d1_bounded() {
            let payload_bytes = payload_canonical_bytes(&payload)?;
            let mut out = BoundedEventJson::new_reserved(capacity, capacity)?;
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
            return out.finish();
        }
        let value = serde_json::to_value(self)
            .map(canonical_json_value)
            .map_err(|error| DomainError::CanonicalJson(error.to_string()))?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(capacity)
            .map_err(|_| DomainError::Incomplete {
                operation: "index canonical event destination",
                limit: capacity,
                observed: capacity,
            })?;
        serde_json::to_writer(&mut output, &value)
            .map_err(|error| DomainError::CanonicalJson(error.to_string()))?;
        if output.len() > capacity {
            return Err(DomainError::Incomplete {
                operation: "index canonical event destination",
                limit: capacity,
                observed: output.len(),
            });
        }
        Ok(output)
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

    /// Decodes the closed payload for an envelope whose stream cursor is
    /// validated online by a durable reader. Envelope shape and payload hash
    /// are still rechecked here; callers receive no access to raw JSON.
    pub fn decode_for_streaming_projection(&self) -> Result<ValidatedEvent<'_>> {
        self.validate()?;
        preflight_event_json_structure(self.payload.get().as_bytes())?;
        Ok(ValidatedEvent {
            envelope: self,
            payload: decoded_payload(decode_canonical_payload(self.payload.get())?),
        })
    }

    /// Validates one complete, confirmed stream prefix and exposes only
    /// typed payloads. Store code must not inspect the private JSON payload.
    pub fn validated_view<'a>(
        version: EventContractVersion,
        run_id: &StableId,
        genesis: EventStreamGenesis<'_>,
        events: &'a [EventEnvelope],
    ) -> Result<ValidatedEventView<'a>> {
        let (genesis_hash, snapshot, verified) = match (version, genesis) {
            (EventContractVersion::V1, EventStreamGenesis::V1(hash)) => (hash.clone(), None, None),
            (EventContractVersion::V2, EventStreamGenesis::V2(bytes)) => {
                let snapshot = RunGenesisSnapshot::from_canonical_bytes(bytes)?;
                (ContentHash::sha256(bytes), Some(snapshot), None)
            }
            (EventContractVersion::V2, EventStreamGenesis::V2Verified(verified)) => {
                (verified.genesis_hash.clone(), None, Some(verified))
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
        if let (EventContractVersion::V2, Some(first), Some(verified)) =
            (version, events.first(), verified)
        {
            let DecodedPayload::RunGenesisManifest(manifest) = first.payload() else {
                return Err(DomainError::EventSequence(
                    "v2 sequence one must carry RunGenesisManifest".to_owned(),
                ));
            };
            verified.validate_manifest(run_id, manifest)?;
        }
        Ok(ValidatedEventView {
            version,
            run_id: run_id.clone(),
            genesis_hash,
            verified_v2_genesis: snapshot.is_some() || verified.is_some(),
            verified_pristine_hash: verified.map(|value| value.pristine_aggregate_hash.clone()),
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
    verified_v2_genesis: bool,
    verified_pristine_hash: Option<ContentHash>,
    events: Vec<ValidatedEvent<'a>>,
}

/// Retained-capacity charge exposed to bounded durable projections.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventViewAccounting {
    retained_bytes: u64,
    scratch_peak_bytes: u64,
}

impl EventViewAccounting {
    #[must_use]
    pub const fn retained_bytes(self) -> u64 {
        self.retained_bytes
    }
    #[must_use]
    pub const fn scratch_peak_bytes(self) -> u64 {
        self.scratch_peak_bytes
    }
}

impl<'a> ValidatedEventView<'a> {
    /// Builds a typed view only after reserving its complete input-derived
    /// allocation charge against the caller's combined working limit.
    pub fn with_working_limit(
        version: EventContractVersion,
        run_id: &StableId,
        genesis: EventStreamGenesis<'_>,
        events: &'a [EventEnvelope],
        already_retained: u64,
        working_limit: u64,
    ) -> Result<(Self, EventViewAccounting)> {
        let working_limit_usize = usize::try_from(working_limit).unwrap_or(usize::MAX);
        let genesis_scratch = match genesis {
            EventStreamGenesis::V1(_) => 0,
            EventStreamGenesis::V2(bytes) => u64::try_from(bytes.len())
                .ok()
                .and_then(|length| length.checked_mul(2))
                .ok_or(DomainError::Incomplete {
                    operation: "validated genesis scratch",
                    limit: working_limit_usize,
                    observed: usize::MAX,
                })?,
            EventStreamGenesis::V2Verified(_) => 0,
        };
        let payload_scratch = events
            .iter()
            .map(|event| u64::try_from(event.payload.get().len()).unwrap_or(u64::MAX))
            .max()
            .unwrap_or(0)
            .checked_mul(2)
            .ok_or(DomainError::Incomplete {
                operation: "validated payload scratch",
                limit: working_limit_usize,
                observed: usize::MAX,
            })?;
        let scratch_peak_bytes = genesis_scratch.max(payload_scratch);
        let view = EventEnvelope::validated_view(version, run_id, genesis, events)?;
        let retained_bytes = view.allocated_bytes()?;
        let observed = already_retained
            .checked_add(retained_bytes)
            .and_then(|value| value.checked_add(scratch_peak_bytes))
            .ok_or(DomainError::Incomplete {
                operation: "validated event view allocation",
                limit: working_limit_usize,
                observed: usize::MAX,
            })?;
        if observed > working_limit {
            return Err(DomainError::Incomplete {
                operation: "validated event view allocation",
                limit: working_limit_usize,
                observed: usize::try_from(observed).unwrap_or(usize::MAX),
            });
        }
        Ok((
            view,
            EventViewAccounting {
                retained_bytes,
                scratch_peak_bytes,
            },
        ))
    }

    fn allocated_bytes(&self) -> Result<u64> {
        let vector = self
            .events
            .capacity()
            .checked_mul(std::mem::size_of::<ValidatedEvent<'_>>())
            .ok_or(DomainError::Incomplete {
                operation: "validated event view retained capacity",
                limit: usize::MAX,
                observed: usize::MAX,
            })?;
        let heap = self.events.iter().fold(0_usize, |total, event| {
            total.saturating_add(event.payload_allocated_bytes())
        });
        u64::try_from(
            vector
                .saturating_add(heap)
                .saturating_add(self.run_id.allocated_bytes())
                .saturating_add(self.genesis_hash.allocated_bytes()),
        )
        .map_err(|_| DomainError::Incomplete {
            operation: "validated event view retained capacity",
            limit: usize::MAX,
            observed: usize::MAX,
        })
    }
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
        match (self.version, self.verified_v2_genesis) {
            (EventContractVersion::V1, false) => {
                if self.genesis_hash != ContentHash::sha256(&canonical_json(initial)?) {
                    return Err(DomainError::Validation(
                        "offline v1 projection initial aggregate does not match its genesis hash"
                            .to_owned(),
                    ));
                }
            }
            (EventContractVersion::V2, true) => {
                let matches = if let Some(expected) = &self.verified_pristine_hash {
                    expected == &ContentHash::sha256(&canonical_json(initial)?)
                } else {
                    let snapshot = RunGenesisSnapshot::from_aggregate(initial)?;
                    let bytes = snapshot.canonical_bytes()?;
                    self.genesis_hash == ContentHash::sha256(&bytes)
                };
                if !matches {
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
    fn payload_allocated_bytes(&self) -> usize {
        match &self.payload {
            DecodedPayload::ObligationTransition { obligation_id, .. } => {
                obligation_id.allocated_bytes()
            }
            DecodedPayload::ClaimProposed(value) => value.allocated_bytes(),
            DecodedPayload::EvidenceRecorded(value) => value.allocated_bytes(),
            DecodedPayload::EvidenceBound(value) => value.allocated_bytes(),
            DecodedPayload::VerificationRecorded(value) => value.allocated_bytes(),
            DecodedPayload::DecisionRecorded(value) => value.allocated_bytes(),
            DecodedPayload::FindingRecorded(value) => value.allocated_bytes(),
            DecodedPayload::RunGenesisManifest(value) => value.allocated_bytes(),
            DecodedPayload::ArtifactRegistered(value) => value.allocated_bytes(),
            DecodedPayload::SnapshotSourcesRecorded(value) => value.allocated_bytes(),
            DecodedPayload::ReviewPlanRecorded(value) => value.allocated_bytes(),
            DecodedPayload::ContextEnvelopeProjected(value) => value.allocated_bytes(),
            DecodedPayload::ReviewExecutionRecorded { execution, claims } => execution
                .allocated_bytes()
                .saturating_add(
                    claims
                        .capacity()
                        .saturating_mul(std::mem::size_of::<ExecutionClaimV2>()),
                )
                .saturating_add(
                    claims
                        .iter()
                        .map(ExecutionClaimV2::allocated_bytes)
                        .sum::<usize>(),
                ),
        }
    }

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
    ReviewExecutionRecorded {
        execution: ExecutionRecord,
        claims: Vec<ExecutionClaimV2>,
    },
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
        PersistedPayload::ReviewExecutionRecorded(value) => {
            DecodedPayload::ReviewExecutionRecorded {
                execution: value.execution,
                claims: value.claims,
            }
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
    /// A D2 execution whose raw CAS bytes were unavailable offline.
    ReviewExecution,
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
    expected_events: Option<Vec<OfflineExpectedEvent>>,
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
            expected_events: Some(
                view.events
                    .iter()
                    .map(|event| OfflineExpectedEvent {
                        id: event.envelope.id.clone(),
                        event_hash: event.envelope.event_hash.clone(),
                    })
                    .collect(),
            ),
            aggregate: initial,
            unreconciled_records: BTreeMap::new(),
            unreconciled_metadata: BTreeMap::new(),
            projected_findings: BTreeMap::new(),
            projected_finding_metadata: BTreeMap::new(),
            projected_context_metadata: BTreeMap::new(),
            unreconciled_order: Vec::new(),
        })
    }

    /// Seeds a V2 projection whose envelopes will be validated and decoded
    /// one at a time by a lock-held durable stream reader.
    pub fn new_streaming_v2(
        run_id: &StableId,
        verified_genesis: &VerifiedV2Genesis,
        initial: ReviewAggregate,
    ) -> Result<Self> {
        if verified_genesis.run_id() != run_id {
            return Err(DomainError::EventSequence(
                "streaming projection run does not match verified genesis".to_owned(),
            ));
        }
        verified_genesis.validate_pristine_aggregate(&initial)?;
        let genesis_hash = verified_genesis.genesis_hash().clone();
        Ok(Self {
            version: EventContractVersion::V2,
            run_id: run_id.clone(),
            next_sequence: 1,
            previous_event_hash: event_chain_genesis_hash(run_id, &genesis_hash)?,
            genesis_hash,
            expected_events: None,
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
        if let Some(expected_events) = &self.expected_events {
            let expected = expected_events
                .get(usize::try_from(self.next_sequence - 1).map_err(|_| {
                    DomainError::EventSequence(
                        "offline projection sequence does not fit usize".to_owned(),
                    )
                })?)
                .ok_or_else(|| {
                    DomainError::EventSequence(
                        "offline projection cannot apply an event beyond its validated view"
                            .to_owned(),
                    )
                })?;
            if envelope.id != expected.id || envelope.event_hash != expected.event_hash {
                return Err(DomainError::EventSequence(
                    "offline projection event is not the expected validated-view event".to_owned(),
                ));
            }
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
            DecodedPayload::ReviewExecutionRecorded { execution, claims } => {
                return self.apply_unreconciled(
                    PersistedPayload::ReviewExecutionRecorded(ReviewExecutionRecorded {
                        execution: execution.clone(),
                        claims: claims.clone(),
                    }),
                    envelope,
                    event.envelope().actor(),
                );
            }
        };
        reject_v2_legacy_execution_payload(self.version, &payload)?;
        validate_v2_completed_transition(self.version, &self.aggregate, &payload)?;
        reject_duplicate_d1_record(&self.aggregate, &payload)?;
        payload.validate_for_enclosing_run(&self.run_id)?;
        let mut next = self.aggregate.clone();
        apply(
            &mut next,
            &payload,
            event.envelope().actor(),
            &self.run_id,
            None,
        )?;
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
        validate_v2_completed_transition(self.version, &self.aggregate, &payload)?;
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
            if matches!(payload, PersistedPayload::ReviewExecutionRecorded(_)) {
                return Err(DomainError::IdCollision { id });
            }
            return Err(DomainError::Validation(format!(
                "offline shadow record ID collision: {id}"
            )));
        }
        if !matches!(payload, PersistedPayload::ReviewExecutionRecorded(_)) {
            let mut candidate = self.shadow_candidate()?;
            apply(&mut candidate, &payload, actor, &self.run_id, None)?;
            candidate.validate()?;
        }
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
        apply(&mut candidate, &payload, actor, &self.run_id, None)?;
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
        // The canonical envelope metadata is retained privately in the
        // authority-free aggregate so later D2 execution/claim events can
        // close their references during offline replay. No runtime admission
        // or reviewer-resume capability is created by this metadata replay.
        self.aggregate.record_offline_execution_envelope(context)?;
        self.advance_offline_cursor(envelope)?;
        Ok(false)
    }

    fn shadow_candidate(&self) -> Result<ReviewAggregate> {
        let mut candidate = self.aggregate.clone();
        for id in &self.unreconciled_order {
            if let Some(payload) = self.unreconciled_records.get(id) {
                if !matches!(payload, PersistedPayload::ReviewExecutionRecorded(_)) {
                    apply(&mut candidate, payload, payload.actor(), &self.run_id, None)?;
                }
            } else if let Some(finding) = self.projected_findings.get(id) {
                apply(
                    &mut candidate,
                    &PersistedPayload::FindingRecorded(finding.clone()),
                    SYSTEM_ACTOR,
                    &self.run_id,
                    None,
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
        self.expected_events.as_ref().is_some_and(|events| {
            usize::try_from(self.next_sequence - 1).is_ok_and(|count| count == events.len())
        })
    }

    /// Confirms the terminal cursor for a streaming projection after the
    /// durable reader has issued its complete-prefix certificate.
    #[must_use]
    pub fn streaming_cursor_matches(&self, event_count: u64, tail_hash: &ContentHash) -> bool {
        self.expected_events.is_none()
            && self.next_sequence.checked_sub(1) == Some(event_count)
            && &self.previous_event_hash == tail_hash
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
        "review_plan_recorded" | "context_envelope_projected" | "review_execution_recorded"
    ) {
        return match raw.kind.as_str() {
            "review_plan_recorded" => Ok(PersistedPayload::ReviewPlanRecorded(
                ReviewPlan::from_event_bytes(raw.data.get().as_bytes())?,
            )),
            "context_envelope_projected" => Ok(PersistedPayload::ContextEnvelopeProjected(
                ReviewContextEnvelope::from_event_bytes(raw.data.get().as_bytes())
                    .map_err(context_domain_error)?,
            )),
            "review_execution_recorded" => {
                preflight_d2_decode_working(raw.data.get().as_bytes(), MAX_D2_WORKING_BYTES)?;
                let recorded: ReviewExecutionRecorded = serde_json::from_str(raw.data.get())
                    .map_err(|error| DomainError::Json(error.to_string()))?;
                recorded.validate_shape()?;
                recorded.validate_decode_working(raw.data.get().len())?;
                Ok(PersistedPayload::ReviewExecutionRecorded(recorded))
            }
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
    reviewer_raw_closure: Option<ReviewerRawClosure>,
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

/// Explicit resource bounds for replaying an imported V2 event prefix.
///
/// The caller owns the retained input prefix; these limits bound the replay
/// operation's event count and canonical envelope bytes before an editable
/// log is returned. It intentionally makes no heap or RSS promise.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventReplayLimits {
    pub max_events: u64,
    pub max_canonical_bytes: u64,
}

impl EventReplayLimits {
    #[must_use]
    pub const fn new(max_events: u64, max_canonical_bytes: u64) -> Self {
        Self {
            max_events,
            max_canonical_bytes,
        }
    }
}

impl EventLog {
    /// Rebuilds an editable V2 log only from canonical genesis bytes, a fully
    /// validated homogeneous prefix, and caller-supplied exact admissions.
    /// This method never derives authority admissions from persisted metadata.
    pub fn replay_validated_v2_prefix(
        run_id: StableId,
        genesis_bytes: &[u8],
        envelopes: &[EventEnvelope],
        admissions: &EventAdmissions,
        limits: EventReplayLimits,
    ) -> Result<Self> {
        preflight_v2_replay_limits(envelopes, limits)?;
        let verified = VerifiedV2Genesis::from_canonical_bytes(&run_id, genesis_bytes)?;
        if verified.genesis_hash != ContentHash::sha256(genesis_bytes) {
            return Err(DomainError::Validation(
                "verified genesis hash mismatch".to_owned(),
            ));
        }
        let initial =
            RunGenesisSnapshot::from_canonical_bytes(genesis_bytes)?.rebuild_aggregate()?;
        Self::replay_envelopes_with_event_capacity(
            EventContractVersion::V2,
            run_id,
            initial,
            envelopes,
            admissions,
            envelopes.len(),
        )
    }

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
        validate_v2_completed_transition(self.version, &self.aggregate, &payload)?;
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
            (
                PersistedPayload::ReviewExecutionRecorded(recorded),
                CommandAdmission::ReviewerRaw(closure),
            ) if closure.matches(recorded) => {}
            (PersistedPayload::ReviewExecutionRecorded(_), _) => {
                return Err(DomainError::Validation(
                    "recording a D2 execution requires its exact raw byte closure".to_owned(),
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
            reviewer_raw_closure,
        ) = match admission {
            CommandAdmission::Evidence(admission) => {
                (Some(admission), None, None, None, None, None)
            }
            CommandAdmission::Binding(admission) => (None, Some(admission), None, None, None, None),
            CommandAdmission::Verification(admission) => {
                (None, None, Some(admission), None, None, None)
            }
            CommandAdmission::Decision(admission) => {
                (None, None, None, Some(admission), None, None)
            }
            CommandAdmission::ContextProjection(admission) => {
                let payload = decode_canonical_payload(envelope.payload.get())?;
                let PersistedPayload::ContextEnvelopeProjected(context) = payload else {
                    return Err(DomainError::Validation(
                        "context admission cannot seal a non-context event".to_owned(),
                    ));
                };
                let positioned =
                    PositionedContextProjectionAdmission::seal(&envelope, &context, admission)?;
                (None, None, None, None, Some(positioned), None)
            }
            CommandAdmission::ReviewerRaw(closure) => (None, None, None, None, None, Some(closure)),
            CommandAdmission::None => (None, None, None, None, None, None),
        };
        self.append_envelope(
            envelope,
            evidence_admission,
            binding_admission,
            verification_admission,
            decision_admission,
            context_projection_admission,
            reviewer_raw_closure,
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
        let reviewer_raw = events
            .iter()
            .filter_map(|event| event.reviewer_raw_closure.clone())
            .collect::<Vec<_>>();
        Self::replay_envelopes(
            version,
            run_id,
            initial,
            &envelopes,
            &EventAdmissions::new(evidence, decisions)
                .with_trace_admissions(bindings, verifications)
                .with_context_admissions(context_projections)
                .with_reviewer_raw(reviewer_raw),
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
        Self::replay_envelopes_with_event_capacity(
            version, run_id, initial, envelopes, admissions, 0,
        )
    }

    fn replay_envelopes_with_event_capacity(
        version: EventContractVersion,
        run_id: StableId,
        initial: ReviewAggregate,
        envelopes: &[EventEnvelope],
        admissions: &EventAdmissions,
        event_capacity: usize,
    ) -> Result<Self> {
        let mut log = Self::new_with_version(version, run_id, initial)?;
        if event_capacity != 0 {
            log.events
                .try_reserve_exact(event_capacity)
                .map_err(|_| DomainError::Incomplete {
                    operation: "V2 replay event output capacity",
                    limit: event_capacity,
                    observed: event_capacity,
                })?;
        }
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
                admissions.reviewer_raw,
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
                admissions.reviewer_raw,
            )?;
        }
        *self = next;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn append_envelope(
        &mut self,
        envelope: EventEnvelope,
        evidence_admission: Option<EvidenceAdmission>,
        binding_admission: Option<EvidenceBindingAdmission>,
        verification_admission: Option<VerificationAdmission>,
        decision_admission: Option<DecisionAdmission>,
        context_projection_admission: Option<PositionedContextProjectionAdmission>,
        reviewer_raw_closure: Option<ReviewerRawClosure>,
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
        validate_v2_completed_transition(self.version, &self.aggregate, &payload)?;
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
        if let PersistedPayload::ReviewExecutionRecorded(recorded) = &payload
            && !reviewer_raw_closure
                .as_ref()
                .is_some_and(|closure| closure.matches(recorded))
        {
            return Err(DomainError::Validation(
                "D2 execution event lacks its exact raw byte closure".to_owned(),
            ));
        }
        let mut next = self.aggregate.clone();
        apply(
            &mut next,
            &payload,
            envelope.actor(),
            &self.run_id,
            reviewer_raw_closure
                .as_ref()
                .map(ReviewerRawClosure::raw_artifact_size),
        )?;
        self.aggregate = next;
        self.events.push(Event {
            envelope,
            evidence_admission,
            binding_admission,
            verification_admission,
            decision_admission,
            context_projection_admission,
            reviewer_raw_closure,
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
        if let PersistedPayload::ReviewExecutionRecorded(recorded) = payload {
            if self
                .aggregate
                .executions()
                .any(|execution| execution.id() == recorded.execution.id())
            {
                return Err(DomainError::IdCollision {
                    id: recorded.execution.id().clone(),
                });
            }
            if let Some(claim) = recorded.claims.iter().find(|claim| {
                self.aggregate
                    .execution_claims()
                    .any(|existing| existing.id() == claim.id())
            }) {
                return Err(DomainError::IdCollision {
                    id: claim.id().clone(),
                });
            }
        }
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
        let reviewer_raw = match payload {
            PersistedPayload::ReviewExecutionRecorded(recorded) => admissions
                .reviewer_raw_for(recorded)
                .ok_or_else(|| {
                    DomainError::Validation(
                        "imported D2 execution lacks its exact raw byte closure".to_owned(),
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
            reviewer_raw,
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
        self.append_envelope(envelope, None, None, None, None, None, None)?;
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

fn replay_incomplete(operation: &'static str, limit: u64, observed: u64) -> DomainError {
    DomainError::Incomplete {
        operation,
        limit: usize::try_from(limit).unwrap_or(usize::MAX),
        observed: usize::try_from(observed).unwrap_or(usize::MAX),
    }
}

fn replay_add(operation: &'static str, limit: u64, total: u64, next: u64) -> Result<u64> {
    let observed = total
        .checked_add(next)
        .ok_or_else(|| replay_incomplete(operation, limit, u64::MAX))?;
    if observed > limit {
        return Err(replay_incomplete(operation, limit, observed));
    }
    Ok(observed)
}

fn preflight_v2_replay_limits(
    envelopes: &[EventEnvelope],
    limits: EventReplayLimits,
) -> Result<()> {
    if envelopes.is_empty() {
        return Err(DomainError::EventSequence(
            "V2 replay requires the sequence-one genesis manifest".to_owned(),
        ));
    }
    let count = u64::try_from(envelopes.len())
        .map_err(|_| replay_incomplete("V2 replay event count", limits.max_events, u64::MAX))?;
    if count > limits.max_events {
        return Err(replay_incomplete(
            "V2 replay event count",
            limits.max_events,
            count,
        ));
    }
    let mut canonical = 0_u64;
    for envelope in envelopes {
        if envelope.contract_version()? != EventContractVersion::V2 {
            return Err(DomainError::Validation(
                "V2 replay refuses a non-V2 envelope".to_owned(),
            ));
        }
        // Exact canonical serialization is performed before any output-log
        // clone. `max_canonical_bytes` is a byte-stream contract, not a heap
        // or RSS assertion.
        let bytes = envelope.canonical_bytes()?;
        canonical = replay_add(
            "V2 replay canonical bytes",
            limits.max_canonical_bytes,
            canonical,
            u64::try_from(bytes.len()).map_err(|_| {
                replay_incomplete(
                    "V2 replay canonical bytes",
                    limits.max_canonical_bytes,
                    u64::MAX,
                )
            })?,
        )?;
    }
    Ok(())
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
    reviewer_raw_size: Option<u64>,
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
        PersistedPayload::ReviewExecutionRecorded(recorded) => {
            let expected_raw_size = reviewer_raw_size.ok_or_else(|| {
                DomainError::Validation(
                    "D2 execution requires verified raw bytes; registration metadata is insufficient"
                        .to_owned(),
                )
            })?;
            aggregate.record_execution(
                expected_run_id,
                recorded.execution.clone(),
                recorded.claims.clone(),
                expected_raw_size,
            )
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

    fn d2_log() -> (
        EventLog,
        ReviewAggregate,
        ReviewPlan,
        StableId,
        ReviewContextEnvelope,
        BTreeMap<StableId, Vec<u8>>,
    ) {
        let mut input: Value = serde_json::from_slice(FIXTURE).unwrap();
        let mut contains = input["relations"][0].clone();
        contains["id"] = Value::String("relation:file-contains-payment-charge".to_owned());
        contains["kind"] = Value::String("contains".to_owned());
        contains["source_id"] = Value::String("file:payment-repository".to_owned());
        contains["target_ids"] =
            Value::Array(vec![Value::String("function:payment-charge".to_owned())]);
        contains["directed"] = Value::Bool(true);
        input["relations"].as_array_mut().unwrap().push(contains);
        let test_artifact = input["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["id"] == "test:double-submit")
            .unwrap();
        test_artifact["location"]["start_line"] = Value::Null;
        test_artifact["location"]["end_line"] = Value::Null;
        let bytes_by_path = BTreeMap::from([
            ("src/checkout_controller.rs", b"checkout\n".repeat(40)),
            ("src/payment_repository.rs", b"repository\n".repeat(40)),
        ]);
        for artifact in input["artifacts"].as_array_mut().unwrap() {
            if artifact["kind"] == "file" {
                let path = artifact["location"]["path"].as_str().unwrap();
                artifact["content_hash"] =
                    Value::String(ContentHash::sha256(&bytes_by_path[path]).to_string());
            }
        }
        let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
        let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
        let initial = ReviewAggregate::new(program, universe, obligations).unwrap();
        let run_id = id("run:d2-core");
        let mut log = EventLog::new(run_id.clone(), initial.clone()).unwrap();
        let snapshot_id = log.aggregate().program().snapshot_id().clone();
        let files = log
            .aggregate()
            .program()
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
            .cloned()
            .collect::<Vec<_>>();
        let mut entries = Vec::new();
        let mut source_by_id = BTreeMap::new();
        for artifact in files {
            let path = artifact.location.as_ref().unwrap().path.clone();
            let bytes = bytes_by_path[&path.as_str()].clone();
            let hash = ContentHash::sha256(&bytes);
            let source = ArtifactSource::SnapshotIngest {
                run_id: run_id.clone(),
                snapshot_id: snapshot_id.clone(),
                adapter_id: "fixture-adapter".to_owned(),
            };
            let registration = ArtifactRegistered::new(
                run_id.clone(),
                ArtifactRegistered::derived_id(
                    &run_id,
                    &hash,
                    "text/plain",
                    ArtifactSensitivity::WorkspaceSource,
                    &source,
                )
                .unwrap(),
                hash.clone(),
                "text/plain",
                u64::try_from(bytes.len()).unwrap(),
                ArtifactSensitivity::WorkspaceSource,
                source,
            )
            .unwrap();
            entries.push(
                SnapshotSourceRecordEntry::new(
                    artifact.id.clone(),
                    path,
                    hash.clone(),
                    registration.registration_id().clone(),
                    hash,
                    u64::try_from(bytes.iter().filter(|byte| **byte == b'\n').count()).unwrap() + 1,
                )
                .unwrap(),
            );
            source_by_id.insert(artifact.id, bytes);
            log.append(EventCommand::artifact_registered(registration))
                .unwrap();
        }
        entries.sort_by(|left, right| left.path().cmp(right.path()));
        log.append(EventCommand::snapshot_sources_recorded(
            SnapshotSourcesRecorded::new(snapshot_id, entries).unwrap(),
        ))
        .unwrap();
        let review_plan = plan(log.aggregate(), PlanBudget::new(16, 16).unwrap()).unwrap();
        log.append(EventCommand::review_plan_recorded(review_plan.clone()))
            .unwrap();
        let (obligation_id, built) = review_plan
            .waves()
            .iter()
            .flat_map(|wave| wave.obligation_ids())
            .find_map(|candidate| {
                let mut session =
                    crate::prepare_context(log.aggregate(), candidate.clone()).ok()?;
                loop {
                    let request = match session.next_source_request() {
                        Ok(Some(request)) => request,
                        Ok(None) => break,
                        Err(_) => return None,
                    };
                    if session
                        .submit_source(&request, &source_by_id[request.artifact_id()])
                        .is_err()
                    {
                        return None;
                    }
                }
                let built = session.finish().ok()?;
                (!built.envelope().normalized_included_source_ids().is_empty())
                    .then(|| (candidate.clone(), built))
            })
            .expect("fixture has a source-grounded scheduled obligation");
        log.append(EventCommand::obligation_transition(
            obligation_id.clone(),
            ObligationLifecycle::Planned,
        ))
        .unwrap();
        log.append(EventCommand::obligation_transition(
            obligation_id.clone(),
            ObligationLifecycle::InProgress,
        ))
        .unwrap();
        let envelope = built.envelope().clone();
        log.append(EventCommand::context_envelope_projected(built))
            .unwrap();
        (
            log,
            initial,
            review_plan,
            obligation_id,
            envelope,
            source_by_id,
        )
    }

    #[test]
    fn replay_validated_v2_prefix_requires_exact_host_admissions() {
        let (log, _initial, _plan, _obligation, _envelope, _sources) = d2_log();
        let genesis = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let envelopes = log
            .events()
            .iter()
            .map(|event| event.envelope().clone())
            .collect::<Vec<_>>();

        assert!(
            EventLog::replay_validated_v2_prefix(
                log.run_id().clone(),
                &genesis,
                &envelopes,
                &EventAdmissions::default(),
                EventReplayLimits::new(u64::MAX, u64::MAX),
            )
            .is_err()
        );

        let context_admissions = log
            .events()
            .iter()
            .filter_map(|event| event.context_projection_admission.clone())
            .collect::<Vec<_>>();
        let replayed = EventLog::replay_validated_v2_prefix(
            log.run_id().clone(),
            &genesis,
            &envelopes,
            &EventAdmissions::default().with_context_admissions(context_admissions.clone()),
            EventReplayLimits::new(u64::MAX, u64::MAX),
        )
        .unwrap();
        assert_eq!(replayed.tail_hash(), log.tail_hash());
        assert_eq!(replayed.events().len(), log.events().len());

        let mut wrong = context_admissions;
        wrong[0].sequence += 1;
        assert!(
            EventLog::replay_validated_v2_prefix(
                log.run_id().clone(),
                &genesis,
                &envelopes,
                &EventAdmissions::default().with_context_admissions(wrong),
                EventReplayLimits::new(u64::MAX, u64::MAX),
            )
            .is_err()
        );
    }

    #[test]
    fn replay_validated_v2_prefix_limits_are_exact_atomic_and_refuse_v1() {
        let log = v2_log("run:replay-limits");
        let genesis = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let envelopes = log
            .events()
            .iter()
            .map(|event| event.envelope().clone())
            .collect::<Vec<_>>();
        let canonical = envelopes
            .iter()
            .try_fold(0_u64, |total, envelope| {
                total
                    .checked_add(u64::try_from(envelope.canonical_bytes()?.len()).unwrap())
                    .ok_or_else(|| {
                        replay_incomplete("V2 replay canonical bytes", u64::MAX, u64::MAX)
                    })
            })
            .unwrap();

        assert!(
            EventLog::replay_validated_v2_prefix(
                log.run_id().clone(),
                &genesis,
                &envelopes,
                &EventAdmissions::default(),
                EventReplayLimits::new(1, canonical),
            )
            .is_ok()
        );
        for limits in [
            EventReplayLimits::new(0, canonical),
            EventReplayLimits::new(1, canonical - 1),
        ] {
            assert!(
                EventLog::replay_validated_v2_prefix(
                    log.run_id().clone(),
                    &genesis,
                    &envelopes,
                    &EventAdmissions::default(),
                    limits,
                )
                .is_err(),
                "limits unexpectedly admitted: {limits:?}"
            );
            assert_eq!(log.events().len(), 1);
            assert_eq!(
                log.aggregate().obligations().count(),
                log.initial.obligations().count()
            );
        }
        assert!(replay_add("V2 replay canonical bytes", u64::MAX, u64::MAX, 1).is_err());
        assert!(matches!(
            EventLog::replay_validated_v2_prefix(
                log.run_id().clone(),
                &genesis,
                &[],
                &EventAdmissions::default(),
                EventReplayLimits::new(1, canonical),
            ),
            Err(DomainError::EventSequence(_))
        ));

        let mut v1_source = v2_log("run:replay-v1-refusal");
        let obligation = v1_source
            .aggregate()
            .obligations()
            .next()
            .unwrap()
            .id()
            .clone();
        v1_source
            .append(EventCommand::obligation_transition(
                obligation,
                ObligationLifecycle::Planned,
            ))
            .unwrap();
        let transition = v1_source.events()[1].envelope();
        let payload = decode_canonical_payload(transition.payload.get()).unwrap();
        let v1 = EventEnvelope::new(
            EventContractVersion::V1,
            v1_source.run_id().clone(),
            v1_source.genesis_hash().clone(),
            1,
            SYSTEM_ACTOR,
            1,
            event_chain_genesis_hash(v1_source.run_id(), v1_source.genesis_hash()).unwrap(),
            payload,
        )
        .unwrap();
        assert!(
            EventLog::replay_validated_v2_prefix(
                v1_source.run_id().clone(),
                &genesis,
                &[v1],
                &EventAdmissions::default(),
                EventReplayLimits::new(1, canonical),
            )
            .is_err(),
            "V1 envelope must be refused"
        );
    }

    #[test]
    fn replay_limits_apply_exact_canonical_sum_to_multi_event_prefix() {
        let (log, _initial, _plan, _obligation, _envelope, _sources) = d2_log();
        let genesis = log
            .run_genesis_snapshot()
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let envelopes = log
            .events()
            .iter()
            .map(|event| event.envelope().clone())
            .collect::<Vec<_>>();
        assert!(envelopes.len() >= 5);
        assert!(
            envelopes
                .iter()
                .any(|event| { event.payload.get().contains("review_plan_recorded") })
        );
        assert!(
            envelopes
                .iter()
                .any(|event| { event.payload.get().contains("context_envelope_projected") })
        );
        let admissions = EventAdmissions::default().with_context_admissions(
            log.events()
                .iter()
                .filter_map(|event| event.context_projection_admission.clone())
                .collect(),
        );
        let canonical = envelopes
            .iter()
            .try_fold(0_u64, |total, envelope| {
                total
                    .checked_add(u64::try_from(envelope.canonical_bytes()?.len()).unwrap())
                    .ok_or_else(|| {
                        replay_incomplete("V2 replay canonical bytes", u64::MAX, u64::MAX)
                    })
            })
            .unwrap();
        let exact = EventReplayLimits::new(u64::try_from(envelopes.len()).unwrap(), canonical);
        assert!(
            EventLog::replay_validated_v2_prefix(
                log.run_id().clone(),
                &genesis,
                &envelopes,
                &admissions,
                exact,
            )
            .is_ok()
        );
        assert!(
            EventLog::replay_validated_v2_prefix(
                log.run_id().clone(),
                &genesis,
                &envelopes,
                &admissions,
                EventReplayLimits::new(u64::try_from(envelopes.len()).unwrap(), canonical - 1),
            )
            .is_err()
        );
    }

    fn append_d2_attempt(
        log: &mut EventLog,
        review_plan: &ReviewPlan,
        obligation_id: &StableId,
        envelope: &ReviewContextEnvelope,
        source_by_id: &BTreeMap<StableId, Vec<u8>>,
        attempt: u32,
        outcome: crate::ExecutionOutcome,
    ) -> (StableId, Option<StableId>) {
        let wave = review_plan
            .waves()
            .iter()
            .find(|wave| wave.obligation_ids().contains(obligation_id))
            .unwrap();
        let input = crate::ExecutionRecordInput::fake(
            review_plan.id().clone(),
            wave.id().clone(),
            obligation_id.clone(),
            envelope.id().clone(),
            envelope.snapshot_id().clone(),
            attempt,
        )
        .unwrap();
        let execution_id = input.execution_id().unwrap();
        let raw = format!("{{\"attempt\":{attempt},\"fixture\":true}}").into_bytes();
        let registration = ArtifactRegistered::reviewer_execution(
            log.run_id().clone(),
            execution_id.clone(),
            crate::execution::FAKE_REVIEWER_ID,
            ContentHash::sha256(&raw),
            "application/json",
            u64::try_from(raw.len()).unwrap(),
        )
        .unwrap();
        log.append(EventCommand::artifact_registered(registration.clone()))
            .unwrap();
        let claims = if outcome.is_structured() {
            let obligation = log
                .aggregate()
                .obligations()
                .find(|obligation| obligation.id() == obligation_id)
                .unwrap();
            vec![
                crate::ExecutionClaimInputV2::new(
                    obligation.property_id(),
                    obligation.normalized_target_refs().clone(),
                    ClaimPolarity::IssueAbsent,
                    "fixture found no issue within the bounded projection",
                    envelope.normalized_included_source_ids().clone(),
                    BTreeSet::new(),
                    BTreeSet::new(),
                    Some(1.0),
                )
                .unwrap(),
            ]
        } else {
            Vec::new()
        };
        let source_buffers = source_by_id.values().collect::<Vec<_>>();
        let bundle = ValidatedExecutionBundle::fake(
            input,
            &registration,
            raw,
            source_buffers,
            claims,
            outcome,
        )
        .unwrap();
        let claim_id = bundle.claims().first().map(|claim| claim.id().clone());
        log.append(EventCommand::review_execution_recorded(bundle))
            .unwrap();
        (execution_id, claim_id)
    }

    #[test]
    fn fake_attempt_state_tracks_none_raw_recorded_and_completed_without_inference() {
        let (mut log, _initial, review_plan, obligation_id, envelope, source_by_id) = d2_log();
        let wave = review_plan
            .waves()
            .iter()
            .find(|wave| wave.obligation_ids().contains(&obligation_id))
            .unwrap();
        let input = crate::ExecutionRecordInput::fake(
            review_plan.id().clone(),
            wave.id().clone(),
            obligation_id.clone(),
            envelope.id().clone(),
            envelope.snapshot_id().clone(),
            1,
        )
        .unwrap();
        let execution_id = input.execution_id().unwrap();
        assert!(matches!(
            log.aggregate().fake_attempt_state(&execution_id),
            crate::FakeAttemptState::None
        ));

        let raw = br#"{"fixture":"progress"}"#.to_vec();
        let registration = ArtifactRegistered::reviewer_execution(
            log.run_id().clone(),
            execution_id.clone(),
            crate::execution::FAKE_REVIEWER_ID,
            ContentHash::sha256(&raw),
            "application/json",
            u64::try_from(raw.len()).unwrap(),
        )
        .unwrap();
        log.append(EventCommand::artifact_registered(registration.clone()))
            .unwrap();
        assert!(matches!(
            log.aggregate().fake_attempt_state(&execution_id),
            crate::FakeAttemptState::RawRegistered { .. }
        ));

        let sources = source_by_id.values().collect::<Vec<_>>();
        let obligation = log
            .aggregate()
            .obligations()
            .find(|obligation| obligation.id() == &obligation_id)
            .unwrap();
        let claims = vec![
            crate::ExecutionClaimInputV2::new(
                obligation.property_id(),
                obligation.normalized_target_refs().clone(),
                ClaimPolarity::IssueAbsent,
                "fixture found no issue within the bounded projection",
                envelope.normalized_included_source_ids().clone(),
                BTreeSet::new(),
                BTreeSet::new(),
                Some(1.0),
            )
            .unwrap(),
        ];
        let bundle = ValidatedExecutionBundle::fake(
            input,
            &registration,
            raw,
            sources,
            claims,
            crate::ExecutionOutcome::Structured,
        )
        .unwrap();
        log.append(EventCommand::review_execution_recorded(bundle))
            .unwrap();
        assert!(matches!(
            log.aggregate().fake_attempt_state(&execution_id),
            crate::FakeAttemptState::ExecutionRecorded { .. }
        ));
        log.append(EventCommand::obligation_transition(
            obligation_id,
            ObligationLifecycle::Completed,
        ))
        .unwrap();
        assert!(matches!(
            log.aggregate().fake_attempt_state(&execution_id),
            crate::FakeAttemptState::Completed { .. }
        ));
    }

    #[test]
    fn d2_registration_identity_is_typed_golden_and_size_is_not_an_id_input() {
        let run_id = id("run:d2-golden");
        let execution_id = id("execution:d2-golden");
        let hash = ContentHash::parse(
            "sha256:27d5941642c432f2d0b588a4aa7761005af04da7d8062ac2be23a27d95331a30",
        )
        .unwrap();
        let registration = ArtifactRegistered::reviewer_execution(
            run_id.clone(),
            execution_id.clone(),
            crate::execution::FAKE_REVIEWER_ID,
            hash.clone(),
            "application/json",
            17,
        )
        .unwrap();
        assert_eq!(
            registration.registration_id().as_str(),
            "registration:sha256:1df56aaaf0c539a2b9e2c65917999b5cf6075f1bf64810a31e08e3a3492df6fc"
        );
        let different_size = ArtifactRegistered::reviewer_execution(
            run_id.clone(),
            execution_id.clone(),
            crate::execution::FAKE_REVIEWER_ID,
            hash.clone(),
            "application/json",
            18,
        )
        .unwrap();
        assert_eq!(
            registration.registration_id(),
            different_size.registration_id()
        );
        let tampered_media = ArtifactRegistered::reviewer_execution(
            run_id,
            execution_id,
            crate::execution::FAKE_REVIEWER_ID,
            hash,
            "text/plain",
            17,
        )
        .unwrap();
        assert_ne!(
            registration.registration_id(),
            tampered_media.registration_id()
        );
    }

    #[test]
    fn d2_raw_size_splice_is_rejected_at_atomic_aggregate_admission() {
        let (mut log, _initial, review_plan, obligation_id, envelope, source_by_id) = d2_log();
        let wave = review_plan
            .waves()
            .iter()
            .find(|wave| wave.obligation_ids().contains(&obligation_id))
            .unwrap();
        let input = crate::ExecutionRecordInput::fake(
            review_plan.id().clone(),
            wave.id().clone(),
            obligation_id,
            envelope.id().clone(),
            envelope.snapshot_id().clone(),
            1,
        )
        .unwrap();
        let raw = br#"{"fixture":"size-closure"}"#;
        let correct = ArtifactRegistered::reviewer_execution(
            log.run_id().clone(),
            input.execution_id().unwrap(),
            crate::execution::FAKE_REVIEWER_ID,
            ContentHash::sha256(raw),
            "application/json",
            u64::try_from(raw.len()).unwrap(),
        )
        .unwrap();
        let source_buffers = source_by_id.values().collect::<Vec<_>>();
        let bundle = ValidatedExecutionBundle::fake(
            input,
            &correct,
            raw.to_vec(),
            source_buffers,
            vec![],
            crate::ExecutionOutcome::ProviderFailure {
                retryable: false,
                diagnostic: "fixture".to_owned(),
            },
        )
        .unwrap();
        let wrong_size = ArtifactRegistered::new(
            correct.run_id().clone(),
            correct.registration_id().clone(),
            correct.cas_hash().clone(),
            correct.media_type(),
            correct.size() + 1,
            correct.sensitivity(),
            correct.source().clone(),
        )
        .unwrap();
        log.append(EventCommand::artifact_registered(wrong_size))
            .unwrap();
        let before_events = log.events().len();
        assert!(matches!(
            log.append(EventCommand::review_execution_recorded(bundle)),
            Err(DomainError::Validation(message))
                if message.contains("raw registration")
        ));
        assert_eq!(log.events().len(), before_events);
        assert_eq!(log.aggregate().executions().count(), 0);
        assert_eq!(log.aggregate().execution_claims().count(), 0);
    }

    #[test]
    fn offline_d2_wrong_size_metadata_without_raw_bytes_is_shadow_only() {
        let (mut log, initial, review_plan, obligation_id, envelope, source_by_id) = d2_log();
        let wave = review_plan
            .waves()
            .iter()
            .find(|wave| wave.obligation_ids().contains(&obligation_id))
            .unwrap();
        let input = crate::ExecutionRecordInput::fake(
            review_plan.id().clone(),
            wave.id().clone(),
            obligation_id,
            envelope.id().clone(),
            envelope.snapshot_id().clone(),
            1,
        )
        .unwrap();
        let raw = br#"{"fixture":"offline-size"}"#;
        let correct = ArtifactRegistered::reviewer_execution(
            log.run_id().clone(),
            input.execution_id().unwrap(),
            crate::execution::FAKE_REVIEWER_ID,
            ContentHash::sha256(raw),
            "application/json",
            u64::try_from(raw.len()).unwrap(),
        )
        .unwrap();
        let source_buffers = source_by_id.values().collect::<Vec<_>>();
        let bundle = ValidatedExecutionBundle::fake(
            input,
            &correct,
            raw.to_vec(),
            source_buffers,
            vec![],
            crate::ExecutionOutcome::ProviderFailure {
                retryable: false,
                diagnostic: "fixture".to_owned(),
            },
        )
        .unwrap();
        let wrong_size = ArtifactRegistered::new(
            correct.run_id().clone(),
            correct.registration_id().clone(),
            correct.cas_hash().clone(),
            correct.media_type(),
            correct.size() + 1,
            correct.sensitivity(),
            correct.source().clone(),
        )
        .unwrap();
        log.append(EventCommand::artifact_registered(wrong_size))
            .unwrap();
        let (recorded, _raw_closure) = bundle.into_parts();
        let execution_event = EventEnvelope::new(
            EventContractVersion::V2,
            log.run_id().clone(),
            log.genesis_hash().clone(),
            log.next_sequence().unwrap(),
            SYSTEM_ACTOR,
            log.next_sequence().unwrap(),
            log.tail_hash().clone(),
            PersistedPayload::ReviewExecutionRecorded(recorded),
        )
        .unwrap();
        let mut envelopes = log.envelopes().cloned().collect::<Vec<_>>();
        envelopes.push(execution_event);
        let genesis = RunGenesisSnapshot::from_aggregate(&initial)
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let view = EventEnvelope::validated_view(
            EventContractVersion::V2,
            log.run_id(),
            EventStreamGenesis::V2(&genesis),
            &envelopes,
        )
        .unwrap();
        let mut offline = OfflineProjectionState::new(&view, initial).unwrap();
        for event in view.events() {
            if matches!(
                event.payload(),
                DecodedPayload::ReviewExecutionRecorded { .. }
            ) {
                assert!(!offline.apply(event).unwrap());
            } else {
                offline.apply(event).unwrap();
            }
        }
        assert_eq!(offline.aggregate().executions().count(), 0);
        assert_eq!(offline.aggregate().execution_claims().count(), 0);
        assert_eq!(
            offline.unreconciled_records().last().unwrap().kind(),
            UnreconciledRecordKind::ReviewExecution
        );
    }

    #[test]
    fn duplicate_artifact_registration_replay_is_typed_and_atomic() {
        let (mut log, _initial, _plan, _obligation, _envelope, _sources) = d2_log();
        let registration = log
            .events()
            .iter()
            .find_map(|event| {
                match decode_canonical_payload(event.envelope().payload.get()).unwrap() {
                    PersistedPayload::ArtifactRegistered(registration) => Some(registration),
                    _ => None,
                }
            })
            .unwrap();
        let duplicate = forged_envelope(
            EventContractVersion::V2,
            log.run_id().clone(),
            log.genesis_hash().clone(),
            log.next_sequence().unwrap(),
            log.tail_hash().clone(),
            PersistedPayload::ArtifactRegistered(registration.clone()),
        );
        let before_events = log.events().len();
        let before_tail = log.tail_hash().clone();
        assert!(matches!(
            log.resume_envelopes(&[duplicate], &EventAdmissions::default()),
            Err(DomainError::IdCollision { id }) if id == *registration.registration_id()
        ));
        assert_eq!(log.events().len(), before_events);
        assert_eq!(log.tail_hash(), &before_tail);
    }

    #[test]
    fn d2_outer_decode_preallocation_refuses_before_envelope_serde() {
        let (mut log, _initial, plan, obligation, envelope, sources) = d2_log();
        append_d2_attempt(
            &mut log,
            &plan,
            &obligation,
            &envelope,
            &sources,
            1,
            crate::ExecutionOutcome::ProviderFailure {
                retryable: false,
                diagnostic: "fixture".to_owned(),
            },
        );
        let bytes = log
            .events()
            .iter()
            .rev()
            .find(|event| {
                event
                    .envelope()
                    .payload
                    .get()
                    .contains("review_execution_recorded")
            })
            .unwrap()
            .envelope()
            .canonical_bytes()
            .unwrap();
        let observed = crate::execution::d2_decode_working_observed(&bytes, usize::MAX).unwrap();
        EventEnvelope::from_json_slice_with_d2_working_limit(&bytes, observed).unwrap();
        assert!(matches!(
            EventEnvelope::from_json_slice_with_d2_working_limit(&bytes, observed - 1),
            Err(DomainError::Incomplete {
                operation: "D2 decode working bytes",
                limit,
                observed: actual,
            }) if limit + 1 == actual && actual == observed
        ));
    }

    fn replace_json_container(input: &str, field: &str, open: u8, close: u8, body: &str) -> String {
        let marker = format!("\"{field}\":{}", char::from(open));
        let open_index = input.find(&marker).unwrap() + marker.len() - 1;
        let bytes = input.as_bytes();
        let mut depth = 0_usize;
        let mut index = open_index;
        let mut in_string = false;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' if in_string => index += 1,
                b'"' => in_string = !in_string,
                byte if !in_string && byte == open => depth += 1,
                byte if !in_string && byte == close => {
                    depth -= 1;
                    if depth == 0 {
                        return format!("{}{}{}", &input[..open_index + 1], body, &input[index..]);
                    }
                }
                _ => {}
            }
            index += 1;
        }
        panic!("fixture container did not close")
    }

    struct D2CountBoundary<'a> {
        open: u8,
        close: u8,
        operation: &'a str,
        limit: usize,
    }

    fn assert_d2_decode_count_boundary(
        base: &str,
        field: &str,
        exact_body: &str,
        over_body: &str,
        boundary: D2CountBoundary<'_>,
    ) {
        let exact = replace_json_container(base, field, boundary.open, boundary.close, exact_body);
        assert!(!matches!(
            EventEnvelope::from_json_slice(exact.as_bytes()),
            Err(DomainError::Incomplete {
                operation: found,
                ..
            }) if found == boundary.operation
        ));
        let over = replace_json_container(base, field, boundary.open, boundary.close, over_body);
        let over_result = EventEnvelope::from_json_slice(over.as_bytes());
        let diagnostic = format!("{over_result:?}");
        assert!(
            matches!(
                over_result,
                Err(DomainError::Incomplete {
                    operation: found,
                    limit: found_limit,
                    observed,
                }) if found == boundary.operation
                    && found_limit == boundary.limit
                    && observed == boundary.limit + 1
            ),
            "field={field} result={diagnostic}"
        );
    }

    #[test]
    fn d2_nested_count_caps_are_preflighted_at_actual_event_decode() {
        let (mut log, _initial, plan, obligation, envelope, sources) = d2_log();
        append_d2_attempt(
            &mut log,
            &plan,
            &obligation,
            &envelope,
            &sources,
            1,
            crate::ExecutionOutcome::Structured,
        );
        let base = String::from_utf8(
            log.events()
                .iter()
                .rev()
                .find(|event| {
                    event
                        .envelope()
                        .payload
                        .get()
                        .contains("review_execution_recorded")
                })
                .unwrap()
                .envelope()
                .canonical_bytes()
                .unwrap(),
        )
        .unwrap();

        let claims_open = base.find("\"claims\":[").unwrap() + "\"claims\":".len();
        let claims_close = {
            let bytes = base.as_bytes();
            let mut depth = 0_usize;
            let mut index = claims_open;
            let mut in_string = false;
            loop {
                match bytes[index] {
                    b'\\' if in_string => index += 1,
                    b'"' => in_string = !in_string,
                    b'[' if !in_string => depth += 1,
                    b']' if !in_string => {
                        depth -= 1;
                        if depth == 0 {
                            break index;
                        }
                    }
                    _ => {}
                }
                index += 1;
            }
        };
        let one_claim = &base[claims_open + 1..claims_close];
        let claims_exact = std::iter::repeat_n(one_claim, crate::execution::MAX_D2_CLAIMS)
            .collect::<Vec<_>>()
            .join(",");
        let claims_over = std::iter::repeat_n(one_claim, crate::execution::MAX_D2_CLAIMS + 1)
            .collect::<Vec<_>>()
            .join(",");
        assert_d2_decode_count_boundary(
            &base,
            "claims",
            &claims_exact,
            &claims_over,
            D2CountBoundary {
                open: b'[',
                close: b']',
                operation: "D2 execution claims",
                limit: crate::execution::MAX_D2_CLAIMS,
            },
        );

        for (field, limit, operation, prefix) in [
            (
                "target_refs",
                crate::execution::MAX_D2_TARGET_REFS,
                "D2 claim target refs",
                "file:t",
            ),
            (
                "source_ids",
                crate::execution::MAX_D2_SOURCE_IDS,
                "D2 claim source IDs",
                "file:s",
            ),
            (
                "assumptions",
                crate::execution::MAX_D2_ASSUMPTIONS,
                "D2 claim assumptions",
                "assumption-",
            ),
            (
                "requested_evidence",
                crate::execution::MAX_D2_REQUESTED_EVIDENCE,
                "D2 requested evidence",
                "request-",
            ),
        ] {
            let values = |count: usize| {
                (0..count)
                    .map(|index| format!("\"{prefix}{index:03}\""))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            assert_d2_decode_count_boundary(
                &base,
                field,
                &values(limit),
                &values(limit + 1),
                D2CountBoundary {
                    open: b'[',
                    close: b']',
                    operation,
                    limit,
                },
            );
        }

        let inference = |count: usize| {
            (0..count)
                .map(|index| format!("\"k{index:03}\":\"v\""))
                .collect::<Vec<_>>()
                .join(",")
        };
        assert_d2_decode_count_boundary(
            &base,
            "inference_settings",
            &inference(crate::execution::MAX_D2_INFERENCE_ENTRIES),
            &inference(crate::execution::MAX_D2_INFERENCE_ENTRIES + 1),
            D2CountBoundary {
                open: b'{',
                close: b'}',
                operation: "D2 inference settings",
                limit: crate::execution::MAX_D2_INFERENCE_ENTRIES,
            },
        );
    }

    #[test]
    fn d2_execution_is_atomic_retryable_and_offline_correct_metadata_without_bytes_is_shadow_only()
    {
        let (mut log, initial, review_plan, obligation_id, envelope, source_by_id) = d2_log();
        assert!(
            log.append(EventCommand::obligation_transition(
                obligation_id.clone(),
                ObligationLifecycle::Completed,
            ))
            .is_err()
        );

        let (failed_id, no_claim) = append_d2_attempt(
            &mut log,
            &review_plan,
            &obligation_id,
            &envelope,
            &source_by_id,
            1,
            crate::ExecutionOutcome::ProviderFailure {
                retryable: true,
                diagnostic: "retry fixture".to_owned(),
            },
        );
        assert!(no_claim.is_none());
        assert_eq!(log.aggregate().executions().count(), 1);
        assert_eq!(log.aggregate().execution_claims().count(), 0);
        assert_eq!(
            log.aggregate()
                .obligations()
                .find(|obligation| obligation.id() == &obligation_id)
                .unwrap()
                .lifecycle(),
            ObligationLifecycle::InProgress
        );
        assert!(
            log.append(EventCommand::obligation_transition(
                obligation_id.clone(),
                ObligationLifecycle::Completed,
            ))
            .is_err()
        );

        let (structured_id, claim_id) = append_d2_attempt(
            &mut log,
            &review_plan,
            &obligation_id,
            &envelope,
            &source_by_id,
            2,
            crate::ExecutionOutcome::Structured,
        );
        assert_ne!(failed_id, structured_id);
        assert!(claim_id.is_some());
        assert_eq!(log.aggregate().executions().count(), 2);
        assert_eq!(log.aggregate().execution_claims().count(), 1);
        log.append(EventCommand::obligation_transition(
            obligation_id.clone(),
            ObligationLifecycle::Completed,
        ))
        .unwrap();

        let envelopes = log.envelopes().cloned().collect::<Vec<_>>();
        let replayed = EventLog::replay(
            EventContractVersion::V2,
            log.run_id().clone(),
            initial.clone(),
            log.events(),
        )
        .unwrap();
        assert_eq!(replayed.aggregate().executions().count(), 2);
        assert_eq!(replayed.aggregate().execution_claims().count(), 1);

        let genesis = RunGenesisSnapshot::from_aggregate(&initial)
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let view = EventEnvelope::validated_view(
            EventContractVersion::V2,
            log.run_id(),
            EventStreamGenesis::V2(&genesis),
            &envelopes,
        )
        .unwrap();
        let mut offline = OfflineProjectionState::new(&view, initial).unwrap();
        for event in view.events() {
            match event.payload() {
                DecodedPayload::ReviewExecutionRecorded { .. } => {
                    assert!(!offline.apply(event).unwrap());
                }
                DecodedPayload::ObligationTransition {
                    next: ObligationLifecycle::Completed,
                    ..
                } => {
                    assert!(offline.apply(event).is_err());
                    break;
                }
                _ => {
                    offline.apply(event).unwrap();
                }
            }
        }
        assert_eq!(offline.aggregate().executions().count(), 0);
        assert_eq!(offline.aggregate().execution_claims().count(), 0);
        assert_eq!(
            offline
                .unreconciled_records()
                .into_iter()
                .filter(|metadata| { metadata.kind() == UnreconciledRecordKind::ReviewExecution })
                .count(),
            2
        );
        assert_eq!(
            offline
                .aggregate()
                .obligations()
                .find(|obligation| obligation.id() == &obligation_id)
                .unwrap()
                .lifecycle(),
            ObligationLifecycle::InProgress
        );
    }

    #[test]
    fn d2_duplicate_execution_and_atomic_claim_mismatch_leave_state_unchanged() {
        let (mut log, initial, review_plan, obligation_id, envelope, source_by_id) = d2_log();
        let (execution_id, _) = append_d2_attempt(
            &mut log,
            &review_plan,
            &obligation_id,
            &envelope,
            &source_by_id,
            1,
            crate::ExecutionOutcome::Structured,
        );
        let before_events = log.events().len();
        let before_executions = log.aggregate().executions().count();
        let before_claims = log.aggregate().execution_claims().count();

        let original_event = log
            .events()
            .iter()
            .rev()
            .find(|event| {
                event
                    .envelope()
                    .payload
                    .get()
                    .contains("review_execution_recorded")
            })
            .unwrap();
        let PersistedPayload::ReviewExecutionRecorded(recorded) =
            decode_canonical_payload(original_event.envelope().payload.get()).unwrap()
        else {
            unreachable!("selected D2 event")
        };
        let raw_closure = original_event.reviewer_raw_closure.clone().unwrap();
        assert_eq!(recorded.execution.id(), &execution_id);
        assert!(matches!(
            log.append(EventCommand {
                payload: PersistedPayload::ReviewExecutionRecorded(recorded.clone()),
                admission: CommandAdmission::ReviewerRaw(raw_closure.clone()),
            }),
            Err(DomainError::IdCollision { id }) if id == execution_id
        ));
        assert_eq!(log.events().len(), before_events);
        assert_eq!(log.aggregate().executions().count(), before_executions);
        assert_eq!(log.aggregate().execution_claims().count(), before_claims);

        let duplicate = forged_envelope(
            EventContractVersion::V2,
            log.run_id().clone(),
            log.genesis_hash().clone(),
            log.next_sequence().unwrap(),
            log.tail_hash().clone(),
            PersistedPayload::ReviewExecutionRecorded(recorded.clone()),
        );
        assert!(matches!(
            log.resume_envelopes(&[duplicate], &EventAdmissions::default()),
            Err(DomainError::IdCollision { id }) if id == execution_id
        ));
        assert_eq!(log.events().len(), before_events);
        assert_eq!(log.aggregate().executions().count(), before_executions);
        assert_eq!(log.aggregate().execution_claims().count(), before_claims);

        let duplicate = forged_envelope(
            EventContractVersion::V2,
            log.run_id().clone(),
            log.genesis_hash().clone(),
            log.next_sequence().unwrap(),
            log.tail_hash().clone(),
            PersistedPayload::ReviewExecutionRecorded(recorded.clone()),
        );
        let mut replay_events = log.events().to_vec();
        replay_events.push(Event {
            envelope: duplicate.clone(),
            evidence_admission: None,
            binding_admission: None,
            verification_admission: None,
            decision_admission: None,
            context_projection_admission: None,
            reviewer_raw_closure: Some(raw_closure),
        });
        assert!(matches!(
            EventLog::replay(
                EventContractVersion::V2,
                log.run_id().clone(),
                initial.clone(),
                &replay_events,
            ),
            Err(DomainError::IdCollision { id }) if id == execution_id
        ));

        let mut envelopes = log.envelopes().cloned().collect::<Vec<_>>();
        envelopes.push(duplicate);
        let genesis = RunGenesisSnapshot::from_aggregate(&initial)
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let view = EventEnvelope::validated_view(
            EventContractVersion::V2,
            log.run_id(),
            EventStreamGenesis::V2(&genesis),
            &envelopes,
        )
        .unwrap();
        let mut offline = OfflineProjectionState::new(&view, initial).unwrap();
        for event in &view.events()[..view.events().len() - 1] {
            offline.apply(event).unwrap();
        }
        let offline_tail = offline.tail_hash().clone();
        let offline_unreconciled = offline.unreconciled_records().len();
        assert!(matches!(
            offline.apply(view.events().last().unwrap()),
            Err(DomainError::IdCollision { id }) if id == execution_id
        ));
        assert_eq!(offline.tail_hash(), &offline_tail);
        assert_eq!(offline.unreconciled_records().len(), offline_unreconciled);
        assert_eq!(offline.aggregate().executions().count(), 0);
        assert_eq!(offline.aggregate().execution_claims().count(), 0);

        let mut mismatched = recorded;
        mismatched.claims.clear();
        let malformed = forged_envelope(
            EventContractVersion::V2,
            log.run_id().clone(),
            log.genesis_hash().clone(),
            log.next_sequence().unwrap(),
            log.tail_hash().clone(),
            PersistedPayload::ReviewExecutionRecorded(mismatched),
        );
        assert!(
            log.resume_envelopes(&[malformed], &EventAdmissions::default())
                .is_err()
        );
        assert_eq!(log.events().len(), before_events);
        assert_eq!(log.aggregate().executions().count(), before_executions);
        assert_eq!(log.aggregate().execution_claims().count(), before_claims);
    }

    #[test]
    fn d2_duplicate_claim_id_is_typed_and_atomic_at_live_resume_replay_and_offline_seams() {
        let (mut log, initial, review_plan, obligation_id, envelope, source_by_id) = d2_log();
        let wave = review_plan
            .waves()
            .iter()
            .find(|wave| wave.obligation_ids().contains(&obligation_id))
            .unwrap();
        let input = crate::ExecutionRecordInput::fake(
            review_plan.id().clone(),
            wave.id().clone(),
            obligation_id.clone(),
            envelope.id().clone(),
            envelope.snapshot_id().clone(),
            1,
        )
        .unwrap();
        let raw = br#"{"fixture":"duplicate-claim"}"#;
        let registration = ArtifactRegistered::reviewer_execution(
            log.run_id().clone(),
            input.execution_id().unwrap(),
            crate::execution::FAKE_REVIEWER_ID,
            ContentHash::sha256(raw),
            "application/json",
            u64::try_from(raw.len()).unwrap(),
        )
        .unwrap();
        log.append(EventCommand::artifact_registered(registration.clone()))
            .unwrap();
        let obligation = log
            .aggregate()
            .obligations()
            .find(|candidate| candidate.id() == &obligation_id)
            .unwrap();
        let claim = crate::ExecutionClaimInputV2::new(
            obligation.property_id(),
            obligation.normalized_target_refs().clone(),
            ClaimPolarity::IssueAbsent,
            "duplicate claim fixture",
            envelope.normalized_included_source_ids().clone(),
            BTreeSet::new(),
            BTreeSet::new(),
            None,
        )
        .unwrap();
        let source_buffers = source_by_id.values().collect::<Vec<_>>();
        let bundle = ValidatedExecutionBundle::fake(
            input,
            &registration,
            raw.to_vec(),
            source_buffers,
            vec![claim],
            crate::ExecutionOutcome::Structured,
        )
        .unwrap();
        let (mut recorded, raw_closure) = bundle.into_parts();
        let duplicate_id = recorded.claims[0].id().clone();
        recorded.claims.push(recorded.claims[0].clone());

        let before_events = log.events().len();
        let before_tail = log.tail_hash().clone();
        assert!(matches!(
            log.append(EventCommand {
                payload: PersistedPayload::ReviewExecutionRecorded(recorded.clone()),
                admission: CommandAdmission::ReviewerRaw(raw_closure.clone()),
            }),
            Err(DomainError::IdCollision { id }) if id == duplicate_id
        ));
        assert_eq!(log.events().len(), before_events);
        assert_eq!(log.tail_hash(), &before_tail);
        assert_eq!(log.aggregate().executions().count(), 0);
        assert_eq!(log.aggregate().execution_claims().count(), 0);

        let malformed = forged_envelope(
            EventContractVersion::V2,
            log.run_id().clone(),
            log.genesis_hash().clone(),
            log.next_sequence().unwrap(),
            log.tail_hash().clone(),
            PersistedPayload::ReviewExecutionRecorded(recorded),
        );
        assert!(matches!(
            log.resume_envelopes(std::slice::from_ref(&malformed), &EventAdmissions::default()),
            Err(DomainError::IdCollision { id }) if id == duplicate_id
        ));
        assert_eq!(log.events().len(), before_events);
        assert_eq!(log.tail_hash(), &before_tail);
        assert_eq!(log.aggregate().executions().count(), 0);
        assert_eq!(log.aggregate().execution_claims().count(), 0);

        let mut replay_events = log.events().to_vec();
        replay_events.push(Event {
            envelope: malformed.clone(),
            evidence_admission: None,
            binding_admission: None,
            verification_admission: None,
            decision_admission: None,
            context_projection_admission: None,
            reviewer_raw_closure: Some(raw_closure),
        });
        assert!(matches!(
            EventLog::replay(
                EventContractVersion::V2,
                log.run_id().clone(),
                initial.clone(),
                &replay_events,
            ),
            Err(DomainError::IdCollision { id }) if id == duplicate_id
        ));

        let mut envelopes = log.envelopes().cloned().collect::<Vec<_>>();
        envelopes.push(malformed);
        let genesis = RunGenesisSnapshot::from_aggregate(&initial)
            .unwrap()
            .canonical_bytes()
            .unwrap();
        assert!(matches!(
            EventEnvelope::validated_view(
                EventContractVersion::V2,
                log.run_id(),
                EventStreamGenesis::V2(&genesis),
                &envelopes,
            ),
            Err(DomainError::IdCollision { id }) if id == duplicate_id
        ));
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
        for event in log.envelopes() {
            assert_eq!(
                event.canonical_bytes().unwrap(),
                event
                    .canonical_bytes_for_index(MAX_D1_EVENT_LINE_BYTES - 1)
                    .unwrap()
            );
        }
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
        let event = v2_log("run:index-writer-equivalence")
            .envelopes()
            .next()
            .unwrap()
            .clone();
        let regular = event.canonical_bytes().unwrap();
        let indexed = event
            .canonical_bytes_for_index(MAX_D1_EVENT_LINE_BYTES - 1)
            .unwrap();
        assert_eq!(regular, indexed);
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

    fn nested_array_json(depth: usize) -> Vec<u8> {
        let mut json = Vec::with_capacity(depth.saturating_mul(2).saturating_add(1));
        json.extend(std::iter::repeat_n(b'[', depth));
        json.push(b'0');
        json.extend(std::iter::repeat_n(b']', depth));
        json
    }

    fn one_member_array_json(elements: usize) -> Vec<u8> {
        let mut json = Vec::with_capacity(elements.saturating_mul(2).saturating_add(8));
        json.extend_from_slice(b"{\"a\":[");
        for element in 0..elements {
            if element != 0 {
                json.push(b',');
            }
            json.push(b'0');
        }
        json.extend_from_slice(b"]}");
        json
    }

    #[test]
    fn event_json_structure_accepts_exact_depth_and_refuses_next_depth() {
        assert!(preflight_event_json_structure(&nested_array_json(MAX_EVENT_JSON_DEPTH)).is_ok());
        assert!(matches!(
            preflight_event_json_structure(&nested_array_json(MAX_EVENT_JSON_DEPTH + 1)),
            Err(DomainError::Incomplete {
                operation: "index event JSON structure",
                limit: MAX_EVENT_JSON_DEPTH,
                observed,
            }) if observed == MAX_EVENT_JSON_DEPTH + 1
        ));
    }

    #[test]
    fn event_json_structure_accepts_exact_fanout_and_refuses_next_value() {
        assert!(
            preflight_event_json_structure(&one_member_array_json(MAX_EVENT_JSON_VALUES - 1))
                .is_ok()
        );
        assert!(matches!(
            preflight_event_json_structure(&one_member_array_json(MAX_EVENT_JSON_VALUES)),
            Err(DomainError::Incomplete {
                operation: "index event JSON structure",
                limit: MAX_EVENT_JSON_VALUES,
                observed,
            }) if observed == MAX_EVENT_JSON_VALUES + 1
        ));
    }

    #[test]
    fn genesis_json_structure_uses_its_fixed_depth_and_fanout_caps() {
        assert!(
            preflight_genesis_json_structure(&nested_array_json(MAX_GENESIS_JSON_DEPTH)).is_ok()
        );
        assert!(matches!(
            preflight_genesis_json_structure(&nested_array_json(MAX_GENESIS_JSON_DEPTH + 1)),
            Err(DomainError::Incomplete {
                operation: "index genesis JSON structure",
                limit: MAX_GENESIS_JSON_DEPTH,
                observed,
            }) if observed == MAX_GENESIS_JSON_DEPTH + 1
        ));
        assert!(
            preflight_genesis_json_structure(&one_member_array_json(MAX_GENESIS_JSON_VALUES - 1))
                .is_ok()
        );
        assert!(matches!(
            preflight_genesis_json_structure(&one_member_array_json(MAX_GENESIS_JSON_VALUES)),
            Err(DomainError::Incomplete {
                operation: "index genesis JSON structure",
                limit: MAX_GENESIS_JSON_VALUES,
                observed,
            }) if observed == MAX_GENESIS_JSON_VALUES + 1
        ));
    }

    fn persisted_family_fanout_json(kind: &str, elements: usize) -> Vec<u8> {
        let mut json = format!("{{\"payload\":{{\"type\":\"{kind}\",\"padding\":[").into_bytes();
        for element in 0..elements {
            if element != 0 {
                json.push(b',');
            }
            json.push(b'0');
        }
        json.extend_from_slice(b"]}}");
        json
    }

    fn persisted_family_depth_json(kind: &str, nested: usize) -> Vec<u8> {
        let mut json = format!("{{\"payload\":{{\"type\":\"{kind}\",\"padding\":").into_bytes();
        json.extend(std::iter::repeat_n(b'[', nested));
        json.push(b'0');
        json.extend(std::iter::repeat_n(b']', nested));
        json.extend_from_slice(b"}}");
        json
    }

    #[test]
    fn every_persisted_family_hits_structural_caps_at_the_actual_index_decode_seam() {
        for kind in [
            "obligation_transition",
            "claim_proposed",
            "evidence_recorded",
            "evidence_bound",
            "verification_recorded",
            "decision_recorded",
            "finding_recorded",
            "run_genesis_manifest",
            "artifact_registered",
            "snapshot_sources_recorded",
            "review_plan_recorded",
            "context_envelope_projected",
        ] {
            // Three enclosing object members precede the array elements.
            let exact_values = persisted_family_fanout_json(kind, MAX_EVENT_JSON_VALUES - 3);
            assert!(
                preflight_event_json_structure(&exact_values).is_ok(),
                "{kind}"
            );
            assert!(!matches!(
                EventEnvelope::from_json_slice_for_index(&exact_values),
                Err(DomainError::Incomplete { .. })
            ));
            let over_values = persisted_family_fanout_json(kind, MAX_EVENT_JSON_VALUES - 2);
            assert!(matches!(
                EventEnvelope::from_json_slice_for_index(&over_values),
                Err(DomainError::Incomplete {
                    operation: "index event JSON structure",
                    limit: MAX_EVENT_JSON_VALUES,
                    observed,
                }) if observed == MAX_EVENT_JSON_VALUES + 1
            ));

            // The envelope and payload objects consume the first two levels.
            let exact_depth = persisted_family_depth_json(kind, MAX_EVENT_JSON_DEPTH - 2);
            assert!(
                preflight_event_json_structure(&exact_depth).is_ok(),
                "{kind}"
            );
            assert!(!matches!(
                EventEnvelope::from_json_slice_for_index(&exact_depth),
                Err(DomainError::Incomplete { .. })
            ));
            let over_depth = persisted_family_depth_json(kind, MAX_EVENT_JSON_DEPTH - 1);
            assert!(matches!(
                EventEnvelope::from_json_slice_for_index(&over_depth),
                Err(DomainError::Incomplete {
                    operation: "index event JSON structure",
                    limit: MAX_EVENT_JSON_DEPTH,
                    observed,
                }) if observed == MAX_EVENT_JSON_DEPTH + 1
            ));
        }
    }

    #[test]
    fn genesis_structural_caps_apply_at_the_actual_index_decode_seam() {
        let exact_values = one_member_array_json(MAX_GENESIS_JSON_VALUES - 1);
        assert!(!matches!(
            RunGenesisSnapshot::from_canonical_bytes_for_index(&exact_values),
            Err(DomainError::Incomplete { .. })
        ));
        assert!(matches!(
            RunGenesisSnapshot::from_canonical_bytes_for_index(&one_member_array_json(
                MAX_GENESIS_JSON_VALUES
            )),
            Err(DomainError::Incomplete {
                operation: "index genesis JSON structure",
                limit: MAX_GENESIS_JSON_VALUES,
                observed,
            }) if observed == MAX_GENESIS_JSON_VALUES + 1
        ));
        let exact_depth = nested_array_json(MAX_GENESIS_JSON_DEPTH);
        assert!(!matches!(
            RunGenesisSnapshot::from_canonical_bytes_for_index(&exact_depth),
            Err(DomainError::Incomplete { .. })
        ));
        assert!(matches!(
            RunGenesisSnapshot::from_canonical_bytes_for_index(&nested_array_json(
                MAX_GENESIS_JSON_DEPTH + 1
            )),
            Err(DomainError::Incomplete {
                operation: "index genesis JSON structure",
                limit: MAX_GENESIS_JSON_DEPTH,
                observed,
            }) if observed == MAX_GENESIS_JSON_DEPTH + 1
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
        let verified = VerifiedV2Genesis::from_canonical_bytes(log.run_id(), &bytes).unwrap();
        EventEnvelope::validated_view(
            EventContractVersion::V2,
            log.run_id(),
            EventStreamGenesis::V2Verified(&verified),
            &log.envelopes().cloned().collect::<Vec<_>>(),
        )
        .unwrap();
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
            assert!(verified.validate_manifest(log.run_id(), &tampered).is_err());
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
