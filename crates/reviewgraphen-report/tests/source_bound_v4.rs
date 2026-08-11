#[path = "support/v4.rs"]
mod support_v4;

use reviewgraphen_core::{ContentHash, StableId, canonical_json};
use reviewgraphen_report::{
    M5BundleIncomplete, ReportError, ReportLimitsV4, generate_v4, generate_v4_with_limits,
    validate_v4_semantics,
};
use reviewgraphen_store::test_support::materialize_m5_m4_prefix_v4;
use reviewgraphen_store::{StoreLimits, StoreRoot};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const V3_SCHEMA_URI: &str = "https://capht.tech/schemas/reviewgraphen/review-report.v3.schema.json";

fn v4_validator() -> jsonschema::Validator {
    let v3: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v3.schema.json"
    ))
    .unwrap();
    let v4: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v4.schema.json"
    ))
    .unwrap();
    jsonschema::options()
        .with_resource(
            V3_SCHEMA_URI,
            jsonschema::Resource::from_contents(v3).unwrap(),
        )
        .build(&v4)
        .unwrap()
}

#[test]
fn completed_v4_report_is_source_bound_canonical_and_deterministic() {
    let fixture = support_v4::completed_fixture();
    let first = fixture.generated("report:report-v4-source-bound");
    let second = fixture.generated("report:report-v4-source-bound");
    assert_eq!(first.canonical_bytes, second.canonical_bytes);
    let report: Value = serde_json::from_slice(&first.canonical_bytes).unwrap();
    validate_v4_semantics(&report).unwrap();
    let reparsed = canonical_json(&report).unwrap();
    let first_difference = first
        .canonical_bytes
        .iter()
        .zip(&reparsed)
        .position(|(left, right)| left != right);
    if let Some(offset) = first_difference {
        let start = offset.saturating_sub(80);
        let end = (offset + 160).min(first.canonical_bytes.len().min(reparsed.len()));
        panic!(
            "first differing byte: {offset}; generated={:?}; canonical={:?}",
            String::from_utf8_lossy(&first.canonical_bytes[start..end]),
            String::from_utf8_lossy(&reparsed[start..end])
        );
    }
    assert_eq!(first.canonical_bytes.len(), reparsed.len());

    let checked_example: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v4.example.json"
    ))
    .unwrap();
    assert_eq!(
        first.canonical_bytes,
        canonical_json(&checked_example).unwrap()
    );
    assert_eq!(
        ContentHash::sha256(&first.canonical_bytes).to_string(),
        include_str!("../../../schemas/reviewgraphen.report.v4.example.sha256").trim()
    );

    let validator = v4_validator();
    let errors = validator
        .iter_errors(&report)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");

    assert_eq!(report["schema"], "reviewgraphen.review.report.v4");
    assert_eq!(report["report_version"], 4);
    assert_eq!(
        report["metadata"]["gluing_profile_descriptor_id"],
        reviewgraphen_core::DOUBLE_SUBMIT_GLUING_DESCRIPTOR_ID
    );
    let result = &report["result"];
    assert_eq!(
        result["gluing_input_descriptors"].as_array().unwrap().len(),
        2
    );
    assert_eq!(result["context_covers"].as_array().unwrap().len(), 1);
    assert_eq!(result["sections"].as_array().unwrap().len(), 2);
    assert_eq!(result["gluing_attempts"].as_array().unwrap().len(), 1);
    assert_eq!(result["restrictions"].as_array().unwrap().len(), 2);
    assert!(result["global_candidates"].as_array().unwrap().is_empty());
    assert_eq!(result["gluing_obstructions"].as_array().unwrap().len(), 1);
    let obstruction_id = result["gluing_obstructions"][0]["obstruction"]["id"]
        .as_str()
        .unwrap();
    assert!(
        result["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["id"] != obstruction_id)
    );

    let losses = report["projection"]["views"][0]["information_loss"]
        .as_array()
        .unwrap();
    let m5 = losses
        .iter()
        .filter(|loss| loss["kind"] == "omitted_m5_gluing_records")
        .collect::<Vec<_>>();
    assert_eq!(m5.len(), 6);
    assert!(m5.iter().all(|loss| {
        loss["affected_properties"] == serde_json::json!(["payment.at_most_once"])
            && loss["meaningful"] == true
            && loss["recoverable"] == true
            && !loss["source_ids"].as_array().unwrap().is_empty()
    }));
    assert!(m5.iter().all(|loss| {
        loss["recovery_ref"]
            .as_str()
            .unwrap()
            .starts_with("reviewgraphen.review.report.v4#/result/")
    }));
    assert!(!m5.iter().any(|loss| {
        loss["recovery_ref"] == "reviewgraphen.review.report.v4#/result/global_candidates"
    }));
}

