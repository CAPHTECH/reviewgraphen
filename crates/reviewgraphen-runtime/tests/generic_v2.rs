use reviewgraphen_core::ContentHash;
use reviewgraphen_runtime::generic::{
    GENERIC_REVIEW_REQUEST_V2_SCHEMA, GenericIngestRequestV2, GenericObserverRequestV2,
    GenericPlanRequest, GenericReviewError, GenericReviewRequestV2,
    admit_fresh_generic_review_artifact_root_v2, decode_and_validate_generic_review_run_v2,
    run_generic_review_v2, validate_generic_review_run_v2_semantics,
};
use serde_json::Value;
use std::{fs, path::Path, process::Command};
use tempfile::tempdir;

fn git(root: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "ReviewGraphen Test")
        .env("GIT_AUTHOR_EMAIL", "reviewgraphen@example.invalid")
        .env("GIT_COMMITTER_NAME", "ReviewGraphen Test")
        .env("GIT_COMMITTER_EMAIL", "reviewgraphen@example.invalid")
        .status()
        .expect("git is available for integration test");
    assert!(status.success());
}

fn request(repository: &Path) -> GenericReviewRequestV2 {
    GenericReviewRequestV2 {
        schema: GENERIC_REVIEW_REQUEST_V2_SCHEMA.to_owned(),
        workspace_admission_root: repository.parent().expect("workspace").to_path_buf(),
        repository_admission_root: repository.to_path_buf(),
        repository_identity: "generic-v2-test-repository".to_owned(),
        base_revision: "HEAD~1".to_owned(),
        target_revision: "HEAD".to_owned(),
        ingest: GenericIngestRequestV2 {
            profile_id: "rust.production.v1".to_owned(),
            profile_version: "1".to_owned(),
            rule_set_hash: ContentHash::sha256(b"generic-v2-test-rules"),
            max_files: 32,
            max_file_bytes: 4 * 1024 * 1024,
            max_total_source_bytes: 256 * 1024,
        },
        plan: GenericPlanRequest {
            max_waves: 8,
            max_obligations_per_wave: 32,
        },
        observer: GenericObserverRequestV2::DeterministicAbstain,
        verifier_descriptor_id: Some("workspace.cargo_test@1".to_owned()),
    }
}

