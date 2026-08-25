use jsonschema::Validator;
use reviewgraphen_core::{ContentHash, StableId, canonical_json};
use reviewgraphen_report::{
    GenericHumanReportError, generate_generic_human_report, generate_generic_human_report_v3,
    validate_generic_human_report, validate_generic_human_report_v3,
};
use reviewgraphen_runtime::generic::{
    GENERIC_REVIEW_REQUEST_V3_SCHEMA, GenericIngestRequestV2, GenericObserverRequestV2,
    GenericPlanRequest, GenericReviewError, GenericReviewRequestV3, run_generic_review_v3,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

fn human_validator() -> Validator {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_human_report.v1.schema.json"
    ))
    .unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn human_v3_validator() -> Validator {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_human_report.v2.schema.json"
    ))
    .unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn audit() -> Value {
    serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_run.v2.example.json"
    ))
    .unwrap()
}

fn bytes(value: &Value) -> Vec<u8> {
    canonical_json(value).unwrap()
}

fn id(number: usize, kind: &str) -> String {
    format!("{kind}:sha256:{number:064x}")
}

fn with_source_occurrence_summary() -> Value {
    let mut value = audit();
    let snapshot_id = value["legacy_ingestion"]["snapshot_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let file_source_id = id(60, "file");
    let bucket = json!({
        "call_kind": "direct",
        "reason": "direct_target_count_zero",
        "observed_occurrence_count": 2,
        "occurrence_id_set_sha256": ContentHash::sha256(b"summary bucket occurrence IDs").to_string()
    });
    let occurrence_id_set_sha256 = ContentHash::sha256(b"summary occurrence IDs").to_string();
    let summary_id = StableId::derived(
        "ingestion-obstruction-summary",
        &BTreeMap::from([
            (
                "schema".to_owned(),
                Value::String("reviewgraphen.ingestion_obstruction_summary.v1".to_owned()),
            ),
            (
                "kind".to_owned(),
                Value::String("call_enumeration_summary".to_owned()),
            ),
            ("severity".to_owned(), Value::String("medium".to_owned())),
            ("snapshot_id".to_owned(), Value::String(snapshot_id.clone())),
            (
                "projection_extractor_id".to_owned(),
                Value::String("reviewgraphen.ingest.rust-call-enumeration@2".to_owned()),
            ),
            (
                "file_source_id".to_owned(),
                Value::String(file_source_id.clone()),
            ),
            (
                "path".to_owned(),
                Value::String("src/example.rs".to_owned()),
            ),
            ("related_capabilities".to_owned(), json!(["direct_calls"])),
            ("observed_occurrence_count".to_owned(), json!(2)),
            (
                "occurrence_id_set_sha256".to_owned(),
                Value::String(occurrence_id_set_sha256.clone()),
            ),
            ("buckets".to_owned(), json!([bucket.clone()])),
            (
                "detail_retention".to_owned(),
                Value::String("spans_and_owner_sources_omitted_rebuildable".to_owned()),
            ),
        ]),
    )
    .unwrap()
    .to_string();
    let summary = json!({
        "schema": "reviewgraphen.ingestion_obstruction_summary.v1",
        "id": summary_id,
        "kind": "call_enumeration_summary",
        "severity": "medium",
        "snapshot_id": snapshot_id,
        "projection_extractor_id": "reviewgraphen.ingest.rust-call-enumeration@2",
        "file_source_id": file_source_id,
        "path": "src/example.rs",
        "related_capabilities": ["direct_calls"],
        "observed_occurrence_count": 2,
        "occurrence_id_set_sha256": occurrence_id_set_sha256,
        "buckets": [bucket],
        "detail_retention": "spans_and_owner_sources_omitted_rebuildable"
    });
    value["ingestion_report_v2"]["source_occurrence_summaries"] = json!([summary.clone()]);
    value["ingestion_report_v2"]["observed_occurrence_count"] = json!(2);
    value["ingestion_report_v2"]["occurrence_id_set_sha256"] =
        summary["occurrence_id_set_sha256"].clone();
    let report_id = StableId::derived(
        "generic-ingestion-report",
        &BTreeMap::from([
            (
                "schema".to_owned(),
                Value::String("reviewgraphen.generic_ingestion_projection.v2".to_owned()),
            ),
            (
                "repository_identity".to_owned(),
                value["legacy_ingestion"]["repository_identity"].clone(),
            ),
            (
                "snapshot_id".to_owned(),
                value["legacy_ingestion"]["snapshot_id"].clone(),
            ),
            (
                "base_commit_oid".to_owned(),
                value["legacy_ingestion"]["base_commit_oid"].clone(),
            ),
            (
                "base_tree_hash".to_owned(),
                value["legacy_ingestion"]["base_tree_hash"].clone(),
            ),
            (
                "target_commit_oid".to_owned(),
                value["legacy_ingestion"]["target_commit_oid"].clone(),
            ),
            (
                "target_tree_hash".to_owned(),
                value["legacy_ingestion"]["target_tree_hash"].clone(),
            ),
            (
                "projection_extractor_id".to_owned(),
                Value::String("reviewgraphen.ingest.rust-call-enumeration@2".to_owned()),
            ),
            (
                "source_occurrence_summary_ids".to_owned(),
                json!([summary["id"].clone()]),
            ),
            ("observed_occurrence_count".to_owned(), json!(2)),
            (
                "occurrence_id_set_sha256".to_owned(),
                summary["occurrence_id_set_sha256"].clone(),
            ),
            (
                "global_direct_calls_limitation_id".to_owned(),
                value["ingestion_report_v2"]["global_direct_calls_limitation"]["id"].clone(),
            ),
        ]),
    )
    .unwrap();
    value["ingestion_report_v2"]["report_id"] = json!(report_id.to_string());
    value["coverage"]["enumeration_obstruction_summary_ids"] = json!([summary["id"].clone()]);
    value["coverage"]["enumeration_obstruction_ids"] = json!([
        value["ingestion_report_v2"]["global_direct_calls_limitation"]["id"].clone(),
        summary["id"].clone()
    ]);
    value["coverage"]["observed_unresolved_call_occurrence_count"] = json!(2);
    value["coverage"]["occurrence_id_set_sha256"] = summary["occurrence_id_set_sha256"].clone();
    value
}

fn observation(kind: &str, number: usize, unicode: bool) -> Value {
    let obligation_id = id(number + 100, "obligation");
    let execution_id = id(number + 200, "execution");
    let raw_response = format!("raw-{kind}-{number}");
    let mut row = json!({
        "kind": kind,
        "schema": "reviewgraphen.generic_observation.v2",
        "observer_id": "reviewer.example@1",
        "obligation_id": obligation_id,
        "execution_id": execution_id,
        "input_manifest_hash": ContentHash::sha256(b"input").to_string(),
        "raw_response": raw_response,
        "raw_response_hash": ContentHash::sha256(raw_response.as_bytes()).to_string()
    });
    match kind {
        "proposed_claim" => {
            row["proposals"] = json!([{
                "property_id": "rust.callee_contract_review@1",
                "target_refs": [id(number + 300, "relation")],
                "polarity": "issue_present",
                "summary": if unicode { "提案\u{0007}\nline" } else { "proposal" },
                "source_ids": [id(number + 400, "file")],
                "assumptions": [],
                "requested_evidence": [],
                "candidate_confidence": null,
                "disposition": "proposed",
                "author_kind": "ai",
                "review_status": "unreviewed"
            }]);
        }
        "deterministic_abstain" => {
            row["observer_id"] = json!("deterministic.abstain@1");
            row["reason"] = json!("required_evidence_unavailable");
            row["detail"] = json!("deterministic.abstain@1 does not evaluate semantic properties");
            let task_id = provider_free_task_id(row["execution_id"].as_str().unwrap());
            let source_inventory_id = id(number + 500, "source-inventory");
            let raw_response = String::from_utf8(
                canonical_json(&json!({
                    "schema": "provider-free.source-grounded-abstention@1",
                    "task_id": task_id,
                    "source_inventory_id": source_inventory_id,
                    "disposition": {
                        "kind": "abstention",
                        "reason": "required_evidence_unavailable",
                        "detail": "deterministic.abstain@1 does not evaluate semantic properties"
                    }
                }))
                .unwrap(),
            )
            .unwrap();
            let raw_response_hash = ContentHash::sha256(raw_response.as_bytes()).to_string();
            let observation_id = StableId::derived(
                "observation",
                &BTreeMap::from([
                    ("execution_id".to_owned(), row["execution_id"].clone()),
                    (
                        "input_manifest_hash".to_owned(),
                        Value::String(ContentHash::sha256(b"packet").to_string()),
                    ),
                    (
                        "kind".to_owned(),
                        Value::String("deterministic_abstain".to_owned()),
                    ),
                    ("obligation_id".to_owned(), row["obligation_id"].clone()),
                    (
                        "raw_response_hash".to_owned(),
                        Value::String(raw_response_hash.clone()),
                    ),
                    (
                        "schema".to_owned(),
                        Value::String("reviewgraphen.generic_observation.v2".to_owned()),
                    ),
                ]),
            )
            .unwrap();
            row["id"] = json!(observation_id.to_string());
            row["input_manifest_hash"] = json!(ContentHash::sha256(b"packet").to_string());
            row["raw_response"] = json!(raw_response);
            row["raw_response_hash"] = json!(raw_response_hash);
        }
        "malformed" => {
            row["reason"] = json!("schema_violation");
            row["diagnostic"] = json!("malformed response");
        }
        "provider_failure" => {
            row["retryable"] = json!(true);
            row["diagnostic"] = json!("provider failure");
        }
        _ => unreachable!(),
    }
    row
}

fn with_outcomes(kind: &str, count: usize) -> Value {
    let mut value = audit();
    let ids = (0..count)
        .map(|number| Value::String(id(number + 100, "obligation")))
        .collect::<Vec<_>>();
    value["plan"]["waves"] = json!([{"id": id(50, "stage"), "obligation_ids": ids}]);
    let observations = (0..count)
        .map(|number| observation(kind, number, number == 0 && kind == "proposed_claim"))
        .collect::<Vec<_>>();
    value["provider_free_packet_bindings"] = if kind == "deterministic_abstain" {
        Value::Array(observations.iter().map(provider_free_binding).collect())
    } else {
        Value::Array(Vec::new())
    };
    value["observations"] = Value::Array(observations);
    for field in [
        "resolved_target_obligation_ids",
        "planned_obligation_ids",
        "executed_obligation_ids",
    ] {
        value["coverage"][field] = ids.clone().into();
    }
    for field in [
        "structured_obligation_ids",
        "abstained_obligation_ids",
        "malformed_obligation_ids",
        "provider_failed_obligation_ids",
    ] {
        value["coverage"][field] = Value::Array(Vec::new());
    }
    let field = match kind {
        "proposed_claim" => "structured_obligation_ids",
        "deterministic_abstain" => "abstained_obligation_ids",
        "malformed" => "malformed_obligation_ids",
        "provider_failure" => "provider_failed_obligation_ids",
        _ => unreachable!(),
    };
    value["coverage"][field] = Value::Array(ids);
    let reason = match kind {
        "deterministic_abstain" => "reviewer_abstained",
        "malformed" => "reviewer_output_malformed",
        "provider_failure" => "reviewer_provider_failure",
        "proposed_claim" => "model_observer_non_authority",
        _ => unreachable!(),
    };
    if !value["authority"]["incomplete_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == reason)
    {
        value["authority"]["incomplete_reasons"]
            .as_array_mut()
            .unwrap()
            .push(json!(reason));
    }
    value
}

fn provider_free_task_id(execution_id: &str) -> String {
    StableId::derived(
        "provider-free-review-task",
        &BTreeMap::from([
            (
                "execution_id".to_owned(),
                Value::String(execution_id.to_owned()),
            ),
            (
                "packet_contract".to_owned(),
                Value::String("provider-free.source-grounded-packet@1".to_owned()),
            ),
            (
                "request_contract".to_owned(),
                Value::String("reviewgraphen.generic_review_request.v2".to_owned()),
            ),
        ]),
    )
    .unwrap()
    .to_string()
}

fn provider_free_binding(observation: &Value) -> Value {
    let execution_id = observation["execution_id"].as_str().unwrap();
    let reviewer_packet_sha256 = observation["input_manifest_hash"].as_str().unwrap();
    let source_inventory_id = serde_json::from_str::<Value>(
        observation["raw_response"].as_str().unwrap(),
    )
    .unwrap()["source_inventory_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let source_inventory_sha256 = ContentHash::sha256(b"source inventory").to_string();
    let task_id = provider_free_task_id(execution_id);
    let binding_id = StableId::derived(
        "provider-free-packet-binding",
        &BTreeMap::from([
            (
                "schema".to_owned(),
                Value::String("reviewgraphen.provider_free_packet_binding.v1".to_owned()),
            ),
            ("task_id".to_owned(), Value::String(task_id.clone())),
            (
                "execution_id".to_owned(),
                Value::String(execution_id.to_owned()),
            ),
            (
                "reviewer_packet_sha256".to_owned(),
                Value::String(reviewer_packet_sha256.to_owned()),
            ),
            (
                "source_inventory_id".to_owned(),
                Value::String(source_inventory_id.clone()),
            ),
            (
                "source_inventory_sha256".to_owned(),
                Value::String(source_inventory_sha256.clone()),
            ),
            (
                "input_manifest_hash".to_owned(),
                Value::String(reviewer_packet_sha256.to_owned()),
            ),
            (
                "observer_id".to_owned(),
                Value::String("deterministic.abstain@1".to_owned()),
            ),
            (
                "observation_record_id".to_owned(),
                observation["id"].clone(),
            ),
        ]),
    )
    .unwrap();
    json!({
        "schema": "reviewgraphen.provider_free_packet_binding.v1",
        "id": binding_id,
        "task_id": task_id,
        "execution_id": execution_id,
        "reviewer_packet_sha256": reviewer_packet_sha256,
        "source_inventory_id": source_inventory_id,
        "source_inventory_sha256": source_inventory_sha256,
        "input_manifest_hash": reviewer_packet_sha256,
        "observer_id": "deterministic.abstain@1",
        "observation_record_id": observation["id"]
    })
}

fn valid_report(value: &Value) -> (Vec<u8>, reviewgraphen_report::GenericHumanReport) {
    let audit = bytes(value);
    let report = generate_generic_human_report(&audit).unwrap();
    assert!(
        human_validator()
            .is_valid(&serde_json::from_slice::<Value>(&report.manifest_bytes).unwrap())
    );
    (audit, report)
}

#[test]
fn snapshot_states_are_audit_projections() {
    let cases = [
        (audit(), "proposed_claims", 0),
        (with_outcomes("proposed_claim", 1), "proposed_claims", 1),
        (with_outcomes("deterministic_abstain", 1), "abstentions", 1),
        (with_outcomes("malformed", 1), "malformed_outputs", 1),
        (with_outcomes("provider_failure", 1), "provider_failures", 1),
    ];
    for (audit, field, expected) in cases {
        let (_, report) = valid_report(&audit);
        let manifest: Value = serde_json::from_slice(&report.manifest_bytes).unwrap();
        assert_eq!(manifest[field].as_array().unwrap().len(), expected);
        assert_eq!(manifest["authority"]["trusted_pass"], false);
    }
}

#[test]
fn unsupported_verifier_and_deferred_obligation_are_retained() {
    let mut value = audit();
    let deferred = id(777, "obligation");
    value["coverage"]["resolved_target_obligation_ids"] = json!([deferred]);
    value["coverage"]["deferred_obligation_ids"] = json!([deferred]);
    value["authority"]["incomplete_reasons"]
        .as_array_mut()
        .unwrap()
        .push(json!("obligations_deferred"));
    value["verifier"] = json!({
        "id": id(778, "verifier"),
        "descriptor_id": "workspace.cargo_test@1",
        "request_id": value["request_id"],
        "snapshot_id": value["legacy_ingestion"]["snapshot_id"],
        "universe_id": value["plan"]["universe_id"],
        "outcome": "unsupported",
        "reason": "workspace_cargo_test_deferred",
        "process_started": false,
        "executable_resolved": false,
        "verifier_observed_obligation_ids": []
    });
    let (_, report) = valid_report(&value);
    let manifest: Value = serde_json::from_slice(&report.manifest_bytes).unwrap();
    assert_eq!(manifest["verifier"]["status"], "unsupported");
    assert_eq!(manifest["deferrals"], json!([id(777, "obligation")]));
    assert!(
        String::from_utf8(report.markdown_bytes)
            .unwrap()
            .contains("No verifier observation was run.")
    );
}

#[test]
fn source_summaries_preserve_kind_reason_source_trace_and_unknown_macro_count() {
    let value = with_source_occurrence_summary();
    let (_, report) = valid_report(&value);
    let manifest: Value = serde_json::from_slice(&report.manifest_bytes).unwrap();
    let candidate_space = &manifest["coverage"]["candidate_space"];
    assert_eq!(
        candidate_space["source_occurrence_summaries"],
        value["ingestion_report_v2"]["source_occurrence_summaries"]
    );
    assert_eq!(
        candidate_space["global_direct_calls_limitation"],
        value["ingestion_report_v2"]["global_direct_calls_limitation"]
    );
    assert!(
        manifest["source_window_trace"]["source_ids"]
            .as_array()
            .unwrap()
            .contains(&json!(id(60, "file")))
    );
    let markdown = String::from_utf8(report.markdown_bytes).unwrap();
    assert!(markdown.contains("direct_target_count_zero"));
    assert!(markdown.contains("Macro latent occurrence count: `unknown`"));
}

#[test]
fn run_v2_version_boundary_is_strict_and_typed() {
    let mut wrong_major = audit();
    wrong_major["schema"] = json!("reviewgraphen.generic_review_run.v1");
    assert!(matches!(
        generate_generic_human_report(&bytes(&wrong_major)),
        Err(GenericHumanReportError::Audit(GenericReviewError::Request(
            "v2 run schema"
        )))
    ));

    let mut foreign_shape = audit();
    foreign_shape["ingestion_report_v2"]["located_call_occurrences"] = json!([]);
    assert!(matches!(
        generate_generic_human_report(&bytes(&foreign_shape)),
        Err(GenericHumanReportError::SchemaViolation {
            name: "run schema",
            ..
        })
    ));
}

#[test]
fn v2_human_report_entry_point_rejects_v3_wire() {
    let v3 = include_bytes!("../../../schemas/reviewgraphen.generic_review_run.v3.example.json");
    assert!(generate_generic_human_report(v3).is_err());
}

#[test]
fn v3_positive_d_pair_is_projected_from_basis_bound_run() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let request = GenericReviewRequestV3 {
        schema: GENERIC_REVIEW_REQUEST_V3_SCHEMA.to_owned(),
        workspace_admission_root: repository.clone(),
        repository_admission_root: repository,
        repository_identity: "reviewgraphen".to_owned(),
        base_revision: "a8b6b24d5ed704f53f721b25db42d5d631f946c7".to_owned(),
        target_revision: "8569a2261e8a62145228872a2fde9f4c48093d00".to_owned(),
        ingest: GenericIngestRequestV2 {
            profile_id: "rust.production.v1".to_owned(),
            profile_version: "1".to_owned(),
            rule_set_hash: ContentHash::sha256(b"reviewgraphen-m20-rule-set@1"),
            max_files: 5_000,
            max_file_bytes: 4 * 1024 * 1024,
            max_total_source_bytes: 64 * 1024 * 1024,
        },
        plan: GenericPlanRequest {
            max_waves: 8,
            max_obligations_per_wave: 32,
        },
        observer: GenericObserverRequestV2::DeterministicAbstain,
        verifier_descriptor_id: None,
        context_policy_id: "context.subject_windows@3".to_owned(),
    };
    let run = run_generic_review_v3(&request).unwrap();
    assert_eq!(
        run.value()["contexts"].as_array().unwrap().len(),
        2,
        "ADR 0038 clause 7 D obligations"
    );
    let report = generate_generic_human_report_v3(&run).unwrap();
    let manifest: Value = serde_json::from_slice(&report.manifest_bytes).unwrap();
    assert!(human_v3_validator().is_valid(&manifest));
    assert_eq!(manifest["contexts"].as_array().unwrap().len(), 2);
    assert_eq!(manifest["authority"]["trusted_pass"], false);
    validate_generic_human_report_v3(&run, &report.manifest_bytes, &report.markdown_bytes).unwrap();
}

#[test]
fn audit_and_projection_tampering_are_rejected() {
    let value = with_outcomes("proposed_claim", 1);
    let (audit, report) = valid_report(&value);
    let mut changed_audit: Value = serde_json::from_slice(&audit).unwrap();
    changed_audit["authority"]["trusted_pass"] = json!(true);
    assert!(generate_generic_human_report(&bytes(&changed_audit)).is_err());

    let mut manifest: Value = serde_json::from_slice(&report.manifest_bytes).unwrap();
    manifest["audit"]["canonical_sha256"] = json!(ContentHash::sha256(b"forged").to_string());
    assert!(matches!(
        validate_generic_human_report(&audit, &bytes(&manifest), &report.markdown_bytes),
        Err(GenericHumanReportError::ManifestMismatch)
    ));
    let mut markdown = report.markdown_bytes.clone();
    markdown.push(b'!');
    assert!(matches!(
        validate_generic_human_report(&audit, &report.manifest_bytes, &markdown),
        Err(GenericHumanReportError::MarkdownMismatch)
    ));
}

#[test]
fn schema_closure_rejects_missing_extra_and_wrong_major() {
    let (_, report) = valid_report(&audit());
    let validator = human_validator();
    let manifest: Value = serde_json::from_slice(&report.manifest_bytes).unwrap();
    assert!(validator.is_valid(&manifest));
    let mut missing = manifest.clone();
    missing.as_object_mut().unwrap().remove("coverage");
    assert!(!validator.is_valid(&missing));
    let mut extra = manifest.clone();
    extra["unexpected"] = json!(true);
    assert!(!validator.is_valid(&extra));
    let mut wrong_major = manifest;
    wrong_major["schema"] = json!("reviewgraphen.generic_review_human_report.v2");
    assert!(!validator.is_valid(&wrong_major));
}

#[test]
fn checked_human_report_example_has_closed_schema_shape() {
    let example: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_human_report.v1.example.json"
    ))
    .unwrap();
    assert!(human_validator().is_valid(&example));
}

#[test]
fn checked_v3_human_report_example_has_closed_schema_shape() {
    let example: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_human_report.v2.example.json"
    ))
    .unwrap();
    assert!(human_v3_validator().is_valid(&example));
}

#[test]
fn deterministic_bounded_lists_and_escaped_unicode_are_stable() {
    let value = with_outcomes("proposed_claim", 2048);
    let (audit, first) = valid_report(&value);
    let second = generate_generic_human_report(&audit).unwrap();
    assert_eq!(first, second);
    let markdown = String::from_utf8(first.markdown_bytes).unwrap();
    assert!(markdown.contains("提案\\u0007\\nline"));
    assert!(!markdown.bytes().any(|byte| byte == 7));
}
