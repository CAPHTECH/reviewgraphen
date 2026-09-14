use reviewgraphen_core::profile::{
    CHANGED_PUBLIC_CALLEE_RULE, CandidateClassification, CandidateEndpoint, Category,
    D_EXCLUDED_WEIGHT, D_OBLIGATION_WEIGHT, DExclusionBindings, DExclusionCandidate,
    DExclusionRecord, FrozenStage0ClusterSet, InvalidPathReason, ProfileError,
    ProfileExclusionCandidate, Stage0ApplicableObligation, Stage0Cluster, Stage0DeferralReason,
    Stage0DeferredObligation, deferred_fraction_gate, evaluate_stage0_gates, rust_production_v1,
    rust_production_v2,
};
use reviewgraphen_core::{ContentHash, StableId};
use std::collections::BTreeSet;

// Intentionally independent from `src/profile.rs`: a changed implementation
// constant must not make this golden assertion pass circularly.
const PROFILE_HASH_GOLDEN: &str =
    "sha256:4b6cca93794ab03b1576e17d2e395ec43f731d316685247a89363ae2e840dd96";
const D_EXCLUSION_CANONICAL_GOLDEN: &[u8] = br#"{"candidate_key":"relation.changed_public_callee@1|relation:call","excluded_weight":"4.0","id":"exclusion:sha256:3bffacce427f6c28eb87ee964b14fed7de6a75526668897eac4855e7cbf0b461","matcher_id":"path.generated_component@1","profile_hash":"sha256:4b6cca93794ab03b1576e17d2e395ec43f731d316685247a89363ae2e840dd96","profile_id":"rust.production.v1","reason_id":"profile.exclude.generated@1","rule":"relation.changed_public_callee@1","snapshot_id":"snapshot:target","source_ids":["artifact:change","artifact:containment","relation:call","symbol:callee","symbol:caller"]}"#;
const D_EXCLUSION_CANONICAL_HASH_GOLDEN: &str =
    "sha256:8e42f5ec003873a589fc192c216d67078f09cf837b8965611e4765edc673063a";

fn id(value: &str) -> StableId {
    StableId::parse(value).unwrap()
}

#[test]
fn production_v2_excludes_benchmark_trees_without_changing_v1() {
    let path = "benchmarks/trial/src/lib.rs";
    assert!(
        rust_production_v1()
            .classify_path(path)
            .unwrap()
            .is_production_rust_source()
    );
    let v2 = rust_production_v2().classify_path(path).unwrap();
    assert!(!v2.is_production_rust_source());
    assert_eq!(v2.matched().unwrap().category(), Category::Test);
    assert_eq!(rust_production_v2().id(), "rust.production.v2");
    assert_eq!(
        rust_production_v2().hash().as_str(),
        "sha256:6f4ec559f8963f0abcc2a78718224fa0c1bd7c25c02302eb8066a7dd3915239d"
    );
}

fn excluded_match() -> reviewgraphen_core::profile::ProfileMatch {
    let profile = rust_production_v1();
    match profile
        .classify_candidate(Some(b"src/generated/lib.rs"), Some(b"src/lib.rs"))
        .unwrap()
    {
        CandidateClassification::Excluded(profile_match) => profile_match,
        CandidateClassification::Included => panic!("fixture must match"),
    }
}

fn record() -> DExclusionRecord {
    rust_production_v1()
        .exclusion_record(
            DExclusionCandidate {
                snapshot_id: id("snapshot:target"),
                relation_id: id("relation:call"),
                caller_id: id("symbol:caller"),
                callee_id: id("symbol:callee"),
                change_artifact_ids: BTreeSet::from([id("artifact:change")]),
                containment_witness_ids: BTreeSet::from([id("artifact:containment")]),
            },
            &excluded_match(),
        )
        .unwrap()
}

