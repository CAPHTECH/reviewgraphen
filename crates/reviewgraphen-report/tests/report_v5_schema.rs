use jsonschema::{Draft, Resource};
use reviewgraphen_report::validate_v5_semantics;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

const V3_URI: &str = "https://capht.tech/schemas/reviewgraphen/review-report.v3.schema.json";
const V4_URI: &str = "https://capht.tech/schemas/reviewgraphen/review-report.v4.schema.json";

fn schema() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/reviewgraphen.report.v5.schema.json");
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

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
        .with_draft(Draft::Draft202012)
        .with_resource(V3_URI, Resource::from_contents(v3).unwrap())
        .with_resource(V4_URI, Resource::from_contents(v4).unwrap())
        .build(&schema())
        .unwrap()
}

fn hash() -> &'static str {
    "sha256:0000000000000000000000000000000000000000000000000000000000000000"
}

fn git_tree(digit: char) -> String {
    format!("git:{}", digit.to_string().repeat(40))
}

fn tuple(body: Value) -> Value {
    json!({"event_id":"event:test", "event_sequence":1, "body_hash":hash(), "body":body})
}

fn status_counts() -> Value {
    json!({"preserved":1,"modified":0,"added":0,"removed":0,"split":0,"merged":0,"unresolved":0})
}

fn closure() -> Value {
    tuple(json!({
        "schema":"reviewgraphen.incremental_source_closure.v5", "id":"incremental-source-closure-v5:test",
        "repository_id":"repository:test", "repository_identity_hash":hash(),
        "source_run_id":"run:source", "source_genesis_hash":hash(), "source_confirmed_offset":1,
        "source_tail_hash":hash(), "source_event_count":1, "source_snapshot_id":"snapshot:source",
        "source_universe_id":"universe:source", "source_index_snapshot_hash":hash(),
        "source_authority_policy_revision_hash":hash(), "source_authority_replay_basis_digest":hash(),
        "source_resolved_target_commit_oid":"000000000000000000000000000000000000000a", "source_target_tree_hash":git_tree('a'),
        "source_gluing_bundle_id":"gluing-bundle-v4:source", "target_run_id":"run:test", "target_genesis_hash":hash(),
        "target_predecessor_offset":1, "target_predecessor_tail_hash":hash(), "target_predecessor_event_count":1,
        "target_snapshot_id":"snapshot:test", "target_universe_id":"universe:test",
        "target_predecessor_index_snapshot_hash":hash(), "target_authority_policy_revision_hash":hash(),
        "target_pre_incremental_authority_replay_basis_digest":hash(),
        "target_resolved_base_commit_oid":"000000000000000000000000000000000000000a", "target_base_tree_hash":git_tree('a'),
        "target_resolved_target_commit_oid":"000000000000000000000000000000000000000b", "target_target_tree_hash":git_tree('b')
    }))
}

fn morphism() -> Value {
    tuple(json!({
        "schema":"reviewgraphen.change_morphism.v5", "id":"change-morphism-v5:test",
        "source_closure_id":"incremental-source-closure-v5:test", "repository_id":"repository:test",
        "source_snapshot_id":"snapshot:source", "target_snapshot_id":"snapshot:test",
        "mapping_policy_descriptor_id":"reviewgraphen.program_mapping@1", "semantic_anchor_descriptor_id":"reviewgraphen.rust_symbol_anchor@1",
        "mapping_count":1, "mapping_set_digest":hash(), "source_domain_count":1, "source_domain_digest":hash(),
        "target_domain_count":1, "target_domain_digest":hash(), "status_counts":status_counts(),
        "source_ids":["incremental-source-closure-v5:test"]
    }))
}

fn correspondence() -> Value {
    tuple(json!({
        "schema":"reviewgraphen.obligation_correspondence.v5", "id":"obligation-correspondence-v5:test",
        "morphism_id":"change-morphism-v5:test", "source_universe_id":"universe:source", "target_universe_id":"universe:test",
        "policy_descriptor_id":"reviewgraphen.obligation_correspondence@1", "entry_count":1, "entry_set_digest":hash(),
        "source_domain_count":1, "source_domain_digest":hash(), "target_domain_count":1, "target_domain_digest":hash(),
        "status_counts":status_counts(), "source_ids":["change-morphism-v5:test","universe:source","universe:test"]
    }))
}

