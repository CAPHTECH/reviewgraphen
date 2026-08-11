//! Strict, opt-in fixtures for cross-crate integration tests.
//!
//! This module is deliberately feature-gated.  It creates a fresh V4 run by
//! using the same public Core and Store admission APIs as a host would; it
//! never writes envelopes or CAS paths directly.  Returned values are
//! descriptive fixture material, not capabilities or expected assertions.

use crate::{
    CasHash, CasStore, EventJournal, JournalError, JournalGenesis, JournalIdentity, StoreError,
    StoreRoot,
};
use base64::{Engine as _, engine::general_purpose::STANDARD_NO_PAD};
use reviewgraphen_core::{
    ArtifactRegisteredV3, ArtifactSensitivity, ArtifactSourceV3, AssignmentValueV4,
    AuthorityTrustRootsV4, ClaimPolarity, ContentHash, ContextError, EventCommand, EventLog,
    EventLogV4, ExecutionClaimInputV2, ExecutionOutcome, ExecutionRecordInput, FAKE_REVIEWER_ID,
    M4_PROPERTY_ID, M4Error, M5DoubleSubmitAssignmentsV4, MvpRulePack, ObligationLifecycle,
    PlanBudget, ProgramSpace, ReviewAggregate, RunGenesisBootstrapRequestV4, SnapshotSourceBundle,
    SnapshotSourceEntry, SnapshotSourceRecordEntry, SnapshotSourcesRecorded, StableId,
    ValidatedExecutionBundle, VerificationBundleRequestV4, evaluate_static_fact_v1, plan,
    prepare_context,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use thiserror::Error;

/// Wire discriminator for the committed source-bound M5 integration fixture.
pub const M5_V4_FIXTURE_SCHEMA: &str = "reviewgraphen.test_fixture.m5_m4_prefix.v1";
const M5_V4_FIXTURE_MANIFEST_B64: &str = include_str!("../fixtures/m5-m4-prefix-v4.manifest.b64");

/// Strict artifact inventory entry.  The bytes are retained so every
/// materialization rechecks the exact CAS hash, size, media type and
/// sensitivity before publication.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureArtifactV4 {
    pub cas_hash: ContentHash,
    /// Standard unpadded base64 of the exact CAS object bytes.
    pub bytes_base64: String,
    pub media_type: String,
    pub sensitivity: ArtifactSensitivity,
    pub size: u64,
}

impl FixtureArtifactV4 {
    fn bytes(&self) -> Result<Vec<u8>, FixtureError> {
        let bytes = STANDARD_NO_PAD
            .decode(&self.bytes_base64)
            .map_err(|_| FixtureError::ArtifactIntegrity)?;
        if STANDARD_NO_PAD.encode(&bytes) != self.bytes_base64 {
            return Err(FixtureError::ArtifactIntegrity);
        }
        Ok(bytes)
    }

    fn validate(&self) -> Result<(), FixtureError> {
        let bytes = self.bytes()?;
        if self.size != u64::try_from(bytes.len()).map_err(|_| FixtureError::Size)?
            || self.cas_hash != ContentHash::sha256(&bytes)
            || self.media_type.is_empty()
        {
            return Err(FixtureError::ArtifactIntegrity);
        }
        Ok(())
    }
}

/// Exact public constructor inputs for the replay roots.  M4 static fact
/// verification needs no harness or human grant, hence both collections stay
/// empty in this deliberately minimal prefix.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureBaseRootsV4 {
    pub policy_revision_hash: ContentHash,
    pub repository_id: StableId,
    pub repository_source_hash: ContentHash,
}

impl FixtureBaseRootsV4 {
    /// Recreates fresh public V4 host roots.  This does not add an authority
    /// or reuse a replay basis.
    pub fn build(&self) -> Result<AuthorityTrustRootsV4, FixtureError> {
        Ok(AuthorityTrustRootsV4::new(
            self.policy_revision_hash.clone(),
            self.repository_id.clone(),
            self.repository_source_hash.clone(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )?)
    }
}

/// Closed input to the profile-owned M5 operation.  It is separate from the
/// base roots because assignments are neither evidence nor authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureAssignmentsV4 {
    pub payment: FixtureAssignmentValueV4,
    pub ui_event: FixtureAssignmentValueV4,
}

/// Strict wire form for Core's intentionally serialize-only assignment enum.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureAssignmentValueV4 {
    Satisfied,
    Required,
    Unknown,
}

impl From<FixtureAssignmentValueV4> for AssignmentValueV4 {
    fn from(value: FixtureAssignmentValueV4) -> Self {
        match value {
            FixtureAssignmentValueV4::Satisfied => Self::Satisfied,
            FixtureAssignmentValueV4::Required => Self::Required,
            FixtureAssignmentValueV4::Unknown => Self::Unknown,
        }
    }
}