#[test]
fn rule_neutral_exclusion_preserves_the_legacy_d_identity_preimage() {
    let profile = rust_production_v1();
    let profile_match = excluded_match();
    let snapshot_id = id("snapshot:target");
    let relation_id = id("relation:call");
    let caller_id = id("symbol:caller");
    let callee_id = id("symbol:callee");
    let change_artifact_ids = BTreeSet::from([id("artifact:change")]);
    let containment_witness_ids = BTreeSet::from([id("artifact:containment")]);
    let legacy = profile
        .exclusion_record(
            DExclusionCandidate {
                snapshot_id: snapshot_id.clone(),
                relation_id: relation_id.clone(),
                caller_id: caller_id.clone(),
                callee_id: callee_id.clone(),
                change_artifact_ids: change_artifact_ids.clone(),
                containment_witness_ids: containment_witness_ids.clone(),
            },
            &profile_match,
        )
        .unwrap();
    let rule_neutral = profile
        .exclusion_record_for_candidate(
            ProfileExclusionCandidate::ChangedPublicCallee {
                snapshot_id,
                relation_id,
                caller_id,
                callee_id,
                change_artifact_ids,
                containment_witness_ids,
            },
            &profile_match,
        )
        .unwrap();

    assert_eq!(rule_neutral.id, legacy.id);
    assert_eq!(rule_neutral.snapshot_id, legacy.snapshot_id);
    assert_eq!(rule_neutral.candidate_key, legacy.candidate_key);
    assert_eq!(rule_neutral.rule, legacy.rule);
    assert_eq!(rule_neutral.profile_id, legacy.profile_id);
    assert_eq!(rule_neutral.profile_hash, legacy.profile_hash);
    assert_eq!(rule_neutral.reason_id, legacy.reason_id);
    assert_eq!(rule_neutral.matcher_id, legacy.matcher_id);
    assert_eq!(rule_neutral.excluded_weight, legacy.excluded_weight);
    assert_eq!(rule_neutral.source_ids, legacy.source_ids);
}

#[test]
fn canonical_profile_is_the_schema_valid_golden_value() {
    let profile = rust_production_v1();
    let example = include_bytes!("../../../schemas/reviewgraphen.review_profile.v1.example.json");
    assert_eq!(profile.canonical_bytes(), example);
    assert_eq!(profile.hash().as_str(), PROFILE_HASH_GOLDEN);
    assert_eq!(
        profile.hash(),
        ContentHash::parse(PROFILE_HASH_GOLDEN).unwrap()
    );
    assert!(reviewgraphen_core::profile::ReviewProfile::from_canonical_bytes(example).is_ok());
    let mut with_trailing_newline = example.to_vec();
    with_trailing_newline.push(b'\n');
    assert!(
        reviewgraphen_core::profile::ReviewProfile::from_canonical_bytes(&with_trailing_newline)
            .is_err()
    );

    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../../schemas/reviewgraphen.review_profile.v1.schema.json"
    ))
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let dto: serde_json::Value = serde_json::from_slice(example).unwrap();
    assert!(validator.is_valid(&dto));
    let mut wrong_dto = dto;
    wrong_dto["id"] = serde_json::Value::String("rust.other.v1".to_owned());
    assert!(!validator.is_valid(&wrong_dto));
}

#[test]
fn every_matcher_and_precedence_overlap_is_deterministic() {
    let profile = rust_production_v1();
    let cases = [
        ("vendor/lib.rs", "path.vendor_component@1", Category::Vendor),
        (
            "third_party/lib.rs",
            "path.vendor_component@1",
            Category::Vendor,
        ),
        (
            "vendored/lib.rs",
            "path.vendor_component@1",
            Category::Vendor,
        ),
        (
            "generated/lib.rs",
            "path.generated_component@1",
            Category::Generated,
        ),
        (
            "target/lib.rs",
            "path.generated_component@1",
            Category::Generated,
        ),
        (
            "src/lib.generated.rs",
            "path.generated_suffix@1",
            Category::Generated,
        ),
        ("tests/lib.rs", "path.test_component@1", Category::Test),
        ("benches/lib.rs", "path.test_component@1", Category::Test),
        ("src/test.rs", "path.test_basename@1", Category::Test),
        ("src/tests.rs", "path.test_basename@1", Category::Test),
        ("src/lib_test.rs", "path.test_suffix@1", Category::Test),
        ("src/lib_tests.rs", "path.test_suffix@1", Category::Test),
        (
            "example/lib.rs",
            "path.example_component@1",
            Category::Example,
        ),
        (
            "examples/lib.rs",
            "path.example_component@1",
            Category::Example,
        ),
        ("doc/lib.rs", "path.docs_component@1", Category::Docs),
        ("docs/lib.rs", "path.docs_component@1", Category::Docs),
        // Category precedence beats matcher array position.
        (
            "vendor/generated/tests/example/doc/lib.rs",
            "path.vendor_component@1",
            Category::Vendor,
        ),
        // Matcher array order beats a later matcher in the same category.
        (
            "target/lib.generated.rs",
            "path.generated_component@1",
            Category::Generated,
        ),
        ("tests/test.rs", "path.test_component@1", Category::Test),
    ];
    for (path, matcher_id, category) in cases {
        let classification = profile.classify_path(path).unwrap();
        let matched = classification.matched().unwrap();
        assert_eq!(matched.matcher_id(), matcher_id, "{path}");
        assert_eq!(matched.category(), category, "{path}");
    }
    assert!(
        profile
            .classify_path("src/lib.rs")
            .unwrap()
            .is_production_rust_source()
    );
    assert!(
        profile
            .classify_path("example/lib.rs")
            .unwrap()
            .is_non_test_rust_source()
    );
    assert!(
        !profile
            .classify_path("tests/lib.rs")
            .unwrap()
            .is_non_test_rust_source()
    );
}

