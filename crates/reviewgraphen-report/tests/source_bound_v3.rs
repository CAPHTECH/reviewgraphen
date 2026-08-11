#[path = "support/v3.rs"]
mod support_v3;

use reviewgraphen_core::{ContentHash, canonical_json};
use reviewgraphen_report::{ReportError, ReportLimits, generate_v3_with_limits};
use serde_json::Value;

#[test]
fn accepted_v3_report_is_source_bound_canonical_and_deterministic() {
    let fixture = support_v3::accepted_fixture();
    let first = fixture.generated("report:report-v3-source-bound");
    let second = fixture.generated("report:report-v3-source-bound");
    assert_eq!(first.canonical_bytes, second.canonical_bytes);
    fixture.rebuild();
    let rebuilt = fixture.generated("report:report-v3-source-bound");
    assert_eq!(first.canonical_bytes, rebuilt.canonical_bytes);

    let report: Value = serde_json::from_slice(&first.canonical_bytes).unwrap();
    assert_eq!(first.canonical_bytes, canonical_json(&report).unwrap());
    let checked = include_bytes!("../../../schemas/reviewgraphen.report.v3.example.json");
    assert_eq!(
        first.canonical_bytes,
        checked.strip_suffix(b"\n").unwrap_or(checked)
    );
    assert_eq!(
        first.accounting.canonical_report_bytes,
        u64::try_from(checked.len() - 1).unwrap()
    );
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v3.schema.json"
    ))
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors = validator
        .iter_errors(&report)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");

    let result = report["result"].as_object().unwrap();
    assert_eq!(result["status"], "completed");
    assert_eq!(
        result["artifact_registrations"].as_array().unwrap().len(),
        3
    );
    for key in [
        "executions",
        "claims",
        "evidence",
        "evidence_bindings",
        "verifications",
        "decisions",
        "findings",
        "claim_assessments",
    ] {
        assert_eq!(result[key].as_array().unwrap().len(), 1, "{key}");
    }
    assert!(result["obstructions"].as_array().unwrap().is_empty());
    let claim_id = fixture.claim_id.as_str();
    assert_eq!(result["claims"][0]["id"], claim_id);
    assert_eq!(result["evidence_bindings"][0]["claim_id"], claim_id);
    assert_eq!(result["verifications"][0]["claim_id"], claim_id);
    assert_eq!(result["decisions"][0]["claim_id"], claim_id);
    assert_eq!(result["findings"][0]["claim_id"], claim_id);
    assert_eq!(result["claim_assessments"][0]["claim_id"], claim_id);
    assert_eq!(result["evidence_bindings"][0]["relation"], "reproduces");
    assert_eq!(result["verifications"][0]["outcome"], "passed");
    assert_eq!(result["decisions"][0]["outcome"], "accept");
    assert_eq!(result["findings"][0]["status"], "accepted");

    let coverage = report["coverage"].as_object().unwrap();
    assert_eq!(coverage["universe_id"], fixture.universe_id.as_str());
    for key in [
        "selected",
        "visited",
        "completed",
        "evidence_supported",
        "verified",
        "fresh_verified",
        "accepted",
    ] {
        assert_eq!(coverage[key], 1, "{key}");
    }
    for key in [
        "visited_obligation_ids",
        "completed_obligation_ids",
        "evidence_supported_obligation_ids",
        "verified_obligation_ids",
        "fresh_verified_obligation_ids",
        "accepted_obligation_ids",
    ] {
        assert_eq!(coverage[key], serde_json::json!([fixture.obligation_id]));
    }
    assert!(
        coverage["denominator_obligation_ids"]
            .as_array()
            .unwrap()
            .contains(&Value::String(fixture.obligation_id.to_string()))
    );

    let source_kinds = result["artifact_registrations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["source"]["kind"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        source_kinds,
        std::collections::BTreeSet::from([
            "external_harness_witness",
            "reviewer_execution",
            "verifier_artifact",
        ])
    );
    let journal = fixture.journal();
    let reader = journal.reader().unwrap();
    assert_eq!(
        report["metadata"]["confirmed_offset"],
        reader.confirmed_offset()
    );
    assert_eq!(
        report["metadata"]["confirmed_event_count"],
        reader.events().len()
    );
    assert_eq!(
        report["metadata"]["confirmed_tail_hash"],
        reader.tail_hash().as_str()
    );
    assert_eq!(
        report["metadata"]["authority_policy_revision_hash"],
        fixture.roots.policy_revision_hash().as_str()
    );
    assert_ne!(
        ContentHash::parse(
            report["metadata"]["authority_replay_basis_digest"]
                .as_str()
                .unwrap()
        )
        .unwrap(),
        ContentHash::sha256(b"")
    );
}

