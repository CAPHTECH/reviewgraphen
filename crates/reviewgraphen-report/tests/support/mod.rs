//! Canonical, source-bound D2 fixtures shared by report integration tests.
//!
//! The fixture intentionally writes the same durable facts that production
//! replay consumes: canonical V2 genesis bytes, source CAS objects, source
//! registrations, snapshot-source closure, plan, lifecycle transitions, and
//! a context envelope. It does not manufacture report-shaped input.

use reviewgraphen_core::{
    ArtifactRegistered, ArtifactSensitivity, ArtifactSource, ContentHash, DecodedPayload,
    EventAdmissions, EventCommand, EventLog, MvpRulePack, ObligationLifecycle, PlanBudget,
    ProgramSpace, ReviewAggregate, SnapshotSourceRecordEntry, SnapshotSourcesRecorded, StableId,
    plan, prepare_context,
};
use reviewgraphen_runtime::FakeAttemptSelection;
use reviewgraphen_store::{
    CasHash, CasStore, DerivedIndex, EventJournal, JournalGenesis, JournalIdentity, StoreLimits,
    StoreRoot,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Cursor};

pub struct SourceFixture {
    _workspace: tempfile::TempDir,
    pub root: StoreRoot,
    pub identity: JournalIdentity,
    pub selection: FakeAttemptSelection,
    context_aggregate: ReviewAggregate,
    source_bytes: BTreeMap<StableId, Vec<u8>>,
}

impl SourceFixture {
    pub fn journal(&self) -> EventJournal<'_> {
        EventJournal::open(&self.root, self.identity.clone()).unwrap()
    }

    pub fn rebuild_index(&self, journal: &EventJournal<'_>) -> DerivedIndex<'_> {
        let cas = CasStore::open(&self.root).unwrap();
        let index = DerivedIndex::open(&self.root).unwrap();
        self.assert_execution_domain_closure(journal);
        index.rebuild(journal, &cas).unwrap();
        index
    }

    /// Recreates the strict replay admissions for the context envelope and
    /// any supplied durable reviewer-execution events.
    pub fn admissions(
        &self,
        journal: &EventJournal<'_>,
        raw: &[(StableId, Vec<u8>)],
    ) -> EventAdmissions {
        let reader = journal.reader().unwrap();
        let context_event = reader
            .events()
            .iter()
            .find(|event| {
                matches!(
                    event.decode_for_streaming_projection().unwrap().payload(),
                    DecodedPayload::ContextEnvelopeProjected(envelope)
                        if envelope.id() == &self.selection.envelope_id
                )
            })
            .unwrap()
            .clone();
        let rebuilt_context = build_context(
            &self.context_aggregate,
            self.selection.obligation_id.clone(),
            &self.source_bytes,
        );
        let artifacts = raw
            .iter()
            .map(|(execution_id, bytes)| {
                let event = reader
                    .events()
                    .iter()
                    .find(|event| {
                        matches!(
                            event.decode_for_streaming_projection().unwrap().payload(),
                            DecodedPayload::ReviewExecutionRecorded { execution, .. }
                                if execution.id() == execution_id
                        )
                    })
                    .unwrap()
                    .clone();
                (event, bytes.clone())
            })
            .collect();
        EventAdmissions::default()
            .with_context_projections(vec![(context_event, rebuilt_context)])
            .unwrap()
            .with_reviewer_artifacts(artifacts)
            .unwrap()
    }
}