impl FixtureAssignmentsV4 {
    pub fn build(&self) -> Result<M5DoubleSubmitAssignmentsV4, FixtureError> {
        Ok(M5DoubleSubmitAssignmentsV4::new(
            self.payment.into(),
            self.ui_event.into(),
        ))
    }
}

/// Versioned manifest containing the exact source-bound V4 M4 prefix.  The
/// canonical genesis and envelopes are preserved as bytes so tests can audit
/// regeneration without rebuilding an expected event ID or prose string.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct M5M4PrefixFixtureManifestV4 {
    pub artifacts: Vec<FixtureArtifactV4>,
    pub assignments: FixtureAssignmentsV4,
    pub base_roots: FixtureBaseRootsV4,
    /// Standard unpadded base64 of the canonical V4 genesis bytes.
    pub canonical_genesis_base64: String,
    /// Standard unpadded base64 of every canonical V4 envelope in order.
    pub canonical_envelopes_base64: Vec<String>,
    pub run_id: StableId,
    pub schema: String,
}

impl M5M4PrefixFixtureManifestV4 {
    /// Strictly loads the committed fixture source.  The outer base64 and the
    /// decoded canonical JSON must both round-trip byte-for-byte before any
    /// Store operation occurs.
    pub fn committed() -> Result<Self, FixtureError> {
        let bytes = decode_canonical_base64(M5_V4_FIXTURE_MANIFEST_B64)?;
        let manifest: Self = serde_json::from_slice(&bytes)?;
        if reviewgraphen_core::canonical_json(&manifest)? != bytes {
            return Err(FixtureError::Manifest);
        }
        manifest.validate()?;
        Ok(manifest)
    }

    fn genesis_bytes(&self) -> Result<Vec<u8>, FixtureError> {
        decode_canonical_base64(&self.canonical_genesis_base64)
    }

    fn envelope_bytes(&self) -> Result<Vec<Vec<u8>>, FixtureError> {
        self.canonical_envelopes_base64
            .iter()
            .map(|value| decode_canonical_base64(value))
            .collect()
    }

    /// Validates exact bytes and strict-canonical event/genesis shape before
    /// a Store root is touched.
    pub fn validate(&self) -> Result<(), FixtureError> {
        if self.schema != M5_V4_FIXTURE_SCHEMA || self.run_id.kind() != "run" {
            return Err(FixtureError::Manifest);
        }
        self.base_roots.build()?;
        self.assignments.build()?;
        for artifact in &self.artifacts {
            artifact.validate()?;
        }
        let genesis = self.genesis_bytes()?;
        let bootstrap = EventLogV4::from_bootstrap_request(RunGenesisBootstrapRequestV4::new(
            self.run_id.clone(),
            genesis.clone(),
            fixture_repository_identity(&genesis)?,
            fixture_snapshot_id(&genesis)?,
            fixture_profile_id(&genesis)?,
            fixture_profile_version(&genesis)?,
        )?)?;
        let actual = bootstrap
            .envelopes()
            .iter()
            .map(|event| event.canonical_bytes())
            .collect::<Result<Vec<_>, _>>()?;
        if self.canonical_envelopes_base64.is_empty()
            || self
                .canonical_envelopes_base64
                .first()
                .map(|value| decode_canonical_base64(value))
                .transpose()?
                .as_ref()
                != actual.first()
        {
            return Err(FixtureError::Manifest);
        }
        let envelopes = self
            .envelope_bytes()?
            .iter()
            .map(|bytes| reviewgraphen_core::EventEnvelope::from_json_slice(bytes))
            .collect::<Result<Vec<_>, _>>()?;
        reviewgraphen_core::EventEnvelope::validate_v4_stream(&self.run_id, &genesis, &envelopes)?;
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, FixtureError> {
        self.validate()?;
        Ok(reviewgraphen_core::canonical_json(self)?)
    }
}

/// Fresh Store materialization of a validated fixture.  It intentionally owns
/// neither a replay session nor a capability; callers reopen through the
/// normal `EventJournal` API and construct roots from the manifest inputs.
pub struct MaterializedM5M4PrefixFixtureV4<'a> {
    journal: EventJournal<'a>,
    manifest: M5M4PrefixFixtureManifestV4,
}

impl<'a> MaterializedM5M4PrefixFixtureV4<'a> {
    pub fn journal(&self) -> &EventJournal<'a> {
        &self.journal
    }

    pub fn into_parts(self) -> (EventJournal<'a>, FixtureBaseRootsV4, FixtureAssignmentsV4) {
        (
            self.journal,
            self.manifest.base_roots,
            self.manifest.assignments,
        )
    }

    pub fn manifest(&self) -> &M5M4PrefixFixtureManifestV4 {
        &self.manifest
    }
}

