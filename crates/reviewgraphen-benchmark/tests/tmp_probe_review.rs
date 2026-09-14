use reviewgraphen_benchmark::*;
use reviewgraphen_core::ContentHash;
use std::collections::{BTreeMap, BTreeSet};

fn hash(s: &str) -> ContentHash {
    let mut v = String::new();
    for c in s.chars() {
        v.push(c);
    }
    ContentHash::parse(v).expect("hash")
}

fn git_hash() -> ContentHash {
    hash("git:1234567890123456789012345678901234567890")
}
fn sha(c: char) -> ContentHash {
    hash(&format!("sha256:{}", c.to_string().repeat(64)))
}

fn execution() -> ExecutionConfig {
    ExecutionConfig {
        provider: "p".into(),
        model: "m".into(),
        model_revision: "r".into(),
        reasoning_effort: "e".into(),
        prompt_template_version: "t".into(),
        tool_policy_version: "tp".into(),
        runner_version: "rv".into(),
        inference_settings: BTreeMap::new(),
        budget: DeclaredBudget {
            wall_clock_seconds: None,
            session_limit: None,
        },
    }
}

fn manifest(arm: Arm) -> TrialManifest {
    let source_inventory = vec![
        SourceInventoryEntry {
            path: "a.rs".into(),
            content_hash: sha('1'),
            line_count: 10,
        },
        SourceInventoryEntry {
            path: "b.rs".into(),
            content_hash: sha('2'),
            line_count: 3,
        },
    ];
    let exec = execution();
    let pch =
        paired_configuration_hash(&git_hash(), &source_inventory, &sha('a'), &exec).expect("pch");
    TrialManifest {
        schema: TRIAL_MANIFEST_SCHEMA.into(),
        trial_id: "t1".into(),
        unit_id: "u1".into(),
        arm: arm.clone(),
        replicate: 1,
        input_tree_hash: git_hash(),
        packet_hashes: BTreeSet::new(),
        expected_packet_ids: matches!(arm, Arm::G3Proxy | Arm::FullReviewGraphen)
            .then(|| BTreeSet::from(["p1".to_owned(), "p2".to_owned()]))
            .unwrap_or_default(),
        source_inventory,
        source_bundle_hash: sha('a'),
        protocol_version: PROTOCOL_VERSION.into(),
        mechanism_ontology_version: MECHANISM_ONTOLOGY_VERSION.into(),
        paired_configuration_hash: pch,
        limitations: BTreeSet::new(),
        execution: exec,
    }
}

fn loc(path: &str, s: u32, e: u32) -> Location {
    Location {
        path: path.into(),
        start_line: s,
        end_line: e,
    }
}

fn finding(id: &str, locations: Vec<Location>) -> CandidateFinding {
    CandidateFinding {
        local_id: id.into(),
        locations,
        mechanism_tags: BTreeSet::from([MechanismId::CheckWriteGap]),
        severity: None,
        rationale: None,
    }
}

fn ob(packet: &str, disp: &str, ids: &[&str]) -> ObligationResult {
    ObligationResult {
        packet_id: packet.into(),
        disposition: disp.into(),
        finding_local_ids: ids.iter().map(|s| s.to_string()).collect(),
    }
}

// independent reference of the intended protocol
fn reference_ok(m: &TrialManifest, c: &CandidateOutput) -> bool {
    if c.trial_id != m.trial_id {
        return false;
    }
    if matches!(c.outcome, Outcome::Structured) {
        let actual: BTreeSet<String> = c
            .obligation_results
            .iter()
            .map(|r| r.packet_id.clone())
            .collect();
        match m.arm {
            Arm::B1FreeForm => {
                if !actual.is_empty() {
                    return false;
                }
            }
            Arm::G3Proxy | Arm::FullReviewGraphen => {
                if actual != m.expected_packet_ids {
                    return false;
                }
                let linked: BTreeSet<&String> = c
                    .obligation_results
                    .iter()
                    .flat_map(|r| r.finding_local_ids.iter())
                    .collect();
                if c.findings.iter().any(|f| !linked.contains(&f.local_id)) {
                    return false;
                }
            }
        }
    }
    for f in &c.findings {
        for l in &f.locations {
            let Some(src) = m.source_inventory.iter().find(|s| s.path == l.path) else {
                return false;
            };
            if l.end_line > src.line_count {
                return false;
            }
        }
    }
    true
}

