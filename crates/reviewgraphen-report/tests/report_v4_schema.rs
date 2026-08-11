//! Contract-only tests for the closed Report V4 wire shape.
//!
//! This is intentionally not a checked-in report example: ADR 0022 requires
//! the canonical V4 fixture/hash to be emitted by the public V4 pipeline.

use jsonschema::Resource;
use reviewgraphen_core::{ContentHash, canonical_json};
use reviewgraphen_report::validate_v4_semantics;
use serde_json::{Value, json};

const V3_URI: &str = "https://capht.tech/schemas/reviewgraphen/review-report.v3.schema.json";
const HASH: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

fn validator() -> jsonschema::Validator {
    let v3: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v3.schema.json"
    ))
    .unwrap();
    let v4: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v4.schema.json"
    ))
    .unwrap();
    jsonschema::options()
        .with_resource(V3_URI, Resource::from_contents(v3).unwrap())
        .build(&v4)
        .expect("v4 schema must be valid Draft 2020-12")
}

fn descriptor(context_id: &str, value: &str, id: &str) -> Value {
    json!({
        "schema": "reviewgraphen.gluing_input_descriptor.v4", "id": id,
        "run_id": "run:v4", "snapshot_id": "snapshot:v4", "universe_id": "universe:v4", "plan_id": "plan:v4",
        "profile_descriptor_id": "reviewgraphen.double_submit_gluing@1", "context_id": context_id,
        "assignment_key": "caller_duplicate_protection", "assignment_value": value,
        "qualification_source_ids": []
    })
}

fn registration(context_id: &str, descriptor_id: &str, registration_id: &str) -> Value {
    json!({
        "schema": "reviewgraphen.artifact_registration.v4.report_item", "event_sequence": 21,
        "event_id": "event:m5-registration", "event_actor": "engine:reviewgraphen.m5_gluing_input@1",
        "registration": {
            "schema": "reviewgraphen.artifact_registration.v4", "id": registration_id, "run_id": "run:v4",
            "cas_hash": HASH, "media_type": "application/vnd.reviewgraphen.gluing-input.v4+json", "size": 1,
            "sensitivity": "canonical_state",
            "source": {
                "kind": "gluing_input", "descriptor_id": descriptor_id,
                "profile_descriptor_id": "reviewgraphen.double_submit_gluing@1", "policy_revision_hash": HASH,
                "repository_id": "repository:v4", "repository_source_hash": HASH, "run_id": "run:v4",
                "genesis_hash": HASH, "snapshot_id": "snapshot:v4", "universe_id": "universe:v4", "plan_id": "plan:v4",
                "context_id": context_id, "descriptor_hash": HASH, "descriptor_size": 1,
                "descriptor_media_type": "application/vnd.reviewgraphen.gluing-input.v4+json", "descriptor_sensitivity": "canonical_state"
            }
        },
        "body_hash": HASH
    })
}

fn section(context_id: &str, id: &str, descriptor_id: &str, registration_id: &str) -> Value {
    json!({
        "schema": "reviewgraphen.section.v4", "id": id, "cover_id": "cover:v4", "context_id": context_id,
        "snapshot_id": "snapshot:v4", "property_id": "payment.at_most_once", "invariant_id": "invariant:payment-at-most-once",
        "obligation_id": format!("obligation:{context_id}"), "claim_id": format!("claim:{context_id}"), "claim_assessment_id": format!("assessment:{context_id}"),
        "input_descriptor_id": descriptor_id, "input_registration_id": registration_id,
        "assignment_key": "caller_duplicate_protection", "assignment_value": "satisfied", "passed_current_verification": true,
        "source_ids": [], "qualification_source_ids": [], "binding_ids": [], "evidence_ids": [], "verification_ids": [], "decision_ids": [], "finding_ids": []
    })
}

fn restriction(section_id: &str, id: &str) -> Value {
    json!({
        "schema": "reviewgraphen.restriction.v4", "id": id, "section_id": section_id,
        "context_pair": ["context:payment", "context:ui-event"], "overlap_member_ids": [],
        "assignment_key": "caller_duplicate_protection", "assignment_value": "satisfied",
        "source_ids": [], "qualification_source_ids": [], "claim_ids": [], "evidence_ids": [], "verification_ids": [], "decision_ids": [], "finding_ids": []
    })
}

