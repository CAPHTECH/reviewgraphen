//! Closed, offline reference-scenario materialization.
//!
//! This module deliberately owns the fixture orchestration rather than
//! depending on Store's test-support feature.  It rebuilds the small
//! double-submit M4 prefix through the same public Core and Store admission
//! APIs used by a host.  No preconstructed envelope is accepted as input.

use quote::ToTokens;
use reviewgraphen_core::{
    ArtifactRegisteredV3, ArtifactSensitivity, ArtifactSourceV3, AssignmentValueV4,
    AuthorityHarnessBindingV3Tuple, AuthorityHumanGrantV3Tuple, AuthorityTrustRootsV4,
    ClaimPolarity, ContentHash, ContextError, DecisionInputV3, DecisionOutcomeV3, EventCommand,
    EventLog, EventLogV4, ExecutionClaimInputV2, ExecutionOutcome, ExecutionRecordInput,
    FIXTURE_DESCRIPTOR_ID, FIXTURE_HARNESS_ID, FIXTURE_HARNESS_REVISION,
    FIXTURE_HARNESS_SOURCE_HASH, FIXTURE_MEDIA_TYPE, FIXTURE_PROCEDURE_ID,
    FIXTURE_TEST_ARTIFACT_ID, FIXTURE_WITNESS_HASH, HumanAuthorityCapabilityV3, M4_PROPERTY_ID,
    M4Error, M5DoubleSubmitAssignmentsV4, MvpRulePack, ObligationLifecycle, PlanBudget,
    ProgramSpace, ReviewAggregate, RunGenesisBootstrapRequestV4, RustSymbolAnchorV1,
    RustSymbolKindV1, SnapshotSourceBundle, SnapshotSourceEntry, SnapshotSourceRecordEntry,
    SnapshotSourcesRecorded, StableId, ValidatedExecutionBundle, VerificationBundleRequestV4,
    evaluate_static_fact_v1, plan, prepare_context,
};
use reviewgraphen_store::{
    CasHash, CasStore, EventJournal, JournalError, ReplayedV4RunSession, StoreError, StoreRoot,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};
use syn::spanned::Spanned;
use syn::{ImplItem, Item, Type};
use thiserror::Error;

const RUN_ID: &str = "run:fixed-offline-double-submit-v1";
const POLICY_BYTES: &[u8] = b"reviewgraphen-fixed-offline-policy-v1";
const CHECKOUT_CONTROLLER_SOURCE: &[u8] =
    include_bytes!("../../../examples/double-submit-payment/fixture/src/checkout_controller.rs");
const PAYMENT_REPOSITORY_SOURCE: &[u8] =
    include_bytes!("../../../examples/double-submit-payment/fixture/src/payment_repository.rs");

/// The closed fixture's only admitted ProgramSpace source files.  Runtime
/// embeds them at build time so the offline CLI neither reopens the workspace
/// nor accepts a caller-selected path.
const FIXTURE_SOURCE_PATHS: [(&str, &str, &[u8]); 2] = [
    (
        "file:checkout-controller",
        "src/checkout_controller.rs",
        CHECKOUT_CONTROLLER_SOURCE,
    ),
    (
        "file:payment-repository",
        "src/payment_repository.rs",
        PAYMENT_REPOSITORY_SOURCE,
    ),
];

#[derive(Clone, Debug)]
pub struct FixedOfflineRootsV4 {
    pub policy_revision_hash: ContentHash,
    pub repository_id: StableId,
    pub repository_source_hash: ContentHash,
    run_id: StableId,
    snapshot_id: StableId,
    universe_id: StableId,
    genesis_hash: ContentHash,
    fixture_claims: BTreeMap<StableId, ContentHash>,
    human_claim_ids: BTreeSet<StableId>,
}

impl FixedOfflineRootsV4 {
    pub fn build(&self) -> Result<AuthorityTrustRootsV4, FixedOfflineError> {
        let harnesses = self
            .fixture_claims
            .iter()
            .map(|(claim_id, claim_body_hash)| {
                Ok(AuthorityHarnessBindingV3Tuple {
                    policy_revision_hash: self.policy_revision_hash.clone(),
                    repository_id: self.repository_id.clone(),
                    repository_source_hash: self.repository_source_hash.clone(),
                    harness_id: FIXTURE_HARNESS_ID.to_owned(),
                    harness_revision: FIXTURE_HARNESS_REVISION.to_owned(),
                    harness_source_hash: ContentHash::parse(FIXTURE_HARNESS_SOURCE_HASH)?,
                    test_artifact_id: StableId::parse(FIXTURE_TEST_ARTIFACT_ID)?,
                    descriptor_id: FIXTURE_DESCRIPTOR_ID.to_owned(),
                    procedure_version: FIXTURE_PROCEDURE_ID.to_owned(),
                    result_hash: ContentHash::parse(FIXTURE_WITNESS_HASH)?,
                    result_size: u64::try_from(reviewgraphen_verifier::FIXTURE_WITNESS_BYTES.len())
                        .map_err(|_| FixedOfflineError::Size)?,
                    result_media_type: FIXTURE_MEDIA_TYPE.to_owned(),
                    result_sensitivity: ArtifactSensitivity::CanonicalState,
                    run_id: self.run_id.clone(),
                    genesis_hash: self.genesis_hash.clone(),
                    snapshot_id: self.snapshot_id.clone(),
                    universe_id: self.universe_id.clone(),
                    property_id: M4_PROPERTY_ID.to_owned(),
                    claim_id: claim_id.clone(),
                    claim_body_hash: claim_body_hash.clone(),
                })
            })
            .collect::<Result<Vec<_>, FixedOfflineError>>()?;
        let human_grants = (!self.human_claim_ids.is_empty())
            .then(|| AuthorityHumanGrantV3Tuple {
                policy_revision_hash: self.policy_revision_hash.clone(),
                actor: "human:fixed-offline-source".to_owned(),
                authority_id: "fixed-offline-source-review-board".to_owned(),
                capabilities: BTreeSet::from([HumanAuthorityCapabilityV3::AcceptFinding]),
                run_id: self.run_id.clone(),
                snapshot_id: self.snapshot_id.clone(),
                universe_id: self.universe_id.clone(),
                property_ids: BTreeSet::from([M4_PROPERTY_ID.to_owned()]),
                claim_ids: self.human_claim_ids.clone(),
                valid_from: "2026-01-01T00:00:00Z".to_owned(),
                valid_until: "2027-01-01T00:00:00Z".to_owned(),
            })
            .into_iter()
            .collect();
        Ok(AuthorityTrustRootsV4::new(
            self.policy_revision_hash.clone(),
            self.repository_id.clone(),
            self.repository_source_hash.clone(),
            harnesses,
            human_grants,
            Vec::new(),
        )?)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FixedOfflineAssignmentsV4;

impl FixedOfflineAssignmentsV4 {
    pub fn build(self) -> Result<M5DoubleSubmitAssignmentsV4, FixedOfflineError> {
        Ok(M5DoubleSubmitAssignmentsV4::new(
            AssignmentValueV4::Required,
            AssignmentValueV4::Satisfied,
        ))
    }
}

pub struct MaterializedFixedOfflineV4<'a> {
    journal: EventJournal<'a>,
    roots: FixedOfflineRootsV4,
}

/// Closed V5 target predecessor plus the report presentation coordinates
/// derived while its plan is created.  These IDs are descriptive request
/// metadata only; they grant no incremental or append authority.
pub struct MaterializedFixedOfflineTargetV5<'a> {
    journal: EventJournal<'a>,
    plan_id: StableId,
    obligation_id: StableId,
}

