use super::*;
use reviewgraphen_core::{CapabilityState, ContentHash, ProgramSpace, StableId, canonical_json};
use serde_json::{Value, json};

const FIXTURE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/structural-sloppiness-v1/fixtures/missing-bridge-program-space.json"
));
const REPORT_SCHEMA_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/structural-sloppiness-v1/schema/structural-sloppiness-report-v1.schema.json"
));

fn fixture_value() -> Value {
    serde_json::from_slice(FIXTURE).expect("fixture JSON")
}

fn program(value: &Value) -> ProgramSpace {
    let bytes = serde_json::to_vec(value).expect("fixture serialization");
    ProgramSpace::from_json_slice(&bytes).expect("fixture must pass the real ProgramSpace boundary")
}

fn base_program() -> ProgramSpace {
    ProgramSpace::from_json_slice(FIXTURE)
        .expect("fixture must pass the real ProgramSpace boundary")
}

fn artifact_mut<'a>(value: &'a mut Value, id: &str) -> &'a mut Value {
    value["artifacts"]
        .as_array_mut()
        .expect("artifacts")
        .iter_mut()
        .find(|artifact| artifact["id"] == id)
        .expect("artifact")
}

fn changed_by_provenance(value: &Value) -> Value {
    value["relations"][0]["provenance"].clone()
}

fn ordinary_provenance(value: &Value) -> Value {
    value["artifacts"]
        .as_array()
        .expect("artifacts")
        .iter()
        .find(|artifact| artifact["id"] == "file:lib")
        .expect("file")
        .get("provenance")
        .expect("provenance")
        .clone()
}

fn push_relation(
    value: &mut Value,
    id: &str,
    kind: &str,
    source: &str,
    target: &str,
    provenance: Value,
) {
    value["relations"]
        .as_array_mut()
        .expect("relations")
        .push(json!({
            "id": id,
            "kind": kind,
            "source_id": source,
            "target_ids": [target],
            "directed": true,
            "attributes": {},
            "provenance": provenance
        }));
}

fn only_observation(report: &AnalysisReport) -> &ObservedGate {
    assert_eq!(report.observed_facts.len(), 1);
    &report.observed_facts[0]
}

fn ids(values: &[&str]) -> Vec<StableId> {
    values
        .iter()
        .map(|value| StableId::parse(*value).expect("stable ID"))
        .collect()
}

#[test]
fn structural_sloppiness_missing_bridge_is_an_observed_gate_and_candidate_only() {
    let report = analyze(&base_program()).expect("acceptance implementation");

    assert_eq!(report.schema, REPORT_SCHEMA);
    assert_eq!(report.contract.id, CONTRACT_ID);
    assert_eq!(report.contract.producer_provenance, PRODUCER_PROVENANCE);
    assert_eq!(report.scope.status, ExerciseStatus::Exercised);
    assert_eq!(report.scope.eligible_count, 1);
    assert!(!only_observation(&report).consumer_observed_changed);
    assert_eq!(report.candidate_claims.len(), 1);
    assert_eq!(
        report.candidate_claims[0].observation_ids,
        vec![report.observed_facts[0].id.clone()]
    );
    assert_eq!(report.candidate_claims[0].disposition, "candidate");
    assert!(report.obstructions.is_empty());
    assert_eq!(report.authority.classification, "non_authority");
    assert!(!report.authority.accepted);
    assert!(!report.authority.verified);
    assert!(!report.authority.human_accepted);
    assert!(!report.authority.sign_off);
}

#[test]
fn structural_sloppiness_actual_added_change_path_shape_is_eligible() {
    let value = fixture_value();
    let change = value["artifacts"]
        .as_array()
        .expect("artifacts")
        .iter()
        .find(|artifact| artifact["id"] == "change:lib")
        .expect("change artifact");
    assert_eq!(change["attributes"]["change_kind"], "added");
    assert_eq!(change["attributes"]["base_path"], "");
    assert_eq!(change["attributes"]["target_path"], "src/lib.rs");

    let report = analyze(&program(&value)).expect("actual producer added shape");
    assert_eq!(report.scope.status, ExerciseStatus::Exercised);
    assert_eq!(report.scope.eligible_count, 1);
}

