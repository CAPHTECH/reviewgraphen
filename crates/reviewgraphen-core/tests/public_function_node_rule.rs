use reviewgraphen_core::{MvpRulePack, ProgramSpace, RuleCoverageV3};
use serde_json::{Value, json};

const NODE_RULE: &str = "node.public_function_contract@1";
const NODE_PROPERTY: &str = "rust.public_function_contract_review@1";

fn production_program(with_diff_or_calls: bool) -> ProgramSpace {
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
    if !with_diff_or_calls {
        value["artifacts"]
            .as_array_mut()
            .expect("artifacts")
            .iter_mut()
            .filter(|artifact| artifact["kind"] == "file")
            .for_each(|artifact| artifact["attributes"]["changed"] = json!(false));
        value["relations"]
            .as_array_mut()
            .expect("relations")
            .retain(|relation| relation["kind"] != "calls" && relation["kind"] != "covers");
        value["contexts"] = json!([]);
        value["invariants"] = json!([]);
        value["extraction"]["limitations"] = json!([]);
        for capability in value["extraction"]["capabilities"]
            .as_object_mut()
            .expect("capabilities")
            .values_mut()
        {
            capability["state"] = json!("complete");
        }
        for capability in ["direct_calls", "interprocedural_data_flow", "test_mapping"] {
            value["extraction"]["capabilities"][capability]["source_ids"] =
                json!(["relation:file-contains-submit"]);
        }
    }
    ProgramSpace::from_json_slice(&serde_json::to_vec(&value).expect("JSON bytes"))
        .expect("valid production ProgramSpace")
}

#[test]
fn snapshot_without_diff_or_calls_still_synthesizes_one_node_per_public_function() {
    let program = production_program(false);
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
    let program = production_program(true);
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

#[test]
fn public_function_without_containment_is_an_explicit_exclusion_not_a_silent_drop() {
    let mut value: Value = serde_json::from_slice(include_bytes!(
        "../../../examples/double-submit-payment/program-space.json"
    ))
    .expect("reference ProgramSpace JSON");
    value["profile"]["id"] = json!("rust.production.v1");
    value["profile"]["version"] = json!("1");
    value["artifacts"]
        .as_array_mut()
        .expect("artifacts")
        .iter_mut()
        .find(|artifact| artifact["id"] == "function:payment-charge")
        .expect("function artifact")["attributes"]["public"] = json!(true);
    value["relations"]
        .as_array_mut()
        .expect("relations")
        .retain(|relation| {
            relation["kind"] != "contains"
                || !relation["target_ids"]
                    .as_array()
                    .is_some_and(|targets| targets.iter().any(|id| id == "function:payment-charge"))
        });
    let program = ProgramSpace::from_json_slice(&serde_json::to_vec(&value).expect("JSON bytes"))
        .expect("valid production ProgramSpace");
    let bundle = MvpRulePack::synthesize_rust_production_v2(&program).expect("mixed synthesis");
    assert!(bundle.obligations().iter().all(|obligation| {
        obligation.version().rule() != NODE_RULE
            || obligation
                .target_refs()
                .first()
                .is_none_or(|target| target.as_str() != "function:payment-charge")
    }));
    assert!(bundle.universe().exclusions().iter().any(|exclusion| {
        exclusion.reason == "node.public_function_missing_containment@1"
            && exclusion
                .source_ids
                .iter()
                .any(|source_id| source_id.as_str() == "function:payment-charge")
    }));
}