#[test]
#[ignore = "maintenance helper: emits the source-bound checked example"]
fn emit_checked_v3_example() {
    let fixture = support_v3::accepted_fixture();
    let report = fixture.generated("report:report-v3-source-bound");
    println!(
        "V3_EXAMPLE={}",
        String::from_utf8(report.canonical_bytes).unwrap()
    );
}

#[test]
fn actual_v3_fixture_enforces_rows_rr3_o3_and_recomputes_s3() {
    let fixture = support_v3::accepted_fixture();
    let request = fixture.request("report:report-v3-bounds");
    let baseline = fixture.generated("report:report-v3-bounds");
    let accounting = baseline.accounting;
    assert_eq!(
        accounting.canonical_report_bytes,
        u64::try_from(baseline.canonical_bytes.len()).unwrap()
    );
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
    let exact = ReportLimits {
        executions: 1,
        raw_registrations: 3,
        claims: 1,
        evidence: 1,
        evidence_bindings: 1,
        verifications: 1,
        decisions: 1,
        findings: 1,
        claim_assessments: 1,
        obstructions: 0,
        views: 1,
        information_loss_records: 1,
        rows: 13,
        canonical_bytes: accounting.canonical_report_bytes,
        working_bytes: projection_peak.max(serialization_peak),
    };
    assert!(
        generate_v3_with_limits(
            &fixture.root,
            fixture.identity.clone(),
            &fixture.roots,
            &request,
            exact,
        )
        .is_ok()
    );
    assert!(serialization_peak > projection_peak);
    assert!(
        generate_v3_with_limits(
            &fixture.root,
            fixture.identity.clone(),
            &fixture.roots,
            &request,
            ReportLimits {
                working_bytes: serialization_peak,
                ..exact
            },
        )
        .is_ok()
    );
    for constrained in [
        ReportLimits {
            raw_registrations: 2,
            ..exact
        },
        ReportLimits {
            executions: 0,
            ..exact
        },
        ReportLimits { claims: 0, ..exact },
        ReportLimits {
            evidence: 0,
            ..exact
        },
        ReportLimits {
            evidence_bindings: 0,
            ..exact
        },
        ReportLimits {
            verifications: 0,
            ..exact
        },
        ReportLimits {
            decisions: 0,
            ..exact
        },
        ReportLimits {
            findings: 0,
            ..exact
        },
        ReportLimits {
            claim_assessments: 0,
            ..exact
        },
        ReportLimits { views: 0, ..exact },
        ReportLimits {
            information_loss_records: 0,
            ..exact
        },
        ReportLimits { rows: 12, ..exact },
    ] {
        assert!(matches!(
            generate_v3_with_limits(
                &fixture.root,
                fixture.identity.clone(),
                &fixture.roots,
                &request,
                constrained,
            ),
            Err(ReportError::Incomplete { .. })
        ));
    }
    assert!(matches!(
        generate_v3_with_limits(
            &fixture.root,
            fixture.identity.clone(),
            &fixture.roots,
            &request,
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
        generate_v3_with_limits(
            &fixture.root,
            fixture.identity.clone(),
            &fixture.roots,
            &request,
            ReportLimits {
                working_bytes: serialization_peak - 1,
                ..exact
            },
        ),
        Err(ReportError::Incomplete {
            operation: "serialization_peak",
            ..
        })
    ));
    let at_projection_peak = generate_v3_with_limits(
        &fixture.root,
        fixture.identity.clone(),
        &fixture.roots,
        &request,
        ReportLimits {
            working_bytes: projection_peak,
            ..exact
        },
    );
    assert!(!matches!(
        at_projection_peak,
        Err(ReportError::Incomplete {
            operation: "projection_peak",
            ..
        })
    ));
    assert!(matches!(
        generate_v3_with_limits(
            &fixture.root,
            fixture.identity.clone(),
            &fixture.roots,
            &request,
            ReportLimits {
                working_bytes: projection_peak - 1,
                ..exact
            },
        ),
        Err(ReportError::Incomplete {
            operation: "projection_peak",
            ..
        })
    ));

    let report: Value = serde_json::from_slice(&baseline.canonical_bytes).unwrap();
    let mut records = Vec::new();
    for key in [
        "artifact_registrations",
        "executions",
        "claims",
        "evidence",
        "evidence_bindings",
        "verifications",
        "decisions",
        "findings",
        "claim_assessments",
        "obstructions",
    ] {
        records.extend(report["result"][key].as_array().unwrap().iter());
    }
    records.extend(report["projection"]["views"].as_array().unwrap().iter());
    let recomputed_s3 = records
        .into_iter()
        .map(|record| u64::try_from(canonical_json(record).unwrap().len()).unwrap())
        .max()
        .unwrap();
    assert_eq!(accounting.largest_record_bytes, recomputed_s3);
    assert_eq!(
        accounting.reserved_report_bytes,
        accounting.realized_report_bytes
    );
}

#[test]
fn static_only_verification_never_becomes_verified_or_accepted() {
    let fixture = support_v3::static_only_fixture();
    let report: Value = serde_json::from_slice(
        &fixture
            .generated("report:report-v3-static-only")
            .canonical_bytes,
    )
    .unwrap();
    let verification = &report["result"]["verifications"][0];
    assert!(matches!(
        verification["outcome"].as_str(),
        Some("inconclusive" | "unsupported")
    ));
    assert_eq!(report["coverage"]["evidence_supported"], 0);
    assert_eq!(report["coverage"]["verified"], 0);
    assert_eq!(report["coverage"]["fresh_verified"], 0);
    assert_eq!(report["coverage"]["accepted"], 0);
    assert_eq!(
        report["coverage"]["verified_obligation_ids"],
        serde_json::json!([])
    );
    assert_eq!(
        report["coverage"]["accepted_obligation_ids"],
        serde_json::json!([])
    );
    assert!(report["result"]["decisions"].as_array().unwrap().is_empty());
    assert!(report["result"]["findings"].as_array().unwrap().is_empty());
}

#[test]
fn v3_report_refuses_wrong_repository_stale_tail_and_selected_cas_tamper() {
    let fixture = support_v3::accepted_fixture();
    let mut wrong = fixture.request("report:report-v3-wrong-repository");
    wrong.repository_id = reviewgraphen_core::StableId::parse("repository:wrong").unwrap();
    assert!(matches!(
        reviewgraphen_report::generate_v3(
            &fixture.root,
            fixture.identity.clone(),
            &fixture.roots,
            &wrong,
        ),
        Err(ReportError::Source("v3 authority repository closure"))
    ));

    fixture.append_gap();
    assert!(
        reviewgraphen_report::generate_v3(
            &fixture.root,
            fixture.identity.clone(),
            &fixture.roots,
            &fixture.request("report:report-v3-stale"),
        )
        .is_err()
    );

    fixture.rebuild();
    let generated = fixture.generated("report:report-v3-cas-tamper");
    let report: Value = serde_json::from_slice(&generated.canonical_bytes).unwrap();
    let hash = report["result"]["artifact_registrations"][0]["cas_hash"]
        .as_str()
        .unwrap();
    let hex = hash.strip_prefix("sha256:").unwrap();
    std::fs::write(
        fixture
            .root
            .path()
            .join("artifacts")
            .join("sha256")
            .join(&hex[..2])
            .join(hex),
        b"tampered selected v3 CAS",
    )
    .unwrap();
    assert!(
        reviewgraphen_report::generate_v3(
            &fixture.root,
            fixture.identity.clone(),
            &fixture.roots,
            &fixture.request("report:report-v3-cas-tamper"),
        )
        .is_err()
    );
}

#[test]
fn later_static_trace_creates_decision_conflict_without_laundering_acceptance() {
    let fixture = support_v3::accepted_fixture();
    fixture.append_static_conflict();
    fixture.rebuild();
    let report: Value = serde_json::from_slice(
        &fixture
            .generated("report:report-v3-later-conflict")
            .canonical_bytes,
    )
    .unwrap();
    assert_eq!(report["coverage"]["evidence_supported"], 1);
    assert_eq!(report["coverage"]["verified"], 1);
    assert_eq!(report["coverage"]["fresh_verified"], 1);
    assert_eq!(report["coverage"]["accepted"], 0);
    assert_eq!(
        report["coverage"]["accepted_obligation_ids"],
        serde_json::json!([])
    );
    assert_eq!(report["result"]["decisions"].as_array().unwrap().len(), 1);
    assert_eq!(report["result"]["findings"].as_array().unwrap().len(), 1);
    let assessment = &report["result"]["claim_assessments"][0];
    assert_eq!(assessment["decision_conflict"], true);
    assert!(assessment["active_decision_id"].is_null());
    assert!(assessment["current_finding_id"].is_null());
    assert_eq!(report["result"]["evidence"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["result"]["evidence_bindings"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        report["result"]["verifications"].as_array().unwrap().len(),
        2
    );
}