fn mapping() -> Value {
    tuple(json!({
        "schema":"reviewgraphen.program_mapping.v5", "id":"program-mapping-v5:test",
        "source_closure_id":"incremental-source-closure-v5:test", "source_snapshot_id":"snapshot:source", "target_snapshot_id":"snapshot:test",
        "object_kind":"artifact", "from_ids":["artifact:source"], "to_ids":["artifact:test"], "status":"preserved",
        "candidate_key_kind":"same_path", "source_body_hashes":[{"id":"artifact:source","body_hash":hash()}],
        "target_body_hashes":[{"id":"artifact:test","body_hash":hash()}], "change_fact_ids":[], "predecessor_mapping_ids":[],
        "successor_ids":["artifact:test"], "source_ids":["artifact:source","artifact:test","incremental-source-closure-v5:test"]
    }))
}

fn partial_action(state: &str, witnesses: Value) -> Value {
    json!({
        "event_id":"event:test", "event_sequence":1, "body_hash":hash(),
        "body": {
            "schema":"reviewgraphen.partial_rerun_action.v5", "id":"partial-rerun-action-v5:test",
            "staleness_assessment_id":"staleness-assessment-v5:test", "subject_kind":"obligation",
            "subject_ids":["obligation:test"], "action":"reproject_context", "prerequisites":[],
            "stale_source_record_ids":[], "reasons":[], "source_ids":[]
        },
        "state":state, "completion_witness_ids":witnesses, "resolved_reviewer_output_records":[]
    })
}

fn gluing_action(prerequisites: Value) -> Value {
    json!({
        "event_id":"event:test", "event_sequence":1, "body_hash":hash(),
        "body": {
            "schema":"reviewgraphen.gluing_rerun_action.v5", "id":"gluing-rerun-action-v5:test",
            "planning_scope_id":"gluing-scope-v5:test", "subject_kind":"gluing_context",
            "subject_ids":["context:test"], "action":"reglue", "prerequisites":prerequisites,
            "reasons":["fresh_target_gluing_required"], "source_ids":[]
        },
        "state":"pending", "completion_witness_ids":[], "resolved_reviewer_output_records":[]
    })
}

