use super::analyze;
use reviewgraphen_core::ProgramSpace;
use serde_json::{Value, json};

#[test]
fn structural_sloppiness_candidates_follow_their_own_id_order() {
    let mut value: Value = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../benchmarks/structural-sloppiness-v1/fixtures/missing-bridge-program-space.json"
    )))
    .unwrap();
    let template = value["relations"][0].clone();
    for index in 0..15 {
        let mut relation = template.clone();
        relation["id"] = json!(format!("relation:ordering-{index}"));
        value["relations"].as_array_mut().unwrap().push(relation);
    }
    let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
    let report = analyze(&program).unwrap();
    assert_eq!(report.candidate_claims.len(), 16);
    assert!(
        report
            .observed_facts
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id)
    );
    assert!(
        report
            .candidate_claims
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id),
        "candidate order must use candidate IDs, not observation IDs"
    );
}