#[test]
fn candidate_tie_breaks_callee_before_caller_and_invalid_paths_are_typed() {
    let profile = rust_production_v1();
    match profile
        .classify_candidate(
            Some(b"src/generated/lib.rs"),
            Some(b"src/generated/main.rs"),
        )
        .unwrap()
    {
        CandidateClassification::Excluded(profile_match) => {
            assert_eq!(profile_match.endpoint(), CandidateEndpoint::Callee);
            assert_eq!(profile_match.matcher_id(), "path.generated_component@1");
        }
        CandidateClassification::Included => panic!("fixture must exclude"),
    }
    for (path, reason) in [
        (b"".as_slice(), InvalidPathReason::Empty),
        (b"src\\lib.rs".as_slice(), InvalidPathReason::Backslash),
        (b"/src/lib.rs".as_slice(), InvalidPathReason::Absolute),
        (b"src//lib.rs".as_slice(), InvalidPathReason::EmptyComponent),
        (b"src/./lib.rs".as_slice(), InvalidPathReason::Dot),
        (b"src/../lib.rs".as_slice(), InvalidPathReason::DotDot),
        (b"src\0lib.rs".as_slice(), InvalidPathReason::Nul),
        (&[0xff], InvalidPathReason::InvalidUtf8),
    ] {
        assert_eq!(
            profile.classify_candidate(Some(path), Some(b"src/lib.rs")),
            Err(ProfileError::InvalidPath {
                endpoint: "callee",
                reason
            })
        );
    }
    assert_eq!(
        profile.classify_candidate(None, Some(b"src/lib.rs")),
        Err(ProfileError::MissingPath { endpoint: "callee" })
    );
}

#[test]
fn exclusion_identity_binds_every_required_field() {
    let original = record();
    let canonical = reviewgraphen_core::canonical_json(&original).unwrap();
    assert_eq!(canonical, D_EXCLUSION_CANONICAL_GOLDEN);
    assert_eq!(
        ContentHash::sha256(&canonical).as_str(),
        D_EXCLUSION_CANONICAL_HASH_GOLDEN
    );
    original.validate().unwrap();

    let bindings = DExclusionBindings {
        snapshot_id: original.snapshot_id.clone(),
        candidate_key: original.candidate_key.clone(),
        rule: original.rule.clone(),
        profile_id: original.profile_id.clone(),
        profile_hash: original.profile_hash.clone(),
        reason_id: original.reason_id.clone(),
        matcher_id: original.matcher_id.clone(),
        excluded_weight: original.excluded_weight.clone(),
        source_ids: original.source_ids.clone(),
    };
    assert_eq!(original.excluded_weight, D_EXCLUDED_WEIGHT);

    let mut candidate = bindings.clone();
    candidate.candidate_key = format!("{CHANGED_PUBLIC_CALLEE_RULE}|relation:other");
    assert_ne!(DExclusionRecord::new(candidate).unwrap().id, original.id);

    let mut matcher = bindings.clone();
    matcher.matcher_id = "path.generated_suffix@1".to_owned();
    assert_ne!(DExclusionRecord::new(matcher).unwrap().id, original.id);

    let mut source = bindings.clone();
    source.source_ids.insert(id("artifact:another"));
    assert_ne!(DExclusionRecord::new(source).unwrap().id, original.id);

    for invalid in [
        {
            let mut value = bindings.clone();
            value.profile_id = "rust.other.v1".to_owned();
            value
        },
        {
            let mut value = bindings.clone();
            value.profile_hash = ContentHash::sha256(b"other");
            value
        },
        {
            let mut value = bindings.clone();
            value.rule = "relation.other@1".to_owned();
            value
        },
        {
            let mut value = bindings.clone();
            value.reason_id = "profile.exclude.vendor@1".to_owned();
            value
        },
        {
            let mut value = bindings.clone();
            value.excluded_weight = "4.1".to_owned();
            value
        },
    ] {
        assert!(DExclusionRecord::new(invalid).is_err());
    }

    let rebuilt = DExclusionRecord::new(bindings).unwrap();
    assert_eq!(rebuilt, original);
    assert_eq!(
        rebuilt.source_ids,
        BTreeSet::from([
            id("artifact:change"),
            id("artifact:containment"),
            id("relation:call"),
            id("symbol:callee"),
            id("symbol:caller"),
        ])
    );
}

