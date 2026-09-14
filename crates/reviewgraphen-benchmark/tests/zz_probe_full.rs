use reviewgraphen_benchmark::real::*;
use reviewgraphen_benchmark::*;
use reviewgraphen_core::ContentHash;
use std::collections::{BTreeMap, BTreeSet};

fn hash(value: char) -> ContentHash {
    ContentHash::parse(format!("sha256:{}", value.to_string().repeat(64))).expect("hash")
}
fn git(value: char) -> ContentHash {
    ContentHash::parse(format!("git:{}", value.to_string().repeat(40))).expect("git")
}

fn trial(
    trial_id: &str,
    trial_unit_id: &str,
    role: RevisionRole,
    rep: u32,
    manifest: char,
    paired: char,
) -> RealInventoryTrial {
    RealInventoryTrial {
        trial_id: trial_id.into(),
        benchmark_unit_id: "real:u1".into(),
        trial_unit_id: trial_unit_id.into(),
        revision_role: role,
        arm: Arm::FullReviewGraphen,
        replicate: rep,
        manifest_hash: hash(manifest),
        paired_configuration_hash: hash(paired),
        real_unit_hash: hash('c'),
        presence_evidence_hash: hash('d'),
    }
}

fn collection(
    trial_id: &str,
    manifest: char,
    candidate: char,
    outcome: CollectionOutcome,
) -> TrialCollection {
    TrialCollection {
        schema: COLLECTION_SCHEMA.into(),
        trial_id: trial_id.into(),
        manifest_hash: hash(manifest),
        candidate_hash: hash(candidate),
        outcome,
        protocol_error: None,
    }
}

fn positive_score(trial_id: &str, manifest: char, candidate: char, outcome: Outcome) -> RealScore {
    let structured = matches!(outcome, Outcome::Structured);
    RealScore {
        schema: REAL_SCORE_SCHEMA.into(),
        trial_id: trial_id.into(),
        benchmark_unit_id: "real:u1".into(),
        trial_unit_id: "opaque:p".into(),
        target_id: "target:u1".into(),
        arm: Arm::FullReviewGraphen,
        replicate: 1,
        revision_role: RevisionRole::PositiveDefectPresent,
        target_expectation: TargetExpectation::Present,
        control_finding_semantics: ControlFindingSemantics::UnlabeledRequiresAdjudication,
        manifest_hash: hash(manifest),
        candidate_hash: hash(candidate),
        oracle_hash: hash('e'),
        real_unit_hash: hash('c'),
        presence_evidence_hash: hash('d'),
        input_tree_hash: git('3'),
        protocol_version: PROTOCOL_VERSION.into(),
        mechanism_ontology_version: MECHANISM_ONTOLOGY_VERSION.into(),
        paired_configuration_hash: hash('b'),
        target_root_count: 2,
        detected_target_root_count: if structured { 1 } else { 0 },
        candidate_count: if structured { 3 } else { 0 },
        target_anchor_matched_candidate_count: if structured { 1 } else { 0 },
        unlabeled_candidate_count: if structured { 2 } else { 0 },
        outcome,
    }
}

fn control_score(trial_id: &str, manifest: char, candidate: char) -> RealScore {
    RealScore {
        schema: REAL_SCORE_SCHEMA.into(),
        trial_id: trial_id.into(),
        benchmark_unit_id: "real:u1".into(),
        trial_unit_id: "opaque:c".into(),
        target_id: "target:u1".into(),
        arm: Arm::FullReviewGraphen,
        replicate: 1,
        revision_role: RevisionRole::MatchedFixControl,
        target_expectation: TargetExpectation::Absent,
        control_finding_semantics: ControlFindingSemantics::UnlabeledRequiresAdjudication,
        manifest_hash: hash(manifest),
        candidate_hash: hash(candidate),
        oracle_hash: hash('f'),
        real_unit_hash: hash('c'),
        presence_evidence_hash: hash('d'),
        input_tree_hash: git('4'),
        protocol_version: PROTOCOL_VERSION.into(),
        mechanism_ontology_version: MECHANISM_ONTOLOGY_VERSION.into(),
        paired_configuration_hash: hash('b'),
        target_root_count: 0,
        detected_target_root_count: 0,
        candidate_count: 2,
        target_anchor_matched_candidate_count: 1,
        unlabeled_candidate_count: 2,
        outcome: Outcome::Structured,
    }
}

