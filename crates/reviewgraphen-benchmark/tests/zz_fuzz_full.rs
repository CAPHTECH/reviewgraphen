use reviewgraphen_benchmark::real::*;
use reviewgraphen_benchmark::*;
use reviewgraphen_core::ContentHash;
use std::collections::{BTreeMap, BTreeSet};

fn hash(v: u8) -> ContentHash {
    let digit = (b'0' + (v % 10)) as char;
    ContentHash::parse(format!("sha256:{}", digit.to_string().repeat(64))).expect("hash")
}
fn git(v: u8) -> ContentHash {
    let digit = (b'0' + (v % 10)) as char;
    ContentHash::parse(format!("git:{}", digit.to_string().repeat(40))).expect("git")
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    fn pick(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const MP: u8 = 1; // manifest hash for positive trial
const MC: u8 = 2; // manifest hash for control trial

fn trial(id: &str, unit: &str, role: RevisionRole, rep: u32, manifest: u8) -> RealInventoryTrial {
    RealInventoryTrial {
        trial_id: id.into(),
        benchmark_unit_id: unit.into(),
        trial_unit_id: format!("opaque:{}", role_tag(&role)),
        revision_role: role,
        arm: Arm::FullReviewGraphen,
        replicate: rep,
        manifest_hash: hash(manifest),
        paired_configuration_hash: hash(3),
        real_unit_hash: hash(4),
        presence_evidence_hash: hash(5),
    }
}
fn role_tag(role: &RevisionRole) -> char {
    match role {
        RevisionRole::PositiveDefectPresent => 'p',
        RevisionRole::MatchedFixControl => 'c',
    }
}

fn trial_ids(unit: &str, rep: u32) -> (String, String) {
    (format!("t:{unit}:{rep}:p"), format!("t:{unit}:{rep}:c"))
}

#[derive(Clone)]
struct Gen {
    inventory: RealTrialInventory,
    collections: Vec<TrialCollection>,
    scores: Vec<RealScore>,
}

fn generate(rng: &mut Rng, units: &[&str], reps: &[u32]) -> Gen {
    let mut trials = Vec::new();
    for unit in units {
        for rep in reps {
            let (p, c) = trial_ids(unit, *rep);
            trials.push(trial(
                &p,
                unit,
                RevisionRole::PositiveDefectPresent,
                *rep,
                MP,
            ));
            trials.push(trial(&c, unit, RevisionRole::MatchedFixControl, *rep, MC));
        }
    }
    let inventory = RealTrialInventory {
        schema: REAL_INVENTORY_SCHEMA.into(),
        corpus_semantics: CorpusSemantics::RegressionFixPair,
        control_finding_semantics: ControlFindingSemantics::UnlabeledRequiresAdjudication,
        trials,
    };

    let mut collections = Vec::new();
    let mut scores = Vec::new();
    for t in &inventory.trials {
        let positive = matches!(t.revision_role, RevisionRole::PositiveDefectPresent);
        let role = if positive {
            RevisionRole::PositiveDefectPresent
        } else {
            RevisionRole::MatchedFixControl
        };
        let coll_choice = rng.pick(6);
        let mut candidate_hash = hash(rng.pick(3) as u8);
        let mut score_outcome = Outcome::Structured;
        if coll_choice < 4 {
            let outcome = match coll_choice {
                0 => CollectionOutcome::Structured,
                1 => CollectionOutcome::Abstained,
                2 => CollectionOutcome::ParseFailure,
                _ => CollectionOutcome::ProtocolInvalid,
            };
            let manifest_ok = rng.pick(2) == 0;
            let protocol_error = if matches!(outcome, CollectionOutcome::ProtocolInvalid) {
                Some("bad".to_owned())
            } else {
                None
            };
            collections.push(TrialCollection {
                schema: COLLECTION_SCHEMA.into(),
                trial_id: t.trial_id.clone(),
                manifest_hash: hash(if manifest_ok {
                    if positive { MP } else { MC }
                } else {
                    9
                }),
                candidate_hash: candidate_hash.clone(),
                outcome,
                protocol_error,
            });
            // score outcome follows collection unless perturbed
            score_outcome = match coll_choice {
                0 => Outcome::Structured,
                1 => Outcome::Abstained,
                2 => Outcome::ParseFailure,
                _ => Outcome::Structured,
            };
            if rng.pick(6) == 0 {
                score_outcome = match score_outcome {
                    Outcome::Structured => Outcome::Abstained,
                    _ => Outcome::Structured,
                };
            }
            if rng.pick(6) == 0 {
                candidate_hash = hash(7);
            }
        }

        if rng.pick(6) < 4 {
            let roots = if positive { 1 + rng.pick(2) as u32 } else { 0 };
            let structured = matches!(score_outcome, Outcome::Structured);
            let cands = if structured { rng.pick(3) as u32 } else { 0 };
            let matched = if cands == 0 {
                0
            } else {
                rng.pick(cands as u64 + 1) as u32
            };
            let detected = if positive {
                rng.pick(roots as u64 + 1) as u32
            } else {
                0
            };
            let unlabeled = if positive {
                cands.saturating_sub(matched)
            } else {
                cands
            };
            let manifest_wrong = rng.pick(7) == 0;
            scores.push(RealScore {
                schema: REAL_SCORE_SCHEMA.into(),
                trial_id: t.trial_id.clone(),
                benchmark_unit_id: t.benchmark_unit_id.clone(),
                trial_unit_id: t.trial_unit_id.clone(),
                target_id: "target".into(),
                arm: Arm::FullReviewGraphen,
                replicate: t.replicate,
                revision_role: role,
                target_expectation: if positive {
                    TargetExpectation::Present
                } else {
                    TargetExpectation::Absent
                },
                control_finding_semantics: ControlFindingSemantics::UnlabeledRequiresAdjudication,
                manifest_hash: hash(if manifest_wrong {
                    8
                } else if positive {
                    MP
                } else {
                    MC
                }),
                candidate_hash: candidate_hash.clone(),
                oracle_hash: hash(if positive { 1 } else { 2 }),
                real_unit_hash: hash(4),
                presence_evidence_hash: hash(5),
                input_tree_hash: git(if positive { 3 } else { 4 }),
                protocol_version: PROTOCOL_VERSION.into(),
                mechanism_ontology_version: MECHANISM_ONTOLOGY_VERSION.into(),
                paired_configuration_hash: t.paired_configuration_hash.clone(),
                target_root_count: roots,
                detected_target_root_count: detected,
                candidate_count: cands,
                target_anchor_matched_candidate_count: matched,
                unlabeled_candidate_count: unlabeled,
                outcome: score_outcome,
            });
        }
    }
    Gen {
        inventory,
        collections,
        scores,
    }
}

fn check(_case: &Gen, s: &RealFullRunSummary, pairs: usize) -> Option<String> {
    let classified = s.valid_collections
        + s.protocol_invalid_trials
        + s.collection_binding_invalid_trials
        + s.missing_trials;
    if classified != s.prepared_trials {
        return Some(format!(
            "trial partition {classified} != {}",
            s.prepared_trials
        ));
    }
    if (s.eligible_fix_pairs + s.excluded_fix_pairs) as usize != pairs {
        return Some(format!(
            "pair partition {} != {pairs}",
            s.eligible_fix_pairs + s.excluded_fix_pairs
        ));
    }
    if s.full_positive_detected_targets > s.total_positive_targets {
        return Some(format!(
            "detected {} > denominator {}",
            s.full_positive_detected_targets, s.total_positive_targets
        ));
    }
    if s.full_positive_unlabeled_findings > s.full_positive_findings {
        return Some("pos unlabeled > pos findings".into());
    }
    if s.full_control_findings != s.full_control_unlabeled_findings {
        return Some("control findings != unlabeled".into());
    }
    if s.full_control_target_anchor_allegations > s.full_control_findings {
        return Some("allegations > control findings".into());
    }
    if s.exclusion_reason_counts.values().any(|v| *v == 0) {
        return Some("zero reason count".into());
    }
    if s.excluded_fix_pairs == 0 && !s.exclusion_reason_counts.is_empty() {
        return Some("reasons without exclusions".into());
    }
    let _ = (BTreeMap::<u8, u8>::new(), BTreeSet::<u8>::new());
    None
}

#[test]
fn fuzz_full_summary() {
    let mut rng = Rng(0x5eed);
    let mut seen_reasons: BTreeMap<String, u32> = BTreeMap::new();
    for i in 0..20000 {
        let units: Vec<&str> = if rng.pick(2) == 0 {
            vec!["u1"]
        } else {
            vec!["u1", "u2"]
        };
        let reps: Vec<u32> = if rng.pick(2) == 0 {
            vec![1]
        } else {
            vec![1, 2]
        };
        let case = generate(&mut rng, &units, &reps);
        let pairs = units.len() * reps.len();
        let expected_scores: usize = case.inventory.trials.len();
        let _ = expected_scores;
        match summarize_real_full_run(&case.inventory, &case.collections, &case.scores) {
            Ok(s) => {
                for (k, v) in &s.exclusion_reason_counts {
                    *seen_reasons.entry(k.clone()).or_insert(0) += 1;
                    let _ = v;
                }
                if let Some(bad) = check(&case, &s, pairs) {
                    panic!(
                        "seed {i}: {bad}\ninventory {}\ncollections {}\nscores {}\nsummary {}",
                        serde_json::to_string(&case.inventory).unwrap(),
                        serde_json::to_string(&case.collections).unwrap(),
                        serde_json::to_string(&case.scores).unwrap(),
                        serde_json::to_string(&s).unwrap()
                    );
                }
                let _ = expected_scores;
            }
            Err(e) => panic!(
                "seed {i}: unexpected error {e}\ninventory {}\ncollections {}\nscores {}",
                serde_json::to_string(&case.inventory).unwrap(),
                serde_json::to_string(&case.collections).unwrap(),
                serde_json::to_string(&case.scores).unwrap()
            ),
        }
    }
    println!("reasons seen: {seen_reasons:?}");
}