#[test]
fn deterministic_v2_run_is_schema_valid_canonical_and_non_authority() {
    let workspace = tempdir().expect("workspace");
    let repository = workspace.path().join("repository");
    fs::create_dir(&repository).expect("repository");
    git(&repository, &["init", "-q"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn caller() -> u64 { callee() }\npub fn callee() -> u64 { 1 }\n",
    )
    .expect("base source");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "base"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn caller() -> u64 { callee() }\npub fn callee() -> u64 { 2 }\n",
    )
    .expect("target source");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "target"]);

    let request = request(&repository);
    let run = run_generic_review_v2(&request).expect("v2 run");
    let bytes = run.canonical_bytes().expect("canonical run");
    let value: Value = serde_json::from_slice(&bytes).expect("run JSON");
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_run.v2.schema.json"
    ))
    .expect("schema JSON");
    let validator = jsonschema::validator_for(&schema).expect("compiled schema");
    assert!(validator.is_valid(&value));
    validate_generic_review_run_v2_semantics(&value).expect("semantic closure");
    assert_eq!(
        decode_and_validate_generic_review_run_v2(&bytes).expect("read-only decode"),
        value
    );
    assert_eq!(value["authority"]["trusted_pass"], false);
    assert_eq!(value["coverage"]["call_graph_complete"], false);
    assert_eq!(
        value["coverage"]["global_call_coverage_claim"],
        "prohibited"
    );
    assert!(
        value["observations"]
            .as_array()
            .expect("observations")
            .iter()
            .all(|item| {
                item["kind"] == "deterministic_abstain"
                    && item["reason"] == "required_evidence_unavailable"
            })
    );
    assert_eq!(
        run.provider_free_record_artifacts().len(),
        run.observations.len()
    );
    for record in run.provider_free_record_artifacts() {
        let packet = record
            .reviewer_packet
            .canonical_bytes()
            .expect("canonical provider-free packet");
        let output = record
            .deterministic_observer_output
            .canonical_bytes()
            .expect("canonical provider-free output");
        assert_eq!(
            packet,
            record
                .reviewer_packet
                .canonical_bytes()
                .expect("repeat packet bytes")
        );
        assert!(!String::from_utf8_lossy(&packet).contains(repository.to_string_lossy().as_ref()));
        assert!(!String::from_utf8_lossy(&output).contains(repository.to_string_lossy().as_ref()));
        assert!(
            record
                .reviewer_packet_filename()
                .ends_with(".provider-free-reviewer-packet.v1.json")
        );
        assert!(
            record
                .deterministic_observer_output_filename()
                .ends_with(".deterministic-observer-output.v1.json")
        );
        assert_eq!(record.execution_filename_sha256().len(), 64);
        let packet_schema: Value = serde_json::from_str(include_str!(
            "../../../schemas/provider-free.source-grounded-packet.v1.schema.json"
        ))
        .expect("packet schema");
        assert!(
            jsonschema::validator_for(&packet_schema)
                .expect("packet schema validator")
                .is_valid(&serde_json::from_slice::<Value>(&packet).expect("packet JSON"))
        );
        let output_schema: Value = serde_json::from_str(include_str!(
            "../../../schemas/provider-free.source-grounded-abstention.v1.schema.json"
        ))
        .expect("output schema");
        assert!(
            jsonschema::validator_for(&output_schema)
                .expect("output schema validator")
                .is_valid(&serde_json::from_slice::<Value>(&output).expect("output JSON"))
        );
        let packet_value: Value = serde_json::from_slice(&packet).expect("packet JSON");
        let output_value: Value = serde_json::from_slice(&output).expect("output JSON");
        assert!(
            jsonschema::validator_for(
                packet_value
                    .get("response_schema")
                    .expect("packet response schema"),
            )
            .expect("embedded response validator")
            .is_valid(&output_value),
            "packet embeds the exact closed observer-output schema"
        );
        let inventory_schema: Value = serde_json::from_str(include_str!(
            "../../../schemas/provider-free.source-inventory.v1.schema.json"
        ))
        .expect("inventory schema");
        assert!(
            jsonschema::validator_for(&inventory_schema)
                .expect("inventory schema validator")
                .is_valid(
                    packet_value
                        .get("source_inventory")
                        .expect("packet source inventory"),
                )
        );
        let packet_text = String::from_utf8_lossy(&packet);
        for forbidden in [
            "relation.changed_public_callee@1",
            "rust.callee_contract_review@1",
            "caller_artifact_id",
            "callee_artifact_id",
            "subject_loss_ids",
        ] {
            assert!(
                !packet_text.contains(forbidden),
                "provider-free lens leak: {forbidden}"
            );
        }
    }

    let replay_file = workspace.path().join("deterministic-observation.json");
    fs::write(
        &replay_file,
        reviewgraphen_core::canonical_json(&run.observations[0]).expect("canonical observation"),
    )
    .expect("replay record");
    let mut replay_request = request.clone();
    replay_request.observer = GenericObserverRequestV2::Replay {
        records: vec![replay_file.clone()],
    };
    let replay = run_generic_review_v2(&replay_request).expect("exact replay");
    assert_eq!(
        run.observations, replay.observations,
        "replay accepts only the exact canonical deterministic observation"
    );
    assert_eq!(
        bytes,
        run_generic_review_v2(&request)
            .expect("second deterministic run")
            .canonical_bytes()
            .expect("second deterministic canonical run"),
        "same deterministic request rebuilds byte-identically"
    );
    let mut mutated_record: Value =
        serde_json::from_slice(&fs::read(&replay_file).expect("replay bytes"))
            .expect("replay JSON");
    mutated_record["raw_response_hash"] =
        "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".into();
    fs::write(
        &replay_file,
        reviewgraphen_core::canonical_json(&mutated_record).expect("mutated canonical record"),
    )
    .expect("mutated replay record");
    assert!(run_generic_review_v2(&replay_request).is_err());

    let mut forged = value.clone();
    forged["authority"]["trusted_pass"] = true.into();
    assert!(validate_generic_review_run_v2_semantics(&forged).is_err());
    let mut forged = value;
    forged["coverage"]["enumeration_obstruction_ids"] = serde_json::json!([]);
    assert!(validate_generic_review_run_v2_semantics(&forged).is_err());

    let mut missing = serde_json::from_slice::<Value>(&bytes).expect("run JSON");
    missing
        .as_object_mut()
        .expect("run object")
        .remove("coverage");
    assert!(!validator.is_valid(&missing));
    let mut extra = serde_json::from_slice::<Value>(&bytes).expect("run JSON");
    extra["accepted_claims"] = serde_json::json!([]);
    assert!(!validator.is_valid(&extra));
    let mut wrong_major = serde_json::from_slice::<Value>(&bytes).expect("run JSON");
    wrong_major["schema"] = "reviewgraphen.generic_review_run.v1".into();
    assert!(!validator.is_valid(&wrong_major));
}

