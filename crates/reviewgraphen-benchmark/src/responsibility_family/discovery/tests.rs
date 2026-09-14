use super::*;
use reviewgraphen_core::{ContentHash, StableId, canonical_json};

const REPORT_SCHEMA_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/exact-body-candidate-report-v2.schema.json"
));
const REPORT_EXAMPLE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/fixtures/exact-body-report-v2.example.json"
));
const NEAR_REPORT_SCHEMA_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/near-body-candidate-report-v1.schema.json"
));
const SIGNAL_REPORT_SCHEMA_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/responsibility-signal-candidate-report-v1.schema.json"
));
const SIGNAL_REPORT_EXAMPLE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/fixtures/responsibility-signal-report-v1.example.json"
));

fn id(value: &str) -> StableId {
    StableId::parse(value).unwrap()
}

fn fact(name: &str, path: &str, body: &str, test_function: bool) -> ExactFunctionFact {
    ExactFunctionFact {
        artifact_id: id(&format!("function:{name}")),
        path: path.to_owned(),
        symbol: name.to_owned(),
        start_line: 10,
        end_line: 20,
        symbol_kind: ExactSymbolKind::Function,
        signature_shape_hash: ContentHash::sha256(format!("signature:{name}").as_bytes()),
        normalized_body_hash: ContentHash::sha256(body.as_bytes()),
        test_scope: Some(if test_function {
            TestScopeFact::Test
        } else {
            TestScopeFact::Production
        }),
        responsibility_shape_hash: Some(ContentHash::sha256(format!("shape:{body}").as_bytes())),
    }
}

#[test]
fn exact_body_groups_are_deterministic_non_authoritative_candidates() {
    let alpha = fact("alpha", "crates/a/src/lib.rs", "same", false);
    let beta = fact("beta", "crates/b/src/lib.rs", "same", false);
    let singleton = fact("singleton", "crates/c/src/lib.rs", "other", false);
    let first = discover_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![beta.clone(), singleton.clone(), alpha.clone()],
        3,
    )
    .unwrap();
    let second = discover_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![alpha, beta, singleton],
        3,
    )
    .unwrap();

    assert_eq!(
        canonical_json(&first).unwrap(),
        canonical_json(&second).unwrap()
    );
    assert_eq!(first.denominator.accepted_rust_symbols, 3);
    assert_eq!(first.denominator.eligible_functions, 3);
    assert_eq!(first.denominator.candidate_groups, 1);
    assert_eq!(first.denominator.candidate_members, 2);
    assert_eq!(first.candidates[0].members.len(), 2);
    assert_eq!(first.authority, AuthorityBoundary::non_authority());
    assert_eq!(first.candidates[0].status, CandidateStatus::CandidateOnly);
}

#[test]
fn profile_excludes_tests_and_non_production_paths_without_hiding_the_denominator() {
    let report = discover_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![
            fact("kept", "crates/a/src/lib.rs", "same", false),
            fact("test", "crates/a/src/lib.rs", "same", true),
            fact("integration", "crates/a/tests/x.rs", "same", false),
            fact("tool", "tools/x/src/main.rs", "same", false),
            ExactFunctionFact {
                test_scope: None,
                ..fact("unknown", "crates/a/src/lib.rs", "same", false)
            },
        ],
        5,
    )
    .unwrap();

    assert_eq!(report.denominator.accepted_rust_symbols, 5);
    assert_eq!(report.denominator.eligible_functions, 1);
    assert_eq!(report.denominator.excluded_test_functions, 1);
    assert_eq!(report.denominator.excluded_profile_paths, 2);
    assert_eq!(report.denominator.excluded_unknown_test_scope, 1);
    assert!(report.candidates.is_empty());
}

