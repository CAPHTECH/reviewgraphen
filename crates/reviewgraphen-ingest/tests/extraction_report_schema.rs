//! Schema/canonical-example validation for `reviewgraphen.extraction_report.v1`.
//!
//! Unlike `schemas/reviewgraphen.migration.example.json` (derived from a
//! fixed, timestamp-free v1 JSON fixture), an M2 `ExtractionReport` is
//! derived from a live `git commit`, whose hash embeds a wall-clock
//! timestamp. There is no byte-exact "canonical example matches a fresh
//! run" fixture to pin here. Instead: the checked-in example is a frozen,
//! self-consistent, schema-valid instance (its own sha256 is checked
//! against its own canonical bytes, so a hand-edit cannot drift silently),
//! and a *separate* test in `tests/m2.rs`
//! (`a_real_ingest_run_extraction_report_validates_against_the_public_schema`)
//! validates a live `ingest()` run's report against this same schema.

use reviewgraphen_core::{ContentHash, canonical_json};
use serde_json::Value;

const SCHEMA: &[u8] =
    include_bytes!("../../../schemas/reviewgraphen.extraction_report.v1.schema.json");
const EXAMPLE: &[u8] =
    include_bytes!("../../../schemas/reviewgraphen.extraction_report.v1.example.json");
const EXAMPLE_HASH: &str =
    include_str!("../../../schemas/reviewgraphen.extraction_report.v1.example.sha256");

fn schema() -> Value {
    serde_json::from_slice(SCHEMA).expect("schema parses as JSON")
}

fn example() -> Value {
    serde_json::from_slice(EXAMPLE).expect("example parses as JSON")
}

#[test]
fn schema_itself_is_a_valid_json_schema() {
    jsonschema::validator_for(&schema()).expect("schema is a valid Draft 2020-12 JSON Schema");
}

#[test]
fn checked_in_example_validates_against_the_schema() {
    jsonschema::validator_for(&schema())
        .expect("schema is valid")
        .validate(&example())
        .expect("checked-in example validates against the schema");
}

#[test]
fn checked_in_example_matches_its_pinned_canonical_sha256() {
    let canonical_bytes = canonical_json(&example()).expect("canonical json");
    assert_eq!(
        ContentHash::sha256(&canonical_bytes).to_string(),
        EXAMPLE_HASH.trim(),
        "reviewgraphen.extraction_report.v1.example.sha256 must match the \
         example's own canonical bytes, so a hand-edit to the example fails \
         this check instead of drifting silently"
    );
}

#[test]
fn missing_required_field_is_rejected() {
    let mut instance = example();
    instance.as_object_mut().unwrap().remove("snapshot_id");
    assert!(
        jsonschema::validator_for(&schema())
            .unwrap()
            .validate(&instance)
            .is_err(),
        "a report missing snapshot_id must fail schema validation"
    );
}

#[test]
fn wrong_schema_discriminator_is_rejected() {
    let mut instance = example();
    instance["schema"] = Value::String("reviewgraphen.extraction_report.v2".to_owned());
    assert!(
        jsonschema::validator_for(&schema())
            .unwrap()
            .validate(&instance)
            .is_err(),
        "an unrecognized schema discriminator must fail validation, never silently degrade"
    );
}

#[test]
fn empty_obstruction_source_ids_is_rejected() {
    let mut instance = example();
    instance["obstructions"][0]["source_ids"] = serde_json::json!([]);
    assert!(
        jsonschema::validator_for(&schema())
            .unwrap()
            .validate(&instance)
            .is_err(),
        "an obstruction with empty source_ids must fail schema validation, matching \
         the non-empty source-trace contract every retained obstruction now satisfies"
    );
}

#[test]
fn unknown_top_level_field_is_rejected() {
    let mut instance = example();
    instance
        .as_object_mut()
        .unwrap()
        .insert("unexpected_field".to_owned(), Value::Bool(true));
    assert!(
        jsonschema::validator_for(&schema())
            .unwrap()
            .validate(&instance)
            .is_err(),
        "additionalProperties: false must reject an unknown top-level field"
    );
}

#[test]
fn invalid_capability_state_enum_value_is_rejected() {
    let mut instance = example();
    instance["capabilities"]["ast"] = Value::String("mostly".to_owned());
    assert!(
        jsonschema::validator_for(&schema())
            .unwrap()
            .validate(&instance)
            .is_err(),
        "a capability state outside the declared enum must fail validation"
    );
}
