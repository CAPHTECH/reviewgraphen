//! Closed, process-free M4 verifier descriptors.
//!
//! This crate deliberately owns no CAS, journal, runtime, reviewer, process,
//! network, workspace, path, command, environment, or tool API. It computes
//! only deterministic descriptor outputs; the later Store/session boundary is
//! responsible for authority, CAS registration, and durable append ordering.

#[path = "../../reviewgraphen-core/src/harness_source_v1.rs"]
mod harness_source_v1;

use reviewgraphen_core::{
    ClaimPolarity, ContentHash, ExecutionClaimV2, FIXTURE_DESCRIPTOR_ID, FIXTURE_HARNESS_ID,
    FIXTURE_HARNESS_REVISION, FIXTURE_PROCEDURE_ID, M4_PROPERTY_ID, Obligation, ProgramSpace,
    STATIC_DESCRIPTOR_ID, STATIC_PROCEDURE_ID, StableId, StaticFactEvaluationV1,
    evaluate_static_fact_v1,
};
use thiserror::Error;

/// Exact canonical witness bytes, with no trailing LF.
pub const FIXTURE_WITNESS_BYTES: &[u8] = b"{\"charge_count\":2,\"expected_max\":1,\"outcome\":\"witnessed\",\"schema\":\"reviewgraphen.test_witness_result.v1\",\"test_artifact_id\":\"test:double-submit\"}";
pub const FIXTURE_WITNESS_SIZE: usize = 145;
pub const FIXTURE_WITNESS_MEDIA_TYPE: &str =
    "application/vnd.reviewgraphen.test-witness+json;version=1";
pub const FIXTURE_HARNESS_SOURCE_HASH: &str =
    "sha256:74d708edd94103e3bab724c71df8c151a89ea613ad06fd8c6bd21ba78027848a";

/// The sole v2 verifier name. Selecting it is intentionally not authority to
/// resolve or execute Cargo.
pub const DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID: &str = "workspace.cargo_test@1";
/// The only outcome for the deferred workspace verifier seam.
pub const DEFERRED_WORKSPACE_CARGO_TEST_OUTCOME: &str = "unsupported";
/// The fixed, typed explanation for the deferred workspace verifier seam.
pub const DEFERRED_WORKSPACE_CARGO_TEST_REASON: &str = "workspace_cargo_test_deferred";

const STATIC_DESCRIPTOR: Descriptor = Descriptor::StaticFact;
const FIXTURE_DESCRIPTOR: Descriptor = Descriptor::FixtureTest;

/// Closed descriptor identity. There is no provider-configurable variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Descriptor {
    StaticFact,
    FixtureTest,
}

/// Immutable allow-list metadata. Every flag is false by construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescriptorCapabilities {
    pub network: bool,
    pub process: bool,
    pub workspace_write: bool,
}

/// Fixed, code-owned descriptor metadata.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescriptorMetadata {
    pub id: &'static str,
    pub procedure_version: &'static str,
    pub evidence_kind: &'static str,
    pub property_id: &'static str,
    pub fixed_harness: Option<FixedHarnessMetadata>,
    pub capabilities: DescriptorCapabilities,
}

/// Registry binding for the one checked-in process-free fixture harness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedHarnessMetadata {
    pub id: &'static str,
    pub revision: &'static str,
    pub source_hash: &'static str,
}

impl Descriptor {
    #[must_use]
    pub const fn metadata(self) -> DescriptorMetadata {
        match self {
            Self::StaticFact => DescriptorMetadata {
                id: STATIC_DESCRIPTOR_ID,
                procedure_version: STATIC_PROCEDURE_ID,
                evidence_kind: "static_fact",
                property_id: M4_PROPERTY_ID,
                fixed_harness: None,
                capabilities: DescriptorCapabilities {
                    network: false,
                    process: false,
                    workspace_write: false,
                },
            },
            Self::FixtureTest => DescriptorMetadata {
                id: FIXTURE_DESCRIPTOR_ID,
                procedure_version: FIXTURE_PROCEDURE_ID,
                evidence_kind: "test_witness",
                property_id: M4_PROPERTY_ID,
                fixed_harness: Some(FixedHarnessMetadata {
                    id: FIXTURE_HARNESS_ID,
                    revision: FIXTURE_HARNESS_REVISION,
                    source_hash: FIXTURE_HARNESS_SOURCE_HASH,
                }),
                capabilities: DescriptorCapabilities {
                    network: false,
                    process: false,
                    workspace_write: false,
                },
            },
        }
    }
}