#[test]
fn compatible_profile_emits_a_source_bound_candidate_and_conditional_loss() {
    let fixture = support_v4::candidate_fixture();
    let request = fixture.request("report:report-v4-candidate");
    let generated = fixture.generated("report:report-v4-candidate");
    let report: Value = serde_json::from_slice(&generated.canonical_bytes).unwrap();
    validate_v4_semantics(&report).unwrap();
    let errors = v4_validator()
        .iter_errors(&report)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        report["result"]["global_candidates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        report["result"]["gluing_obstructions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let candidate_item = &report["result"]["global_candidates"][0];
    assert_eq!(
        candidate_item["body_hash"],
        ContentHash::sha256(&canonical_json(&candidate_item["candidate"]).unwrap()).to_string()
    );
    let recovery_refs = report["projection"]["views"][0]["information_loss"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|loss| loss["recovery_ref"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(recovery_refs.contains("reviewgraphen.review.report.v4#/result/global_candidates"));
    assert!(!recovery_refs.contains("reviewgraphen.review.report.v4#/result/gluing_obstructions"));
    assert_eq!(
        generated.accounting.reserved_report_bytes,
        generated.accounting.realized_report_bytes
    );
    assert!(
        generated.accounting.largest_record_bytes
            >= u64::try_from(canonical_json(candidate_item).unwrap().len()).unwrap()
    );
    let mut candidate_link_tamper = report.clone();
    candidate_link_tamper["result"]["global_candidates"][0]["candidate"]["required_section_ids"]
        .as_array_mut()
        .unwrap()
        .pop();
    let candidate_hash = ContentHash::sha256(
        &canonical_json(&candidate_link_tamper["result"]["global_candidates"][0]["candidate"])
            .unwrap(),
    );
    candidate_link_tamper["result"]["global_candidates"][0]["body_hash"] =
        Value::String(candidate_hash.to_string());
    assert!(validate_v4_semantics(&candidate_link_tamper).is_err());

    let mut loss_tamper = report;
    let candidate_loss = loss_tamper["projection"]["views"][0]["information_loss"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|loss| {
            loss["recovery_ref"] == "reviewgraphen.review.report.v4#/result/global_candidates"
        })
        .unwrap();
    candidate_loss["source_ids"].as_array_mut().unwrap().clear();
    assert!(validate_v4_semantics(&loss_tamper).is_err());
    assert!(matches!(
        generate_v4_with_limits(
            &fixture.root,
            fixture.identity,
            fixture.base_roots.build().unwrap(),
            fixture.assignments.build().unwrap(),
            &request,
            ReportLimitsV4 {
                global_candidates: 0,
                ..ReportLimitsV4::default()
            },
        ),
        Err(ReportError::Incomplete {
            operation: "global_candidates",
            limit: 0,
            observed: 1,
        })
    ));
}

#[test]
fn unknown_profile_emits_one_source_bound_obstruction() {
    let fixture = support_v4::unknown_fixture();
    let generated = fixture.generated("report:report-v4-unknown");
    let report: Value = serde_json::from_slice(&generated.canonical_bytes).unwrap();
    validate_v4_semantics(&report).unwrap();
    let errors = v4_validator()
        .iter_errors(&report)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");

    let result = &report["result"];
    assert!(result["global_candidates"].as_array().unwrap().is_empty());
    assert_eq!(result["gluing_obstructions"].as_array().unwrap().len(), 1);
    assert_eq!(result["gluing_attempts"][0]["attempt"]["result"], "unknown");
    assert_eq!(
        result["gluing_attempts"][0]["attempt"]["obstruction_id"],
        result["gluing_obstructions"][0]["obstruction"]["id"]
    );
    assert_eq!(
        result["gluing_obstructions"][0]["obstruction"]["kind"],
        "section_unknown"
    );
    let recovery_refs = report["projection"]["views"][0]["information_loss"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|loss| loss["recovery_ref"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(recovery_refs.contains("reviewgraphen.review.report.v4#/result/gluing_obstructions"));
    assert!(!recovery_refs.contains("reviewgraphen.review.report.v4#/result/global_candidates"));
}

#[test]
fn abbreviated_source_metadata_hash_is_refused_before_report_output() {
    let fixture = support_v4::short_metadata_hash_fixture();
    let request = fixture.request("report:report-v4-short-metadata-hash");
    let error = generate_v4(
        &fixture.root,
        fixture.identity,
        fixture.base_roots.build().unwrap(),
        fixture.assignments.build().unwrap(),
        &request,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ReportError::UnsupportedMetadataHash {
            field: "rule_set_hash",
            ..
        }
    ));
}

#[test]
fn abbreviated_extractor_hash_is_independently_refused() {
    let fixture = support_v4::short_extractor_hash_fixture();
    let request = fixture.request("report:report-v4-short-extractor-hash");
    let error = generate_v4(
        &fixture.root,
        fixture.identity,
        fixture.base_roots.build().unwrap(),
        fixture.assignments.build().unwrap(),
        &request,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ReportError::UnsupportedMetadataHash {
            field: "extractor_set_hash",
            ..
        }
    ));
}

#[test]
fn abbreviated_repository_source_root_is_refused_after_full_metadata_hashes() {
    let fixture = support_v4::short_repository_source_hash_fixture();
    let request = fixture.request("report:report-v4-short-repository-source-hash");
    let error = generate_v4(
        &fixture.root,
        fixture.identity,
        fixture.base_roots.build().unwrap(),
        fixture.assignments.build().unwrap(),
        &request,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ReportError::UnsupportedMetadataHash {
            field: "repository_source_hash",
            ..
        }
    ));
}

#[test]
fn legal_zero_one_two_input_prefixes_return_typed_incomplete_without_output() {
    for registered_inputs in 0..=2_u64 {
        let workspace = tempfile::tempdir().unwrap();
        let root = StoreRoot::open(workspace.path(), StoreLimits::default()).unwrap();
        let fixture = materialize_m5_m4_prefix_v4(&root).unwrap();
        let (journal, base_roots, assignments) = fixture.into_parts();
        let identity = journal.reader().unwrap().identity().clone();
        if registered_inputs != 0 {
            journal
                .with_m5_gluing_profile_session(
                    base_roots.build().unwrap(),
                    assignments.build().unwrap(),
                    |profile| {
                        for _ in 0..registered_inputs {
                            assert!(profile.publish_next_gluing_input()?.is_some());
                        }
                        Ok(())
                    },
                )
                .unwrap();
        }
        let request = reviewgraphen_report::ReportRequestV4 {
            report_id: StableId::parse(format!("report:incomplete-{registered_inputs}")).unwrap(),
            repository_id: base_roots.repository_id.clone(),
            program_space_ref: StableId::parse("program-space:incomplete").unwrap(),
            plan_id: StableId::parse("plan:incomplete").unwrap(),
            selected_obligation_ids: BTreeSet::from([
                StableId::parse("obligation:incomplete").unwrap()
            ]),
            tool_versions: BTreeMap::from([("reviewgraphen.report".into(), "0.1.0".into())]),
        };
        assert!(matches!(
            generate_v4(
                &root,
                identity,
                base_roots.build().unwrap(),
                assignments.build().unwrap(),
                &request,
            ),
            Err(ReportError::M5BundleIncomplete(M5BundleIncomplete {
                registered_inputs: observed,
            })) if observed == registered_inputs
        ));
    }
}

#[test]
fn v4_report_refuses_assignment_denominator_cas_and_index_tamper() {
    let fixture = support_v4::completed_fixture();
    let request = fixture.request("report:report-v4-source-tamper");
    let swapped = reviewgraphen_store::test_support::FixtureAssignmentsV4 {
        payment: fixture.assignments.ui_event,
        ui_event: fixture.assignments.payment,
    };
    assert!(
        generate_v4(
            &fixture.root,
            fixture.identity.clone(),
            fixture.base_roots.build().unwrap(),
            swapped.build().unwrap(),
            &request,
        )
        .is_err()
    );

    let mut wrong_denominator = request.clone();
    wrong_denominator.selected_obligation_ids =
        BTreeSet::from([StableId::parse("obligation:not-in-denominator").unwrap()]);
    assert!(
        generate_v4(
            &fixture.root,
            fixture.identity.clone(),
            fixture.base_roots.build().unwrap(),
            fixture.assignments.build().unwrap(),
            &wrong_denominator,
        )
        .is_err()
    );

    let report: Value =
        serde_json::from_slice(&fixture.generated("report:cas-tamper").canonical_bytes).unwrap();
    let cas_hash = report["result"]["gluing_input_descriptors"][0]["registration"]["registration"]
        ["cas_hash"]
        .as_str()
        .unwrap();
    let hex = cas_hash.strip_prefix("sha256:").unwrap();
    let object = fixture
        .root
        .path()
        .join("artifacts")
        .join("sha256")
        .join(&hex[..2])
        .join(hex);
    std::fs::write(object, b"corrupt descriptor").unwrap();
    assert!(
        generate_v4(
            &fixture.root,
            fixture.identity.clone(),
            fixture.base_roots.build().unwrap(),
            fixture.assignments.build().unwrap(),
            &request,
        )
        .is_err()
    );

    let fixture = support_v4::completed_fixture();
    std::fs::remove_file(
        fixture
            .root
            .path()
            .join("indexes")
            .join("reviewgraphen.sqlite"),
    )
    .unwrap();
    assert!(
        generate_v4(
            &fixture.root,
            fixture.identity.clone(),
            fixture.base_roots.build().unwrap(),
            fixture.assignments.build().unwrap(),
            &fixture.request("report:index-tamper"),
        )
        .is_err()
    );
}

#[test]
fn v4_limits_are_exact_and_every_m5_class_is_independently_bounded() {
    let fixture = support_v4::completed_fixture();
    let request = fixture.request("report:report-v4-limits");
    let baseline = fixture.generated("report:report-v4-limits");
    let accounting = baseline.accounting;
    let projection_peak =
        accounting.journal_bytes + accounting.index_bytes + accounting.reserved_report_bytes;
    let serialization_peak = accounting.journal_bytes
        + accounting.index_bytes
        + accounting.realized_report_bytes
        + accounting.largest_record_bytes
        + accounting.canonical_report_bytes;
    let exact = ReportLimitsV4 {
        inherited: reviewgraphen_report::ReportLimits {
            canonical_bytes: accounting.canonical_report_bytes,
            working_bytes: projection_peak.max(serialization_peak),
            ..reviewgraphen_report::ReportLimits::default()
        },
        ..ReportLimitsV4::default()
    };
    assert!(
        generate_v4_with_limits(
            &fixture.root,
            fixture.identity.clone(),
            fixture.base_roots.build().unwrap(),
            fixture.assignments.build().unwrap(),
            &request,
            exact,
        )
        .is_ok()
    );
    for constrained in [
        ReportLimitsV4 {
            artifact_registrations_v4: 1,
            ..exact
        },
        ReportLimitsV4 {
            gluing_input_descriptors: 1,
            ..exact
        },
        ReportLimitsV4 {
            context_covers: 0,
            ..exact
        },
        ReportLimitsV4 {
            sections: 1,
            ..exact
        },
        ReportLimitsV4 {
            gluing_attempts: 0,
            ..exact
        },
        ReportLimitsV4 {
            restrictions: 1,
            ..exact
        },
        ReportLimitsV4 {
            gluing_obstructions: 0,
            ..exact
        },
    ] {
        assert!(matches!(
            generate_v4_with_limits(
                &fixture.root,
                fixture.identity.clone(),
                fixture.base_roots.build().unwrap(),
                fixture.assignments.build().unwrap(),
                &request,
                constrained,
            ),
            Err(ReportError::Incomplete { .. })
        ));
    }
    assert!(matches!(
        generate_v4_with_limits(
            &fixture.root,
            fixture.identity.clone(),
            fixture.base_roots.build().unwrap(),
            fixture.assignments.build().unwrap(),
            &request,
            ReportLimitsV4 {
                inherited: reviewgraphen_report::ReportLimits {
                    canonical_bytes: accounting.canonical_report_bytes - 1,
                    ..exact.inherited
                },
                ..exact
            },
        ),
        Err(ReportError::Incomplete {
            operation: "canonical_report_bytes",
            ..
        })
    ));
    assert!(matches!(
        generate_v4_with_limits(
            &fixture.root,
            fixture.identity.clone(),
            fixture.base_roots.build().unwrap(),
            fixture.assignments.build().unwrap(),
            &request,
            ReportLimitsV4 {
                inherited: reviewgraphen_report::ReportLimits {
                    working_bytes: exact.inherited.working_bytes - 1,
                    ..exact.inherited
                },
                ..exact
            },
        ),
        Err(ReportError::Incomplete { .. })
    ));
}

#[test]
#[ignore = "maintenance helper: emits the source-bound checked v4 example"]
fn emit_checked_v4_example() {
    let fixture = support_v4::completed_fixture();
    let generated = fixture.generated("report:report-v4-source-bound");
    println!(
        "V4_EXAMPLE={}",
        String::from_utf8(generated.canonical_bytes).unwrap()
    );
    println!(
        "V4_SHA256={}",
        ContentHash::sha256(
            &fixture
                .generated("report:report-v4-source-bound")
                .canonical_bytes
        )
    );
}