#[test]
fn one_body_mutation_removes_the_candidate() {
    let baseline = discover_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![
            fact("alpha", "crates/a/src/lib.rs", "same", false),
            fact("beta", "crates/b/src/lib.rs", "same", false),
        ],
        2,
    )
    .unwrap();
    let changed = discover_facts(
        id("snapshot:two"),
        "2.0.119".to_owned(),
        vec![
            fact("alpha", "crates/a/src/lib.rs", "same", false),
            fact("beta", "crates/b/src/lib.rs", "changed", false),
        ],
        2,
    )
    .unwrap();

    assert_eq!(baseline.candidates.len(), 1);
    assert!(changed.candidates.is_empty());
}

#[test]
fn invalid_fact_closure_is_rejected() {
    let duplicate = fact("alpha", "crates/a/src/lib.rs", "same", false);
    assert!(
        discover_facts(
            id("snapshot:one"),
            "2.0.119".to_owned(),
            vec![duplicate.clone(), duplicate],
            2,
        )
        .is_err()
    );
}

#[test]
fn larger_exact_bodies_are_ranked_first() {
    let mut long_alpha = fact("long_alpha", "crates/a/src/lib.rs", "long", false);
    let mut long_beta = fact("long_beta", "crates/b/src/lib.rs", "long", false);
    long_alpha.end_line = 40;
    long_beta.end_line = 35;
    let report = discover_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![
            fact("short_alpha", "crates/a/src/lib.rs", "short", false),
            fact("short_beta", "crates/b/src/lib.rs", "short", false),
            long_alpha,
            long_beta,
        ],
        4,
    )
    .unwrap();

    assert_eq!(report.candidates[0].minimum_member_span_lines, 26);
    assert_eq!(report.candidates[1].minimum_member_span_lines, 11);
}

#[test]
fn report_satisfies_closed_schema() {
    let report = discover_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![
            fact("alpha", "crates/a/src/lib.rs", "same", false),
            fact("beta", "crates/b/src/lib.rs", "same", false),
        ],
        2,
    )
    .unwrap();
    let mut document = serde_json::to_value(report).unwrap();
    let schema: serde_json::Value = serde_json::from_str(REPORT_SCHEMA_JSON).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(&document));

    document["authority"]["accepted"] = serde_json::json!(true);
    assert!(!validator.is_valid(&document));

    let example: serde_json::Value = serde_json::from_str(REPORT_EXAMPLE_JSON).unwrap();
    assert!(validator.is_valid(&example));
}

#[test]
fn near_clone_groups_identifier_and_literal_variants_but_not_control_flow_variants() {
    let mut alpha = fact("alpha", "rust/a/src/lib.rs", "exact-a", false);
    let mut beta = fact("beta", "rust/b/src/lib.rs", "exact-b", false);
    let mut different = fact("different", "rust/c/src/lib.rs", "exact-c", false);
    alpha.responsibility_shape_hash = Some(ContentHash::sha256(b"same-shape"));
    beta.responsibility_shape_hash = Some(ContentHash::sha256(b"same-shape"));
    different.responsibility_shape_hash = Some(ContentHash::sha256(b"different-shape"));

    let report = discover_near_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![different, beta, alpha],
        3,
    )
    .unwrap();

    assert_eq!(report.denominator.eligible_functions, 3);
    assert_eq!(report.denominator.candidate_groups, 1);
    assert_eq!(report.denominator.candidate_members, 2);
    assert_eq!(report.candidates[0].members.len(), 2);
    assert_eq!(report.candidates[0].distinct_exact_body_hashes, 2);
    assert_eq!(report.authority, AuthorityBoundary::non_authority());
    let schema: serde_json::Value = serde_json::from_str(NEAR_REPORT_SCHEMA_JSON).unwrap();
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&serde_json::to_value(report).unwrap())
    );
}