/// Fail-closed errors for the narrow pure verifier surface.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum VerifierError {
    #[error(transparent)]
    Core(#[from] reviewgraphen_core::M4Error),
    #[error("unsupported verifier descriptor")]
    UnsupportedDescriptor,
    #[error("unsupported verifier property `{property_id}`")]
    UnsupportedProperty { property_id: String },
    #[error("fixture verifier supports only issue_present claims")]
    UnsupportedPolarity,
    #[error("fixture witness bytes are not the one compiled-in canonical result")]
    WitnessBytesMismatch,
    #[error("compiled fixture witness constant violates its exact contract")]
    FixtureConstantMismatch,
    #[error("checked-in fixture harness source hash differs from registry")]
    HarnessSourceMismatch,
    #[error("deferred workspace verifier request is schema-invalid: `{field}` is not allowed")]
    DeferredWorkspaceRequestSchema { field: &'static str },
    #[error("deferred workspace verifier request is schema-invalid: unsupported descriptor")]
    DeferredWorkspaceDescriptorSchema,
    #[error("deferred workspace unsupported record is invalid: `{field}`")]
    DeferredWorkspaceRecordInvalid { field: &'static str },
}

pub type Result<T> = std::result::Result<T, VerifierError>;

/// A request field which is forbidden by the deferred workspace verifier
/// descriptor. Presence is invalid even if its value is empty.
///
/// This closed list gives the v2 request validator a typed way to reject every
/// execution-control field before the seam can produce its typed result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferredWorkspaceRequestField<'a> {
    Command,
    Executable,
    Argv,
    Path,
    Cwd,
    Environment,
    Mount,
    Cache,
    Credential,
    Toolchain,
    ResourceLimit,
    Identity,
    /// An unknown schema field is also fail-closed.
    Other(&'a str),
}

impl DeferredWorkspaceRequestField<'_> {
    const fn name(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Executable => "executable",
            Self::Argv => "argv",
            Self::Path => "path",
            Self::Cwd => "cwd",
            Self::Environment => "environment",
            Self::Mount => "mount",
            Self::Cache => "cache",
            Self::Credential => "credential",
            Self::Toolchain => "toolchain",
            Self::ResourceLimit => "resource_limit",
            Self::Identity => "identity",
            Self::Other(_) => "unknown_field",
        }
    }
}

/// The narrow v2 input admitted by the deferred workspace verifier seam.
///
/// `descriptor_id` may be absent, in which case validation produces no record.
/// Its only non-null value is [`DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID`].
/// The request carries no executable data; callers that decoded such fields
/// must pass their presence through `forbidden_fields` and receive a schema
/// error rather than an unsupported outcome.
#[derive(Clone, Copy, Debug)]
pub struct DeferredWorkspaceVerifierRequest<'a> {
    pub descriptor_id: Option<&'a str>,
    pub request_id: &'a StableId,
    pub snapshot_id: &'a StableId,
    pub universe_id: &'a StableId,
    /// Opaque, untrusted workspace fixture bytes. The deferred seam may carry
    /// them only to prove that they cannot select process behavior; they are
    /// never parsed, resolved, executed, retained, or bound into the result.
    pub untrusted_fixture: &'a [u8],
    pub forbidden_fields: &'a [DeferredWorkspaceRequestField<'a>],
}

