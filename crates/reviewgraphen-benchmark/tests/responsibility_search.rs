//! Acceptance contract for the benchmark-only planned-responsibility search.
//!
//! This test deliberately drives the public benchmark CLI.  It does not
//! treat a syntax candidate as an accepted family, a verified clause, or a
//! sign-off decision.

use reviewgraphen_core::{ContentHash, StableId, canonical_json};
use reviewgraphen_ingest::{IngestRequest, ingest_v2};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const CONTRACT_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/planned-responsibility-contract-v1.schema.json"
));
const REPORT_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/responsibility-search-report-v1.schema.json"
));
const CONTRACT_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/fixtures/planned-responsibility-contract-v1.example.json"
));
const REPORT_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/fixtures/responsibility-search-report-v1.example.json"
));

const RULE: &str = "reviewgraphen.benchmark.planned-responsibility-search@1";
const SIGNAL_EXTRACTOR: &str = "reviewgraphen.ingest.rust-responsibility-signals@1";
const FIXTURE_GIT_DATE: &str = "2000-01-01T00:00:00Z";

fn json_fixture(input: &str) -> Value {
    serde_json::from_str(input).expect("checked-in JSON fixture")
}

fn validate(schema: &str, value: &Value) -> bool {
    let schema: Value = serde_json::from_str(schema).expect("schema JSON");
    jsonschema::validator_for(&schema)
        .expect("valid JSON schema")
        .is_valid(value)
}

fn git<const N: usize>(root: &Path, args: [&str; N]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_AUTHOR_DATE", FIXTURE_GIT_DATE)
        .env("GIT_COMMITTER_DATE", FIXTURE_GIT_DATE)
        .args(args)
        .output()
        .expect("git launches");
    assert!(
        output.status.success(),
        "git succeeds: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git emits UTF-8")
        .trim()
        .to_owned()
}

fn ingested_program(source: &str) -> Value {
    let workspace = tempfile::tempdir().expect("temporary workspace");
    let root = workspace.path().join("repository");
    fs::create_dir(&root).expect("repository root");
    git(&root, ["init", "--quiet"]);
    git(
        &root,
        ["config", "user.email", "reviewgraphen@example.test"],
    );
    git(&root, ["config", "user.name", "ReviewGraphen test"]);
    let source_path = root.join("crates/search_fixture/src/lib.rs");
    fs::create_dir_all(source_path.parent().expect("source parent")).expect("source parent");
    fs::write(&source_path, "pub fn before() {}\n").expect("base source");
    git(&root, ["add", "."]);
    git(&root, ["commit", "--quiet", "-m", "base"]);
    let base = git(&root, ["rev-parse", "HEAD"]);
    fs::write(&source_path, source).expect("target source");
    git(&root, ["add", "."]);
    git(&root, ["commit", "--quiet", "-m", "target"]);
    let target = git(&root, ["rev-parse", "HEAD"]);
    let result = ingest_v2(&IngestRequest::new(
        workspace.path(),
        &root,
        "reviewgraphen.test/planned-responsibility-search",
        &base,
        &target,
    ))
    .expect("accepted Rust ProgramSpace");
    serde_json::to_value(result.legacy.program_space).expect("ProgramSpace JSON")
}

fn structural_fixture_source() -> String {
    r#"
pub struct Input(pub u64);
pub struct Output(pub u64);

fn convert(input: Input) -> Output { Output(input.0) }
fn normalize(output: Output) -> Output { output }
fn special(output: Output) -> Output { output }
fn write(output: Output) -> Output { output }

pub fn parse_alpha(input: Input) -> Output {
    let converted = convert(input);
    let normalized = normalize(converted);
    write(special(normalized))
}

pub fn parse_beta(input: Input) -> Output {
    let converted = convert(input);
    let normalized = normalize(converted);
    write(special(normalized))
}

pub fn parse_gamma(input: Input) -> Output {
    let normalized = normalize(convert(input));
    write(normalized)
}
"#
    .to_owned()
}

fn high_frequency_fixture_source() -> String {
    let mut source = structural_fixture_source();
    source.push_str("\nfn generic(input: Input) -> Output { Output(input.0) }\n");
    for index in 0..33 {
        source.push_str(&format!(
            "pub fn generic_{index:02}(input: Input) -> Output {{ normalize(generic(input)) }}\n"
        ));
    }
    source
}

