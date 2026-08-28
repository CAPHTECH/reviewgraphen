use reviewgraphen_core::canonical_json;
use reviewgraphen_runtime::generic::{
    decode_and_validate_generic_review_request_v4, decode_and_validate_generic_review_run_v4_wire,
    run_generic_review_v4,
};
use serde_json::json;
use std::{fs, path::Path, process::Command};
use tempfile::tempdir;

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "ReviewGraphen Test")
        .env("GIT_AUTHOR_EMAIL", "reviewgraphen@example.invalid")
        .env("GIT_COMMITTER_NAME", "ReviewGraphen Test")
        .env("GIT_COMMITTER_EMAIL", "reviewgraphen@example.invalid")
        .output()
        .expect("git is available");
    assert!(output.status.success(), "git failed: {arguments:?}");
    String::from_utf8(output.stdout)
        .expect("git stdout UTF-8")
        .trim()
        .to_owned()
}

#[test]
fn request_policy_registry_is_exact_and_rejects_cross_family_mutation() {
    let bytes =
        include_bytes!("../../../schemas/reviewgraphen.generic_review_request.v4.example.json");
    let request = decode_and_validate_generic_review_request_v4(bytes).expect("v4 request");
    let mut forged = serde_json::to_value(request).expect("request JSON");
    forged["context_policies"]["node.public_function_contract@1"]["policy_id"] =
        json!("context.subject_windows@3");
    assert!(
        decode_and_validate_generic_review_request_v4(
            &canonical_json(&forged).expect("canonical forged request")
        )
        .is_err()
    );
}

#[test]
fn wire_rebuilds_the_closed_mixed_plan_and_coverage_example() {
    let bytes = include_bytes!("../../../schemas/reviewgraphen.generic_review_run.v4.example.json");
    let value: serde_json::Value = serde_json::from_slice(bytes).expect("example JSON");
    let canonical = canonical_json(&value).expect("canonical example");
    decode_and_validate_generic_review_run_v4_wire(&canonical).expect("closed v4 wire example");
}

#[test]
fn node_context_rejects_relation_fields_and_relation_context_rejects_node_fields() {
    let bytes = include_bytes!("../../../schemas/reviewgraphen.generic_review_run.v4.example.json");
    let mut value: serde_json::Value = serde_json::from_slice(bytes).expect("example JSON");
    value["contexts"] = json!([{
        "wave_id":"plan-wave:example",
        "context":{"obligation_id":"obligation:n-example","context_policy":{"policy_id":"context.subject_windows@4"},"caller_artifact_id":"artifact:forged"}
    }]);
    assert!(
        decode_and_validate_generic_review_run_v4_wire(
            &canonical_json(&value).expect("canonical forged run")
        )
        .is_err()
    );
}

#[test]
fn rule_property_policy_tuple_is_exact() {
    let bytes = include_bytes!("../../../schemas/reviewgraphen.generic_review_run.v4.example.json");
    let mut value: serde_json::Value = serde_json::from_slice(bytes).expect("example JSON");
    value["obligation_contract"][2]["property_id"] = json!("rust.forged@1");
    assert!(
        decode_and_validate_generic_review_run_v4_wire(
            &canonical_json(&value).expect("canonical forged run")
        )
        .is_err()
    );
}

#[test]
fn real_rust_snapshot_defers_d_capability_gap_and_plans_node_contexts() {
    let temporary = tempdir().expect("temporary repository");
    let repository = temporary.path().join("repository");
    fs::create_dir(&repository).expect("repository directory");
    git(&repository, &["init", "-q"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn changed_contract() -> u64 { 1 }\n\npub fn independent_public_function() -> u64 { 7 }\n",
    )
    .expect("base source");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "base"]);
    let base = git(&repository, &["rev-parse", "HEAD"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn changed_contract() -> u64 { 2 }\n\npub fn independent_public_function() -> u64 { 7 }\n",
    )
    .expect("target source");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "target"]);
    let target = git(&repository, &["rev-parse", "HEAD"]);

    let mut request: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../schemas/reviewgraphen.generic_review_request.v4.example.json"
    ))
    .expect("request example");
    let root = repository.display().to_string();
    request["workspace_admission_root"] = json!(root);
    request["repository_admission_root"] = json!(repository.display().to_string());
    request["repository_identity"] = json!("reviewgraphen/v4-runtime-test@1");
    request["base_revision"] = json!(base);
    request["target_revision"] = json!(target);
    request["ingest"]["max_files"] = json!(32);
    request["ingest"]["max_file_bytes"] = json!(1_048_576);
    request["ingest"]["max_total_source_bytes"] = json!(1_048_576);
    request["verifier_descriptor_id"] = serde_json::Value::Null;
    let request = decode_and_validate_generic_review_request_v4(
        &canonical_json(&request).expect("canonical request"),
    )
    .expect("closed request");

    let run = run_generic_review_v4(&request).expect("mixed production v4 run");
    let value = run.value();
    let contracts = value["obligation_contract"].as_array().expect("contracts");
    let gap = contracts
        .iter()
        .find(|contract| contract["rule_id"] == "capability_gap.origin_rule@1")
        .expect("D candidate-space gap contract");
    assert_eq!(gap["target_kind"], "subgraph");
    assert_eq!(gap["applicability_status"], "unknown");
    let deferred = value["plan"]["deferred_obligation_ids"]
        .as_array()
        .expect("deferred IDs");
    assert!(deferred.iter().any(|id| id == &gap["id"]));
    assert!(
        value["contexts"]
            .as_array()
            .expect("contexts")
            .iter()
            .all(|row| {
                row["context"]["context_policy"]["policy_id"] == "context.subject_windows@4"
            })
    );
    decode_and_validate_generic_review_run_v4_wire(&canonical_json(value).expect("canonical run"))
        .expect("closed mixed run");
}