/// A field forbidden on the closed unsupported record itself.
///
/// These variants make process/executable/output/resource/obligation material
/// invalid data instead of optional metadata that a future caller could ignore.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeferredWorkspaceUnsupportedRecordField<'a> {
    ProcessIdentity(&'a str),
    ExecutableIdentity(&'a str),
    Output(&'a str),
    ResourceObservation(&'a str),
    ObligationBinding(&'a StableId),
    Other(&'a str),
}

impl DeferredWorkspaceUnsupportedRecordField<'_> {
    const fn name(self) -> &'static str {
        match self {
            Self::ProcessIdentity(_) => "process_identity",
            Self::ExecutableIdentity(_) => "executable_identity",
            Self::Output(_) => "output",
            Self::ResourceObservation(_) => "resource_observation",
            Self::ObligationBinding(_) => "obligation_binding",
            Self::Other(_) => "unknown_field",
        }
    }
}

/// Decoded form accepted only to validate the closed unsupported-record
/// contract. Successful validation returns [`DeferredWorkspaceUnsupportedRecord`],
/// whose private fields prevent adding execution data afterwards.
#[derive(Clone, Copy, Debug)]
pub struct DeferredWorkspaceUnsupportedRecordInput<'a> {
    pub id: &'a StableId,
    pub descriptor_id: &'a str,
    pub request_id: &'a StableId,
    pub snapshot_id: &'a StableId,
    pub universe_id: &'a StableId,
    pub outcome: &'a str,
    pub reason: &'a str,
    pub process_started: bool,
    pub executable_resolved: bool,
    pub verifier_observed: &'a [StableId],
    pub forbidden_field: Option<DeferredWorkspaceUnsupportedRecordField<'a>>,
}

/// The one closed, non-authoritative result permitted for a selected deferred
/// workspace verifier descriptor. It is neither a verifier observation nor
/// evidence material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeferredWorkspaceUnsupportedRecord {
    id: StableId,
    request_id: StableId,
    snapshot_id: StableId,
    universe_id: StableId,
}

impl DeferredWorkspaceUnsupportedRecord {
    /// Validates every fixed field, the empty observation numerator, and the
    /// exact stable-ID preimage before constructing the closed record.
    pub fn validate(input: DeferredWorkspaceUnsupportedRecordInput<'_>) -> Result<Self> {
        validate_deferred_workspace_id_kinds(
            input.request_id,
            input.snapshot_id,
            input.universe_id,
            |field| VerifierError::DeferredWorkspaceRecordInvalid { field },
        )?;
        if input.descriptor_id != DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID {
            return Err(VerifierError::DeferredWorkspaceRecordInvalid {
                field: "descriptor_id",
            });
        }
        if input.outcome != DEFERRED_WORKSPACE_CARGO_TEST_OUTCOME {
            return Err(VerifierError::DeferredWorkspaceRecordInvalid { field: "outcome" });
        }
        if input.reason != DEFERRED_WORKSPACE_CARGO_TEST_REASON {
            return Err(VerifierError::DeferredWorkspaceRecordInvalid { field: "reason" });
        }
        if input.process_started {
            return Err(VerifierError::DeferredWorkspaceRecordInvalid {
                field: "process_started",
            });
        }
        if input.executable_resolved {
            return Err(VerifierError::DeferredWorkspaceRecordInvalid {
                field: "executable_resolved",
            });
        }
        if !input.verifier_observed.is_empty() {
            return Err(VerifierError::DeferredWorkspaceRecordInvalid {
                field: "verifier_observed",
            });
        }
        if let Some(field) = input.forbidden_field {
            return Err(VerifierError::DeferredWorkspaceRecordInvalid {
                field: field.name(),
            });
        }

        let expected_id = deferred_workspace_unsupported_id(
            input.request_id,
            input.snapshot_id,
            input.universe_id,
        )?;
        if input.id != &expected_id {
            return Err(VerifierError::DeferredWorkspaceRecordInvalid { field: "id" });
        }
        Ok(Self {
            id: expected_id,
            request_id: input.request_id.clone(),
            snapshot_id: input.snapshot_id.clone(),
            universe_id: input.universe_id.clone(),
        })
    }

    /// Stable identity of this exact closed unsupported result.
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    #[must_use]
    pub const fn descriptor_id(&self) -> &'static str {
        DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID
    }

    #[must_use]
    pub fn request_id(&self) -> &StableId {
        &self.request_id
    }

    #[must_use]
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }

    #[must_use]
    pub fn universe_id(&self) -> &StableId {
        &self.universe_id
    }

    #[must_use]
    pub const fn outcome(&self) -> &'static str {
        DEFERRED_WORKSPACE_CARGO_TEST_OUTCOME
    }

    #[must_use]
    pub const fn reason(&self) -> &'static str {
        DEFERRED_WORKSPACE_CARGO_TEST_REASON
    }

    #[must_use]
    pub const fn process_started(&self) -> bool {
        false
    }

    #[must_use]
    pub const fn executable_resolved(&self) -> bool {
        false
    }

    /// The resolved-target verifier-observed numerator is permanently empty.
    #[must_use]
    pub fn verifier_observed(&self) -> &[StableId] {
        &[]
    }

    /// Canonical bytes of the exact StableId preimage. No schema tag, output,
    /// process identity, resource observation, or obligation binding is bound.
    #[must_use]
    pub fn id_preimage(&self) -> Vec<u8> {
        deferred_workspace_unsupported_preimage(
            &self.request_id,
            &self.snapshot_id,
            &self.universe_id,
        )
    }
}