fn high_cardinality_corpus_signal_fixture_source() -> String {
    let mut source = structural_fixture_source();
    for index in 0..82 {
        source.push_str(&format!("\nfn corpus_operation_{index:03}() {{}}\n"));
    }
    source.push_str("\npub fn unrelated_operation_catalog() {\n");
    for index in 0..82 {
        source.push_str(&format!("    corpus_operation_{index:03}();\n"));
    }
    source.push_str("}\n");
    source
}

fn contract_for(program: &Value) -> Value {
    let mut contract = json_fixture(CONTRACT_FIXTURE);
    contract["snapshot_id"] = program["snapshot"]["id"].clone();
    contract
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).expect("JSON serialization")).expect("fixture write");
}

fn run_search(program: &Value, contract: &Value) -> (TempDir, PathBuf, Output) {
    let temp = tempfile::tempdir().expect("temporary CLI directory");
    let program_path = temp.path().join("program-space.json");
    let contract_path = temp.path().join("contract.json");
    let output_path = temp.path().join("report.json");
    write_json(&program_path, program);
    write_json(&contract_path, contract);
    let output = Command::new(env!("CARGO_BIN_EXE_reviewgraphen-responsibility-family"))
        .args([
            "search-responsibility",
            "--program-space",
            program_path.to_str().expect("UTF-8 path"),
            "--contract",
            contract_path.to_str().expect("UTF-8 path"),
            "--output",
            output_path.to_str().expect("UTF-8 path"),
        ])
        .output()
        .expect("benchmark CLI launches");
    (temp, output_path, output)
}

fn assert_exit(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn report(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("report bytes")).expect("report JSON")
}

fn signals(callable: &[&str], signature: &[&str], operation: &[&str]) -> Value {
    json!({
        "callable": callable,
        "signature": signature,
        "operation": operation
    })
}

fn candidate_by_symbol<'a>(report: &'a Value, symbol: &str) -> &'a Value {
    report["candidates"]
        .as_array()
        .expect("candidate array")
        .iter()
        .find(|candidate| {
            candidate["symbol"]
                .as_str()
                .is_some_and(|value| value.ends_with(symbol))
        })
        .expect("named candidate")
}

fn accepted_rust_artifact_attributes_by_label_suffix_mut<'a>(
    program: &'a mut Value,
    label_suffix: &str,
) -> &'a mut serde_json::Map<String, Value> {
    let artifacts = program["artifacts"]
        .as_array_mut()
        .expect("accepted artifacts");
    let position = artifacts
        .iter()
        .position(|artifact| {
            artifact["kind"] == "function"
                && artifact["language"] == "rust"
                && artifact["label"]
                    .as_str()
                    .is_some_and(|label| label.ends_with(label_suffix))
        })
        .expect("accepted Rust artifact with label suffix");
    artifacts[position]["attributes"]
        .as_object_mut()
        .expect("accepted Rust artifact attributes")
}

fn accepted_rust_artifact_and_anchor_by_label_suffix<'a>(
    program: &'a Value,
    label_suffix: &str,
) -> (&'a Value, &'a Value) {
    let mut matches = program["artifacts"]
        .as_array()
        .expect("accepted artifacts")
        .iter()
        .filter(|artifact| {
            artifact["kind"] == "function"
                && artifact["language"] == "rust"
                && artifact["label"]
                    .as_str()
                    .is_some_and(|label| label.ends_with(label_suffix))
        });
    let artifact = matches
        .next()
        .expect("accepted Rust artifact with label suffix");
    assert!(
        matches.next().is_none(),
        "label suffix must identify exactly one accepted Rust function"
    );
    let artifact_id = artifact["id"].as_str().expect("accepted artifact ID");
    let anchor = &program["incremental_facts"]["rust_symbol_anchors"][artifact_id];
    assert!(
        anchor.is_object(),
        "accepted Rust artifact must have an anchor keyed by its actual artifact ID"
    );
    (artifact, anchor)
}

fn expected_candidate_id(search_id: &str, artifact_id: &str) -> String {
    StableId::derived(
        "responsibility-search-candidate",
        &BTreeMap::from([
            (
                "artifact_id".to_owned(),
                Value::String(artifact_id.to_owned()),
            ),
            ("search_id".to_owned(), Value::String(search_id.to_owned())),
        ]),
    )
    .expect("candidate identity")
    .to_string()
}