#[test]
fn v2_schemas_reject_missing_extra_and_wrong_major_fields() {
    let root = tempdir().expect("root");
    let request = request(root.path());
    let mut value = serde_json::to_value(&request).expect("request JSON");
    let request_schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_request.v2.schema.json"
    ))
    .expect("request schema JSON");
    let validator = jsonschema::validator_for(&request_schema).expect("compiled request schema");
    assert!(validator.is_valid(&value));
    assert_eq!(value["ingest"]["max_file_bytes"], 4 * 1024 * 1024);
    let mut beyond_file_limit = value.clone();
    beyond_file_limit["ingest"]["max_file_bytes"] = (4 * 1024 * 1024 + 1).into();
    assert!(!validator.is_valid(&beyond_file_limit));
    value.as_object_mut().expect("object").remove("observer");
    assert!(!validator.is_valid(&value));
    let mut value = serde_json::to_value(&request).expect("request JSON");
    value["unexpected"] = true.into();
    assert!(!validator.is_valid(&value));
    let mut value = serde_json::to_value(&request).expect("request JSON");
    value["schema"] = "reviewgraphen.generic_review_request.v1".into();
    assert!(!validator.is_valid(&value));
    let legacy_schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_request.v1.schema.json"
    ))
    .expect("legacy request schema JSON");
    assert!(
        !jsonschema::validator_for(&legacy_schema)
            .expect("compiled legacy request schema")
            .is_valid(&serde_json::to_value(&request).expect("v2 request JSON"))
    );
}

#[test]
fn v2_schema_examples_are_valid_and_run_example_has_read_only_closure() {
    let request_schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_request.v2.schema.json"
    ))
    .expect("request schema");
    let request_example: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_request.v2.example.json"
    ))
    .expect("request example");
    assert!(
        jsonschema::validator_for(&request_schema)
            .expect("request validator")
            .is_valid(&request_example)
    );

    let run_schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_run.v2.schema.json"
    ))
    .expect("run schema");
    let run_example: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_run.v2.example.json"
    ))
    .expect("run example");
    assert!(
        jsonschema::validator_for(&run_schema)
            .expect("run validator")
            .is_valid(&run_example)
    );
    validate_generic_review_run_v2_semantics(&run_example).expect("read-only run closure");

    for (schema, example) in [
        (
            include_str!("../../../schemas/provider-free.source-grounded-packet.v1.schema.json"),
            include_str!("../../../schemas/provider-free.source-grounded-packet.v1.example.json"),
        ),
        (
            include_str!(
                "../../../schemas/provider-free.source-grounded-abstention.v1.schema.json"
            ),
            include_str!(
                "../../../schemas/provider-free.source-grounded-abstention.v1.example.json"
            ),
        ),
        (
            include_str!("../../../schemas/provider-free.source-inventory.v1.schema.json"),
            include_str!("../../../schemas/provider-free.source-inventory.v1.example.json"),
        ),
    ] {
        let schema: Value = serde_json::from_str(schema).expect("provider-free schema");
        let example: Value = serde_json::from_str(example).expect("provider-free example");
        assert!(
            jsonschema::validator_for(&schema)
                .expect("provider-free validator")
                .is_valid(&example),
            "provider-free schema example is valid"
        );
    }
}