/// Typed result of the deferred workspace verifier seam. The enum makes
/// disabled verification record-free and selected verification exactly one
/// closed unsupported record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeferredWorkspaceVerifierResolution {
    Disabled,
    Unsupported(DeferredWorkspaceUnsupportedRecord),
}

/// Validates and resolves the fixed deferred verifier seam without resolving
/// an executable, spawning a process, or producing observation/evidence data.
pub fn resolve_deferred_workspace_verifier(
    request: DeferredWorkspaceVerifierRequest<'_>,
) -> Result<DeferredWorkspaceVerifierResolution> {
    validate_deferred_workspace_id_kinds(
        request.request_id,
        request.snapshot_id,
        request.universe_id,
        |field| VerifierError::DeferredWorkspaceRequestSchema { field },
    )?;
    // This is intentionally the only interaction with fixture bytes. It is a
    // compiler-visible, process-free read: fixture contents cannot become a
    // command, executable, path, environment, or output channel.
    std::hint::black_box(request.untrusted_fixture);
    if let Some(field) = request.forbidden_fields.first().copied() {
        return Err(VerifierError::DeferredWorkspaceRequestSchema {
            field: field.name(),
        });
    }
    match request.descriptor_id {
        None => Ok(DeferredWorkspaceVerifierResolution::Disabled),
        Some(DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID) => {
            let id = deferred_workspace_unsupported_id(
                request.request_id,
                request.snapshot_id,
                request.universe_id,
            )?;
            let record = DeferredWorkspaceUnsupportedRecord::validate(
                DeferredWorkspaceUnsupportedRecordInput {
                    id: &id,
                    descriptor_id: DEFERRED_WORKSPACE_CARGO_TEST_DESCRIPTOR_ID,
                    request_id: request.request_id,
                    snapshot_id: request.snapshot_id,
                    universe_id: request.universe_id,
                    outcome: DEFERRED_WORKSPACE_CARGO_TEST_OUTCOME,
                    reason: DEFERRED_WORKSPACE_CARGO_TEST_REASON,
                    process_started: false,
                    executable_resolved: false,
                    verifier_observed: &[],
                    forbidden_field: None,
                },
            )?;
            Ok(DeferredWorkspaceVerifierResolution::Unsupported(record))
        }
        Some(_) => Err(VerifierError::DeferredWorkspaceDescriptorSchema),
    }
}

