use reviewgraphen_core::{
    ContentHash, ContextSubjectWindowsPolicyV2, ContextSubjectWindowsPolicyV3,
    ContextWindowInputV2, ContextWindowLossReasonV2, ContextWindowRoleV2, Severity, StableId,
    resolve_subject_windows_v2,
};

const ADR0038_SUBJECT_WINDOWS_V2_HASH: &str =
    "sha256:7c4ceca165588cd38b28cc6882bb68a1ff4040dbb19ee34dfd792216d70bbf26";
const ADR0038_SUBJECT_WINDOWS_V2_BYTES: &[u8] = br#"{"anchors_per_file":1024,"assumptions":"empty","callees_depth":3,"callers_depth":2,"candidate_order":["subject_priority","distance","path_rank","artifact_id"],"canonical_envelope_bytes":786432,"contains_edges":1000000,"discovery_paths":20,"edge_kind_direction_order":["calls:forward","calls:reverse","contains:forward","contains:reverse","covers:forward","covers:reverse"],"excerpt_lines":400,"final_window_order":["source_artifact_id","start_line","end_line","window_id"],"included_files":64,"loss_reason_precedence":["missing_location","missing_source","giant_line","per_window_lines","per_window_bytes","per_file_window_cap","total_window_cap","total_excerpt_bytes","overlap_unmergeable","path_cap","test_cap","not_reached","included_file_cap","artifact_bytes_cap","total_resolved_bytes_cap"],"max_assumptions":64,"max_candidates":4096,"max_discovered_structural_ids":4096,"max_excerpt_bytes":262144,"max_losses":64,"max_resolved_artifact_bytes":1048576,"max_resolved_bytes":8388608,"max_string_bytes":16384,"max_total_excerpt_bytes":1048576,"max_unknowns":64,"obligations_per_envelope":1,"policy_id":"context.subject_windows@2","related_tests":10,"relation_scan":1000000,"seed_fields":["source_ids","target_refs","context_ids"],"source_candidate_denominator":"all_accepted_file_artifacts_with_exact_snapshot_source_registration_closure","subject_endpoints":2,"subject_order":["callee","caller"],"support_anchor_denominator":"reached_range_bearing_accepted_artifacts_with_exact_path_reverse_contains_file","unknown_reason_ids":["unresolved_invariant_scope","unresolved_relation_endpoint","unresolved_review_context_member","unresolved_seed_reference"],"window_candidate_order":["priority","role","source_artifact_id","start_line","end_line","owner_id"],"window_merge":"same_source_overlap_or_adjacent_if_union_within_per_window_bounds","windows_per_envelope":8,"windows_per_file":4}"#;
const ADR0038_SUBJECT_WINDOWS_V3_HASH: &str =
    "sha256:932bfa18c5d286c63196366d6d2dc1aaf402f50baa1f1ab5f075b1007be55dd8";
const ADR0038_SUBJECT_WINDOWS_V3_BYTES: &[u8] = br#"{"accepted_file_denominator_bound":"request.ingest.max_files","anchors_per_file":1024,"assumptions":"empty","callees_depth":3,"callers_depth":2,"candidate_order":["subject_priority","distance","path_rank","artifact_id"],"canonical_envelope_bytes":786432,"contains_edges":1000000,"discovery_paths":20,"edge_kind_direction_order":["calls:forward","calls:reverse","contains:forward","contains:reverse","covers:forward","covers:reverse"],"excerpt_lines":400,"final_window_order":["source_artifact_id","start_line","end_line","window_id"],"included_files":64,"latent_cardinality":"known_zero_or_unknown_with_qualification_ids","loss_reason_precedence":["missing_location","missing_source","giant_line","per_window_lines","per_window_bytes","per_file_window_cap","total_window_cap","total_excerpt_bytes","overlap_unmergeable","path_cap","test_cap","not_reached","included_file_cap","artifact_bytes_cap","total_resolved_bytes_cap"],"materialized_source_denominator":"subject_file_ids_union_reached_file_ids","max_assumptions":64,"max_discovered_structural_ids":4096,"max_excerpt_bytes":262144,"max_materialized_source_candidates":4096,"max_resolved_artifact_bytes":1048576,"max_resolved_bytes":8388608,"max_string_bytes":16384,"max_subject_losses":2,"max_support_loss_summaries":15,"max_total_excerpt_bytes":1048576,"max_unknowns":64,"obligations_per_envelope":1,"policy_id":"context.subject_windows@3","related_tests":10,"relation_scan":1000000,"seed_fields":["source_ids","target_refs","context_ids"],"source_candidate_denominator":"all_accepted_file_ids_known_count_and_sorted_id_set_sha256","subject_endpoints":2,"subject_order":["callee","caller"],"support_anchor_denominator":"reached_range_bearing_exact_path_anchor_ids_known_count_and_sorted_id_set_sha256","support_loss_summary":"reason_known_count_and_sorted_anchor_id_set_sha256","unknown_reason_ids":["unresolved_invariant_scope","unresolved_relation_endpoint","unresolved_review_context_member","unresolved_seed_reference"],"window_candidate_order":["priority","role","source_artifact_id","start_line","end_line","owner_id"],"window_merge":"same_source_overlap_or_adjacent_if_union_within_per_window_bounds","windows_per_envelope":8,"windows_per_file":4}"#;

