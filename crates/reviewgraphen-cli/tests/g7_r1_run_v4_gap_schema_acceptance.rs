//! G7-R1 acceptance O1 (Acc-R1, frozen before production): run-v4 gap-arm
//! widening, scope 1.
//!
//! The `capability_gap.origin_rule@1` arm of
//! `schemas/reviewgraphen.generic_review_run.v4.schema.json` must accept a
//! `target_support_capabilities` array that contains `"direct_calls"` plus any
//! subset of `{ast, changed_structure, containment}`, sorted, without
//! duplicates; `enumeration_capabilities` stays `const []`; no other arm or
//! field changes.
//!
//! Two layers are asserted separately (so a rejection is attributable):
//! * structural: the printed run-v4 schema (`schema print`) compiled with the
//!   `jsonschema` crate the CLI itself uses;
//! * CLI `schema validate` (structure + `validate_generic_review_run_v4_wire_structure`,
//!   `crates/reviewgraphen-cli/src/lib.rs` `semantic_validation`).
//!
//! Fixture: the checked-in `schemas/reviewgraphen.generic_review_run.v4.example.json`
//! (whose gap row is `obligation_contract[1]`) with exactly one field mutated.

use reviewgraphen_cli::run;
use reviewgraphen_core::canonical_json;
use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;

const RUN_V4: &str = "reviewgraphen.generic_review_run.v4";
const GAP_RULE: &str = "capability_gap.origin_rule@1";
const D_RULE: &str = "relation.changed_public_callee@1";
const NODE_RULE: &str = "node.public_function_contract@1";

fn cli(arguments: &[&str]) -> reviewgraphen_cli::CommandOutcome {
    run(arguments
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect())
}

fn example() -> Value {
    serde_json::from_slice(include_bytes!(
        "../../../schemas/reviewgraphen.generic_review_run.v4.example.json"
    ))
    .expect("run-v4 example JSON")
}

fn row_index(run: &Value, rule: &str) -> usize {
    let rows = run["obligation_contract"]
        .as_array()
        .expect("contract rows");
    let hits = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row["rule_id"] == rule)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(hits.len(), 1, "example has exactly one {rule} row");
    hits[0]
}

fn with_field(rule: &str, field: &str, value: Value) -> Value {
    let mut run = example();
    let index = row_index(&run, rule);
    run["obligation_contract"][index][field] = value;
    run
}

fn gap_target_support(capabilities: &[&str]) -> Value {
    with_field(GAP_RULE, "target_support_capabilities", json!(capabilities))
}

fn structural() -> jsonschema::Validator {
    let outcome = cli(&["schema", "print", RUN_V4]);
    assert_eq!(outcome.exit_code, 0, "run-v4 schema is registered");
    let schema: Value = serde_json::from_slice(&outcome.stdout).expect("printed schema JSON");
    jsonschema::validator_for(&schema).expect("printed run-v4 schema compiles")
}

fn cli_validate(document: &Value) -> (u8, Value) {
    let directory = TempDir::new().expect("temporary directory");
    let path = directory.path().join("audit.run.v4.json");
    fs::write(&path, canonical_json(document).expect("canonical document")).expect("write");
    let outcome = cli(&["schema", "validate", path.to_str().expect("UTF-8 path")]);
    let verdict = serde_json::from_slice(&outcome.stdout).unwrap_or(Value::Null);
    (outcome.exit_code, verdict)
}

fn assert_valid(document: &Value, name: &str) {
    assert!(
        structural().is_valid(document),
        "{name}: structural run-v4 schema must accept"
    );
    let (exit, verdict) = cli_validate(document);
    assert_eq!(
        exit, 0,
        "{name}: `schema validate` must accept; verdict={verdict}"
    );
    assert_eq!(verdict, json!({"schema": RUN_V4, "valid": true}), "{name}");
}

fn assert_invalid(document: &Value, name: &str) {
    assert!(
        !structural().is_valid(document),
        "{name}: structural run-v4 schema must reject"
    );
    let (exit, verdict) = cli_validate(document);
    assert_eq!(
        exit, 3,
        "{name}: `schema validate` must reject; verdict={verdict}"
    );
    assert_eq!(
        verdict,
        json!({"valid": false, "reason": "schema_invalid"}),
        "{name}"
    );
}

/// Control: the unmutated example, including its `["direct_calls"]` gap row,
/// is valid before and after the widening (escape check for the helpers).
#[test]
fn o1_control_unmutated_example_is_valid_in_both_layers() {
    let run = example();
    assert_eq!(
        run["obligation_contract"][row_index(&run, GAP_RULE)]["target_support_capabilities"],
        json!(["direct_calls"])
    );
    assert_valid(&run, "unmutated example");
}

