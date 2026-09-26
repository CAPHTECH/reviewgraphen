//! G7-R1 acceptance O3 (Acc-R1, frozen before production),
//! scope 3: "validate run-v4 against its
//! schema before `ValidatedGenericReviewRunV4::new` (generic.rs:3140)".
//!
//! Interface assumed `[R]`: the crate-private constructor
//! `ValidatedGenericReviewRunV4::new(serde_json::Value)` itself refuses a
//! value that fails `schemas/reviewgraphen.generic_review_run.v4.schema.json`,
//! i.e. it becomes fallible (`-> Result<ValidatedGenericReviewRunV4, _>`,
//! any error type). Every construction site then earns the type's name,
//! including `run_generic_review_v4` (generic.rs:3140) and
//! `decode_generic_review_run_v4_with_basis` (generic.rs:5444).
//!
//! `Construction` lets this file compile against both the current infallible
//! signature (every value is "accepted" -> the refusal tests are red) and the
//! fallible one (green once validation is in place). This is wiring for a
//! crate-private item only; it is declared from generic.rs under
//! `#[cfg(test)]` and is absent from non-test builds.

use super::ValidatedGenericReviewRunV4;
use reviewgraphen_core::canonical_json;
use serde_json::{Value, json};

trait Construction {
    fn accepted(self) -> bool;
}

impl Construction for ValidatedGenericReviewRunV4 {
    fn accepted(self) -> bool {
        true
    }
}

impl<E> Construction for Result<ValidatedGenericReviewRunV4, E> {
    fn accepted(self) -> bool {
        self.is_ok()
    }
}

fn construct(value: Value) -> bool {
    ValidatedGenericReviewRunV4::new(value).accepted()
}

fn example() -> Value {
    serde_json::from_slice(include_bytes!(
        "../../../schemas/reviewgraphen.generic_review_run.v4.example.json"
    ))
    .expect("run-v4 example JSON")
}

fn gap_index(run: &Value) -> usize {
    run["obligation_contract"]
        .as_array()
        .expect("contract")
        .iter()
        .position(|row| row["rule_id"] == "capability_gap.origin_rule@1")
        .expect("example gap row")
}

fn schema_valid(value: &Value) -> bool {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_run.v4.schema.json"
    ))
    .expect("schema JSON");
    jsonschema::validator_for(&schema)
        .expect("schema compiles")
        .is_valid(value)
}

#[test]
fn o3_control_schema_valid_example_is_constructible() {
    let value = example();
    assert!(
        schema_valid(&value),
        "precondition: example is schema-valid"
    );
    assert!(
        construct(value.clone()),
        "a schema-valid run-v4 must construct"
    );
    let bytes = canonical_json(&value).expect("canonical");
    assert!(!bytes.is_empty());
}

/// The `include!`d-fragment gap shape is constructible once the schema is widened
/// (O1); red/green follows the schema, not this module.
#[test]
fn o3_widened_gap_shape_is_constructible() {
    let mut value = example();
    let index = gap_index(&value);
    value["obligation_contract"][index]["target_support_capabilities"] =
        json!(["ast", "containment", "direct_calls"]);
    assert!(
        construct(value),
        "partial ast/containment gap row must construct after widening"
    );
}

#[test]
fn o3_out_of_schema_gap_row_is_refused() {
    for shape in [
        json!(["ast", "bogus", "direct_calls"]),
        json!(["direct_calls", "ast"]),
        json!(["ast", "containment"]),
    ] {
        let mut value = example();
        let index = gap_index(&value);
        value["obligation_contract"][index]["target_support_capabilities"] = shape.clone();
        assert!(
            !schema_valid(&value),
            "precondition: {shape} is out of schema"
        );
        assert!(
            !construct(value),
            "ValidatedGenericReviewRunV4 from out-of-schema gap row {shape} must be refused"
        );
    }
}

#[test]
fn o3_out_of_schema_top_level_and_arm_mutations_are_refused() {
    let mut forged_rule = example();
    forged_rule["obligation_contract"][0]["property_id"] = json!("rust.forged@1");
    let mut unknown_field = example();
    unknown_field["forged_top_level_field"] = json!(true);
    let mut trusted = example();
    trusted["authority"]["trusted_pass"] = json!(true);
    let mut wrong_schema = example();
    wrong_schema["schema"] = json!("reviewgraphen.generic_review_run.v3");
    for (name, value) in [
        ("forged property", forged_rule),
        ("unknown top-level field", unknown_field),
        ("trusted_pass true", trusted),
        ("wrong schema id", wrong_schema),
    ] {
        assert!(
            !schema_valid(&value),
            "precondition: {name} is out of schema"
        );
        assert!(
            !construct(value),
            "ValidatedGenericReviewRunV4 from {name} must be refused"
        );
    }
}