#[test]
fn subject_windows_policy_v2_has_the_adr_golden_bytes_and_hash() {
    let policy = ContextSubjectWindowsPolicyV2::fixed();
    let bytes = policy
        .canonical_bytes()
        .expect("fixed policy is serializable");
    assert_eq!(bytes, ADR0038_SUBJECT_WINDOWS_V2_BYTES);
    assert_eq!(
        ContentHash::sha256(&bytes).as_str(),
        ADR0038_SUBJECT_WINDOWS_V2_HASH
    );
    assert_eq!(policy.hash().as_str(), ADR0038_SUBJECT_WINDOWS_V2_HASH);
    assert_eq!(
        ContextSubjectWindowsPolicyV2::GOLDEN_HASH,
        ADR0038_SUBJECT_WINDOWS_V2_HASH
    );
    let decoded: ContextSubjectWindowsPolicyV2 =
        serde_json::from_slice(ADR0038_SUBJECT_WINDOWS_V2_BYTES).expect("exact DTO decodes");
    assert_eq!(decoded, policy);
    let mut mutated: serde_json::Value =
        serde_json::from_slice(ADR0038_SUBJECT_WINDOWS_V2_BYTES).expect("fixture JSON");
    mutated["excerpt_lines"] = serde_json::json!(399);
    assert!(serde_json::from_value::<ContextSubjectWindowsPolicyV2>(mutated).is_err());
    let duplicated = std::str::from_utf8(ADR0038_SUBJECT_WINDOWS_V2_BYTES)
        .expect("UTF-8 fixture")
        .replacen(
            "{\"anchors_per_file\":1024,",
            "{\"anchors_per_file\":1024,\"anchors_per_file\":1024,",
            1,
        );
    assert!(serde_json::from_str::<ContextSubjectWindowsPolicyV2>(&duplicated).is_err());
}

#[test]
fn subject_windows_policy_v3_is_golden_and_cross_decode_is_forbidden() {
    let policy = ContextSubjectWindowsPolicyV3::fixed();
    let bytes = policy.canonical_bytes().expect("fixed v3 policy");
    assert_eq!(bytes, ADR0038_SUBJECT_WINDOWS_V3_BYTES);
    assert_eq!(
        ContentHash::sha256(&bytes).as_str(),
        ADR0038_SUBJECT_WINDOWS_V3_HASH
    );
    assert_eq!(policy.hash().as_str(), ADR0038_SUBJECT_WINDOWS_V3_HASH);
    assert_eq!(
        ContextSubjectWindowsPolicyV3::GOLDEN_HASH,
        ADR0038_SUBJECT_WINDOWS_V3_HASH
    );
    assert_eq!(
        serde_json::from_slice::<ContextSubjectWindowsPolicyV3>(ADR0038_SUBJECT_WINDOWS_V3_BYTES)
            .expect("v3 decodes"),
        policy
    );
    assert!(
        serde_json::from_slice::<ContextSubjectWindowsPolicyV2>(ADR0038_SUBJECT_WINDOWS_V3_BYTES)
            .is_err()
    );
    assert!(
        serde_json::from_slice::<ContextSubjectWindowsPolicyV3>(ADR0038_SUBJECT_WINDOWS_V2_BYTES)
            .is_err()
    );
}

fn id(value: &str) -> StableId {
    StableId::parse(value.to_owned()).expect("stable fixture ID")
}

