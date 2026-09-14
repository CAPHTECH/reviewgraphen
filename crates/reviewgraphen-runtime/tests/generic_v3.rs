use reviewgraphen_core::{ContentHash, ContextBuildEffect, ContextBuildTrace, canonical_json};
use reviewgraphen_runtime::generic::{
    GENERIC_REVIEW_REQUEST_V2_SCHEMA, GENERIC_REVIEW_REQUEST_V3_SCHEMA,
    GENERIC_REVIEW_RUN_V2_SCHEMA, GENERIC_REVIEW_RUN_V3_SCHEMA, GenericIngestRequestV2,
    GenericObserverRequestV2, GenericPlanRequest, GenericReviewBasisLifecycleEvent,
    GenericReviewBasisLifecycleTrace, GenericReviewRequestV2, GenericReviewRequestV3,
    decode_and_validate_generic_review_request_v2, decode_and_validate_generic_review_request_v3,
    decode_and_validate_generic_review_run_v2, decode_and_validate_generic_review_run_v3,
    run_generic_review_v2, run_generic_review_v3, run_generic_review_v3_with_probe,
    run_generic_review_v3_with_probes, validate_generic_review_run_v3_wire_structure,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
    sync::Arc,
};
use tempfile::tempdir;

const POSITIVE_BASE: &str = "a8b6b24d5ed704f53f721b25db42d5d631f946c7";
const POSITIVE_TARGET: &str = "8569a2261e8a62145228872a2fde9f4c48093d00";
const V3_POLICY: &str = "context.subject_windows@3";

fn git(root: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .env("GIT_AUTHOR_NAME", "ReviewGraphen Test")
        .env("GIT_AUTHOR_EMAIL", "reviewgraphen@example.invalid")
        .env("GIT_COMMITTER_NAME", "ReviewGraphen Test")
        .env("GIT_COMMITTER_EMAIL", "reviewgraphen@example.invalid")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .status()
        .expect("git is available for integration test");
    assert!(status.success());
}

