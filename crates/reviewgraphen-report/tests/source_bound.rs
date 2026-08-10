mod support;

use reviewgraphen_core::{
    ArtifactRegistered, ArtifactSensitivity, ArtifactSource, ContentHash, EventCommand,
    FAKE_REVIEWER_ID, StableId, canonical_json,
};
use reviewgraphen_report::{
    PreReviewObstruction, ReportError, ReportLimits, ReportRequest, generate_v2,
    generate_v2_with_limits,
};
use reviewgraphen_reviewer::{FakeFixture, FakeReviewer, FixtureKey, ReviewerOutcome};
use reviewgraphen_runtime::{prepare_fake_attempt, run_fresh_fake_attempt};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn request(fixture: &support::SourceFixture, report_id: &str) -> ReportRequest {
    ReportRequest {
        report_id: StableId::parse(report_id).unwrap(),
        repository_id: StableId::parse("repository:double-submit-payment").unwrap(),
        program_space_ref: StableId::parse("program-space:snapshot:double-submit-v1").unwrap(),
        plan_id: fixture.selection.plan_id.clone(),
        selected_obligation_ids: BTreeSet::from([fixture.selection.obligation_id.clone()]),
        tool_versions: BTreeMap::from([("reviewgraphen.fake-reviewer".to_owned(), "1".to_owned())]),
        pre_review_obstructions: vec![],
    }
}

fn structured_fixture(
    fixture: &support::SourceFixture,
    session: &reviewgraphen_store::ReplayedV2RunSession,
    selection: reviewgraphen_runtime::FakeAttemptSelection,
) -> (FixtureKey, FakeFixture) {
    let prepared = prepare_fake_attempt(session, &fixture.root, selection).unwrap();
    let obligation = session
        .aggregate()
        .unwrap()
        .obligations()
        .find(|value| value.id() == &fixture.selection.obligation_id)
        .unwrap();
    let target = obligation.normalized_target_refs().iter().next().unwrap();
    let source = prepared
        .request()
        .envelope()
        .normalized_included_source_ids()
        .iter()
        .next()
        .unwrap();
    let raw = format!("{{\"abstention\":null,\"claims\":[{{\"assumptions\":[],\"candidate_confidence\":1.0,\"polarity\":\"issue_present\",\"property_id\":\"{}\",\"requested_evidence\":[],\"source_ids\":[\"{}\"],\"summary\":\"source-bound fixture issue\",\"target_refs\":[\"{}\"]}}],\"execution_id\":\"{}\",\"schema\":\"reviewgraphen.reviewer_output.v1\"}}", obligation.property_id(), source, target, prepared.execution_id()).into_bytes();
    (
        FixtureKey::new(
            BTreeSet::from([fixture.selection.obligation_id.clone()]),
            prepared.request().envelope().snapshot_id().clone(),
        )
        .unwrap(),
        FakeFixture::new(raw, ReviewerOutcome::Structured).unwrap(),
    )
}