#[test]
fn structural_sloppiness_consumer_predicate_preserves_both_exact_or_branches() {
    let mut own = fixture_value();
    artifact_mut(&mut own, "function:target")["attributes"]["changed"] = json!(true);
    let own_report = analyze(&program(&own)).expect("own marker branch");
    assert!(only_observation(&own_report).own_changed);
    assert!(only_observation(&own_report).consumer_observed_changed);
    assert!(own_report.candidate_claims.is_empty());

    let mut bridge = fixture_value();
    let provenance = changed_by_provenance(&bridge);
    push_relation(
        &mut bridge,
        "relation:change-contains-target",
        "contains",
        "change:lib",
        "function:target",
        provenance,
    );
    let bridge_report = analyze(&program(&bridge)).expect("one-hop incoming contains branch");
    let observation = only_observation(&bridge_report);
    assert!(!observation.own_changed);
    assert_eq!(observation.marked_container_ids, ids(&["change:lib"]));
    assert_eq!(
        observation.matching_contains_relation_ids,
        ids(&["relation:change-contains-target"])
    );
    assert!(observation.consumer_observed_changed);
    assert!(bridge_report.candidate_claims.is_empty());
}

#[test]
fn structural_sloppiness_any_marked_incoming_container_counts_not_only_the_producer_change() {
    let mut value = fixture_value();
    artifact_mut(&mut value, "file:lib")["attributes"]["changed"] = json!(true);
    let provenance = ordinary_provenance(&value);
    push_relation(
        &mut value,
        "relation:ordinary-marked-container",
        "contains",
        "file:lib",
        "function:target",
        provenance,
    );

    let report = analyze(&program(&value)).expect("any marked container contract");
    assert!(only_observation(&report).consumer_observed_changed);
    assert_eq!(
        only_observation(&report).marked_container_ids,
        ids(&["file:lib"])
    );
    assert!(report.candidate_claims.is_empty());
}

#[test]
fn structural_sloppiness_consumer_bridge_is_exactly_one_hop_not_transitive() {
    let mut value = fixture_value();
    artifact_mut(&mut value, "file:lib")["attributes"]["changed"] = json!(true);
    let provenance = ordinary_provenance(&value);
    push_relation(
        &mut value,
        "relation:marked-file-contains-change",
        "contains",
        "file:lib",
        "change:lib",
        provenance.clone(),
    );
    push_relation(
        &mut value,
        "relation:unmarked-change-contains-target",
        "contains",
        "change:lib",
        "function:target",
        provenance,
    );
    artifact_mut(&mut value, "change:lib")["attributes"]
        .as_object_mut()
        .expect("attributes")
        .remove("changed");

    let report = analyze(&program(&value)).expect("bounded one-hop consumer predicate");
    assert!(!only_observation(&report).consumer_observed_changed);
    assert_eq!(report.candidate_claims.len(), 1);
}

#[test]
fn structural_sloppiness_missing_marker_and_missing_reciprocal_edge_are_reported() {
    let mut missing_marker = fixture_value();
    artifact_mut(&mut missing_marker, "change:lib")["attributes"]
        .as_object_mut()
        .expect("attributes")
        .remove("changed");
    let provenance = changed_by_provenance(&missing_marker);
    push_relation(
        &mut missing_marker,
        "relation:unmarked-change-contains-target",
        "contains",
        "change:lib",
        "function:target",
        provenance,
    );
    let marker_report = analyze(&program(&missing_marker)).expect("missing marker");
    assert_eq!(marker_report.scope.eligible_count, 1);
    assert!(!only_observation(&marker_report).consumer_observed_changed);
    assert_eq!(marker_report.candidate_claims.len(), 1);

    let bridge_report = analyze(&base_program()).expect("missing reciprocal edge");
    assert_eq!(bridge_report.scope.eligible_count, 1);
    assert!(
        only_observation(&bridge_report)
            .matching_contains_relation_ids
            .is_empty()
    );
    assert_eq!(bridge_report.candidate_claims.len(), 1);
}

#[test]
fn structural_sloppiness_rejects_wrong_direction_wrong_target_and_unmarked_containers() {
    for (id, source, target, marked_file) in [
        (
            "relation:wrong-target",
            "change:lib",
            "function:other",
            false,
        ),
        ("relation:reversed", "function:target", "change:lib", false),
        (
            "relation:ordinary-unmarked",
            "file:lib",
            "function:target",
            false,
        ),
    ] {
        let mut value = fixture_value();
        artifact_mut(&mut value, "file:lib")["attributes"]["changed"] = json!(marked_file);
        let provenance = changed_by_provenance(&value);
        push_relation(&mut value, id, "contains", source, target, provenance);
        let report = analyze(&program(&value)).expect("misleading relation remains analyzable");
        assert!(
            !only_observation(&report).consumer_observed_changed,
            "{id} must not satisfy contains(X, S) with X.changed == true"
        );
        assert_eq!(report.candidate_claims.len(), 1, "{id}");
    }
}