fn v4_report() -> Value {
    let mut report: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v3.example.json"
    ))
    .unwrap();
    report["schema"] = json!("reviewgraphen.review.report.v4");
    report["report_version"] = json!(4);
    report["metadata"]["event_contract_version"] = json!("reviewgraphen.review_event.v4");
    report["metadata"]["index_projection_version"] = json!("reviewgraphen.index_projection.v5");
    report["metadata"]["gluing_profile_descriptor_id"] =
        json!("reviewgraphen.double_submit_gluing@1");
    report["scenario"]["context_cover_ids"] = json!(["cover:v4"]);

    let payment_descriptor = descriptor("context:payment", "satisfied", "descriptor:payment");
    let ui_descriptor = descriptor("context:ui-event", "satisfied", "descriptor:ui");
    let payment_registration = registration(
        "context:payment",
        "descriptor:payment",
        "registration:payment",
    );
    let ui_registration = registration("context:ui-event", "descriptor:ui", "registration:ui");
    let payment_section = section(
        "context:payment",
        "section:payment",
        "descriptor:payment",
        "registration:payment",
    );
    let ui_section = section(
        "context:ui-event",
        "section:ui",
        "descriptor:ui",
        "registration:ui",
    );
    let payment_restriction = restriction("section:payment", "restriction:payment");
    let ui_restriction = restriction("section:ui", "restriction:ui");

    report["result"]["gluing_input_descriptors"] = json!([
        {"schema": "reviewgraphen.gluing_input_descriptor.v4.report_item", "descriptor": payment_descriptor, "descriptor_body_hash": HASH, "registration_id": "registration:payment", "registration": payment_registration},
        {"schema": "reviewgraphen.gluing_input_descriptor.v4.report_item", "descriptor": ui_descriptor, "descriptor_body_hash": HASH, "registration_id": "registration:ui", "registration": ui_registration}
    ]);
    report["result"]["context_covers"] = json!([{
        "event_sequence": 23, "event_id": "event:m5-bundle", "body_hash": HASH,
        "cover": {"schema": "reviewgraphen.context_cover.v4", "id": "cover:v4", "run_id": "run:v4", "snapshot_id": "snapshot:v4", "universe_id": "universe:v4", "plan_id": "plan:v4", "profile_descriptor_id": "reviewgraphen.double_submit_gluing@1", "selected_obligation_ids": ["obligation:payment"], "required_context_ids": ["context:payment", "context:ui-event"], "cover_domain_ids": [], "covered_domain_ids": [], "uncovered_domain_ids": [], "source_ids": []}
    }]);
    report["result"]["sections"] = json!([
        {"event_sequence": 23, "event_id": "event:m5-bundle", "section": payment_section, "body_hash": HASH},
        {"event_sequence": 23, "event_id": "event:m5-bundle", "section": ui_section, "body_hash": HASH}
    ]);
    report["result"]["restrictions"] = json!([
        {"event_sequence": 23, "event_id": "event:m5-bundle", "restriction": payment_restriction, "body_hash": HASH},
        {"event_sequence": 23, "event_id": "event:m5-bundle", "restriction": ui_restriction, "body_hash": HASH}
    ]);
    report["result"]["gluing_attempts"] = json!([{
        "event_sequence": 23, "event_id": "event:m5-bundle", "body_hash": HASH,
        "attempt": {"schema": "reviewgraphen.gluing_attempt.v4", "id": "attempt:v4", "cover_id": "cover:v4", "snapshot_id": "snapshot:v4", "property_id": "payment.at_most_once", "invariant_id": "invariant:payment-at-most-once", "input_descriptor_ids": ["descriptor:payment", "descriptor:ui"], "section_ids": ["section:payment", "section:ui"], "restriction_ids": ["restriction:payment", "restriction:ui"], "result": "glued", "global_candidate_id": "candidate:v4", "obstruction_id": null, "source_ids": [], "claim_ids": [], "evidence_ids": [], "verification_ids": [], "decision_ids": [], "finding_ids": []}
    }]);
    report["result"]["global_candidates"] = json!([{
        "event_sequence": 23, "event_id": "event:m5-bundle", "body_hash": HASH,
        "candidate": {"schema": "reviewgraphen.global_candidate.v4", "id": "candidate:v4", "cover_id": "cover:v4", "invariant_id": "invariant:payment-at-most-once", "property_id": "payment.at_most_once", "required_section_ids": ["section:payment", "section:ui"], "restriction_ids": ["restriction:payment", "restriction:ui"], "qualification_source_ids": [], "source_ids": [], "claim_ids": [], "evidence_ids": [], "verification_ids": [], "decision_ids": [], "finding_ids": []}
    }]);
    report["result"]["gluing_obstructions"] = json!([]);
    report
}