impl<'a> MaterializedFixedOfflineTargetV5<'a> {
    pub fn into_parts(self) -> (EventJournal<'a>, StableId, StableId) {
        (self.journal, self.plan_id, self.obligation_id)
    }
}

impl<'a> MaterializedFixedOfflineV4<'a> {
    pub fn into_parts(
        self,
    ) -> (
        EventJournal<'a>,
        FixedOfflineRootsV4,
        FixedOfflineAssignmentsV4,
    ) {
        (self.journal, self.roots, FixedOfflineAssignmentsV4)
    }
}

#[derive(Debug, Error)]
pub enum FixedOfflineError {
    #[error(transparent)]
    Core(#[from] reviewgraphen_core::DomainError),
    #[error(transparent)]
    Context(#[from] ContextError),
    #[error(transparent)]
    M4(#[from] M4Error),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("the committed fixed-offline source is invalid")]
    Fixture,
    #[error("the fixed-offline fixture bytes do not match the compiled canonical sources")]
    FixtureBinding,
    #[error("the fixed-offline byte length is not representable")]
    Size,
}

/// Materializes exactly one fixed, process-free reference prefix. It never
/// receives a path, command, model response, network input, or event bytes.
pub fn materialize_double_submit_m4_prefix_v4<'a>(
    root: &'a StoreRoot,
) -> Result<MaterializedFixedOfflineV4<'a>, FixedOfflineError> {
    let (program, sources, repository_source_hash) = fixed_program()?;
    let (universe, obligations) = MvpRulePack::synthesize(&program)?.into_parts();
    let aggregate = ReviewAggregate::new(program.clone(), universe, obligations)?;
    let run_id = StableId::parse(RUN_ID)?;
    let universe_id = aggregate.universe().id().clone();
    let local = EventLog::new_v3(run_id.clone(), aggregate)?;
    let genesis = local.run_genesis_snapshot()?.canonical_bytes()?;
    let mut roots = FixedOfflineRootsV4 {
        policy_revision_hash: ContentHash::sha256(POLICY_BYTES),
        repository_id: program.repository_id().clone(),
        repository_source_hash,
        run_id: run_id.clone(),
        snapshot_id: program.snapshot_id().clone(),
        universe_id,
        genesis_hash: ContentHash::sha256(&genesis),
        fixture_claims: BTreeMap::new(),
        human_claim_ids: BTreeSet::new(),
    };
    // Source bytes are admitted to CAS before their corresponding durable
    // registrations are prepared below. Their registration still flows only
    // through the V4 authority-aware session.
    for bytes in sources.values() {
        put(root, bytes)?;
    }
    let bootstrap = EventLogV4::from_bootstrap_request(RunGenesisBootstrapRequestV4::new(
        run_id,
        genesis.clone(),
        fixture_repository_identity(&genesis)?,
        fixture_snapshot_id(&genesis)?,
        fixture_profile_id(&genesis)?,
        fixture_profile_version(&genesis)?,
    )?)?;
    let (journal, _) = EventJournal::publish_new_v4(root, bootstrap)?;
    let authority_roots = roots.build()?;
    let (mut session, mut basis) = journal.replayed_v4_session(&authority_roots)?;
    let fixture_claims = build_m4_prefix(root, &mut session, &mut basis, &program, &sources)?;
    drop(session);
    roots.human_claim_ids = fixture_claims.keys().cloned().collect();
    roots.fixture_claims = fixture_claims;
    let authority_roots = roots.build()?;
    let (mut session, mut basis) = journal.replayed_v4_session(&authority_roots)?;
    for claim_id in &roots.human_claim_ids {
        // The output and witness registrations are deliberately separate
        // authority positions. Core derives both byte strings from the exact
        // trusted harness binding; Runtime only publishes those bytes to CAS
        // and persists the resulting sealed registrations.
        let output_execution = session.execute_fixture_harness(claim_id, &basis)?;
        put(root, output_execution.fixture_result_bytes())?;
        let output =
            session.prepare_fixture_verifier_output_registration(&output_execution, &basis)?;
        let output = session.append_inherited_artifact_registration(output, &mut basis)?;

        let witness_execution = session.execute_fixture_harness(claim_id, &basis)?;
        put(root, witness_execution.witness_bytes())?;
        let witness =
            session.prepare_external_harness_witness_registration(&witness_execution, &basis)?;
        let witness = session.append_inherited_artifact_registration(witness, &mut basis)?;
        let witness = session.admit_external_witness(witness_execution, witness.core(), &basis)?;
        let verified = session.mint_verification_bundle(
            VerificationBundleRequestV4::fixture(
                claim_id.clone(),
                output.core().registration_id().clone(),
            ),
            Some(witness),
            &basis,
        )?;
        session.append_verification_bundle(verified, &mut basis)?;
    }
    for claim_id in &roots.human_claim_ids {
        session.append_human_decision(
            AuthorityHumanGrantV3Tuple {
                policy_revision_hash: roots.policy_revision_hash.clone(),
                actor: "human:fixed-offline-source".to_owned(),
                authority_id: "fixed-offline-source-review-board".to_owned(),
                capabilities: BTreeSet::from([HumanAuthorityCapabilityV3::AcceptFinding]),
                run_id: roots.run_id.clone(),
                snapshot_id: roots.snapshot_id.clone(),
                universe_id: roots.universe_id.clone(),
                property_ids: BTreeSet::from([M4_PROPERTY_ID.to_owned()]),
                claim_ids: roots.human_claim_ids.clone(),
                valid_from: "2026-01-01T00:00:00Z".to_owned(),
                valid_until: "2027-01-01T00:00:00Z".to_owned(),
            },
            claim_id,
            DecisionInputV3::new(
                DecisionOutcomeV3::Accept,
                "human:fixed-offline-source",
                "fixed-offline-source-review-board",
                "fixed source accepts the verified double-submit issue",
                "2026-08-12T00:00:00Z",
                None,
            ),
            "2026-08-12T00:00:00Z",
            &mut basis,
        )?;
        session.append_human_finding(claim_id, "reviewgraphen.finding_projection@1", &mut basis)?;
    }
    drop(session);
    Ok(MaterializedFixedOfflineV4 { journal, roots })
}

/// Publishes the sole fixed V5 target predecessor used by the offline CLI.
///
/// This is deliberately a typed constructor, not a journal import: the
/// accepted target ProgramSpace, snapshot-source registrations, and plan are
/// rebuilt from the closed fixture and then replay-validated by Core before
/// Store publishes the chain.  Its Git closure starts at the source fixture's
/// accepted target commit, so Store can prove incremental continuity without
/// receiving a caller-supplied revision, path, or event bytes.
pub fn publish_double_submit_target_v5<'a>(
    root: &'a StoreRoot,
) -> Result<MaterializedFixedOfflineTargetV5<'a>, FixedOfflineError> {
    let (program, sources, _) = fixed_target_program()?;
    for bytes in sources.values() {
        put(root, bytes)?;
    }
    let (universe, obligations) = MvpRulePack::synthesize(&program)?.into_parts();
    let aggregate = ReviewAggregate::new(program.clone(), universe, obligations)?;
    let run_id = StableId::parse("run:fixed-offline-double-submit-v2")?;
    let mut local = EventLog::new_v3(run_id.clone(), aggregate)?;
    let genesis = local.run_genesis_snapshot()?.canonical_bytes()?;
    let mut registrations = Vec::new();
    let mut entries = Vec::new();
    for artifact in program
        .artifacts()
        .iter()
        .filter(|item| item.kind == "file")
    {
        let bytes = sources
            .get(&artifact.id)
            .ok_or(FixedOfflineError::Fixture)?;
        let hash = ContentHash::sha256(bytes);
        let registration = ArtifactRegisteredV3::new(
            run_id.clone(),
            hash.clone(),
            "text/plain",
            u64::try_from(bytes.len()).map_err(|_| FixedOfflineError::Size)?,
            ArtifactSensitivity::WorkspaceSource,
            ArtifactSourceV3::SnapshotIngest {
                adapter_id: "reviewgraphen-fixed-offline@1".to_owned(),
                run_id: run_id.clone(),
                snapshot_id: program.snapshot_id().clone(),
            },
        )?;
        let path = artifact
            .location
            .as_ref()
            .ok_or(FixedOfflineError::Fixture)?
            .path
            .clone();
        entries.push(SnapshotSourceRecordEntry::new(
            artifact.id.clone(),
            path,
            hash.clone(),
            registration.registration_id().clone(),
            hash,
            line_count(bytes),
        )?);
        registrations.push(registration);
    }
    registrations.sort_by(|left, right| left.registration_id().cmp(right.registration_id()));
    entries.sort_by(|left, right| left.path().cmp(right.path()));
    let recorded = SnapshotSourcesRecorded::new(program.snapshot_id().clone(), entries)?;
    for registration in &registrations {
        local.append(EventCommand::artifact_registered_v3(registration.clone()))?;
    }
    local.append(EventCommand::snapshot_sources_recorded(recorded.clone()))?;
    let review_plan = plan(local.aggregate(), PlanBudget::new(16, 16)?)?;
    let obligation_id = review_plan
        .waves()
        .iter()
        .flat_map(|wave| wave.obligation_ids())
        .find(|candidate| {
            local
                .aggregate()
                .obligations()
                .find(|obligation| obligation.id() == *candidate)
                .is_some_and(|obligation| obligation.property_id() == M4_PROPERTY_ID)
        })
        .cloned()
        .ok_or(FixedOfflineError::Fixture)?;
    let plan_id = review_plan.id().clone();
    let bootstrap = RunGenesisBootstrapRequestV4::new(
        run_id,
        genesis,
        program.repository_identity(),
        program.snapshot_id().clone(),
        program.profile_id(),
        program.profile_version(),
    )?;
    let log = reviewgraphen_core::EventLogV5::from_planned_bootstrap_request(
        bootstrap,
        registrations,
        recorded,
        review_plan,
    )?;
    Ok(MaterializedFixedOfflineTargetV5 {
        journal: EventJournal::publish_new_v5(root, log)?.0,
        plan_id,
        obligation_id,
    })
}

