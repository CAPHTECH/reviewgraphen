//! Public-API event-v3 fixture used by report-v3 source-bound tests.

use reviewgraphen_core::{
    ArtifactRegisteredV3, ArtifactSensitivity, ArtifactSourceV3, AuthorityTrustRootsV3,
    ClaimPolarity, ContentHash, DecisionInputV3, DecisionOutcomeV3, EventCommand, EventLog,
    ExecutionClaimInputV2, ExecutionOutcome, ExecutionRecordInput, FINDING_PROJECTION_ID,
    FIXTURE_DESCRIPTOR_ID, FIXTURE_HARNESS_ID, FIXTURE_HARNESS_REVISION,
    FIXTURE_HARNESS_SOURCE_HASH, FIXTURE_MEDIA_TYPE, FIXTURE_PROCEDURE_ID,
    FIXTURE_TEST_ARTIFACT_ID, FIXTURE_WITNESS_HASH, HarnessTrustRootInputV3,
    HumanAuthorityCapabilityV3, HumanTrustGrantInputV3, M4_PROPERTY_ID, MvpRulePack,
    ObligationLifecycle, PlanBudget, ProgramSpace, ReviewAggregate, SnapshotSourceRecordEntry,
    SnapshotSourcesRecorded, StableId, ValidatedExecutionBundle, VerifierArtifactRoleV3,
    canonical_json, evaluate_static_fact_v1, plan, prepare_context,
};
use reviewgraphen_report::{ReportRequestV3, generate_v3};
use reviewgraphen_runtime::m4_verification::{
    record_current_finding, record_human_decision, verify_fixed_fixture, verify_static_fact,
};
use reviewgraphen_store::{
    CasHash, CasStore, DerivedIndexV4, EventJournal, JournalGenesis, JournalIdentity, StoreLimits,
    StoreRoot,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};

pub struct V3Fixture {
    _workspace: tempfile::TempDir,
    pub root: StoreRoot,
    pub identity: JournalIdentity,
    pub roots: AuthorityTrustRootsV3,
    pub obligation_id: StableId,
    pub claim_id: StableId,
    pub plan_id: StableId,
    pub snapshot_id: StableId,
    pub universe_id: StableId,
}

impl V3Fixture {
    pub fn journal(&self) -> EventJournal<'_> {
        EventJournal::open(&self.root, self.identity.clone()).unwrap()
    }

    pub fn rebuild(&self) {
        let journal = self.journal();
        DerivedIndexV4::open(&self.root)
            .unwrap()
            .rebuild_v4(&journal, &self.roots)
            .unwrap();
    }

    pub fn request(&self, report_id: &str) -> ReportRequestV3 {
        ReportRequestV3 {
            report_id: id(report_id),
            repository_id: self.roots.repository_id().clone(),
            program_space_ref: id(&format!("program-space:{}", self.snapshot_id)),
            plan_id: self.plan_id.clone(),
            selected_obligation_ids: BTreeSet::from([self.obligation_id.clone()]),
            tool_versions: BTreeMap::from([
                ("reviewgraphen.runtime".into(), "0.1.0".into()),
                ("reviewgraphen.verifier.fixture".into(), "1".into()),
            ]),
        }
    }

    pub fn generated(&self, report_id: &str) -> reviewgraphen_report::GeneratedReport {
        generate_v3(
            &self.root,
            self.identity.clone(),
            &self.roots,
            &self.request(report_id),
        )
        .unwrap()
    }

    pub fn append_gap(&self) {
        let bytes = b"report-v3 stale tail gap";
        put(&self.root, bytes);
        let journal = self.journal();
        let (mut session, mut basis) = journal.replayed_v3_session(&self.roots).unwrap();
        let registration = ArtifactRegisteredV3::new(
            session.run_id().unwrap().clone(),
            ContentHash::sha256(bytes),
            "text/plain",
            bytes.len() as u64,
            ArtifactSensitivity::WorkspaceSource,
            ArtifactSourceV3::SnapshotIngest {
                adapter_id: "report-v3-stale-gap".into(),
                run_id: session.run_id().unwrap().clone(),
                snapshot_id: self.snapshot_id.clone(),
            },
        )
        .unwrap();
        session
            .append_nonauthority_registration_v3(registration, &mut basis)
            .unwrap();
    }

    pub fn append_static_conflict(&self) {
        let journal = self.journal();
        let (mut session, mut basis) = journal.replayed_v3_session(&self.roots).unwrap();
        let evaluation = {
            let aggregate = session.aggregate().unwrap();
            let claim = aggregate
                .execution_claims()
                .find(|claim| claim.id() == &self.claim_id)
                .unwrap();
            let obligation = aggregate
                .obligations()
                .find(|obligation| Some(obligation.id()) == claim.obligation_ids().first())
                .unwrap();
            evaluate_static_fact_v1(aggregate.program(), obligation, claim).unwrap()
        };
        let input_bytes = canonical_json(evaluation.input()).unwrap();
        let output_bytes = canonical_json(evaluation.result()).unwrap();
        put(&self.root, &input_bytes);
        put(&self.root, &output_bytes);
        let input = session
            .prepare_static_verifier_artifact_registration(
                self.claim_id.clone(),
                VerifierArtifactRoleV3::Input,
                ContentHash::sha256(&input_bytes),
                input_bytes.len() as u64,
                &basis,
            )
            .unwrap();
        let input_id = input.registration_id().clone();
        session
            .append_authority_registration(input, &mut basis)
            .unwrap();
        let output = session
            .prepare_static_verifier_artifact_registration(
                self.claim_id.clone(),
                VerifierArtifactRoleV3::Output,
                ContentHash::sha256(&output_bytes),
                output_bytes.len() as u64,
                &basis,
            )
            .unwrap();
        let output_id = output.registration_id().clone();
        session
            .append_authority_registration(output, &mut basis)
            .unwrap();
        let bundle = session
            .mint_static_verification_bundle(&self.claim_id, &input_id, &output_id, &basis)
            .unwrap();
        session
            .append_verification_bundle(bundle, &mut basis)
            .unwrap();
    }
}