#[test]
fn near_clone_excludes_exact_duplicates_and_missing_shape_facts_with_denominators() {
    let mut exact_a = fact("exact_a", "crates/a/src/lib.rs", "same", false);
    let mut exact_b = fact("exact_b", "crates/b/src/lib.rs", "same", false);
    let mut missing = fact("missing", "crates/c/src/lib.rs", "other", false);
    exact_a.responsibility_shape_hash = Some(ContentHash::sha256(b"same-shape"));
    exact_b.responsibility_shape_hash = Some(ContentHash::sha256(b"same-shape"));
    missing.responsibility_shape_hash = None;

    let report = discover_near_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![exact_a, exact_b, missing],
        3,
    )
    .unwrap();

    assert!(report.candidates.is_empty());
    assert_eq!(report.denominator.excluded_missing_shape_fact, 1);
    assert_eq!(report.denominator.excluded_exact_only_groups, 1);
}

fn signal_fact(
    name: &str,
    shape: &str,
    callable: &[&str],
    signature: &[&str],
    operation: &[&str],
) -> ResponsibilitySignalFact {
    ResponsibilitySignalFact {
        artifact_id: id(&format!("function:{name}")),
        path: format!("crates/{name}/src/lib.rs"),
        symbol: name.to_owned(),
        start_line: 10,
        end_line: 20,
        symbol_kind: ExactSymbolKind::Function,
        signature_shape_hash: ContentHash::sha256(format!("signature:{name}").as_bytes()),
        normalized_body_hash: ContentHash::sha256(format!("body:{name}").as_bytes()),
        test_scope: Some(TestScopeFact::Production),
        responsibility_shape_hash: Some(ContentHash::sha256(shape.as_bytes())),
        responsibility_signals_extractor: Some(
            "reviewgraphen.ingest.rust-responsibility-signals@1".to_owned(),
        ),
        responsibility_signals: Some(ResponsibilitySignals {
            callable: callable.iter().map(|value| (*value).to_owned()).collect(),
            signature: signature.iter().map(|value| (*value).to_owned()).collect(),
            operation: operation.iter().map(|value| (*value).to_owned()).collect(),
        }),
    }
}

fn candidate_id_for_members(
    report: &ResponsibilitySignalCandidateReport,
    expected: &[&str],
) -> StableId {
    report
        .candidates
        .iter()
        .find(|candidate| {
            candidate
                .members
                .iter()
                .map(|member| member.artifact_id.to_string())
                .eq(expected.iter().map(|member| (*member).to_owned()))
        })
        .expect("candidate pair exists")
        .candidate_id
        .clone()
}

#[test]
fn signal_discovery_pairs_structurally_different_members_by_two_signal_channels() {
    let alpha = signal_fact(
        "alpha",
        "shape-a",
        &["load"],
        &["Request"],
        &["normalize", "parse"],
    );
    let beta = signal_fact(
        "beta",
        "shape-b",
        &["store"],
        &["Request"],
        &["normalize", "parse", "persist"],
    );
    let first = discover_signal_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![beta.clone(), alpha.clone()],
        2,
    )
    .unwrap();
    let second = discover_signal_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![alpha, beta],
        2,
    )
    .unwrap();

    assert_eq!(
        canonical_json(&first).unwrap(),
        canonical_json(&second).unwrap(),
        "input permutation must not change report bytes"
    );
    assert_eq!(first.denominator.accepted_rust_symbols, 2);
    assert_eq!(first.denominator.eligible_functions, 2);
    assert_eq!(first.denominator.evaluated_distinct_shape_pairs, 1);
    assert_eq!(first.denominator.candidate_pairs, 1);
    assert_eq!(first.denominator.candidate_members, 2);
    assert_eq!(
        first.extractor.id,
        "reviewgraphen.benchmark.rust-responsibility-signal-pairs@1"
    );
    assert_eq!(
        first.selection.signal_fact_extractor,
        "reviewgraphen.ingest.rust-responsibility-signals@1"
    );
    assert_eq!(first.candidates[0].matched_callable, Vec::<String>::new());
    assert_eq!(first.candidates[0].matched_signature, ["Request"]);
    assert_eq!(
        first.candidates[0].matched_operation,
        ["normalize", "parse"]
    );
    assert_eq!(first.candidates[0].members.len(), 2);
    assert_eq!(first.candidates[0].status, CandidateStatus::CandidateOnly);
    assert_eq!(first.authority, AuthorityBoundary::non_authority());
}