#[test]
fn structured_execution_generates_completed_schema_valid_report() {
    let fixture = support::source_fixture();
    let journal = fixture.journal();
    let admissions = fixture.admissions(&journal, &[]);
    let mut session = journal.replayed_v2_session(&admissions).unwrap();
    let reviewer = FakeReviewer::new(vec![structured_fixture(
        &fixture,
        &session,
        fixture.selection.clone(),
    )])
    .unwrap();
    run_fresh_fake_attempt(
        &mut session,
        &fixture.root,
        &reviewer,
        fixture.selection.clone(),
    )
    .unwrap();
    drop(session);
    fixture.rebuild_index(&journal);
    let first = generate_v2(
        &fixture.root,
        fixture.identity.clone(),
        &request(&fixture, "report:source-bound-completed"),
    )
    .unwrap();
    let second = generate_v2(
        &fixture.root,
        fixture.identity.clone(),
        &request(&fixture, "report:source-bound-completed"),
    )
    .unwrap();
    assert_eq!(first.canonical_bytes, second.canonical_bytes);
    let exact_counts = ReportLimits {
        executions: 1,
        raw_registrations: 1,
        claims: 1,
        obstructions: 0,
        views: 1,
        information_loss_records: 1,
        rows: 5,
        ..ReportLimits::default()
    };
    assert!(
        generate_v2_with_limits(
            &fixture.root,
            fixture.identity.clone(),
            &request(&fixture, "report:source-bound-completed"),
            exact_counts,
        )
        .is_ok()
    );
    for constrained in [
        ReportLimits {
            executions: 0,
            ..exact_counts
        },
        ReportLimits {
            raw_registrations: 0,
            ..exact_counts
        },
        ReportLimits {
            claims: 0,
            ..exact_counts
        },
        ReportLimits {
            rows: 4,
            ..exact_counts
        },
    ] {
        assert!(matches!(
            generate_v2_with_limits(
                &fixture.root,
                fixture.identity.clone(),
                &request(&fixture, "report:source-bound-completed"),
                constrained,
            ),
            Err(ReportError::Incomplete { .. })
        ));
    }
    let report: Value = serde_json::from_slice(&first.canonical_bytes).unwrap();
    assert_eq!(first.canonical_bytes, canonical_json(&report).unwrap());
    assert_eq!(report["result"]["status"], "completed");
    assert_eq!(
        report["result"]["executions"][0]["reviewer_id"],
        FAKE_REVIEWER_ID
    );
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v2.schema.json"
    ))
    .unwrap();
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&report)
    );
}

#[test]
fn provider_failure_generates_partial_schema_valid_report() {
    let fixture = support::source_fixture();
    let journal = fixture.journal();
    let admissions = fixture.admissions(&journal, &[]);
    let mut session = journal.replayed_v2_session(&admissions).unwrap();
    let key = FixtureKey::new(
        BTreeSet::from([fixture.selection.obligation_id.clone()]),
        session
            .aggregate()
            .unwrap()
            .universe()
            .snapshot_id()
            .clone(),
    )
    .unwrap();
    let raw = b"{\"diagnostic\":\"fixture provider failure\",\"kind\":\"provider_failure\",\"retryable\":true}".to_vec();
    let reviewer = FakeReviewer::new(vec![(
        key,
        FakeFixture::new(
            raw,
            ReviewerOutcome::ProviderFailure {
                retryable: true,
                diagnostic: "fixture provider failure".to_owned(),
            },
        )
        .unwrap(),
    )])
    .unwrap();
    run_fresh_fake_attempt(
        &mut session,
        &fixture.root,
        &reviewer,
        fixture.selection.clone(),
    )
    .unwrap();
    drop(session);
    fixture.rebuild_index(&journal);
    let report: Value = serde_json::from_slice(
        &generate_v2(
            &fixture.root,
            fixture.identity.clone(),
            &request(&fixture, "report:source-bound-partial"),
        )
        .unwrap()
        .canonical_bytes,
    )
    .unwrap();
    assert_eq!(report["result"]["status"], "partial");
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v2.schema.json"
    ))
    .unwrap();
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&report)
    );
}