fn validate_deferred_workspace_id_kinds(
    request_id: &StableId,
    snapshot_id: &StableId,
    universe_id: &StableId,
    error: impl FnOnce(&'static str) -> VerifierError + Copy,
) -> Result<()> {
    if request_id.kind() != "request" {
        return Err(error("request_id"));
    }
    if snapshot_id.kind() != "snapshot" {
        return Err(error("snapshot_id"));
    }
    if universe_id.kind() != "universe" {
        return Err(error("universe_id"));
    }
    Ok(())
}

fn deferred_workspace_unsupported_id(
    request_id: &StableId,
    snapshot_id: &StableId,
    universe_id: &StableId,
) -> Result<StableId> {
    let preimage = deferred_workspace_unsupported_preimage(request_id, snapshot_id, universe_id);
    StableId::parse(format!(
        "verifier-unsupported:{}",
        ContentHash::sha256(&preimage)
    ))
    .map_err(|_| VerifierError::DeferredWorkspaceRecordInvalid { field: "id" })
}

fn deferred_workspace_unsupported_preimage(
    request_id: &StableId,
    snapshot_id: &StableId,
    universe_id: &StableId,
) -> Vec<u8> {
    // All interpolated values are StableId grammar values and the remaining
    // strings are fixed ASCII constants, so this is canonical JSON without a
    // general JSON serializer or a permissive decoder.
    format!(
        concat!(
            "{{\"descriptor_id\":\"workspace.cargo_test@1\",",
            "\"executable_resolved\":false,\"outcome\":\"unsupported\",",
            "\"process_started\":false,",
            "\"reason\":\"workspace_cargo_test_deferred\",",
            "\"request_id\":\"{}\",\"snapshot_id\":\"{}\",",
            "\"universe_id\":\"{}\"}}"
        ),
        request_id, snapshot_id, universe_id
    )
    .into_bytes()
}

/// Resolves only the two compiled descriptors. It treats every other string
/// as data and never interprets it as a command, path, profile, or tool call.
pub fn descriptor_by_id(descriptor_id: &str) -> Result<Descriptor> {
    match descriptor_id {
        STATIC_DESCRIPTOR_ID => Ok(STATIC_DESCRIPTOR),
        FIXTURE_DESCRIPTOR_ID => Ok(FIXTURE_DESCRIPTOR),
        _ => Err(VerifierError::UnsupportedDescriptor),
    }
}

/// Returns every available descriptor in deterministic registry order.
#[must_use]
pub const fn descriptors() -> [Descriptor; 2] {
    [STATIC_DESCRIPTOR, FIXTURE_DESCRIPTOR]
}

/// Runs the core-owned static applicability algorithm. This method cannot
/// mint a verification bundle, append events, or create accepted state.
pub fn evaluate_static(
    program: &ProgramSpace,
    obligation: &Obligation,
    claim: &ExecutionClaimV2,
) -> Result<StaticFactEvaluationV1> {
    Ok(evaluate_static_fact_v1(program, obligation, claim)?)
}

/// Pure, non-authoritative fixture witness validation result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedFixtureWitnessV1 {
    claim_id: StableId,
    subject_ids: Vec<StableId>,
    witness_hash: ContentHash,
}

/// Exact, non-authoritative output from the closed fixture harness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedFixtureHarnessOutputV1 {
    witness_bytes: Vec<u8>,
    witness_hash: ContentHash,
    media_type: &'static str,
    property_id: &'static str,
    polarity: ClaimPolarity,
    test_artifact_id: StableId,
}

impl FixedFixtureHarnessOutputV1 {
    #[must_use]
    pub fn witness_bytes(&self) -> &[u8] {
        &self.witness_bytes
    }

    #[must_use]
    pub fn witness_hash(&self) -> &ContentHash {
        &self.witness_hash
    }

    #[must_use]
    pub const fn media_type(&self) -> &'static str {
        self.media_type
    }

    #[must_use]
    pub const fn property_id(&self) -> &'static str {
        self.property_id
    }

    #[must_use]
    pub const fn polarity(&self) -> ClaimPolarity {
        self.polarity
    }

    #[must_use]
    pub fn test_artifact_id(&self) -> &StableId {
        &self.test_artifact_id
    }
}

impl FixedFixtureWitnessV1 {
    #[must_use]
    pub fn claim_id(&self) -> &StableId {
        &self.claim_id
    }

    #[must_use]
    pub fn subject_ids(&self) -> &[StableId] {
        &self.subject_ids
    }

    #[must_use]
    pub fn witness_hash(&self) -> &ContentHash {
        &self.witness_hash
    }
}

/// Checks the exact source text of the fixed harness against the code-owned
/// registry. This check neither runs nor locates a harness.
pub fn verify_harness_source() -> Result<()> {
    verify_harness_source_bytes(include_bytes!(
        "../../reviewgraphen-core/src/harness_source_v1.rs"
    ))
}

fn verify_harness_source_bytes(source_bytes: &[u8]) -> Result<()> {
    let actual = ContentHash::sha256(source_bytes);
    if actual.as_str() != FIXTURE_HARNESS_SOURCE_HASH {
        return Err(VerifierError::HarnessSourceMismatch);
    }
    if harness_source_v1::HARNESS_ID != FIXTURE_HARNESS_ID
        || harness_source_v1::HARNESS_REVISION != FIXTURE_HARNESS_REVISION
    {
        return Err(VerifierError::HarnessSourceMismatch);
    }
    Ok(())
}

fn verify_harness_output_bytes(witness_bytes: &[u8]) -> Result<()> {
    if witness_bytes.len() != FIXTURE_WITNESS_SIZE
        || witness_bytes != FIXTURE_WITNESS_BYTES
        || ContentHash::sha256(witness_bytes).as_str() != reviewgraphen_core::FIXTURE_WITNESS_HASH
    {
        return Err(VerifierError::FixtureConstantMismatch);
    }
    Ok(())
}