#[test]
fn differential_probe() {
    let outcomes = [
        Outcome::Structured,
        Outcome::Abstained,
        Outcome::ParseFailure,
    ];
    let finding_sets: Vec<Vec<CandidateFinding>> = vec![
        vec![],
        vec![finding("f1", vec![loc("a.rs", 6, 6)])],
        vec![
            finding("f1", vec![loc("a.rs", 1, 10)]),
            finding("f2", vec![loc("b.rs", 3, 3)]),
        ],
        vec![finding("f1", vec![loc("a.rs", 1, 11)])],
        vec![finding("f1", vec![loc("a.rs", 5, 20)])],
        vec![finding("f1", vec![loc("missing.rs", 1, 1)])],
        vec![finding("f1", vec![loc("a.rs", 0, 1)])],
        vec![finding("f1", vec![loc("a.rs", 6, 6), loc("b.rs", 1, 4)])],
    ];
    let ob_sets: Vec<Vec<ObligationResult>> = vec![
        vec![],
        vec![ob("p1", "issue_present", &["f1"])],
        vec![ob("p1", "issue_absent", &[])],
        vec![
            ob("p1", "issue_present", &["f1"]),
            ob("p2", "issue_absent", &[]),
        ],
        vec![
            ob("p1", "issue_present", &["f1"]),
            ob("p2", "issue_present", &["f2"]),
        ],
        vec![
            ob("pX", "issue_present", &["f1"]),
            ob("p2", "issue_absent", &[]),
        ],
        vec![
            ob("p1", "issue_present", &["f1"]),
            ob("p2", "issue_absent", &[]),
            ob("p2b", "abstained", &[]),
        ],
        vec![
            ob("p1", "not_applicable", &[]),
            ob("p2", "not_applicable", &[]),
        ],
        vec![
            ob("p1", "issue_present", &["f1"]),
            ob("p2", "issue_present", &["f1"]),
        ],
        vec![ob("p1", "issue_present", &["f9"])],
    ];

    let arms = [Arm::B1FreeForm, Arm::G3Proxy, Arm::FullReviewGraphen];
    let mut mismatches = Vec::new();
    let mut accepted = 0usize;
    let mut total = 0usize;
    for arm in &arms {
        let m = manifest(arm.clone());
        m.validate().expect("manifest");
        for outcome in &outcomes {
            for fs in &finding_sets {
                for os in &ob_sets {
                    let c = CandidateOutput {
                        schema: CANDIDATE_OUTPUT_SCHEMA.into(),
                        trial_id: "t1".into(),
                        outcome: outcome.clone(),
                        findings: fs.clone(),
                        obligation_results: os.clone(),
                    };
                    // skip inputs rejected by standalone validation
                    if c.validate().is_err() {
                        continue;
                    }
                    total += 1;
                    let got = validate_candidate_against_manifest(&m, &c).is_ok();
                    let want = reference_ok(&m, &c);
                    if got {
                        accepted += 1;
                    }
                    if got != want {
                        mismatches.push((
                            arm.clone(),
                            serde_json::to_string(&c).unwrap(),
                            format!("got_ok={got} want_ok={want}"),
                        ));
                    }
                }
            }
        }
    }
    println!(
        "total={total} accepted={accepted} mismatches={}",
        mismatches.len()
    );
    for (arm, cand, msg) in mismatches.iter().take(10) {
        println!("MISMATCH arm={arm:?} {msg}\ncand={cand}");
    }
    assert!(mismatches.is_empty(), "divergence from reference");
}