#[test]
fn resolver_merges_adjacent_subject_windows_but_keeps_disjoint_windows() {
    let bytes = b"one\ntwo\nthree\nfour\nfive\n";
    let hash = ContentHash::sha256(bytes);
    let inputs = [
        ContextWindowInputV2 {
            source_artifact_id: id("file:one"),
            registration_id: id("registration:one"),
            content_hash: hash.clone(),
            cas_hash: hash.clone(),
            bytes,
            start_line: 2,
            end_line: 2,
            owner_id: id("function:callee"),
            role: ContextWindowRoleV2::Callee,
        },
        ContextWindowInputV2 {
            source_artifact_id: id("file:one"),
            registration_id: id("registration:one"),
            content_hash: hash.clone(),
            cas_hash: hash.clone(),
            bytes,
            start_line: 3,
            end_line: 3,
            owner_id: id("function:caller"),
            role: ContextWindowRoleV2::Caller,
        },
        ContextWindowInputV2 {
            source_artifact_id: id("file:one"),
            registration_id: id("registration:one"),
            content_hash: hash.clone(),
            cas_hash: hash.clone(),
            bytes,
            start_line: 5,
            end_line: 5,
            owner_id: id("function:support"),
            role: ContextWindowRoleV2::Support,
        },
    ];
    let (windows, losses) =
        resolve_subject_windows_v2(&id("snapshot:one"), &id("obligation:one"), &inputs)
            .expect("valid resolver input");
    assert!(losses.is_empty());
    assert_eq!(windows.len(), 2);
    assert_eq!(
        (
            windows[0].range().start_line(),
            windows[0].range().end_line()
        ),
        (2, 3)
    );
    assert_eq!(windows[0].owner_ids().len(), 2);
    assert_eq!(
        (
            windows[1].range().start_line(),
            windows[1].range().end_line()
        ),
        (5, 5)
    );
    assert_ne!(windows[0].id(), windows[1].id());
}

fn line_bytes(lines: usize) -> Vec<u8> {
    (0..lines)
        .flat_map(|line| format!("{line:04}\n").into_bytes())
        .collect()
}

#[test]
fn subjects_after_line_400_are_not_displaced_by_lower_support() {
    let bytes = line_bytes(900);
    let hash = ContentHash::sha256(&bytes);
    let inputs = [
        ContextWindowInputV2 {
            source_artifact_id: id("file:one"),
            registration_id: id("registration:one"),
            content_hash: hash.clone(),
            cas_hash: hash.clone(),
            bytes: &bytes,
            start_line: 1,
            end_line: 400,
            owner_id: id("function:support"),
            role: ContextWindowRoleV2::Support,
        },
        ContextWindowInputV2 {
            source_artifact_id: id("file:one"),
            registration_id: id("registration:one"),
            content_hash: hash.clone(),
            cas_hash: hash.clone(),
            bytes: &bytes,
            start_line: 700,
            end_line: 710,
            owner_id: id("function:caller"),
            role: ContextWindowRoleV2::Caller,
        },
        ContextWindowInputV2 {
            source_artifact_id: id("file:one"),
            registration_id: id("registration:one"),
            content_hash: hash.clone(),
            cas_hash: hash,
            bytes: &bytes,
            start_line: 500,
            end_line: 510,
            owner_id: id("function:callee"),
            role: ContextWindowRoleV2::Callee,
        },
    ];
    let (windows, losses) =
        resolve_subject_windows_v2(&id("snapshot:one"), &id("obligation:one"), &inputs)
            .expect("subject-first resolution");
    assert!(losses.is_empty());
    assert_eq!(windows.len(), 3);
    assert!(windows.iter().any(|window| {
        window.roles().contains(&ContextWindowRoleV2::Callee) && window.range().start_line() == 500
    }));
    assert!(windows.iter().any(|window| {
        window.roles().contains(&ContextWindowRoleV2::Caller) && window.range().start_line() == 700
    }));
}

#[test]
fn overlap_that_cannot_merge_keeps_callee_and_records_caller_loss() {
    let bytes = line_bytes(500);
    let hash = ContentHash::sha256(&bytes);
    let inputs = [
        ContextWindowInputV2 {
            source_artifact_id: id("file:one"),
            registration_id: id("registration:one"),
            content_hash: hash.clone(),
            cas_hash: hash.clone(),
            bytes: &bytes,
            start_line: 1,
            end_line: 400,
            owner_id: id("function:callee"),
            role: ContextWindowRoleV2::Callee,
        },
        ContextWindowInputV2 {
            source_artifact_id: id("file:one"),
            registration_id: id("registration:one"),
            content_hash: hash.clone(),
            cas_hash: hash,
            bytes: &bytes,
            start_line: 400,
            end_line: 401,
            owner_id: id("function:caller"),
            role: ContextWindowRoleV2::Caller,
        },
    ];
    let (windows, losses) =
        resolve_subject_windows_v2(&id("snapshot:one"), &id("obligation:one"), &inputs)
            .expect("typed non-merge");
    assert_eq!(windows.len(), 1);
    assert_eq!(
        (
            windows[0].range().start_line(),
            windows[0].range().end_line()
        ),
        (1, 400)
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].reason(),
        ContextWindowLossReasonV2::OverlapUnmergeable
    );
    assert_eq!(losses[0].severity(), Severity::High);
    assert_eq!(losses[0].endpoint_id(), &id("function:caller"));
}