fn assert_invalid(value: &Value) {
    let errors = validator()
        .iter_errors(value)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(!errors.is_empty(), "mutation unexpectedly accepted");
}

fn rehash_descriptor_registration(report: &mut Value, position: usize) {
    let bytes = canonical_json(
        &report["result"]["gluing_input_descriptors"][position]["registration"]["registration"],
    )
    .unwrap();
    report["result"]["gluing_input_descriptors"][position]["registration"]["body_hash"] =
        Value::String(ContentHash::sha256(&bytes).to_string());
}

#[test]
fn v4_schema_accepts_the_closed_nested_m5_shape() {
    let report = v4_report();
    let errors = validator()
        .iter_errors(&report)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn v4_schema_rejects_m5_closure_and_order_mutations() {
    let mut report = v4_report();
    report["result"]["gluing_input_descriptors"][0]["registration"]["event_actor"] =
        json!("engine:other@1");
    assert_invalid(&report);

    let mut report = v4_report();
    report["result"]["gluing_input_descriptors"][0]["registration"]["registration"]["unexpected"] =
        json!(true);
    assert_invalid(&report);

    let mut report = v4_report();
    let descriptors = report["result"]["gluing_input_descriptors"]
        .as_array_mut()
        .unwrap();
    descriptors.swap(0, 1);
    assert_invalid(&report);

    let mut report = v4_report();
    let sections = report["result"]["sections"].as_array_mut().unwrap();
    sections.swap(0, 1);
    assert_invalid(&report);

    let mut report = v4_report();
    let item = report["result"]["sections"][0].as_object_mut().unwrap();
    let section = item.remove("section").unwrap();
    item.insert("id".to_owned(), section["id"].clone());
    assert_invalid(&report);
}

#[test]
fn checked_example_rejects_loss_linkage_result_and_semantic_order_mutations() {
    let example: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v4.example.json"
    ))
    .unwrap();
    assert!(validator().is_valid(&example));
    validate_v4_semantics(&example).unwrap();

    let m5_loss_position = example["projection"]["views"][0]["information_loss"]
        .as_array()
        .unwrap()
        .iter()
        .position(|loss| loss["kind"] == "omitted_m5_gluing_records")
        .unwrap();
    let mut wrong_reason = example.clone();
    wrong_reason["projection"]["views"][0]["information_loss"][m5_loss_position]["reason"] =
        json!("wrong reason");
    assert_invalid(&wrong_reason);

    let mut wrong_ref = example.clone();
    wrong_ref["projection"]["views"][0]["information_loss"][m5_loss_position]["recovery_ref"] =
        json!("reviewgraphen.review.report.v4#/result/sections");
    assert_invalid(&wrong_ref);

    let mut reversed_restrictions = example.clone();
    reversed_restrictions["result"]["restrictions"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    assert!(validator().is_valid(&reversed_restrictions));
    assert!(validate_v4_semantics(&reversed_restrictions).is_err());

    let mut reversed_descriptors = example.clone();
    reversed_descriptors["result"]["gluing_input_descriptors"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    assert!(validate_v4_semantics(&reversed_descriptors).is_err());

    let mut reversed_sections = example.clone();
    reversed_sections["result"]["sections"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    assert!(validate_v4_semantics(&reversed_sections).is_err());

    let mut failed_with_candidate = example.clone();
    failed_with_candidate["result"]["global_candidates"] =
        v4_report()["result"]["global_candidates"].clone();
    assert_invalid(&failed_with_candidate);

    let mut wrong_attempt_link = example;
    wrong_attempt_link["result"]["gluing_attempts"][0]["attempt"]["obstruction_id"] =
        json!("obstruction:wrong");
    assert!(validator().is_valid(&wrong_attempt_link));
    assert!(validate_v4_semantics(&wrong_attempt_link).is_err());
}

#[test]
fn semantic_validation_checks_every_view_and_complete_descriptor_source_tuple() {
    let example: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v4.example.json"
    ))
    .unwrap();

    let mut two_views = example.clone();
    let mut second = two_views["projection"]["views"][0].clone();
    second["kind"] = json!("human");
    two_views["projection"]["views"]
        .as_array_mut()
        .unwrap()
        .push(second);
    assert!(validator().is_valid(&two_views));
    validate_v4_semantics(&two_views).unwrap();

    let mut missing_second_view_loss = two_views.clone();
    let losses = missing_second_view_loss["projection"]["views"][1]["information_loss"]
        .as_array_mut()
        .unwrap();
    let position = losses
        .iter()
        .position(|loss| loss["kind"] == "omitted_m5_gluing_records")
        .unwrap();
    losses.remove(position);
    assert!(validator().is_valid(&missing_second_view_loss));
    assert!(validate_v4_semantics(&missing_second_view_loss).is_err());

    let mut wrong_third_view_sources = two_views;
    let mut third = wrong_third_view_sources["projection"]["views"][0].clone();
    third["kind"] = json!("ci");
    let m5_loss = third["information_loss"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|loss| loss["kind"] == "omitted_m5_gluing_records")
        .unwrap();
    m5_loss["source_ids"] = json!(["source:wrong"]);
    wrong_third_view_sources["projection"]["views"]
        .as_array_mut()
        .unwrap()
        .push(third);
    assert!(validator().is_valid(&wrong_third_view_sources));
    assert!(validate_v4_semantics(&wrong_third_view_sources).is_err());

    for (field, value) in [
        ("descriptor_hash", json!(HASH)),
        ("descriptor_size", json!(0)),
        ("run_id", json!("run:wrong")),
        ("snapshot_id", json!("snapshot:wrong")),
        ("universe_id", json!("universe:wrong")),
        ("plan_id", json!("plan:wrong")),
        ("repository_id", json!("repository:wrong")),
        ("genesis_hash", json!(HASH)),
        ("policy_revision_hash", json!(HASH)),
    ] {
        let mut mutation = example.clone();
        mutation["result"]["gluing_input_descriptors"][0]["registration"]["registration"]["source"]
            [field] = value;
        rehash_descriptor_registration(&mut mutation, 0);
        assert!(validator().is_valid(&mutation), "schema rejected {field}");
        assert!(
            validate_v4_semantics(&mutation).is_err(),
            "semantic validator accepted {field}"
        );
    }

    let mut registration_run = example;
    registration_run["result"]["gluing_input_descriptors"][0]["registration"]["registration"]["run_id"] =
        json!("run:wrong");
    rehash_descriptor_registration(&mut registration_run, 0);
    assert!(validator().is_valid(&registration_run));
    assert!(validate_v4_semantics(&registration_run).is_err());

    let mut repository_source_split: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v4.example.json"
    ))
    .unwrap();
    repository_source_split["result"]["gluing_input_descriptors"][1]["registration"]["registration"]
        ["source"]["repository_source_hash"] = json!(format!("sha256:{}", "a".repeat(64)));
    rehash_descriptor_registration(&mut repository_source_split, 1);
    assert!(validator().is_valid(&repository_source_split));
    assert!(validate_v4_semantics(&repository_source_split).is_err());
}