#[test]
fn retry_attempts_are_complete_source_bound_history_not_a_latest_attempt_view() {
    let fixture = support::source_fixture();
    let journal = fixture.journal();
    let admissions = fixture.admissions(&journal, &[]);
    let mut first_session = journal.replayed_v2_session(&admissions).unwrap();
    let key = FixtureKey::new(
        BTreeSet::from([fixture.selection.obligation_id.clone()]),
        first_session
            .aggregate()
            .unwrap()
            .universe()
            .snapshot_id()
            .clone(),
    )
    .unwrap();
    let failure_raw =
        b"{\"diagnostic\":\"retry then succeed\",\"kind\":\"provider_failure\",\"retryable\":true}"
            .to_vec();
    let first_reviewer = FakeReviewer::new(vec![(
        key,
        FakeFixture::new(
            failure_raw.clone(),
            ReviewerOutcome::ProviderFailure {
                retryable: true,
                diagnostic: "retry then succeed".to_owned(),
            },
        )
        .unwrap(),
    )])
    .unwrap();
    let first = run_fresh_fake_attempt(
        &mut first_session,
        &fixture.root,
        &first_reviewer,
        fixture.selection.clone(),
    )
    .unwrap();
    drop(first_session);

    let admissions = fixture.admissions(&journal, &[(first.execution_id, failure_raw)]);
    let mut second_session = journal.replayed_v2_session(&admissions).unwrap();
    let mut second_selection = fixture.selection.clone();
    second_selection.attempt = 2;
    let second_reviewer = FakeReviewer::new(vec![structured_fixture(
        &fixture,
        &second_session,
        second_selection.clone(),
    )])
    .unwrap();
    run_fresh_fake_attempt(
        &mut second_session,
        &fixture.root,
        &second_reviewer,
        second_selection,
    )
    .unwrap();
    drop(second_session);
    fixture.rebuild_index(&journal);

    let report: Value = serde_json::from_slice(
        &generate_v2(
            &fixture.root,
            fixture.identity.clone(),
            &request(&fixture, "report:source-bound-retry"),
        )
        .unwrap()
        .canonical_bytes,
    )
    .unwrap();
    assert_eq!(report["result"]["status"], "completed");
    assert_eq!(report["result"]["executions"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["result"]["artifact_registrations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(report["result"]["claims"].as_array().unwrap().len(), 1);
    assert_eq!(
        report["result"]["executions"][0]["outcome"]["kind"],
        "provider_failure"
    );
    assert_eq!(
        report["result"]["executions"][1]["outcome"]["kind"],
        "structured"
    );
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v2.schema.json"
    ))
    .unwrap();
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&report)
    );
}