#[derive(Debug, Error)]
pub enum FixtureError {
    #[error(transparent)]
    Core(#[from] reviewgraphen_core::DomainError),
    #[error(transparent)]
    M5(#[from] reviewgraphen_core::M5Error),
    #[error(transparent)]
    M4(#[from] M4Error),
    #[error(transparent)]
    Context(#[from] ContextError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("unable to create the isolated fixture workspace: {0}")]
    Temp(#[source] std::io::Error),
    #[error("fixture manifest is not the supported strict V4 M4 prefix")]
    Manifest,
    #[error("fixture artifact bytes, hash, size, media type, or sensitivity are inconsistent")]
    ArtifactIntegrity,
    #[error("fixture byte length does not fit u64")]
    Size,
}

/// Builds the fixture manifest and materializes its fresh, fully verified M4
/// static prefix through public Core+Store operations.
pub fn materialize_m5_m4_prefix_v4<'a>(
    root: &'a StoreRoot,
) -> Result<MaterializedM5M4PrefixFixtureV4<'a>, FixtureError> {
    let manifest = M5M4PrefixFixtureManifestV4::committed()?;
    manifest.validate()?;
    for artifact in &manifest.artifacts {
        artifact.validate()?;
        let bytes = artifact.bytes()?;
        CasStore::open(root)?.put(
            &CasHash::parse(artifact.cas_hash.to_string())?,
            Some(artifact.size),
            Cursor::new(bytes),
        )?;
    }
    let genesis = manifest.genesis_bytes()?;
    let bootstrap = EventLogV4::from_bootstrap_request(RunGenesisBootstrapRequestV4::new(
        manifest.run_id.clone(),
        genesis.clone(),
        fixture_repository_identity(&genesis)?,
        fixture_snapshot_id(&genesis)?,
        fixture_profile_id(&genesis)?,
        fixture_profile_version(&genesis)?,
    )?)?;
    let (journal, _) = EventJournal::publish_new_v4(root, bootstrap)?;
    let roots = manifest.base_roots.build()?;
    let (mut session, mut basis) = journal.replayed_v4_session(&roots)?;
    // The non-genesis envelopes are created by the sealed public admission
    // paths below.  Compare their canonical bytes afterwards, instead of
    // accepting preconstructed fixture envelopes as append authority.
    let derived_artifacts = build_m4_prefix(root, &mut session, &mut basis, &manifest)?;
    drop(session);
    let (_, source_by_id) = fixture_program()?;
    let mut expected_artifacts = source_by_id
        .into_values()
        .map(|bytes| fixture_artifact(bytes, "text/plain", ArtifactSensitivity::WorkspaceSource))
        .collect::<Result<Vec<_>, _>>()?;
    expected_artifacts.extend(derived_artifacts);
    expected_artifacts.sort_by(|left, right| left.cas_hash.cmp(&right.cas_hash));
    if expected_artifacts != manifest.artifacts {
        return Err(FixtureError::Manifest);
    }
    let observed = journal
        .reader()?
        .events()
        .iter()
        .map(|event| event.canonical_bytes())
        .collect::<Result<Vec<_>, _>>()?;
    if observed
        .iter()
        .map(|bytes| STANDARD_NO_PAD.encode(bytes))
        .collect::<Vec<_>>()
        != manifest.canonical_envelopes_base64
    {
        return Err(FixtureError::Manifest);
    }
    Ok(MaterializedM5M4PrefixFixtureV4 { journal, manifest })
}

/// Builds a fully canonical D2-through-M4 stream whose declared profile is
/// deliberately foreign to the closed M5 Runtime operation. This is test
/// support for proving profile binding; unlike a corrupt-manifest negative,
/// every genesis, envelope, artifact and authority replay is valid.
pub fn materialize_foreign_profile_m5_m4_prefix_v4<'a>(
    root: &'a StoreRoot,
) -> Result<MaterializedM5M4PrefixFixtureV4<'a>, FixtureError> {
    let mut manifest = build_fixture_manifest_with_profile("code-review")?;
    materialize_source_manifest(root, &mut manifest)?;
    manifest.validate()?;
    let identity = JournalIdentity::new(
        manifest.run_id.clone(),
        JournalGenesis::V4(manifest.genesis_bytes()?),
    )?;
    let journal = EventJournal::open(root, identity)?;
    Ok(MaterializedM5M4PrefixFixtureV4 { journal, manifest })
}

/// Deterministically regenerates all source-bound bytes.  This routine does
/// not publish anything and does not accept a caller-provided record ID.
pub fn regenerate_m5_m4_prefix_manifest_v4() -> Result<M5M4PrefixFixtureManifestV4, FixtureError> {
    let workspace = tempfile::tempdir().map_err(FixtureError::Temp)?;
    let root = StoreRoot::open(workspace.path(), crate::StoreLimits::default())?;
    let mut manifest = build_fixture_manifest()?;
    materialize_source_manifest(&root, &mut manifest)?;
    manifest.validate()?;
    Ok(manifest)
}

fn fixture_program() -> Result<(ProgramSpace, BTreeMap<StableId, Vec<u8>>), FixtureError> {
    fixture_program_with_profile("double-submit-payment")
}

fn fixture_program_with_profile(
    profile_id: &str,
) -> Result<(ProgramSpace, BTreeMap<StableId, Vec<u8>>), FixtureError> {
    let mut value: Value = serde_json::from_slice(include_bytes!(
        "../../../examples/double-submit-payment/program-space.json"
    ))?;
    value["profile"]["id"] = Value::String(profile_id.to_owned());
    value["profile"]["version"] = Value::String("1".to_owned());
    for context in value["contexts"]
        .as_array_mut()
        .ok_or(FixtureError::Manifest)?
    {
        let source_id = if context["id"] == reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID {
            Some("file:checkout-controller")
        } else if context["id"] == reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID {
            Some("file:payment-repository")
        } else {
            None
        };
        if let Some(source_id) = source_id {
            let members = context["member_ids"]
                .as_array_mut()
                .ok_or(FixtureError::Manifest)?;
            members.push(Value::String(source_id.to_owned()));
            members.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
            members.dedup_by(|left, right| left == right);
        }
    }
    let test = value["artifacts"]
        .as_array_mut()
        .ok_or(FixtureError::Manifest)?
        .iter_mut()
        .find(|artifact| artifact["id"] == reviewgraphen_core::FIXTURE_TEST_ARTIFACT_ID)
        .ok_or(FixtureError::Manifest)?;
    test["location"]["start_line"] = Value::Null;
    test["location"]["end_line"] = Value::Null;
    let invariant = value["invariants"]
        .as_array_mut()
        .ok_or(FixtureError::Manifest)?
        .iter_mut()
        .find(|invariant| invariant["property_id"] == M4_PROPERTY_ID)
        .ok_or(FixtureError::Manifest)?;
    invariant["scope_ids"] = serde_json::json!([
        reviewgraphen_core::DOUBLE_SUBMIT_PAYMENT_CONTEXT_ID,
        reviewgraphen_core::DOUBLE_SUBMIT_UI_CONTEXT_ID,
    ]);
    value["evidence"] = serde_json::json!([{
        "id": "evidence:fixture-seeded-payment-source",
        "kind": "static_analysis",
        "target_ids": ["function:payment-charge"],
        "artifact_ref": null,
        "content_hash": null,
        "attributes": {"fixture": true},
        "provenance": value["artifacts"][0]["provenance"].clone(),
    }]);
    let mut contains = value["relations"][0].clone();
    contains["id"] = Value::String("relation:fixture-file-contains-payment-charge".to_owned());
    contains["kind"] = Value::String("contains".to_owned());
    contains["source_id"] = Value::String("file:payment-repository".to_owned());
    contains["target_ids"] = serde_json::json!(["function:payment-charge"]);
    contains["directed"] = Value::Bool(true);
    value["relations"]
        .as_array_mut()
        .ok_or(FixtureError::Manifest)?
        .push(contains);
    let sources = BTreeMap::from([
        ("src/checkout_controller.rs", b"checkout\n".repeat(40)),
        ("src/payment_repository.rs", b"repository\n".repeat(40)),
    ]);
    let mut by_id = BTreeMap::new();
    for artifact in value["artifacts"]
        .as_array_mut()
        .ok_or(FixtureError::Manifest)?
    {
        if artifact["kind"] == "file" {
            let path = artifact["location"]["path"]
                .as_str()
                .ok_or(FixtureError::Manifest)?;
            let bytes = sources.get(path).ok_or(FixtureError::Manifest)?.clone();
            artifact["content_hash"] = Value::String(ContentHash::sha256(&bytes).to_string());
            by_id.insert(
                StableId::parse(artifact["id"].as_str().ok_or(FixtureError::Manifest)?)?,
                bytes,
            );
        }
    }
    Ok((
        ProgramSpace::from_json_slice(&serde_json::to_vec(&value)?)?,
        by_id,
    ))
}

/// Runs the source manifest through the ordinary Store path exactly once and
/// captures only canonical, already-durable bytes afterwards.  It accepts no
/// envelopes as input and therefore cannot turn a fixture file into append
/// authority.
fn materialize_source_manifest(
    root: &StoreRoot,
    manifest: &mut M5M4PrefixFixtureManifestV4,
) -> Result<(), FixtureError> {
    if !manifest.canonical_envelopes_base64.is_empty() {
        return Err(FixtureError::Manifest);
    }
    for artifact in &manifest.artifacts {
        put_fixture_artifact(root, artifact)?;
    }
    let bootstrap = EventLogV4::from_bootstrap_request(RunGenesisBootstrapRequestV4::new(
        manifest.run_id.clone(),
        manifest.genesis_bytes()?,
        fixture_repository_identity(&manifest.genesis_bytes()?)?,
        fixture_snapshot_id(&manifest.genesis_bytes()?)?,
        fixture_profile_id(&manifest.genesis_bytes()?)?,
        fixture_profile_version(&manifest.genesis_bytes()?)?,
    )?)?;
    let (journal, _) = EventJournal::publish_new_v4(root, bootstrap)?;
    let roots = manifest.base_roots.build()?;
    let (mut session, mut basis) = journal.replayed_v4_session(&roots)?;
    let mut derived = build_m4_prefix(root, &mut session, &mut basis, manifest)?;
    drop(session);
    let envelopes = journal
        .reader()?
        .events()
        .iter()
        .map(|event| event.canonical_bytes())
        .collect::<Result<Vec<_>, _>>()?;
    manifest.artifacts.append(&mut derived);
    manifest
        .artifacts
        .sort_by(|left, right| left.cas_hash.cmp(&right.cas_hash));
    if manifest
        .artifacts
        .windows(2)
        .any(|pair| pair[0].cas_hash == pair[1].cas_hash)
    {
        return Err(FixtureError::Manifest);
    }
    manifest.canonical_envelopes_base64 = envelopes
        .iter()
        .map(|bytes| STANDARD_NO_PAD.encode(bytes))
        .collect();
    Ok(())
}

fn build_fixture_manifest() -> Result<M5M4PrefixFixtureManifestV4, FixtureError> {
    build_fixture_manifest_with_profile("double-submit-payment")
}

fn build_fixture_manifest_with_profile(
    profile_id: &str,
) -> Result<M5M4PrefixFixtureManifestV4, FixtureError> {
    // Materialize once in a private, public-API-only in-memory workflow.  The
    // exact published stream is captured by a separate Store root in tests;
    // this construction produces the deterministic source and root inputs.
    let (program, source_by_id) = fixture_program_with_profile(profile_id)?;
    let (universe, obligations) = MvpRulePack::synthesize(&program)?.into_parts();
    let aggregate = ReviewAggregate::new(program.clone(), universe, obligations)?;
    let run_id = StableId::parse("run:m5-m4-prefix-fixture")?;
    let log = EventLog::new_v3(run_id.clone(), aggregate)?;
    let genesis = log.run_genesis_snapshot()?.canonical_bytes()?;
    let base_roots = FixtureBaseRootsV4 {
        policy_revision_hash: ContentHash::sha256(b"reviewgraphen-m5-fixture-policy-v1"),
        repository_id: program.repository_id().clone(),
        repository_source_hash: fixture_repository_source_hash()?,
    };
    let mut artifacts = source_by_id
        .values()
        .cloned()
        .map(|bytes| FixtureArtifactV4 {
            cas_hash: ContentHash::sha256(&bytes),
            size: bytes.len() as u64,
            bytes_base64: STANDARD_NO_PAD.encode(bytes),
            media_type: "text/plain".to_owned(),
            sensitivity: ArtifactSensitivity::WorkspaceSource,
        })
        .collect::<Vec<_>>();
    artifacts.sort_by(|left, right| left.cas_hash.cmp(&right.cas_hash));
    // Raw reviewer and static verifier bytes are source-bound artifacts too.
    // They are appended by `build_m4_prefix`; the canonical envelope sequence
    // is filled by `materialize` after the same public admissions run.
    Ok(M5M4PrefixFixtureManifestV4 {
        artifacts,
        assignments: FixtureAssignmentsV4 {
            payment: FixtureAssignmentValueV4::Required,
            ui_event: FixtureAssignmentValueV4::Satisfied,
        },
        base_roots,
        canonical_genesis_base64: STANDARD_NO_PAD.encode(genesis),
        canonical_envelopes_base64: Vec::new(),
        run_id,
        schema: M5_V4_FIXTURE_SCHEMA.to_owned(),
    })
}

fn build_m4_prefix(
    root: &StoreRoot,
    session: &mut crate::ReplayedV4RunSession<'_, '_>,
    basis: &mut reviewgraphen_core::AuthorityReplayBasisV4,
    manifest: &M5M4PrefixFixtureManifestV4,
) -> Result<Vec<FixtureArtifactV4>, FixtureError> {
    let program = fixture_genesis(&manifest.genesis_bytes()?)?
        .rebuild_aggregate()?
        .program()
        .clone();
    let (_, source_by_id) = fixture_program()?;
    let (universe, obligations) = MvpRulePack::synthesize(&program)?.into_parts();
    let initial = ReviewAggregate::new(program.clone(), universe, obligations)?;
    let mut local = EventLog::new_v3(manifest.run_id.clone(), initial)?;
    let snapshot_id = program.snapshot_id().clone();
    let mut registrations = Vec::new();
    let mut source_entries = Vec::new();
    let mut bundle_entries = Vec::new();
    for artifact in program
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
    {
        let bytes = source_by_id
            .get(&artifact.id)
            .ok_or(FixtureError::Manifest)?;
        let hash = ContentHash::sha256(bytes);
        let path = artifact
            .location
            .as_ref()
            .ok_or(FixtureError::Manifest)?
            .path
            .clone();
        let registration = ArtifactRegisteredV3::new(
            manifest.run_id.clone(),
            hash.clone(),
            "text/plain",
            u64::try_from(bytes.len()).map_err(|_| FixtureError::Size)?,
            ArtifactSensitivity::WorkspaceSource,
            ArtifactSourceV3::SnapshotIngest {
                adapter_id: "reviewgraphen-fixture-source@1".to_owned(),
                run_id: manifest.run_id.clone(),
                snapshot_id: snapshot_id.clone(),
            },
        )?;
        source_entries.push(SnapshotSourceRecordEntry::new(
            artifact.id.clone(),
            path.clone(),
            hash.clone(),
            registration.registration_id().clone(),
            hash.clone(),
            line_count(bytes),
        )?);
        bundle_entries.push(SnapshotSourceEntry::new(
            artifact.id.clone(),
            path,
            hash.clone(),
            hash,
            bytes.clone(),
        ));
        registrations.push((artifact.id.clone(), registration));
    }
    source_entries.sort_by(|left, right| left.path().cmp(right.path()));
    let source_record = SnapshotSourcesRecorded::new(snapshot_id, source_entries)?;
    let source_bundle = SnapshotSourceBundle::new(&program, bundle_entries)?;
    for (_, registration) in &registrations {
        local.append(EventCommand::artifact_registered_v3(registration.clone()))?;
    }
    local.append(EventCommand::snapshot_sources_recorded(
        source_record.clone(),
    ))?;
    let review_plan = plan(local.aggregate(), PlanBudget::new(16, 16)?)?;
    local.append(EventCommand::review_plan_recorded(review_plan.clone()))?;
    let (obligation_id, built) = review_plan
        .waves()
        .iter()
        .flat_map(|wave| wave.obligation_ids())
        .find_map(|candidate| {
            let obligation = local
                .aggregate()
                .obligations()
                .find(|obligation| obligation.id() == candidate)?;
            if obligation.property_id() != M4_PROPERTY_ID {
                return None;
            }
            let mut context = prepare_context(local.aggregate(), candidate.clone()).ok()?;
            while let Some(request) = context.next_source_request().ok()? {
                context
                    .submit_source(&request, source_by_id.get(request.artifact_id())?)
                    .ok()?;
            }
            let built = context.finish().ok()?;
            (!built.envelope().normalized_included_source_ids().is_empty())
                .then(|| (candidate.clone(), built))
        })
        .ok_or(FixtureError::Manifest)?;
    let wave_id = review_plan
        .waves()
        .iter()
        .find(|wave| wave.obligation_ids().contains(&obligation_id))
        .ok_or(FixtureError::Manifest)?
        .id()
        .clone();
    let envelope = built.envelope().clone();
    let mut v4_context = prepare_context(local.aggregate(), obligation_id.clone())?;
    while let Some(request) = v4_context.next_source_request()? {
        let bytes = source_by_id
            .get(request.artifact_id())
            .ok_or(FixtureError::Manifest)?;
        v4_context.submit_source(&request, bytes)?;
    }
    let v4_built = v4_context.finish()?;
    local.append(EventCommand::obligation_transition(
        obligation_id.clone(),
        ObligationLifecycle::Planned,
    ))?;
    local.append(EventCommand::obligation_transition(
        obligation_id.clone(),
        ObligationLifecycle::InProgress,
    ))?;
    local.append(EventCommand::context_envelope_projected(built))?;
    let execution_input = ExecutionRecordInput::fake(
        review_plan.id().clone(),
        wave_id,
        obligation_id.clone(),
        envelope.id().clone(),
        envelope.snapshot_id().clone(),
        1,
    )?;
    let execution_id = execution_input.execution_id()?;
    let raw = reviewgraphen_core::canonical_json(&BTreeMap::from([
        (
            "execution_id".to_owned(),
            Value::String(execution_id.to_string()),
        ),
        (
            "schema".to_owned(),
            Value::String("reviewgraphen.fixture_raw.v1".to_owned()),
        ),
    ]))?;
    let raw_artifact = fixture_artifact(
        raw.clone(),
        "application/json",
        ArtifactSensitivity::Sensitive,
    )?;
    put_fixture_artifact(root, &raw_artifact)?;
    let raw_registration = ArtifactRegisteredV3::new(
        manifest.run_id.clone(),
        ContentHash::sha256(&raw),
        "application/json",
        u64::try_from(raw.len()).map_err(|_| FixtureError::Size)?,
        ArtifactSensitivity::Sensitive,
        ArtifactSourceV3::ReviewerExecution {
            execution_id,
            reviewer_id: FAKE_REVIEWER_ID.to_owned(),
            run_id: manifest.run_id.clone(),
        },
    )?;
    let obligation = local
        .aggregate()
        .obligations()
        .find(|item| item.id() == &obligation_id)
        .ok_or(FixtureError::Manifest)?;
    let claim_inputs = [
        StableId::parse("file:checkout-controller")?,
        StableId::parse("file:payment-repository")?,
    ]
    .into_iter()
    .map(|source_id| {
        ExecutionClaimInputV2::new(
            obligation.property_id(),
            obligation.normalized_target_refs().clone(),
            ClaimPolarity::IssuePresent,
            format!("fixture:{}:{}", obligation.property_id(), source_id),
            BTreeSet::from([source_id]),
            BTreeSet::new(),
            BTreeSet::new(),
            Some(1.0),
        )
    })
    .collect::<Result<Vec<_>, _>>()?;
    let source_buffers = source_by_id.values().collect::<Vec<_>>();
    let execution = ValidatedExecutionBundle::fake_v3(
        execution_input,
        &raw_registration,
        raw,
        source_buffers,
        claim_inputs,
        ExecutionOutcome::Structured,
    )?;
    let claim_ids = execution
        .claims()
        .iter()
        .map(|claim| claim.id().clone())
        .collect::<Vec<_>>();
    if claim_ids.len() != 2 {
        return Err(FixtureError::Manifest);
    }
    local.append(EventCommand::artifact_registered_v3(
        raw_registration.clone(),
    ))?;
    local.append(EventCommand::review_execution_recorded(execution.clone()))?;
    let evaluations = claim_ids
        .iter()
        .map(|claim_id| {
            let claim_record = local
                .aggregate()
                .execution_claims()
                .find(|item| item.id() == claim_id)
                .ok_or(FixtureError::Manifest)?;
            Ok((
                claim_id.clone(),
                evaluate_static_fact_v1(
                    local.aggregate().program(),
                    local
                        .aggregate()
                        .obligations()
                        .find(|item| item.id() == &obligation_id)
                        .ok_or(FixtureError::Manifest)?,
                    claim_record,
                )?,
            ))
        })
        .collect::<Result<Vec<_>, FixtureError>>()?;

    for (artifact_id, registration) in registrations {
        let prepared = session.prepare_snapshot_artifact_registration(
            registration,
            &source_bundle,
            &artifact_id,
            basis,
        )?;
        session.append_inherited_artifact_registration(prepared, basis)?;
    }
    append_d2(
        session,
        EventCommand::snapshot_sources_recorded(source_record),
        basis,
    )?;
    append_d2(
        session,
        EventCommand::review_plan_recorded(review_plan),
        basis,
    )?;
    append_d2(
        session,
        EventCommand::obligation_transition(obligation_id.clone(), ObligationLifecycle::Planned),
        basis,
    )?;
    append_d2(
        session,
        EventCommand::obligation_transition(obligation_id, ObligationLifecycle::InProgress),
        basis,
    )?;
    append_d2(
        session,
        EventCommand::context_envelope_projected(v4_built),
        basis,
    )?;
    let raw_admission =
        session.prepare_reviewer_raw_artifact_registration(raw_registration, &execution, basis)?;
    session.append_inherited_artifact_registration(raw_admission, basis)?;
    append_d2(
        session,
        EventCommand::review_execution_recorded(execution),
        basis,
    )?;

    let mut derived = vec![raw_artifact];
    for (claim_id, evaluation) in evaluations {
        let input_bytes = evaluation.input().canonical_bytes()?;
        let output_bytes = evaluation.result().canonical_bytes()?;
        let input_artifact = fixture_artifact(
            input_bytes.clone(),
            "application/vnd.reviewgraphen.static-fact-input+json;version=1",
            ArtifactSensitivity::CanonicalState,
        )?;
        let output_artifact = fixture_artifact(
            output_bytes.clone(),
            "application/vnd.reviewgraphen.static-fact-result+json;version=1",
            ArtifactSensitivity::CanonicalState,
        )?;
        put_fixture_artifact(root, &input_artifact)?;
        put_fixture_artifact(root, &output_artifact)?;
        let input = session.prepare_static_verifier_input_registration(
            &claim_id,
            ContentHash::sha256(&input_bytes),
            u64::try_from(input_bytes.len()).map_err(|_| FixtureError::Size)?,
            basis,
        )?;
        let input_receipt = session.append_inherited_artifact_registration(input, basis)?;
        let output = session.prepare_static_verifier_output_registration(
            &claim_id,
            ContentHash::sha256(&output_bytes),
            u64::try_from(output_bytes.len()).map_err(|_| FixtureError::Size)?,
            basis,
        )?;
        let output_receipt = session.append_inherited_artifact_registration(output, basis)?;
        let bundle = session.mint_verification_bundle(
            VerificationBundleRequestV4::static_fact(
                claim_id,
                input_receipt.core().registration_id().clone(),
                output_receipt.core().registration_id().clone(),
            ),
            None,
            basis,
        )?;
        session.append_verification_bundle(bundle, basis)?;
        derived.push(input_artifact);
        derived.push(output_artifact);
    }
    Ok(derived)
}

fn append_d2(
    session: &mut crate::ReplayedV4RunSession<'_, '_>,
    command: EventCommand,
    basis: &mut reviewgraphen_core::AuthorityReplayBasisV4,
) -> Result<(), FixtureError> {
    let prepared = session.prepare_inherited_d2_event(command, basis)?;
    session.append_inherited_d2_event(prepared, basis)?;
    Ok(())
}

fn fixture_artifact(
    bytes: Vec<u8>,
    media_type: &str,
    sensitivity: ArtifactSensitivity,
) -> Result<FixtureArtifactV4, FixtureError> {
    let artifact = FixtureArtifactV4 {
        cas_hash: ContentHash::sha256(&bytes),
        size: u64::try_from(bytes.len()).map_err(|_| FixtureError::Size)?,
        bytes_base64: STANDARD_NO_PAD.encode(bytes),
        media_type: media_type.to_owned(),
        sensitivity,
    };
    artifact.validate()?;
    Ok(artifact)
}

fn put_fixture_artifact(
    root: &StoreRoot,
    artifact: &FixtureArtifactV4,
) -> Result<(), FixtureError> {
    artifact.validate()?;
    CasStore::open(root)?.put(
        &CasHash::parse(artifact.cas_hash.to_string())?,
        Some(artifact.size),
        Cursor::new(artifact.bytes()?),
    )?;
    Ok(())
}

fn line_count(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.iter().filter(|byte| **byte == b'\n').count())
        .unwrap_or(u64::MAX)
        .saturating_add(1)
}