fn report() -> Value {
    let metadata = json!({
        "report_id":"report:test","run_id":"run:test","profile_id":"code-review@1",
        "rule_set_hash":hash(),"extractor_set_hash":hash(),"policy_version":"default@1",
        "event_contract_version":"reviewgraphen.review_event.v5","index_projection_version":"reviewgraphen.index_projection.v6",
        "genesis_hash":hash(),"confirmed_offset":1,"confirmed_tail_hash":hash(),"confirmed_event_count":1,
        "tool_versions":{"reviewgraphen.runtime":"0.1.0"},"authority_policy_revision_hash":hash(),"authority_replay_basis_digest":hash(),
        "gluing_profile_descriptor_id":"reviewgraphen.double_submit_gluing@1","incremental_policy_descriptor_id":"reviewgraphen.incremental_gate@1",
        "source_run_id":"run:source","source_genesis_hash":hash(),"source_confirmed_offset":1,"source_tail_hash":hash(),"source_event_count":1,"source_index_snapshot_hash":hash(),"source_authority_policy_revision_hash":hash(),"source_authority_replay_basis_digest":hash(),
        "target_predecessor_offset":1,"target_predecessor_tail_hash":hash(),"target_predecessor_event_count":1,"target_predecessor_index_snapshot_hash":hash(),"target_pre_incremental_authority_replay_basis_digest":hash(),"target_index_snapshot_hash":hash()
    });
    let mut result = serde_json::Map::new();
    result.insert("status".to_owned(), json!("partial"));
    for name in [
        "artifact_registrations",
        "artifact_registrations_v5",
        "executions",
        "claims",
        "evidence",
        "evidence_bindings",
        "verifications",
        "decisions",
        "findings",
        "obstructions",
        "gluing_input_descriptors",
        "context_covers",
        "sections",
        "gluing_attempts",
        "restrictions",
        "global_candidates",
        "gluing_obstructions",
        "program_mappings",
        "obligation_correspondence_entries",
        "historical_record_assessments",
        "gluing_freshness",
        "preservation_evidence",
        "preservation_verifications",
        "partial_rerun_actions",
        "gluing_rerun_actions",
    ] {
        result.insert(name.to_owned(), json!([]));
    }
    for name in [
        "staleness_assessment",
        "partial_rerun_plan",
        "gluing_rerun_plan",
    ] {
        result.insert(name.to_owned(), Value::Null);
    }
    let mut coverage = serde_json::Map::new();
    coverage.insert("universe_id".to_owned(), json!("universe:test"));
    for name in [
        "denominator_obligation_ids",
        "visited_obligation_ids",
        "completed_obligation_ids",
        "evidence_supported_obligation_ids",
        "m5_dependent_successor_obligation_ids",
        "required_human_resolution_obligation_ids",
        "structurally_preserved_obligation_ids",
        "native_verified_obligation_ids",
        "verified_obligation_ids",
        "fresh_verified_obligation_ids",
        "accepted_obligation_ids",
    ] {
        coverage.insert(name.to_owned(), json!([]));
    }
    for name in [
        "selected",
        "visited",
        "completed",
        "evidence_supported",
        "m5_dependent_successors",
        "required_human_resolutions",
        "structurally_preserved",
        "native_verified",
        "verified",
        "fresh_verified",
        "accepted",
    ] {
        coverage.insert(name.to_owned(), json!(0));
    }
    json!({"schema":"reviewgraphen.review.report.v5","report_type":"review","report_version":5,"metadata":metadata,
      "scenario":{"repository_id":"repository:test","snapshot_id":"snapshot:test","program_space_ref":"program-space:snapshot:test","universe_id":"universe:test","plan_id":"plan:test","selected_obligation_ids":[],"artifact_registration_ids":[],"preservation_registration_ids":[],"context_cover_ids":[],"incremental_source_closure":closure(),"change_morphism":morphism(),"obligation_correspondence":correspondence()},
      "result":Value::Object(result),"coverage":Value::Object(coverage),"projection":{"views":[
        {"kind":"human","source_ids":["incremental-source-closure-v5:test"],"information_loss":[loss()],"payload":{"status":"partial","execution_ids":[],"claim_ids":[],"obstruction_kinds":[]}},
        {"kind":"ci","source_ids":["incremental-source-closure-v5:test"],"information_loss":[loss()],"payload":{"status":"partial","execution_ids":[],"claim_ids":[],"obstruction_kinds":[]}},
        {"kind":"machine","source_ids":["incremental-source-closure-v5:test"],"information_loss":[loss()],"payload":{"status":"partial","execution_ids":[],"claim_ids":[],"obstruction_kinds":[]}}]},
      "gate":{"schema":"reviewgraphen.incremental_gate.v5","id":"incremental-gate-v5:test","policy_descriptor_id":"reviewgraphen.incremental_gate@1","status":"pass","required_fresh_obligation_ids":[],"blocking_ids":[],"incomplete_ids":[],"reasons":[],"source_ids":["change-morphism-v5:test","incremental-source-closure-v5:test","obligation-correspondence-v5:test","partial-rerun-plan-v5:test","staleness-assessment-v5:test"],"body_hash":hash()}})
}

fn loss() -> Value {
    json!({"kind":"omitted_m6_incremental_records","reason":"view omits complete records from reviewgraphen.review.report.v5#/gate","source_ids":["incremental-gate-v5:test"],"affected_properties":["reviewgraphen.capability_gap"],"meaningful":true,"recoverable":true,"recovery_ref":"reviewgraphen.review.report.v5#/gate"})
}

fn semantically_valid_report() -> Value {
    let mut value = report();
    for key in [
        "denominator_obligation_ids",
        "native_verified_obligation_ids",
        "verified_obligation_ids",
        "fresh_verified_obligation_ids",
    ] {
        value["coverage"][key] = json!(["obligation:test"]);
    }
    for key in ["native_verified", "verified", "fresh_verified"] {
        value["coverage"][key] = json!(1);
    }
    value
}