#[test]
fn checked_in_real_subject_fixtures_match_literal_upstream_hashes() {
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let manifest: Value = serde_json::from_slice(
        &fs::read(fixture_root.join("real-subject-pairs.v1.json")).expect("fixture manifest"),
    )
    .expect("fixture manifest JSON");
    assert_eq!(
        manifest["schema"],
        "reviewgraphen.runtime.real_subject_fixture_manifest.v1"
    );
    let pairs = manifest["pairs"].as_array().expect("fixture pairs");
    assert_eq!(pairs.len(), 5);
    assert_eq!(
        pairs
            .iter()
            .filter(|pair| pair["repository"] == "fsl")
            .count(),
        3
    );
    assert_eq!(
        pairs
            .iter()
            .filter(|pair| pair["repository"] == "casegraphen")
            .count(),
        2
    );
    for pair in pairs {
        for field in ["base_commit", "target_commit"] {
            let oid = pair[field].as_str().expect("literal commit OID");
            assert_eq!(oid.len(), 40);
            assert!(oid.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
        let target_fixture = pair["target_fixture"].as_str().expect("target fixture");
        let target_bytes =
            fs::read(fixture_root.join(target_fixture)).expect("target fixture bytes");
        assert_eq!(
            ContentHash::sha256(&target_bytes).as_str(),
            pair["target_sha256"].as_str().expect("literal target hash")
        );
        match (pair["base_fixture"].as_str(), pair["base_sha256"].as_str()) {
            (Some(base_fixture), Some(base_sha256)) => {
                let base_bytes =
                    fs::read(fixture_root.join(base_fixture)).expect("base fixture bytes");
                assert_eq!(ContentHash::sha256(&base_bytes).as_str(), base_sha256);
            }
            (None, None) => {}
            _ => panic!("base fixture/hash presence mismatch"),
        }
        assert!(
            pair["target_path"]
                .as_str()
                .is_some_and(|path| path.ends_with(".rs"))
        );
        assert!(
            pair["clause_7_count"]
                .as_u64()
                .is_some_and(|count| count > 0)
        );

        let workspace = tempdir().expect("fixture workspace");
        let repository = workspace.path().join("repository");
        fs::create_dir(&repository).expect("fixture repository");
        git(&repository, &["init", "-q"]);
        let target_path = pair["target_path"].as_str().expect("target path");
        let repository_path = repository.join(target_path);
        if let Some(parent) = repository_path.parent() {
            fs::create_dir_all(parent).expect("fixture source parent");
        }
        if let Some(base_fixture) = pair["base_fixture"].as_str() {
            fs::write(
                &repository_path,
                fs::read(fixture_root.join(base_fixture)).expect("base fixture bytes"),
            )
            .expect("base fixture source");
            git(&repository, &["add", target_path]);
            git(&repository, &["commit", "-q", "-m", "fixture base"]);
        } else {
            git(
                &repository,
                &["commit", "-q", "--allow-empty", "-m", "fixture base"],
            );
        }
        fs::write(&repository_path, &target_bytes).expect("target fixture source");
        git(&repository, &["add", target_path]);
        git(&repository, &["commit", "-q", "-m", "fixture target"]);

        let expected_clause_7 = pair["clause_7_count"].as_u64().unwrap() as usize;
        let probe = Arc::new(ContextBuildTrace::default());
        let basis_probe = Arc::new(GenericReviewBasisLifecycleTrace::default());
        let run = run_generic_review_v3_with_probes(
            &v3_request(&repository),
            Some(probe.clone()),
            Some(basis_probe.clone()),
        )
        .unwrap_or_else(|error| {
            panic!(
                "{} fixture runtime failed: {error}",
                pair["repository"].as_str().unwrap()
            )
        });
        if pair["target_commit"] == "076fad556730314636140611e559281d68f863cd" {
            let contexts = run.value()["contexts"]
                .as_array()
                .expect("recursive real-corpus contexts");
            assert_eq!(contexts.len(), 3);
            for envelope in contexts {
                let context = &envelope["context"];
                assert_eq!(
                    context["caller_artifact_id"], context["callee_artifact_id"],
                    "the real cluster contains a valid self-recursive call relation"
                );
                let outcomes = context["subject_outcomes"]
                    .as_array()
                    .expect("recursive subject outcomes");
                assert_eq!(outcomes.len(), 2);
                assert_eq!(outcomes[0]["role"], "callee");
                assert_eq!(outcomes[1]["role"], "caller");
                assert_eq!(outcomes[0]["endpoint_id"], outcomes[1]["endpoint_id"]);
                assert_eq!(outcomes[0]["state"], "admitted");
                assert_eq!(outcomes[1]["state"], "admitted");
            }
        }
        let basis_events = basis_probe.snapshot();
        assert!(matches!(
            basis_events.first(),
            Some(GenericReviewBasisLifecycleEvent::SharedInputsCreated {
                aggregate_debug_length: 35,
                source_map_debug_length: 35,
                ..
            })
        ));
        assert_eq!(basis_events.len(), 2 + expected_clause_7 * 2);
        assert!(matches!(
            basis_events.last(),
            Some(GenericReviewBasisLifecycleEvent::InputCloneCounts {
                aggregate_clones: 0,
                source_map_clones: 0,
            })
        ));
        let shared_identities = match &basis_events[0] {
            GenericReviewBasisLifecycleEvent::SharedInputsCreated {
                aggregate_identity,
                source_map_identity,
                ..
            } => (*aggregate_identity, *source_map_identity),
            _ => unreachable!(),
        };
        for events in basis_events[1..basis_events.len() - 1].chunks_exact(2) {
            assert!(matches!(
                events,
                [
                    GenericReviewBasisLifecycleEvent::BasisCreated {
                        aggregate_identity,
                        source_map_identity,
                        aggregate_strong_count: 2,
                        source_map_strong_count: 2,
                    },
                    GenericReviewBasisLifecycleEvent::BasisDropped {
                        aggregate_identity: dropped_aggregate,
                        source_map_identity: dropped_source_map,
                        aggregate_strong_count: 1,
                        source_map_strong_count: 1,
                    }
                ] if (*aggregate_identity, *source_map_identity) == shared_identities
                    && (*dropped_aggregate, *dropped_source_map) == shared_identities
            ));
        }
        let observed_clause_7 = run.value()["obligation_contract"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|obligation| {
                obligation["rule_id"] == "relation.changed_public_callee@1"
                    && obligation["property_id"] == "rust.callee_contract_review@1"
            })
            .count();
        assert_eq!(observed_clause_7, expected_clause_7);
        assert_eq!(
            run.value()["contexts"].as_array().unwrap().len(),
            expected_clause_7,
            "every literal clause-7 obligation must complete context construction"
        );

        let run_value = serde_json::to_value(&run).expect("fixture run JSON");
        let mut expected_terminal = BTreeMap::<(String, bool), usize>::new();
        let mut admitted_sources = BTreeMap::<String, usize>::new();
        for context in run_value["contexts"].as_array().expect("fixture contexts") {
            let outcomes = context["context"]["subject_outcomes"]
                .as_array()
                .expect("subject outcomes");
            assert_eq!(outcomes.len(), 2, "one caller and one callee outcome");
            assert_eq!(
                outcomes
                    .iter()
                    .map(|outcome| outcome["role"].as_str().unwrap())
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from(["callee", "caller"])
            );
            for outcome in outcomes {
                let admitted = outcome["state"] == "admitted";
                let endpoint = if admitted {
                    *admitted_sources
                        .entry(outcome["source_artifact_id"].as_str().unwrap().to_owned())
                        .or_default() += 1;
                    outcome["endpoint_id"].as_str().unwrap()
                } else {
                    assert_eq!(outcome["loss"]["severity"], "high");
                    outcome["loss"]["endpoint_id"].as_str().unwrap()
                };
                *expected_terminal
                    .entry((endpoint.to_owned(), admitted))
                    .or_default() += 1;
            }
        }

        let trace = probe.snapshot();
        let mut observed_terminal = BTreeMap::<(String, bool), usize>::new();
        let mut submitted_sources = BTreeMap::<String, usize>::new();
        for effect in trace {
            match effect {
                ContextBuildEffect::SubjectOutcome {
                    endpoint_id,
                    submitted,
                } => {
                    *observed_terminal
                        .entry((endpoint_id.to_string(), submitted))
                        .or_default() += 1;
                }
                ContextBuildEffect::SourceSubmitted { artifact_id } => {
                    *submitted_sources
                        .entry(artifact_id.to_string())
                        .or_default() += 1;
                }
                _ => {}
            }
        }
        assert_eq!(observed_terminal, expected_terminal);
        assert_eq!(
            observed_terminal.values().sum::<usize>(),
            expected_clause_7 * 2
        );
        for source in admitted_sources.keys() {
            assert!(
                submitted_sources
                    .get(source)
                    .is_some_and(|count| *count > 0),
                "admitted subject source must have a source-submit effect"
            );
        }
    }
}

#[test]
fn checked_in_real_profile_exclusion_fixture_reaches_a_valid_empty_target_denominator() {
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let manifest: Value = serde_json::from_slice(
        &fs::read(fixture_root.join("real-profile-exclusions.v1.json"))
            .expect("profile exclusion fixture manifest"),
    )
    .expect("profile exclusion fixture manifest JSON");
    assert_eq!(
        manifest["schema"],
        "reviewgraphen.runtime.real_profile_exclusion_fixture_manifest.v1"
    );
    let pair = &manifest["pairs"][0];
    for (fixture_field, hash_field) in [
        ("base_fixture", "base_sha256"),
        ("target_fixture", "target_sha256"),
    ] {
        let bytes =
            fs::read(fixture_root.join(pair[fixture_field].as_str().expect("fixture path")))
                .expect("profile exclusion fixture bytes");
        assert_eq!(
            ContentHash::sha256(&bytes).as_str(),
            pair[hash_field].as_str().expect("fixture hash")
        );
    }

    let workspace = tempdir().expect("profile exclusion fixture workspace");
    let repository = workspace.path().join("repository");
    fs::create_dir(&repository).expect("profile exclusion fixture repository");
    git(&repository, &["init", "-q"]);
    let target_path = pair["target_path"].as_str().expect("target path");
    let repository_path = repository.join(target_path);
    fs::create_dir_all(repository_path.parent().expect("target parent"))
        .expect("target parent directories");
    fs::write(
        &repository_path,
        fs::read(fixture_root.join(pair["base_fixture"].as_str().unwrap())).expect("base fixture"),
    )
    .expect("base source");
    git(&repository, &["add", target_path]);
    git(&repository, &["commit", "-q", "-m", "fixture base"]);
    fs::write(
        &repository_path,
        fs::read(fixture_root.join(pair["target_fixture"].as_str().unwrap()))
            .expect("target fixture"),
    )
    .expect("target source");
    git(&repository, &["add", target_path]);
    git(&repository, &["commit", "-q", "-m", "fixture target"]);

    let run = run_generic_review_v3(&v3_request(&repository))
        .expect("real profile exclusion must be a valid universe qualification");
    assert_eq!(
        run.value()["coverage"]["resolved_target_obligation_ids"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(run.value()["contexts"].as_array().map(Vec::len), Some(0));
}

fn v2_request(repository: &Path) -> GenericReviewRequestV2 {
    GenericReviewRequestV2 {
        schema: GENERIC_REVIEW_REQUEST_V2_SCHEMA.to_owned(),
        workspace_admission_root: repository.parent().expect("workspace").to_path_buf(),
        repository_admission_root: repository.to_path_buf(),
        repository_identity: "generic-v3-test-repository".to_owned(),
        base_revision: "HEAD~1".to_owned(),
        target_revision: "HEAD".to_owned(),
        ingest: GenericIngestRequestV2 {
            profile_id: "rust.production.v1".to_owned(),
            profile_version: "1".to_owned(),
            rule_set_hash: ContentHash::sha256(b"generic-v3-test-rules"),
            max_files: 32,
            max_file_bytes: 4 * 1024 * 1024,
            max_total_source_bytes: 256 * 1024,
        },
        plan: GenericPlanRequest {
            max_waves: 8,
            max_obligations_per_wave: 32,
        },
        observer: GenericObserverRequestV2::DeterministicAbstain,
        verifier_descriptor_id: None,
    }
}

fn v3_request(repository: &Path) -> GenericReviewRequestV3 {
    let v2 = v2_request(repository);
    GenericReviewRequestV3 {
        schema: GENERIC_REVIEW_REQUEST_V3_SCHEMA.to_owned(),
        workspace_admission_root: v2.workspace_admission_root,
        repository_admission_root: v2.repository_admission_root,
        repository_identity: v2.repository_identity,
        base_revision: v2.base_revision,
        target_revision: v2.target_revision,
        ingest: v2.ingest,
        plan: v2.plan,
        observer: v2.observer,
        verifier_descriptor_id: v2.verifier_descriptor_id,
        context_policy_id: V3_POLICY.to_owned(),
    }
}

fn changed_public_callee_repository() -> tempfile::TempDir {
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
    workspace
}

fn support_loss_repository() -> tempfile::TempDir {
    let workspace = tempdir().expect("workspace");
    let repository = workspace.path().join("repository");
    fs::create_dir(&repository).expect("repository");
    git(&repository, &["init", "-q"]);
    let callers = (0..12)
        .map(|index| {
            format!(
                "pub fn caller_{index}() -> u64 {{\n{}    callee()\n}}\n",
                "    // support-window-cap\n".repeat(45)
            )
        })
        .collect::<String>();
    fs::write(
        repository.join("lib.rs"),
        format!("pub fn callee() -> u64 {{ 1 }}\n{callers}"),
    )
    .expect("base source");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "base"]);
    fs::write(
        repository.join("lib.rs"),
        format!("pub fn callee() -> u64 {{ 2 }}\n{callers}"),
    )
    .expect("target source");
    git(&repository, &["add", "lib.rs"]);
    git(&repository, &["commit", "-q", "-m", "target"]);
    workspace
}

fn schema(path: &str) -> Value {
    serde_json::from_str(path).expect("schema JSON")
}

#[test]
fn v2_request_cannot_select_v3_and_only_emits_v2() {
    let workspace = changed_public_callee_repository();
    let repository = workspace.path().join("repository");
    let request = v2_request(&repository);
    let mut forged = serde_json::to_value(&request).expect("v2 request JSON");
    forged["context_policy_id"] = json!(V3_POLICY);
    assert!(
        decode_and_validate_generic_review_request_v2(
            &canonical_json(&forged).expect("canonical forged v2 request")
        )
        .is_err()
    );

    let run = run_generic_review_v2(&request).expect("v2 run");
    assert_eq!(run.schema, GENERIC_REVIEW_RUN_V2_SCHEMA);
    assert_ne!(run.schema, GENERIC_REVIEW_RUN_V3_SCHEMA);
}

#[test]
fn v3_request_boundary_typed_rejects_every_non_fixed_selector_form() {
    let workspace = changed_public_callee_repository();
    let request = v3_request(&workspace.path().join("repository"));
    let good = serde_json::to_value(&request).expect("v3 request JSON");
    let request_schema = schema(include_str!(
        "../../../schemas/reviewgraphen.generic_review_request.v3.schema.json"
    ));
    let validator = jsonschema::validator_for(&request_schema).expect("v3 request schema");
    assert!(validator.is_valid(&good));

    let variants = [
        ("missing", {
            let mut value = good.clone();
            value
                .as_object_mut()
                .expect("object")
                .remove("context_policy_id");
            value
        }),
        ("other", {
            let mut value = good.clone();
            value["context_policy_id"] = json!("context.subject_windows@4");
            value
        }),
        ("inline parameters", {
            let mut value = good.clone();
            value["context_policy_parameters"] = json!({"included_files": 65});
            value
        }),
        ("inline hash", {
            let mut value = good.clone();
            value["context_policy_hash"] =
                json!("sha256:932bfa18c5d286c63196366d6d2dc1aaf402f50baa1f1ab5f075b1007be55dd8");
            value
        }),
        ("v2 schema", {
            let mut value = good.clone();
            value["schema"] = json!(GENERIC_REVIEW_REQUEST_V2_SCHEMA);
            value
        }),
    ];
    for (name, invalid) in variants {
        assert!(!validator.is_valid(&invalid), "schema accepts {name}");
        assert!(
            decode_and_validate_generic_review_request_v3(
                &canonical_json(&invalid).expect("canonical invalid v3 request")
            )
            .is_err(),
            "typed v3 decoder accepts {name}"
        );
    }
}

#[test]
fn v3_run_schema_example_is_real_and_has_read_only_closure() {
    let run_schema = schema(include_str!(
        "../../../schemas/reviewgraphen.generic_review_run.v3.schema.json"
    ));
    let example: Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.generic_review_run.v3.example.json"
    ))
    .expect("v3 run example JSON");
    assert!(
        jsonschema::validator_for(&run_schema)
            .expect("v3 run schema")
            .is_valid(&example)
    );
    validate_generic_review_run_v3_wire_structure(&example).expect("v3 example wire closure");

    let mutations = [
        ("unknown key", {
            let mut value = example.clone();
            value["harmless_extra"] = json!(0);
            value
        }),
        ("missing required", {
            let mut value = example.clone();
            value.as_object_mut().unwrap().remove("authority");
            value
        }),
        ("wrong type", {
            let mut value = example.clone();
            value["contexts"] = json!({});
            value
        }),
    ];
    for (name, mutation) in mutations {
        assert!(
            matches!(
                decode_and_validate_generic_review_run_v3(
                    &canonical_json(&mutation).expect("canonical schema mutation")
                ),
                Err(reviewgraphen_runtime::generic::GenericReviewError::Request(
                    "v3 run schema validation"
                ))
            ),
            "typed decoder accepts {name}"
        );
    }
}