#[test]
fn structural_sloppiness_eligibility_requires_public_free_function_and_both_provenances() {
    let cases = [
        ("non_public", ExclusionReason::NonPublicFunction),
        ("method", ExclusionReason::NotFreeFunction),
        (
            "producer_provenance",
            ExclusionReason::ProducerProvenanceMismatch,
        ),
        (
            "change_provenance",
            ExclusionReason::ChangeProvenanceMismatch,
        ),
        ("change_shape", ExclusionReason::ChangeShapeInvalid),
    ];

    for (case, reason) in cases {
        let mut value = fixture_value();
        match case {
            "non_public" => {
                artifact_mut(&mut value, "function:target")["attributes"]["public"] = json!(false);
            }
            "method" => {
                value["relations"][0]["source_id"] = json!("method:target");
            }
            "producer_provenance" => {
                value["relations"][0]["provenance"]["extraction_method"] =
                    json!("fixture.changed_by.v1");
            }
            "change_provenance" => {
                artifact_mut(&mut value, "change:lib")["provenance"]["extraction_method"] =
                    json!("fixture.change.v1");
            }
            "change_shape" => {
                artifact_mut(&mut value, "change:lib")["attributes"]
                    .as_object_mut()
                    .expect("attributes")
                    .remove("target_path");
            }
            _ => unreachable!(),
        }

        let report = analyze(&program(&value)).expect("ineligible input still reports scope");
        assert_eq!(report.scope.status, ExerciseStatus::NotExercised, "{case}");
        assert_eq!(report.scope.eligible_count, 0, "{case}");
        assert_eq!(report.scope.excluded_count, 1, "{case}");
        assert_eq!(
            report.scope.excluded_by_reason.get(&reason),
            Some(&1),
            "{case}"
        );
        assert!(report.observed_facts.is_empty(), "{case}");
        assert!(report.candidate_claims.is_empty(), "{case}");
    }
}

#[test]
fn structural_sloppiness_zero_denominator_is_not_exercised_never_clean() {
    let mut value = fixture_value();
    value["relations"] = json!([]);
    value["extraction"]["capabilities"]["changed_structure"]["source_ids"] = json!(["change:lib"]);
    let report = analyze(&program(&value)).expect("zero denominator report");

    assert_eq!(report.scope.status, ExerciseStatus::NotExercised);
    assert_eq!(report.scope.eligible_count, 0);
    assert!(report.observed_facts.is_empty());
    assert!(report.candidate_claims.is_empty());
    assert!(!report.authority.sign_off);
}

#[test]
fn structural_sloppiness_partial_extraction_keeps_observation_and_blocks_global_authority() {
    let mut value = fixture_value();
    value["extraction"]["capabilities"]["containment"]["state"] = json!("partial");
    value["extraction"]["limitations"] = json!([{
        "id": "limitation:partial-containment",
        "kind": "projection_loss",
        "description": "fixture intentionally omits some containment edges",
        "severity": "medium",
        "source_ids": ["snapshot:structural-sloppiness-missing-bridge"],
        "related_capabilities": ["containment"]
    }]);
    let report = analyze(&program(&value)).expect("partial extraction report");

    assert_eq!(report.scope.eligible_count, 1);
    assert_eq!(report.candidate_claims.len(), 1);
    assert!(!only_observation(&report).consumer_observed_changed);
    assert!(report.scope.capabilities.iter().any(|capability| {
        capability.capability == "containment" && capability.state == CapabilityState::Partial
    }));
    assert_eq!(
        report.scope.limitation_ids,
        ids(&["limitation:partial-containment"])
    );
    assert!(
        report
            .obstructions
            .iter()
            .any(|obstruction| obstruction.kind == "partial_extraction")
    );
    assert!(!report.authority.accepted);
    assert!(!report.authority.verified);
    assert!(!report.authority.sign_off);
}