type FixedProgram = (ProgramSpace, BTreeMap<StableId, Vec<u8>>, ContentHash);

fn fixed_program() -> Result<FixedProgram, FixedOfflineError> {
    let sources = compiled_fixture_sources()?;
    fixed_program_from_sources(&sources)
}

/// Validates the source binding before rebuilding the ProgramSpace. Production
/// has exactly one source input: the Rust files compiled into this binary.
/// A byte/ref swap is therefore a materialization failure, never an implicit
/// rewrite of the durable source registrations.
fn fixed_program_from_sources(
    sources: &BTreeMap<StableId, Vec<u8>>,
) -> Result<FixedProgram, FixedOfflineError> {
    if sources != &compiled_fixture_sources()? {
        return Err(FixedOfflineError::FixtureBinding);
    }
    build_fixed_program(sources)
}

fn compiled_fixture_sources() -> Result<BTreeMap<StableId, Vec<u8>>, FixedOfflineError> {
    FIXTURE_SOURCE_PATHS
        .iter()
        .map(|(artifact_id, _, bytes)| Ok((StableId::parse(*artifact_id)?, bytes.to_vec())))
        .collect()
}

fn build_fixed_program(
    sources: &BTreeMap<StableId, Vec<u8>>,
) -> Result<FixedProgram, FixedOfflineError> {
    let mut value: Value = serde_json::from_slice(include_bytes!(
        "../../../examples/double-submit-payment/program-space.json"
    ))?;
    expand_legacy_hashes(&mut value);
    value["schema"] = Value::String("reviewgraphen.program_space.input.v3".to_owned());
    value["source"]["kind"] = Value::String("git".to_owned());
    value["source"]["revision"] = Value::String(format!("{:040x}", 10));
    value["source"]["content_hash"] = Value::String(format!("git:{:040x}", 11));
    value["snapshot"]["base_revision"] = Value::String(format!("{:040x}", 9));
    value["snapshot"]["target_revision"] = Value::String(format!("{:040x}", 10));
    value["snapshot"]["tree_hash"] = Value::String(format!("git:{:040x}", 11));
    value["profile"]["id"] = Value::String("double-submit-payment".to_owned());
    value["profile"]["version"] = Value::String("1".to_owned());
    for context in value["contexts"]
        .as_array_mut()
        .ok_or(FixedOfflineError::Fixture)?
    {
        let source = match context["id"].as_str() {
            Some("context:ui-event") => Some("file:checkout-controller"),
            Some("context:payment") => Some("file:payment-repository"),
            _ => None,
        };
        if let Some(source) = source {
            let members = context["member_ids"]
                .as_array_mut()
                .ok_or(FixedOfflineError::Fixture)?;
            members.push(Value::String(source.to_owned()));
            members.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
            members.dedup();
        }
    }
    let test = value["artifacts"]
        .as_array_mut()
        .ok_or(FixedOfflineError::Fixture)?
        .iter_mut()
        .find(|item| item["id"] == reviewgraphen_core::FIXTURE_TEST_ARTIFACT_ID)
        .ok_or(FixedOfflineError::Fixture)?;
    test["location"]["start_line"] = Value::Null;
    test["location"]["end_line"] = Value::Null;
    let invariant = value["invariants"]
        .as_array_mut()
        .ok_or(FixedOfflineError::Fixture)?
        .iter_mut()
        .find(|item| item["property_id"] == M4_PROPERTY_ID)
        .ok_or(FixedOfflineError::Fixture)?;
    invariant["scope_ids"] = json!(["context:payment", "context:ui-event"]);
    value["evidence"] = json!([{"id":"evidence:fixed-payment-source","kind":"static_analysis",
        "target_ids":["function:payment-charge"],"artifact_ref":null,"content_hash":null,
        "attributes":{"fixed_offline":true},"provenance":value["artifacts"][0]["provenance"].clone()}]);
    let mut contains = value["relations"][0].clone();
    contains["id"] = Value::String("relation:fixed-file-contains-payment-charge".to_owned());
    contains["kind"] = Value::String("contains".to_owned());
    contains["source_id"] = Value::String("file:payment-repository".to_owned());
    contains["target_ids"] = json!(["function:payment-charge"]);
    contains["directed"] = Value::Bool(true);
    value["relations"]
        .as_array_mut()
        .ok_or(FixedOfflineError::Fixture)?
        .push(contains);
    let mut stripe_contains = value["relations"]
        .as_array()
        .and_then(|relations| relations.last())
        .cloned()
        .ok_or(FixedOfflineError::Fixture)?;
    stripe_contains["id"] = Value::String("relation:fixed-file-contains-stripe-charge".to_owned());
    stripe_contains["target_ids"] = json!(["function:stripe-charge"]);
    value["relations"]
        .as_array_mut()
        .ok_or(FixedOfflineError::Fixture)?
        .push(stripe_contains);
    let fixture_paths = FIXTURE_SOURCE_PATHS
        .iter()
        .map(|(artifact_id, path, _)| Ok((StableId::parse(*artifact_id)?, *path)))
        .collect::<Result<BTreeMap<_, _>, FixedOfflineError>>()?;
    for artifact in value["artifacts"]
        .as_array_mut()
        .ok_or(FixedOfflineError::Fixture)?
    {
        if artifact["kind"] == "file" {
            let artifact_id =
                StableId::parse(artifact["id"].as_str().ok_or(FixedOfflineError::Fixture)?)?;
            let path = artifact["location"]["path"]
                .as_str()
                .ok_or(FixedOfflineError::Fixture)?;
            if fixture_paths.get(&artifact_id) != Some(&path) {
                return Err(FixedOfflineError::FixtureBinding);
            }
            let bytes = sources
                .get(&artifact_id)
                .ok_or(FixedOfflineError::FixtureBinding)?;
            artifact["content_hash"] = Value::String(ContentHash::sha256(bytes).to_string());
        }
    }
    let mut anchors = serde_json::Map::new();
    for (artifact_id, source_artifact_id, owner, method) in FIXTURE_METHOD_FACTS {
        let source_id = StableId::parse(source_artifact_id)?;
        let source = sources
            .get(&source_id)
            .ok_or(FixedOfflineError::FixtureBinding)?;
        let fact = extract_method_fact(source, owner, method)?;
        let artifact = value["artifacts"]
            .as_array_mut()
            .ok_or(FixedOfflineError::Fixture)?
            .iter_mut()
            .find(|candidate| candidate["id"] == artifact_id)
            .ok_or(FixedOfflineError::Fixture)?;
        // The checked-in ProgramSpace intentionally models these call-path
        // endpoints as generic function facts. Keep that public fact kind
        // stable while deriving the anchor body and signature from the Rust
        // impl method it denotes.
        artifact["kind"] = Value::String("function".to_owned());
        artifact["language"] = Value::String("rust".to_owned());
        artifact["location"] = json!({
            "path": fixture_paths.get(&source_id).ok_or(FixedOfflineError::FixtureBinding)?,
            "start_line": fact.start_line,
            "end_line": fact.end_line,
        });
        artifact["provenance"]["extraction_method"] =
            Value::String("reviewgraphen.ingest.rust_syn.v1".to_owned());
        anchors.insert(artifact_id.to_owned(), serde_json::to_value(fact.anchor)?);
    }
    for relation in value["relations"]
        .as_array_mut()
        .ok_or(FixedOfflineError::Fixture)?
    {
        relation["ordered_target_ids"] = relation["target_ids"].clone();
    }
    value["incremental_facts"] = json!({
        "git_revision_closure": {
            "base_commit_oid": format!("{:040x}", 9),
            "base_tree_hash": format!("git:{:040x}", 8),
            "target_commit_oid": format!("{:040x}", 10),
            "target_tree_hash": format!("git:{:040x}", 11),
        },
        "rust_anchor_extractor_id": reviewgraphen_core::RUST_SYMBOL_ANCHOR_EXTRACTOR_V1,
        "rust_anchor_syn_version": reviewgraphen_core::RUST_SYMBOL_ANCHOR_SYN_VERSION_V1,
        "rust_symbol_anchors": anchors,
    });
    let repository_source_hash = ContentHash::parse(format!("git:{:040x}", 11))?;
    Ok((
        ProgramSpace::from_json_slice(&serde_json::to_vec(&value)?)?,
        sources.clone(),
        repository_source_hash,
    ))
}