#[test]
fn signal_discovery_requires_operation_jaccard_at_the_declared_boundary() {
    let below_a = signal_fact(
        "below_a",
        "below-shape-a",
        &["load"],
        &[],
        &["normalize", "parse", "unique_a"],
    );
    let below_b = signal_fact(
        "below_b",
        "below-shape-b",
        &["load"],
        &[],
        &["normalize", "parse", "unique_b"],
    );
    let below_support_a = signal_fact(
        "below_support_a",
        "below-support-shape-a",
        &["support_a"],
        &[],
        &["unique_a"],
    );
    let below_support_b = signal_fact(
        "below_support_b",
        "below-support-shape-b",
        &["support_b"],
        &[],
        &["unique_b"],
    );
    let below = discover_signal_facts(
        id("snapshot:below"),
        "2.0.119".to_owned(),
        vec![below_a, below_b, below_support_a, below_support_b],
        4,
    )
    .unwrap();

    assert!(
        below.candidates.is_empty(),
        "two shared operations with intersection/union 2/4 = 500,000 ppm must be excluded"
    );

    let boundary_a = signal_fact(
        "boundary_a",
        "boundary-shape-a",
        &["load"],
        &[],
        &["alpha", "beta", "gamma", "left"],
    );
    let boundary_b = signal_fact(
        "boundary_b",
        "boundary-shape-b",
        &["load"],
        &[],
        &["alpha", "beta", "gamma", "right"],
    );
    let boundary_support_left = signal_fact(
        "boundary_support_left",
        "boundary-support-shape-left",
        &["support_left"],
        &[],
        &["left"],
    );
    let boundary_support_right = signal_fact(
        "boundary_support_right",
        "boundary-support-shape-right",
        &["support_right"],
        &[],
        &["right"],
    );
    let first = discover_signal_facts(
        id("snapshot:boundary"),
        "2.0.119".to_owned(),
        vec![
            boundary_b.clone(),
            boundary_support_left.clone(),
            boundary_a.clone(),
            boundary_support_right.clone(),
        ],
        4,
    )
    .unwrap();
    let second = discover_signal_facts(
        id("snapshot:boundary"),
        "2.0.119".to_owned(),
        vec![
            boundary_support_right,
            boundary_a,
            boundary_support_left,
            boundary_b,
        ],
        4,
    )
    .unwrap();

    assert_eq!(
        canonical_json(&first).unwrap(),
        canonical_json(&second).unwrap(),
        "the boundary candidate report must remain deterministic under input permutation"
    );
    assert_eq!(first.denominator.candidate_pairs, 1);
    assert_eq!(first.candidates.len(), 1);
    assert_eq!(first.candidates[0].operation_jaccard_ppm, 600_000);
    assert_eq!(first.selection.minimum_operation_jaccard_ppm, 600_000);
}

#[test]
fn operation_jaccard_excludes_corpus_singletons_from_union_and_intersection() {
    let alpha = signal_fact(
        "alpha",
        "shape-a",
        &["load"],
        &[],
        &["alpha_only", "normalize", "parse", "validate"],
    );
    let beta = signal_fact(
        "beta",
        "shape-b",
        &["load"],
        &[],
        &["beta_only", "normalize", "parse", "validate"],
    );

    let report = discover_signal_facts(
        id("snapshot:selective-jaccard"),
        "2.0.119".to_owned(),
        vec![alpha, beta],
        2,
    )
    .unwrap();

    assert_eq!(report.selection.max_selective_term_frequency, 32);
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(
        report.candidates[0].matched_operation,
        ["normalize", "parse", "validate"]
    );
    assert_eq!(
        report.candidates[0].operation_jaccard_ppm, 1_000_000,
        "df=1 operation terms must be excluded from both the Jaccard union and intersection"
    );
}

