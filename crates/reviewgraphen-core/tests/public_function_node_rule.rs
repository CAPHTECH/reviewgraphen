use reviewgraphen_core::{MvpRulePack, ProgramSpace, RuleCoverageV3};
use serde_json::{Value, json};

const NODE_RULE: &str = "node.public_function_contract@1";
const NODE_PROPERTY: &str = "rust.public_function_contract_review@1";

fn production_program() -> ProgramSpace {
    let mut value: Value = serde_json::from_slice(include_bytes!(
        "../../../examples/double-submit-payment/program-space.json"
    ))
    .expect("reference ProgramSpace JSON");
    value["profile"]["id"] = json!("rust.production.v1");
    value["profile"]["version"] = json!("1");
    for id in ["file:checkout-controller", "file:payment-repository"] {
        value["artifacts"]
            .as_array_mut()
            .expect("artifacts")
            .iter_mut()
            .find(|artifact| artifact["id"] == id)
            .expect("file artifact")["kind"] = json!("module");
    }
    value["artifacts"]
        .as_array_mut()
        .expect("artifacts")
        .iter_mut()
        .find(|artifact| artifact["id"] == "function:payment-charge")
        .expect("function artifact")["attributes"]["public"] = json!(true);
    let mut payment_contains = value["relations"].as_array().expect("relations")[0].clone();
    payment_contains["id"] = json!("relation:payment-module-contains-charge");
    payment_contains["source_id"] = json!("file:payment-repository");
    payment_contains["target_ids"] = json!(["function:payment-charge"]);
    value["relations"]
        .as_array_mut()
        .expect("relations")
        .push(payment_contains);
    value["extraction"]["capabilities"]["ast"]["state"] = json!("complete");
    value["extraction"]["capabilities"]["containment"] = json!({
        "state": "complete",
        "source_ids": [
            "relation:file-contains-submit",
            "relation:payment-module-contains-charge"
        ]
    });
    ProgramSpace::from_json_slice(&serde_json::to_vec(&value).expect("JSON bytes"))
        .expect("valid production ProgramSpace")
}

#[test]
fn snapshot_without_diff_or_calls_still_synthesizes_one_node_per_public_function() {
    let program = production_program();
    let bundle = MvpRulePack::synthesize_rust_production_v2(&program).expect("mixed synthesis");
    let nodes = bundle
        .obligations()
        .iter()
        .filter(|obligation| obligation.version().rule() == NODE_RULE)
        .collect::<Vec<_>>();
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().all(|obligation| {
        obligation.property_id() == NODE_PROPERTY
            && obligation.target_kind() == "node"
            && obligation.target_refs().len() == 1
            && obligation
                .required_capabilities()
                .iter()
                .eq(["ast", "containment"])
    }));
    assert_eq!(bundle.universe().rule_coverages().len(), 2);
    assert!(matches!(
        &bundle.universe().rule_coverages()[0],
        RuleCoverageV3::ResolvedTargetWithCandidateGap(_)
    ));
    assert_eq!(bundle.universe().rule_coverages()[1].rule_id(), NODE_RULE);
    assert_eq!(
        bundle.universe().rule_coverages()[1]
            .eligible_target_obligation_ids()
            .expect("Node single-layer coverage")
            .len(),
        2
    );
}

#[test]
fn legacy_production_selector_remains_d_only() {
    let program = production_program();
    let legacy = MvpRulePack::synthesize(&program).expect("legacy synthesis");
    let d_only = MvpRulePack::synthesize_changed_public_callee(&program).expect("D synthesis");
    assert_eq!(legacy.contract().canonical(), d_only.contract().canonical());
    assert!(
        legacy
            .obligations()
            .iter()
            .all(|obligation| obligation.version().rule() != NODE_RULE)
    );
}