/// Verifies the fixed literal itself before it is used as a witness.
pub fn verify_fixture_constants() -> Result<()> {
    if FIXTURE_WITNESS_MEDIA_TYPE != reviewgraphen_core::FIXTURE_MEDIA_TYPE {
        return Err(VerifierError::FixtureConstantMismatch);
    }
    verify_harness_output_bytes(FIXTURE_WITNESS_BYTES)?;
    verify_harness_source()?;
    verify_harness_output_bytes(&harness_source_v1::run_duplicate_submit_harness())
}

/// Executes the only registered fixture semantics in-process. This output is
/// descriptive data, not a capability to append evidence or accept a claim.
pub fn execute_fixed_fixture_harness() -> Result<FixedFixtureHarnessOutputV1> {
    verify_fixture_constants()?;
    let witness_bytes = harness_source_v1::run_duplicate_submit_harness();
    let test_artifact_id = StableId::parse(reviewgraphen_core::FIXTURE_TEST_ARTIFACT_ID)
        .map_err(|_| VerifierError::FixtureConstantMismatch)?;
    Ok(FixedFixtureHarnessOutputV1 {
        witness_hash: ContentHash::sha256(&witness_bytes),
        witness_bytes,
        media_type: FIXTURE_WITNESS_MEDIA_TYPE,
        property_id: M4_PROPERTY_ID,
        polarity: ClaimPolarity::IssuePresent,
        test_artifact_id,
    })
}