fn assert_path_match(path: &str, matcher_id: &str, category: Category) {
    let matched = rust_production_v1()
        .classify_path(path)
        .unwrap()
        .matched()
        .unwrap();
    assert_eq!(matched.matcher_id(), matcher_id);
    assert_eq!(matched.category(), category);
}

#[test]
fn matcher_vendor_component_is_exact() {
    assert_path_match("vendor/lib.rs", "path.vendor_component@1", Category::Vendor);
}

#[test]
fn matcher_generated_component_is_exact() {
    assert_path_match(
        "target/lib.rs",
        "path.generated_component@1",
        Category::Generated,
    );
}

#[test]
fn matcher_generated_suffix_is_exact() {
    assert_path_match(
        "src/lib.generated.rs",
        "path.generated_suffix@1",
        Category::Generated,
    );
}

#[test]
fn matcher_test_component_is_exact() {
    assert_path_match("benches/lib.rs", "path.test_component@1", Category::Test);
}

#[test]
fn matcher_test_basename_is_exact() {
    assert_path_match("src/tests.rs", "path.test_basename@1", Category::Test);
}

#[test]
fn matcher_test_suffix_is_exact() {
    assert_path_match("src/lib_tests.rs", "path.test_suffix@1", Category::Test);
}

#[test]
fn matcher_example_component_is_exact() {
    assert_path_match(
        "examples/lib.rs",
        "path.example_component@1",
        Category::Example,
    );
}

#[test]
fn matcher_docs_component_is_exact() {
    assert_path_match("docs/lib.rs", "path.docs_component@1", Category::Docs);
}

#[test]
fn precedence_uses_category_before_matcher_position() {
    assert_path_match(
        "vendor/generated/tests/example/doc/lib.rs",
        "path.vendor_component@1",
        Category::Vendor,
    );
}

#[test]
fn precedence_uses_matcher_position_before_endpoint() {
    assert_path_match(
        "target/lib.generated.rs",
        "path.generated_component@1",
        Category::Generated,
    );
}

fn assert_invalid_callee(path: &[u8], reason: InvalidPathReason) {
    assert_eq!(
        rust_production_v1().classify_candidate(Some(path), Some(b"src/lib.rs")),
        Err(ProfileError::InvalidPath {
            endpoint: "callee",
            reason,
        })
    );
}

#[test]
fn invalid_path_non_utf8_is_typed() {
    assert_invalid_callee(&[0xff], InvalidPathReason::InvalidUtf8);
}

#[test]
fn invalid_path_parent_component_is_typed() {
    assert_invalid_callee(b"src/../lib.rs", InvalidPathReason::DotDot);
}

#[test]
fn invalid_path_absolute_is_typed() {
    assert_invalid_callee(b"/src/lib.rs", InvalidPathReason::Absolute);
}

#[test]
fn invalid_path_windows_separator_is_typed() {
    assert_invalid_callee(b"src\\lib.rs", InvalidPathReason::Backslash);
}

#[test]
fn invalid_path_nul_control_character_is_typed() {
    assert_invalid_callee(b"src\0lib.rs", InvalidPathReason::Nul);
}

#[test]
fn invalid_path_empty_is_typed() {
    assert_invalid_callee(b"", InvalidPathReason::Empty);
}

#[test]
fn invalid_path_empty_component_is_typed() {
    assert_invalid_callee(b"src//lib.rs", InvalidPathReason::EmptyComponent);
}

#[test]
fn extreme_length_path_is_not_silently_normalized() {
    let path = format!("src/{}.rs", "x".repeat(65_536));
    let classification = rust_production_v1().classify_path(&path).unwrap();
    assert!(classification.is_production_rust_source());
}