#[test]
fn v4_schema_does_not_upcast_frozen_report_contracts() {
    let v3: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.report.v3.example.json"
    ))
    .unwrap();
    assert_invalid(&v3);
    let v4 = v4_report();
    for (path, expected) in [
        (
            "../../../schemas/reviewgraphen.report.schema.json",
            "reviewgraphen.review.report.v1",
        ),
        (
            "../../../schemas/reviewgraphen.report.v2.schema.json",
            "reviewgraphen.review.report.v2",
        ),
        (
            "../../../schemas/reviewgraphen.report.v3.schema.json",
            "reviewgraphen.review.report.v3",
        ),
    ] {
        let schema: Value = match path {
            "../../../schemas/reviewgraphen.report.schema.json" => serde_json::from_str(
                include_str!("../../../schemas/reviewgraphen.report.schema.json"),
            )
            .unwrap(),
            "../../../schemas/reviewgraphen.report.v2.schema.json" => serde_json::from_str(
                include_str!("../../../schemas/reviewgraphen.report.v2.schema.json"),
            )
            .unwrap(),
            _ => serde_json::from_str(include_str!(
                "../../../schemas/reviewgraphen.report.v3.schema.json"
            ))
            .unwrap(),
        };
        assert_eq!(schema["properties"]["schema"]["const"], expected, "{path}");
        assert!(
            !jsonschema::validator_for(&schema).unwrap().is_valid(&v4),
            "{path} must not accept a v4 report"
        );
    }
}