/// All eight sorted shapes: `direct_calls` plus each sorted subset of
/// `{ast, changed_structure, containment}`.
#[test]
fn o1_every_sorted_subset_with_direct_calls_is_schema_valid() {
    let subsets: [&[&str]; 8] = [
        &["direct_calls"],
        &["ast", "direct_calls"],
        &["changed_structure", "direct_calls"],
        &["containment", "direct_calls"],
        &["ast", "changed_structure", "direct_calls"],
        &["ast", "containment", "direct_calls"],
        &["changed_structure", "containment", "direct_calls"],
        &["ast", "changed_structure", "containment", "direct_calls"],
    ];
    for subset in subsets {
        assert_valid(
            &gap_target_support(subset),
            &format!("gap target support {subset:?}"),
        );
    }
}

#[test]
fn o1_missing_direct_calls_is_schema_invalid() {
    for shape in [
        &[][..],
        &["ast"][..],
        &["ast", "containment"][..],
        &["changed_structure"][..],
        &["ast", "changed_structure", "containment"][..],
    ] {
        assert_invalid(
            &gap_target_support(shape),
            &format!("no direct_calls {shape:?}"),
        );
    }
}

#[test]
fn o1_direct_calls_absent_but_ast_present_is_schema_invalid() {
    assert_invalid(&gap_target_support(&["ast"]), "ast without direct_calls");
    assert_invalid(
        &gap_target_support(&["ast", "containment"]),
        "ast+containment without direct_calls",
    );
}

#[test]
fn o1_unsorted_is_schema_invalid() {
    for shape in [
        &["direct_calls", "ast"][..],
        &["containment", "ast", "direct_calls"][..],
        &["ast", "direct_calls", "containment"][..],
        &["direct_calls", "changed_structure", "containment", "ast"][..],
    ] {
        assert_invalid(&gap_target_support(shape), &format!("unsorted {shape:?}"));
    }
}

#[test]
fn o1_duplicate_is_schema_invalid() {
    for shape in [
        &["direct_calls", "direct_calls"][..],
        &["ast", "ast", "direct_calls"][..],
        &["ast", "containment", "containment", "direct_calls"][..],
    ] {
        assert_invalid(&gap_target_support(shape), &format!("duplicate {shape:?}"));
    }
}

#[test]
fn o1_unknown_capability_is_schema_invalid() {
    for shape in [
        &["concurrency_model", "direct_calls"][..],
        &["ast", "bogus", "direct_calls"][..],
        &["direct_calls", "zzz"][..],
        &["AST", "direct_calls"][..],
    ] {
        assert_invalid(&gap_target_support(shape), &format!("unknown {shape:?}"));
    }
    let mut non_string = example();
    let index = row_index(&non_string, GAP_RULE);
    non_string["obligation_contract"][index]["target_support_capabilities"] =
        json!([1, "direct_calls"]);
    assert_invalid(&non_string, "non-string capability");
}

#[test]
fn o1_non_empty_enumeration_capabilities_is_schema_invalid() {
    for (target, enumeration) in [
        (json!(["direct_calls"]), json!(["direct_calls"])),
        (
            json!(["ast", "containment", "direct_calls"]),
            json!(["direct_calls"]),
        ),
        (
            json!(["ast", "containment", "direct_calls"]),
            json!(["ast"]),
        ),
    ] {
        let mut run = gap_target_support(&[]);
        let index = row_index(&run, GAP_RULE);
        run["obligation_contract"][index]["target_support_capabilities"] = target.clone();
        run["obligation_contract"][index]["enumeration_capabilities"] = enumeration.clone();
        assert_invalid(
            &run,
            &format!("gap enumeration {enumeration} with target {target}"),
        );
    }
}

/// Scope guard: "no other arm or field changes". These pass before and after
/// the fix; they fail only if the widening leaks into the D/Node arms or the
/// gap arm's other fields.
#[test]
fn o1_other_arms_and_gap_fields_stay_closed() {
    assert_invalid(
        &with_field(
            NODE_RULE,
            "target_support_capabilities",
            json!(["ast", "containment", "direct_calls"]),
        ),
        "Node arm with direct_calls",
    );
    assert_invalid(
        &with_field(
            D_RULE,
            "target_support_capabilities",
            json!(["ast", "containment"]),
        ),
        "D arm without changed_structure",
    );
    assert_invalid(
        &with_field(
            D_RULE,
            "target_support_capabilities",
            json!(["ast", "changed_structure", "containment", "direct_calls"]),
        ),
        "D arm with direct_calls in target support",
    );
    assert_invalid(
        &with_field(GAP_RULE, "applicability_status", json!("applicable")),
        "gap row applicable",
    );
    assert_invalid(
        &with_field(GAP_RULE, "target_kind", json!("relation")),
        "gap row relation target",
    );
}