#[test]
fn same_shape_and_insufficient_signal_pairs_are_excluded_and_counted() {
    let same_a = signal_fact(
        "same_a",
        "same-shape",
        &["load"],
        &[],
        &["normalize", "parse"],
    );
    let same_b = signal_fact(
        "same_b",
        "same-shape",
        &["load"],
        &[],
        &["normalize", "parse"],
    );
    let operation_only_a = signal_fact(
        "operation_only_a",
        "operation-a",
        &["left"],
        &["Left"],
        &["decode", "write"],
    );
    let operation_only_b = signal_fact(
        "operation_only_b",
        "operation-b",
        &["right"],
        &["Right"],
        &["decode", "write"],
    );
    let one_operation_a = signal_fact(
        "one_operation_a",
        "one-a",
        &["fetch"],
        &[],
        &["read", "unique_a"],
    );
    let one_operation_b = signal_fact(
        "one_operation_b",
        "one-b",
        &["fetch"],
        &[],
        &["read", "unique_b"],
    );
    let report = discover_signal_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![
            same_a,
            same_b,
            operation_only_a,
            operation_only_b,
            one_operation_a,
            one_operation_b,
        ],
        6,
    )
    .unwrap();

    assert!(report.candidates.is_empty());
    assert_eq!(report.denominator.excluded_same_shape_pairs, 1);
    assert_eq!(report.denominator.evaluated_distinct_shape_pairs, 14);
}

#[test]
fn missing_or_wrong_signal_extractor_is_an_explicit_unknown_exclusion() {
    let valid_a = signal_fact("valid_a", "a", &["load"], &[], &["parse", "write"]);
    let valid_b = signal_fact("valid_b", "b", &["load"], &[], &["parse", "write"]);
    let mut missing = signal_fact("missing", "c", &["load"], &[], &["parse", "write"]);
    missing.responsibility_signals = None;
    missing.responsibility_signals_extractor = None;
    let mut wrong = signal_fact("wrong", "d", &["load"], &[], &["parse", "write"]);
    wrong.responsibility_signals_extractor = Some("other.extractor@1".to_owned());

    let report = discover_signal_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![wrong, valid_b, missing, valid_a],
        4,
    )
    .unwrap();

    assert_eq!(report.denominator.eligible_functions, 2);
    assert_eq!(report.denominator.excluded_unknown_signal_fact, 2);
    assert_eq!(report.denominator.candidate_pairs, 1);
    assert!(
        report.unknowns.iter().any(|unknown| {
            unknown.contains("missing or unsupported responsibility-signal facts")
        })
    );
}

#[test]
fn candidate_identity_binds_members_and_snapshot_but_not_rank() {
    let alpha = signal_fact("alpha", "a", &["load"], &[], &["parse", "write"]);
    let beta = signal_fact("beta", "b", &["load"], &[], &["parse", "write"]);
    let gamma = signal_fact(
        "gamma",
        "c",
        &["strong"],
        &["Shared"],
        &["a", "b", "c", "d"],
    );
    let delta = signal_fact(
        "delta",
        "d",
        &["strong"],
        &["Shared"],
        &["a", "b", "c", "d"],
    );
    let base = discover_signal_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![alpha.clone(), beta.clone()],
        2,
    )
    .unwrap();
    let reranked = discover_signal_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![delta, beta.clone(), gamma, alpha.clone()],
        4,
    )
    .unwrap();
    let other_snapshot = discover_signal_facts(
        id("snapshot:two"),
        "2.0.119".to_owned(),
        vec![alpha.clone(), beta],
        2,
    )
    .unwrap();
    let replacement = signal_fact("replacement", "e", &["load"], &[], &["parse", "write"]);
    let other_member = discover_signal_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![alpha, replacement],
        2,
    )
    .unwrap();

    let base_id = candidate_id_for_members(&base, &["function:alpha", "function:beta"]);
    assert_eq!(
        base_id,
        candidate_id_for_members(&reranked, &["function:alpha", "function:beta"]),
        "rank is triage-only and must not enter identity"
    );
    assert_ne!(
        base_id,
        candidate_id_for_members(&other_snapshot, &["function:alpha", "function:beta"])
    );
    assert_ne!(
        base_id,
        candidate_id_for_members(&other_member, &["function:alpha", "function:replacement"])
    );
}