#[test]
fn every_observation_variant_is_a_valid_non_authority_audit_row() {
    let workspace = tempdir().expect("workspace");
    let repository = workspace.path().join("repository");
    fs::create_dir(&repository).expect("repository");
    git(&repository, &["init", "-q"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn caller() -> u64 { callee() }\npub fn callee() -> u64 { 1 }\n",
    )
    .expect("base source");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "base"]);
    fs::write(
        repository.join("lib.rs"),
        "pub fn caller() -> u64 { callee() }\npub fn callee() -> u64 { 2 }\n",
    )
    .expect("target source");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "target"]);
    let run = run_generic_review_v2(&request(&repository)).expect("v2 run");
    let base = serde_json::to_value(run).expect("run JSON");
    let schema: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_run.v2.schema.json"
    ))
    .expect("run schema");
    let validator = jsonschema::validator_for(&schema).expect("run validator");

    for kind in ["proposed_claim", "malformed", "provider_failure"] {
        let mut value = base.clone();
        value["provider_free_packet_bindings"] = serde_json::json!([]);
        let row = value["observations"][0]
            .as_object_mut()
            .expect("observation");
        let obligation_id = row["obligation_id"].clone();
        let execution_id = row["execution_id"].clone();
        let input_manifest_hash = row["input_manifest_hash"].clone();
        let replacement = match kind {
            "proposed_claim" => serde_json::json!({
                "kind": kind,
                "schema": "reviewgraphen.generic_observation.v2",
                "observer_id": "test-observer@1",
                "obligation_id": obligation_id,
                "execution_id": execution_id,
                "input_manifest_hash": input_manifest_hash,
                "raw_response": "{}",
                "raw_response_hash": ContentHash::sha256(b"{}").to_string(),
                "proposals": [{
                    "property_id": "rust.callee_contract_review@1",
                    "target_refs": ["relation:synthetic"],
                    "polarity": "issue_present",
                    "summary": "A proposed source-grounded concern.",
                    "source_ids": ["file:synthetic"],
                    "assumptions": [],
                    "requested_evidence": [],
                    "candidate_confidence": 0.5,
                    "disposition": "proposed",
                    "author_kind": "ai",
                    "review_status": "unreviewed"
                }]
            }),
            "malformed" => serde_json::json!({
                "kind": kind,
                "schema": "reviewgraphen.generic_observation.v2",
                "observer_id": "test-observer@1",
                "obligation_id": obligation_id,
                "execution_id": execution_id,
                "input_manifest_hash": input_manifest_hash,
                "raw_response": "{}",
                "raw_response_hash": ContentHash::sha256(b"{}").to_string(),
                "reason": "schema_violation",
                "diagnostic": "The observer output did not satisfy the closed response schema."
            }),
            "provider_failure" => serde_json::json!({
                "kind": kind,
                "schema": "reviewgraphen.generic_observation.v2",
                "observer_id": "test-observer@1",
                "obligation_id": obligation_id,
                "execution_id": execution_id,
                "input_manifest_hash": input_manifest_hash,
                "raw_response": "{}",
                "raw_response_hash": ContentHash::sha256(b"{}").to_string(),
                "retryable": true,
                "diagnostic": "The observer transport failed before a response was parsed."
            }),
            _ => unreachable!(),
        };
        *row = replacement.as_object().expect("replacement object").clone();
        value["coverage"]["abstained_obligation_ids"] = serde_json::json!([]);
        value["coverage"]["structured_obligation_ids"] = if kind == "proposed_claim" {
            serde_json::json!([value["coverage"]["executed_obligation_ids"][0].clone()])
        } else {
            serde_json::json!([])
        };
        value["coverage"]["malformed_obligation_ids"] = if kind == "malformed" {
            serde_json::json!([value["coverage"]["executed_obligation_ids"][0].clone()])
        } else {
            serde_json::json!([])
        };
        value["coverage"]["provider_failed_obligation_ids"] = if kind == "provider_failure" {
            serde_json::json!([value["coverage"]["executed_obligation_ids"][0].clone()])
        } else {
            serde_json::json!([])
        };
        assert!(validator.is_valid(&value), "{kind} schema validation");
        validate_generic_review_run_v2_semantics(&value).expect("non-authority semantic closure");
        assert_eq!(value["authority"]["trusted_pass"], false);
        assert!(value.get("accepted_claims").is_none());
        assert!(value.get("evidence").is_none());
        assert!(value.get("verification").is_none());
        assert!(value.get("decision").is_none());
        assert!(value.get("finding").is_none());
    }
}

