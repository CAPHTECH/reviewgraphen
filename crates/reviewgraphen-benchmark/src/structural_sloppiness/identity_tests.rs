use super::{analyze, validate_report};
use reviewgraphen_core::ProgramSpace;
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[test]
fn structural_sloppiness_distinguishes_colon_bearing_identity_tuples() {
    let mut value: Value = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../benchmarks/structural-sloppiness-v1/fixtures/missing-bridge-program-space.json"
    )))
    .expect("fixture JSON");
    let artifacts = value["artifacts"].as_array_mut().expect("artifacts");
    let change = artifacts
        .iter_mut()
        .find(|artifact| artifact["id"] == "change:lib")
        .expect("change artifact");
    change["id"] = json!("change:z");
    let mut second_change = change.clone();
    second_change["id"] = json!("change:y:change:z");
    artifacts.push(second_change);

    value["relations"][0]["id"] = json!("relation:x:change:y");
    value["relations"][0]["target_ids"] = json!(["change:z"]);
    let mut second_relation = value["relations"][0].clone();
    second_relation["id"] = json!("relation:x");
    second_relation["target_ids"] = json!(["change:y:change:z"]);
    value["relations"]
        .as_array_mut()
        .expect("relations")
        .push(second_relation);
    value["extraction"]["capabilities"]["changed_structure"]["source_ids"] = json!([
        "change:z",
        "change:y:change:z",
        "relation:x",
        "relation:x:change:y"
    ]);

    let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&value).expect("JSON"))
        .expect("the collision input is a valid accepted ProgramSpace");
    let report = analyze(&program).expect("analyze");
    assert_eq!(report.scope.eligible_count, 2);
    let observations = report
        .observed_facts
        .iter()
        .map(|observation| &observation.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(observations.len(), 2, "distinct gates need distinct IDs");
    let claims = report
        .candidate_claims
        .iter()
        .map(|claim| &claim.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(claims.len(), 2, "distinct candidates need distinct IDs");
    validate_report(&program, &report).expect("complete report validates");
}