#[test]
fn line_cap_is_inclusive_and_plus_one_is_typed() {
    let bytes = line_bytes(401);
    let hash = ContentHash::sha256(&bytes);
    let exact = ContextWindowInputV2 {
        source_artifact_id: id("file:exact"),
        registration_id: id("registration:exact"),
        content_hash: hash.clone(),
        cas_hash: hash.clone(),
        bytes: &bytes,
        start_line: 1,
        end_line: 400,
        owner_id: id("function:exact"),
        role: ContextWindowRoleV2::Callee,
    };
    let plus_one = ContextWindowInputV2 {
        source_artifact_id: id("file:plus-one"),
        registration_id: id("registration:plus-one"),
        content_hash: hash.clone(),
        cas_hash: hash,
        bytes: &bytes,
        start_line: 1,
        end_line: 401,
        owner_id: id("function:plus-one"),
        role: ContextWindowRoleV2::Caller,
    };
    let (windows, losses) = resolve_subject_windows_v2(
        &id("snapshot:one"),
        &id("obligation:one"),
        &[exact, plus_one],
    )
    .expect("exact/+1 line cap");
    assert_eq!(windows.len(), 1);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].reason(),
        ContextWindowLossReasonV2::PerWindowLines
    );
}

#[test]
fn byte_cap_is_inclusive_and_giant_line_plus_one_is_typed() {
    let exact_bytes = vec![b'x'; 262_144];
    let plus_one_bytes = vec![b'y'; 262_145];
    let exact_hash = ContentHash::sha256(&exact_bytes);
    let plus_one_hash = ContentHash::sha256(&plus_one_bytes);
    let inputs = [
        ContextWindowInputV2 {
            source_artifact_id: id("file:exact"),
            registration_id: id("registration:exact"),
            content_hash: exact_hash.clone(),
            cas_hash: exact_hash,
            bytes: &exact_bytes,
            start_line: 1,
            end_line: 1,
            owner_id: id("function:exact"),
            role: ContextWindowRoleV2::Callee,
        },
        ContextWindowInputV2 {
            source_artifact_id: id("file:plus-one"),
            registration_id: id("registration:plus-one"),
            content_hash: plus_one_hash.clone(),
            cas_hash: plus_one_hash,
            bytes: &plus_one_bytes,
            start_line: 1,
            end_line: 1,
            owner_id: id("function:plus-one"),
            role: ContextWindowRoleV2::Caller,
        },
    ];
    let (windows, losses) =
        resolve_subject_windows_v2(&id("snapshot:one"), &id("obligation:one"), &inputs)
            .expect("exact/+1 byte cap");
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].excerpt_byte_length(), 262_144);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].reason(), ContextWindowLossReasonV2::GiantLine);
}

#[test]
fn per_file_and_total_window_caps_are_exact_and_support_losses_are_low() {
    let bytes = line_bytes(20);
    let hash = ContentHash::sha256(&bytes);
    let owners = (0..9)
        .map(|index| id(&format!("function:support-{index}")))
        .collect::<Vec<_>>();
    let files = (0..9)
        .map(|index| id(&format!("file:{index}")))
        .collect::<Vec<_>>();
    let registrations = (0..9)
        .map(|index| id(&format!("registration:{index}")))
        .collect::<Vec<_>>();
    let total_inputs = (0..9)
        .map(|index| ContextWindowInputV2 {
            source_artifact_id: files[index].clone(),
            registration_id: registrations[index].clone(),
            content_hash: hash.clone(),
            cas_hash: hash.clone(),
            bytes: &bytes,
            start_line: 1,
            end_line: 1,
            owner_id: owners[index].clone(),
            role: ContextWindowRoleV2::Support,
        })
        .collect::<Vec<_>>();
    let (windows, losses) =
        resolve_subject_windows_v2(&id("snapshot:one"), &id("obligation:one"), &total_inputs)
            .expect("total cap");
    assert_eq!(windows.len(), 8);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].reason(),
        ContextWindowLossReasonV2::TotalWindowCap
    );
    assert_eq!(losses[0].severity(), Severity::Low);

    let file = id("file:shared");
    let registration = id("registration:shared");
    let per_file_inputs = (0..5)
        .map(|index| ContextWindowInputV2 {
            source_artifact_id: file.clone(),
            registration_id: registration.clone(),
            content_hash: hash.clone(),
            cas_hash: hash.clone(),
            bytes: &bytes,
            start_line: 1 + index * 3,
            end_line: 1 + index * 3,
            owner_id: owners[index as usize].clone(),
            role: ContextWindowRoleV2::Support,
        })
        .collect::<Vec<_>>();
    let (windows, losses) =
        resolve_subject_windows_v2(&id("snapshot:one"), &id("obligation:two"), &per_file_inputs)
            .expect("per-file cap");
    assert_eq!(windows.len(), 4);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].reason(),
        ContextWindowLossReasonV2::PerFileWindowCap
    );
}