#[test]
fn v2_and_v3_runs_are_cross_decode_incompatible_and_preserve_v2_domains() {
    let runtime_source = include_str!("../src/generic.rs");
    assert!(
        runtime_source.contains("ContextValidationBasisV3::from_accepted_snapshot(")
            && runtime_source.contains("let built = session.finish()?")
            && runtime_source.contains(
                "validate_subject_windows_v3_against_basis(&context_value, &validation_basis)?"
            ),
        "the live integration route must retain and consume its trusted validation basis"
    );
    let workspace = changed_public_callee_repository();
    let repository = workspace.path().join("repository");
    let v2 = run_generic_review_v2(&v2_request(&repository)).expect("v2 run");
    let v3 = run_generic_review_v3(&v3_request(&repository)).expect("v3 run");
    let v2_bytes = v2.canonical_bytes().expect("v2 canonical bytes");
    let v3_bytes = v3.canonical_bytes().expect("v3 canonical bytes");
    assert!(decode_and_validate_generic_review_run_v2(&v3_bytes).is_err());
    assert!(decode_and_validate_generic_review_run_v3(&v2_bytes).is_err());
    assert_eq!(
        ContentHash::sha256(&v2_bytes).as_str(),
        "sha256:c148c90da6cd080a601e7129f4179d8cf5ed3dd0c33da786313a8ed4897a938c",
        "independent canonical v2 run golden"
    );
    assert_eq!(
        ContentHash::sha256(&v3_bytes).as_str(),
        "sha256:5d9f9e56879c1705900c2db2579ab666bb87d0999879a76d1f6bcb7ae9d4e884",
        "basis-bound live validation must preserve the independent canonical v3 run golden"
    );

    let v2_value: Value = serde_json::from_slice(&v2_bytes).expect("v2 JSON");
    let v3_value: Value = serde_json::from_slice(&v3_bytes).expect("v3 JSON");
    for field in [
        "legacy_ingestion",
        "ingestion_report_v2",
        "obligation_contract",
        "plan",
        "coverage",
        "verifier",
        "authority",
    ] {
        assert_eq!(
            canonical_json(&v2_value[field]).expect("canonical v2 inherited domain"),
            canonical_json(&v3_value[field]).expect("canonical v3 inherited domain"),
            "ADR 0038 §8.5 requires v2 domain `{field}` to remain byte-identical"
        );
    }

    let v3_schema = schema(include_str!(
        "../../../schemas/reviewgraphen.generic_review_run.v3.schema.json"
    ));
    assert!(
        jsonschema::validator_for(&v3_schema)
            .expect("v3 run schema")
            .is_valid(&v3_value)
    );
}