/// Keep the source fixture honest at the same seam as the v3 index.  These
/// assertions intentionally inspect decoded canonical journal events instead
/// of manufacturing a detached execution/claim vector for the report tests.
impl SourceFixture {
    fn assert_execution_domain_closure(&self, journal: &EventJournal<'_>) {
        let reader = journal.reader().unwrap();
        let mut plan = None;
        let mut envelope = None;
        let mut registrations = BTreeMap::new();
        for event in reader.events() {
            match event.decode_for_streaming_projection().unwrap().payload() {
                DecodedPayload::ReviewPlanRecorded(value) => plan = Some(value.clone()),
                DecodedPayload::ContextEnvelopeProjected(value) => envelope = Some(value.clone()),
                DecodedPayload::ArtifactRegistered(value) => {
                    registrations.insert(value.registration_id().clone(), value.clone());
                }
                DecodedPayload::ReviewExecutionRecorded { execution, claims } => {
                    let plan = plan.as_ref().unwrap();
                    let envelope = envelope.as_ref().unwrap();
                    let wave = plan
                        .waves()
                        .iter()
                        .find(|wave| wave.id() == execution.wave_id())
                        .unwrap();
                    let registration = registrations
                        .get(execution.raw_artifact_registration_id())
                        .unwrap();
                    assert_eq!(plan.snapshot_id(), execution.snapshot_id());
                    assert_eq!(envelope.snapshot_id(), execution.snapshot_id());
                    assert_eq!(envelope.obligation_ids(), execution.obligation_ids());
                    assert!(
                        execution
                            .obligation_ids()
                            .iter()
                            .all(|id| wave.obligation_ids().contains(id))
                    );
                    assert_eq!(registration.cas_hash(), execution.raw_artifact_hash());
                    assert_eq!(registration.sensitivity(), ArtifactSensitivity::Sensitive);
                    assert!(matches!(
                        registration.source(),
                        ArtifactSource::ReviewerExecution { run_id, execution_id, reviewer_id }
                            if run_id == event.run_id()
                                && execution_id == execution.id()
                                && reviewer_id == execution.reviewer_id()
                    ));
                    let envelope_sources = envelope
                        .included_sources()
                        .iter()
                        .map(|source| source.artifact_id().clone())
                        .collect::<std::collections::BTreeSet<_>>();
                    let claim_ids = claims
                        .iter()
                        .map(|claim| claim.id().clone())
                        .collect::<std::collections::BTreeSet<_>>();
                    assert_eq!(&claim_ids, execution.parsed_claim_ids());
                    for claim in claims {
                        let obligation = self
                            .context_aggregate
                            .obligations()
                            .find(|obligation| claim.obligation_ids().contains(obligation.id()))
                            .unwrap();
                        assert_eq!(claim.execution_id(), execution.id());
                        assert_eq!(claim.obligation_ids(), execution.obligation_ids());
                        assert_eq!(claim.property_id(), obligation.property_id());
                        assert!(
                            claim
                                .target_refs()
                                .is_subset(obligation.normalized_target_refs())
                        );
                        assert!(claim.source_ids().is_subset(&envelope_sources));
                    }
                    assert_eq!(execution.outcome().is_structured(), !claims.is_empty());
                }
                _ => {}
            }
        }
    }
}