#[test]
fn total_excerpt_byte_cap_is_inclusive_and_plus_one_is_typed() {
    let bytes = vec![b'x'; 262_144];
    let hash = ContentHash::sha256(&bytes);
    let files = (0..5)
        .map(|index| id(&format!("file:bytes-{index}")))
        .collect::<Vec<_>>();
    let registrations = (0..5)
        .map(|index| id(&format!("registration:bytes-{index}")))
        .collect::<Vec<_>>();
    let owners = (0..5)
        .map(|index| id(&format!("function:bytes-{index}")))
        .collect::<Vec<_>>();
    let inputs = (0..5)
        .map(|index| ContextWindowInputV2 {
            source_artifact_id: files[index].clone(),
            registration_id: registrations[index].clone(),
            content_hash: hash.clone(),
            cas_hash: hash.clone(),
            bytes: &bytes,
            start_line: 1,
            end_line: 1,
            owner_id: owners[index].clone(),
            role: ContextWindowRoleV2::Support,
        })
        .collect::<Vec<_>>();
    let (windows, losses) =
        resolve_subject_windows_v2(&id("snapshot:one"), &id("obligation:one"), &inputs)
            .expect("total byte cap");
    assert_eq!(windows.len(), 4);
    assert_eq!(
        windows
            .iter()
            .map(|window| window.excerpt_byte_length())
            .sum::<u64>(),
        1_048_576
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].reason(),
        ContextWindowLossReasonV2::TotalExcerptBytes
    );
}

#[test]
fn source_hash_range_and_role_are_identity_inputs_and_bad_bytes_fail() {
    let bytes = b"one\ntwo\nthree\n";
    let hash = ContentHash::sha256(bytes);
    let make = |source: &str, start_line, role| ContextWindowInputV2 {
        source_artifact_id: id(source),
        registration_id: id("registration:one"),
        content_hash: hash.clone(),
        cas_hash: hash.clone(),
        bytes,
        start_line,
        end_line: start_line,
        owner_id: id("function:one"),
        role,
    };
    let window_id = |input| {
        resolve_subject_windows_v2(&id("snapshot:one"), &id("obligation:one"), &[input])
            .expect("valid identity variant")
            .0
            .remove(0)
            .id()
            .clone()
    };
    let baseline = window_id(make("file:one", 1, ContextWindowRoleV2::Callee));
    assert_ne!(
        baseline,
        window_id(make("file:two", 1, ContextWindowRoleV2::Callee))
    );
    assert_ne!(
        baseline,
        window_id(make("file:one", 2, ContextWindowRoleV2::Callee))
    );
    assert_ne!(
        baseline,
        window_id(make("file:one", 1, ContextWindowRoleV2::Caller))
    );

    let bad = ContextWindowInputV2 {
        content_hash: ContentHash::sha256(b"different"),
        ..make("file:one", 1, ContextWindowRoleV2::Callee)
    };
    assert!(
        resolve_subject_windows_v2(&id("snapshot:one"), &id("obligation:one"), &[bad],).is_err()
    );

    let one = make("file:one", 1, ContextWindowRoleV2::Callee);
    let inconsistent = ContextWindowInputV2 {
        registration_id: id("registration:different"),
        ..make("file:one", 2, ContextWindowRoleV2::Caller)
    };
    assert!(
        resolve_subject_windows_v2(
            &id("snapshot:one"),
            &id("obligation:one"),
            &[one, inconsistent],
        )
        .is_err()
    );
}