const FIXTURE_METHOD_FACTS: [(&str, &str, &str, &str); 3] = [
    (
        "function:checkout-submit",
        "file:checkout-controller",
        "CheckoutController",
        "submit",
    ),
    (
        "function:payment-charge",
        "file:payment-repository",
        "PaymentRepository",
        "charge",
    ),
    (
        "function:stripe-charge",
        "file:payment-repository",
        "StripeClient",
        "charge",
    ),
];

struct ExtractedMethodFact {
    anchor: RustSymbolAnchorV1,
    start_line: u64,
    end_line: u64,
}

/// Replays the pinned Rust-anchor canonicalization over the exact embedded
/// fixture syntax tree. The ProgramSpace IDs remain fixture IDs, but their
/// anchor and location come from the declaration those IDs denote -- never
/// from an ID-shaped placeholder string.
fn extract_method_fact(
    source: &[u8],
    expected_owner: &str,
    expected_method: &str,
) -> Result<ExtractedMethodFact, FixedOfflineError> {
    let text = std::str::from_utf8(source).map_err(|_| FixedOfflineError::Fixture)?;
    let file = syn::parse_file(text).map_err(|_| FixedOfflineError::Fixture)?;
    for item in file.items {
        let Item::Impl(implementation) = item else {
            continue;
        };
        if impl_owner(&implementation.self_ty).as_deref() != Some(expected_owner) {
            continue;
        }
        let trait_path = implementation
            .trait_
            .as_ref()
            .map(|(bang, path, _)| format!("{} {}", bang.to_token_stream(), path.to_token_stream()))
            .unwrap_or_default();
        for implementation_item in implementation.items {
            let ImplItem::Fn(method) = implementation_item else {
                continue;
            };
            if method.sig.ident != expected_method {
                continue;
            }
            let signature = format!(
                "{} {} {} {} {} {} {} {} {}",
                attribute_tokens(&implementation.attrs),
                implementation.defaultness.to_token_stream(),
                implementation.unsafety.to_token_stream(),
                implementation.generics.to_token_stream(),
                trait_path,
                implementation.self_ty.to_token_stream(),
                attribute_tokens(&method.attrs),
                method.vis.to_token_stream(),
                method.sig.to_token_stream(),
            );
            let span = method.span();
            return Ok(ExtractedMethodFact {
                anchor: RustSymbolAnchorV1::new(
                    RustSymbolKindV1::Function,
                    ContentHash::sha256(signature.as_bytes()),
                    ContentHash::sha256(method.block.to_token_stream().to_string().as_bytes()),
                )
                .map_err(|_| FixedOfflineError::Fixture)?,
                start_line: u64::try_from(span.start().line.max(1))
                    .map_err(|_| FixedOfflineError::Size)?,
                end_line: u64::try_from(span.end().line.max(span.start().line).max(1))
                    .map_err(|_| FixedOfflineError::Size)?,
            });
        }
    }
    Err(FixedOfflineError::Fixture)
}