fn fixture_genesis(input: &[u8]) -> Result<reviewgraphen_core::RunGenesisSnapshot, FixtureError> {
    Ok(reviewgraphen_core::RunGenesisSnapshot::from_canonical_bytes(input)?)
}

fn fixture_repository_identity(input: &[u8]) -> Result<String, FixtureError> {
    Ok(fixture_genesis(input)?
        .rebuild_aggregate()?
        .program()
        .repository_identity()
        .to_owned())
}

fn fixture_snapshot_id(input: &[u8]) -> Result<StableId, FixtureError> {
    Ok(fixture_genesis(input)?
        .rebuild_aggregate()?
        .program()
        .snapshot_id()
        .clone())
}

fn fixture_profile_id(input: &[u8]) -> Result<String, FixtureError> {
    Ok(fixture_genesis(input)?
        .rebuild_aggregate()?
        .program()
        .profile_id()
        .to_owned())
}

fn fixture_profile_version(input: &[u8]) -> Result<String, FixtureError> {
    Ok(fixture_genesis(input)?
        .rebuild_aggregate()?
        .program()
        .profile_version()
        .to_owned())
}

fn fixture_repository_source_hash() -> Result<ContentHash, FixtureError> {
    let value: Value = serde_json::from_slice(include_bytes!(
        "../../../examples/double-submit-payment/program-space.json"
    ))?;
    Ok(ContentHash::parse(
        value["source"]["content_hash"]
            .as_str()
            .ok_or(FixtureError::Manifest)?
            .to_owned(),
    )?)
}