#[test]
fn high_frequency_terms_use_the_fixed_max_32_or_ceil_one_fifth_rule() {
    let mut facts = (0..166)
        .map(|index| {
            signal_fact(
                &format!("member_{index:03}"),
                &format!("shape-{index}"),
                &[&format!("callable_{index:03}")],
                &[],
                &[&format!("operation_{index:03}")],
            )
        })
        .collect::<Vec<_>>();
    for fact in facts.iter_mut().take(35) {
        let signals = fact.responsibility_signals.as_mut().unwrap();
        signals.callable.push("generic".to_owned());
        signals.operation.push("convert".to_owned());
        signals.callable.sort();
        signals.operation.sort();
    }
    let report =
        discover_signal_facts(id("snapshot:one"), "2.0.119".to_owned(), facts, 166).unwrap();

    assert_eq!(report.selection.max_selective_term_frequency, 34);
    assert!(
        report
            .ignored_high_frequency_terms
            .callable
            .contains(&"generic".to_owned())
    );
    assert!(
        report
            .ignored_high_frequency_terms
            .operation
            .contains(&"convert".to_owned())
    );
    assert!(report.candidates.is_empty());
}

#[test]
fn signal_report_is_closed_non_authoritative_and_rejects_accepted_true() {
    let report = discover_signal_facts(
        id("snapshot:one"),
        "2.0.119".to_owned(),
        vec![
            signal_fact("alpha", "a", &["load"], &[], &["parse", "write"]),
            signal_fact("beta", "b", &["load"], &[], &["parse", "write"]),
        ],
        2,
    )
    .unwrap();
    let schema: serde_json::Value = serde_json::from_str(SIGNAL_REPORT_SCHEMA_JSON).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let document = serde_json::to_value(report).unwrap();
    assert!(validator.is_valid(&document));

    let example: serde_json::Value = serde_json::from_str(SIGNAL_REPORT_EXAMPLE_JSON).unwrap();
    assert!(
        validator.is_valid(&example),
        "the checked-in public report fixture must satisfy the closed schema"
    );

    let mut no_callable_or_signature_match = document.clone();
    no_callable_or_signature_match["candidates"][0]["matched_callable"] = serde_json::json!([]);
    no_callable_or_signature_match["candidates"][0]["matched_signature"] = serde_json::json!([]);
    assert!(
        !validator.is_valid(&no_callable_or_signature_match),
        "a candidate must match at least one callable or signature term"
    );

    let mut one_operation_match = document.clone();
    one_operation_match["candidates"][0]["matched_operation"] = serde_json::json!(["parse"]);
    assert!(
        !validator.is_valid(&one_operation_match),
        "a candidate must preserve the declared minimum of two operation matches"
    );

    let mut below_jaccard_threshold = document.clone();
    below_jaccard_threshold["candidates"][0]["operation_jaccard_ppm"] = serde_json::json!(599_999);
    assert!(
        !validator.is_valid(&below_jaccard_threshold),
        "a candidate below the declared 600,000 ppm boundary must be rejected"
    );

    let mut missing_selection_threshold = document.clone();
    missing_selection_threshold["selection"]
        .as_object_mut()
        .unwrap()
        .remove("minimum_operation_jaccard_ppm");
    assert!(!validator.is_valid(&missing_selection_threshold));

    let mut missing_candidate_measurement = document.clone();
    missing_candidate_measurement["candidates"][0]
        .as_object_mut()
        .unwrap()
        .remove("operation_jaccard_ppm");
    assert!(!validator.is_valid(&missing_candidate_measurement));

    let mut accepted = document.clone();
    accepted["authority"]["accepted"] = serde_json::json!(true);
    assert!(!validator.is_valid(&accepted));

    let mut extra = document;
    extra["accepted"] = serde_json::json!(true);
    assert!(!validator.is_valid(&extra));
}
