use crate::{
    ContentHash, Decision, DecisionAdmission, DomainError, Evidence, EvidenceAdmission,
    EvidenceBinding, EvidenceSnapshotAdmission, Finding, ObligationLifecycle, Result,
    ReviewAggregate, ReviewClaim, StableId, TrustedHumanAdmission, Verification, canonical_json,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

const EVENT_SCHEMA: &str = "reviewgraphen.review_event.v1";
const SYSTEM_ACTOR: &str = "reviewgraphen-core@1";

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
}

#[derive(Default)]
struct MatchedAdmissions {
    evidence: Option<EvidenceAdmission>,
    binding: Option<EvidenceBindingAdmission>,
    verification: Option<VerificationAdmission>,
    decision: Option<DecisionAdmission>,
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
        }
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
}

impl PersistedPayload {
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
        }
    }
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
    payload: Value,
    payload_hash: ContentHash,
    previous_event_hash: ContentHash,
    event_hash: ContentHash,
}

impl EventEnvelope {
    fn new(
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
        let payload =
            serde_json::to_value(payload).map_err(|error| DomainError::Json(error.to_string()))?;
        let payload_hash = ContentHash::sha256(&canonical_json(&payload)?);
        let id = event_id(
            &run_id,
            &genesis_hash,
            sequence,
            &actor,
            logical_time,
            &payload_hash,
            &previous_event_hash,
        )?;
        let event_hash = envelope_hash(EventHashInput {
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
            schema: EVENT_SCHEMA.to_owned(),
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
        Ok(envelope)
    }

    /// Imports and structurally validates one JSON envelope. Applying it still
    /// requires replay with its matching run-bound decision admissions.
    pub fn from_json_slice(input: &[u8]) -> Result<Self> {
        serde_json::from_slice(input).map_err(|error| DomainError::Json(error.to_string()))
    }

    /// Validates deterministic envelope bindings and every nested untrusted
    /// DTO before the record can enter a replay stream.
    pub fn validate(&self) -> Result<()> {
        if self.schema != EVENT_SCHEMA {
            return Err(DomainError::EventSequence(
                "unsupported event schema".to_owned(),
            ));
        }
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
        let payload = decode_canonical_payload(self.payload.clone())?;
        if self.actor != payload.actor() {
            return Err(DomainError::EventSequence(
                "event actor must match the typed payload authority".to_owned(),
            ));
        }
        let expected_hash = ContentHash::sha256(&canonical_json(&self.payload)?);
        if self.payload_hash != expected_hash {
            return Err(DomainError::EventSequence(
                "event payload hash does not match payload".to_owned(),
            ));
        }
        let expected_id = event_id(
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
        Ok(())
    }

    /// Validates a contiguous prefix, including the requested run even for an
    /// empty stream and the exact initial aggregate hash for every event.
    pub fn validate_sequence(
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
    payload: Value,
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
struct RawPayloadHeader {
    #[serde(rename = "type")]
    kind: String,
    data: Value,
}

fn decode_payload(value: Value) -> Result<PersistedPayload> {
    let raw: RawPayloadHeader =
        serde_json::from_value(value).map_err(|error| DomainError::Json(error.to_string()))?;
    match raw.kind.as_str() {
        "obligation_transition" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct RawTransition {
                obligation_id: StableId,
                next: ObligationLifecycle,
            }
            let data: RawTransition = serde_json::from_value(raw.data)
                .map_err(|error| DomainError::Json(error.to_string()))?;
            Ok(PersistedPayload::ObligationTransition {
                obligation_id: data.obligation_id,
                next: data.next,
            })
        }
        "claim_proposed" => Ok(PersistedPayload::ClaimProposed(
            ReviewClaim::from_event_value(raw.data)?,
        )),
        "evidence_recorded" => Ok(PersistedPayload::EvidenceRecorded(Box::new(
            Evidence::from_event_value(raw.data)?,
        ))),
        "evidence_bound" => Ok(PersistedPayload::EvidenceBound(
            EvidenceBinding::from_event_value(raw.data)?,
        )),
        "verification_recorded" => Ok(PersistedPayload::VerificationRecorded(
            Verification::from_event_value(raw.data)?,
        )),
        "decision_recorded" => Ok(PersistedPayload::DecisionRecorded(
            Decision::from_event_value(raw.data)?,
        )),
        "finding_recorded" => Ok(PersistedPayload::FindingRecorded(
            Finding::from_event_value(raw.data)?,
        )),
        _ => Err(DomainError::EventSequence(
            "unknown persisted event payload type".to_owned(),
        )),
    }
}

fn decode_canonical_payload(value: Value) -> Result<PersistedPayload> {
    let payload = decode_payload(value.clone())?;
    let canonical_raw = canonical_json(&value)?;
    let canonical_typed = canonical_json(&payload)?;
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
        if run_id.kind() != "run" {
            return Err(DomainError::EventSequence(
                "event stream requires a run ID even when empty".to_owned(),
            ));
        }
        initial.validate()?;
        initial.validate_pristine_for_event_log()?;
        let genesis_hash = ContentHash::sha256(&canonical_json(&initial)?);
        let tail_hash = event_chain_genesis_hash(&run_id, &genesis_hash)?;
        Ok(Self {
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
        let (payload, admission) = self.normalized_payload(command)?;
        let sequence = self.next_sequence()?;
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
            _ => {}
        }
        let actor = payload.actor().to_owned();
        let envelope = EventEnvelope::new(
            self.run_id.clone(),
            self.genesis_hash.clone(),
            sequence,
            actor,
            sequence,
            self.tail_hash.clone(),
            payload,
        )?;
        let (evidence_admission, binding_admission, verification_admission, decision_admission) =
            match admission {
                CommandAdmission::Evidence(admission) => (Some(admission), None, None, None),
                CommandAdmission::Binding(admission) => (None, Some(admission), None, None),
                CommandAdmission::Verification(admission) => (None, None, Some(admission), None),
                CommandAdmission::Decision(admission) => (None, None, None, Some(admission)),
                CommandAdmission::None => (None, None, None, None),
            };
        self.append_envelope(
            envelope,
            evidence_admission,
            binding_admission,
            verification_admission,
            decision_admission,
        )
    }

    /// Replays exact in-process events from the same initial aggregate.
    pub fn replay(run_id: StableId, initial: ReviewAggregate, events: &[Event]) -> Result<Self> {
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
        Self::replay_envelopes(
            run_id,
            initial,
            &envelopes,
            &EventAdmissions::new(evidence, decisions)
                .with_trace_admissions(bindings, verifications),
        )
    }

    /// Replays JSON-imported envelopes only with matching exact host admissions.
    pub fn replay_envelopes(
        run_id: StableId,
        initial: ReviewAggregate,
        envelopes: &[EventEnvelope],
        admissions: &EventAdmissions,
    ) -> Result<Self> {
        let mut log = Self::new(run_id, initial)?;
        EventEnvelope::validate_sequence(&log.run_id, &log.genesis_hash, envelopes)?;
        for envelope in envelopes {
            let payload = decode_canonical_payload(envelope.payload.clone())?;
            let admissions = log.admissions_for(&payload, admissions)?;
            log.append_envelope(
                envelope.clone(),
                admissions.evidence,
                admissions.binding,
                admissions.verification,
                admissions.decision,
            )?;
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
        for envelope in envelopes {
            let payload = decode_canonical_payload(envelope.payload.clone())?;
            let admissions = self.admissions_for(&payload, admissions)?;
            self.append_envelope(
                envelope.clone(),
                admissions.evidence,
                admissions.binding,
                admissions.verification,
                admissions.decision,
            )?;
        }
        Ok(())
    }

    fn append_envelope(
        &mut self,
        envelope: EventEnvelope,
        evidence_admission: Option<EvidenceAdmission>,
        binding_admission: Option<EvidenceBindingAdmission>,
        verification_admission: Option<VerificationAdmission>,
        decision_admission: Option<DecisionAdmission>,
    ) -> Result<&Event> {
        let expected_sequence = self.next_sequence()?;
        envelope.validate()?;
        if envelope.run_id != self.run_id
            || envelope.genesis_hash != self.genesis_hash
            || envelope.sequence != expected_sequence
            || envelope.previous_event_hash != self.tail_hash
        {
            return Err(DomainError::EventSequence(
                "event run ID, genesis hash, and sequence must continue this log".to_owned(),
            ));
        }
        let payload = decode_canonical_payload(envelope.payload.clone())?;
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
        let mut next = self.aggregate.clone();
        apply(&mut next, &payload, envelope.actor())?;
        self.aggregate = next;
        self.events.push(Event {
            envelope,
            evidence_admission,
            binding_admission,
            verification_admission,
            decision_admission,
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

    fn admissions_for(
        &self,
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
        Ok(MatchedAdmissions {
            evidence,
            binding,
            verification,
            decision,
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

    /// Stable run binding required when minting exact replay admissions.
    #[must_use]
    pub fn run_id(&self) -> &StableId {
        &self.run_id
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
}

fn event_id(
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
        ("schema".to_owned(), Value::String(EVENT_SCHEMA.to_owned())),
        (
            "sequence".to_owned(),
            Value::Number(serde_json::Number::from(sequence)),
        ),
    ]);
    StableId::derived("event", &bindings)
}

struct EventHashInput<'a> {
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
        ("schema".to_owned(), Value::String(EVENT_SCHEMA.to_owned())),
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

fn apply(aggregate: &mut ReviewAggregate, payload: &PersistedPayload, actor: &str) -> Result<()> {
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
    }
}