#[test]
fn zero_attempt_source_fixture_generates_unsupported_input_only_with_exact_obstruction() {
    let fixture = support::source_fixture();
    let journal = fixture.journal();
    fixture.rebuild_index(&journal);
    let mut report_request = request(&fixture, "report:source-bound-unsupported");
    report_request.pre_review_obstructions = vec![PreReviewObstruction {
        message: "The source fixture deliberately has no reviewer attempt.".to_owned(),
        source_ids: BTreeSet::from([fixture.selection.obligation_id.clone()]),
        blocks: BTreeSet::from([fixture.selection.obligation_id.clone()]),
    }];
    let report: Value = serde_json::from_slice(
        &generate_v2(&fixture.root, fixture.identity.clone(), &report_request)
            .unwrap()
            .canonical_bytes,
    )
    .unwrap();
    assert_eq!(report["result"]["status"], "unsupported_input");
    assert!(
        report["result"]["executions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(report["result"]["claims"].as_array().unwrap().is_empty());
    assert!(
        report["scenario"]["artifact_registration_ids"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn report_refuses_a_stale_index_tail_and_an_out_of_plan_selected_set() {
    let fixture = support::source_fixture();
    let journal = fixture.journal();
    fixture.rebuild_index(&journal);
    let invalid_request = ReportRequest {
        selected_obligation_ids: BTreeSet::from([
            StableId::parse("obligation:not-in-plan").unwrap()
        ]),
        ..request(&fixture, "report:source-bound-invalid-selected")
    };
    assert!(matches!(
        generate_v2(&fixture.root, fixture.identity.clone(), &invalid_request),
        Err(ReportError::Source("selected obligation outside plan"))
    ));

    let admissions = fixture.admissions(&journal, &[]);
    let mut session = journal.replayed_v2_session(&admissions).unwrap();
    let run_id = session.run_id().unwrap().clone();
    let snapshot_id = session
        .aggregate()
        .unwrap()
        .universe()
        .snapshot_id()
        .clone();
    let source = ArtifactSource::SnapshotIngest {
        run_id: run_id.clone(),
        snapshot_id,
        adapter_id: "report-tail-tamper@1".to_owned(),
    };
    let hash = ContentHash::sha256(b"index must not silently become stale");
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
    session
        .append_command(EventCommand::artifact_registered(
            ArtifactRegistered::new(
                run_id,
                registration_id,
                hash,
                "text/plain",
                37,
                ArtifactSensitivity::WorkspaceSource,
                source,
            )
            .unwrap(),
        ))
        .unwrap();
    drop(session);
    assert!(matches!(
        generate_v2(
            &fixture.root,
            fixture.identity.clone(),
            &request(&fixture, "report:source-bound-stale-index"),
        ),
        Err(ReportError::Index(_))
    ));
}

#[test]
fn report_rechecks_referenced_raw_cas_bytes_hash_and_size() {
    let fixture = support::source_fixture();
    let journal = fixture.journal();
    let admissions = fixture.admissions(&journal, &[]);
    let mut session = journal.replayed_v2_session(&admissions).unwrap();
    let reviewer = FakeReviewer::new(vec![structured_fixture(
        &fixture,
        &session,
        fixture.selection.clone(),
    )])
    .unwrap();
    run_fresh_fake_attempt(
        &mut session,
        &fixture.root,
        &reviewer,
        fixture.selection.clone(),
    )
    .unwrap();
    drop(session);
    fixture.rebuild_index(&journal);
    let generated = generate_v2(
        &fixture.root,
        fixture.identity.clone(),
        &request(&fixture, "report:source-bound-cas-tamper"),
    )
    .unwrap();
    let report: Value = serde_json::from_slice(&generated.canonical_bytes).unwrap();
    let hash = report["result"]["artifact_registrations"][0]["cas_hash"]
        .as_str()
        .unwrap();
    let hex = &hash["sha256:".len()..];
    std::fs::write(
        fixture
            .root
            .path()
            .join("artifacts")
            .join("sha256")
            .join(&hex[..2])
            .join(hex),
        b"tampered raw",
    )
    .unwrap();
    let projection_peak = generated
        .accounting
        .journal_bytes
        .checked_add(generated.accounting.index_bytes)
        .and_then(|value| value.checked_add(generated.accounting.reserved_report_bytes))
        .unwrap();
    // A working-set refusal must win over the deliberately corrupted raw CAS
    // object. This proves report-shape preflight occurs before raw allocation
    // or read, rather than merely accounting after construction.
    let bounded = generate_v2_with_limits(
        &fixture.root,
        fixture.identity.clone(),
        &request(&fixture, "report:source-bound-cas-tamper"),
        ReportLimits {
            working_bytes: projection_peak - 1,
            ..ReportLimits::default()
        },
    );
    assert!(
        matches!(
            bounded,
            Err(ReportError::Incomplete {
                operation: "projection_peak",
                ..
            })
        ),
        "unexpected preflight result: {bounded:?}"
    );
    assert!(matches!(
        generate_v2(
            &fixture.root,
            fixture.identity.clone(),
            &request(&fixture, "report:source-bound-cas-tamper"),
        ),
        Err(ReportError::Store(_))
    ));
}

#[test]
fn request_metadata_obstruction_order_and_source_resolution_fail_closed() {
    let fixture = support::source_fixture();
    let journal = fixture.journal();
    fixture.rebuild_index(&journal);

    let mut invalid = request(&fixture, "report:invalid-tool-version");
    invalid
        .tool_versions
        .insert("reviewgraphen.fake-reviewer".to_owned(), String::new());
    assert!(matches!(
        generate_v2(&fixture.root, fixture.identity.clone(), &invalid),
        Err(ReportError::Source(
            "selected obligations and nonempty tool versions are required"
        ))
    ));

    let mut invalid = request(&fixture, "report:invalid-program-space-ref");
    invalid.program_space_ref = StableId::parse("program-space:forged").unwrap();
    assert!(matches!(
        generate_v2(&fixture.root, fixture.identity.clone(), &invalid),
        Err(ReportError::Source("repository/program-space closure"))
    ));

    let selected = fixture.selection.obligation_id.clone();
    let valid = |message: &str| PreReviewObstruction {
        message: message.to_owned(),
        source_ids: BTreeSet::from([selected.clone()]),
        blocks: BTreeSet::from([selected.clone()]),
    };
    let mut invalid = request(&fixture, "report:invalid-empty-obstruction-message");
    invalid.pre_review_obstructions = vec![valid("")];
    assert!(matches!(
        generate_v2(&fixture.root, fixture.identity.clone(), &invalid),
        Err(ReportError::Source(
            "pre-review obstruction fields must be nonempty"
        ))
    ));

    let mut invalid = request(&fixture, "report:invalid-obstruction-order");
    invalid.pre_review_obstructions = vec![valid("z-last"), valid("a-first")];
    assert!(matches!(
        generate_v2(&fixture.root, fixture.identity.clone(), &invalid),
        Err(ReportError::Source(
            "pre-review obstructions must be canonical and duplicate-free"
        ))
    ));
    let duplicate = valid("duplicate");
    invalid.pre_review_obstructions = vec![duplicate.clone(), duplicate];
    assert!(matches!(
        generate_v2(&fixture.root, fixture.identity.clone(), &invalid),
        Err(ReportError::Source(
            "pre-review obstructions must be canonical and duplicate-free"
        ))
    ));

    let mut invalid = request(&fixture, "report:forged-obstruction-source");
    invalid.pre_review_obstructions = vec![PreReviewObstruction {
        message: "forged source".to_owned(),
        source_ids: BTreeSet::from([StableId::parse("artifact:forged-source").unwrap()]),
        blocks: BTreeSet::from([selected]),
    }];
    assert!(matches!(
        generate_v2(&fixture.root, fixture.identity.clone(), &invalid),
        Err(ReportError::Source(
            "unresolved pre-review obstruction source"
        ))
    ));
}

#[test]
fn repeated_provider_failures_keep_attempts_but_deduplicate_view_kinds() {
    let fixture = support::source_fixture();
    let journal = fixture.journal();
    let admissions = fixture.admissions(&journal, &[]);
    let mut first_session = journal.replayed_v2_session(&admissions).unwrap();
    let key = FixtureKey::new(
        BTreeSet::from([fixture.selection.obligation_id.clone()]),
        first_session
            .aggregate()
            .unwrap()
            .universe()
            .snapshot_id()
            .clone(),
    )
    .unwrap();
    let first_raw = b"{\"diagnostic\":\"provider failure one\",\"kind\":\"provider_failure\",\"retryable\":true}".to_vec();
    let reviewer = FakeReviewer::new(vec![(
        key,
        FakeFixture::new(
            first_raw.clone(),
            ReviewerOutcome::ProviderFailure {
                retryable: true,
                diagnostic: "provider failure one".to_owned(),
            },
        )
        .unwrap(),
    )])
    .unwrap();
    let first = run_fresh_fake_attempt(
        &mut first_session,
        &fixture.root,
        &reviewer,
        fixture.selection.clone(),
    )
    .unwrap();
    drop(first_session);

    let admissions = fixture.admissions(&journal, &[(first.execution_id, first_raw)]);
    let mut second_session = journal.replayed_v2_session(&admissions).unwrap();
    let mut second_selection = fixture.selection.clone();
    second_selection.attempt = 2;
    let key = FixtureKey::new(
        BTreeSet::from([fixture.selection.obligation_id.clone()]),
        second_session
            .aggregate()
            .unwrap()
            .universe()
            .snapshot_id()
            .clone(),
    )
    .unwrap();
    let second_raw = b"{\"diagnostic\":\"provider failure two\",\"kind\":\"provider_failure\",\"retryable\":false}".to_vec();
    let reviewer = FakeReviewer::new(vec![(
        key,
        FakeFixture::new(
            second_raw,
            ReviewerOutcome::ProviderFailure {
                retryable: false,
                diagnostic: "provider failure two".to_owned(),
            },
        )
        .unwrap(),
    )])
    .unwrap();
    run_fresh_fake_attempt(
        &mut second_session,
        &fixture.root,
        &reviewer,
        second_selection,
    )
    .unwrap();
    drop(second_session);
    fixture.rebuild_index(&journal);
    let report: Value = serde_json::from_slice(
        &generate_v2(
            &fixture.root,
            fixture.identity.clone(),
            &request(&fixture, "report:repeated-provider-failure"),
        )
        .unwrap()
        .canonical_bytes,
    )
    .unwrap();
    assert_eq!(report["result"]["status"], "partial");
    assert_eq!(report["result"]["executions"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["result"]["obstructions"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        report["projection"]["views"][0]["payload"]["obstruction_kinds"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v2.schema.json"
    ))
    .unwrap();
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&report)
    );
}

#[test]
fn actual_source_bound_report_enforces_exact_and_plus_one_output_and_working_limits() {
    let fixture = support::source_fixture();
    let journal = fixture.journal();
    fixture.rebuild_index(&journal);
    let mut report_request = request(&fixture, "report:actual-bounds");
    report_request.pre_review_obstructions = vec![PreReviewObstruction {
        message: "actual bounds fixture".to_owned(),
        source_ids: BTreeSet::from([fixture.selection.obligation_id.clone()]),
        blocks: BTreeSet::from([fixture.selection.obligation_id.clone()]),
    }];
    let baseline = generate_v2(&fixture.root, fixture.identity.clone(), &report_request).unwrap();
    let accounting = baseline.accounting;
    let projection_peak = accounting
        .journal_bytes
        .checked_add(accounting.index_bytes)
        .and_then(|value| value.checked_add(accounting.reserved_report_bytes))
        .unwrap();
    let serialization_peak = accounting
        .journal_bytes
        .checked_add(accounting.index_bytes)
        .and_then(|value| value.checked_add(accounting.realized_report_bytes))
        .and_then(|value| value.checked_add(accounting.largest_record_bytes))
        .and_then(|value| value.checked_add(accounting.canonical_report_bytes))
        .unwrap();
    let exact_working = projection_peak.max(serialization_peak);
    let exact = ReportLimits {
        executions: 0,
        raw_registrations: 0,
        claims: 0,
        obstructions: 1,
        views: 1,
        information_loss_records: 1,
        rows: 3,
        canonical_bytes: accounting.canonical_report_bytes,
        working_bytes: exact_working,
    };
    assert!(
        generate_v2_with_limits(
            &fixture.root,
            fixture.identity.clone(),
            &report_request,
            exact,
        )
        .is_ok()
    );
    assert!(matches!(
        generate_v2_with_limits(
            &fixture.root,
            fixture.identity.clone(),
            &report_request,
            ReportLimits {
                canonical_bytes: accounting.canonical_report_bytes - 1,
                ..exact
            },
        ),
        Err(ReportError::Incomplete {
            operation: "canonical_report_bytes",
            ..
        })
    ));
    assert!(matches!(
        generate_v2_with_limits(
            &fixture.root,
            fixture.identity.clone(),
            &report_request,
            ReportLimits {
                working_bytes: exact_working - 1,
                ..exact
            },
        ),
        Err(ReportError::Incomplete { .. })
    ));
    for constrained in [
        ReportLimits {
            obstructions: 0,
            ..exact
        },
        ReportLimits { views: 0, ..exact },
        ReportLimits {
            information_loss_records: 0,
            ..exact
        },
        ReportLimits { rows: 2, ..exact },
    ] {
        assert!(matches!(
            generate_v2_with_limits(
                &fixture.root,
                fixture.identity.clone(),
                &report_request,
                constrained,
            ),
            Err(ReportError::Incomplete { .. })
        ));
    }
}