#[test]
fn logically_identical_clones_at_different_paths_emit_identical_audit_bytes() {
    let root = tempdir().expect("root");
    let source = root.path().join("source");
    fs::create_dir(&source).expect("source repository");
    git(&source, &["init", "-q"]);
    fs::write(
        source.join("lib.rs"),
        "pub fn caller() -> u64 { callee() }\npub fn callee() -> u64 { 1 }\n",
    )
    .expect("base source");
    git(&source, &["add", "lib.rs"]);
    git(&source, &["commit", "-q", "-m", "base"]);
    fs::write(
        source.join("lib.rs"),
        "pub fn caller() -> u64 { callee() }\npub fn callee() -> u64 { 2 }\n",
    )
    .expect("target source");
    git(&source, &["add", "lib.rs"]);
    git(&source, &["commit", "-q", "-m", "target"]);

    let first = root.path().join("clone-at-first-physical-path");
    let second = root.path().join("clone-at-second-physical-path");
    for clone in [&first, &second] {
        let status = Command::new("git")
            .args(["clone", "-q", "--no-local"])
            .arg(&source)
            .arg(clone)
            .status()
            .expect("clone command");
        assert!(status.success());
    }
    let mut first_request = request(&first);
    let mut second_request = request(&second);
    first_request.repository_identity = "logical/example-repository".to_owned();
    second_request.repository_identity = "logical/example-repository".to_owned();
    let first_run = run_generic_review_v2(&first_request).expect("first clone run");
    let second_run = run_generic_review_v2(&second_request).expect("second clone run");
    let first_bytes = first_run.canonical_bytes().expect("first bytes");
    let second_bytes = second_run.canonical_bytes().expect("second bytes");
    assert_eq!(first_bytes, second_bytes);
    assert_eq!(
        first_run.provider_free_record_artifacts().len(),
        second_run.provider_free_record_artifacts().len()
    );
    for (first_record, second_record) in first_run
        .provider_free_record_artifacts()
        .iter()
        .zip(second_run.provider_free_record_artifacts())
    {
        assert_eq!(
            first_record
                .reviewer_packet
                .canonical_bytes()
                .expect("first packet bytes"),
            second_record
                .reviewer_packet
                .canonical_bytes()
                .expect("second packet bytes")
        );
        assert_eq!(
            first_record
                .deterministic_observer_output
                .canonical_bytes()
                .expect("first output bytes"),
            second_record
                .deterministic_observer_output
                .canonical_bytes()
                .expect("second output bytes")
        );
    }
}

#[test]
fn provider_free_artifact_root_admission_is_fresh_and_symlink_safe() {
    let root = tempdir().expect("root");
    let fresh = root.path().join(".reviewgraphen-quickstart-output");
    admit_fresh_generic_review_artifact_root_v2(&fresh).expect("fresh root admitted");
    assert!(fresh.is_dir());
    assert!(matches!(
        admit_fresh_generic_review_artifact_root_v2(&fresh),
        Err(GenericReviewError::ArtifactRootAlreadyExists)
    ));
    let existing_file = root.path().join("existing-output-file");
    fs::write(&existing_file, "not a directory").expect("existing output file");
    assert!(matches!(
        admit_fresh_generic_review_artifact_root_v2(&existing_file),
        Err(GenericReviewError::ArtifactRootAlreadyExists)
    ));
    let traversal = root.path().join("nested").join("..").join("other-output");
    assert!(matches!(
        admit_fresh_generic_review_artifact_root_v2(&traversal),
        Err(GenericReviewError::ArtifactRootRejected("path traversal"))
    ));
    #[cfg(unix)]
    {
        let linked_parent = root.path().join("linked-parent");
        std::os::unix::fs::symlink(root.path(), &linked_parent).expect("parent symlink");
        assert!(matches!(
            admit_fresh_generic_review_artifact_root_v2(&linked_parent.join("output")),
            Err(GenericReviewError::ArtifactRootRejected("symlink parent"))
        ));
        let symlink = root.path().join("output-symlink");
        std::os::unix::fs::symlink(root.path(), &symlink).expect("output symlink");
        assert!(matches!(
            admit_fresh_generic_review_artifact_root_v2(&symlink),
            Err(GenericReviewError::ArtifactRootAlreadyExists)
        ));
    }
}