fn exclusion_bindings() -> DExclusionBindings {
    let original = record();
    DExclusionBindings {
        snapshot_id: original.snapshot_id,
        candidate_key: original.candidate_key,
        rule: original.rule,
        profile_id: original.profile_id,
        profile_hash: original.profile_hash,
        reason_id: original.reason_id,
        matcher_id: original.matcher_id,
        excluded_weight: original.excluded_weight,
        source_ids: original.source_ids,
    }
}

#[test]
fn exclusion_id_mutation_profile_fails_closed() {
    let mut bindings = exclusion_bindings();
    bindings.profile_id = "rust.other.v1".to_owned();
    assert!(DExclusionRecord::new(bindings).is_err());
}

#[test]
fn exclusion_id_mutation_rule_fails_closed() {
    let mut bindings = exclusion_bindings();
    bindings.rule = "relation.other@1".to_owned();
    assert!(DExclusionRecord::new(bindings).is_err());
}

#[test]
fn exclusion_id_mutation_candidate_changes_id() {
    let original = record();
    let mut bindings = exclusion_bindings();
    bindings.candidate_key = format!("{CHANGED_PUBLIC_CALLEE_RULE}|relation:other");
    assert_ne!(DExclusionRecord::new(bindings).unwrap().id, original.id);
}

#[test]
fn exclusion_id_mutation_reason_fails_closed() {
    let mut bindings = exclusion_bindings();
    bindings.reason_id = "profile.exclude.vendor@1".to_owned();
    assert!(DExclusionRecord::new(bindings).is_err());
}

#[test]
fn exclusion_id_mutation_matcher_changes_id() {
    let original = record();
    let mut bindings = exclusion_bindings();
    bindings.matcher_id = "path.generated_suffix@1".to_owned();
    assert_ne!(DExclusionRecord::new(bindings).unwrap().id, original.id);
}

#[test]
fn exclusion_id_mutation_weight_fails_closed() {
    let mut bindings = exclusion_bindings();
    bindings.excluded_weight = "4.1".to_owned();
    assert!(DExclusionRecord::new(bindings).is_err());
}

#[test]
fn exclusion_id_mutation_source_changes_id() {
    let original = record();
    let mut bindings = exclusion_bindings();
    bindings.source_ids.insert(id("artifact:another"));
    assert_ne!(DExclusionRecord::new(bindings).unwrap().id, original.id);
}