fn impl_owner(value: &Type) -> Option<String> {
    match value {
        Type::Path(path) if path.qself.is_none() => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

fn attribute_tokens(attributes: &[syn::Attribute]) -> String {
    attributes
        .iter()
        .map(|attribute| attribute.to_token_stream().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn fixed_target_program() -> Result<FixedProgram, FixedOfflineError> {
    let (source, bytes, _) = fixed_program()?;
    let mut target = serde_json::to_value(&source)?;
    fn replace(value: &mut Value, from: &str, to: &str) {
        match value {
            Value::String(text) if text == from => *text = to.to_owned(),
            Value::Array(values) => values.iter_mut().for_each(|value| replace(value, from, to)),
            Value::Object(values) => values
                .values_mut()
                .for_each(|value| replace(value, from, to)),
            _ => {}
        }
    }
    replace(
        &mut target,
        source.snapshot_id().as_str(),
        "snapshot:double-submit-fixed-offline-v2",
    );
    let target_commit_oid = format!("{:040x}", 12);
    let target_tree_hash = format!("git:{:040x}", 13);
    target["snapshot"]["base_revision"] = Value::String(source.target_revision().to_owned());
    target["snapshot"]["target_revision"] = Value::String(target_commit_oid.clone());
    target["snapshot"]["tree_hash"] = Value::String(target_tree_hash.clone());
    target["source"]["revision"] = Value::String(target_commit_oid.clone());
    target["source"]["content_hash"] = Value::String(target_tree_hash.clone());
    let revisions = &mut target["incremental_facts"]["git_revision_closure"];
    revisions["base_commit_oid"] = Value::String(source.target_revision().to_owned());
    revisions["base_tree_hash"] = Value::String(format!("git:{:040x}", 11));
    revisions["target_commit_oid"] = Value::String(target_commit_oid);
    revisions["target_tree_hash"] = Value::String(target_tree_hash.clone());
    Ok((
        ProgramSpace::from_json_slice(&serde_json::to_vec(&target)?)?,
        bytes,
        ContentHash::parse(target_tree_hash)?,
    ))
}

fn build_m4_prefix(
    root: &StoreRoot,
    session: &mut ReplayedV4RunSession<'_, '_>,
    basis: &mut reviewgraphen_core::AuthorityReplayBasisV4,
    program: &ProgramSpace,
    sources: &BTreeMap<StableId, Vec<u8>>,
) -> Result<BTreeMap<StableId, ContentHash>, FixedOfflineError> {
    let run_id = StableId::parse(RUN_ID)?;
    let (universe, obligations) = MvpRulePack::synthesize(program)?.into_parts();
    let mut local = EventLog::new_v3(
        run_id.clone(),
        ReviewAggregate::new(program.clone(), universe, obligations)?,
    )?;
    let mut registrations = Vec::new();
    let mut entries = Vec::new();
    let mut bundle = Vec::new();
    for artifact in program
        .artifacts()
        .iter()
        .filter(|item| item.kind == "file")
    {
        let bytes = sources
            .get(&artifact.id)
            .ok_or(FixedOfflineError::Fixture)?;
        let hash = ContentHash::sha256(bytes);
        let registration = ArtifactRegisteredV3::new(
            run_id.clone(),
            hash.clone(),
            "text/plain",
            u64::try_from(bytes.len()).map_err(|_| FixedOfflineError::Size)?,
            ArtifactSensitivity::WorkspaceSource,
            ArtifactSourceV3::SnapshotIngest {
                adapter_id: "reviewgraphen-fixed-offline@1".to_owned(),
                run_id: run_id.clone(),
                snapshot_id: program.snapshot_id().clone(),
            },
        )?;
        let path = artifact
            .location
            .as_ref()
            .ok_or(FixedOfflineError::Fixture)?
            .path
            .clone();
        entries.push(SnapshotSourceRecordEntry::new(
            artifact.id.clone(),
            path.clone(),
            hash.clone(),
            registration.registration_id().clone(),
            hash.clone(),
            line_count(bytes),
        )?);
        bundle.push(SnapshotSourceEntry::new(
            artifact.id.clone(),
            path,
            hash.clone(),
            hash,
            bytes.clone(),
        ));
        registrations.push((artifact.id.clone(), registration));
    }
    entries.sort_by(|a, b| a.path().cmp(b.path()));
    let recorded = SnapshotSourcesRecorded::new(program.snapshot_id().clone(), entries)?;
    let source_bundle = SnapshotSourceBundle::new(program, bundle)?;
    for (_, registration) in &registrations {
        local.append(EventCommand::artifact_registered_v3(registration.clone()))?;
    }
    local.append(EventCommand::snapshot_sources_recorded(recorded.clone()))?;
    let review_plan = plan(local.aggregate(), PlanBudget::new(16, 16)?)?;
    local.append(EventCommand::review_plan_recorded(review_plan.clone()))?;
    let mut selected = None;
    for candidate in review_plan
        .waves()
        .iter()
        .flat_map(|wave| wave.obligation_ids())
    {
        let obligation = local
            .aggregate()
            .obligations()
            .find(|obligation| obligation.id() == candidate)
            .ok_or(FixedOfflineError::Fixture)?;
        if obligation.property_id() != M4_PROPERTY_ID {
            continue;
        }
        let mut context = prepare_context(local.aggregate(), candidate.clone())?;
        while let Some(request) = context.next_source_request()? {
            context.submit_source(
                &request,
                sources
                    .get(request.artifact_id())
                    .ok_or(FixedOfflineError::FixtureBinding)?,
            )?;
        }
        let built = context.finish()?;
        if !built.envelope().normalized_included_source_ids().is_empty() {
            selected = Some((candidate.clone(), built));
            break;
        }
    }
    let (obligation_id, built) = selected.ok_or(FixedOfflineError::Fixture)?;
    let wave_id = review_plan
        .waves()
        .iter()
        .find(|wave| wave.obligation_ids().contains(&obligation_id))
        .ok_or(FixedOfflineError::Fixture)?
        .id()
        .clone();
    let envelope = built.envelope().clone();
    let mut v4_context = prepare_context(local.aggregate(), obligation_id.clone())?;
    while let Some(request) = v4_context.next_source_request()? {
        v4_context.submit_source(
            &request,
            sources
                .get(request.artifact_id())
                .ok_or(FixedOfflineError::Fixture)?,
        )?;
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
    let input = ExecutionRecordInput::fake(
        review_plan.id().clone(),
        wave_id,
        obligation_id.clone(),
        envelope.id().clone(),
        envelope.snapshot_id().clone(),
        1,
    )?;
    let execution_id = input.execution_id()?;
    let raw = reviewgraphen_core::canonical_json(&BTreeMap::from([
        (
            "execution_id".to_owned(),
            Value::String(execution_id.to_string()),
        ),
        (
            "schema".to_owned(),
            Value::String("reviewgraphen.fixed-offline.raw.v1".to_owned()),
        ),
    ]))?;
    put(root, &raw)?;
    let raw_registration = ArtifactRegisteredV3::new(
        run_id.clone(),
        ContentHash::sha256(&raw),
        "application/json",
        u64::try_from(raw.len()).map_err(|_| FixedOfflineError::Size)?,
        ArtifactSensitivity::Sensitive,
        ArtifactSourceV3::ReviewerExecution {
            execution_id,
            reviewer_id: reviewgraphen_core::FAKE_REVIEWER_ID.to_owned(),
            run_id: run_id.clone(),
        },
    )?;
    let obligation = local
        .aggregate()
        .obligations()
        .find(|o| o.id() == &obligation_id)
        .ok_or(FixedOfflineError::Fixture)?
        .clone();
    let claims = ["file:checkout-controller", "file:payment-repository"]
        .into_iter()
        .map(|id| {
            ExecutionClaimInputV2::new(
                obligation.property_id(),
                obligation.normalized_target_refs().clone(),
                ClaimPolarity::IssuePresent,
                format!("fixed:{}:{id}", obligation.property_id()),
                BTreeSet::from([StableId::parse(id).ok()?]),
                BTreeSet::new(),
                BTreeSet::new(),
                Some(1.0),
            )
            .ok()
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(FixedOfflineError::Fixture)?;
    let execution = ValidatedExecutionBundle::fake_v3(
        input,
        &raw_registration,
        raw,
        sources.values().collect(),
        claims,
        ExecutionOutcome::Structured,
    )?;
    let claim_ids = execution
        .claims()
        .iter()
        .map(|claim| claim.id().clone())
        .collect::<Vec<_>>();
    let fixture_claims = execution
        .claims()
        .iter()
        .take(1)
        .map(|claim| Ok((claim.id().clone(), claim.body_hash()?)))
        .collect::<Result<BTreeMap<_, _>, FixedOfflineError>>()?;
    local.append(EventCommand::artifact_registered_v3(
        raw_registration.clone(),
    ))?;
    local.append(EventCommand::review_execution_recorded(execution.clone()))?;
    let evaluations = claim_ids
        .iter()
        .map(|id| {
            let claim = local
                .aggregate()
                .execution_claims()
                .find(|claim| claim.id() == id)
                .ok_or(FixedOfflineError::Fixture)?;
            Ok((
                id.clone(),
                evaluate_static_fact_v1(local.aggregate().program(), &obligation, claim)?,
            ))
        })
        .collect::<Result<Vec<_>, FixedOfflineError>>()?;
    for (artifact, registration) in registrations {
        let prepared = session.prepare_snapshot_artifact_registration(
            registration,
            &source_bundle,
            &artifact,
            basis,
        )?;
        session.append_inherited_artifact_registration(prepared, basis)?;
    }
    append(
        session,
        EventCommand::snapshot_sources_recorded(recorded),
        basis,
    )?;
    append(
        session,
        EventCommand::review_plan_recorded(review_plan),
        basis,
    )?;
    append(
        session,
        EventCommand::obligation_transition(obligation_id.clone(), ObligationLifecycle::Planned),
        basis,
    )?;
    append(
        session,
        EventCommand::obligation_transition(obligation_id.clone(), ObligationLifecycle::InProgress),
        basis,
    )?;
    append(
        session,
        EventCommand::context_envelope_projected(v4_built),
        basis,
    )?;
    let admission =
        session.prepare_reviewer_raw_artifact_registration(raw_registration, &execution, basis)?;
    session.append_inherited_artifact_registration(admission, basis)?;
    append(
        session,
        EventCommand::review_execution_recorded(execution),
        basis,
    )?;
    for (claim_id, evaluation) in evaluations {
        let input = evaluation.input().canonical_bytes()?;
        let output = evaluation.result().canonical_bytes()?;
        put(root, &input)?;
        put(root, &output)?;
        let input_receipt = session.append_inherited_artifact_registration(
            session.prepare_static_verifier_input_registration(
                &claim_id,
                ContentHash::sha256(&input),
                u64::try_from(input.len()).map_err(|_| FixedOfflineError::Size)?,
                basis,
            )?,
            basis,
        )?;
        let output_receipt = session.append_inherited_artifact_registration(
            session.prepare_static_verifier_output_registration(
                &claim_id,
                ContentHash::sha256(&output),
                u64::try_from(output.len()).map_err(|_| FixedOfflineError::Size)?,
                basis,
            )?,
            basis,
        )?;
        let verified = session.mint_verification_bundle(
            VerificationBundleRequestV4::static_fact(
                claim_id,
                input_receipt.core().registration_id().clone(),
                output_receipt.core().registration_id().clone(),
            ),
            None,
            basis,
        )?;
        session.append_verification_bundle(verified, basis)?;
    }
    Ok(fixture_claims)
}

fn append(
    session: &mut ReplayedV4RunSession<'_, '_>,
    command: EventCommand,
    basis: &mut reviewgraphen_core::AuthorityReplayBasisV4,
) -> Result<(), FixedOfflineError> {
    let prepared = session.prepare_inherited_d2_event(command, basis)?;
    session.append_inherited_d2_event(prepared, basis)?;
    Ok(())
}
fn put(root: &StoreRoot, bytes: &[u8]) -> Result<(), FixedOfflineError> {
    let hash = ContentHash::sha256(bytes);
    CasStore::open(root)?.put(
        &CasHash::parse(hash.to_string())?,
        Some(u64::try_from(bytes.len()).map_err(|_| FixedOfflineError::Size)?),
        Cursor::new(bytes),
    )?;
    Ok(())
}
fn line_count(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.iter().filter(|byte| **byte == b'\n').count())
        .unwrap_or(u64::MAX)
        .saturating_add(1)
}
fn genesis(input: &[u8]) -> Result<reviewgraphen_core::RunGenesisSnapshot, FixedOfflineError> {
    Ok(reviewgraphen_core::RunGenesisSnapshot::from_canonical_v4_bytes_for_store(input)?)
}
fn fixture_repository_identity(input: &[u8]) -> Result<String, FixedOfflineError> {
    Ok(genesis(input)?
        .rebuild_aggregate()?
        .program()
        .repository_identity()
        .to_owned())
}
fn fixture_snapshot_id(input: &[u8]) -> Result<StableId, FixedOfflineError> {
    Ok(genesis(input)?
        .rebuild_aggregate()?
        .program()
        .snapshot_id()
        .clone())
}
fn fixture_profile_id(input: &[u8]) -> Result<String, FixedOfflineError> {
    Ok(genesis(input)?
        .rebuild_aggregate()?
        .program()
        .profile_id()
        .to_owned())
}
fn fixture_profile_version(input: &[u8]) -> Result<String, FixedOfflineError> {
    Ok(genesis(input)?
        .rebuild_aggregate()?
        .program()
        .profile_version()
        .to_owned())
}
fn expand_legacy_hashes(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                expand_legacy_hashes(value)
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                expand_legacy_hashes(value)
            }
        }
        Value::String(text) => {
            if let Some(hex) = text.strip_prefix("sha256:")
                && hex.len() == 16
                && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                *text = format!("sha256:{hex}{hex}{hex}{hex}");
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materializes_the_closed_fixed_offline_prefix() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(
            workspace.path(),
            reviewgraphen_store::StoreLimits::default(),
        )
        .unwrap();
        materialize_double_submit_m4_prefix_v4(&root).unwrap();
    }

    #[test]
    fn materialized_prefix_completes_the_closed_m5_profile() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(
            workspace.path(),
            reviewgraphen_store::StoreLimits::default(),
        )
        .unwrap();
        let fixture = materialize_double_submit_m4_prefix_v4(&root).unwrap();
        let (journal, roots, assignments) = fixture.into_parts();
        crate::m5_gluing::run_double_submit_payment_profile_conflict_v4(
            &journal,
            roots.build().unwrap(),
            assignments.build().unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn publishes_the_closed_fixed_offline_target() {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(
            workspace.path(),
            reviewgraphen_store::StoreLimits::default(),
        )
        .unwrap();
        publish_double_submit_target_v5(&root).unwrap();
    }

    #[test]
    fn compiled_fixture_bytes_bind_source_records_and_reject_mutation() {
        let expected = compiled_fixture_sources().unwrap();
        assert_eq!(
            expected
                .get(&StableId::parse("file:checkout-controller").unwrap())
                .map(Vec::as_slice),
            Some(CHECKOUT_CONTROLLER_SOURCE)
        );
        assert_eq!(
            expected
                .get(&StableId::parse("file:payment-repository").unwrap())
                .map(Vec::as_slice),
            Some(PAYMENT_REPOSITORY_SOURCE)
        );

        let (first_program, first_sources, _) = fixed_program().unwrap();
        let (second_program, second_sources, _) = fixed_program().unwrap();
        assert_eq!(first_sources, second_sources);
        assert_eq!(
            serde_json::to_vec(&first_program).unwrap(),
            serde_json::to_vec(&second_program).unwrap()
        );
        for artifact in first_program
            .artifacts()
            .iter()
            .filter(|artifact| artifact.kind == "file")
        {
            let bytes = first_sources.get(&artifact.id).unwrap();
            assert_eq!(
                artifact.content_hash.as_ref(),
                Some(&ContentHash::sha256(bytes))
            );
        }
        let anchors = first_program.accepted_rust_symbol_anchors().unwrap();
        for (artifact_id, source_artifact_id, owner, method) in FIXTURE_METHOD_FACTS {
            let source = first_sources
                .get(&StableId::parse(source_artifact_id).unwrap())
                .unwrap();
            assert_eq!(
                anchors.get(&StableId::parse(artifact_id).unwrap()),
                Some(&extract_method_fact(source, owner, method).unwrap().anchor)
            );
        }

        let mut mutated = expected;
        mutated
            .get_mut(&StableId::parse("file:checkout-controller").unwrap())
            .unwrap()
            .push(b' ');
        assert!(matches!(
            fixed_program_from_sources(&mutated),
            Err(FixedOfflineError::FixtureBinding)
        ));
    }
}