fn id(value: &str) -> StableId {
    StableId::parse(value).unwrap()
}

fn put(root: &StoreRoot, bytes: &[u8]) {
    let hash = CasHash::parse(ContentHash::sha256(bytes).to_string()).unwrap();
    CasStore::open(root)
        .unwrap()
        .put(
            &hash,
            Some(u64::try_from(bytes.len()).unwrap()),
            Cursor::new(bytes),
        )
        .unwrap();
}

pub fn accepted_fixture() -> V3Fixture {
    fixture_with_authority(true)
}

pub fn static_only_fixture() -> V3Fixture {
    fixture_with_authority(false)
}

fn fixture_with_authority(accepted: bool) -> V3Fixture {
    let workspace = tempfile::tempdir().unwrap();
    let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
    let mut input: Value = serde_json::from_slice(include_bytes!(
        "../../../../examples/double-submit-payment/program-space.json"
    ))
    .unwrap();
    input["source"]["content_hash"] = Value::String(format!("sha256:{}", "11".repeat(32)));
    input["profile"]["rule_set_hash"] = Value::String(format!("sha256:{}", "33".repeat(32)));
    input["extraction"]["adapter_set_hash"] = Value::String(format!("sha256:{}", "77".repeat(32)));
    let mut contains = input["relations"][0].clone();
    contains["id"] = Value::String("relation:file-contains-payment-charge-report-v3".into());
    contains["kind"] = Value::String("contains".into());
    contains["source_id"] = Value::String("file:payment-repository".into());
    contains["target_ids"] = json!(["function:payment-charge"]);
    contains["directed"] = Value::Bool(true);
    input["relations"].as_array_mut().unwrap().push(contains);
    let test = input["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|artifact| artifact["id"] == FIXTURE_TEST_ARTIFACT_ID)
        .unwrap();
    test["location"]["start_line"] = Value::Null;
    test["location"]["end_line"] = Value::Null;
    let invariant = input["invariants"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|invariant| invariant["property_id"] == M4_PROPERTY_ID)
        .unwrap();
    invariant["scope_ids"] = json!(["context:payment", "context:ui-event"]);
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
    let repository_source_hash =
        ContentHash::parse(input["source"]["content_hash"].as_str().unwrap()).unwrap();
    let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&input).unwrap()).unwrap();
    let repository_id = program.repository_id().clone();
    let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
    let mut log = EventLog::new_v3(
        id("run:report-v3-source-bound"),
        ReviewAggregate::new(program, universe, obligations).unwrap(),
    )
    .unwrap();
    let run_id = log.run_id().clone();
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
        let bytes = bytes_by_path[path.as_str()].clone();
        put(&root, &bytes);
        let hash = ContentHash::sha256(&bytes);
        let registration = ArtifactRegisteredV3::new(
            run_id.clone(),
            hash.clone(),
            "text/plain",
            bytes.len() as u64,
            ArtifactSensitivity::WorkspaceSource,
            ArtifactSourceV3::SnapshotIngest {
                adapter_id: "report-v3-source-fixture".into(),
                run_id: run_id.clone(),
                snapshot_id: snapshot_id.clone(),
            },
        )
        .unwrap();
        entries.push(
            SnapshotSourceRecordEntry::new(
                artifact.id.clone(),
                path,
                hash.clone(),
                registration.registration_id().clone(),
                hash,
                bytes.iter().filter(|byte| **byte == b'\n').count() as u64 + 1,
            )
            .unwrap(),
        );
        source_by_id.insert(artifact.id, bytes);
        log.append(EventCommand::artifact_registered_v3(registration))
            .unwrap();
    }
    entries.sort_by(|left, right| left.path().cmp(right.path()));
    log.append(EventCommand::snapshot_sources_recorded(
        SnapshotSourcesRecorded::new(snapshot_id.clone(), entries).unwrap(),
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
            let obligation = log
                .aggregate()
                .obligations()
                .find(|obligation| obligation.id() == candidate)?;
            if obligation.property_id() != M4_PROPERTY_ID {
                return None;
            }
            let mut context = prepare_context(log.aggregate(), candidate.clone()).ok()?;
            while let Some(request) = context.next_source_request().ok()? {
                context
                    .submit_source(&request, &source_by_id[request.artifact_id()])
                    .ok()?;
            }
            let built = context.finish().ok()?;
            (!built.envelope().normalized_included_source_ids().is_empty())
                .then(|| (candidate.clone(), built))
        })
        .unwrap();
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
    let wave = review_plan
        .waves()
        .iter()
        .find(|wave| wave.obligation_ids().contains(&obligation_id))
        .unwrap();
    let execution_input = ExecutionRecordInput::fake(
        review_plan.id().clone(),
        wave.id().clone(),
        obligation_id.clone(),
        envelope.id().clone(),
        envelope.snapshot_id().clone(),
        1,
    )
    .unwrap();
    let execution_id = execution_input.execution_id().unwrap();
    let raw = br#"{"attempt":1,"fixture":true,"version":3}"#.to_vec();
    put(&root, &raw);
    let raw_registration = ArtifactRegisteredV3::new(
        run_id.clone(),
        ContentHash::sha256(&raw),
        "application/json",
        raw.len() as u64,
        ArtifactSensitivity::Sensitive,
        ArtifactSourceV3::ReviewerExecution {
            execution_id,
            reviewer_id: reviewgraphen_core::FAKE_REVIEWER_ID.into(),
            run_id: run_id.clone(),
        },
    )
    .unwrap();
    log.append(EventCommand::artifact_registered_v3(
        raw_registration.clone(),
    ))
    .unwrap();
    let obligation = log
        .aggregate()
        .obligations()
        .find(|obligation| obligation.id() == &obligation_id)
        .unwrap();
    let claim = ExecutionClaimInputV2::new(
        obligation.property_id(),
        obligation.normalized_target_refs().clone(),
        ClaimPolarity::IssuePresent,
        "source-bound report-v3 fixture demonstrates duplicate submit",
        envelope.normalized_included_source_ids().clone(),
        BTreeSet::new(),
        BTreeSet::new(),
        Some(1.0),
    )
    .unwrap();
    let source_buffers = source_by_id.values().collect::<Vec<_>>();
    let execution = ValidatedExecutionBundle::fake_v3(
        execution_input,
        &raw_registration,
        raw,
        source_buffers,
        vec![claim],
        ExecutionOutcome::Structured,
    )
    .unwrap();
    let claim_id = execution.claims()[0].id().clone();
    log.append(EventCommand::review_execution_recorded(execution))
        .unwrap();

    let policy = ContentHash::sha256(b"report-v3-source-bound-policy");
    let claim_record = log
        .aggregate()
        .execution_claims()
        .find(|claim| claim.id() == &claim_id)
        .unwrap();
    let roots = AuthorityTrustRootsV3::new(
        policy.clone(),
        repository_id.clone(),
        repository_source_hash.clone(),
        vec![HarnessTrustRootInputV3 {
            policy_revision_hash: policy.clone(),
            repository_id,
            repository_source_hash,
            harness_id: FIXTURE_HARNESS_ID.into(),
            harness_revision: FIXTURE_HARNESS_REVISION.into(),
            harness_source_hash: ContentHash::parse(FIXTURE_HARNESS_SOURCE_HASH).unwrap(),
            test_artifact_id: id(FIXTURE_TEST_ARTIFACT_ID),
            descriptor_id: FIXTURE_DESCRIPTOR_ID.into(),
            procedure_version: FIXTURE_PROCEDURE_ID.into(),
            result_hash: ContentHash::parse(FIXTURE_WITNESS_HASH).unwrap(),
            result_size: 145,
            result_media_type: FIXTURE_MEDIA_TYPE.into(),
            result_sensitivity: ArtifactSensitivity::CanonicalState,
            run_id: run_id.clone(),
            genesis_hash: log.genesis_hash().clone(),
            snapshot_id: snapshot_id.clone(),
            universe_id: log.aggregate().universe().id().clone(),
            property_id: M4_PROPERTY_ID.into(),
            claim_id: claim_id.clone(),
            claim_body_hash: claim_record.body_hash().unwrap(),
        }],
        vec![HumanTrustGrantInputV3 {
            policy_revision_hash: policy,
            actor: "human:report-reviewer".into(),
            authority_id: "report-review-board".into(),
            capabilities: BTreeSet::from([
                HumanAuthorityCapabilityV3::AcceptFinding,
                HumanAuthorityCapabilityV3::RejectFinding,
            ]),
            run_id: run_id.clone(),
            snapshot_id: snapshot_id.clone(),
            universe_id: log.aggregate().universe().id().clone(),
            property_ids: BTreeSet::from([M4_PROPERTY_ID.into()]),
            claim_ids: BTreeSet::from([claim_id.clone()]),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_until: "2027-01-01T00:00:00Z".into(),
        }],
    )
    .unwrap();
    let genesis = log
        .run_genesis_snapshot()
        .unwrap()
        .canonical_bytes()
        .unwrap();
    put(&root, &genesis);
    let identity = JournalIdentity::new(run_id.clone(), JournalGenesis::V3(genesis)).unwrap();
    let journal =
        EventJournal::initialize_v3(&root, identity.clone(), log.events()[0].envelope().clone())
            .unwrap();
    let (mut session, mut basis) = journal.replayed_v3_session(&roots).unwrap();
    let files = session
        .aggregate()
        .unwrap()
        .program()
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
        .cloned()
        .collect::<Vec<_>>();
    let mut entries = Vec::new();
    for artifact in files {
        let path = artifact.location.as_ref().unwrap().path.clone();
        let bytes = &source_by_id[&artifact.id];
        let hash = ContentHash::sha256(bytes);
        let registration = ArtifactRegisteredV3::new(
            run_id.clone(),
            hash.clone(),
            "text/plain",
            bytes.len() as u64,
            ArtifactSensitivity::WorkspaceSource,
            ArtifactSourceV3::SnapshotIngest {
                adapter_id: "report-v3-source-fixture".into(),
                run_id: run_id.clone(),
                snapshot_id: snapshot_id.clone(),
            },
        )
        .unwrap();
        entries.push(
            SnapshotSourceRecordEntry::new(
                artifact.id,
                path,
                hash.clone(),
                registration.registration_id().clone(),
                hash,
                bytes.iter().filter(|byte| **byte == b'\n').count() as u64 + 1,
            )
            .unwrap(),
        );
        session
            .append_nonauthority_registration_v3(registration, &mut basis)
            .unwrap();
    }
    entries.sort_by(|left, right| left.path().cmp(right.path()));
    session
        .append_snapshot_sources_v3(
            SnapshotSourcesRecorded::new(snapshot_id.clone(), entries).unwrap(),
            &mut basis,
        )
        .unwrap();
    let persisted_plan = plan(
        session.aggregate().unwrap(),
        PlanBudget::new(16, 16).unwrap(),
    )
    .unwrap();
    let plan_id = persisted_plan.id().clone();
    session
        .append_review_plan_v3(persisted_plan.clone(), &mut basis)
        .unwrap();
    session
        .append_obligation_transition_v3(
            obligation_id.clone(),
            ObligationLifecycle::Planned,
            &mut basis,
        )
        .unwrap();
    session
        .append_obligation_transition_v3(
            obligation_id.clone(),
            ObligationLifecycle::InProgress,
            &mut basis,
        )
        .unwrap();
    let mut context = prepare_context(session.aggregate().unwrap(), obligation_id.clone()).unwrap();
    while let Some(request) = context.next_source_request().unwrap() {
        context
            .submit_source(&request, &source_by_id[request.artifact_id()])
            .unwrap();
    }
    let built = context.finish().unwrap();
    let envelope = built.envelope().clone();
    session
        .append_context_projection_v3(built, &mut basis)
        .unwrap();
    let wave = persisted_plan
        .waves()
        .iter()
        .find(|wave| wave.obligation_ids().contains(&obligation_id))
        .unwrap();
    let execution_input = ExecutionRecordInput::fake(
        persisted_plan.id().clone(),
        wave.id().clone(),
        obligation_id.clone(),
        envelope.id().clone(),
        envelope.snapshot_id().clone(),
        1,
    )
    .unwrap();
    let execution_id = execution_input.execution_id().unwrap();
    let raw = br#"{"attempt":1,"fixture":true,"version":3}"#.to_vec();
    let raw_registration = ArtifactRegisteredV3::new(
        run_id,
        ContentHash::sha256(&raw),
        "application/json",
        raw.len() as u64,
        ArtifactSensitivity::Sensitive,
        ArtifactSourceV3::ReviewerExecution {
            execution_id,
            reviewer_id: reviewgraphen_core::FAKE_REVIEWER_ID.into(),
            run_id: session.run_id().unwrap().clone(),
        },
    )
    .unwrap();
    session
        .append_nonauthority_registration_v3(raw_registration.clone(), &mut basis)
        .unwrap();
    let obligation = session
        .aggregate()
        .unwrap()
        .obligations()
        .find(|obligation| obligation.id() == &obligation_id)
        .unwrap();
    let claim = ExecutionClaimInputV2::new(
        obligation.property_id(),
        obligation.normalized_target_refs().clone(),
        ClaimPolarity::IssuePresent,
        "source-bound report-v3 fixture demonstrates duplicate submit",
        envelope.normalized_included_source_ids().clone(),
        BTreeSet::new(),
        BTreeSet::new(),
        Some(1.0),
    )
    .unwrap();
    let source_buffers = source_by_id.values().collect::<Vec<_>>();
    let execution = ValidatedExecutionBundle::fake_v3(
        execution_input,
        &raw_registration,
        raw,
        source_buffers,
        vec![claim],
        ExecutionOutcome::Structured,
    )
    .unwrap();
    assert_eq!(execution.claims()[0].id(), &claim_id);
    session
        .append_review_execution_v3(execution, &mut basis)
        .unwrap();
    if accepted {
        verify_fixed_fixture(&mut session, &root, &claim_id, &mut basis).unwrap();
        record_human_decision(
            &mut session,
            &claim_id,
            DecisionInputV3::new(
                DecisionOutcomeV3::Accept,
                "human:report-reviewer",
                "report-review-board",
                "explicit acceptance of the reproduced source-bound witness",
                "2026-08-10T00:00:00Z",
                None,
            ),
            &mut basis,
        )
        .unwrap();
        record_current_finding(&mut session, &claim_id, FINDING_PROJECTION_ID, &mut basis).unwrap();
    } else {
        verify_static_fact(&mut session, &root, &claim_id, &mut basis).unwrap();
    }
    session
        .append_obligation_transition_v3(
            obligation_id.clone(),
            ObligationLifecycle::Completed,
            &mut basis,
        )
        .unwrap();
    let universe_id = session.aggregate().unwrap().universe().id().clone();
    drop(session);
    drop(journal);
    let fixture = V3Fixture {
        _workspace: workspace,
        root,
        identity,
        roots,
        obligation_id,
        claim_id,
        plan_id,
        snapshot_id,
        universe_id,
    };
    fixture.rebuild();
    fixture
}