#[test]
fn planned_responsibility_schemas_are_closed_and_enforce_the_public_thresholds() {
    let contract = json_fixture(CONTRACT_FIXTURE);
    let report = json_fixture(REPORT_FIXTURE);
    assert!(validate(CONTRACT_SCHEMA, &contract));
    assert!(validate(REPORT_SCHEMA, &report));

    let mut empty_program_space_profile = report.clone();
    empty_program_space_profile["program_space"]["profile_id"] = json!("");
    assert!(!validate(REPORT_SCHEMA, &empty_program_space_profile));

    let mut supported_program_hashes = report.clone();
    supported_program_hashes["program_space"]["rule_set_hash"] = json!("sha256:3333333333333333");
    supported_program_hashes["program_space"]["extractor_set_hash"] = json!("git:abcdef12");
    assert!(validate(REPORT_SCHEMA, &supported_program_hashes));

    for invalid_program_hash in ["sha256:3333333", "sha512:33333333", "sha256:3333333g"] {
        let mut invalid_program_hash_report = report.clone();
        invalid_program_hash_report["program_space"]["rule_set_hash"] = json!(invalid_program_hash);
        assert!(!validate(REPORT_SCHEMA, &invalid_program_hash_report));
    }

    let mut contract_extra = contract.clone();
    contract_extra["authority"] = json!("not allowed on an input contract");
    assert!(!validate(CONTRACT_SCHEMA, &contract_extra));

    let mut accepted_authority = report.clone();
    accepted_authority["authority"]["accepted"] = json!(true);
    assert!(!validate(REPORT_SCHEMA, &accepted_authority));

    let mut below_threshold = report.clone();
    below_threshold["candidates"][0]["coverage"]["query_operation_coverage_ppm"] = json!(599_999);
    assert!(!validate(REPORT_SCHEMA, &below_threshold));

    let mut threshold = report.clone();
    threshold["candidates"][0]["coverage"]["query_operation_coverage_ppm"] = json!(600_000);
    assert!(validate(REPORT_SCHEMA, &threshold));

    let mut one_operation = report.clone();
    one_operation["candidates"][0]["coverage"]["selective_operation_matches"] = json!(1);
    assert!(!validate(REPORT_SCHEMA, &one_operation));

    let mut no_subject = report.clone();
    no_subject["candidates"][0]["coverage"]["selective_callable_or_signature_matches"] = json!(0);
    assert!(!validate(REPORT_SCHEMA, &no_subject));

    let mut duplicate_clause = report.clone();
    duplicate_clause["candidates"][0]["unverified_clause_ids"] =
        json!(["clause:nonempty-input", "clause:nonempty-input"]);
    assert!(!validate(REPORT_SCHEMA, &duplicate_clause));

    let mut missing_clause_ids = report.clone();
    missing_clause_ids["candidates"][0]
        .as_object_mut()
        .expect("candidate object")
        .remove("unverified_clause_ids");
    assert!(!validate(REPORT_SCHEMA, &missing_clause_ids));

    let mut extra = report;
    extra["accepted"] = json!(true);
    assert!(!validate(REPORT_SCHEMA, &extra));
}