#[test]
fn v3_semantic_validator_rejects_each_rebuilt_context_commitment_tamper() {
    let workspace = support_loss_repository();
    let run = run_generic_review_v3(&v3_request(&workspace.path().join("repository")))
        .expect("support-loss v3 run");
    let value = serde_json::to_value(run).expect("v3 run JSON");
    validate_generic_review_run_v3_wire_structure(&value).expect("v3 wire closure");

    let context = &value["contexts"][0]["context"];
    assert_eq!(
        context["subject_outcomes"].as_array().map(Vec::len),
        Some(2)
    );
    assert!(
        context["windows"]
            .as_array()
            .expect("windows")
            .iter()
            .any(|window| !window["support_anchor_ids"]
                .as_array()
                .expect("anchors")
                .is_empty())
    );
    assert!(
        !context["support_loss_summaries"]
            .as_array()
            .expect("support summaries")
            .is_empty()
    );

    for denominator in [
        "accepted_file_denominator",
        "reached_file_denominator",
        "materialized_source_denominator",
        "support_anchor_denominator",
    ] {
        let mut forged = value.clone();
        forged["contexts"][0]["context"][denominator]["observed_count"] = json!(0_u64);
        assert!(
            validate_generic_review_run_v3_wire_structure(&forged).is_err(),
            "semantic validator accepts tampered {denominator}"
        );
    }

    let mutations = [
        ("latent cardinality", {
            let mut forged = value.clone();
            forged["contexts"][0]["context"]["latent_cardinality"] = json!({"state": "known_zero"});
            forged
        }),
        ("subject outcome", {
            let mut forged = value.clone();
            forged["contexts"][0]["context"]["subject_outcomes"][0]["endpoint_id"] =
                json!("function:forged");
            forged
        }),
        ("anchor-bearing window", {
            let mut forged = value.clone();
            let window = forged["contexts"][0]["context"]["windows"]
                .as_array_mut()
                .expect("windows")
                .iter_mut()
                .find(|window| {
                    !window["support_anchor_ids"]
                        .as_array()
                        .expect("anchors")
                        .is_empty()
                })
                .expect("anchor-bearing window");
            window["support_anchor_ids"] = json!([]);
            forged
        }),
        ("support-loss summary", {
            let mut forged = value.clone();
            forged["contexts"][0]["context"]["support_loss_summaries"][0]["observed_count"] =
                json!(0_u64);
            forged
        }),
    ];
    for (name, forged) in mutations {
        assert!(
            validate_generic_review_run_v3_wire_structure(&forged).is_err(),
            "semantic validator accepts tampered {name}"
        );
    }
}