fn stage0_cluster(index: usize, applicable: usize, deferred: usize) -> Stage0Cluster {
    let applicable = (0..applicable)
        .map(|obligation| {
            (
                id(&format!("obligation:c{index:03}-{obligation:03}")),
                Stage0ApplicableObligation {
                    weight: D_OBLIGATION_WEIGHT.to_owned(),
                },
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let deferred = applicable
        .iter()
        .take(deferred)
        .map(|(id, obligation)| {
            (
                id.clone(),
                Stage0DeferredObligation {
                    weight: obligation.weight.clone(),
                    reason: Stage0DeferralReason::BudgetExhausted,
                },
            )
        })
        .collect();
    Stage0Cluster {
        id: id(&format!("cluster:c{index:03}")),
        applicable,
        deferred,
    }
}

fn stage0_clusters(applicable: usize, deferred: usize) -> Vec<Stage0Cluster> {
    let mut clusters = (0..300)
        .map(|index| stage0_cluster(index, 0, 0))
        .collect::<Vec<_>>();
    clusters[0] = stage0_cluster(0, applicable, deferred);
    clusters
}

fn evaluate_stage0(clusters: Vec<Stage0Cluster>) -> reviewgraphen_core::profile::Stage0Gates {
    let frozen =
        FrozenStage0ClusterSet::new(clusters.iter().map(|cluster| cluster.id.clone()).collect())
            .unwrap();
    evaluate_stage0_gates(frozen, clusters).unwrap()
}

#[test]
fn planning_retains_deferred_ids_in_the_applicable_denominator() {
    let gates = evaluate_stage0(stage0_clusters(2, 1));
    assert_eq!(gates.applicable_obligation_ids.len(), 2);
    assert_eq!(gates.deferred_obligation_ids.len(), 1);
    assert!(
        gates
            .deferred_obligation_ids
            .is_subset(&gates.applicable_obligation_ids)
    );
    let deferred = gates.clusters[&id("cluster:c000")]
        .deferred
        .get(&id("obligation:c000-000"))
        .unwrap();
    assert_eq!(deferred.weight, D_OBLIGATION_WEIGHT);
    assert_eq!(deferred.reason, Stage0DeferralReason::BudgetExhausted);
}

#[test]
fn stage0_canonical_artifact_retains_c_every_a_c_and_every_d_c() {
    let gates = evaluate_stage0(stage0_clusters(2, 1));
    let bytes = gates.canonical_bytes().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["cluster_ids"].as_array().unwrap().len(), 300);
    let cluster = &value["clusters"]["cluster:c000"];
    assert!(cluster["applicable"].get("obligation:c000-000").is_some());
    assert!(cluster["deferred"].get("obligation:c000-000").is_some());
    assert_eq!(
        cluster["deferred"]["obligation:c000-000"]["weight"],
        D_OBLIGATION_WEIGHT
    );
    assert_eq!(
        cluster["deferred"]["obligation:c000-000"]["reason"],
        "budget_exhausted"
    );
    assert_eq!(gates.canonical_hash().unwrap(), ContentHash::sha256(&bytes));
}

#[test]
fn stage0_canonical_artifact_is_independent_of_cluster_input_order() {
    let clusters = stage0_clusters(2, 1);
    let frozen =
        FrozenStage0ClusterSet::new(clusters.iter().map(|cluster| cluster.id.clone()).collect())
            .unwrap();
    let forward = evaluate_stage0_gates(frozen.clone(), clusters.clone()).unwrap();
    let mut reversed = clusters;
    reversed.reverse();
    let backward = evaluate_stage0_gates(frozen, reversed).unwrap();
    assert_eq!(
        forward.canonical_bytes().unwrap(),
        backward.canonical_bytes().unwrap()
    );
}

#[test]
fn stage0_frozen_cluster_domain_rejects_wrong_namespace() {
    let mut ids = stage0_clusters(0, 0)
        .into_iter()
        .map(|cluster| cluster.id)
        .collect::<Vec<_>>();
    ids[0] = id("relation:not-a-cluster");
    assert!(FrozenStage0ClusterSet::new(ids).is_err());
}

#[test]
fn stage0_deferred_prerequisite_reason_is_retained() {
    let mut clusters = stage0_clusters(1, 1);
    clusters[0]
        .deferred
        .get_mut(&id("obligation:c000-000"))
        .unwrap()
        .reason = Stage0DeferralReason::PrerequisiteDeferred;
    let gates = evaluate_stage0(clusters);
    assert_eq!(
        gates.clusters[&id("cluster:c000")].deferred[&id("obligation:c000-000")].reason,
        Stage0DeferralReason::PrerequisiteDeferred
    );
}

#[test]
fn stage0_deferred_fraction_overflow_is_typed() {
    assert!(deferred_fraction_gate(usize::MAX, usize::MAX).is_err());
}

#[test]
fn stage0_fan_out_p95_exact_boundary_passes() {
    let mut clusters = stage0_clusters(0, 0);
    for (index, cluster) in clusters.iter_mut().enumerate().skip(284) {
        *cluster = stage0_cluster(index, 50, 0);
    }
    let gates = evaluate_stage0(clusters);
    assert_eq!(gates.p95_rank, 285);
    assert_eq!(gates.p95_count, 50);
    assert!(gates.fan_out_passes);
}

#[test]
fn stage0_fan_out_p95_plus_one_fails() {
    let mut clusters = stage0_clusters(0, 0);
    for (index, cluster) in clusters.iter_mut().enumerate().skip(284) {
        *cluster = stage0_cluster(index, 51, 0);
    }
    let gates = evaluate_stage0(clusters);
    assert_eq!(gates.p95_count, 51);
    assert!(!gates.fan_out_passes);
}

#[test]
fn stage0_deferred_fraction_exact_boundary_passes() {
    let gates = evaluate_stage0(stage0_clusters(20, 1));
    assert!(gates.deferred_fraction_passes);
}

#[test]
fn stage0_deferred_fraction_smallest_plus_one_fails() {
    let gates = evaluate_stage0(stage0_clusters(19, 1));
    assert!(!gates.deferred_fraction_passes);
}

#[test]
fn stage0_zero_obligation_clusters_are_retained_and_pass_both_gates() {
    let gates = evaluate_stage0(stage0_clusters(0, 0));
    assert_eq!(gates.cluster_ids.len(), 300);
    assert_eq!(gates.fan_out_counts.len(), 300);
    assert_eq!(gates.p95_count, 0);
    assert!(gates.fan_out_passes);
    assert!(gates.deferred_fraction_passes);
}