#[test]
fn search_responsibility_includes_structurally_different_and_exact_members_with_a_clause_frontier()
{
    let program = ingested_program(&structural_fixture_source());
    assert_eq!(program["profile"]["id"], "code-review");
    let contract = contract_for(&program);
    let clause_ids = contract["clauses"]
        .as_array()
        .expect("contract clauses")
        .iter()
        .map(|clause| clause["clause_id"].as_str().expect("clause ID").to_owned())
        .collect::<Vec<_>>();

    let (alpha_artifact, alpha_anchor) =
        accepted_rust_artifact_and_anchor_by_label_suffix(&program, "::parse_alpha");
    let (beta_artifact, beta_anchor) =
        accepted_rust_artifact_and_anchor_by_label_suffix(&program, "::parse_beta");
    let (gamma_artifact, gamma_anchor) =
        accepted_rust_artifact_and_anchor_by_label_suffix(&program, "::parse_gamma");
    assert_eq!(
        alpha_anchor["normalized_body_hash"], beta_anchor["normalized_body_hash"],
        "the fixture contains an exact-shape pair"
    );
    assert_ne!(
        alpha_anchor["normalized_body_hash"], gamma_anchor["normalized_body_hash"],
        "the fixture also contains a structurally different member"
    );

    let (first_temp, first_path, first_run) = run_search(&program, &contract);
    assert_exit(&first_run, 0);
    let first_bytes = fs::read(&first_path).expect("first report bytes");
    let first = report(&first_path);
    assert!(validate(REPORT_SCHEMA, &first));

    let (_second_temp, second_path, second_run) = run_search(&program, &contract);
    assert_exit(&second_run, 0);
    assert_eq!(
        first_bytes,
        fs::read(second_path).expect("second report bytes")
    );

    let contract_hash =
        ContentHash::sha256(&canonical_json(&contract).expect("canonical contract"));
    assert_eq!(
        first["contract"]["canonical_hash"],
        contract_hash.to_string()
    );
    assert_eq!(first["contract"]["clause_count"], json!(clause_ids.len()));
    assert_eq!(first["rule"], RULE);
    assert_eq!(first["signal_extractor"], SIGNAL_EXTRACTOR);
    assert_eq!(
        first["contract_clause_handling"],
        json!({
            "semantic_content_retained_by": "contract_hash_only",
            "semantic_content_evaluated": false
        })
    );
    assert_eq!(
        first["authority"],
        json!({
            "classification": "non_authority",
            "accepted": false,
            "verified": false,
            "human_accepted": false,
            "sign_off": false
        })
    );
    assert_eq!(
        first["selection"]["candidate_selection"],
        "all_eligible_matching_candidates"
    );
    assert_eq!(first["candidates"].as_array().expect("candidates").len(), 3);
    assert_eq!(first["denominator"]["candidate_count"], json!(3));
    assert_eq!(
        first["denominator"]["candidate_clause_obligations"],
        json!(9)
    );
    assert_eq!(
        first["program_space"]["profile_id"],
        program["profile"]["id"]
    );
    assert_eq!(
        first["program_space"]["rule_set_hash"],
        program["profile"]["rule_set_hash"]
    );
    assert_eq!(
        first["program_space"]["policy_version"],
        program["profile"]["policy_version"]
    );

    let symbols = first["candidates"]
        .as_array()
        .expect("candidate array")
        .iter()
        .map(|candidate| candidate["symbol"].as_str().expect("candidate symbol"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        symbols,
        BTreeSet::from([
            alpha_artifact["label"]
                .as_str()
                .expect("alpha artifact label"),
            beta_artifact["label"]
                .as_str()
                .expect("beta artifact label"),
            gamma_artifact["label"]
                .as_str()
                .expect("gamma artifact label"),
        ])
    );
    for candidate in first["candidates"].as_array().expect("candidate array") {
        assert_eq!(candidate["unverified_clause_ids"], json!(clause_ids));
        assert_eq!(
            candidate["candidate_id"],
            expected_candidate_id(
                first["search_id"].as_str().expect("search ID"),
                candidate["artifact_id"].as_str().expect("artifact ID")
            ),
            "rank is deliberately absent from candidate identity"
        );
        assert!(
            candidate["source_hash"]
                .as_str()
                .is_some_and(|hash| hash.starts_with("sha256:"))
        );
        assert!(
            candidate["member_hash"]
                .as_str()
                .is_some_and(|hash| hash.starts_with("sha256:"))
        );
        assert!(
            candidate["coverage"]["selective_callable_or_signature_matches"]
                .as_u64()
                .unwrap()
                >= 1
        );
        assert!(
            candidate["coverage"]["selective_operation_matches"]
                .as_u64()
                .unwrap()
                >= 2
        );
        assert!(
            candidate["coverage"]["query_operation_coverage_ppm"]
                .as_u64()
                .unwrap()
                >= 600_000
        );
    }

    let mut changed_contract = contract.clone();
    changed_contract["unknowns"] = json!([
        "dynamic dispatch is not resolved",
        "opaque extension dispatch is not resolved"
    ]);
    let (_changed_temp, changed_path, changed_run) = run_search(&program, &changed_contract);
    assert_exit(&changed_run, 0);
    let changed = report(&changed_path);
    assert_ne!(
        first["search_id"], changed["search_id"],
        "full canonical contract hash binds search identity"
    );
    assert_ne!(
        candidate_by_symbol(&first, "parse_alpha")["candidate_id"],
        candidate_by_symbol(&changed, "parse_alpha")["candidate_id"],
        "candidate identity is bound through search identity"
    );

    let mut changed_source = structural_fixture_source();
    changed_source.push_str("\n// a distinct accepted snapshot\n");
    let changed_program = ingested_program(&changed_source);
    let changed_snapshot_contract = contract_for(&changed_program);
    let (_snapshot_temp, snapshot_path, snapshot_run) =
        run_search(&changed_program, &changed_snapshot_contract);
    assert_exit(&snapshot_run, 0);
    let changed_snapshot = report(&snapshot_path);
    assert_ne!(
        first["search_id"], changed_snapshot["search_id"],
        "snapshot binds search identity"
    );
    drop(first_temp);
}

#[test]
fn search_responsibility_identity_binds_the_complete_program_space_basis() {
    let program = ingested_program(&structural_fixture_source());
    let contract = contract_for(&program);
    let (_baseline_temp, baseline_path, baseline_run) = run_search(&program, &contract);
    assert_exit(&baseline_run, 0);
    let baseline = report(&baseline_path);

    for (section, field, output_field, value) in [
        ("profile", "id", "profile_id", json!("code-review-r6")),
        ("profile", "version", "profile_version", json!("r6")),
        (
            "profile",
            "rule_set_hash",
            "rule_set_hash",
            json!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        ),
        (
            "extraction",
            "adapter_set_hash",
            "extractor_set_hash",
            json!("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        ),
        ("profile", "policy_version", "policy_version", json!("r6")),
    ] {
        let mut mutated_program = program.clone();
        mutated_program[section][field] = value.clone();
        let (_temp, output_path, output) = run_search(&mutated_program, &contract);
        assert_exit(&output, 0);
        let mutated = report(&output_path);
        assert_eq!(mutated["program_space"][output_field], value);
        assert_ne!(
            baseline["search_id"], mutated["search_id"],
            "mutating ProgramSpace profile {field} must change search identity"
        );
        assert_ne!(
            candidate_by_symbol(&baseline, "parse_alpha")["candidate_id"],
            candidate_by_symbol(&mutated, "parse_alpha")["candidate_id"],
            "candidate identity must be derived through the changed search identity"
        );
        let candidate = candidate_by_symbol(&mutated, "parse_alpha");
        assert_eq!(
            candidate["candidate_id"],
            expected_candidate_id(
                mutated["search_id"].as_str().expect("search ID"),
                candidate["artifact_id"].as_str().expect("artifact ID"),
            ),
            "candidate identity remains search ID plus artifact ID"
        );
    }
}

#[test]
fn search_responsibility_rejects_malformed_supported_signals_but_excludes_unknown_extractors() {
    let program = ingested_program(&structural_fixture_source());
    let contract = contract_for(&program);

    let mut non_string_item = program.clone();
    let attributes = accepted_rust_artifact_attributes_by_label_suffix_mut(
        &mut non_string_item,
        "::parse_alpha",
    );
    assert_eq!(
        attributes["responsibility_signals_extractor"],
        SIGNAL_EXTRACTOR
    );
    attributes["responsibility_signals"]["operation"]
        .as_array_mut()
        .expect("accepted operation signal array")
        .push(json!(7));

    let mut extra_key = program.clone();
    let attributes =
        accepted_rust_artifact_attributes_by_label_suffix_mut(&mut extra_key, "::parse_alpha");
    assert_eq!(
        attributes["responsibility_signals_extractor"],
        SIGNAL_EXTRACTOR
    );
    attributes["responsibility_signals"]["extra"] = json!(true);

    for (name, malformed) in [
        ("non-string-item", non_string_item),
        ("extra-key", extra_key),
    ] {
        let (_temp, output_path, output) = run_search(&malformed, &contract);
        assert_exit(&output, 4);
        assert!(
            !output_path.exists(),
            "{name} supported signal fact must not leave output"
        );
    }

    let (_baseline_temp, baseline_path, baseline_run) = run_search(&program, &contract);
    assert_exit(&baseline_run, 0);
    let baseline = report(&baseline_path);
    let baseline_exclusions = baseline["denominator"]["excluded_unknown_signal_fact"]
        .as_u64()
        .expect("baseline unknown-signal exclusion count");

    let mut missing_extractor = program.clone();
    accepted_rust_artifact_attributes_by_label_suffix_mut(&mut missing_extractor, "::parse_alpha")
        .remove("responsibility_signals_extractor");
    let mut wrong_extractor = program.clone();
    accepted_rust_artifact_attributes_by_label_suffix_mut(&mut wrong_extractor, "::parse_alpha")
        ["responsibility_signals_extractor"] = json!("other.extractor@1");

    for (name, unknown) in [
        ("missing-extractor", missing_extractor),
        ("wrong-extractor", wrong_extractor),
    ] {
        let (_temp, output_path, output) = run_search(&unknown, &contract);
        assert_exit(&output, 0);
        let unknown_report = report(&output_path);
        assert!(validate(REPORT_SCHEMA, &unknown_report));
        assert_eq!(
            unknown_report["denominator"]["excluded_unknown_signal_fact"],
            json!(baseline_exclusions + 1),
            "{name} remains a declared unknown exclusion"
        );
    }
}

#[test]
fn planned_contract_unknowns_reserve_the_report_unknown_capacity() {
    let program = ingested_program(&structural_fixture_source());
    let contract = contract_for(&program);
    let unknowns = |count| {
        Value::Array(
            (0..count)
                .map(|index| Value::String(format!("contract unknown {index:02}")))
                .collect(),
        )
    };

    let mut sixty_two = contract.clone();
    sixty_two["unknowns"] = unknowns(62);
    assert!(
        validate(CONTRACT_SCHEMA, &sixty_two),
        "62 contract unknowns remain a schema-valid input"
    );
    let (_temp, output_path, output) = run_search(&program, &sixty_two);
    assert_exit(&output, 0);
    let sixty_two_report = report(&output_path);
    assert!(validate(REPORT_SCHEMA, &sixty_two_report));
    assert!(
        sixty_two_report["unknowns"]
            .as_array()
            .expect("report unknowns")
            .len()
            <= 64,
        "two fixed report unknowns leave a closed-report maximum of 64"
    );

    let mut sixty_three = contract;
    sixty_three["unknowns"] = unknowns(63);
    assert!(
        !validate(CONTRACT_SCHEMA, &sixty_three),
        "63 contract unknowns exceed the input schema cap"
    );
    let (_temp, output_path, output) = run_search(&program, &sixty_three);
    assert_exit(&output, 4);
    assert!(
        !output_path.exists(),
        "over-cap contract must not leave an output"
    );
}

#[cfg(unix)]
#[test]
fn search_responsibility_mid_write_failure_leaves_no_final_output() {
    let program = ingested_program(&structural_fixture_source());
    let mut contract = contract_for(&program);
    contract["unknowns"] = Value::Array(
        (0..62)
            .map(|index| Value::String(format!("{index:02}-{}", "x".repeat(4_093))))
            .collect(),
    );
    assert!(
        validate(CONTRACT_SCHEMA, &contract),
        "the file-limit probe uses a schema-valid, sufficiently large contract"
    );

    let temp = tempfile::tempdir().expect("temporary file-limit directory");
    let program_path = temp.path().join("program-space.json");
    let contract_path = temp.path().join("contract.json");
    let output_path = temp.path().join("report.json");
    write_json(&program_path, &program);
    write_json(&contract_path, &contract);
    let output = Command::new("sh")
        .args([
            "-c",
            "ulimit -f 1; exec \"$@\"",
            "reviewgraphen-responsibility-family",
        ])
        .arg(env!("CARGO_BIN_EXE_reviewgraphen-responsibility-family"))
        .args([
            "search-responsibility",
            "--program-space",
            program_path.to_str().expect("UTF-8 path"),
            "--contract",
            contract_path.to_str().expect("UTF-8 path"),
            "--output",
            output_path.to_str().expect("UTF-8 path"),
        ])
        .output()
        .expect("file-limited benchmark CLI launches");
    assert!(
        !output.status.success(),
        "file-limited write must fail: stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output_path.exists(),
        "a failed fresh-output write must not leave the authoritative final path"
    );
}

#[test]
fn search_responsibility_keeps_df_one_absent_and_high_frequency_terms_in_the_contract_denominator()
{
    let program = ingested_program(&high_frequency_fixture_source());
    let mut contract = contract_for(&program);
    contract["search_signals"] = signals(&["alpha"], &[], &["convert", "special"]);
    let (_df_one_temp, df_one_path, df_one_run) = run_search(&program, &contract);
    assert_exit(&df_one_run, 0);
    let df_one = report(&df_one_path);
    assert_eq!(
        df_one["query_signals"]["selective"]["callable"],
        json!(["alpha"])
    );
    assert_eq!(
        df_one["candidates"].as_array().expect("candidates").len(),
        1
    );
    let (alpha_artifact, _) =
        accepted_rust_artifact_and_anchor_by_label_suffix(&program, "::parse_alpha");
    assert_eq!(
        candidate_by_symbol(&df_one, "parse_alpha")["symbol"],
        alpha_artifact["label"]
    );

    let mut subject_zero = contract.clone();
    subject_zero["search_signals"] = signals(&["absent_subject"], &[], &["convert", "normalize"]);
    let (_subject_temp, subject_path, subject_run) = run_search(&program, &subject_zero);
    assert_exit(&subject_run, 0);
    let subject = report(&subject_path);
    assert!(
        subject["candidates"]
            .as_array()
            .expect("candidates")
            .is_empty()
    );
    assert_eq!(
        subject["denominator"]["candidate_clause_obligations"],
        json!(0)
    );

    let mut one_operation = contract.clone();
    one_operation["search_signals"] = signals(&["alpha"], &[], &["convert", "missing"]);
    let (_one_operation_temp, one_operation_path, one_operation_run) =
        run_search(&program, &one_operation);
    assert_exit(&one_operation_run, 0);
    assert!(
        report(&one_operation_path)["candidates"]
            .as_array()
            .expect("candidates")
            .is_empty()
    );

    let mut high_only = contract.clone();
    high_only["search_signals"] = signals(&["generic"], &[], &["generic", "normalize"]);
    let (_high_temp, high_path, high_run) = run_search(&program, &high_only);
    assert_exit(&high_run, 0);
    let high = report(&high_path);
    assert!(
        high["candidates"]
            .as_array()
            .expect("candidates")
            .is_empty()
    );
    assert_eq!(
        high["query_signals"]["high_frequency"]["callable"],
        json!(["generic"])
    );
    assert_eq!(
        high["query_signals"]["high_frequency"]["operation"],
        json!(["generic", "normalize"])
    );

    let mut denominator = contract;
    denominator["search_signals"] = signals(
        &["alpha"],
        &[],
        &[
            "convert",
            "generic",
            "missing",
            "normalize",
            "special",
            "write",
        ],
    );
    let (_denominator_temp, denominator_path, denominator_run) = run_search(&program, &denominator);
    assert_exit(&denominator_run, 0);
    let denominator_report = report(&denominator_path);
    assert!(
        denominator_report["candidates"]
            .as_array()
            .expect("candidates")
            .is_empty(),
        "absent and high-frequency operations remain in the six-term coverage denominator"
    );
    assert_eq!(
        denominator_report["query_signals"]["absent"]["operation"],
        json!(["missing"])
    );
    assert_eq!(
        denominator_report["query_signals"]["high_frequency"]["operation"],
        json!(["generic", "normalize"])
    );

    let mut exact_threshold = denominator.clone();
    exact_threshold["search_signals"] = signals(
        &["alpha"],
        &[],
        &["convert", "generic", "normalize", "special", "write"],
    );
    let (_threshold_temp, threshold_path, threshold_run) = run_search(&program, &exact_threshold);
    assert_exit(&threshold_run, 0);
    let threshold = report(&threshold_path);
    let candidate = candidate_by_symbol(&threshold, "parse_alpha");
    assert_eq!(
        candidate["coverage"]["selective_operation_matches"],
        json!(3)
    );
    assert_eq!(candidate["coverage"]["query_operation_terms"], json!(5));
    assert_eq!(
        candidate["coverage"]["query_operation_coverage_ppm"],
        json!(600_000)
    );
}

#[test]
fn search_responsibility_does_not_apply_query_caps_to_accepted_corpus_signal_lists() {
    let program = ingested_program(&high_cardinality_corpus_signal_fixture_source());
    let contract = contract_for(&program);
    let (irrelevant_artifact, _) = accepted_rust_artifact_and_anchor_by_label_suffix(
        &program,
        "::unrelated_operation_catalog",
    );
    assert_eq!(
        irrelevant_artifact["attributes"]["test_scope"],
        json!("production"),
        "the high-cardinality function is an eligible production artifact"
    );
    let corpus_operations =
        irrelevant_artifact["attributes"]["responsibility_signals"]["operation"]
            .as_array()
            .expect("accepted irrelevant artifact operation signals")
            .iter()
            .map(|value| value.as_str().expect("operation signal string"))
            .collect::<Vec<_>>();
    assert!(
        corpus_operations.len() > 64,
        "the accepted irrelevant artifact has more operations than the query cap"
    );
    assert!(
        corpus_operations.windows(2).all(|pair| pair[0] < pair[1]),
        "accepted corpus operation signals are sorted and unique"
    );
    assert!(
        contract["search_signals"]["operation"]
            .as_array()
            .expect("contract operation query terms")
            .len()
            <= 64,
        "the planned contract query remains within its distinct query cap"
    );

    let (_temp, output_path, output) = run_search(&program, &contract);
    assert_exit(&output, 0);
    let result = report(&output_path);
    assert!(validate(REPORT_SCHEMA, &result));

    let (alpha_artifact, _) =
        accepted_rust_artifact_and_anchor_by_label_suffix(&program, "::parse_alpha");
    let (beta_artifact, _) =
        accepted_rust_artifact_and_anchor_by_label_suffix(&program, "::parse_beta");
    let (gamma_artifact, _) =
        accepted_rust_artifact_and_anchor_by_label_suffix(&program, "::parse_gamma");
    let symbols = result["candidates"]
        .as_array()
        .expect("candidate array")
        .iter()
        .map(|candidate| candidate["symbol"].as_str().expect("candidate symbol"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        symbols,
        BTreeSet::from([
            alpha_artifact["label"]
                .as_str()
                .expect("alpha artifact label"),
            beta_artifact["label"]
                .as_str()
                .expect("beta artifact label"),
            gamma_artifact["label"]
                .as_str()
                .expect("gamma artifact label"),
        ]),
        "the irrelevant high-cardinality artifact must not prevent intended parse candidates"
    );
}

#[test]
fn search_responsibility_rejects_bad_contracts_and_never_writes_partial_output() {
    let program = ingested_program(&structural_fixture_source());
    let contract = contract_for(&program);

    let invalid_cases = [
        ("wrong-snapshot", {
            let mut value = contract.clone();
            value["snapshot_id"] = json!("snapshot:other");
            value
        }),
        ("wrong-extractor", {
            let mut value = contract.clone();
            value["signal_extractor"] = json!("other.extractor@1");
            value
        }),
        ("unsorted-clauses", {
            let mut value = contract.clone();
            value["clauses"].as_array_mut().expect("clauses").reverse();
            value
        }),
        ("duplicate-clause-id", {
            let mut value = contract.clone();
            let first = value["clauses"][0].clone();
            value["clauses"]
                .as_array_mut()
                .expect("clauses")
                .insert(1, first);
            value
        }),
        ("over-limit-clauses", {
            let mut value = contract.clone();
            value["clauses"] = Value::Array(
                (0..65)
                    .map(|index| {
                        json!({
                            "clause_id": format!("clause:{index:02}"),
                            "kind": "precondition",
                            "statement": "must remain bounded"
                        })
                    })
                    .collect(),
            );
            value
        }),
        ("over-limit-utf8", {
            let mut value = contract.clone();
            value["unknowns"] = json!(["é".repeat(2_049)]);
            value
        }),
    ];
    for (name, invalid) in invalid_cases {
        let (_temp, output_path, output) = run_search(&program, &invalid);
        assert_exit(&output, 4);
        assert!(
            !output_path.exists(),
            "{name} must not leave a partial output"
        );
    }

    let temp = tempfile::tempdir().expect("temporary malformed input directory");
    let program_path = temp.path().join("program-space.json");
    let contract_path = temp.path().join("contract.json");
    let output_path = temp.path().join("report.json");
    write_json(&program_path, &program);
    fs::write(&contract_path, b"{").expect("malformed contract write");
    let malformed = Command::new(env!("CARGO_BIN_EXE_reviewgraphen-responsibility-family"))
        .args([
            "search-responsibility",
            "--program-space",
            program_path.to_str().expect("UTF-8 path"),
            "--contract",
            contract_path.to_str().expect("UTF-8 path"),
            "--output",
            output_path.to_str().expect("UTF-8 path"),
        ])
        .output()
        .expect("benchmark CLI launches");
    assert_exit(&malformed, 3);
    assert!(
        !output_path.exists(),
        "malformed input must not leave a partial output"
    );

    let temp = tempfile::tempdir().expect("temporary overwrite directory");
    let program_path = temp.path().join("program-space.json");
    let contract_path = temp.path().join("contract.json");
    let output_path = temp.path().join("report.json");
    write_json(&program_path, &program);
    write_json(&contract_path, &contract);
    fs::write(&output_path, b"preserve existing output").expect("existing output");
    let overwrite = Command::new(env!("CARGO_BIN_EXE_reviewgraphen-responsibility-family"))
        .args([
            "search-responsibility",
            "--program-space",
            program_path.to_str().expect("UTF-8 path"),
            "--contract",
            contract_path.to_str().expect("UTF-8 path"),
            "--output",
            output_path.to_str().expect("UTF-8 path"),
        ])
        .output()
        .expect("benchmark CLI launches");
    assert_exit(&overwrite, 5);
    assert_eq!(
        fs::read(&output_path).expect("existing output bytes"),
        b"preserve existing output"
    );
}