#[test]
fn structural_sloppiness_output_order_is_deterministic() {
    let mut left = fixture_value();
    let target = artifact_mut(&mut left, "function:other").clone();
    assert_eq!(target["id"], "function:other");
    let provenance = changed_by_provenance(&left);
    push_relation(
        &mut left,
        "relation:other-changed-by-lib",
        "changed_by",
        "function:other",
        "change:lib",
        provenance,
    );
    let mut right = left.clone();
    right["artifacts"]
        .as_array_mut()
        .expect("artifacts")
        .reverse();
    right["relations"]
        .as_array_mut()
        .expect("relations")
        .reverse();

    let left_report = analyze(&program(&left)).expect("left ordering");
    let right_report = analyze(&program(&right)).expect("right ordering");
    assert_eq!(left_report, right_report);
    assert_eq!(
        canonical_json(&left_report).expect("canonical left"),
        canonical_json(&right_report).expect("canonical right")
    );
    assert!(
        left_report
            .observed_facts
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id)
    );
}

#[test]
fn structural_sloppiness_binds_full_input_and_rejects_stale_or_tampered_reports() {
    let input = base_program();
    let mut report = analyze(&input).expect("bound report");
    let expected_hash = ContentHash::sha256(&canonical_json(&input).expect("canonical input"));
    assert_eq!(report.input.program_space_hash, expected_hash);
    validate_report(&input, &report).expect("fresh report validates");

    report.input.program_space_hash = ContentHash::sha256(b"tampered binding");
    assert!(validate_report(&input, &report).is_err());

    let mut report = analyze(&input).expect("bound report");
    report.candidate_claims[0].statement.push_str(" tampered");
    assert!(validate_report(&input, &report).is_err());

    let mut other_value = fixture_value();
    artifact_mut(&mut other_value, "function:target")["attributes"]["changed"] = json!(true);
    assert!(
        validate_report(
            &program(&other_value),
            &analyze(&input).expect("old report")
        )
        .is_err()
    );
}

#[test]
fn structural_sloppiness_schema_document_is_valid() {
    let schema: Value = serde_json::from_str(REPORT_SCHEMA_JSON).expect("schema JSON");
    jsonschema::validator_for(&schema).expect("valid Draft 2020-12 schema");
}

#[test]
fn structural_sloppiness_report_serialization_satisfies_closed_schema() {
    let report = analyze(&base_program()).expect("schema report");
    let schema: Value = serde_json::from_str(REPORT_SCHEMA_JSON).expect("schema JSON");
    let validator = jsonschema::validator_for(&schema).expect("valid Draft 2020-12 schema");
    let mut report_json = serde_json::to_value(&report).expect("report JSON");
    assert!(validator.is_valid(&report_json));

    report_json["undeclared_authority"] = json!(true);
    assert!(!validator.is_valid(&report_json));
    report_json
        .as_object_mut()
        .expect("report object")
        .remove("undeclared_authority");
    report_json
        .as_object_mut()
        .expect("report object")
        .remove("authority");
    assert!(!validator.is_valid(&report_json));
}

#[test]
fn structural_sloppiness_flat_endpoint_erasure_is_an_explicit_loss_ablation() {
    let mut matching = fixture_value();
    let provenance = changed_by_provenance(&matching);
    push_relation(
        &mut matching,
        "relation:bridge-under-ablation",
        "contains",
        "change:lib",
        "function:target",
        provenance.clone(),
    );
    let mut rewired = fixture_value();
    push_relation(
        &mut rewired,
        "relation:bridge-under-ablation",
        "contains",
        "change:lib",
        "function:other",
        provenance,
    );
    let matching_program = program(&matching);
    let rewired_program = program(&rewired);

    assert_eq!(
        flat_projection_canonical_bytes(&matching_program).expect("matching flat projection"),
        flat_projection_canonical_bytes(&rewired_program).expect("rewired flat projection"),
        "artifact records and relation kind/provenance inventory are unchanged when endpoints vanish"
    );
    let matching_report = analyze(&matching_program).expect("matching graph report");
    let rewired_report = analyze(&rewired_program).expect("rewired graph report");
    assert!(only_observation(&matching_report).consumer_observed_changed);
    assert!(!only_observation(&rewired_report).consumer_observed_changed);
    assert_ne!(matching_report, rewired_report);
    assert!(
        matching_report
            .scope
            .information_loss
            .iter()
            .any(|loss| loss == "report_projection_omits_unrelated_program_facts")
    );
}
