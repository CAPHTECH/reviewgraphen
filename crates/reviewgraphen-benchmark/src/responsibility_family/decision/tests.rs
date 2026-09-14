use super::*;
use crate::responsibility_family::{ContractBinding, ExtractorBinding, MemberBinding};
use reviewgraphen_core::{ContentHash, StableId, canonical_json};
use std::collections::BTreeSet;

const PROPOSAL_SCHEMA_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/responsibility-family-decision-proposal-v1.schema.json"
));

const FSL_CANDIDATE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/fixtures/fsl-kernel-digest-candidate.json"
));
const FSL_ASSESSMENT_INPUT_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/fixtures/fsl-kernel-digest-assessment-input.json"
));
const FSL_ASSESSMENT_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/fixtures/fsl-kernel-digest-assessment.json"
));
const FSL_PROPOSAL_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/responsibility-family-v1/fixtures/fsl-kernel-digest-proposal.json"
));

fn id(value: &str) -> StableId {
    StableId::parse(value).unwrap()
}

fn hash(value: &str) -> ContentHash {
    ContentHash::sha256(value.as_bytes())
}

fn member(name: &str) -> MemberBinding {
    MemberBinding {
        member_id: id(&format!("responsibility-member:{name}")),
        path: format!("src/{name}.rs"),
        symbol: format!("validate_{name}"),
        anchor: hash(name),
        purpose_constraints: BTreeSet::from([format!("preserve-{name}-error")]),
        source_ids: vec![id(&format!("artifact:{name}"))],
    }
}

fn candidate() -> ResponsibilityFamilyCandidate {
    ResponsibilityFamilyCandidate {
        schema: CANDIDATE_SCHEMA.to_owned(),
        candidate_id: id("responsibility-family-candidate:path-policy"),
        proposed_family_id: id("responsibility-family:path-policy"),
        snapshot_id: id("snapshot:one"),
        proposed_common_contract: ContractBinding {
            id: "snapshot-relative-path@1".to_owned(),
            hash: hash("contract"),
        },
        extractor: ExtractorBinding {
            id: "syn-item-fn-tokens".to_owned(),
            version: "1".to_owned(),
        },
        members: vec![member("alpha"), member("beta")],
        unknowns: vec!["semantic equivalence is not established".to_owned()],
    }
}

#[test]
fn fsl_kernel_digest_fixture_replays_to_shared_conformance() {
    let candidate: ResponsibilityFamilyCandidate =
        serde_json::from_str(FSL_CANDIDATE_JSON).unwrap();
    let input: DecisionAssessmentInput = serde_json::from_str(FSL_ASSESSMENT_INPUT_JSON).unwrap();
    let assessment: DecisionAssessment = serde_json::from_str(FSL_ASSESSMENT_JSON).unwrap();
    let proposal: DecisionProposal = serde_json::from_str(FSL_PROPOSAL_JSON).unwrap();

    assert_eq!(build_assessment(&candidate, &input).unwrap(), assessment);
    assert_eq!(propose(&candidate, &assessment).unwrap(), proposal);
    assert_eq!(
        proposal.proposed_option,
        DecisionOption::SharedConformanceTest
    );
    validate_proposal(&candidate, &assessment, &proposal).unwrap();
}

fn assessed(
    candidate: &ResponsibilityFamilyCandidate,
    default: DecisionDisposition,
) -> DecisionAssessment {
    let universe = obligations(candidate).unwrap();
    let obligation_universe_hash = ContentHash::sha256(&canonical_json(&universe).unwrap());
    DecisionAssessment {
        schema: ASSESSMENT_SCHEMA.to_owned(),
        candidate_hash: universe.input.candidate_hash.clone(),
        obligation_universe_hash,
        results: universe
            .obligations
            .iter()
            .map(|obligation| DecisionResult {
                obligation_id: obligation.id.clone(),
                disposition: if obligation.property == DecisionProperty::SeparationRationale
                    && default == DecisionDisposition::Supports
                {
                    DecisionDisposition::Opposes
                } else {
                    default
                },
                source_ids: obligation.source_ids.clone(),
                evidence_ids: vec![id(&format!("evidence:{}", obligation.id))],
                verification_ids: vec![id(&format!("verification:{}", obligation.id))],
            })
            .collect(),
    }
}