/// Validates only the one compiled fixture witness. The returned value is
/// intentionally not an authority capability and cannot produce evidence,
/// verification, acceptance, a finding, or a durable write on its own.
pub fn validate_fixed_fixture_witness(
    claim: &ExecutionClaimV2,
    witness_bytes: &[u8],
) -> Result<FixedFixtureWitnessV1> {
    let execution = execute_fixed_fixture_harness()?;
    if claim.property_id() != M4_PROPERTY_ID {
        return Err(VerifierError::UnsupportedProperty {
            property_id: claim.property_id().to_owned(),
        });
    }
    if claim.polarity() != ClaimPolarity::IssuePresent {
        return Err(VerifierError::UnsupportedPolarity);
    }
    if witness_bytes != execution.witness_bytes() {
        return Err(VerifierError::WitnessBytesMismatch);
    }
    let mut subject_ids = claim.target_refs().iter().cloned().collect::<Vec<_>>();
    subject_ids.push(execution.test_artifact_id().clone());
    subject_ids.sort_unstable();
    subject_ids.dedup();
    Ok(FixedFixtureWitnessV1 {
        claim_id: claim.id().clone(),
        subject_ids,
        witness_hash: execution.witness_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reviewgraphen_core::{
        ArtifactRegistered, ExecutionClaimInputV2, ExecutionOutcome, ExecutionRecordInput,
        FAKE_REVIEWER_ID, ProgramSpace, StaticApplicabilityV1, ValidatedExecutionBundle,
        VerificationOutcomeV3, canonical_json,
    };
    use serde_json::{Value, json};
    use std::collections::BTreeSet;

    const FIXTURE: &[u8] =
        include_bytes!("../../../examples/double-submit-payment/program-space.json");

    fn program() -> ProgramSpace {
        ProgramSpace::from_json_slice(FIXTURE).unwrap()
    }

    fn static_obligation(program: &ProgramSpace) -> Obligation {
        serde_json::from_value(json!({
            "id":"obligation:verifier-static",
            "target_kind":"artifact",
            "target_refs":["artifact:target"],
            "normalized_target_refs":["artifact:target"],
            "semantic_key":"payment.at_most_once|artifact:target",
            "property_id":M4_PROPERTY_ID,
            "property_version":"1",
            "context_ids":["context:ui-event"],
            "normalized_context_ids":["context:ui-event"],
            "required_capabilities":[],
            "evidence_required":true,
            "accepted_evidence_modes":["static_fact"],
            "applicability_status":"applicable",
            "applicability_reasons":[],
            "qualification_ids":[],
            "weight":1.0,
            "version":{
                "profile":"code-review@1",
                "rule":"payment-at-most-once@1",
                "extractor_set":"sha256:01234567",
                "snapshot":program.snapshot_id()
            },
            "lifecycle":"generated",
            "depends_on":[],
            "normalized_depends_on":[],
            "generator_ids":[],
            "source_ids":["artifact:source"],
            "normalized_source_ids":["artifact:source"]
        }))
        .unwrap()
    }

    fn execution_claim(
        obligation: &Obligation,
        property_id: &str,
        polarity: ClaimPolarity,
    ) -> ExecutionClaimV2 {
        let input = ExecutionRecordInput::fake(
            StableId::parse("plan:verifier").unwrap(),
            StableId::parse("schedule-wave:verifier").unwrap(),
            obligation.id().clone(),
            StableId::parse("context-envelope:verifier").unwrap(),
            StableId::parse("snapshot:double-submit-v1").unwrap(),
            1,
        )
        .unwrap();
        let raw = br#"{\"claims\":[{\"fixture\":true}]}"#;
        let registration = ArtifactRegistered::reviewer_execution(
            StableId::parse("run:verifier").unwrap(),
            input.execution_id().unwrap(),
            FAKE_REVIEWER_ID,
            ContentHash::sha256(raw),
            "application/json",
            u64::try_from(raw.len()).unwrap(),
        )
        .unwrap();
        let source = b"verifier fixture source".to_vec();
        let claim_input = ExecutionClaimInputV2::new(
            property_id,
            obligation.normalized_target_refs().clone(),
            polarity,
            "fixture claim",
            obligation.normalized_source_ids().clone(),
            BTreeSet::new(),
            BTreeSet::new(),
            Some(0.99),
        )
        .unwrap();
        ValidatedExecutionBundle::fake(
            input,
            &registration,
            raw.to_vec(),
            vec![&source],
            vec![claim_input],
            ExecutionOutcome::Structured,
        )
        .unwrap()
        .claims()[0]
            .clone()
    }

    #[test]
    fn registry_is_closed_deterministic_and_process_free() {
        assert_eq!(
            descriptors(),
            [Descriptor::StaticFact, Descriptor::FixtureTest]
        );
        for descriptor in descriptors() {
            let metadata = descriptor.metadata();
            assert_eq!(descriptor_by_id(metadata.id).unwrap(), descriptor);
            assert_eq!(metadata.property_id, M4_PROPERTY_ID);
            match descriptor {
                Descriptor::StaticFact => assert_eq!(metadata.fixed_harness, None),
                Descriptor::FixtureTest => assert_eq!(
                    metadata.fixed_harness,
                    Some(FixedHarnessMetadata {
                        id: FIXTURE_HARNESS_ID,
                        revision: FIXTURE_HARNESS_REVISION,
                        source_hash: FIXTURE_HARNESS_SOURCE_HASH,
                    })
                ),
            }
            assert!(!metadata.capabilities.network);
            assert!(!metadata.capabilities.process);
            assert!(!metadata.capabilities.workspace_write);
        }
        assert!(matches!(
            descriptor_by_id("sh -c ignored"),
            Err(VerifierError::UnsupportedDescriptor)
        ));
        assert_eq!(
            descriptor_by_id(&"profile=custom;argv=sh;cwd=/;env=all;network=true".repeat(128)),
            Err(VerifierError::UnsupportedDescriptor)
        );
    }

    #[test]
    fn fixture_literal_and_checked_in_harness_are_exact() {
        assert_eq!(FIXTURE_WITNESS_BYTES.len(), FIXTURE_WITNESS_SIZE);
        assert_eq!(
            ContentHash::sha256(FIXTURE_WITNESS_BYTES).as_str(),
            reviewgraphen_core::FIXTURE_WITNESS_HASH
        );
        verify_fixture_constants().unwrap();

        let output = execute_fixed_fixture_harness().unwrap();
        assert_eq!(output.witness_bytes(), FIXTURE_WITNESS_BYTES);
        assert_eq!(output.witness_bytes().len(), 145);
        assert_eq!(
            output.witness_hash().as_str(),
            reviewgraphen_core::FIXTURE_WITNESS_HASH
        );
        assert_eq!(output.media_type(), FIXTURE_WITNESS_MEDIA_TYPE);
        assert_eq!(output.property_id(), M4_PROPERTY_ID);
        assert_eq!(output.polarity(), ClaimPolarity::IssuePresent);
        assert_eq!(
            output.test_artifact_id().as_str(),
            reviewgraphen_core::FIXTURE_TEST_ARTIFACT_ID
        );

        assert_eq!(
            verify_harness_source_bytes(b"mutated source"),
            Err(VerifierError::HarnessSourceMismatch)
        );
        let mut mutated_output = output.witness_bytes().to_vec();
        mutated_output[0] = b'[';
        assert_eq!(
            verify_harness_output_bytes(&mutated_output),
            Err(VerifierError::FixtureConstantMismatch)
        );
    }

    #[test]
    fn fixture_witness_rejects_wrong_claim_or_one_byte_mutation() {
        let program = program();
        let obligation = static_obligation(&program);
        let claim = execution_claim(&obligation, M4_PROPERTY_ID, ClaimPolarity::IssuePresent);
        let accepted = validate_fixed_fixture_witness(&claim, FIXTURE_WITNESS_BYTES).unwrap();
        assert_eq!(accepted.claim_id(), claim.id());
        assert_eq!(
            accepted.subject_ids(),
            [
                StableId::parse("artifact:target").unwrap(),
                StableId::parse(reviewgraphen_core::FIXTURE_TEST_ARTIFACT_ID).unwrap(),
            ]
        );

        let mut changed = FIXTURE_WITNESS_BYTES.to_vec();
        changed[0] = b'[';
        assert_eq!(
            validate_fixed_fixture_witness(&claim, &changed),
            Err(VerifierError::WitnessBytesMismatch)
        );

        let wrong_property =
            execution_claim(&obligation, "other.property", ClaimPolarity::IssuePresent);
        assert!(matches!(
            validate_fixed_fixture_witness(&wrong_property, FIXTURE_WITNESS_BYTES),
            Err(VerifierError::UnsupportedProperty { .. })
        ));
        let wrong_polarity =
            execution_claim(&obligation, M4_PROPERTY_ID, ClaimPolarity::IssueAbsent);
        assert_eq!(
            validate_fixed_fixture_witness(&wrong_polarity, FIXTURE_WITNESS_BYTES),
            Err(VerifierError::UnsupportedPolarity)
        );
    }

    #[test]
    fn static_evaluation_covers_cardinality_bounds_and_order_determinism() {
        let program = program();
        let obligation = static_obligation(&program);
        let claim = execution_claim(&obligation, M4_PROPERTY_ID, ClaimPolarity::IssuePresent);
        let first = evaluate_static(&program, &obligation, &claim).unwrap();
        let second = evaluate_static(&program, &obligation, &claim).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.result().applicability(),
            StaticApplicabilityV1::Absent
        );
        assert_ne!(first.result().outcome(), VerificationOutcomeV3::Passed);
        assert!(first.result().canonical_bytes().unwrap().len() <= 65_536);
        assert!(canonical_json(first.input()).unwrap().len() <= 65_536);

        let mut unique_value: Value = serde_json::from_slice(FIXTURE).unwrap();
        unique_value["invariants"][0]["scope_ids"] = json!(["context:ui-event"]);
        let unique_program: ProgramSpace = serde_json::from_value(unique_value.clone()).unwrap();
        let unique = evaluate_static(&unique_program, &obligation, &claim).unwrap();
        assert_eq!(
            unique.result().applicability(),
            StaticApplicabilityV1::Unique
        );
        assert_ne!(unique.result().outcome(), VerificationOutcomeV3::Passed);

        let mut second_invariant = unique_value["invariants"][0].clone();
        second_invariant["id"] = json!("invariant:verifier-second");
        unique_value["invariants"]
            .as_array_mut()
            .unwrap()
            .push(second_invariant);
        let ambiguous_program: ProgramSpace = serde_json::from_value(unique_value).unwrap();
        let ambiguous = evaluate_static(&ambiguous_program, &obligation, &claim).unwrap();
        assert_eq!(
            ambiguous.result().applicability(),
            StaticApplicabilityV1::Ambiguous
        );
        assert_ne!(ambiguous.result().outcome(), VerificationOutcomeV3::Passed);

        let unsupported_claim =
            execution_claim(&obligation, "other.property", ClaimPolarity::IssuePresent);
        let unsupported = evaluate_static(&program, &obligation, &unsupported_claim).unwrap();
        assert_eq!(
            unsupported.result().applicability(),
            StaticApplicabilityV1::UnsupportedProperty
        );
        assert_eq!(
            unsupported.result().outcome(),
            VerificationOutcomeV3::Unsupported
        );
    }
}