#[test]
fn v5_semantics_requires_exact_zero_claim_cardinality_row() {
    let mut value = semantically_valid_report();
    value["result"]["obstructions"] = json!([{
        "kind":"m6_claim_cardinality_unsupported",
        "message":"M6 requires exactly one parsed claim; observed 0",
        "blocks":["obligation:test"],
        "source_ids":[
            "context-envelope:test",
            "event:execution",
            "execution:test",
            "incremental-source-closure-v5:test",
            "partial-rerun-action-v5:test",
            "partial-rerun-plan-v5:test",
            "registration:test"
        ]
    }]);
    assert!(validate_v5_semantics(&value).is_ok());
    value["result"]["obstructions"][0]["message"] = json!("caller selected one claim");
    assert!(validate_v5_semantics(&value).is_err());
}

#[test]
fn v5_schema_is_strict_at_envelope_and_gate_boundaries() {
    let validator = validator();
    let valid = report();
    assert!(validator.is_valid(&valid));
    let mut hostile = valid.clone();
    hostile["gate"]["untrusted"] = json!(true);
    assert!(!validator.is_valid(&hostile));
    let mut hostile = valid;
    hostile["metadata"]["event_contract_version"] = json!("reviewgraphen.review_event.v4");
    assert!(!validator.is_valid(&hostile));
    let mut hostile = report();
    hostile["scenario"]["incremental_source_closure"]["body"]["untyped"] = json!(true);
    assert!(!validator.is_valid(&hostile));
    let mut hostile = report();
    hostile["scenario"]["change_morphism"]["body"]["schema"] =
        json!("reviewgraphen.incremental_source_closure.v5");
    assert!(!validator.is_valid(&hostile));
    let mut hostile = report();
    hostile["result"]["program_mappings"] = json!([correspondence()]);
    assert!(!validator.is_valid(&hostile));
    let mut hostile = report();
    hostile["result"]["program_mappings"] = json!([mapping()]);
    assert!(validator.is_valid(&hostile));
    let mut hostile = report();
    hostile["projection"]["views"].as_array_mut().unwrap().pop();
    assert!(!validator.is_valid(&hostile));
    let mut hostile = report();
    hostile["projection"]["views"][0]["payload"]
        .as_object_mut()
        .unwrap()
        .remove("claim_ids");
    assert!(!validator.is_valid(&hostile));
    let mut hostile = report();
    hostile["projection"]["views"][0]["payload"]["extra"] = json!(true);
    assert!(!validator.is_valid(&hostile));
    // V5 deliberately does not retain the V3 replay-derived assessment
    // family: there is no durable V5 event witness for its tuple.  Keep this
    // contract closed so a caller cannot smuggle an un-witnessed assessment
    // array back into the terminal result.
    let mut hostile = report();
    hostile["result"]["claim_assessments"] = json!([]);
    assert!(!validator.is_valid(&hostile));
    let mut lossless = report();
    lossless["projection"]["views"][0]["information_loss"] = json!([]);
    assert!(validator.is_valid(&lossless));
    let mut hostile = report();
    hostile["gate"]["status"] = json!("pass");
    hostile["gate"]["reasons"] = json!(["rerun_pending"]);
    assert!(!validator.is_valid(&hostile));
}