pub fn source_fixture() -> SourceFixture {
    let mut program_value: Value = serde_json::from_str(include_str!(
        "../../../../examples/double-submit-payment/program-space.json"
    ))
    .unwrap();
    program_value["profile"]["rule_set_hash"] = Value::String(format!("sha256:{}", "3".repeat(64)));
    program_value["extraction"]["adapter_set_hash"] =
        Value::String(format!("sha256:{}", "7".repeat(64)));
    let mut relation = program_value["relations"][0].clone();
    relation["id"] = json!("relation:file-contains-payment-charge-report");
    relation["kind"] = json!("contains");
    relation["source_id"] = json!("file:payment-repository");
    relation["target_ids"] = json!(["function:payment-charge"]);
    relation["directed"] = json!(true);
    program_value["relations"]
        .as_array_mut()
        .unwrap()
        .push(relation);
    let test = program_value["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|artifact| artifact["id"] == "test:double-submit")
        .unwrap();
    test["location"]["start_line"] = Value::Null;
    test["location"]["end_line"] = Value::Null;

    let mut source_bytes = BTreeMap::new();
    for artifact in program_value["artifacts"].as_array_mut().unwrap() {
        if artifact["kind"] != "file" {
            continue;
        }
        let bytes = format!("// {}\n", artifact["id"].as_str().unwrap())
            .repeat(10_000)
            .into_bytes();
        artifact["content_hash"] = json!(ContentHash::sha256(&bytes).to_string());
        source_bytes.insert(
            StableId::parse(artifact["id"].as_str().unwrap()).unwrap(),
            bytes,
        );
    }
    let program: ProgramSpace = serde_json::from_value(program_value).unwrap();
    let (universe, obligations) = MvpRulePack::synthesize(&program).unwrap().into_parts();
    let initial = ReviewAggregate::new(program.clone(), universe, obligations).unwrap();
    let run_id = StableId::parse("run:report-source-fixture").unwrap();
    let mut log = EventLog::new(run_id.clone(), initial).unwrap();

    let mut source_entries = Vec::new();
    for artifact in program
        .artifacts()
        .iter()
        .filter(|artifact| artifact.kind == "file")
    {
        let bytes = source_bytes[&artifact.id].clone();
        let hash = ContentHash::sha256(&bytes);
        let source = ArtifactSource::SnapshotIngest {
            run_id: run_id.clone(),
            snapshot_id: program.snapshot_id().clone(),
            adapter_id: "report-source-fixture@1".to_owned(),
        };
        let registration_id = StableId::derived(
            "registration",
            &BTreeMap::from([
                ("run_id".to_owned(), Value::String(run_id.to_string())),
                ("cas_hash".to_owned(), Value::String(hash.to_string())),
                (
                    "media_type".to_owned(),
                    Value::String("text/plain".to_owned()),
                ),
                (
                    "sensitivity".to_owned(),
                    Value::String("workspace_source".to_owned()),
                ),
                ("source".to_owned(), serde_json::to_value(&source).unwrap()),
            ]),
        )
        .unwrap();
        log.append(EventCommand::artifact_registered(
            ArtifactRegistered::new(
                run_id.clone(),
                registration_id.clone(),
                hash.clone(),
                "text/plain",
                u64::try_from(bytes.len()).unwrap(),
                ArtifactSensitivity::WorkspaceSource,
                source,
            )
            .unwrap(),
        ))
        .unwrap();
        source_entries.push(
            SnapshotSourceRecordEntry::new(
                artifact.id.clone(),
                artifact.location.as_ref().unwrap().path.clone(),
                hash.clone(),
                registration_id,
                hash,
                line_count(&bytes),
            )
            .unwrap(),
        );
    }
    source_entries.sort_by(|left, right| left.path().cmp(right.path()));
    log.append(EventCommand::snapshot_sources_recorded(
        SnapshotSourcesRecorded::new(program.snapshot_id().clone(), source_entries).unwrap(),
    ))
    .unwrap();
    let plan = plan(log.aggregate(), PlanBudget::new(16, 16).unwrap()).unwrap();
    log.append(EventCommand::review_plan_recorded(plan.clone()))
        .unwrap();
    let obligation_id = plan
        .waves()
        .iter()
        .flat_map(|wave| wave.obligation_ids())
        .find(|candidate| {
            !build_context(log.aggregate(), (*candidate).clone(), &source_bytes)
                .envelope()
                .normalized_included_source_ids()
                .is_empty()
        })
        .unwrap()
        .clone();
    let context_aggregate = log.aggregate().clone();
    let context = build_context(log.aggregate(), obligation_id.clone(), &source_bytes);
    let envelope_id = context.envelope().id().clone();
    let wave_id = plan
        .waves()
        .iter()
        .find(|wave| wave.obligation_ids().contains(&obligation_id))
        .unwrap()
        .id()
        .clone();
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
    log.append(EventCommand::context_envelope_projected(context))
        .unwrap();

    let workspace = tempfile::tempdir().unwrap();
    let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
    let genesis = log
        .run_genesis_snapshot()
        .unwrap()
        .canonical_bytes()
        .unwrap();
    let genesis_hash = CasHash::parse(ContentHash::sha256(&genesis).to_string()).unwrap();
    let cas = CasStore::open(&root).unwrap();
    cas.put(
        &genesis_hash,
        Some(u64::try_from(genesis.len()).unwrap()),
        Cursor::new(genesis.as_slice()),
    )
    .unwrap();
    for bytes in source_bytes.values() {
        let hash = CasHash::parse(ContentHash::sha256(bytes).to_string()).unwrap();
        cas.put(
            &hash,
            Some(u64::try_from(bytes.len()).unwrap()),
            Cursor::new(bytes),
        )
        .unwrap();
    }
    let identity = JournalIdentity::new(run_id, JournalGenesis::V2(genesis)).unwrap();
    let journal = EventJournal::initialize_v2(
        &root,
        identity.clone(),
        log.events().first().unwrap().envelope().clone(),
    )
    .unwrap();
    let mut writer = journal.writer().unwrap();
    for event in log.events().iter().skip(1) {
        writer.append(event.envelope().clone()).unwrap();
    }
    drop(writer);

    SourceFixture {
        _workspace: workspace,
        root,
        identity,
        selection: FakeAttemptSelection {
            plan_id: plan.id().clone(),
            wave_id,
            envelope_id,
            obligation_id,
            attempt: 1,
        },
        context_aggregate,
        source_bytes,
    }
}

fn build_context(
    aggregate: &ReviewAggregate,
    obligation_id: StableId,
    source_bytes: &BTreeMap<StableId, Vec<u8>>,
) -> reviewgraphen_core::BuiltContextProjection {
    let mut session = prepare_context(aggregate, obligation_id).unwrap();
    while let Some(request) = session.next_source_request().unwrap() {
        session
            .submit_source(&request, &source_bytes[request.artifact_id()])
            .unwrap();
    }
    session.finish().unwrap()
}

fn line_count(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.iter().filter(|byte| **byte == b'\n').count()).unwrap() + 1
}