fn inv(trials: Vec<RealInventoryTrial>) -> RealTrialInventory {
    RealTrialInventory {
        schema: REAL_INVENTORY_SCHEMA.into(),
        corpus_semantics: CorpusSemantics::RegressionFixPair,
        control_finding_semantics: ControlFindingSemantics::UnlabeledRequiresAdjudication,
        trials,
    }
}

fn show(label: &str, result: Result<RealFullRunSummary>) {
    match result {
        Ok(s) => println!("{label}: OK {}", serde_json::to_string(&s).expect("json")),
        Err(e) => println!("{label}: ERR {e}"),
    }
}

#[test]
fn probe() {
    let _ = (BTreeMap::<u8, u8>::new(), BTreeSet::<u8>::new());
    let pt = trial(
        "trial:p",
        "opaque:p",
        RevisionRole::PositiveDefectPresent,
        1,
        'a',
        'b',
    );
    let ct = trial(
        "trial:c",
        "opaque:c",
        RevisionRole::MatchedFixControl,
        1,
        'a',
        'b',
    );

    // 1 happy path
    show(
        "1-happy",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[
                collection("trial:p", 'a', 'e', CollectionOutcome::Structured),
                collection("trial:c", 'a', 'f', CollectionOutcome::Structured),
            ],
            &[
                positive_score("trial:p", 'a', 'e', Outcome::Structured),
                control_score("trial:c", 'a', 'f'),
            ],
        ),
    );

    // 2 positive collection missing
    show(
        "2-pos-collection-missing",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[collection(
                "trial:c",
                'a',
                'f',
                CollectionOutcome::Structured,
            )],
            &[
                positive_score("trial:p", 'a', 'e', Outcome::Structured),
                control_score("trial:c", 'a', 'f'),
            ],
        ),
    );

    // 3 positive collection outcome disagrees with score outcome
    show(
        "3-outcome-disagree",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[
                collection("trial:p", 'a', 'e', CollectionOutcome::Abstained),
                collection("trial:c", 'a', 'f', CollectionOutcome::Structured),
            ],
            &[
                positive_score("trial:p", 'a', 'e', Outcome::Structured),
                control_score("trial:c", 'a', 'f'),
            ],
        ),
    );

    // 4 positive score missing
    show(
        "4-pos-score-missing",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[
                collection("trial:p", 'a', 'e', CollectionOutcome::Structured),
                collection("trial:c", 'a', 'f', CollectionOutcome::Structured),
            ],
            &[control_score("trial:c", 'a', 'f')],
        ),
    );

    // 5 positive collection protocol invalid
    let mut pinvalid = collection("trial:p", 'a', 'e', CollectionOutcome::ProtocolInvalid);
    pinvalid.protocol_error = Some("bad".into());
    show(
        "5-pos-protocol-invalid",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[
                pinvalid,
                collection("trial:c", 'a', 'f', CollectionOutcome::Structured),
            ],
            &[
                positive_score("trial:p", 'a', 'e', Outcome::Structured),
                control_score("trial:c", 'a', 'f'),
            ],
        ),
    );

    // 6 positive collection manifest mismatch
    show(
        "6-manifest-mismatch",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[
                collection("trial:p", '7', 'e', CollectionOutcome::Structured),
                collection("trial:c", 'a', 'f', CollectionOutcome::Structured),
            ],
            &[
                positive_score("trial:p", 'a', 'e', Outcome::Structured),
                control_score("trial:c", 'a', 'f'),
            ],
        ),
    );

    // 7 both collections missing, no scores
    show(
        "7-all-missing",
        summarize_real_full_run(&inv(vec![pt.clone(), ct.clone()]), &[], &[]),
    );

    // 8 abstained positive score, valid collection: denominator retained?
    show(
        "8-abstain-positive",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[
                collection("trial:p", 'a', 'e', CollectionOutcome::Abstained),
                collection("trial:c", 'a', 'f', CollectionOutcome::Structured),
            ],
            &[
                positive_score("trial:p", 'a', 'e', Outcome::Abstained),
                control_score("trial:c", 'a', 'f'),
            ],
        ),
    );

    // 9 two pairs sharing benchmark unit, different replicates
    let pt2 = trial(
        "trial:p2",
        "opaque:p",
        RevisionRole::PositiveDefectPresent,
        2,
        'a',
        'b',
    );
    let ct2 = trial(
        "trial:c2",
        "opaque:c",
        RevisionRole::MatchedFixControl,
        2,
        'a',
        'b',
    );
    show(
        "9-two-replicates-one-broken",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone(), pt2.clone(), ct2.clone()]),
            &[
                collection("trial:p", 'a', 'e', CollectionOutcome::Structured),
                collection("trial:c", 'a', 'f', CollectionOutcome::Structured),
                collection("trial:c2", 'a', 'f', CollectionOutcome::Structured),
            ],
            &[
                positive_score("trial:p", 'a', 'e', Outcome::Structured),
                control_score("trial:c", 'a', 'f'),
                control_score("trial:c2", 'a', 'f'),
            ],
        ),
    );

    // 10 score whose trial_unit_id belongs to control but trial_id is positive
    let mut swapped = positive_score("trial:p", 'a', 'e', Outcome::Structured);
    swapped.trial_unit_id = "opaque:c".into();
    show(
        "10-swapped-unit-binding",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[
                collection("trial:p", 'a', 'e', CollectionOutcome::Structured),
                collection("trial:c", 'a', 'f', CollectionOutcome::Structured),
            ],
            &[swapped, control_score("trial:c", 'a', 'f')],
        ),
    );

    // 11 duplicate collection
    show(
        "11-duplicate-collection",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[
                collection("trial:p", 'a', 'e', CollectionOutcome::Structured),
                collection("trial:p", 'a', 'e', CollectionOutcome::Structured),
                collection("trial:c", 'a', 'f', CollectionOutcome::Structured),
            ],
            &[
                positive_score("trial:p", 'a', 'e', Outcome::Structured),
                control_score("trial:c", 'a', 'f'),
            ],
        ),
    );

    // 12 score candidate hash matches collection but score arm not full handled upstream
    let mut weird = positive_score("trial:p", 'a', 'e', Outcome::Structured);
    weird.detected_target_root_count = 2;
    weird.unlabeled_candidate_count = 2;
    show(
        "12-detected-with-findings",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[
                collection("trial:p", 'a', 'e', CollectionOutcome::Structured),
                collection("trial:c", 'a', 'f', CollectionOutcome::Structured),
            ],
            &[weird, control_score("trial:c", 'a', 'f')],
        ),
    );

    // 13 abstained positive score claiming detected roots
    let mut phantom = positive_score("trial:p", 'a', 'e', Outcome::Abstained);
    phantom.detected_target_root_count = 2;
    show(
        "13-abstain-phantom-detected",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[
                collection("trial:p", 'a', 'e', CollectionOutcome::Abstained),
                collection("trial:c", 'a', 'f', CollectionOutcome::Structured),
            ],
            &[phantom, control_score("trial:c", 'a', 'f')],
        ),
    );

    // 14 positive and control scores both bound to the same candidate hash
    show(
        "14-same-candidate",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[
                collection("trial:p", 'a', 'e', CollectionOutcome::Structured),
                collection("trial:c", 'a', 'e', CollectionOutcome::Structured),
            ],
            &[
                positive_score("trial:p", 'a', 'e', Outcome::Structured),
                control_score("trial:c", 'a', 'e'),
            ],
        ),
    );

    // 15 control score has nonzero target_root_count -> validate error?
    let mut bad = control_score("trial:c", 'a', 'f');
    bad.target_root_count = 0;
    bad.detected_target_root_count = 0;
    bad.unlabeled_candidate_count = 2;
    show(
        "15-control-ok",
        summarize_real_full_run(
            &inv(vec![pt.clone(), ct.clone()]),
            &[
                collection("trial:p", 'a', 'e', CollectionOutcome::Structured),
                collection("trial:c", 'a', 'f', CollectionOutcome::Structured),
            ],
            &[
                positive_score("trial:p", 'a', 'e', Outcome::Structured),
                bad,
            ],
        ),
    );

    // 16 inventory with a B1 trial -> rejected
    let mut b1 = trial(
        "trial:b",
        "opaque:p",
        RevisionRole::PositiveDefectPresent,
        1,
        'a',
        'b',
    );
    b1.arm = Arm::B1FreeForm;
    show(
        "16-nonfull-inventory",
        summarize_real_full_run(&inv(vec![b1, ct.clone()]), &[], &[]),
    );
}
