use super::{AnalysisReport, analyze};
use reviewgraphen_core::ProgramSpace;
use serde_json::json;

#[test]
fn structural_sloppiness_report_deserialization_rejects_extra_top_level_fields() {
    let program = ProgramSpace::from_json_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../benchmarks/structural-sloppiness-v1/fixtures/missing-bridge-program-space.json"
    )))
    .unwrap();
    let mut value = serde_json::to_value(analyze(&program).unwrap()).unwrap();
    value["unexpected_authority"] = json!(true);
    let bytes = serde_json::to_vec(&value).unwrap();
    let error = serde_json::from_slice::<AnalysisReport>(&bytes)
        .expect_err("CLI report deserialization must reject extra fields");
    assert!(error.to_string().contains("unknown field"));
}