#[test]
fn v5_schema_refuses_every_result_family_category_swap_and_bad_action_completion() {
    let validator = validator();
    let array_families = [
        "artifact_registrations",
        "artifact_registrations_v5",
        "executions",
        "claims",
        "evidence",
        "evidence_bindings",
        "verifications",
        "decisions",
        "findings",
        "gluing_input_descriptors",
        "context_covers",
        "sections",
        "gluing_attempts",
        "restrictions",
        "global_candidates",
        "gluing_obstructions",
        "obligation_correspondence_entries",
        "historical_record_assessments",
        "gluing_freshness",
        "preservation_evidence",
        "preservation_verifications",
        "gluing_rerun_actions",
    ];
    for family in array_families {
        let mut hostile = report();
        hostile["result"][family] = json!([mapping()]);
        assert!(
            !validator.is_valid(&hostile),
            "a program mapping must not validate as {family}"
        );
    }
    for family in [
        "staleness_assessment",
        "partial_rerun_plan",
        "gluing_rerun_plan",
    ] {
        let mut hostile = report();
        hostile["result"][family] = mapping();
        assert!(
            !validator.is_valid(&hostile),
            "a program mapping must not validate as {family}"
        );
    }

    let mut valid = report();
    valid["result"]["partial_rerun_actions"] = json!([partial_action("pending", json!([]))]);
    assert!(validator.is_valid(&valid));
    valid["result"]["partial_rerun_actions"][0]["completion_witness_ids"] = json!(["event:test"]);
    assert!(!validator.is_valid(&valid));

    let mut complete = report();
    complete["result"]["partial_rerun_actions"] = json!([partial_action("complete", json!([]))]);
    assert!(!validator.is_valid(&complete));
    complete["result"]["partial_rerun_actions"][0]["completion_witness_ids"] =
        json!(["event:test"]);
    assert!(validator.is_valid(&complete));

    // A reviewer may already be resolved while the verifier remains pending.
    // The report must not require minted execution/claim records prematurely.
    let mut verifier_pending = report();
    verifier_pending["result"]["partial_rerun_actions"] =
        json!([partial_action("pending", json!([]))]);
    verifier_pending["result"]["partial_rerun_actions"][0]["body"]["action"] =
        json!("rerun_verifier");
    assert!(validator.is_valid(&verifier_pending));

    let mut verifier_complete_without_outputs = verifier_pending;
    verifier_complete_without_outputs["result"]["partial_rerun_actions"][0]["state"] =
        json!("complete");
    verifier_complete_without_outputs["result"]["partial_rerun_actions"][0]["completion_witness_ids"] =
        json!(["event:test"]);
    assert!(!validator.is_valid(&verifier_complete_without_outputs));

    let mut non_verifier_outputs = report();
    non_verifier_outputs["result"]["partial_rerun_actions"] =
        json!([partial_action("complete", json!(["event:test"]))]);
    non_verifier_outputs["result"]["partial_rerun_actions"][0]["resolved_reviewer_output_records"] =
        json!([{}, {}]);
    assert!(!validator.is_valid(&non_verifier_outputs));
}

#[test]
fn v5_schema_accepts_only_the_exact_external_harness_registration_shape() {
    let validator = validator();
    let mut value = report();
    value["result"]["artifact_registrations"] = json!([tuple(json!({
        "registration_id":"registration:test", "run_id":"run:test", "cas_hash":hash(),
        "media_type":"application/vnd.reviewgraphen.test-witness+json;version=1", "size":145,
        "sensitivity":"canonical_state", "source":{
            "kind":"external_harness_witness", "claim_body_hash":hash(), "claim_id":"claim:test",
            "descriptor_id":"reviewgraphen.fixture_test_verifier@1", "genesis_hash":hash(),
            "harness_id":"reviewgraphen.double_submit_harness@1", "harness_revision":"1",
            "harness_source_hash":hash(), "policy_revision_hash":hash(),
            "procedure_version":"reviewgraphen.fixture_test.duplicate_submit@1",
            "property_id":"payment.at_most_once", "repository_id":"repository:test",
            "repository_source_hash":git_tree('a'), "run_id":"run:test",
            "snapshot_id":"snapshot:test", "test_artifact_id":"test:double-submit",
            "universe_id":"universe:test"
        }
    }))]);
    assert!(validator.is_valid(&value));

    let mut wrong_hash = value.clone();
    wrong_hash["result"]["artifact_registrations"][0]["body"]["source"]["repository_source_hash"] =
        json!(hash());
    assert!(!validator.is_valid(&wrong_hash));

    let mut unversioned_media_type = value.clone();
    unversioned_media_type["result"]["artifact_registrations"][0]["body"]["media_type"] =
        json!("application/vnd.reviewgraphen.test-witness+json");
    assert!(!validator.is_valid(&unversioned_media_type));

    value["result"]["artifact_registrations"][0]["body"]["source"]["untrusted"] = json!(true);
    assert!(!validator.is_valid(&value));
}