fn decode_canonical_base64(value: &str) -> Result<Vec<u8>, FixtureError> {
    let bytes = STANDARD_NO_PAD
        .decode(value)
        .map_err(|_| FixtureError::Manifest)?;
    if STANDARD_NO_PAD.encode(&bytes) != value {
        return Err(FixtureError::Manifest);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regenerates_the_same_source_bound_v4_m4_prefix() {
        let left = regenerate_m5_m4_prefix_manifest_v4().unwrap();
        let right = regenerate_m5_m4_prefix_manifest_v4().unwrap();
        assert_eq!(
            left.canonical_bytes().unwrap(),
            right.canonical_bytes().unwrap()
        );
        assert_eq!(
            left.canonical_bytes().unwrap(),
            M5M4PrefixFixtureManifestV4::committed()
                .unwrap()
                .canonical_bytes()
                .unwrap()
        );
        assert!(!left.canonical_envelopes_base64.is_empty());
    }

    #[test]
    fn materialized_prefix_replays_from_its_exact_public_roots() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), crate::StoreLimits::default()).unwrap();
        let fixture = materialize_m5_m4_prefix_v4(&root).unwrap();
        let (journal, roots, assignments) = fixture.into_parts();
        let roots = roots.build().unwrap();
        assignments.build().unwrap();
        let (_session, basis) = journal.replayed_v4_session(&roots).unwrap();
        assert!(basis.confirmed_event_count() > 1);
    }
}