fn set(
    candidate: &ResponsibilityFamilyCandidate,
    assessment: &mut DecisionAssessment,
    property: DecisionProperty,
    member_id: Option<&StableId>,
    disposition: DecisionDisposition,
) {
    let universe = obligations(candidate).unwrap();
    let obligation = universe
        .obligations
        .iter()
        .find(|item| item.property == property && item.member_id.as_ref() == member_id)
        .unwrap();
    assessment
        .results
        .iter_mut()
        .find(|item| item.obligation_id == obligation.id)
        .unwrap()
        .disposition = disposition;
}

#[test]
fn obligation_universe_is_finite_deterministic_and_non_authoritative() {
    let first = obligations(&candidate()).unwrap();
    let second = obligations(&candidate()).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.denominator.members, 2);
    assert_eq!(first.denominator.obligations, 15);
    assert_eq!(first.authority, DecisionAuthority::non_authority());
    assert_eq!(
        canonical_json(&first).unwrap(),
        canonical_json(&second).unwrap()
    );
}

#[test]
fn complete_supported_assessment_proposes_shared_validator() {
    let candidate = candidate();
    let assessment = assessed(&candidate, DecisionDisposition::Supports);
    let proposal = propose(&candidate, &assessment).unwrap();
    assert_eq!(proposal.proposed_option, DecisionOption::SharedValidator);
    assert!(proposal.unresolved_obligation_ids.is_empty());
    validate_proposal(&candidate, &assessment, &proposal).unwrap();
}

#[test]
fn validator_constraint_opposition_falls_back_to_shared_conformance() {
    let candidate = candidate();
    let mut assessment = assessed(&candidate, DecisionDisposition::Supports);
    set(
        &candidate,
        &mut assessment,
        DecisionProperty::TypedErrorPreservation,
        Some(&candidate.members[0].member_id),
        DecisionDisposition::Opposes,
    );
    let proposal = propose(&candidate, &assessment).unwrap();
    assert_eq!(
        proposal.proposed_option,
        DecisionOption::SharedConformanceTest
    );
}

#[test]
fn supported_separation_with_opposed_change_reason_proposes_separation() {
    let candidate = candidate();
    let mut assessment = assessed(&candidate, DecisionDisposition::Supports);
    set(
        &candidate,
        &mut assessment,
        DecisionProperty::CommonChangeReason,
        None,
        DecisionDisposition::Opposes,
    );
    set(
        &candidate,
        &mut assessment,
        DecisionProperty::SeparationRationale,
        None,
        DecisionDisposition::Supports,
    );
    let proposal = propose(&candidate, &assessment).unwrap();
    assert_eq!(
        proposal.proposed_option,
        DecisionOption::IntentionalSeparation
    );
}

#[test]
fn unsupported_or_incomplete_assessment_never_becomes_a_decision() {
    let candidate = candidate();
    let mut assessment = assessed(&candidate, DecisionDisposition::Inconclusive);
    for result in &mut assessment.results {
        result.evidence_ids.clear();
        result.verification_ids.clear();
    }
    assert_eq!(
        propose(&candidate, &assessment).unwrap().proposed_option,
        DecisionOption::Inconclusive
    );

    assessment.results.pop();
    assert!(propose(&candidate, &assessment).is_err());
}

#[test]
fn decision_proposal_satisfies_its_closed_json_schema() {
    let candidate = candidate();
    let assessment = assessed(&candidate, DecisionDisposition::Supports);
    let mut document = serde_json::to_value(propose(&candidate, &assessment).unwrap()).unwrap();
    let schema: serde_json::Value = serde_json::from_str(PROPOSAL_SCHEMA_JSON).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(&document));

    document["accepted_family"] = serde_json::json!(true);
    assert!(!validator.is_valid(&document));
}