#[test]
fn v5_schema_and_semantics_close_git_revision_and_prefix_counts() {
    let validator = validator();
    let mut invalid_tree = report();
    invalid_tree["scenario"]["incremental_source_closure"]["body"]["source_target_tree_hash"] =
        json!(hash());
    assert!(!validator.is_valid(&invalid_tree));

    let mut invalid_oid = report();
    invalid_oid["scenario"]["incremental_source_closure"]["body"]["source_resolved_target_commit_oid"] =
        json!("ABCDEF");
    assert!(!validator.is_valid(&invalid_oid));

    let mut zero_source = report();
    zero_source["scenario"]["incremental_source_closure"]["body"]["source_event_count"] = json!(0);
    assert!(!validator.is_valid(&zero_source));

    let mut unequal_base = semantically_valid_report();
    unequal_base["scenario"]["incremental_source_closure"]["body"]["target_resolved_base_commit_oid"] =
        json!("000000000000000000000000000000000000000c");
    assert!(validate_v5_semantics(&unequal_base).is_err());

    let mut unchanged_target = semantically_valid_report();
    unchanged_target["scenario"]["incremental_source_closure"]["body"]["target_resolved_target_commit_oid"] =
        json!("000000000000000000000000000000000000000a");
    assert!(validate_v5_semantics(&unchanged_target).is_err());
}

#[test]
fn v5_schema_allows_the_adr_gluing_prerequisite_bound_of_eight() {
    let validator = validator();
    let prerequisites = json!(
        (0..8)
            .map(|number| json!({
                "kind":"existing_target_record", "record_id":format!("record:{number}"),
                "body_hash":hash(), "event_id":format!("event:{number}")
            }))
            .collect::<Vec<_>>()
    );
    let mut valid = report();
    valid["result"]["gluing_rerun_actions"] = json!([gluing_action(prerequisites)]);
    assert!(validator.is_valid(&valid));

    let prerequisites = json!(
        (0..9)
            .map(|number| json!({
                "kind":"existing_target_record", "record_id":format!("record:{number}"),
                "body_hash":hash(), "event_id":format!("event:{number}")
            }))
            .collect::<Vec<_>>()
    );
    valid["result"]["gluing_rerun_actions"] = json!([gluing_action(prerequisites)]);
    assert!(!validator.is_valid(&valid));
}

#[test]
fn v5_semantics_refuses_axis_and_view_loss_laundering() {
    let mut value = semantically_valid_report();
    assert!(validate_v5_semantics(&value).is_ok());
    value["coverage"]["verified_obligation_ids"] = json!([]);
    value["coverage"]["verified"] = json!(0);
    assert!(validate_v5_semantics(&value).is_err());

    let mut value = semantically_valid_report();
    value["projection"]["views"][0]["information_loss"] =
        json!((0..17).map(|_| loss()).collect::<Vec<_>>());
    // Schema uniqueness is deliberately independent; semantic validation
    // still rejects a hostile noncanonical array before source-bound output.
    assert!(validate_v5_semantics(&value).is_err());
}

#[test]
fn v5_semantics_refuses_gate_loss_and_action_state_swaps() {
    let mut value = semantically_valid_report();
    value["gate"]["status"] = json!("blocked");
    assert!(validate_v5_semantics(&value).is_err());

    let mut value = semantically_valid_report();
    value["gate"]["status"] = json!("incomplete");
    value["gate"]["blocking_ids"] = json!(["finding:test"]);
    value["gate"]["incomplete_ids"] = json!(["obligation:test"]);
    value["gate"]["reasons"] = json!(["fresh_verification_missing"]);
    assert!(validate_v5_semantics(&value).is_err());

    let mut value = semantically_valid_report();
    value["projection"]["views"][0]["information_loss"][0]["recovery_ref"] =
        json!("reviewgraphen.review.report.v5#/not-a-record-family");
    assert!(validate_v5_semantics(&value).is_err());

    let mut value = semantically_valid_report();
    value["result"]["partial_rerun_actions"] =
        json!([partial_action("pending", json!(["event:test"]))]);
    assert!(validate_v5_semantics(&value).is_err());
}