#[test]
fn v3_positive_pair_reaches_context_construction() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Independent Git-tree oracle: this does not inspect a context/run value.
    let tree = Command::new("git")
        .args(["ls-tree", "-r", POSITIVE_TARGET])
        .current_dir(&repository)
        .output()
        .expect("Git-tree oracle");
    assert!(tree.status.success());
    assert_eq!(
        tree.stdout
            .split(|byte| *byte == b'\n')
            .filter(|row| !row.is_empty())
            .count(),
        4_816
    );
    assert!(String::from_utf8(tree.stdout).unwrap().contains(
        "4ec5cc757eca70c13698a34e6f892f19c07c1a79\tcrates/reviewgraphen-core/src/context.rs"
    ));
    let request = GenericReviewRequestV3 {
        schema: GENERIC_REVIEW_REQUEST_V3_SCHEMA.to_owned(),
        workspace_admission_root: repository.clone(),
        repository_admission_root: repository.clone(),
        repository_identity: "reviewgraphen".to_owned(),
        base_revision: POSITIVE_BASE.to_owned(),
        target_revision: POSITIVE_TARGET.to_owned(),
        ingest: GenericIngestRequestV2 {
            profile_id: "rust.production.v1".to_owned(),
            profile_version: "1".to_owned(),
            rule_set_hash: ContentHash::sha256(b"reviewgraphen-m20-rule-set@1"),
            max_files: 5_000,
            max_file_bytes: 4 * 1024 * 1024,
            max_total_source_bytes: 64 * 1024 * 1024,
        },
        plan: GenericPlanRequest {
            max_waves: 8,
            max_obligations_per_wave: 32,
        },
        observer: GenericObserverRequestV2::DeterministicAbstain,
        verifier_descriptor_id: None,
        context_policy_id: V3_POLICY.to_owned(),
    };
    let probe = ContextBuildTrace::default();
    let run = run_generic_review_v3_with_probe(&request, Some(Arc::new(probe.clone())))
        .expect("ADR positive D pair reaches v3 context");
    assert_eq!(run.value()["schema"], GENERIC_REVIEW_RUN_V3_SCHEMA);
    assert_eq!(
        run.value()["contexts"].as_array().unwrap().len(),
        2,
        "ADR §5.4: two substantive D obligations"
    );
    let value = serde_json::to_value(run).expect("v3 run JSON");
    validate_generic_review_run_v3_wire_structure(&value).expect("v3 wire closure");

    let trace = probe.snapshot();
    let ids = |select: fn(&ContextBuildEffect) -> Option<_>| {
        trace.iter().filter_map(select).collect::<BTreeSet<_>>()
    };
    let metadata = ids(|effect| match effect {
        ContextBuildEffect::CandidateMetadataVisit { artifact_id } => Some(artifact_id.clone()),
        _ => None,
    });
    let materialized = ids(|effect| match effect {
        ContextBuildEffect::CandidateMaterialized { artifact_id } => Some(artifact_id.clone()),
        _ => None,
    });
    let requested = ids(|effect| match effect {
        ContextBuildEffect::SourceBytesRequested { artifact_id } => Some(artifact_id.clone()),
        _ => None,
    });
    let submitted = ids(|effect| match effect {
        ContextBuildEffect::SourceSubmitted { artifact_id } => Some(artifact_id.clone()),
        _ => None,
    });
    assert_eq!(
        metadata.len(),
        1,
        "only the independently reached file is visited"
    );
    assert_eq!(materialized, metadata);
    assert_eq!(requested, metadata);
    assert_eq!(submitted, metadata);
    assert!(!trace.iter().any(|effect| matches!(
        effect,
        ContextBuildEffect::FullArtifactScan | ContextBuildEffect::FullRelationScan
    )));
}
